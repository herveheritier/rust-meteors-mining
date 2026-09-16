#!/usr/bin/env python3
"""Petit réseau de neurones en **Python standard uniquement** (aucune
dépendance - la convention de `tools/trainer/`) pour l'apprentissage par
**imitation de l'autopilote** (`imitate.py`, `dagger.py`).

- `obs_features(obs)` : transforme une observation JSON (`/obs`, ou celle des
  trajectoires du banc d'essai) en un **vecteur de features à taille fixe** -
  le réseau n'apprend que sur ce vecteur, jamais sur le JSON brut ;
- `MLP` : perceptron multicouche à une couche cachée (tanh) et deux têtes de
  sortie - **sigmoïdes indépendantes** pour `up`/`down`/`fire` (le pilote peut
  pousser et tirer en même temps) et **softmax mutuellement exclusif** pour la
  rotation (`left`/`right`/`none`) : l'autopilote n'appuie jamais gauche et
  droite ensemble, et deux sigmoïdes indépendantes laissaient la politique
  entraînée coincée à les enfoncer toutes les deux (aucune rotation, orbite) -
  le softmax force le choix d'un seul sens de rotation ;
- `action_target(action)` : cibles binaires (sigmoïdes) + un-seul (softmax)
  d'une action de l'autopilote (`expert` de l'observation, ou `action` des
  trajectoires du banc d'essai) ;
- `save_nn` / `load_nn` : poids sérialisés en JSON (comme `policy.json`),
  rejouables par `evaluate.py --strategy nn --policy nn_policy.json`.
"""

from __future__ import annotations

import json
import math
import random
from typing import Any, Optional

# ── extraction de features ──────────────────────────────────────────────────

#: Version du format de features : si elle change (nouveaux champs, autre
#: normalisation), les politiques entraînées avec l'ancienne version ne sont
#: plus rejouables - le fichier de politique porte cette version.
#:
#: v6 : ajout des **variables de décision EVA** (`_eva_decision_features`) -
#: sans elles, l'imitation de l'autopilote EVA est structurellement
#: impossible (voir plus bas).
#:
#: v7 : ajout des **grandeurs de décision du vaisseau** qui manquaient - le
#: **mode de déplacement** (`moving_mode`, la conduite 4 WAYS n'obéit pas aux
#: mêmes commandes), `supplies_affordable` (mission de ravitaillement) et les
#: **balles en vol** (`bullets`, la retenue de feu) avec les deux grandeurs de
#: retenue de feu (`_ship_decision_features`). Sans elles, deux états de
#: features identiques portent des commandes opposées : c'est le même conflit
#: de représentation que celui corrigé côté EVA en v6.
#:
#: v8 : ajout de la **visée de mission** de la conduite vaisseau
#: (`_ship_drive_features`) - la mission de la frame rejouée depuis les mêmes
#: variables que `src/autopilot.rs` (soute pleine, garde de la station,
#: ravitaillement, hostile à portée, minerai gardé), le **cap effectif**
#: (esquive ou cible), son erreur d'alignement, la **vitesse visée**, la
#: vitesse projetée, les composantes du mode **4 WAYS** et les drapeaux de la
#: conduite (esquive, menace devant, survitesse, arrêt). En v7, aucune erreur
#: d'alignement exposée ne portait la visée réelle de l'expert (mesuré : sens
#: de rotation expliqué à 50,3 % par l'erreur vers la station, soit le
#: hasard) : le signe de rotation était structurellement indécidable - la
#: visée de mission est cette information manquante, exactement comme
#: `eva_thrust_err` côté EVA en v6.
#:
#: **v9 (mesurée puis écartée).** Une version 9 ajoutait les **seuils de
#: conduite** (l'erreur en unités du seuil, les comparaisons de rotation /
#: poussée / survitesse / esquive / arrêt, la branche 4 WAYS, et le seuil
#: d'alignement de tir) : elle rend la loi **entièrement décidable** - exactitude
#: d'imitation **100 %** contre 78,9 % en v8 - mais la boucle fermée **régresse**
#: dans le jeu (**3/12** livraisons contre 7/12). Le facteur limitant n'était
#: donc pas la **représentation** mais l'**écart de distribution** entre le
#: micro-simulateur et la partie (voir `docs/AUTOENTRAINEMENT.md` §5 nonies ter) :
#: le format reste en **v8**, et l'effort se porte sur des données **du jeu réel**.
FEATURES_VERSION = 8

#: Actions (boutons) prédits par le réseau - l'ordre définit les indices de
#: sortie sigmoïde (`up`, `down`, `fire`) ; la rotation (left/right) est une
#: tête softmax à part (voir `TURN_HEAD`).
ACTIONS = ("up", "down", "left", "right", "fire")

#: Types d'objets proches encodés en one-hot (même libellé que l'observation).
NEARBY_KINDS = ("meteore", "minerai", "alien", "portail", "mine")

#: Nombre d'objets proches pris en compte (les plus proches) : le reste de la
#: liste est ignoré - couvre l'horizon utile de l'autopilote (tir, minage).
NEARBY_SLOTS = 6

#: Modes de déplacement du vaisseau (`src/config.rs`, `MOVING_MODE_*`) :
#: encodés en **one-hot**. La conduite de l'autopilote en dépend (4 WAYS pousse
#: dans les axes de l'écran, les autres orientent le nez) - sans cette
#: information, deux observations de cinématique identique portent des
#: commandes opposées.
MOVING_MODES = 4

#: Nombre de **balles en vol** encodées en slots (les plus proches) : la
#: retenue de feu de l'autopilote vaisseau (cible déjà achevée par les balles)
#: ne se lit pas dans la cinématique du vaisseau.
BULLET_SLOTS = 4

#: Longueur d'un slot de balle : `dx, dy, dist, vx, vy`. Pas de one-hot (une
#: balle est toujours de type `balle`) et pas de vie (sans valeur de décision).
BULLET_SLOT_LEN = 5

#: Indices des sorties : 0..2 sigmoïdes (up, down, fire), 3..5 softmax de
#: rotation (left, right, none).
SIGMOID_OUTPUTS = 3
TURN_HEAD = 3  # left / right / none
NONE_CLASS = 2

#: Nombre total de sorties du réseau (3 sigmoïdes + 3 classes de rotation).
OUTPUT_COUNT = SIGMOID_OUTPUTS + TURN_HEAD

#: Échelles de normalisation (le monde fait 3960×3540, vitesses ~centaines
#: d'unités/s, vie de forme ~quelques dizaines).
SCALE_DIST = 2000.0
SCALE_SPEED = 300.0
SCALE_RADIUS = 100.0
SCALE_LIFE = 50.0
SCALE_CREDITS = 5000.0


def _clamp(x: float, lo: float = -1.0, hi: float = 1.0) -> float:
    return min(max(x, lo), hi)


def _wrap_angle(a: float) -> float:
    """Ramène un angle dans ]−π, π] (comme le jeu)."""
    return (a + math.pi) % math.tau - math.pi


def _angle_feature(a: float) -> float:
    """Angle normalisé dans ]−1, 1] (invariant par tour complet)."""
    return _clamp((_wrap_angle(a) / math.pi))


