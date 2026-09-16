#!/usr/bin/env python3
"""Micro-simulateur du **vaisseau** pour l'auto-entraînement : la boucle de
minage (décoller → miner → décharger) côté simulateur, sur le modèle de
`eva_env.py` pour le cosmonaute.

## Ce qui est **fidèle** (porté du jeu, validable)

- la **cinématique du vaisseau** : les quatre modes de déplacement
  (`src/input.rs::player_controls` - DIRECTIONAL, INERTIAL, REALISTIC,
  4 WAYS), `thrust_vector`, `realistic_rotation_after_input`, les constantes
  `PLAYER_ACCELERATION` / `PLAYER_ROTATION_SPEED` et le déplacement
  (`shape::moving_shape`) ;
- le **tir** : cadence (`PLAYER_FIRE_COOLDOWN`), balle au pivot du vaisseau,
  `direction = −orientation`, `velocity = vitesse du vaisseau + 2`, rayon 10
  (`generate::fire_bullet`, tir classique) ;
- le **minage** : un tir retire **un triangle** au météore (le champ minier
  n'a pas de triangle minéralisé : aucun minerai libéré tant que le météore
  vit) ; à la destruction, les minerais (`minerals`) sont **libérés** à la
  position du météore avec dispersion (`release_meteor_minerals`) ;
- le **champ minier** de l'épisode (`generate::seed_mining_field`) : 8
  météores inertes de 4-6 triangles, **minéralisés**, semés sur un anneau
  450-1200 u autour de la station - la cible de la boucle ;
- la **collecte** (contact vaisseau/minerai, soute non pleine), l'**économie**
  Progression (carburant 100, 2 u/s de poussée, munitions 30, 1 par tir,
  soute 5, prix du carburant/munitions), l'**accostage** (centre dans le
  cercle de 15 u **et** vitesse < 0,5 u/s → déchargement, ravitaillement,
  livraison) et la **récompense d'épisode** (règles du jeu).

## Ce qui est **approché** (la frontière hybride, documentée)

- la **géométrie** : un météore est un **cercle** (position + rayon) au lieu
  d'un mesh de triangles, et les collisions sont **cercle/cercle** (pas de
  SAT triangle à triangle). L'éclatement en fragments non adjacents, les
  griffures de la station et les chocs météore↔météore ne sont pas simulés ;
- les **départs perturbés** placent le vaisseau hors de son départ à quai
  (le jeu ne le fait pas) : c'est un outil d'entraînement, pas une règle du
  jeu - la **vraie partie fait foi** ;
- quelques détails d'économie (valeurs d'éléments, prix exacts du catalogue
  d'armes) sont ramenés à des valeurs simples et documentées.

L'observation produite a le **même format JSON** que `/obs` : les politiques
et `ship_autopilot_ref.py` (l'autopilote vaisseau porté) s'y exécutent sans
changement - la mesure « politique vs autopilote » est donc possible hors
ligne, sans processus headless.
"""

from __future__ import annotations

import math
import random
from typing import Any, Optional

# Réutilise **les mêmes règles de récompense** que le jeu / l'entraîneur
# (`eva_env.episode_reward` : +1000 réussi − 2 s/s − pénalité d'arrivée trop
# rapide ; échec : −2 s/s − 50 − distance finale × 0,1).
from eva_env import episode_reward

# ── constantes du jeu (src/config.rs, src/input.rs, src/scenario.rs) ────────
TAU = 2.0 * math.pi
PLAYER_ACCELERATION = 0.05            # unités/frame (×60 = unités/s²)
PLAYER_ROTATION_SPEED = TAU / 210.0   # rad/frame (×60 = rad/s)
PLAYER_ROTATION_ACCELERATION = PLAYER_ROTATION_SPEED * 2.0
PLAYER_FIRE_COOLDOWN = 1.0 / 3.0      # s entre deux tirs
STATION_DOCK_DISTANCE = 15.0          # cercle d'accostage (centre de la base)
STATION_DOCK_SPEED = 0.5              # u/s - il faut ralentir pour accoster
CARGO_SIZE = 5                        # soute de base

