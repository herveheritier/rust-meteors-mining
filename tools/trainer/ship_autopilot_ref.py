#!/usr/bin/env python3
"""Portage Python de la loi **vaisseau** de l'autopilote du jeu
(`src/autopilot.rs::autopilot_inputs`), symétrique de `autopilot_ref.py` (qui
porte la loi EVA) : la **référence hors-ligne** de la boucle de minage du
vaisseau - mesurer l'écart d'une politique apprise à l'autopilote du jeu, et
l'utiliser comme **expert étiqueteur** de l'amorce par imitation (cible
`ship`, `warmstart.py`), sans processus headless.

La loi est portée **telle quelle** (mêmes constantes, mêmes formules, même
ordre de décision) depuis l'observation publiée par le jeu (`/obs`) :

- **mission de la frame** (priorité) : soute pleine → rentrer accoster ;
  hostile menaçant la station (rayon `STATION_GUARD_RADIUS`) → le détruire ;
  réserves basses **et** payables (économie) → rentrer se ravitailler ;
  hostile à portée de tir ou minerai « gardé » → le détruire d'abord ; sinon
  minerai le plus proche → le collecter ; sinon hostile → l'attaquer ; enfin
  stationner près de la station ;
- **tir** indépendant : tout hostile à portée de tir (`FIRE_RANGE`) et le nez
  aligné (`AIM_TOLERANCE`), sauf **retenue de feu** (cible quasi détruite
  achevée par des balles en vol, ou minerais dans le corridor de tir) ;
- **conduite** : esquive latérale si un hostile va passer trop près
  (`AVOID_*`), sinon cap sur la cible de la mission avec la vitesse visée
  selon la distance (`desired_speed`) - orientation du nez + poussée/frein,
  ou poussée dans les 4 directions de l'écran en mode 4 WAYS.

Ce que l'observation doit exposer pour que ce portage soit **fidèle** (ajouté
à `src/driver.rs`) : le **mode de déplacement** (`moving_mode`), le **centre
du corps** de chaque objet (`center_x/center_y` - la visée réelle, un météore
asymétrique a son corps décalé de `position`), la liste des **balles en vol**
(`bullets` - la retenue de feu ne se lit pas dans la cinématique) et
`supplies_affordable` (les prix du magasin ne sont pas dans l'observation).

Deux écarts assumés, documentés ici :

- `nearby` est plafonné aux `MAX_NEARBY_OBJECTS` (32) objets les plus proches ;
  la loi du jeu parcourt **toutes** les formes. Les cibles utiles (portée de
  tir 210 u, garde 240 u, dégagement 130 u) sont dans ce rayon, l'écart est
  donc marginal ;
- l'autopilote gère aussi l'**accostage** (déchargement, ravitaillement) via
  `autopilot_handle_dock` / `autopilot_handle_shop` : ce sont des actions de
  boîte de dialogue, hors de la loi de pilotage portée ici.
"""

from __future__ import annotations

import math
from typing import Any, Callable, Optional

from eva_env import FRAMES_PER_SECOND, WORLD_H, WORLD_W, wrap_angle

# ── réglages du comportement (src/autopilot.rs, unités/frame ou unités) ────
CRUISE_SPEED = 1.8            # vitesse de croisière visée (unités/frame)
ATTACK_STANDOFF = 70.0        # distance de sécurité devant un hostile
STATION_GUARD_RADIUS = 240.0  # hostile « menaçant la station »
FIRE_RANGE = 210.0            # portée de tir
AIM_TOLERANCE = 0.14          # tolérance d'alignement de tir (~8°)
THRUST_DEADBAND = 0.16        # tolérance d'alignement de poussée (~9°)
TURN_DEADBAND = 0.10          # tolérance de rotation (~6°)
PATROL_RADIUS = 120.0         # rayon de stationnement
DOCK_SLOW_ZONE = 90.0         # rayon de ralentissement à l'accostage
LOW_SUPPLY_RATIO = 0.30       # seuil de ravitaillement (réserves)
MINERAL_CLEARANCE = 130.0     # minerai « gardé » par un hostile
HOLD_FIRE_LIFE = 2            # cible quasi détruite (retenue de feu)
HOLD_FIRE_RADIUS = 150.0      # balle en vol « comptant » pour l'achever
HOLD_FIRE_MINERAL_RADIUS = 60.0  # minerai dans le corridor de tir
AVOID_RADIUS = 360.0          # détection d'évitement
AVOID_CLEARANCE = 90.0        # séparation minimale au passage au plus près
AVOID_TIME = 1.5              # fenêtre de temps du passage au plus près (s)
DODGE_BRAKE_SPEED = 0.5       # freiner pendant l'esquive au-dessus (unités/frame)

