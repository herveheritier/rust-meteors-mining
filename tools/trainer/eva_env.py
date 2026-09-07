#!/usr/bin/env python3
"""Environnement EVA pour l'auto-entraînement : géométrie partagée entre le
jeu réel et le **micro-simulateur** (mêmes conventions que `src/driver.rs`,
`src/autopilot.rs`, `src/input.rs` et `src/docking.rs`).

Le simulateur reproduit **exactement** les formules du jeu (voir `PORTAGE.md`
§6 et le code) pour que les politiques et les récompenses soient
transférables de la simulation vers la partie réelle :

- la vitesse est stockée **par frame** (`v`), la position bouge de
  `x += cos(direction)·v`, `y −= sin(direction)·v` à chaque pas de 1/60 s
  (`moving_shape`) - l'observation rapporte des unités/s (×60) ;
- `right` ajoute `PLAYER_ROTATION_SPEED` à l'orientation, `left` la retranche
  (le sens des touches du cosmonaute, `cosmonaut_apply_inputs`) ;
- la poussée (↑) combine le vecteur vitesse courant avec l'accélération le
  long de l'orientation (`thrust_vector` avec `sy = −1` : le déplacement
  vertical est `−sin`, les deux signes se compensent) ;
- la **récupération** se déclenche dès que le cosmonaute entre dans le cercle
  de rayon `STATION_DOCK_DISTANCE` (15) autour du centre de la station - ni
  la vitesse ni l'orientation n'y changent rien, comme dans le jeu.

Convention angulaire (écran, y vers le bas) : 0 = vers l'est, et la poussée
dans une direction `a` fait avancer selon `(cos a, −sin a)` - c'est la
convention de l'observation (`direction`, `orientation`) et de l'autopilote
(`screen_angle_to` = `atan2(dy, dx)` sur le delta torique vers la cible).
"""

from __future__ import annotations

import math
import random
from typing import Any, Optional

# ── constantes du jeu (src/config.rs, src/input.rs) ─────────────────────────
TAU = 2.0 * math.pi
PLAYER_ACCELERATION = 0.05          # unités/frame (×60 = unités/s²)
PLAYER_ROTATION_SPEED = TAU / 210.0  # rad/frame (×60 = rad/s)
STATION_DOCK_DISTANCE = 15.0        # déclenche la récupération du cosmonaute

# monde torique (src/config.rs) ; la station est au centre (W/2, H/2) mais le
# simulateur la pose en (0, 0) - seuls les deltas comptent, ils sont toriques
WORLD_W = 3960.0
WORLD_H = 3540.0

FRAMES_PER_SECOND = 60.0


def wrap_angle(a: float) -> float:
    """Ramène un angle dans ]−π, π]."""
    return (a + math.pi) % TAU - math.pi


def wrapped_dxdy(x1: float, y1: float, x2: float, y2: float) -> tuple[float, float]:
    """Delta torique le plus court du point 1 vers le point 2 (mêmes axes que
    le monde du jeu : le delta « pilote → station » de l'observation)."""
    dx = x2 - x1
    dy = y2 - y1
    dx -= WORLD_W * round(dx / WORLD_W)
    dy -= WORLD_H * round(dy / WORLD_H)
    return dx, dy


def eva_aim(obs: dict[str, Any]) -> float:
    """Angle écran (convention du jeu) de la station depuis le pilote :
    l'orientation du cosmonaute doit s'y aligner pour pousser vers elle.
    Réutilise directement les deltas toriques de l'observation
    (`station_dx/dy`) - même formule que `screen_angle_to` du jeu."""
    return math.atan2(obs["station_dy"], obs["station_dx"])


def eva_speed_along(obs: dict[str, Any], aim: float) -> float:
    """Vitesse d'approche réelle (unités/s) projetée sur la direction de la
    station - positive = on s'en rapproche (cf. `autopilot_eva_inputs`)."""
    vx = obs["eva"]["vx"]
    vy = obs["eva"]["vy"]
    return vx * math.cos(aim) + vy * math.sin(aim)