def _pilot_features(body: dict[str, Any], station_dx: float, station_dy: float) -> list[float]:
    """Features d'un pilote (vaisseau ou EVA) : cinématique brute **plus** les
    grandeurs que l'autopilote calcule pour décider (viser la station) :

    - `aim`    : angle écran vers la station (l'orientation doit s'y aligner) ;
    - `err`    : erreur d'alignement `aim − orientation` (signe → tourner
      gauche/droite, |err| → seuil de poussée) ;
    - `v_along`: vitesse projetée sur la direction de la station (positive =
      on s'en rapproche - décide pousser ou contre-pousser) ;
    - `v_tang` : vitesse perpendiculaire (l'orbite autour de la base - la
      contre-poussée qui la tue).

    Fournir ces grandeurs dérivées rend l'imitation robuste en boucle fermée :
    le réseau n'a pas à ré-apprendre l'`atan2` et les projections - il apprend
    la **décision** (le seuil), pas le calcul.
    """
    aim = math.atan2(station_dy, station_dx)
    orientation = body.get("orientation", 0.0)
    vx = body.get("vx", 0.0)
    vy = body.get("vy", 0.0)
    speed = body.get("speed", 0.0)
    v_along = vx * math.cos(aim) + vy * math.sin(aim)
    v_tang = math.sqrt(max(0.0, speed * speed - v_along * v_along))
    return [
        body.get("x", 0.0) / SCALE_DIST,
        body.get("y", 0.0) / SCALE_DIST,
        _clamp(vx / SCALE_SPEED),
        _clamp(vy / SCALE_SPEED),
        _clamp(speed / SCALE_SPEED),
        _angle_feature(body.get("direction", 0.0)),
        _angle_feature(orientation),
        _clamp(body.get("rotation", 0.0) / math.tau),
        # grandeurs dérivées (celles que l'autopilote utilise)
        _angle_feature(aim),
        _clamp(_wrap_angle(aim - orientation) / math.pi),
        _clamp(v_along / SCALE_SPEED),
        _clamp(v_tang / SCALE_SPEED),
    ]


#: Constantes de la loi EVA de l'autopilote du jeu (`src/autopilot.rs`),
#: en unités/frame - utilisées pour exposer à l'apprenant les grandeurs que
#: l'expert calcule pour décider (voir `_eva_decision_features`).
_EVA_ARRIVAL_SPEED = 0.5
_EVA_SPEED_BAND = 0.15
_EVA_TURN_FRAMES = math.pi / (math.tau / 210.0)  # demi-tour (frames)
_EVA_ACCEL = 0.05
_EVA_TANG_BRAKE_HI = 0.25
_DOCK_DISTANCE = 15.0


def _eva_decision_features(obs: dict[str, Any], station_dx: float,
                           station_dy: float) -> list[float]:
    """Variables de décision de l'autopilote EVA, **celles qui rendent
    l'imitation possible** :

    - `eva_braking` : approche trop rapide pour s'arrêter avant le cercle
      d'accostage (l'expert **contre-pousse**, nez à l'opposé de la station) ;
    - `eva_tang_brake` : composante tangentielle forte (l'expert casse
      l'orbite, nez à l'opposé de la **vitesse**) ;
    - `eva_thrust_err` : erreur d'alignement par rapport à la direction de
      poussée **résultante** (`aim`, `aim+π` en freinage, `π−direction` en
      cassure d'orbite), normalisée dans [−1, 1].

    Sans `eva_thrust_err`, deux états de cinématique quasi identique portent
    des actions **opposées** selon que l'expert freine ou non : la feature
    `err` de `_pilot_features` (visée de la station) vaut alors ≈ 0 dans les
    deux cas, et le réseau ne peut pas trancher - c'est le conflit mesuré
    (amorce qui pousse nez désaligné → accélération tangentielle → fuite).
    Le freinage EVA est spécifié par le jeu ; ces features-là le portent.
    """
    eva = obs.get("eva", {})
    d = obs.get("station_dist", 0.0)
    aim = math.atan2(station_dy, station_dx)
    v_along = (eva.get("vx", 0.0) * math.cos(aim)
               + eva.get("vy", 0.0) * math.sin(aim)) / 60.0
    speed = eva.get("speed", 0.0) / 60.0
    v_tang = math.sqrt(max(0.0, speed * speed - v_along * v_along))
    tang = v_tang > _EVA_TANG_BRAKE_HI
    braking = v_along > _EVA_ARRIVAL_SPEED + _EVA_SPEED_BAND and (
        (d - _DOCK_DISTANCE)
        < v_along * _EVA_TURN_FRAMES
        + (v_along * v_along - _EVA_ARRIVAL_SPEED * _EVA_ARRIVAL_SPEED) / (2.0 * _EVA_ACCEL)
    )
    if tang:
        thrust_dir = math.pi - eva.get("direction", 0.0)
    elif braking:
        thrust_dir = aim + math.pi
    else:
        thrust_dir = aim
    thrust_err = _wrap_angle(thrust_dir - eva.get("orientation", 0.0))
    return [1.0 if braking else 0.0, 1.0 if tang else 0.0, _angle_feature(thrust_err)]


#: Constantes de la **retenue de feu** de l'autopilote vaisseau
#: (`src/autopilot.rs`) - rejouées par `_ship_decision_features`.
_SHIP_FIRE_RANGE = 210.0
_SHIP_HOLD_FIRE_LIFE = 2
_SHIP_HOLD_FIRE_RADIUS = 150.0
_SHIP_HOLD_FIRE_MINERAL_RADIUS = 60.0
_SHIP_HOSTILE_KINDS = ("meteore", "alien")


def _finite(value: Any) -> float:
    """Valeur numérique **finie** : un champ absent, `null` ou non fini vaut
    zéro.

    Le jeu peut publier `null` pour un flottant non fini (serde_json sérialise
    ainsi NaN/∞ - cas d'un centre de forme dégénéré). Sans ce filtre, un seul
    `None` ferait lever l'arithmétique et un seul NaN **contaminerait tout le
    vecteur de features** (les NaN se propagent dans les sommes) : le portage
    Rust doit reproduire exactement la même neutralisation.
    """
    try:
        x = float(value)
    except (TypeError, ValueError):
        return 0.0
    return x if math.isfinite(x) else 0.0


def _body_distance(a: dict[str, Any], b: dict[str, Any]) -> float:
    """Distance entre les **centres de corps** de deux objets de
    `nearby`/`bullets` : chaque objet porte son delta depuis le pilote
    (`dx`/`dy`) et son décalage de corps (`center_x`/`center_y`) - la visée
    réelle de l'autopilote (un météore asymétrique a son corps décalé)."""
    dx = (_finite(b.get("dx")) + _finite(b.get("center_x"))) - (
        _finite(a.get("dx")) + _finite(a.get("center_x")))
    dy = (_finite(b.get("dy")) + _finite(b.get("center_y"))) - (
        _finite(a.get("dy")) + _finite(a.get("center_y")))
    return math.hypot(dx, dy)


