#!/usr/bin/env python3
"""Tests de l'**algorithme génétique sélectif** (`ga.py`) : moteurs de
sélection/variation, fitness de la loi recâblée (génome A), format de sortie,
et non-régression du chargement dans `policies.py`.

    python3 -m unittest test_ga -v
"""

from __future__ import annotations

import json
import os
import random
import tempfile
import unittest
from typing import Any

import ga
import ship_autopilot_ref
from ship_env import ShipSim


def obs_min() -> dict[str, Any]:
    """Observation minimale vaisseau (le port de la loi doit y répondre)."""
    return {
        "pilot": "vaisseau",
        "ship": {"x": 0.0, "y": 0.0, "vx": 0.0, "vy": 0.0, "speed": 0.0,
                 "direction": 0.0, "orientation": 0.0, "rotation": 0.0,
                 "center_x": 0.0, "center_y": 0.0},
        "station_dx": 0.0, "station_dy": 0.0, "station_dist": 0.0,
        "moving_mode": 2, "economy": True, "fuel": 100.0, "fuel_cap": 100.0,
        "ammo": 30, "ammo_cap": 30, "credits": 0,
        "cargo_qty": 0, "cargo_cap": 5, "supplies_affordable": True,
        "nearby": [], "bullets": [],
    }


class TournamentTest(unittest.TestCase):
    def test_the_best_contender_wins(self) -> None:
        rng = random.Random(0)
        scores = [(1.0, "a"), (5.0, "b"), (3.0, "c")]
        for _ in range(20):
            _, winner = ga.tournament(scores, 3, rng)
            self.assertEqual(winner, "b", "le tournoi plein choisit le meilleur")

    def test_a_tournament_of_one_takes_a_random_contender(self) -> None:
        rng = random.Random(1)
        winners = {ga.tournament([(1.0, "a"), (2.0, "b")], 1, rng)[1] for _ in range(40)}
        self.assertEqual(winners, {"a", "b"}, "k=1 : tirage uniforme")


class CrossoverTest(unittest.TestCase):
    def test_every_gene_comes_from_a_parent(self) -> None:
        rng = random.Random(0)
        a = {k: float(i) for i, k in enumerate(ga.LAW_BOUNDS)}
        b = {k: float(i) + 100.0 for i, k in enumerate(ga.LAW_BOUNDS)}
        for _ in range(10):
            child = ga.uniform_crossover(a, b, rng)
            for k in a:
                self.assertIn(child[k], (a[k], b[k]), f"{k} : gène hybride interdit")

    def test_two_crossovers_can_differ(self) -> None:
        rng = random.Random(2)
        a = {k: 0.0 for k in ga.LAW_BOUNDS}
        b = {k: 1.0 for k in ga.LAW_BOUNDS}
        seen = {ga.uniform_crossover(a, b, rng)["CRUISE_SPEED"] for _ in range(30)}
        self.assertEqual(len(seen), 2, "le croisement explore les deux parents")


class MutationTest(unittest.TestCase):
    def test_mutation_stays_within_bounds(self) -> None:
        rng = random.Random(3)
        for _ in range(200):
            gene = ga.clip_law({k: rng.uniform(*ga.LAW_BOUNDS[k]) for k in ga.LAW_BOUNDS})
            mutated = ga.mutate_law(gene, 0.5, rng)
            for k, v in mutated.items():
                lo, hi = ga.LAW_BOUNDS[k]
                self.assertGreaterEqual(v, lo)
                self.assertLessEqual(v, hi)

    def test_zero_sigma_is_a_no_op(self) -> None:
        gene = dict(ga.LAW_DEFAULTS)
        self.assertEqual(ga.mutate_law(gene, 0.0, random.Random(0)), gene)


class LawPolicyTest(unittest.TestCase):
    def test_constants_are_restored_after_the_call(self) -> None:
        before = {k: getattr(ship_autopilot_ref, k) for k in ga.LAW_BOUNDS}
        policy = ga.make_law_policy(ga.clip_law(
            {k: ga.LAW_BOUNDS[k][1] for k in ga.LAW_BOUNDS}))
        policy(obs_min())
        after = {k: getattr(ship_autopilot_ref, k) for k in ga.LAW_BOUNDS}
        self.assertEqual(before, after, "la loi portée doit être intacte hors de la décision")

    def test_the_reference_setting_is_reachable(self) -> None:
        # la population initiale contient le réglage du jeu : sa politique doit
        # produire la même commande que la loi originale (mêmes constantes)
        obs = obs_min()
        ref = ship_autopilot_ref.autopilot_ship_inputs(obs)
        got = ga.make_law_policy(dict(ga.LAW_DEFAULTS))(obs)
        self.assertEqual(got, ref)

    def test_the_recabled_law_can_complete_a_mining_loop(self) -> None:
        # le réglage du jeu livre l'épisode dans le simulateur : la fitness du
        # membre « expert » de la génération 0 doit être positive
        env = ShipSim()
        score = ga.fitness(ga.make_law_policy(dict(ga.LAW_DEFAULTS)), [3], 150.0, 0.0, env)
        self.assertGreater(score, 0.0, "l'autopilote réglé réussit la boucle de minage")