# modes de déplacement (src/config.rs)
MOVING_MODE_INERTIAL = 0
MOVING_MODE_4_WAYS = 1
MOVING_MODE_DIRECTIONAL = 2
MOVING_MODE_REALISTIC = 3

# monde torique (src/config.rs) - la station est posée en (0, 0)
WORLD_W = 3960.0
WORLD_H = 3540.0
FRAMES_PER_SECOND = 60.0

# champ minier (src/generate.rs::seed_mining_field)
MINING_FIELD_COUNT = 8
MINING_FIELD_RADIUS_MIN = 450.0
MINING_FIELD_RADIUS_MAX = 1200.0
MINING_FIELD_TRIANGLES_MIN = 4
MINING_FIELD_TRIANGLES_MAX = 6
MINERAL_SCATTER_SPEED = 0.15   # u/frame - évite les minerais empilés
MINERAL_SPAWN_SPREAD = 10.0    # u - dispersion des minerais libérés

# économie (scénario Progression, src/scenario.rs / src/marketplace.rs)
ECONOMY_START_FUEL = 100.0
ECONOMY_FUEL_PER_SECOND = 2.0
ECONOMY_START_AMMO = 30
ECONOMY_AMMO_PER_SHOT = 1
ECONOMY_AMMO_CAPACITY = 30
FUEL_PRICE = 1          # crédits par paquet de FUEL_STEP unités
FUEL_STEP = 10.0
AMMO_PRICE = 1          # crédits par paquet de AMMO_STEP munitions
AMMO_STEP = 5

# géométrie approximée (frontière hybride) : rayons des corps « ronds »
SHIP_RADIUS = 10.0
BULLET_RADIUS = 10.0
MINERAL_RADIUS = 10.0
METEOR_RADIUS_MIN = 22.0
METEOR_RADIUS_MAX = 42.0

#: Facteur de collision des météores : `radius` est le **maximum** des
#: distances sommet+hauteur (une borne), pas le rayon d'un disque - un mesh
#: irrégulier présente une surface bien plus petite dans la plupart des
#: directions. La collision par cercles doit donc utiliser un rayon
#: **effectif** plus petit, sinon on « touche » un météore là où le jeu (SAT
#: triangle à triangle) ne le fait pas : mesuré contre la vraie partie de
#: référence (approche la plus proche 44 u d'un météore de rayon 40), le
#: facteur 0,6 garantit l'absence de contact fantôme.
METEOR_COLLISION_FACTOR = 0.6

#: Marge de **ramassage** des minerais (unités) : le contact réel est celui de
#: deux meshes (vaisseau ~10 + minerai ~10) mais la tolérance est plus large en
#: pratique - mesuré sur la vraie partie de référence, le vaisseau ramasse un
#: minerai dont le voisin le plus proche du même type est encore à ~35 u. Sans
#: cette marge, un quasi-passage se transforme en orbite (le vaisseau ne peut
#: plus revenir sur un minerai manqué de quelques unités).
PICKUP_MARGIN = 15.0

#: Durée de vie d'une balle (s) : le jeu recycle les formes lointaines ; ici
#: la balle disparaît après un trajet raisonnable (approximation).
BULLET_LIFETIME = 2.0

#: Valeur (crédits) d'un minerai au déchargement par élément (approximation
#: de `ELEMENT_VALUES` : or/fer/eau valent 1, le platine du boss 10).
ELEMENT_VALUES = {1: 1, 2: 1, 3: 1, 4: 10}

#: Nombre maximal d'objets rapportés dans l'observation (comme le jeu).
MAX_NEARBY_OBJECTS = 32

#: Dénouements d'un épisode vaisseau (mêmes libellés que le jeu).
OUTCOME_DELIVERED = "delivered"
OUTCOME_DESTROYED = "destroyed"

