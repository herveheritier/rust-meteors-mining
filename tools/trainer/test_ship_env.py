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
from ship_autopilot_ref import autopilot_ship_inputs
from ship_env import (
    DOCK_ANIMATION_DURATION,
    DOCK_RETRACT_DURATION,
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
    load_real_fields,
)
from ship_warmstart import (
    cap_dataset,
    default_ship_perturbation,
    perturbed_ship_starts,
    ship_dagger_transitions,
    ship_expert_transitions,
    sim_comparison_ship,
)
from validate_ship_env import (AGREEMENT_THRESHOLD, DEFAULT_POLICY, agree,
                               representativity, sim_episode, succeeded)

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures",
                       "ship_physics_windows.json")


def _in_flight(env: ShipSim) -> None:
    """Place le vaisseau **en vol libre** (le jeu le démarre toujours à quai,
    liens attachés) : rien en cours au titre de l'accostage, et le pilote
    n'est pas « à la station »."""
    env.dock_links = False
    env.dock_retract = 0.0
    env.dock_anim = 0.0
    env.dock_box = False
    env.player_at_station = 0
    env.done = False


def _set_ship(env: ShipSim, state: dict[str, float], moving_mode: int) -> None:
    """Injecte un état de vaisseau (issu d'une observation réelle) dans le
    simulateur, pour rejouer une action connue.

    Le fixture ne porte pas l'état d'accostage de la fenêtre : on pose le
    pilote **à la station** (`player_at_station = -1`), le seul réglage qui ne
    déclenche pas d'animation d'arrivée depuis un état injecté - une fenêtre où
    le vaisseau est **tenu** (départ) se rejoue alors comme le jeu, où l'action
    du pilote est ignorée (rotation sans poussée, position et vitesse
    inchangées)."""
    env.ship.update({
        "x": state["x"], "y": state["y"],
        "direction": state["direction"],
        "orientation": state["orientation"],
        "rotation": state["rotation"],
        "velocity": state["speed"] / FRAMES_PER_SECOND,
    })
    env.moving_mode = moving_mode
    _in_flight(env)
    env.player_at_station = -1


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

    def _free_flying(self, **kwargs) -> ShipSim:
        """Vaisseau libre **hors de la base** (hors du cercle d'accostage)."""
        env = ShipSim(**kwargs)
        env.reset(1, 0.0, 0.0)
        _in_flight(env)
        env.ship["x"], env.ship["y"] = 200.0, 0.0
        return env

    def test_inertial_thrust_matches_thrust_vector(self) -> None:
        env = self._free_flying(moving_mode=MOVING_MODE_INERTIAL)
        # une frame de poussée vers l'est
        env.step(up=True)
        self.assertAlmostEqual(env.ship["velocity"], PLAYER_ACCELERATION, places=12)
        self.assertAlmostEqual(env.ship["direction"], 0.0, places=12)

    def test_directional_rotation_sets_orientation_opposite_to_direction(self) -> None:
        env = self._free_flying(moving_mode=MOVING_MODE_DIRECTIONAL)
        env.step(right=True)
        self.assertAlmostEqual(env.ship["orientation"], -env.ship["direction"], places=12)

    def test_realistic_rotation_has_inertia(self) -> None:
        env = self._free_flying(moving_mode=MOVING_MODE_REALISTIC)
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

    def _flying_env(self, **kwargs) -> ShipSim:
        """Épisode dont le vaisseau est déjà **libre et hors de la base** (le
        jeu le démarre à quai : ces tests portent sur la boucle de jeu en vol,
        pas sur le départ, et rester dans le cercle d'accostage déclencherait
        l'animation d'accostage)."""
        env = self._docked_env(**kwargs)
        _in_flight(env)
        env.ship["x"], env.ship["y"] = 200.0, 0.0
        return env

    def test_fire_consumes_one_ammo_and_respects_cooldown(self) -> None:
        env = self._flying_env()
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
            "kind": "meteore", "x": 260.0, "y": 0.0, "direction": 0.0,
            "velocity": 0.0, "orientation": 0.0, "rotation": 0.0,
            "radius": 20.0, "life": 2, "minerals": 2,
        }]
        env.ship.update({"x": 200.0, "y": 0.0, "orientation": 0.0,
                         "direction": 0.0, "velocity": 0.0})
        _in_flight(env)
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
        _in_flight(env)
        env.step()  # la collecte se fait au contact
        self.assertEqual(env.cargo_qty, 1)
        self.assertEqual([o for o in env.objects if o["kind"] == "minerai"], [])

    def test_fuel_drains_while_thrusting(self) -> None:
        env = self._flying_env()
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
        _in_flight(fast)
        fast.ship["velocity"] = 0.6
        fast._docking()
        self.assertFalse(fast.docked)
        self.assertEqual(fast.player_at_station, 0, "trop rapide : pilote libre")

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
        _in_flight(env)
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


