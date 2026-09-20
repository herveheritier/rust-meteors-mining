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
#: paramétrée, les autres sont des lignes de base. `nn` (réseau de neurones
#: entraîné par imitation, `imitate.py`) et `ppo` (réseau entraîné par
#: renforcement, `ppo.py`) se pilotent aussi en simulateur EVA (les champs
#: manquants de l'observation valent zéro) - leur vraie évaluation reste la
#: partie réelle / le mode hybride. `autopilot_sim` est le portage Python de
#: l'autopilote du jeu (`autopilot_ref.py`) : la **référence hors-ligne**
#: (l'original exige un processus de jeu), utilisée pour mesurer l'écart des
#: politiques apprises sans lancer le jeu.
SIM_STRATEGIES = ("idle", "random", "seek", "nn", "ppo", "autopilot_sim")

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


def ppo_policy(path: str) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique **PPO** entraînée par renforcement sur l'observation complète
    (`ppo.py`, sortie `ppo_policy.json`) : charge les poids et renvoie la
    fonction obs → commande (l'action de plus grande probabilité parmi les
    combinaisons poussée/rotation de `EVA_ACTIONS`). Fonctionne pour la tâche
    EVA."""
    from nn import obs_features
    from ppo import load_ppo

    net, actions = load_ppo(path)

    def choose(obs: dict[str, Any]) -> dict[str, bool]:
        cmd = empty_cmd()
        _, probs, _ = net.forward(obs_features(obs))
        cmd.update(actions[net.greedy(probs)])
        return cmd

    return choose


def nn_policy(path: str) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique **réseau de neurones** entraînée hors-ligne par imitation de
    l'autopilote (`imitate.py`, sortie `nn_policy.json`) : charge les poids
    et renvoie la fonction obs → commande (chaque bouton activé si la sortie
    sigmoïde dépasse 0,5). Fonctionne pour la tâche EVA comme pour la boucle
    de minage du vaisseau - le réseau a appris les deux sur les trajectoires."""
    from nn import SIGMOID_OUTPUTS, load_nn, obs_features

    net = load_nn(path)

    def choose(obs: dict[str, Any]) -> dict[str, bool]:
        cmd = empty_cmd()
        out = net.forward(obs_features(obs))
        # sigmoïdes indépendantes (up/down/fire) puis rotation softmax
        # mutuellement exclusive (left/right/none) - l'expert n'appuie jamais
        # gauche et droite ensemble, et le réseau non plus
        for a, v in zip(("up", "down", "fire"), out[:SIGMOID_OUTPUTS]):
            cmd[a] = v >= 0.5
        turn = net.turn_action(out)
        cmd["left"] = turn == "left"
        cmd["right"] = turn == "right"
        return cmd

    return choose


def autopilot_sim_policy() -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique de l'**autopilote du jeu porté en Python**
    (`autopilot_ref.py`) - la référence hors-ligne (l'autopilote original vit
    dans le jeu et exige un processus headless). Elle porte un état interne
    (freinage tangentiel) et expose `.reset()` à appeler entre les épisodes."""
    from autopilot_ref import autopilot_ref_policy

    return autopilot_ref_policy()


def load_policy_file(path: str) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Charge une politique depuis un fichier JSON en **détectant sa nature**
    (champ `policy`) : paramètres de `seek` (sortie de `cem.py`), réseau `nn`
    (`imitate.py`/`dagger.py`) ou réseau `ppo` (`ppo.py`). C'est ce que
    consomment `evaluate.py --policy` et le test de non-régression."""
    import json

    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    kind = data.get("policy", "seek")
    if kind == "ga-law":
        return ga_law_policy(path)
    if kind == "nn":
        return nn_policy(path)
    if kind == "ppo":
        return ppo_policy(path)
    params = data.get("params")
    return lambda obs: seek(obs, params)  # noqa: E731 - paramètres entraînés ou défauts


def ga_law_policy(path: str) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique **génome A** de l'algorithme génétique (`ga.py`, sortie
    `ga_policy.json`) : la loi portée de l'autopilote vaisseau recâblée sur
    les gènes optimisés. Rejouable dans le simulateur comme contre la vraie
    partie (le champ `params` porte les constantes)."""
    import json

    from ga import make_law_policy

    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("policy") != "ga-law":
        raise ValueError(f"{path} n'est pas une politique ga-law (champ `policy`)")
    return make_law_policy(dict(data["params"]))


def policy_for(strategy: str, rng: Optional[random.Random] = None) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Renvoie la fonction de politique d'une stratégie simulable (sans
    fichier) - `nn`/`ppo` nécessitent `--policy` (voir `policy_file`)."""
    if strategy == "idle":
        return idle
    if strategy == "random":
        return random_policy(rng)
    if strategy == "seek":
        return seek
    if strategy == "autopilot_sim":
        return autopilot_sim_policy()
    raise ValueError(f"stratégie inconnue : {strategy} (attendues : {', '.join(SIM_STRATEGIES)})")