def _ship_decision_features(obs: dict[str, Any]) -> list[float]:
    """Grandeurs de décision de la **retenue de feu** de l'autopilote
    vaisseau - les deux cas où l'expert **ne tire pas** alors que tout le reste
    de l'état pousse à tirer :

    - `hold_fire_bullets` : la cible de tir (hostile vivant à portée) est
      quasi détruite et une **balle en vol** l'achève déjà (tirer encore
      traverserait le point de mort et détruirait les minerais libérés) ;
    - `hold_fire_mineral` : des minerais vivent dans le corridor de tir de
      cette cible (le fragment les absorberait).

    Sans ces deux indicateurs, la cible ``life <= 2`` et les balles en vol
    existent dans les features mais leur **proximité mutuelle** ne se lit pas :
    le même vecteur porte `fire` et pas `fire` selon la position relative des
    balles à la cible - le conflit de représentation mesuré (mêmes
    caractéristiques que `eva_braking` en v6).
    """
    nearby = obs.get("nearby", [])
    bullets = obs.get("bullets", [])
    # cible de tir de l'expert : l'hostile vivant le plus proche, à portée
    target: Optional[dict[str, Any]] = None
    best = _SHIP_FIRE_RANGE
    for o in nearby:
        if o.get("kind") not in _SHIP_HOSTILE_KINDS or o.get("life", 0) <= 0:
            continue
        d = o.get("dist", 0.0)
        if d < best:
            best = d
            target = o
    if target is None:
        return [0.0, 0.0]
    hold_bullets = 0.0
    if target.get("life", 0) <= _SHIP_HOLD_FIRE_LIFE:
        for b in bullets:
            if _body_distance(b, target) < _SHIP_HOLD_FIRE_RADIUS:
                hold_bullets = 1.0
                break
    hold_mineral = 0.0
    for m in nearby:
        if m.get("kind") != "minerai" or m.get("life", 0) <= 0:
            continue
        if _body_distance(m, target) < _SHIP_HOLD_FIRE_MINERAL_RADIUS:
            hold_mineral = 1.0
            break
    return [hold_bullets, hold_mineral]


#: Constantes de la **conduite** de l'autopilote vaisseau (`src/autopilot.rs`,
#: unités/frame ou unités) - rejouées par `_ship_drive_features`.
_SHIP_CRUISE_SPEED = 1.8
_SHIP_ATTACK_STANDOFF = 70.0
_SHIP_STATION_GUARD_RADIUS = 240.0
_SHIP_MINERAL_CLEARANCE = 130.0
_SHIP_PATROL_RADIUS = 120.0
_SHIP_DOCK_SLOW_ZONE = 90.0
_SHIP_LOW_SUPPLY_RATIO = 0.30
_SHIP_AVOID_RADIUS = 360.0
_SHIP_AVOID_CLEARANCE = 90.0
_SHIP_AVOID_TIME = 1.5
_SHIP_SETTLE_BAND = 0.4
_SHIP_WORLD_W = 3960.0
_SHIP_WORLD_H = 3540.0

#: Nombre de features de `_ship_drive_features` (4 de mission + 10 de conduite).
SHIP_DRIVE_FEATURES = 14


def _wrap_tor(dx: float, dy: float) -> tuple[float, float]:
    """Ramène un delta écran dans le monde **torique** (le plus court), comme
    `wrapped_delta` du jeu - les deltas de l'observation le sont déjà, mais
    leur **différence** (station ↔ objet, objet ↔ objet) peut sortir du
    demi-monde et doit l'être aussi."""
    dx -= _SHIP_WORLD_W * round(dx / _SHIP_WORLD_W)
    dy -= _SHIP_WORLD_H * round(dy / _SHIP_WORLD_H)
    return dx, dy