def wrap_angle(a: float) -> float:
    """Ramène un angle dans ]−π, π]."""
    return (a + math.pi) % TAU - math.pi


def wrapped_dxdy(x1: float, y1: float, x2: float, y2: float) -> tuple[float, float]:
    """Delta torique le plus court du point 1 vers le point 2."""
    dx = x2 - x1
    dy = y2 - y1
    dx -= WORLD_W * round(dx / WORLD_W)
    dy -= WORLD_H * round(dy / WORLD_H)
    return dx, dy


def thrust_vector(body: dict[str, float], acc: float, orientation: float,
                  sx: float, sy: float) -> None:
    """Ajoute une poussée le long de `orientation` (port de `thrust_vector`) :
    combine la vitesse courante et la poussée, recalcule direction/vitesse."""
    dx1 = math.cos(body["direction"]) * body["velocity"]
    dy1 = math.sin(body["direction"]) * body["velocity"]
    dx2 = math.cos(orientation) * acc * sx
    dy2 = math.sin(orientation) * acc * sy
    dx = dx1 + dx2
    dy = dy1 + dy2
    body["direction"] = math.atan2(dy, dx)
    body["velocity"] = math.hypot(dx, dy)


def realistic_rotation_after_input(current: float, right: bool, left: bool,
                                   dt: float) -> float:
    """Vitesse angulaire du mode REALISTIC (port de `realistic_rotation_after_input`)."""
    max_speed = PLAYER_ROTATION_SPEED * 60.0
    accel = PLAYER_ROTATION_ACCELERATION * 60.0
    direction = 1.0 if right and not left else (-1.0 if left and not right else 0.0)
    return min(max(current + direction * accel * dt, -max_speed), max_speed)


