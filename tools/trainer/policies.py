#!/usr/bin/env python3
"""Politiques de pilotage du cosmonaute EVA pour l'auto-entraînement.

Chaque politique est une fonction `obs → commande` (les mêmes clés que le
`POST /cmd` de l'interface : up/down/left/right/fire). Le système entraîné
est **indépendant du jeu** : il ne connaît que l'observation JSON.

- `idle`    : aucune commande (le cosmonaute reste immobile) - ligne de base.
- `random`  : commande tirée au hasard à chaque frame - ligne de base.
- `seek`    : contrôleur **paramétré** qui rentre à la station - s'oriente
  vers le centre (←/→), pousse (↑) quand il est aligné et **borne sa
  vitesse** : croisière modérée, ralentissement à l'approche et
  **contre-poussée** (nez à l'opposé de la station) s'il arrive trop vite -
  le cosmonaute n'a pas de frein. Ses 5 paramètres (`turn_db`,
  `thrust_db`, `cruise`, `slow_zone`, `band`) sont la cible de
  l'entraînement par CEM (`cem.py`). `turn_db` est le réglage critique :
  trop lâche, l'approche dérive et le cosmonaute se met en orbite autour de
  la base au lieu d'entrer dans le cercle d'accostage.

Le déplacement suit les conventions du jeu (voir `eva_env.py`) : `right`
augmente l'orientation, `left` la diminue, `up` pousse le long du nez.
"""

from __future__ import annotations

import math
import random
from typing import Any, Callable, Optional

from eva_env import (
    STATION_DOCK_DISTANCE,
    eva_aim,
    eva_speed_along,
    wrap_angle,
)

# ── stratégies de base ──────────────────────────────────────────────────────

#: Types de stratégies simulables (sans le jeu) - `seek` porte la politique
#: entraînable, les autres sont des lignes de base.
SIM_STRATEGIES = ("idle", "random", "seek")

#: Stratégie « pilote automatique du jeu » : seulement en direct (le jeu
#: pilote lui-même via `POST /cmd {"autopilot": true}`).
LIVE_AUTOPILOT = "autopilot"

# ── actions ─────────────────────────────────────────────────────────────────
#: Actions discrètes du cosmonaute EVA (poussée et/ou rotation).
EVA_ACTIONS = (
    {},
    {"up": True},
    {"right": True},
    {"left": True},
    {"up": True, "right": True},
    {"up": True, "left": True},
)


def empty_cmd() -> dict[str, bool]:
    return {"up": False, "down": False, "left": False, "right": False, "fire": False}


def idle(obs: dict[str, Any]) -> dict[str, bool]:
    return empty_cmd()


def random_policy(rng: Optional[random.Random] = None) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Ligne de base aléatoire : une action discrète uniforme par frame.
    Seedée pour rester reproductible (`random.Random`)."""
    rng = rng if rng is not None else random.Random()

    def choose(obs: dict[str, Any]) -> dict[str, bool]:
        cmd = empty_cmd()
        cmd.update(rng.choice(EVA_ACTIONS))
        return cmd

    return choose


# ── contrôleur « rentrer à la station » paramétré (la politique entraînée) ──

#: Paramètres par défaut = réglage robuste trouvé sur le simulateur à 60 Hz
#: (même cadence que l'interface réelle) : bande d'alignement serrée (la clé
#: d'une approche qui vise juste), croisière modérée, ralentissement étendu.
#: L'entraînement CEM part de là (`--init expert`) ou d'un départ naïf qui
#: échoue (`--init naive`) pour mesurer le progrès.
SEEK_DEFAULTS: dict[str, float] = {
    "turn_db": 0.05,   # tolérance d'alignement avant de tourner (rad)
    "thrust_db": 0.08,  # tolérance d'alignement avant de pousser (rad)
    "cruise": 55.0,    # vitesse de croisière visée (unités/s)
    "slow_zone": 250.0,  # rayon du ralentissement à l'approche (unités)
    "band": 12.0,      # demi-bande de vitesse (unités/s) - hystérésis
}

#: Bornes de recherche de l'entraînement CEM (min, max) par paramètre.
SEEK_BOUNDS: dict[str, tuple[float, float]] = {
    "turn_db": (0.02, 0.5),
    "thrust_db": (0.02, 0.5),
    "cruise": (20.0, 150.0),
    "slow_zone": (0.0, 600.0),
    "band": (1.0, 120.0),
}


def seek(obs: dict[str, Any], p: Optional[dict[str, float]] = None) -> dict[str, bool]:
    """Contrôleur homing paramétré - **externe au jeu** : il ne reçoit que
    l'observation JSON. La poussée est vectorielle (↑, pas de frein) : on
    oriente le nez vers la station (ou à l'opposé pour contre-pousser), on
    pousse quand on est aligné, et on ne pousse jamais dans le cercle
    d'accostage (la dérive déclenche la récupération). Les paramètres sont
    la cible de l'entraînement (`cem.py`) : la **bande d'alignement**
    (`turn_db`) conditionne la précision de la ligne d'approche (trop lâche,
    le cosmonaute dérive et se met en orbite autour de la base au lieu
    d'entrer dans le petit cercle d'accostage - c'est l'échec du départ
    naïf), `cruise`/`slow_zone`/`band` règlent la vitesse d'approche et le
    ralentissement."""
    cmd = empty_cmd()
    if obs.get("pilot") != "eva" or not obs.get("eva_active"):
        return cmd  # plus le cosmonaute qui pilote : plus rien à faire
    params = dict(SEEK_DEFAULTS)
    if p:
        params.update(p)
    turn_db = params["turn_db"]
    thrust_db = params["thrust_db"]
    cruise = params["cruise"]
    slow_zone = params["slow_zone"]
    band = params["band"]

    aim = eva_aim(obs)
    d = obs["station_dist"]
    o = obs["eva"]["orientation"]
    v_along = eva_speed_along(obs, aim)

    # vitesse visée : croisière au loin, puis ralentissement franc dans
    # l'anneau (dérive déclenchée sous ~15 u - cercle d'accostage)
    if d < slow_zone:
        desired = min(cruise, d * 1.8 + 4.8)
    else:
        desired = cruise
    # approche trop rapide : contre-poussée (nez à l'opposé de la station)
    braking = v_along > desired + band
    thrust_dir = aim + math.pi if braking else aim
    err = wrap_angle(thrust_dir - o)
    if err > turn_db:
        cmd["right"] = True
    elif err < -turn_db:
        cmd["left"] = True
    # poussée uniquement aligné, hors du cercle d'accostage
    if abs(err) <= turn_db and d > STATION_DOCK_DISTANCE:
        if braking or v_along < desired - band:
            cmd["up"] = True
    return cmd


def policy_for(strategy: str, rng: Optional[random.Random] = None) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Renvoie la fonction de politique d'une stratégie simulable."""
    if strategy == "idle":
        return idle
    if strategy == "random":
        return random_policy(rng)
    if strategy == "seek":
        return seek
    raise ValueError(f"stratégie inconnue : {strategy} (attendues : {', '.join(SIM_STRATEGIES)})")