class GenomeNNTest(unittest.TestCase):
    def test_flatten_round_trip(self) -> None:
        from nn import MLP, OUTPUT_COUNT, feature_size

        rng = random.Random(0)
        net = MLP(feature_size(), 8, OUTPUT_COUNT, rng)
        genome = ga.nn_flatten(net)
        expected = (len(net.w1) * len(net.w1[0]) + len(net.b1)
                    + len(net.w2) * len(net.w2[0]) + len(net.b2))
        self.assertEqual(len(genome), expected, "taille du génome = taille des poids")
        clone = ga._clone_net(net)
        ga.nn_unflatten(clone, genome)
        self.assertEqual(ga.nn_flatten(clone), genome, "aplatissement réversible")

    def test_nn_child_is_decoupled_from_its_parent(self) -> None:
        # le croisement réseau clone le parent AVANT d'écrire le génome enfant :
        # muter un enfant ne doit pas corrompre son parent (bug d'aliasing)
        from nn import MLP, OUTPUT_COUNT, feature_size

        rng = random.Random(0)
        base = MLP(feature_size(), 8, OUTPUT_COUNT, rng)
        parent = {"net": ga._clone_net(base)}
        before = ga.nn_flatten(parent["net"])
        child_net = ga._clone_net(parent["net"])
        ga.nn_unflatten(child_net, [v + 1.0 for v in before])
        self.assertEqual(ga.nn_flatten(parent["net"]), before)


class RunGaHooksTest(unittest.TestCase):
    def test_on_candidate_reports_every_evaluation(self) -> None:
        # un événement par candidat évalué, génération 0 comprise : c'est ce
        # que l'afficheur échantillonne pour montrer la population ordinaire
        rng = random.Random(0)
        population = [dict(ga.LAW_DEFAULTS)]
        for _ in range(2):
            population.append(ga.clip_law(
                {k: rng.uniform(*ga.LAW_BOUNDS[k]) for k in ga.LAW_BOUNDS}))
        events: list[tuple[int, int, float]] = []
        ga.run_ga(
            "law", population, [3],
            timeout=60.0, robustness=0.0, gens=1, elites=1, tournament_k=2,
            sigma_ratio=0.2, sigma_min_ratio=0.05, rng=random.Random(1),
            on_candidate=lambda gen, index, s, genome: events.append((gen, index, s)),
        )
        self.assertEqual([(g, i) for g, i, _ in events],
                         [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2)],
                         "génération 0 puis re-évaluation, dans l'ordre")
        self.assertTrue(all(isinstance(s, float) for _, _, s in events),
                        "l'événement porte la fitness du candidat")


class SerialisationTest(unittest.TestCase):
    def test_law_payload_round_trips_through_policies(self) -> None:
        import policies

        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "ga_policy.json")
            gene = ga.clip_law(dict(ga.LAW_DEFAULTS))
            ga.save_law_policy(path, gene, 950.0, [1, 2, 3])
            with open(path, encoding="utf-8") as f:
                data = json.load(f)
            self.assertEqual(data["policy"], "ga-law")
            self.assertEqual(data["params"]["CRUISE_SPEED"], round(gene["CRUISE_SPEED"], 4))
            policy = policies.load_policy_file(path)
            cmd = policy(obs_min())
            self.assertEqual(set(cmd), {"up", "down", "left", "right", "fire"})

    def test_nn_payload_loads_back(self) -> None:
        from nn import MLP, OUTPUT_COUNT, feature_size, load_nn

        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "nn_policy_ga.json")
            net = MLP(feature_size(), 8, OUTPUT_COUNT, random.Random(0))
            ga.save_nn_ga(path, net, 800.0, [1])
            reloaded = load_nn(path)
            self.assertEqual(ga.nn_flatten(reloaded), ga.nn_flatten(net))


if __name__ == "__main__":
    unittest.main()