class ShipSim:
    """Micro-simulateur de la boucle de minage du vaisseau (voir l'en-tête du
    module pour la frontière de fidélité).

    Le **mode de déplacement** par défaut est **DIRECTIONAL**, celui de la vraie
    partie : les 90 fenêtres de `fixtures/ship_physics_windows.json` (une vraie
    partie `economy`, headless) sont **toutes** en mode 2 - c'est donc la branche
    validée à la précision machine. Un autre défaut entraînerait sur un mode que
    le jeu n'utilise pas : mesuré, un réseau entraîné en REALISTIC (3) alors que
    la partie tourne en DIRECTIONAL (2) recevait un one-hot de mode **jamais vu**
    et prenait de mauvaises décisions en boucle fermée.
    """

    def __init__(self, seed: int = 0, *, moving_mode: int = MOVING_MODE_DIRECTIONAL,
                 economy: bool = True, timeout: float = 120.0) -> None:
        self.rng = random.Random(seed)
        self.dt = 1.0 / FRAMES_PER_SECOND
        self.seed = seed
        self.moving_mode = moving_mode
        self.economy = economy
        self.timeout = timeout
        self.cargo_cap = CARGO_SIZE
        self.ammo_cap = ECONOMY_AMMO_CAPACITY if economy else 0
        self.fuel_cap = ECONOMY_START_FUEL if economy else 0.0
        # état (rempli par `reset`)
        self.ship: dict[str, float] = {}
        self.objects: list[dict[str, Any]] = []
        self.bullets: list[dict[str, float]] = []
        self.cargo_qty = 0
        self.cargo_elements: list[int] = []
        self.fuel = 0.0
        self.ammo = 0
        self.credits = 0
        self.fire_cooldown = 0.0
        self.docked = False
        #: Les liens d'accostage sont désarmés dès qu'on quitte le quai : il
        #: faut **sortir de la base** avant de pouvoir accoster de nouveau
        #: (sinon le vaisseau se raccroche au cercle de 15 u à chaque pas).
        self.dock_armed = True
        self.meteors_destroyed = 0
        self.frame = 0
        self.t = 0.0
        self.done = False
        self.outcome: Optional[str] = None
        self.deliveries = 0
        self.entry_speed = 0.0

    # ── remise à zéro ───────────────────────────────────────────────────────
    def reset(self, seed: int, x: float = 0.0, y: float = 0.0,
              orientation: float = 0.0, vx: float = 0.0, vy: float = 0.0, *,
              moving_mode: Optional[int] = None, cargo: int = 0,
              fuel: Optional[float] = None, ammo: Optional[int] = None,
              credits: int = 0) -> dict[str, Any]:
        """Départ d'épisode. Le départ **nominal du jeu** est le vaisseau
        **à quai** au centre de la station (`x = y = vx = vy = 0`) ; les
        autres états servent aux **départs perturbés** de l'entraînement
        (`ship_warmstart.py`). `vx/vy` sont en unités/s, mêmes axes que
        l'observation (`ship.vx/vy`)."""
        self.rng = random.Random(seed)
        self.seed = seed
        if moving_mode is not None:
            self.moving_mode = moving_mode
        speed = math.hypot(vx, vy) / FRAMES_PER_SECOND
        direction = math.atan2(-vy, vx) if speed > 0.0 else 0.0
        self.ship = {
            "x": x % WORLD_W,
            "y": y % WORLD_H,
            "direction": direction,
            "velocity": speed,
            "orientation": orientation,
            "rotation": 0.0,
        }
        self._seed_mining_field()
        self.bullets = []
        self.cargo_qty = 0
        self.cargo_elements = []
        self.fuel = self.fuel_cap if fuel is None else min(max(fuel, 0.0), self.fuel_cap)
        self.ammo = self.ammo_cap if ammo is None else min(max(ammo, 0), self.ammo_cap)
        self.credits = credits
        # soute de départ : minerais d'éléments 1..3 (départ en mission)
        self.cargo_qty = max(0, min(cargo, self.cargo_cap))
        self.cargo_elements = [1 + (i % 3) for i in range(self.cargo_qty)]
        self.fire_cooldown = 0.0
        self.meteors_destroyed = 0
        self.frame = 0
        self.t = 0.0
        self.done = False
        self.outcome = None
        self.deliveries = 0
        self.entry_speed = 0.0
        # à quai si le départ est au centre et (presque) immobile - le seuil
        # `STATION_DOCK_SPEED` est comparé à la vitesse **par frame** (comme le
        # jeu : `shapes[PLAYER].velocity < STATION_DOCK_SPEED`)
        self.docked = (self.station_dist() < STATION_DOCK_DISTANCE
                       and self.ship["velocity"] < STATION_DOCK_SPEED)
        self.dock_armed = self.docked
        if self.docked:
            # un départ à quai avec une soute non vide se décharge tout de suite
            # (comme au retour à la base) - le départ nominal du jeu a la soute
            # vide, ce cas ne concerne que les départs perturbés
            self._unload()
        return self.obs()

    def _seed_mining_field(self) -> None:
        """Champ minier déterministe de l'épisode - port de
        `generate::seed_mining_field` (8 météores inertes, minéralisés, sur un
        anneau 450-1200 u). La **géométrie** est un cercle (frontière hybride)."""
        self.objects = []
        for i in range(MINING_FIELD_COUNT):
            angle = TAU * i / MINING_FIELD_COUNT
            radius = MINING_FIELD_RADIUS_MIN + (
                MINING_FIELD_RADIUS_MAX - MINING_FIELD_RADIUS_MIN) * self.rng.random()
            life = MINING_FIELD_TRIANGLES_MIN + int(
                self.rng.random() * (MINING_FIELD_TRIANGLES_MAX - MINING_FIELD_TRIANGLES_MIN))
            self.objects.append({
                "kind": "meteore",
                "x": (math.cos(angle) * radius) % WORLD_W,
                "y": (math.sin(angle) * radius) % WORLD_H,
                "direction": self.rng.random() * TAU,  # orient de la vitesse : nul
                "velocity": 0.0,                         # météores du champ inertes
                "orientation": 0.0,
                "rotation": 0.0,
                "radius": METEOR_RADIUS_MIN
                + self.rng.random() * (METEOR_RADIUS_MAX - METEOR_RADIUS_MIN),
                "life": life,
                "minerals": life,  # un minerai par triangle, libérés à la mort
            })

    # ── état / observation ─────────────────────────────────────────────────
    def station_dist(self) -> float:
        dx, dy = wrapped_dxdy(self.ship["x"], self.ship["y"], 0.0, 0.0)
        return math.hypot(dx, dy)

    def speed(self) -> float:
        return self.ship["velocity"] * FRAMES_PER_SECOND

    def supplies_affordable(self) -> bool:
        """Au moins un paquet de carburant ou de munitions est payable
        (`autopilot::supplies_affordable`, simplifié aux prix de base)."""
        if not self.economy or self.credits < 1:
            return False
        return self.fuel < self.fuel_cap or self.ammo < self.ammo_cap

    @staticmethod
    def _collision_radius(body: dict[str, Any]) -> float:
        """Rayon de collision **effectif** d'un corps : le rayon observé, réduit
        pour les météores (borne géométrique, voir `METEOR_COLLISION_FACTOR`)."""
        if body.get("kind") == "meteore":
            return body["radius"] * METEOR_COLLISION_FACTOR
        return body["radius"]

    def _obj_obs(self, o: dict[str, Any]) -> dict[str, Any]:
        dx, dy = wrapped_dxdy(self.ship["x"], self.ship["y"], o["x"], o["y"])
        speed = o["velocity"] * FRAMES_PER_SECOND
        return {
            "kind": o["kind"],
            "dx": dx,
            "dy": dy,
            "dist": math.hypot(dx, dy),
            "vx": math.cos(o["direction"]) * speed,
            "vy": -math.sin(o["direction"]) * speed,
            "radius": o["radius"],
            "life": o.get("life", 1),
            "center_x": 0.0,
            "center_y": 0.0,
        }

    def obs(self) -> dict[str, Any]:
        """Observation au format `/obs` du jeu."""
        s = self.ship
        speed = self.speed()
        dx, dy = wrapped_dxdy(s["x"], s["y"], 0.0, 0.0)
        nearby = [self._obj_obs(o) for o in self.objects
                  if o.get("life", 1) > 0]
        nearby.sort(key=lambda o: o["dist"])
        nearby = nearby[:MAX_NEARBY_OBJECTS]
        bullets = [self._obj_obs(b) for b in self.bullets if b.get("life", 1) > 0]
        bullets.sort(key=lambda o: o["dist"])
        bullets = bullets[:MAX_NEARBY_OBJECTS]
        return {
            "frame": self.frame,
            "t": self.t,
            "pilot": "vaisseau",
            "ship": {
                "x": s["x"], "y": s["y"],
                "vx": math.cos(s["direction"]) * speed,
                "vy": -math.sin(s["direction"]) * speed,
                "speed": speed,
                "direction": s["direction"],
                "orientation": s["orientation"],
                "rotation": s["rotation"],
                "center_x": 0.0, "center_y": 0.0,
            },
            "eva": {"x": 0.0, "y": 0.0, "vx": 0.0, "vy": 0.0, "speed": 0.0,
                    "direction": 0.0, "orientation": 0.0, "rotation": 0.0,
                    "center_x": 0.0, "center_y": 0.0},
            "eva_active": False,
            "station_x": 0.0,
            "station_y": 0.0,
            "station_radius": 162.0,
            "station_dx": dx,
            "station_dy": dy,
            "station_dist": math.hypot(dx, dy),
            "docked": self.docked,
            "dock_anim": 0.0,
            "dock_retract": 0.0,
            "dock_box": self.docked,
            "eva_recovery": 0.0,
            "eva_crossfade": 0.0,
            "eva_tang_braking": False,
            "paused": False,
            "game_over": False,
            "autopilot": False,
            "driver_engaged": True,
            "moving_mode": self.moving_mode,
            "economy": self.economy,
            "fuel": self.fuel,
            "fuel_cap": self.fuel_cap,
            "ammo": self.ammo,
            "ammo_cap": self.ammo_cap,
            "credits": self.credits,
            "cargo_qty": self.cargo_qty,
            "cargo_cap": self.cargo_cap,
            "supplies_affordable": self.supplies_affordable(),
            "meteors_destroyed": self.meteors_destroyed,
            "score": self.meteors_destroyed,
            "episode_id": 0,
            "episode_steps": self.frame,
            "episode_t": self.t,
            "episode_done": self.done,
            "episode_outcome": self.outcome,
            "episode_deliveries": self.deliveries,
            "episode_collected": 0,
            "objectives": [],
            "objectives_total": 0,
            "objectives_completed": 0,
            "objective_bonus": 0.0,
            "expert": {"up": False, "down": False, "left": False, "right": False,
                       "fire": False},
            "nearby": nearby,
            "bullets": bullets,
        }

    # ── pas de physique ────────────────────────────────────────────────────
    def step(self, up: bool = False, down: bool = False, left: bool = False,
             right: bool = False, fire: bool = False) -> dict[str, Any]:
        """Un pas de 1/60 s - mêmes formules que le jeu (voir l'en-tête)."""
        if self.done:
            return self.obs()
        dt = self.dt
        # quitter le quai : toute commande de déplacement rétracte les liens
        # (dans le jeu, `player_moving_input` déclenche la rétraction)
        if self.docked and (up or down or left or right):
            self.docked = False
            self.dock_armed = False  # il faudra ressortir de la base
        self._apply_inputs(up, down, left, right)
        # tir (cooldown, munitions)
        if fire:
            self._try_fire()
        self.fire_cooldown = max(0.0, self.fire_cooldown - dt)
        # déplacement de toutes les formes
        self._move_all(dt)
        self._collisions()
        self._docking()
        self.frame += 1
        self.t += dt
        if not self.done and self.t >= self.timeout:
            self.done = True  # délai dépassé (échec, `outcome = None`)
        return self.obs()

    def _apply_inputs(self, up: bool, down: bool, left: bool, right: bool) -> None:
        dt = self.dt
        acc = PLAYER_ACCELERATION * 60.0 * dt
        fuel_ok = (not self.economy) or self.fuel > 0.0
        s = self.ship
        mode = self.moving_mode
        thrusting = False
        if mode == MOVING_MODE_DIRECTIONAL:
            s["rotation"] = 0.0
            if fuel_ok and up:
                s["velocity"] += acc
                thrusting = True
            if right:
                s["direction"] -= PLAYER_ROTATION_SPEED * 60.0 * dt
                s["orientation"] = -s["direction"]
            if fuel_ok and down:
                if s["velocity"] > 0.0:
                    s["velocity"] -= acc
                    thrusting = True
                else:
                    s["velocity"] = 0.0
            if left:
                s["direction"] += PLAYER_ROTATION_SPEED * 60.0 * dt
                s["orientation"] = -s["direction"]
        elif mode == MOVING_MODE_INERTIAL:
            s["rotation"] = 0.0
            if fuel_ok and up:
                thrust_vector(s, acc, s["orientation"], 1.0, -1.0)
                thrusting = True
            if right:
                s["orientation"] += PLAYER_ROTATION_SPEED * 60.0 * dt
            if fuel_ok and down:
                thrust_vector(s, acc, s["orientation"], -1.0, 1.0)
                thrusting = True
            if left:
                s["orientation"] -= PLAYER_ROTATION_SPEED * 60.0 * dt
        elif mode == MOVING_MODE_REALISTIC:
            if fuel_ok and up:
                thrust_vector(s, acc, s["orientation"], 1.0, -1.0)
                thrusting = True
            s["rotation"] = realistic_rotation_after_input(s["rotation"], right, left, dt)
            if fuel_ok and down:
                thrust_vector(s, acc, s["orientation"], -1.0, 1.0)
                thrusting = True
        elif mode == MOVING_MODE_4_WAYS:
            s["rotation"] = 0.0
            for active, (sx, sy) in ((up, (0.0, 1.0)), (right, (1.0, 0.0)),
                                     (down, (0.0, -1.0)), (left, (-1.0, 0.0))):
                if active and fuel_ok:
                    dx = math.cos(s["direction"]) * s["velocity"] + acc * sx
                    dy = math.sin(s["direction"]) * s["velocity"] + acc * sy
                    s["direction"] = math.atan2(dy, dx)
                    s["velocity"] = math.hypot(dx, dy)
                    s["orientation"] = -s["direction"]
        # carburant : la poussée avant/arrière consomme (`consume_fuel`)
        if self.economy and thrusting:
            self.fuel = max(0.0, self.fuel - ECONOMY_FUEL_PER_SECOND * dt)

    def _try_fire(self) -> None:
        if self.fire_cooldown > 0.0:
            return
        if self.economy and self.ammo < ECONOMY_AMMO_PER_SHOT:
            return
        s = self.ship
        self.bullets.append({
            "kind": "balle",
            "x": s["x"],
            "y": s["y"],
            "direction": -s["orientation"],
            "velocity": s["velocity"] + 2.0,
            "orientation": s["orientation"],
            "rotation": 0.0,
            "radius": BULLET_RADIUS,
            "life": 1,
            "age": 0.0,
        })
        self.fire_cooldown = PLAYER_FIRE_COOLDOWN
        if self.economy:
            self.ammo -= ECONOMY_AMMO_PER_SHOT

    def _move_all(self, dt: float) -> None:
        for body in [self.ship, *self.objects, *self.bullets]:
            body["x"] = (body["x"] + math.cos(body["direction"])
                         * 60.0 * body["velocity"] * dt) % WORLD_W
            body["y"] = (body["y"] - math.sin(body["direction"])
                         * 60.0 * body["velocity"] * dt) % WORLD_H
            # `moving_shape` : l'orientation tourne à `rotation` rad/s (mode
            # REALISTIC ; les autres modes posent l'orientation directement)
            body["orientation"] += body.get("rotation", 0.0) * dt
            if body.get("kind") == "balle":
                body["age"] += dt
        self.bullets = [b for b in self.bullets if b["age"] < BULLET_LIFETIME]

    def _collisions(self) -> None:
        # balles → objet (météore détruit triangle par triangle, minerai détruit)
        for b in self.bullets:
            if b.get("life", 1) <= 0:
                continue
            for o in self.objects:
                if o.get("life", 1) <= 0:
                    continue
                dx, dy = wrapped_dxdy(b["x"], b["y"], o["x"], o["y"])
                if math.hypot(dx, dy) > BULLET_RADIUS + o["radius"]:
                    continue
                b["life"] = 0
                if o["kind"] == "minerai":
                    o["life"] = 0
                elif o["kind"] == "meteore":
                    o["life"] -= 1
                    if o["life"] <= 0:
                        self._destroy_meteor(o)
                break
        # vaisseau → minerai (collecte) puis météore (destruction)
        s = self.ship
        for o in self.objects:
            if o.get("life", 1) <= 0:
                continue
            dx, dy = wrapped_dxdy(s["x"], s["y"], o["x"], o["y"])
            reach = SHIP_RADIUS + self._collision_radius(o)
            if o["kind"] == "minerai":
                reach += PICKUP_MARGIN  # tolérance de ramassage (voir constante)
            if math.hypot(dx, dy) > reach:
                continue
            if o["kind"] == "minerai" and not self.docked:
                if self.cargo_qty < self.cargo_cap:
                    self.cargo_qty += 1
                    self.cargo_elements.append(o.get("element", 1))
                    o["life"] = 0
            elif o["kind"] == "meteore" and not self.docked:
                # contact avec un météore : vaisseau détruit (`game.rs`)
                self.done = True
                self.outcome = OUTCOME_DESTROYED
                self.entry_speed = self.speed()
                return
        self.objects = [o for o in self.objects if o.get("life", 1) > 0]

    def _destroy_meteor(self, o: dict[str, Any]) -> None:
        """Météore détruit : libère ses minerais à sa position avec dispersion
        (`release_meteor_minerals`)."""
        self.meteors_destroyed += 1
        for _ in range(int(o.get("minerals", 0))):
            offset_x = (self.rng.random() - 0.5) * 2.0 * MINERAL_SPAWN_SPREAD
            offset_y = (self.rng.random() - 0.5) * 2.0 * MINERAL_SPAWN_SPREAD
            velocity = MINERAL_SCATTER_SPEED * (2.0 * self.rng.random() - 1.0)
            self.objects.append({
                "kind": "minerai",
                "x": (o["x"] + offset_x) % WORLD_W,
                "y": (o["y"] + offset_y) % WORLD_H,
                "direction": self.rng.random() * TAU,
                "velocity": velocity,
                "orientation": 0.0,
                "rotation": 0.0,
                "radius": MINERAL_RADIUS,
                "life": 1,
                "element": 1 + int(self.rng.random() * 3.0),
            })
        o["minerals"] = 0

    def _docking(self) -> None:
        if self.done:
            return
        s = self.ship
        dist = self.station_dist()
        # désarmé tant que le vaisseau n'a pas quitté la base (il vient de
        # décoller) : l'accostage ne se redéclenche qu'au **retour**
        if not self.dock_armed:
            if dist > 200.0:
                self.dock_armed = True
            return
        if not self.docked:
            # vitesse **par frame** comparée à `STATION_DOCK_SPEED` (0,5 u/frame
            # = 30 u/s) - dans la zone, presque immobile
            if dist < STATION_DOCK_DISTANCE and self.ship["velocity"] < STATION_DOCK_SPEED:
                self.docked = True
                self._unload()
            return
        # à quai : le vaisseau est tenu au centre de la station
        s["x"], s["y"] = 0.0, 0.0
        s["velocity"] = 0.0
        s["direction"] = 0.0

    def _unload(self) -> None:
        """Déchargement + ravitaillement à l'accostage : la soute devient des
        crédits, le carburant/munitions se rachètent si payable. Une soute non
        vide déchargée **termine** l'épisode (`delivered`)."""
        if self.cargo_qty > 0:
            self.credits += sum(ELEMENT_VALUES.get(e, 1) for e in self.cargo_elements)
            self.cargo_qty = 0
            self.cargo_elements = []
            self.deliveries += 1
            # vitesse d'entrée (u/s) : < 30 (0,5 u/frame) puisque l'accostage
            # exige d'être presque immobile
            self.entry_speed = self.speed()
            self.done = True
            self.outcome = OUTCOME_DELIVERED
            return
        # soute vide : ravitaillement (carburant puis munitions) si payable
        if not self.economy:
            return
        if self.credits >= 1 and self.fuel < self.fuel_cap:
            packs = math.ceil((self.fuel_cap - self.fuel) / FUEL_STEP)
            if packs * FUEL_PRICE <= self.credits:
                self.credits -= packs * FUEL_PRICE
                self.fuel = self.fuel_cap
        if self.credits >= 1 and self.ammo < self.ammo_cap:
            packs = math.ceil((self.ammo_cap - self.ammo) / AMMO_STEP)
            if packs * AMMO_PRICE <= self.credits:
                self.credits -= packs * AMMO_PRICE
                self.ammo = self.ammo_cap


def episode_outcome(env: ShipSim) -> dict[str, Any]:
    """Dénouement d'un épisode du simulateur, au format attendu par
    `episode_reward` (mêmes règles que le jeu / l'entraîneur)."""
    success = env.outcome in (OUTCOME_DELIVERED, "objectives_complete", "eva_recovered")
    return {
        "success": success,
        "outcome": env.outcome,
        "seconds": env.t,
        "entry_speed": env.entry_speed if success else 0.0,
        "final_dist": env.station_dist(),
        "objective_bonus": 0.0,
    }