#: Modes de déplacement du jeu (`src/config.rs`) : seule la branche 4 WAYS
#: change la conduite, les autres orientent le nez (SEUL REALISTIC est
#: distingué pour le contre-commande de rotation).
MOVING_MODE_INERTIAL = 0
MOVING_MODE_4_WAYS = 1
MOVING_MODE_DIRECTIONAL = 2
MOVING_MODE_REALISTIC = 3

#: Types d'objet de `nearby` considérés hostiles (`is_hostile` du jeu).
HOSTILE_KINDS = ("meteore", "alien")


def _empty_cmd() -> dict[str, bool]:
    return {"up": False, "down": False, "left": False, "right": False, "fire": False}


def _wrap_delta(dx: float, dy: float) -> tuple[float, float]:
    """Ramène un delta écran dans le monde torique (mêmes axes que le jeu)."""
    dx -= WORLD_W * round(dx / WORLD_W)
    dy -= WORLD_H * round(dy / WORLD_H)
    return dx, dy


def _is_hostile(o: dict[str, Any]) -> bool:
    return o.get("kind") in HOSTILE_KINDS and o.get("life", 0) > 0


def _is_mineral(o: dict[str, Any]) -> bool:
    return o.get("kind") == "minerai" and o.get("life", 0) > 0


