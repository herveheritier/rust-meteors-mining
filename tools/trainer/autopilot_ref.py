#!/usr/bin/env python3
"""Portage Python de la loi EVA de l'**autopilote du jeu**
(`src/autopilot.rs::autopilot_eva_inputs`), pour disposer d'une **référence
hors-ligne** : le jeu n'est pas nécessaire pour mesurer l'écart entre une
politique apprise et l'autopilote (test de non-régression, mesure « vs
autopilote » sans processus headless).

La loi est portée **telle quelle** (mêmes constantes, mêmes formules) : c'est
elle qui a été mesurée à ~943,8 sur la tâche EVA (graines 1..6, départ 300 u)
dans `docs/AUTOENTRAINEMENT.md` §5 ter. Comme les constantes du jeu sont en
**unités/frame** et que l'observation rapporte des **unités/s**, le portage
convertit les vitesses à l'entrée (÷ 60) et travaille ensuite en unités/frame,
exactement comme le Rust.

Ce que fait l'autopilote EVA (résumé de `src/autopilot.rs`) :

- il vise le centre de la station (`aim`) et borne sa vitesse d'approche :
  croisière `EVA_MAX_SPEED` au loin, visée qui décroît **linéairement** dans
  l'anneau `EVA_SLOW_ZONE` jusqu'à `EVA_ARRIVAL_SPEED` au cercle d'accostage ;
- il **contre-pousse** (nez à l'opposé de la station) quand son approche est
  trop rapide pour s'arrêter à temps (demi-tour `EVA_TURN_FRAMES` puis
  décélération) ;
- il **casse les orbites** par un freinage **tangentiel** (nez à l'opposé de
  la vitesse) déclenché au-dessus de `EVA_TANG_BRAKE_HI` avec hystérésis
  (relâché sous `EVA_TANG_BRAKE_LO`) - état porté par le contrôleur ;
- il coupe les gaz pendant les réorientations (poussée seulement nez aligné
  dans la bande `EVA_TURN_DEADBAND_*`) et **dans** le cercle d'accostage (la
  dérive suffit à déclencher la récupération).

L'autopilote ne tire jamais et n'évite rien : le cosmonaute est un
non-collider. Sur la tâche vaisseau (`ship`), c'est `autopilot_inputs` qu'il
faudrait porter - hors périmètre ici (le test ne porte que sur l'EVA).
"""

from __future__ import annotations

import math
from typing import Any, Callable

from eva_env import (
    FRAMES_PER_SECOND,
    PLAYER_ACCELERATION,
    PLAYER_ROTATION_SPEED,
    STATION_DOCK_DISTANCE,
    eva_aim,
    wrap_angle,
)

# ── constantes de la loi EVA du jeu (src/autopilot.rs, unités/frame) ────────
EVA_MAX_SPEED = 1.5           # croisière loin de la station (90 u/s)
EVA_ARRIVAL_SPEED = 0.5       # vitesse d'arrivée au cercle d'accostage (30 u/s)
EVA_SLOW_ZONE = 240.0         # rayon du ralentissement à l'approche (unités)
EVA_SPEED_BAND = 0.15         # demi-bande de vitesse (9 u/s) - hystérésis
EVA_TURN_FRAMES = math.pi / PLAYER_ROTATION_SPEED  # demi-tour ≈ 105 frames
EVA_TURN_DEADBAND_MIN = 0.015
EVA_TURN_DEADBAND_SCALE = 0.75
EVA_TANG_BRAKE_HI = 0.25      # déclenche le freinage tangentiel (15 u/s)
EVA_TANG_BRAKE_LO = 0.12      # relâche le freinage tangentiel (7,2 u/s)

#: Période de la boucle de contrôle du jeu (le mode headless avance au même
#: pas fixe de 1/60 s).
DT = 1.0 / FRAMES_PER_SECOND


def _empty_cmd() -> dict[str, bool]:
    return {"up": False, "down": False, "left": False, "right": False, "fire": False}


