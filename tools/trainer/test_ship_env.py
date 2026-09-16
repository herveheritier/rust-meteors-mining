#!/usr/bin/env python3
"""Tests hors-ligne du **micro-simulateur vaisseau** (`ship_env.py`) et de son
**amorce par départs perturbés** (`ship_warmstart.py`) - `python3 -m unittest`,
aucune dépendance, aucun processus de jeu.

Deux niveaux de fidélité sont verrouillés :

1. **Physique** : les fenêtres `fixtures/ship_physics_windows.json` sont un
   extrait d'une **vraie partie** (mode headless, autopilote) - chaque fenêtre
   porte l'état du vaisseau, l'action de l'autopilote et l'état suivant. Le
   simulateur doit reproduire **exactement** chaque pas (à la précision machine
   près). C'est la preuve que la cinématique est portée fidèlement, sans lancer
   le jeu ;
2. **Boucle de jeu** : tir (cadence, munitions), minage (un tir = un triangle,
   libération des minerais à la destruction), collecte, carburant, accostage et
   livraison - les règles de `game.rs` / `generate.rs` telles que le
   micro-simulateur les applique.

Les frontières **approchées** (rayon de collision des météores, tolérance de
ramassage, génération du champ synthétique) sont documentées dans `ship_env.py`
et ne sont pas testées comme des règles du jeu.
"""

from __future__ import annotations

import json
import math
import os
import random
import unittest

import ship_env
from ship_env import (
    ECONOMY_START_AMMO,
    ECONOMY_START_FUEL,
    FRAMES_PER_SECOND,
    MOVING_MODE_DIRECTIONAL,
    MOVING_MODE_INERTIAL,
    MOVING_MODE_REALISTIC,
    OUTCOME_DELIVERED,
    OUTCOME_DESTROYED,
    PLAYER_ACCELERATION,
    STATION_DOCK_DISTANCE,
    STATION_DOCK_SPEED,
    WORLD_H,
    WORLD_W,
    ShipSim,
    episode_outcome,
    episode_reward,
)
from ship_warmstart import (
    default_ship_perturbation,
    perturbed_ship_starts,
    ship_expert_transitions,
    sim_comparison_ship,
)

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures",
                       "ship_physics_windows.json")


def _set_ship(env: ShipSim, state: dict[str, float], moving_mode: int) -> None:
    """Injecte un état de vaisseau (issu d'une observation réelle) dans le
    simulateur, pour rejouer une action connue."""
    env.ship.update({
        "x": state["x"], "y": state["y"],
        "direction": state["direction"],
        "orientation": state["orientation"],
        "rotation": state["rotation"],
        "velocity": state["speed"] / FRAMES_PER_SECOND,
    })
    env.moving_mode = moving_mode
    env.docked = False
    env.dock_armed = False
    env.done = False


def _toroidal_distance(ax: float, ay: float, bx: float, by: float) -> float:
    dx = ax - bx
    dy = ay - by
    dx -= WORLD_W * round(dx / WORLD_W)
    dy -= WORLD_H * round(dy / WORLD_H)
    return math.hypot(dx, dy)


class TestPhysicsFidelity(unittest.TestCase):
    """La physique doit reproduire **exactement** la vraie partie."""

    def test_recorded_windows_are_reproduced(self) -> None:
        with open(FIXTURE, encoding="utf-8") as f:
            fixture = json.load(f)
        env = ShipSim()
        env.reset(0, 0.0, 0.0, 0.0, 0.0, 0.0)
        worst = 0.0
        for window in fixture["windows"]:
            _set_ship(env, window["state"], window["moving_mode"])
            a = window["action"]
            env.step(a["up"], a["down"], a["left"], a["right"], False)
            nxt = window["next"]
            worst = max(
                worst,
                _toroidal_distance(env.ship["x"], env.ship["y"], nxt["x"], nxt["y"]),
                abs(env.ship["velocity"] * FRAMES_PER_SECOND - nxt["speed"]),
            )
        self.assertLess(worst, 1e-9,
                        f"la physique diverge de la vraie partie (erreur {worst})")
        self.assertGreater(len(fixture["windows"]), 50)

    def test_inertial_thrust_matches_thrust_vector(self) -> None:
        env = ShipSim(moving_mode=MOVING_MODE_INERTIAL)
        env.reset(1, 0.0, 0.0, orientation=0.0)
        # une frame de poussée vers l'est
        env.step(up=True)
        self.assertAlmostEqual(env.ship["velocity"], PLAYER_ACCELERATION, places=12)
        self.assertAlmostEqual(env.ship["direction"], 0.0, places=12)

    def test_directional_rotation_sets_orientation_opposite_to_direction(self) -> None:
        env = ShipSim(moving_mode=MOVING_MODE_DIRECTIONAL)
        env.reset(1, 0.0, 0.0)
        env.step(right=True)
        self.assertAlmostEqual(env.ship["orientation"], -env.ship["direction"], places=12)

    def test_realistic_rotation_has_inertia(self) -> None:
        env = ShipSim(moving_mode=MOVING_MODE_REALISTIC)
        env.reset(1, 0.0, 0.0)
        env.step(right=True)          # accélère la rotation
        rotation_spinning = env.ship["rotation"]
        self.assertGreater(rotation_spinning, 0.0)
        env.step()                    # relâché : la rotation persiste
        self.assertAlmostEqual(env.ship["rotation"], rotation_spinning, places=12)

    def test_deterministic_episode(self) -> None:
        def rollout() -> list[float]:
            env = ShipSim(7)
            obs = env.reset(7, 0.0, 0.0)
            trace = []
            for _ in range(120):
                obs = env.step(up=True, right=True)
                trace.append(obs["ship"]["x"])
            return trace

        self.assertEqual(rollout(), rollout())