def _obj_delta(a: dict[str, Any], b: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **centre de corps** de `a` vers `b` (coordonnées écran).

    Chaque objet de `nearby`/`bullets` porte son delta depuis la **position**
    du pilote (`dx`/`dy`) et son décalage de corps (`center_x`/`center_y`) :
    le delta de corps à corps est `(d_b + c_b) − (d_a + c_a)`.
    """
    dx = (b.get("dx", 0.0) + b.get("center_x", 0.0)) - (a.get("dx", 0.0) + a.get("center_x", 0.0))
    dy = (b.get("dy", 0.0) + b.get("center_y", 0.0)) - (a.get("dy", 0.0) + a.get("center_y", 0.0))
    return _wrap_delta(dx, dy)


def _ship_obj_delta(obs: dict[str, Any], b: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **centre du vaisseau** vers le **centre de corps** de `b`."""
    ship = obs.get("ship", {})
    dx = (b.get("dx", 0.0) + b.get("center_x", 0.0)) - ship.get("center_x", 0.0)
    dy = (b.get("dy", 0.0) + b.get("center_y", 0.0)) - ship.get("center_y", 0.0)
    return _wrap_delta(dx, dy)


def _ship_station_delta(obs: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **centre du vaisseau** vers la **station** (`station.position`,
    centre nul) - la cible d'accostage / de stationnement."""
    ship = obs.get("ship", {})
    dx = obs.get("station_dx", 0.0) - ship.get("center_x", 0.0)
    dy = obs.get("station_dy", 0.0) - ship.get("center_y", 0.0)
    return _wrap_delta(dx, dy)


def _station_obj_delta(obs: dict[str, Any], b: dict[str, Any]) -> tuple[float, float]:
    """Delta torique de la **station** (`station.position`) vers le centre de
    corps de `b` - la distance qui décide de la garde de la station."""
    dx = obs.get("station_dx", 0.0) - (b.get("dx", 0.0) + b.get("center_x", 0.0))
    dy = obs.get("station_dy", 0.0) - (b.get("dy", 0.0) + b.get("center_y", 0.0))
    return _wrap_delta(dx, dy)


def _dist(delta: tuple[float, float]) -> float:
    return math.hypot(delta[0], delta[1])


def supplies_low(obs: dict[str, Any]) -> bool:
    """Carburant ou munitions sous le seuil de ravitaillement (économie) -
    `supplies_low` de `src/autopilot.rs`, calculé des seules quantités
    exposées (`fuel`/`fuel_cap`, `ammo`/`ammo_cap`)."""
    fuel_cap = obs.get("fuel_cap", 0.0)
    if fuel_cap > 0.0 and obs.get("fuel", 0.0) < fuel_cap * LOW_SUPPLY_RATIO:
        return True
    ammo_cap = obs.get("ammo_cap", 0)
    return ammo_cap > 0 and obs.get("ammo", 0) < ammo_cap * LOW_SUPPLY_RATIO


def desired_speed(goal: str, d: float) -> float:
    """Vitesse visée selon la mission et la distance à la cible (unités/frame) -
    `desired_speed` du jeu."""
    if goal == "dock":
        if d < DOCK_SLOW_ZONE:
            return min(d * 0.04 + 0.02, 0.9)
        return CRUISE_SPEED
    if goal == "attack":
        return min(max(0.0, d - ATTACK_STANDOFF) * 0.05, CRUISE_SPEED)
    if goal == "collect":
        return min(max(d * 0.12, 0.3), 1.4)
    # patrol : approcher puis s'arrêter hors de la zone d'accostage
    if d < PATROL_RADIUS:
        return 0.0
    return min(max(d * 0.08, 0.3), CRUISE_SPEED)


def collision_threat(
    obs: dict[str, Any], hostiles: list[dict[str, Any]], attack_target: Optional[dict[str, Any]]
) -> Optional[dict[str, Any]]:
    """Menace de collision imminente : premier hostile dont le **passage au
    plus près** survient dans moins de `AVOID_TIME` s à moins de
    `AVOID_CLEARANCE` unités. La cible d'attaque courante est ignorée tant
    qu'elle est à distance de tir (on s'en approche, on ne l'évite pas)."""
    ship = obs.get("ship", {})
    pvx = ship.get("vx", 0.0)
    pvy = ship.get("vy", 0.0)
    threat: Optional[dict[str, Any]] = None
    threat_t = math.inf
    for s in hostiles:
        r = _ship_obj_delta(obs, s)
        rlen = _dist(r)
        # cible d'attaque en approche (hors standoff) : pas une menace
        if s is attack_target and rlen >= ATTACK_STANDOFF:
            continue
        if rlen > AVOID_RADIUS:
            continue
        vrx = s.get("vx", 0.0) - pvx
        vry = s.get("vy", 0.0) - pvy
        v2 = vrx * vrx + vry * vry
        t = -(r[0] * vrx + r[1] * vry) / v2 if v2 > 1e-9 else 0.0
        if t < 0.0:
            continue  # déjà en train de s'éloigner
        cx = r[0] + vrx * t
        cy = r[1] + vry * t
        if math.hypot(cx, cy) < AVOID_CLEARANCE and t < AVOID_TIME and t < threat_t:
            threat = s
            threat_t = t
    return threat


def avoid_aim(obs: dict[str, Any], s: dict[str, Any]) -> tuple[float, float]:
    """Direction d'esquive face à l'hostile `s` : perpendiculaire à la
    trajectoire d'approche relative, côté qui écarte déjà le vaisseau.
    Renvoie `(angle écran, vitesse visée)`."""
    ship = obs.get("ship", {})
    pvx = ship.get("vx", 0.0)
    pvy = ship.get("vy", 0.0)
    r = _ship_obj_delta(obs, s)
    rlen = _dist(r)
    vrx = s.get("vx", 0.0) - pvx
    vry = s.get("vy", 0.0) - pvy
    v2 = vrx * vrx + vry * vry
    if v2 > 1e-9:
        vlen = math.sqrt(v2)
        wx, wy = vrx / vlen, vry / vlen
    elif rlen > 1e-9:
        wx, wy = r[0] / rlen, r[1] / rlen
    else:
        wx, wy = 1.0, 0.0
    p1 = (wy, -wx)
    p2 = (-wy, wx)
    e = p1 if pvx * p1[0] + pvy * p1[1] >= pvx * p2[0] + pvy * p2[1] else p2
    return math.atan2(e[1], e[0]), CRUISE_SPEED


def autopilot_ship_inputs(obs: dict[str, Any]) -> dict[str, bool]:
    """Entrées de pilotage de l'autopilote du jeu pour une observation
    (`/obs`) - le portage de `autopilot_inputs`. Renvoie la même structure que
    `expert` (mêmes primitives que les touches)."""
    out = _empty_cmd()
    if obs.get("pilot") != "vaisseau":
        return out  # plus le vaisseau qui pilote (cosmonaute EVA) : rien à faire
    ship = obs.get("ship", {})
    nearby = [o for o in obs.get("nearby", []) if o.get("life", 0) > 0]
    bullets = [b for b in obs.get("bullets", []) if b.get("life", 0) > 0]
    moving_mode = obs.get("moving_mode", MOVING_MODE_DIRECTIONAL)

    # ── choix de la mission de la frame ─────────────────────────────────────
    capacity = obs.get("cargo_cap", 0)
    cargo_full = capacity > 0 and obs.get("cargo_qty", 0) >= capacity
    low_supplies = (
        bool(obs.get("economy"))
        and supplies_low(obs)
        and bool(obs.get("supplies_affordable"))
    )
    guard: Optional[tuple[float, dict[str, Any]]] = None
    mineral: Optional[tuple[float, dict[str, Any]]] = None
    hostile: Optional[tuple[float, dict[str, Any]]] = None
    for o in nearby:
        if _is_hostile(o):
            # hostile menaçant la station (mission prioritaire)
            ds = _dist(_station_obj_delta(obs, o))
            if ds < STATION_GUARD_RADIUS and (guard is None or ds < guard[0]):
                guard = (ds, o)
            # hostile le plus proche (pour miner, et cible de tir)
            dp = _dist(_ship_obj_delta(obs, o))
            if hostile is None or dp < hostile[0]:
                hostile = (dp, o)
        elif _is_mineral(o):
            dp = _dist(_ship_obj_delta(obs, o))
            if mineral is None or dp < mineral[0]:
                mineral = (dp, o)
    hostile_in_range = hostile is not None and hostile[0] < FIRE_RANGE
    mineral_guarded = mineral is not None and hostile is not None and (
        _dist(_obj_delta(hostile[1], mineral[1])) < MINERAL_CLEARANCE
    )
    attack_target: Optional[dict[str, Any]] = None
    if cargo_full:
        goal, target_obj = "dock", None
    elif guard is not None:
        attack_target = guard[1]
        goal, target_obj = "attack", guard[1]
    elif low_supplies:
        goal, target_obj = "dock", None
    elif hostile_in_range or mineral_guarded:
        attack_target = hostile[1]  # type: ignore[index]
        goal, target_obj = "attack", hostile[1]  # type: ignore[index]
    elif mineral is not None:
        goal, target_obj = "collect", mineral[1]
    elif hostile is not None:
        attack_target = hostile[1]
        goal, target_obj = "attack", hostile[1]
    else:
        goal, target_obj = "patrol", None

    # ── tir : tout hostile à portée, si le nez est aligné ───────────────────
    fire_target: Optional[dict[str, Any]] = None
    best_fire = FIRE_RANGE
    for o in nearby:
        if not _is_hostile(o):
            continue
        d = _dist(_ship_obj_delta(obs, o))
        if d < best_fire:
            best_fire = d
            fire_target = o
    if fire_target is not None:
        # minerais dans le corridor de tir (le fragment vivant va les absorber)
        minerals_near = any(
            _is_mineral(m)
            and _dist(_obj_delta(fire_target, m)) < HOLD_FIRE_MINERAL_RADIUS
            for m in nearby
        )
        # cible quasi détruite achevée par des balles en vol : on retient le feu
        holding = minerals_near or (
            fire_target.get("life", 0) <= HOLD_FIRE_LIFE
            and any(
                _dist(_obj_delta(fire_target, b)) < HOLD_FIRE_RADIUS for b in bullets
            )
        )
        if not holding:
            fdx, fdy = _ship_obj_delta(obs, fire_target)
            target_aim = math.atan2(fdy, fdx)
            if abs(wrap_angle(target_aim - ship.get("orientation", 0.0))) < AIM_TOLERANCE:
                out["fire"] = True

    # ── conduite : évitement de collision ou cible de la mission ────────────
    hostiles = [o for o in nearby if _is_hostile(o)]
    threat = collision_threat(obs, hostiles, attack_target)
    if threat is not None:
        aim, desired = avoid_aim(obs, threat)
    else:
        if target_obj is None:  # dock / patrol : la station
            delta = _ship_station_delta(obs)
        else:
            delta = _ship_obj_delta(obs, target_obj)
        d = _dist(delta)
        aim = math.atan2(delta[1], delta[0])
        desired = desired_speed(goal, d)
    # la menace est-elle devant (dans le sens d'avancement) ?
    threat_ahead = False
    if threat is not None:
        r = _ship_obj_delta(obs, threat)
        rlen = _dist(r)
        if rlen >= 1e-9:
            vx = math.cos(ship.get("direction", 0.0))
            vy = -math.sin(ship.get("direction", 0.0))
            threat_ahead = (vx * r[0] + vy * r[1]) / rlen > 0.0

    # vitesse du vaisseau **par frame** (l'observation rapporte des unités/s)
    velocity = ship.get("speed", 0.0) / FRAMES_PER_SECOND
    if moving_mode == MOVING_MODE_4_WAYS:
        # poussée dans les 4 directions de l'écran vers le vecteur visé
        vx = math.cos(ship.get("direction", 0.0)) * velocity
        vy = -math.sin(ship.get("direction", 0.0)) * velocity
        ex = math.cos(aim) * desired - vx
        ey = math.sin(aim) * desired - vy
        if abs(ex) > abs(ey):
            out["right" if ex > 0.0 else "left"] = True
        elif ey > 0.0:
            out["down"] = True
        else:
            out["up"] = True
        return out

    # DIRECTIONAL / INERTIAL / REALISTIC : orienter le nez puis pousser
    err = wrap_angle(aim - ship.get("orientation", 0.0))
    if err > TURN_DEADBAND:
        out["right"] = True
    elif err < -TURN_DEADBAND:
        out["left"] = True
    vx = math.cos(ship.get("direction", 0.0)) * velocity
    vy = -math.sin(ship.get("direction", 0.0)) * velocity
    v_along = vx * math.cos(aim) + vy * math.sin(aim)
    overspeed = v_along > desired + 0.15
    settle = desired < 0.4 and v_along > 0.02
    if abs(err) < THRUST_DEADBAND:
        if v_along < desired - 0.1:
            out["up"] = True
        elif overspeed or settle:
            out["down"] = True
    elif threat is not None and threat_ahead and velocity > DODGE_BRAKE_SPEED:
        # esquive avec la menace devant : freiner pendant le virage
        out["down"] = True
    elif velocity > desired + 0.15 or (desired < 0.4 and velocity > 0.02):
        out["down"] = True
    # REALISTIC : contre-commande pour arrêter la rotation résiduelle
    if (
        moving_mode == MOVING_MODE_REALISTIC
        and abs(ship.get("rotation", 0.0)) > 0.06
        and abs(err) < TURN_DEADBAND
    ):
        out["left" if ship.get("rotation", 0.0) > 0.0 else "right"] = True
    return out


def ship_autopilot_ref_policy() -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Politique `obs → commande` de l'autopilote vaisseau de référence. La loi
    vaisseau n'a **pas d'état interne** (contrairement au freinage tangentiel
    de l'EVA) : la même observation donne toujours la même commande."""
    return autopilot_ship_inputs