class TestDockingSequence(unittest.TestCase):
    """Départ et accostage **tels que le jeu les enchaîne** (`docking.rs`,
    `game.rs::update`) : liens attachés, rétraction de 1,5 s (entrées
    ignorées), animation de 3 s, boîte DOCK STATION, puis livraison."""

    def test_departure_holds_the_ship_while_the_links_retract(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        self.assertTrue(env.dock_links, "à quai au lancement")
        env.step(up=True, fire=True)
        # la commande de mouvement a détaché les liens : rétraction en cours,
        # vaisseau figé au centre et **entrées ignorées** (tir compris)
        self.assertTrue(env.docked)
        self.assertAlmostEqual(env.dock_retract, DOCK_RETRACT_DURATION - 1.0 / FRAMES_PER_SECOND,
                               places=12)
        self.assertEqual((env.ship["x"], env.ship["y"]), (0.0, 0.0))
        self.assertEqual(env.ship["velocity"], 0.0)
        self.assertEqual(env.ship["orientation"], 0.0)
        self.assertEqual(env.ammo, ECONOMY_START_AMMO,
                         "le tir est ignoré pendant la rétraction (le jeu sort de `update`)")
        self.assertEqual([b for b in env.bullets if b.get("life", 1) > 0], [])

    def test_retraction_lasts_one_and_a_half_seconds_then_the_ship_is_free(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        expected = DOCK_RETRACT_DURATION * FRAMES_PER_SECOND  # 90 frames
        free_at = None
        for i in range(1, int(expected) + 10):
            env.step(up=True)
            if env.ship["velocity"] > 0.0:
                free_at = i
                break
        self.assertIsNotNone(free_at, "le vaisseau finit par être libre")
        # la rétraction dure 1,5 s : la première frame de poussée suit (±1 frame
        # d'arrondi de l'horloge de rétraction)
        self.assertAlmostEqual(free_at, expected + 1.0, delta=1.5)
        self.assertGreater(free_at, expected - 2.0)

    def test_firing_at_the_dock_without_moving_is_allowed(self) -> None:
        """Les liens attachés n'interdisent pas le tir tant qu'on ne demande pas
        à partir : le jeu tire à quai (mesuré : 5 munitions en 2 s)."""
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        env.step(fire=True)
        self.assertEqual(env.ammo, ECONOMY_START_AMMO - 1)
        self.assertTrue(env.dock_links, "toujours à quai")

    def test_arrival_starts_the_dock_animation_then_the_box_delivers(self) -> None:
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        _in_flight(env)
        env.cargo_qty = 3
        env.cargo_elements = [1, 2, 3]
        env.ship.update({"x": 0.0, "y": 0.0, "velocity": 0.0, "rotation": 0.0})
        env.step()
        self.assertEqual(env.dock_anim, DOCK_ANIMATION_DURATION,
                         "l'animation démarre à l'arrivée, pour 3 s")
        self.assertFalse(env.done, "la livraison est datée à la boîte, pas à l'arrivée")
        frames = round(DOCK_ANIMATION_DURATION * FRAMES_PER_SECOND)
        for _ in range(frames + 3):
            env.step()
            if env.dock_box:
                break
        self.assertTrue(env.dock_box, "la boîte s'ouvre à la fin de l'animation")
        self.assertAlmostEqual(env.t, DOCK_ANIMATION_DURATION,
                               delta=2.0 / FRAMES_PER_SECOND,
                               msg="l'arrivée à la livraison prend l'animation (3 s)")
        env.step()
        self.assertTrue(env.done)
        self.assertEqual(env.outcome, OUTCOME_DELIVERED)
        self.assertEqual(env.entry_speed, 0.0, "vaisseau immobile pendant l'animation")
        self.assertGreaterEqual(env.t, DOCK_ANIMATION_DURATION)

    def test_departure_adds_the_retraction_to_the_episode_clock(self) -> None:
        """Le délai de la rétraction compte dans l'horloge de l'épisode : la
        même trajectoire dure 1,5 s de plus qu'avec l'ancien modèle (départ
        immédiat), d'où une récompense plus basse - la mesure hors ligne doit
        la compter comme le jeu."""
        env = ShipSim()
        env.reset(1, 0.0, 0.0)
        pinned = 0
        for _ in range(120):
            env.step(up=True)
            if env.ship["x"] == 0.0 and env.ship["y"] == 0.0:
                pinned += 1  # vaisseau figé au centre : frame de rétraction
        self.assertAlmostEqual(env.t, 2.0, places=12)
        self.assertAlmostEqual(pinned, DOCK_RETRACT_DURATION * FRAMES_PER_SECOND, delta=2,
                               msg="la rétraction occupe 1,5 s de l'horloge de l'épisode")
        self.assertAlmostEqual(env.ship["velocity"], (120 - pinned) * PLAYER_ACCELERATION,
                               places=9,
                               msg="aucune poussée pendant la rétraction, une par frame libre")


FIELD_FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures",
                             "ship_mining_fields.json")


@unittest.skipUnless(os.path.exists(FIELD_FIXTURE),
                     "fixture des champs réels absent "
                     "(`measure_in_game.py --fields`)")
class TestRepresentativity(unittest.TestCase):
    """Le simulateur rejoue le **jeu** : même champ minier par graine (importé
    du fixture, pas synthétisé) et **même classement des cerveaux**.

    C'est le verrou du chantier « rendre le micro-simulateur représentatif » :
    avant, le champ était synthétisé (un autre monde pour la même graine) et le
    simulateur pouvait désigner le mauvais réseau. La mesure complète (temps,
    accord graine par graine, verdict) est `validate_ship_env.py`.
    """

    @classmethod
    def setUpClass(cls) -> None:
        with open(FIELD_FIXTURE, encoding="utf-8") as f:
            cls.fixture = json.load(f)
        cls.seeds = sorted(int(k) for k in cls.fixture.get("seeds", {}))
        cls.fields = load_real_fields(FIELD_FIXTURE)
        # Une seule mesure pour toute la classe (12 graines × 2 cerveaux, ~30 s) :
        # c'est elle qui porte le verdict hors ligne, comparée au jeu.
        cls.report = representativity(FIELD_FIXTURE, DEFAULT_POLICY, timeout=150.0)

    def test_recorded_seeds_seed_the_games_own_mining_field(self) -> None:
        """Le monde du simulateur est **celui de la graine** : positions, rayons
        et triangles vivants des météores enregistrés."""
        self.assertTrue(self.seeds, "fixture vide")
        for seed in self.seeds:
            env = ShipSim()
            env.reset(seed, 0.0, 0.0)
            got = sorted((round(o["x"], 9), round(o["y"], 9),
                          round(o["radius"], 9), o["life"]) for o in env.objects)
            want = sorted((round(m["x"], 9), round(m["y"], 9),
                           round(m["radius"], 9), m["life"])
                          for m in self.fields[seed])
            self.assertEqual(got, want, f"graine {seed} : champ synthétisé au lieu du réel")

    def test_scripted_law_replays_the_games_outcomes(self) -> None:
        """Sur les graines enregistrées, la loi scriptée portée (fidèle à 100 %)
        doit tomber du **même côté que le jeu** (livré / non livré) presque
        partout - c'est ce qui rend la mesure hors ligne utilisable."""
        verdict = self.report["verdict"].get("scripté")
        self.assertIsNotNone(verdict, "aucune graine de référence dans le fixture")
        self.assertGreaterEqual(verdict["judged"], 10, "graines de référence insuffisantes")
        self.assertGreaterEqual(
            verdict["rate"], AGREEMENT_THRESHOLD,
            f"accord avec le jeu : {verdict['agree']}/{verdict['judged']} graines seulement")

    def test_the_offline_ranking_follows_the_game(self) -> None:
        """Le **classement** hors ligne ne doit pas **inverser** celui du jeu :
        si la partie préfère nettement un cerveau, le simulateur ne peut pas
        préférer l'autre - c'est tout l'objet de la mesure (sélectionner sans
        lancer le jeu).

        C'est la propriété que le chantier « représentatif » est venu corriger :
        avec un champ minier **synthétisé** et un départ immédiat, le
        micro-simulateur désignait le mauvais réseau (dit autrement : l'écart de
        récompense hors ligne était anti-corrélé au résultat réel).
        """
        ref = self.report["verdict"].get("scripté")
        pol = self.report["verdict"].get("appris")
        self.assertIsNotNone(ref, "aucune graine de référence dans le fixture")
        self.assertIsNotNone(pol, "aucune graine mesurée pour le réseau embarqué")
        self.assertGreater(pol["compared"], 0, "réseau embarqué jamais mesuré par le jeu")
        margin = ref["game_delivered"] - pol["game_delivered"]
        if margin == 0:
            self.skipTest("la paire de cerveaux enregistrée n'est pas discriminante")
        self.assertGreaterEqual(
            ref["sim_delivered"], pol["sim_delivered"],
            f"le simulateur classe **dernier** le cerveau que le jeu classe devant "
            f"(jeu {ref['game_delivered']} scripté vs {pol['game_delivered']} appris · "
            f"simulateur {ref['sim_delivered']} vs {pol['sim_delivered']})")
        self.assertLess(
            pol["sim_delivered"] - ref["sim_delivered"], 2,
            "le simulateur préfère **nettement** le cerveau que le jeu classe dernier")

    def test_the_remaining_gap_is_the_hybrid_frontier(self) -> None:
        """Le résidu est **mesuré et nommé**, pas caché : sur cette paire, les
        graines d'écart sont des **destructions** et des **délais**.

        Le simulateur est plus **clément** que la partie (météores en cercles,
        tolérance de ramassage) : il livre des graines que le jeu perd en
        collision. La mesure le dit - l'accord du cerveau **appris** peut rester
        sous le seuil et le rapport doit alors **avertir** au lieu de laisser
        croire à une sélection valide.
        """
        pol = self.report["verdict"].get("appris")
        self.assertIsNotNone(pol)
        if pol["representative"]:
            self.skipTest("le cerveau appris est rejoué fidèlement : rien à nommer")
        self.assertLess(pol["rate"], AGREEMENT_THRESHOLD)
        wrecked = [r["seed"] for r in self.report["rows"]
                   if (r["game_learned"] or {}).get("outcome") == OUTCOME_DESTROYED
                   and succeeded(r["sim_learned"])]
        self.assertTrue(wrecked, "aucune graine où le jeu détruit et le simulateur livre")
        self.assertEqual(self.report["verdict"]["missing_field"], [],
                         "des graines sont mesurées sur un monde synthétisé")


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


def _active(label: list[float]) -> bool:
    """Le pas est-il **actif** (poussée, frein, tir ou rotation) ? Le vecteur
    cible est `3 sigmoïdes + un one-hot de rotation (left, right, none)`."""
    return any(v >= 0.5 for v in label[:3]) or label[3] >= 0.5 or label[4] >= 0.5


class TestDagger(unittest.TestCase):
    """Itérations DAgger **dans le simulateur** : la politique joue l'épisode,
    l'expert étiquette les états qu'elle visite réellement.

    C'est le remède à la **dérive de distribution** du clonage pur : la
    politique n'a aucun label pour sortir des états où elle se retrouve seule
    (mesuré : elle se gare à ~400 u de la station, soute vide, et n'en sort
    plus - la tête de rotation choisit « none » à 1.0 de confiance alors que
    l'expert tourne).
    """

    @staticmethod
    def _idle(_obs: dict) -> dict[str, bool]:
        """Politique témoin qui ne touche à rien."""
        return {"up": False, "down": False, "left": False, "right": False,
                "fire": False}

    def test_states_are_labelled_by_the_expert_not_by_the_policy(self) -> None:
        """Les transitions portent l'action de l'**expert**, pas celle de la
        politique : sinon le DAgger ne serait qu'une imitation de soi-même."""
        from nn import feature_size

        starts = perturbed_ship_starts([1], [600.0], random.Random(0),
                                       **default_ship_perturbation(600.0, 1.0))
        out = ship_dagger_transitions(self._idle, starts, timeout=20.0, stride=5)
        self.assertIn(1, out, "les transitions sont agrégées par graine")
        rows = out[1]
        self.assertTrue(rows)
        x, y = rows[0]
        self.assertEqual(len(x), feature_size())
        self.assertEqual(len(y), 6)
        # la politique ne fait rien : si les étiquettes étaient les siennes,
        # elles seraient toutes inactives. L'expert, lui, corrige.
        self.assertTrue(any(_active(y) for _, y in rows),
                        "aucune étiquette active : ce ne sont pas les actions de l'expert")

    def test_dagger_visits_states_the_expert_never_shows(self) -> None:
        """La politique qui ne fait rien dérive hors de la distribution de
        l'expert : le DAgger doit précisément enregistrer ces états-là (c'est
        la dérive de distribution que le clonage pur ne sait pas corriger)."""
        starts = perturbed_ship_starts([1], [600.0], random.Random(0),
                                       **default_ship_perturbation(600.0, 1.0))
        dagger = ship_dagger_transitions(self._idle, starts, timeout=20.0, stride=20)
        expert = ship_expert_transitions([1], [600.0], random.Random(0), 20.0,
                                         stride=20,
                                         **default_ship_perturbation(600.0, 1.0))
        seen = [x for x, _ in expert]
        self.assertTrue(seen and dagger[1], "jeux vides")
        # état de la politique **loin** de tout état de l'expert : on mesure la
        # plus petite distance L∞ aux états de l'expert (échantillonné pour
        # garder le test rapide en Python pur).
        sample = dagger[1][::max(1, len(dagger[1]) // 40)]
        reference = seen[::max(1, len(seen) // 40)]
        farthest = max(min(max(abs(a - b) for a, b in zip(x, r))
                           for r in reference) for x, _ in sample)
        self.assertGreater(farthest, 0.05,
                           "la politique ne quitte jamais la distribution de l'expert "
                           "- le DAgger n'apporterait rien")

    def test_stall_cutoff_ends_an_idle_episode(self) -> None:
        """Un roulage DAgger où la politique ne commande **rien** est le point
        fixe de la trappe : le rejouer des milliers de pas ne produit aucun
        état nouveau et coûte ~1,3 ms le pas. Le coupe-circuit (`stall_steps`)
        doit abréger l'épisode."""
        starts = perturbed_ship_starts([1], [600.0], random.Random(0),
                                       **default_ship_perturbation(600.0, 1.0))
        full = ship_dagger_transitions(self._idle, starts, timeout=30.0, stride=5)
        cut = ship_dagger_transitions(self._idle, starts, timeout=30.0, stride=5,
                                      stall_steps=100)
        self.assertIn(1, cut, "transitions agrégées par graine")
        self.assertLess(len(cut[1]), len(full[1]),
                        "l'épisode figé n'a pas été coupé")
        # 100 pas d'inaction, sous-échantillonnés au pas 5 → 20 transitions
        self.assertLessEqual(len(cut[1]) // len(starts), 21)

    def test_stall_cutoff_spares_a_policy_that_acts(self) -> None:
        """Seule l'inaction **totale** est le signal de calage : une politique
        qui commande quelque chose ne doit pas être tronquée."""
        starts = perturbed_ship_starts([1], [600.0], random.Random(0),
                                       **default_ship_perturbation(600.0, 1.0))

        def always_up(_obs: dict) -> dict[str, bool]:
            return {"up": True, "down": False, "left": False, "right": False,
                    "fire": False}

        full = ship_dagger_transitions(always_up, starts, timeout=20.0, stride=5)
        cut = ship_dagger_transitions(always_up, starts, timeout=20.0, stride=5,
                                      stall_steps=100)
        self.assertEqual(len(cut[1]), len(full[1]),
                         "une politique qui commande a été coupée à tort")

    def test_cap_dataset_truncates_in_lockstep(self) -> None:
        rng = random.Random(0)
        x, y = cap_dataset([[1.0]] * 10, [[0.0]] * 10, 4, rng)
        self.assertEqual(len(x), 4)
        self.assertEqual(len(y), 4)
        x2, y2 = cap_dataset([[1.0], [2.0]], [[0.0], [1.0]], 10, rng)
        self.assertEqual(x2, [[1.0], [2.0]], "sous le plafond : jeu inchangé")
        self.assertEqual(len(y2), 2)


if __name__ == "__main__":
    unittest.main()