class AutopilotEva:
    """Le pilotage EVA de l'autopilote du jeu, **avec son état interne** (le
    freinage tangentiel à hystérésis `eva_tang_braking` vaut pour tout
    l'épisode : deux cinématiques identiques peuvent appeler des actions
    opposées selon lui). Une instance par épisode, `reset()` au départ."""

    def __init__(self, dt: float = DT) -> None:
        self.dt = dt
        self.tang_braking = False

    def reset(self) -> None:
        """Remet l'hystérésis du freinage tangentiel à zéro (départ
        d'épisode)."""
        self.tang_braking = False

    def choose(self, obs: dict[str, Any]) -> dict[str, bool]:
        """Commande de l'autopilote pour une observation (format `/obs`)."""
        cmd = _empty_cmd()
        if obs.get("pilot") != "eva" or not obs.get("eva_active"):
            return cmd  # plus le cosmonaute qui pilote : plus rien à faire
        eva = obs.get("eva", {})
        d = obs.get("station_dist", 0.0)
        aim = eva_aim(obs)
        # vitesses de l'observation (unités/s) → unités/frame (formules du jeu)
        vx = eva.get("vx", 0.0) / FRAMES_PER_SECOND
        vy = eva.get("vy", 0.0) / FRAMES_PER_SECOND
        v_along = vx * math.cos(aim) + vy * math.sin(aim)
        v_tang = math.sqrt(max(0.0, eva.get("speed", 0.0) ** 2 / FRAMES_PER_SECOND ** 2
                               - v_along * v_along))
        # vitesse visée : croisière rapide au loin, décroissance linéaire dans
        # l'anneau jusqu'à la vitesse d'arrivée au cercle d'accostage
        if d < EVA_SLOW_ZONE:
            desired = min(
                EVA_MAX_SPEED,
                EVA_ARRIVAL_SPEED
                + (EVA_MAX_SPEED - EVA_ARRIVAL_SPEED)
                * ((d - STATION_DOCK_DISTANCE) / (EVA_SLOW_ZONE - STATION_DOCK_DISTANCE)),
            )
        else:
            desired = EVA_MAX_SPEED
        # freinage anticipé : approche trop rapide pour s'arrêter à temps
        braking = v_along > EVA_ARRIVAL_SPEED + EVA_SPEED_BAND and (
            (d - STATION_DOCK_DISTANCE)
            < v_along * EVA_TURN_FRAMES
            + (v_along * v_along - EVA_ARRIVAL_SPEED * EVA_ARRIVAL_SPEED)
            / (2.0 * PLAYER_ACCELERATION)
        )
        # orbite (vitesse surtout tangentielle) : freinage tangentiel à
        # hystérésis - état interne de l'autopilote
        if self.tang_braking:
            if v_tang < EVA_TANG_BRAKE_LO:
                self.tang_braking = False
        elif v_tang > EVA_TANG_BRAKE_HI:
            self.tang_braking = True
        if self.tang_braking:
            braking = True
        # direction de poussée : la station, son opposé (freinage), ou
        # l'opposé de la vitesse (cassure d'orbite)
        if self.tang_braking:
            thrust_dir = math.pi - eva.get("direction", 0.0)
        elif braking:
            thrust_dir = aim + math.pi
        else:
            thrust_dir = aim
        # bande d'alignement : au moins un demi-pas de rotation par frame
        turn_db = max(
            EVA_TURN_DEADBAND_MIN,
            PLAYER_ROTATION_SPEED * FRAMES_PER_SECOND * self.dt * EVA_TURN_DEADBAND_SCALE,
        )
        err = wrap_angle(thrust_dir - eva.get("orientation", 0.0))
        if err > turn_db:
            cmd["right"] = True
        elif err < -turn_db:
            cmd["left"] = True
        # poussée seulement aligné et hors du cercle d'accostage
        if abs(err) <= turn_db and d > STATION_DOCK_DISTANCE:
            if braking:
                cmd["up"] = True
            elif v_along < desired - EVA_SPEED_BAND:
                cmd["up"] = True
        return cmd


def autopilot_ref_policy() -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique `obs → commande` de l'autopilote de référence (état interne
    conservé). L'appelant appelle `policy.reset()` (attribut posé sur la
    fonction) au début de chaque épisode."""
    pilot = AutopilotEva()

    def choose(obs: dict[str, Any]) -> dict[str, bool]:
        return pilot.choose(obs)

    choose.reset = pilot.reset  # type: ignore[attr-defined]
    return choose