class TestGameLoop(unittest.TestCase):
    """Tir, minage, collecte, carburant et accostage."""

    def _docked_env(self, **kwargs) -> ShipSim:
        env = ShipSim(**kwargs)
        env.reset(1, 0.0, 0.0, 0.0, 0.0, 0.0)
        return env

    def test_fire_consumes_one_ammo_and_respects_cooldown(self) -> None:
        env = self._docked_env()
        env.step(up=True, fire=True)  # quitte le quai + tire
        bullets = len(env.bullets)
        ammo_after = env.ammo
        self.assertGreaterEqual(bullets, 1)
        self.assertEqual(ammo_after, ECONOMY_START_AMMO - 1)
        # immédiatement après : cooldown actif, pas de nouveau tir
        env.step(up=True, fire=True)
        self.assertEqual(len([b for b in env.bullets if b.get("life", 1) > 0]), bullets)
        self.assertEqual(env.ammo, ammo_after)

    def test_bullet_destroys_meteor_and_releases_minerals(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        # place un météore de 2 triangles juste devant, immobile
        env.objects = [{
            "kind": "meteore", "x": 60.0, "y": 0.0, "direction": 0.0,
            "velocity": 0.0, "orientation": 0.0, "rotation": 0.0,
            "radius": 20.0, "life": 2, "minerals": 2,
        }]
        env.ship.update({"x": 0.0, "y": 0.0, "orientation": 0.0,
                         "direction": 0.0, "velocity": 0.0})
        env.docked = False
        env.dock_armed = False
        for _ in range(200):
            env.step(fire=True)
            if env.meteors_destroyed:
                break
        self.assertEqual(env.meteors_destroyed, 1)
        minerals = [o for o in env.objects if o["kind"] == "minerai"]
        self.assertEqual(len(minerals), 2, "un minerai par unité de `minerals`")

    def test_mineral_is_collected_into_cargo(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        env.objects = [{
            "kind": "minerai", "x": 5.0, "y": 0.0, "direction": 0.0,
            "velocity": 0.0, "orientation": 0.0, "rotation": 0.0,
            "radius": 10.0, "life": 1, "element": 1,
        }]
        env.ship.update({"x": 0.0, "y": 0.0, "direction": 0.0, "velocity": 0.0})
        env.docked = False
        env.dock_armed = False
        env.step()  # la collecte se fait au contact
        self.assertEqual(env.cargo_qty, 1)
        self.assertEqual([o for o in env.objects if o["kind"] == "minerai"], [])

    def test_fuel_drains_while_thrusting(self) -> None:
        env = self._docked_env()
        env.step(up=True)
        self.assertLess(env.fuel, ECONOMY_START_FUEL)

    def test_docking_uses_per_frame_speed_threshold(self) -> None:
        # dans la zone d'accostage, à 0,4 u/frame (< STATION_DOCK_SPEED) : accoste
        slow = ShipSim()
        slow.reset(1, 0.0, 0.0, 0.0, 0.0, 0.0, cargo=2)
        slow.step()  # au centre, immobile : livraison
        self.assertTrue(slow.docked)
        self.assertTrue(slow.done)
        self.assertEqual(slow.outcome, OUTCOME_DELIVERED)
        # à 0,6 u/frame (> STATION_DOCK_SPEED) : pas d'accostage
        fast = ShipSim()
        fast.reset(1, 0.0, 0.0, 0.0, 0.0, 0.0)
        fast.ship["velocity"] = 0.6
        fast.docked = False
        fast.dock_armed = True
        fast._docking()
        self.assertFalse(fast.docked)

    def test_delivery_credits_and_reward(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0, 0.0, 0.0, 0.0, cargo=5)
        env.step()
        self.assertEqual(env.outcome, OUTCOME_DELIVERED)
        self.assertEqual(env.cargo_qty, 0)
        self.assertGreater(env.credits, 0)
        outcome = episode_outcome(env)
        reward = episode_reward(None, outcome)
        self.assertGreater(reward, 900.0, "livraison rapide : récompense proche de +1000")

    def test_ship_is_destroyed_on_meteor_contact(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        env.objects = [{
            "kind": "meteore", "x": 1.0, "y": 0.0, "direction": 0.0,
            "velocity": 0.0, "orientation": 0.0, "rotation": 0.0,
            "radius": 20.0, "life": 5, "minerals": 5,
        }]
        env.ship.update({"x": 0.0, "y": 0.0, "direction": 0.0, "velocity": 0.0})
        env.docked = False
        env.dock_armed = False
        env.step()
        self.assertTrue(env.done)
        self.assertEqual(env.outcome, OUTCOME_DESTROYED)

    def test_observation_has_the_game_shape(self) -> None:
        env = ShipSim()
        obs = env.reset(1, 0.0, 0.0)
        for key in ("ship", "station_dx", "station_dy", "nearby", "bullets",
                    "fuel", "fuel_cap", "ammo", "ammo_cap", "cargo_qty",
                    "cargo_cap", "moving_mode", "economy", "supplies_affordable"):
            self.assertIn(key, obs)
        self.assertEqual(obs["pilot"], "vaisseau")
        self.assertEqual(len(obs["nearby"]), ship_env.MINING_FIELD_COUNT)


class TestWarmstart(unittest.TestCase):
    """Départs perturbés et amorce par imitation (cible ship)."""

    def test_nominal_starts_include_the_docked_departure(self) -> None:
        starts = perturbed_ship_starts([1, 2], [400.0], random.Random(0))
        self.assertIn((1, 0.0, 0.0, 0.0, 0.0, 0.0, 0, None, None, 0), starts)

    def test_zero_scale_is_the_old_nominal_start_set(self) -> None:
        perturb = default_ship_perturbation(400.0, 0.0)
        self.assertEqual(perturb["cargo_starts"], ())
        starts = perturbed_ship_starts([1], [400.0], random.Random(0), **perturb)
        # une graine × une distance, perturbation nulle = le seul départ nominal
        self.assertEqual(len(starts), 1)

    def test_perturbation_adds_starts(self) -> None:
        perturb = default_ship_perturbation(400.0, 1.0)
        starts = perturbed_ship_starts([1], [400.0], random.Random(0), **perturb)
        self.assertGreater(len(starts), 2)
        self.assertTrue(any(s[6] > 0 for s in starts), "soute entamée")
        self.assertTrue(any(s[7] is not None for s in starts), "réserves basses")

    def test_expert_transitions_have_the_network_feature_size(self) -> None:
        from nn import feature_size

        transitions = ship_expert_transitions(
            [1], [400.0], random.Random(0), timeout=20.0, stride=5,
            **default_ship_perturbation(400.0, 1.0))
        self.assertGreater(len(transitions), 0)
        x, y = transitions[0]
        self.assertEqual(len(x), feature_size())
        self.assertEqual(len(y), 6)  # 3 sigmoïdes + 3 classes de rotation

    def test_sim_comparison_returns_a_gap(self) -> None:
        from ship_autopilot_ref import autopilot_ship_inputs

        cmp = sim_comparison_ship(autopilot_ship_inputs, [1, 3], timeout=90.0)
        self.assertEqual(cmp["policy_ok"], cmp["reference_ok"])
        self.assertAlmostEqual(cmp["gap"], 0.0)
        self.assertGreater(len(cmp["policy"]), 0)

    def test_reference_delivers_in_the_simulator(self) -> None:
        """L'autopilote vaisseau porté doit boucler la boucle de minage dans le
        micro-simulateur sur au moins une graine - sinon l'environnement n'est
        pas un banc d'essai utilisable."""
        from ship_autopilot_ref import autopilot_ship_inputs

        cmp = sim_comparison_ship(autopilot_ship_inputs, [1, 3], timeout=90.0)
        self.assertGreaterEqual(cmp["reference_ok"], 1)
        self.assertGreater(cmp["reference_mean"], 0.0)


if __name__ == "__main__":
    unittest.main()