def _ship_body_delta(obs: dict[str, Any], b: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **centre du vaisseau** → **centre de corps** de `b` (la
    visée réelle de l'autopilote : un météore asymétrique a son corps décalé)."""
    ship = obs.get("ship", {})
    dx = (_finite(b.get("dx")) + _finite(b.get("center_x"))) - _finite(ship.get("center_x"))
    dy = (_finite(b.get("dy")) + _finite(b.get("center_y"))) - _finite(ship.get("center_y"))
    return _wrap_tor(dx, dy)


def _ship_station_delta(obs: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **centre du vaisseau** → **station** (cible d'accostage /
    de stationnement) - `station_dx/dy` partent de la position du pilote, dont
    on retire le décalage de corps du vaisseau."""
    ship = obs.get("ship", {})
    return _wrap_tor(
        _finite(obs.get("station_dx")) - _finite(ship.get("center_x")),
        _finite(obs.get("station_dy")) - _finite(ship.get("center_y")),
    )


def _station_body_delta(obs: dict[str, Any], b: dict[str, Any]) -> tuple[float, float]:
    """Delta torique **station** → centre de corps de `b` (la distance qui
    décide de la garde de la station)."""
    return _wrap_tor(
        _finite(obs.get("station_dx")) - (_finite(b.get("dx")) + _finite(b.get("center_x"))),
        _finite(obs.get("station_dy")) - (_finite(b.get("dy")) + _finite(b.get("center_y"))),
    )


def _ship_supplies_low(obs: dict[str, Any]) -> bool:
    """`autopilot::supplies_low` : carburant ou munitions sous le seuil de
    ravitaillement (`LOW_SUPPLY_RATIO`), calculé des seules quantités
    exposées."""
    fuel_cap = obs.get("fuel_cap", 0.0)
    if fuel_cap > 0.0 and obs.get("fuel", 0.0) < fuel_cap * _SHIP_LOW_SUPPLY_RATIO:
        return True
    ammo_cap = obs.get("ammo_cap", 0)
    return ammo_cap > 0 and obs.get("ammo", 0) < ammo_cap * _SHIP_LOW_SUPPLY_RATIO


def _ship_mission(
    obs: dict[str, Any], nearby: list[dict[str, Any]]
) -> tuple[str, Optional[dict[str, Any]], Optional[dict[str, Any]]]:
    """Mission de la frame de l'autopilote vaisseau (le choix de `Goal` de
    `src/autopilot.rs`, priorité incluse) : renvoie `(mission, cible, cible
    d'attaque)`. La cible est `None` pour l'accostage et le stationnement (la
    station), la cible d'attaque sert à ignorer la cible en approche dans
    l'évitement."""
    capacity = obs.get("cargo_cap", 0)
    cargo_full = capacity > 0 and obs.get("cargo_qty", 0) >= capacity
    low_supplies = (
        bool(obs.get("economy"))
        and _ship_supplies_low(obs)
        and bool(obs.get("supplies_affordable"))
    )
    guard: Optional[tuple[float, dict[str, Any]]] = None
    mineral: Optional[tuple[float, dict[str, Any]]] = None
    hostile: Optional[tuple[float, dict[str, Any]]] = None
    for o in nearby:
        if o.get("kind") not in _SHIP_HOSTILE_KINDS or o.get("life", 0) <= 0:
            continue
        ds = math.hypot(*_station_body_delta(obs, o))
        if ds < _SHIP_STATION_GUARD_RADIUS and (guard is None or ds < guard[0]):
            guard = (ds, o)
        dp = math.hypot(*_ship_body_delta(obs, o))
        if hostile is None or dp < hostile[0]:
            hostile = (dp, o)
    for o in nearby:
        if o.get("kind") != "minerai" or o.get("life", 0) <= 0:
            continue
        dp = math.hypot(*_ship_body_delta(obs, o))
        if mineral is None or dp < mineral[0]:
            mineral = (dp, o)
    hostile_in_range = hostile is not None and hostile[0] < _SHIP_FIRE_RANGE
    mineral_guarded = mineral is not None and hostile is not None and (
        _body_distance(hostile[1], mineral[1]) < _SHIP_MINERAL_CLEARANCE
    )
    if cargo_full:
        return "dock", None, None
    if guard is not None:
        return "attack", guard[1], guard[1]
    if low_supplies:
        return "dock", None, None
    if hostile_in_range or mineral_guarded:
        return "attack", hostile[1], hostile[1]  # type: ignore[index]
    if mineral is not None:
        return "collect", mineral[1], None
    if hostile is not None:
        return "attack", hostile[1], hostile[1]
    return "patrol", None, None


def _ship_desired_speed(goal: str, d: float) -> float:
    """`desired_speed` du jeu : croisière, rampes d'arrêt de l'attaque et de
    la collecte, ralentissement d'accostage, rayon de stationnement."""
    if goal == "dock":
        if d < _SHIP_DOCK_SLOW_ZONE:
            return min(d * 0.04 + 0.02, 0.9)
        return _SHIP_CRUISE_SPEED
    if goal == "attack":
        return min(max(0.0, d - _SHIP_ATTACK_STANDOFF) * 0.05, _SHIP_CRUISE_SPEED)
    if goal == "collect":
        return min(max(d * 0.12, 0.3), 1.4)
    if d < _SHIP_PATROL_RADIUS:
        return 0.0
    return min(max(d * 0.08, 0.3), _SHIP_CRUISE_SPEED)


def _ship_collision_threat(
    obs: dict[str, Any],
    hostiles: list[dict[str, Any]],
    attack_target: Optional[dict[str, Any]],
) -> Optional[dict[str, Any]]:
    """`collision_threat` du jeu : premier hostile dont le passage au plus près
    survient dans la fenêtre de temps à moins du dégagement minimal. La cible
    d'attaque en approche (hors distance de sécurité) est ignorée."""
    ship = obs.get("ship", {})
    pvx = ship.get("vx", 0.0)
    pvy = ship.get("vy", 0.0)
    threat: Optional[dict[str, Any]] = None
    threat_t = math.inf
    for s in hostiles:
        dx, dy = _ship_body_delta(obs, s)
        rlen = math.hypot(dx, dy)
        if s is attack_target and rlen >= _SHIP_ATTACK_STANDOFF:
            continue
        if rlen > _SHIP_AVOID_RADIUS:
            continue
        vrx = s.get("vx", 0.0) - pvx
        vry = s.get("vy", 0.0) - pvy
        v2 = vrx * vrx + vry * vry
        t = -(dx * vrx + dy * vry) / v2 if v2 > 1e-9 else 0.0
        if t < 0.0:
            continue  # déjà en train de s'éloigner
        if math.hypot(dx + vrx * t, dy + vry * t) < _SHIP_AVOID_CLEARANCE \
                and t < _SHIP_AVOID_TIME and t < threat_t:
            threat = s
            threat_t = t
    return threat


def _ship_avoid_aim(obs: dict[str, Any], s: dict[str, Any]) -> tuple[float, float]:
    """`avoid_aim` du jeu : direction d'esquive perpendiculaire à l'approche
    relative, côté qui écarte déjà le vaisseau. Renvoie `(cap, vitesse visée)`."""
    ship = obs.get("ship", {})
    pvx = ship.get("vx", 0.0)
    pvy = ship.get("vy", 0.0)
    dx, dy = _ship_body_delta(obs, s)
    rlen = math.hypot(dx, dy)
    vrx = s.get("vx", 0.0) - pvx
    vry = s.get("vy", 0.0) - pvy
    v2 = vrx * vrx + vry * vry
    if v2 > 1e-9:
        vlen = math.sqrt(v2)
        wx, wy = vrx / vlen, vry / vlen
    elif rlen > 1e-9:
        wx, wy = dx / rlen, dy / rlen
    else:
        wx, wy = 1.0, 0.0
    p1 = (wy, -wx)
    p2 = (-wy, wx)
    e = p1 if pvx * p1[0] + pvy * p1[1] >= pvx * p2[0] + pvy * p2[1] else p2
    return math.atan2(e[1], e[0]), _SHIP_CRUISE_SPEED


def _ship_drive_features(obs: dict[str, Any]) -> list[float]:
    """**Visée de mission** de la conduite vaisseau - les grandeurs que
    l'autopilote calcule pour décider où aller et à quelle vitesse, rejouées
    depuis l'observation (comme `_ship_decision_features` pour le tir) :

    - la **mission de la frame** en one-hot (`dock`, `attack`, `collect`,
      `patrol`) : la conduite vise la **cible de la mission**, pas l'objet le
      plus proche - sans elle, le signe de rotation est indécidable (mesuré :
      l'erreur vers la station n'explique le sens de rotation qu'à 50,3 %) ;
    - le **cap effectif** (`aim`, esquive comprise) et son erreur
      d'alignement `err` - le seuil de rotation et de poussée se lit dessus ;
    - la **vitesse visée** (`desired_speed`) et la vitesse projetée sur le cap
      (`v_along`) - les seuils d'accélération, de survitesse et d'arrêt ;
    - les composantes du mode **4 WAYS** (`ex`, `ey` : l'écart au vecteur de
      vitesse visé, qui décide des quatre directions de poussée à l'écran) ;
    - les drapeaux de la conduite : **esquive** active, menace **devant**
      (freinage pendant l'esquive), **survitesse**, **arrêt** (`settle`).

    Hors vaisseau (le cosmonaute EVA pilote) : tout à zéro - la conduite
    vaisseau n'a pas cours.
    """
    if obs.get("pilot", "vaisseau") != "vaisseau":
        return [0.0] * SHIP_DRIVE_FEATURES
    ship = obs.get("ship", {})
    nearby = obs.get("nearby", [])
    goal, target, attack = _ship_mission(obs, nearby)
    hostiles = [o for o in nearby
                if o.get("kind") in _SHIP_HOSTILE_KINDS and o.get("life", 0) > 0]
    threat = _ship_collision_threat(obs, hostiles, attack)
    if threat is not None:
        # esquive : la conduite remplace le cap de mission par l'esquive
        aim, desired = _ship_avoid_aim(obs, threat)
    else:
        if target is None:
            dx, dy = _ship_station_delta(obs)
        else:
            dx, dy = _ship_body_delta(obs, target)
        aim = math.atan2(dy, dx)
        desired = _ship_desired_speed(goal, math.hypot(dx, dy))
    orientation = ship.get("orientation", 0.0)
    direction = ship.get("direction", 0.0)
    err = _wrap_angle(aim - orientation)
    # vitesse **par frame** (l'observation rapporte des unités/s) : la
    # conduite compare des unités/frame (`CRUISE_SPEED`, `desired`)
    velocity = ship.get("speed", 0.0) / 60.0
    vx = math.cos(direction) * velocity
    vy = -math.sin(direction) * velocity
    v_along = vx * math.cos(aim) + vy * math.sin(aim)
    ex = math.cos(aim) * desired - vx
    ey = math.sin(aim) * desired - vy
    threat_ahead = False
    if threat is not None:
        dx, dy = _ship_body_delta(obs, threat)
        rlen = math.hypot(dx, dy)
        if rlen >= 1e-9:
            threat_ahead = (math.cos(direction) * dx - math.sin(direction) * dy) / rlen > 0.0
    overspeed = v_along > desired + 0.15
    settle = desired < _SHIP_SETTLE_BAND and v_along > 0.02
    f = [1.0 if goal == g else 0.0 for g in ("dock", "attack", "collect", "patrol")]
    f.extend([
        _angle_feature(aim),
        _clamp(err / math.pi),
        _clamp(desired / _SHIP_CRUISE_SPEED, 0.0, 1.0),
        _clamp(v_along / _SHIP_CRUISE_SPEED),
        _clamp(ex / _SHIP_CRUISE_SPEED),
        _clamp(ey / _SHIP_CRUISE_SPEED),
        1.0 if threat is not None else 0.0,
        1.0 if threat_ahead else 0.0,
        1.0 if overspeed else 0.0,
        1.0 if settle else 0.0,
    ])
    return f


def _moving_mode_features(obs: dict[str, Any]) -> list[float]:
    """Mode de déplacement du vaisseau en **one-hot** (`src/config.rs`) : la
    conduite de l'autopilote en dépend (4 WAYS pousse dans les axes de l'écran,
    les autres orientent le nez). Un mode absent ou inconnu compte comme
    INERTIAL (0) - la valeur par défaut de l'observation côté jeu - et un mode
    hors bornes laisse le one-hot à zéro."""
    mode = obs.get("moving_mode", 0)
    return [1.0 if mode == m else 0.0 for m in range(MOVING_MODES)]


def _bullets_features(obs: dict[str, Any]) -> list[float]:
    """**Balles en vol** (les `BULLET_SLOTS` plus proches) : leur nombre
    (normalisé) puis la cinématique relative de chacune. L'autopilote vaisseau
    s'en sert pour **retenir son feu** ; la cinématique du vaisseau ne le dit
    pas. Les slots non utilisés restent à zéro."""
    bullets = obs.get("bullets", [])[: BULLET_SLOTS]
    f = [_clamp(len(bullets) / BULLET_SLOTS, 0.0, 1.0)]
    for b in bullets:
        f.extend(
            [
                _clamp(b.get("dx", 0.0) / SCALE_DIST),
                _clamp(b.get("dy", 0.0) / SCALE_DIST),
                _clamp(b.get("dist", 0.0) / SCALE_DIST, 0.0, 1.0),
                _clamp(b.get("vx", 0.0) / SCALE_SPEED),
                _clamp(b.get("vy", 0.0) / SCALE_SPEED),
            ]
        )
    f.extend([0.0] * (BULLET_SLOT_LEN * (BULLET_SLOTS - len(bullets))))
    return f


def obs_features(obs: dict[str, Any]) -> list[float]:
    """Vecteur de features à taille fixe d'une observation JSON.

    Les champs manquants valent zéro (tolérant : une observation de
    micro-simulateur EVA n'a pas d'objets proches ni d'économie, un épisode
    vaisseau n'a pas d'EVA active…). L'ordre des features est stable - c'est
    lui qui définit l'entrée du réseau.
    """
    ship = obs.get("ship", {})
    eva = obs.get("eva", {})
    station_dx = obs.get("station_dx", 0.0)
    station_dy = obs.get("station_dy", 0.0)
    f: list[float] = [
        # ── contexte global ─────────────────────────────────────────────────
        1.0 if obs.get("eva_active") else 0.0,          # pilote EVA ?
        1.0 if obs.get("docked") else 0.0,              # accosté / déchargement
        1.0 if obs.get("economy") else 0.0,             # scénario à économie
        # hystérésis du frein tangentiel de l'autopilote EVA (état interne -
        # deux cinématiques identiques peuvent porter des actions expert
        # contradictoires sans lui : c'est la bande 7-15 u/s de vitesse
        # tangentielle, le régime d'orbite)
        1.0 if obs.get("eva_tang_braking") else 0.0,
        station_dx / SCALE_DIST,
        station_dy / SCALE_DIST,
        obs.get("station_dist", 0.0) / SCALE_DIST,
        # ── cinématique du vaisseau (brute + dérivée vers la station) ───────
        *_pilot_features(ship, station_dx, station_dy),
        # ── cinématique du cosmonaute EVA (brute + dérivée) ─────────────────
        *_pilot_features(eva, station_dx, station_dy),
        # ── décisions de l'autopilote EVA (freinage, orbite, visée réelle) ──
        *_eva_decision_features(obs, station_dx, station_dy),
        # ── décisions de l'autopilote vaisseau (retenue de feu) ─────────────
        *_ship_decision_features(obs),
        # ── visée de mission de la conduite (cap, vitesse visée, esquive) ───
        *_ship_drive_features(obs),
        # ── économie (boucle de minage du vaisseau) ─────────────────────────
        obs.get("fuel", 0.0) / max(obs.get("fuel_cap", 0.0), 1.0),
        obs.get("ammo", 0) / max(obs.get("ammo_cap", 0), 1),
        obs.get("credits", 0) / SCALE_CREDITS,
        obs.get("cargo_qty", 0) / max(obs.get("cargo_cap", 0), 1),
        # ── mode de déplacement (la conduite en dépend) ────────────────────
        *_moving_mode_features(obs),
        # ── réserves payables au magasin (mission de ravitaillement) ───────
        1.0 if obs.get("supplies_affordable") else 0.0,
        # ── objectifs DAG du scénario (Phase 2 - épisodes à objectifs) : la
        # mission courante et sa progression, pour que le réseau apprenne sur
        # le **langage de tâche** du scénario (combien d'objectifs restent,
        # où en est la mission débloquée) ──
        *_objective_features(obs),
    ]
    # ── objets proches (les plus proches, slots fixes) ──────────────────────
    nearby = obs.get("nearby", [])[: NEARBY_SLOTS]
    for o in nearby:
        kind = o.get("kind", "")
        for k in NEARBY_KINDS:
            f.append(1.0 if kind == k else 0.0)
        f.extend(
            [
                _clamp(o.get("dx", 0.0) / SCALE_DIST),
                _clamp(o.get("dy", 0.0) / SCALE_DIST),
                _clamp(o.get("dist", 0.0) / SCALE_DIST, 0.0, 1.0),
                _clamp(o.get("vx", 0.0) / SCALE_SPEED),
                _clamp(o.get("vy", 0.0) / SCALE_SPEED),
                _clamp(o.get("radius", 0.0) / SCALE_RADIUS, 0.0, 1.0),
                _clamp(o.get("life", 0.0) / SCALE_LIFE, 0.0, 1.0),
            ]
        )
    # slots restants : zéros (pas d'objet à cette distance)
    for _ in range(NEARBY_SLOTS - len(nearby)):
        f.extend([0.0] * (len(NEARBY_KINDS) + 7))
    # ── balles en vol (retenue de feu de l'autopilote vaisseau) ────────────
    f.extend(_bullets_features(obs))
    return f


def _objective_features(obs: dict[str, Any]) -> list[float]:
    """Features des **objectifs DAG** du scénario courant (Phase 2 - épisodes
    à objectifs, champ `objectives` de l'observation) : la mission comme
    signal de tâche. Chaque objectif expose `unlocked` (mission en cours),
    `completed`, et la progression chiffrée de sa condition (`current` /
    `required`). Features :

    - part complétée (0..1) et part restante (`1 − part`) du scénario ;
    - la **mission courante** (premier objectif débloqué non complété) :
      sa progression `current / required` (bornée 0..1) et son rang dans la
      chaîne DAG (normalisé 0..1) - l'entraîneur sait où il en est.

    Sans objectifs (épisode libre / EVA), toutes les features valent zéro."""
    objectives = obs.get("objectives", [])
    if not objectives:
        return [0.0, 0.0, 0.0, 0.0, 0.0]  # pas de scénario à objectifs
    total = float(len(objectives))
    completed = sum(1 for o in objectives if o.get("completed"))
    done_part = _clamp(completed / total, 0.0, 1.0)
    mission: dict[str, Any] = {}
    mission_idx = 0.0
    for i, o in enumerate(objectives):
        if o.get("unlocked") and not o.get("completed"):
            mission = o
            mission_idx = float(i) / total
            break
    required = mission.get("required", 0.0)
    progress = 0.0
    if required > 0.0:
        progress = _clamp(mission.get("current", 0.0) / required, 0.0, 1.0)
    # 1 si une mission est en cours (débloquée et non complétée)
    has_mission = 1.0 if mission else 0.0
    return [done_part, 1.0 - done_part, has_mission, mission_idx, progress]


def feature_size() -> int:
    """Taille du vecteur de features (fixe, dérivée des constantes ci-dessus)."""
    return len(obs_features({}))


# ── cibles d'apprentissage ──────────────────────────────────────────────────

def action_target(action: dict[str, Any]) -> list[float]:
    """Cibles d'une action de l'autopilote (le champ `expert` de
    l'observation, ou l'`action` des trajectoires du banc d'essai) : trois
    sigmoïdes binaires (`up`, `down`, `fire`) puis un un-seul de rotation
    (`left`, `right`, `none` - l'expert n'appuie jamais les deux ensemble)."""
    y = [1.0 if action.get(a) else 0.0 for a in ACTIONS[:SIGMOID_OUTPUTS]]
    if action.get("left"):
        turn = 0
    elif action.get("right"):
        turn = 1
    else:
        turn = NONE_CLASS
    y += [1.0 if k == turn else 0.0 for k in range(TURN_HEAD)]
    return y


# ── le réseau ───────────────────────────────────────────────────────────────

class MLP:
    """Perceptron multicouche : entrée → cachée (tanh) → deux têtes de sortie.

    - `up`/`down`/`fire` : sigmoïdes indépendantes (entropie croisée binaire -
      le pilote peut pousser et tirer en même temps) ;
    - rotation : **softmax** sur {left, right, none} (entropie croisée) -
      mutuellement exclusif, comme l'autopilote : deux sigmoïdes indépendantes
      laissaient la politique coincée à enfoncer gauche **et** droite ensemble
      (aucune rotation nette - c'est l'échec « coincé en orbite » mesuré en
      boucle fermée, que le softmax supprime structurellement).

    Une seule couche cachée suffit pour imiter les décisions de l'autopilote
    (missions, visée, seuils) ; l'entraînement est une descente de gradient
    par lots avec élan (`momentum`).
    """

    def __init__(self, inputs: int, hidden: int, outputs: int, rng: Optional[random.Random] = None) -> None:
        self.inputs = inputs
        self.hidden = hidden
        self.outputs = outputs
        rng = rng or random.Random()
        # initialisation de Xavier : variances adaptées aux tailles des couches
        def init(fan_in: int, fan_out: int) -> list[list[float]]:
            limit = math.sqrt(6.0 / (fan_in + fan_out))
            return [[rng.uniform(-limit, limit) for _ in range(fan_out)] for _ in range(fan_in)]

        self.w1 = init(inputs, hidden)   # inputs × hidden
        self.b1 = [0.0] * hidden
        self.w2 = init(hidden, outputs)  # hidden × outputs
        self.b2 = [0.0] * outputs

    # ── propagation ─────────────────────────────────────────────────────────
    def forward(self, x: list[float]) -> list[float]:
        """Sorties pour une entrée : 3 sigmoïdes (up, down, fire) puis 3
        probabilités softmax (left, right, none) - somme des 3 dernières = 1."""
        h = [math.tanh(sum(x[i] * self.w1[i][j] for i in range(self.inputs)) + self.b1[j])
             for j in range(self.hidden)]
        z = [sum(h[j] * self.w2[j][k] for j in range(self.hidden)) + self.b2[k]
             for k in range(self.outputs)]
        out = [1.0 / (1.0 + math.exp(-z[k])) for k in range(SIGMOID_OUTPUTS)]
        m = max(z[SIGMOID_OUTPUTS:])
        exp_turn = [math.exp(z[SIGMOID_OUTPUTS + k] - m) for k in range(TURN_HEAD)]
        s = sum(exp_turn)
        out += [e / s for e in exp_turn]
        return out

    def predict(self, obs: dict[str, Any]) -> list[float]:
        """Sorties pour une observation JSON (chemin court pour `policies`)."""
        return self.forward(obs_features(obs))

    def turn_action(self, out: list[float]) -> str:
        """Rotation choisie par la tête softmax (le plus probable)."""
        return ("left", "right", "none")[max(range(TURN_HEAD), key=lambda k: out[SIGMOID_OUTPUTS + k])]

    # ── entraînement ────────────────────────────────────────────────────────
    def train(
        self,
        X: list[list[float]],
        Y: list[list[float]],
        epochs: int = 50,
        lr: float = 0.1,
        momentum: float = 0.9,
        batch_size: int = 64,
        patience: int = 15,
        noise: float = 0.0,
        rng: Optional[random.Random] = None,
        backend: str = "python",
    ) -> dict[str, float]:
        """Descente de gradient par lots (mini-lots) sur (X, Y).

        `X` : vecteurs de features, `Y` : cibles `action_target`. Perte =
        entropie croisée binaire sur les sigmoïdes + entropie croisée sur le
        softmax de rotation. Renvoie un résumé : perte finale, exactitude
        globale (toutes les actions justes), exactitude par action, et le
        nombre d'époques réellement effectuées (arrêt précoce).

        `backend` : `"python"` (défaut, **aucune dépendance** - la convention
        de `tools/trainer/`), `"numpy"` (accéléré, exige numpy) ou `"auto"`
        (numpy s'il est importable, sinon repli Python pur). Les deux chemins
        implémentent **le même** calcul (même perte, même élan, même arrêt
        précoce) ; seul le chemin vectorisé rend atteignables les réseaux
        larges et les gros jeux de données que le Python pur ne tient pas
        (quelques minutes contre plusieurs heures). `backend="python"` par
        défaut : ce qui est rejouable (l'inférence, le portage Rust) ne dépend
        jamais de numpy.
        """
        if backend != "python":
            try:
                import numpy  # noqa: F401
            except ImportError:
                if backend == "numpy":
                    raise
            else:
                res = self._train_numpy(
                    X, Y, epochs, lr, momentum, batch_size, patience, noise, rng)
                res["backend"] = "numpy"
                return res
        res = self._train_python(
            X, Y, epochs, lr, momentum, batch_size, patience, noise, rng)
        res["backend"] = "python"
        return res

    def _train_python(
        self,
        X: list[list[float]],
        Y: list[list[float]],
        epochs: int = 50,
        lr: float = 0.1,
        momentum: float = 0.9,
        batch_size: int = 64,
        patience: int = 15,
        noise: float = 0.0,
        rng: Optional[random.Random] = None,
    ) -> dict[str, float]:
        """Chemin d'entraînement **Python pur** (défaut, sans dépendance)."""
        rng = rng or random.Random()
        n = len(X)
        # élan (momentum) : mêmes dimensions que les poids
        v_w1 = [[0.0] * self.hidden for _ in range(self.inputs)]
        v_b1 = [0.0] * self.hidden
        v_w2 = [[0.0] * self.outputs for _ in range(self.hidden)]
        v_b2 = [0.0] * self.outputs

        def bce_loss(y: float, p: float) -> float:
            p = min(max(p, 1e-9), 1.0 - 1e-9)
            return -(y * math.log(p) + (1.0 - y) * math.log(1.0 - p))

        def total_loss(y: list[float], out: list[float]) -> float:
            loss = 0.0
            for k in range(SIGMOID_OUTPUTS):
                loss += bce_loss(y[k], out[k])
            # softmax de rotation : −log p(classe juste)
            turn = max(range(TURN_HEAD), key=lambda k: y[SIGMOID_OUTPUTS + k])
            p = min(max(out[SIGMOID_OUTPUTS + turn], 1e-9), 1.0)
            loss += -math.log(p)
            return loss

        best_loss = float("inf")
        stagnant = 0
        epochs_done = 0
        best_w1 = [row[:] for row in self.w1]
        best_b1 = list(self.b1)
        best_w2 = [row[:] for row in self.w2]
        best_b2 = list(self.b2)
        for epoch in range(epochs):
            # une époque = un passage complet en mini-lots mélangés
            idx = list(range(n))
            rng.shuffle(idx)
            epoch_loss = 0.0
            for start in range(0, n, batch_size):
                batch = idx[start : start + batch_size]
                # gradients accumulés sur le lot
                g_w1 = [[0.0] * self.hidden for _ in range(self.inputs)]
                g_b1 = [0.0] * self.hidden
                g_w2 = [[0.0] * self.outputs for _ in range(self.hidden)]
                g_b2 = [0.0] * self.outputs
                for i in batch:
                    x = X[i]
                    y = Y[i]
                    # bruit gaussien sur l'entrée (régularisation) : le réseau
                    # doit répondre juste aussi aux observations **légèrement
                    # hors distribution** - en boucle fermée, chaque erreur
                    # déplace la trajectoire et c'est exactement ce régime qui
                    # décide (la dérive de distribution du clonage)
                    if noise > 0.0:
                        x = [v + rng.gauss(0.0, noise) for v in x]
                    # ── avant ──
                    h = [math.tanh(sum(x[a] * self.w1[a][j] for a in range(self.inputs)) + self.b1[j])
                         for j in range(self.hidden)]
                    z = [sum(h[j] * self.w2[j][k] for j in range(self.hidden)) + self.b2[k]
                         for k in range(self.outputs)]
                    out = [1.0 / (1.0 + math.exp(-z[k])) for k in range(SIGMOID_OUTPUTS)]
                    m = max(z[SIGMOID_OUTPUTS:])
                    exp_turn = [math.exp(z[SIGMOID_OUTPUTS + k] - m) for k in range(TURN_HEAD)]
                    s = sum(exp_turn)
                    out += [e / s for e in exp_turn]
                    epoch_loss += total_loss(y, out)
                    # ── arrière : d = p − y pour sigmoïde (BCE) **et** softmax
                    # (entropie croisée) - la même forme sert aux deux têtes ──
                    d_out = [out[k] - y[k] for k in range(self.outputs)]
                    # dérivée de tanh : 1 − h²
                    d_h = [0.0] * self.hidden
                    for j in range(self.hidden):
                        d_h[j] = (1.0 - h[j] * h[j]) * sum(d_out[k] * self.w2[j][k] for k in range(self.outputs))
                    for k in range(self.outputs):
                        g_b2[k] += d_out[k]
                        for j in range(self.hidden):
                            g_w2[j][k] += d_out[k] * h[j]
                    for j in range(self.hidden):
                        g_b1[j] += d_h[j]
                        for a in range(self.inputs):
                            g_w1[a][j] += d_h[j] * x[a]
                # ── mise à jour avec élan (moyenne sur le lot) ──
                scale = lr / len(batch)
                for a in range(self.inputs):
                    for j in range(self.hidden):
                        v_w1[a][j] = momentum * v_w1[a][j] + scale * g_w1[a][j]
                        self.w1[a][j] -= v_w1[a][j]
                for j in range(self.hidden):
                    v_b1[j] = momentum * v_b1[j] + scale * g_b1[j]
                    self.b1[j] -= v_b1[j]
                for j in range(self.hidden):
                    for k in range(self.outputs):
                        v_w2[j][k] = momentum * v_w2[j][k] + scale * g_w2[j][k]
                        self.w2[j][k] -= v_w2[j][k]
                for k in range(self.outputs):
                    v_b2[k] = momentum * v_b2[k] + scale * g_b2[k]
                    self.b2[k] -= v_b2[k]
            loss = epoch_loss / n
            if loss < best_loss - 1e-5:
                best_loss = loss
                stagnant = 0
                best_w1 = [row[:] for row in self.w1]
                best_b1 = list(self.b1)
                best_w2 = [row[:] for row in self.w2]
                best_b2 = list(self.b2)
            else:
                stagnant += 1
                if stagnant >= patience:
                    break  # arrêt précoce : plus de progrès
            epochs_done += 1
        # restaure les poids de la meilleure époque (l'arrêt précoce évite de
        # finir sur une époque de sur-apprentissage)
        self.w1, self.b1, self.w2, self.b2 = best_w1, best_b1, best_w2, best_b2
        summary = self.summary(X, Y)
        summary["epochs_done"] = epochs_done
        return summary

    def _train_numpy(
        self,
        X: list[list[float]],
        Y: list[list[float]],
        epochs: int = 50,
        lr: float = 0.1,
        momentum: float = 0.9,
        batch_size: int = 64,
        patience: int = 15,
        noise: float = 0.0,
        rng: Optional[random.Random] = None,
    ) -> dict[str, float]:
        """Même entraînement que `_train_python`, **vectorisé par numpy**.

        Le calcul est celui du chemin Python (BCE sur les sigmoïdes `up`/`down`/
        `fire` + entropie croisée du softmax de rotation, élan identique, arrêt
        précoce identique) ; seuls l'ordre des opérations flottantes et le
        tirage du bruit d'entrée diffèrent - les poids obtenus ne sont donc pas
        identiques au bit près à ceux du Python pur, ce qui est sans
        conséquence : le réseau est ensuite **rejoué** (`forward`, Python pur,
        et le portage Rust) sur les poids sauvegardés, jamais ré-entraîné.
        """
        import numpy as np

        rng = rng or random.Random()
        n = len(X)
        Xa = np.array(X, dtype=np.float64)
        Ya = np.array(Y, dtype=np.float64)
        w1 = np.array(self.w1, dtype=np.float64)
        b1 = np.array(self.b1, dtype=np.float64)
        w2 = np.array(self.w2, dtype=np.float64)
        b2 = np.array(self.b2, dtype=np.float64)
        # élan : mêmes dimensions que les poids
        v_w1 = np.zeros_like(w1)
        v_b1 = np.zeros_like(b1)
        v_w2 = np.zeros_like(w2)
        v_b2 = np.zeros_like(b2)

        best_loss = float("inf")
        stagnant = 0
        epochs_done = 0
        best = (w1.copy(), b1.copy(), w2.copy(), b2.copy())
        for _ in range(epochs):
            idx = list(range(n))
            rng.shuffle(idx)
            epoch_loss = 0.0
            for start in range(0, n, batch_size):
                batch = idx[start : start + batch_size]
                xb = Xa[batch]
                yb = Ya[batch]
                if noise > 0.0:
                    xb = xb + np.array(
                        [[rng.gauss(0.0, noise) for _ in range(xb.shape[1])]
                         for _ in range(xb.shape[0])])
                # ── avant ──
                h = np.tanh(xb @ w1 + b1)
                z = h @ w2 + b2
                out = np.empty_like(z)
                out[:, :SIGMOID_OUTPUTS] = 1.0 / (1.0 + np.exp(-z[:, :SIGMOID_OUTPUTS]))
                zt = z[:, SIGMOID_OUTPUTS:]
                zt = zt - zt.max(axis=1, keepdims=True)
                et = np.exp(zt)
                out[:, SIGMOID_OUTPUTS:] = et / et.sum(axis=1, keepdims=True)
                # ── perte (mêmes bornes que le Python pur) ──
                ps = np.clip(out[:, :SIGMOID_OUTPUTS], 1e-9, 1.0 - 1e-9)
                bce = -(yb[:, :SIGMOID_OUTPUTS] * np.log(ps)
                        + (1.0 - yb[:, :SIGMOID_OUTPUTS]) * np.log(1.0 - ps))
                pturn = np.clip(
                    (out[:, SIGMOID_OUTPUTS:] * yb[:, SIGMOID_OUTPUTS:]).sum(axis=1),
                    1e-9, 1.0)
                epoch_loss += float(bce.sum() - np.log(pturn).sum())
                # ── arrière : d = p − y pour la sigmoïde (BCE) **et** le softmax
                # (entropie croisée) - la même forme sert aux deux têtes ──
                d_out = out - yb
                d_h = (1.0 - h * h) * (d_out @ w2.T)
                # ── mise à jour avec élan (moyenne sur le lot) ──
                scale = lr / len(batch)
                v_w1 = momentum * v_w1 + scale * (xb.T @ d_h)
                v_b1 = momentum * v_b1 + scale * d_h.sum(axis=0)
                v_w2 = momentum * v_w2 + scale * (h.T @ d_out)
                v_b2 = momentum * v_b2 + scale * d_out.sum(axis=0)
                w1 -= v_w1
                b1 -= v_b1
                w2 -= v_w2
                b2 -= v_b2
            loss = epoch_loss / n
            if loss < best_loss - 1e-5:
                best_loss = loss
                stagnant = 0
                best = (w1.copy(), b1.copy(), w2.copy(), b2.copy())
            else:
                stagnant += 1
                if stagnant >= patience:
                    break  # arrêt précoce : plus de progrès
            epochs_done += 1
        # restaure les poids de la meilleure époque, en **listes Python** (le
        # JSON et l'inférence `forward` ne connaissent que ça)
        self.w1 = best[0].tolist()
        self.b1 = best[1].tolist()
        self.w2 = best[2].tolist()
        self.b2 = best[3].tolist()
        summary = self.summary(X, Y)
        summary["epochs_done"] = epochs_done
        return summary

    def summary(self, X: list[list[float]], Y: list[list[float]]) -> dict[str, float]:
        """Exactitudes sur (X, Y) : globale (toutes les actions justes) et par
        action (positions de `ACTIONS`), plus la perte moyenne (BCE +
        entropie croisée de rotation)."""
        n = len(X)
        loss = 0.0
        per_action = [0.0] * len(ACTIONS)
        exact = 0
        # positions des sigmoïdes dans `ACTIONS` : up=0, down=1, fire=4
        sigmoid_slot = {"up": 0, "down": 1, "fire": 4}
        for x, y in zip(X, Y):
            out = self.forward(x)
            ok_turn = self.turn_action(out) == self.turn_action(y)
            for k, a in enumerate(("up", "down", "fire")):
                p = min(max(out[k], 1e-9), 1.0 - 1e-9)
                loss += -(y[k] * math.log(p) + (1.0 - y[k]) * math.log(1.0 - p))
                per_action[sigmoid_slot[a]] += 1.0 if (out[k] >= 0.5) == (y[k] >= 0.5) else 0.0
            turn = max(range(TURN_HEAD), key=lambda k: y[SIGMOID_OUTPUTS + k])
            p = min(max(out[SIGMOID_OUTPUTS + turn], 1e-9), 1.0)
            loss += -math.log(p)
            for a in ("left", "right"):
                pred = self.turn_action(out) == a
                want = self.turn_action(y) == a
                per_action[ACTIONS.index(a)] += 1.0 if pred == want else 0.0
            if ok_turn and all((out[k] >= 0.5) == (y[k] >= 0.5) for k in range(SIGMOID_OUTPUTS)):
                exact += 1
        return {
            "loss": loss / n,
            "accuracy": exact / n,
            "per_action": [pa / n for pa in per_action],
        }


# ── sérialisation (même esprit que policy.json) ─────────────────────────────

def save_nn(path: str, net: MLP, meta: Optional[dict[str, Any]] = None) -> None:
    """Écrit les poids + métadonnées (dont la version des features) en JSON."""
    payload: dict[str, Any] = {
        "policy": "nn",
        "features_version": FEATURES_VERSION,
        "inputs": net.inputs,
        "hidden": net.hidden,
        "outputs": net.outputs,
        "actions": list(ACTIONS),
        "sigmoid_outputs": SIGMOID_OUTPUTS,
        "turn_head": TURN_HEAD,
        "w1": net.w1,
        "b1": net.b1,
        "w2": net.w2,
        "b2": net.b2,
    }
    if meta:
        payload["meta"] = meta
    with open(path, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")


def load_nn(path: str) -> MLP:
    """Recharge un réseau depuis un fichier JSON (sortie de `save_nn`)."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("policy") != "nn":
        raise ValueError(f"{path} n'est pas une politique réseau de neurones")
    if data.get("features_version") != FEATURES_VERSION:
        raise ValueError(
            f"{path} : version de features {data.get('features_version')} "
            f"≠ {FEATURES_VERSION} attendue (re-entraîner avec imitate.py)"
        )
    if data.get("sigmoid_outputs") != SIGMOID_OUTPUTS or data.get("turn_head") != TURN_HEAD:
        raise ValueError(f"{path} : tête de sortie incompatible (re-entraîner)")
    net = MLP(data["inputs"], data["hidden"], data["outputs"])
    net.w1 = data["w1"]
    net.b1 = data["b1"]
    net.w2 = data["w2"]
    net.b2 = data["b2"]
    return net