class EvaSim:
    """Micro-simulateur du cosmonaute EVA (mêmes formules que le jeu).
    Produit des observations au **même format JSON** que le jeu (`/obs`),
    pour que politiques et entraîneur soient interchangeables entre la
    simulation et la partie réelle."""

    def __init__(self, seed: int = 0) -> None:
        self.rng = random.Random(seed)
        self.dt = 1.0 / FRAMES_PER_SECOND
        self.frame = 0
        self.t = 0.0
        self.x = 0.0
        self.y = 0.0
        self.orientation = 0.0
        self.dir = 0.0     # direction de la vitesse (radians, convention jeu)
        self.v = 0.0       # vitesse par frame (×60 = unités/s)
        self.done = False  # récupéré (entré dans le cercle d'accostage)
        self.entry_speed = 0.0  # vitesse à la récupération (unités/s)

    def reset(self, seed: int, x: float, y: float, orientation: float = 0.0) -> dict[str, Any]:
        """Départ d'épisode : même situation que `reset_episode` du jeu
        (`activate_cosmonaut` : position du crash, orientation 0, immobile)."""
        self.rng = random.Random(seed)
        self.frame = 0
        self.t = 0.0
        self.x = x % WORLD_W
        self.y = y % WORLD_H
        self.orientation = orientation
        self.dir = 0.0
        self.v = 0.0
        self.done = False
        self.entry_speed = 0.0
        return self.obs()

    def step(self, up: bool, right: bool, left: bool) -> dict[str, Any]:
        """Un pas de physique (1/60 s) - mêmes formules que le jeu
        (`cosmonaut_apply_inputs` + `moving_shape`)."""
        dt = self.dt
        # rotation ←/→ du cosmonaute (sens des touches du jeu)
        if right:
            self.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt
        if left:
            self.orientation -= PLAYER_ROTATION_SPEED * 60.0 * dt
        # poussée vectorielle le long de l'orientation (`thrust_vector`, sy=-1)
        if up:
            acc = PLAYER_ACCELERATION * 60.0 * dt
            dx = math.cos(self.dir) * self.v + math.cos(self.orientation) * acc
            dy = math.sin(self.dir) * self.v - math.sin(self.orientation) * acc
            self.dir = math.atan2(dy, dx)
            self.v = math.hypot(dx, dy)
        # déplacement (`moving_shape`) + monde torique
        self.x = (self.x + math.cos(self.dir) * 60.0 * self.v * dt) % WORLD_W
        self.y = (self.y - math.sin(self.dir) * 60.0 * self.v * dt) % WORLD_H
        self.frame += 1
        self.t += dt
        # récupération : entrée dans le cercle d'accostage au centre
        if not self.done and self.station_dist() < STATION_DOCK_DISTANCE:
            self.done = True
            self.entry_speed = self.v * 60.0
        return self.obs()

    def station_dist(self) -> float:
        dx, dy = wrapped_dxdy(self.x, self.y, 0.0, 0.0)
        return math.hypot(dx, dy)

    # ── observation au format `/obs` du jeu ─────────────────────────────────
    def obs(self) -> dict[str, Any]:
        speed = self.v * 60.0
        dx, dy = wrapped_dxdy(self.x, self.y, 0.0, 0.0)
        return {
            "frame": self.frame,
            "t": self.t,
            "pilot": "vaisseau" if self.done else "eva",
            "ship": {"x": 0.0, "y": 0.0, "vx": 0.0, "vy": 0.0, "speed": 0.0,
                     "direction": 0.0, "orientation": 0.0, "rotation": 0.0},
            "eva": {
                "x": self.x,
                "y": self.y,
                "vx": math.cos(self.dir) * speed,
                "vy": -math.sin(self.dir) * speed,
                "speed": speed,
                "direction": self.dir,
                "orientation": self.orientation,
                "rotation": 0.0,
            },
            "eva_active": not self.done,
            "station_x": 0.0,
            "station_y": 0.0,
            "station_radius": 162.0,
            "station_dx": dx,
            "station_dy": dy,
            "station_dist": math.hypot(dx, dy),
            "docked": self.done,
            "dock_anim": 0.0,
            "dock_retract": 0.0,
            "dock_box": False,
            "eva_recovery": 0.0,
            "eva_crossfade": 0.0,
            "paused": False,
            "game_over": False,
            "autopilot": False,
            "driver_engaged": False,
            "economy": False,
            "fuel": 0.0,
            "fuel_cap": 0.0,
            "ammo": 0,
            "ammo_cap": 0,
            "credits": 0,
            "cargo_qty": 0,
            "cargo_cap": 0,
            "meteors_destroyed": 0,
            "score": 0,
            "nearby": [],
        }


def spawn_position(seed: int, spawn_dist: float) -> tuple[float, float]:
    """Position de crash d'un épisode EVA : à `spawn_dist` unités du centre
    de la station (delta torique), direction tirée de la graine - déterministe
    et identique dans le simulateur et le vrai jeu (la graine y régénère le
    monde ET tire la direction du crash)."""
    rng = random.Random(seed)
    angle = rng.uniform(0.0, TAU)
    return (math.cos(angle) * spawn_dist) % WORLD_W, (math.sin(angle) * spawn_dist) % WORLD_H


def episode_reward(obs_first: Optional[dict[str, Any]], outcome: dict[str, Any]) -> float:
    """Récompense d'un épisode EVA : +1000 si le cosmonaute est récupéré
    (entré dans le cercle d'accostage), moins le temps passé, moins une
    pénalité quand il arrive trop vite (le retour doit rester contrôlé -
    c'est ce que cherche aussi l'autopilote du jeu)."""
    seconds = outcome.get("seconds", 0.0)
    if outcome.get("success"):
        entry = outcome.get("entry_speed", 0.0)
        overshoot = max(0.0, entry - 30.0) * 5.0
        return 1000.0 - 2.0 * seconds - overshoot
    # échec (délai dépassé) : pénalité croissante avec le temps perdu, plus
    # une prime de progression - rester proche de la base (même en orbite)
    # paie mieux que dériver au loin, pour guider l'entraînement vers
    # l'épisode réussi
    final_dist = outcome.get("final_dist", 0.0)
    return -2.0 * seconds - 50.0 - final_dist * 0.1
