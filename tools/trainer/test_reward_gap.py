#!/usr/bin/env python3
"""Test de **non-régression** de l'écart de récompense entre une politique
apprise et l'**autopilote du jeu** (tâche EVA, micro-simulateur).

L'autopilote du jeu vit dans le processus Rust ; il est **porté en Python**
(`autopilot_ref.py`, mêmes formules et mêmes constantes) pour que ce test
tourne sans lancer le jeu. Le test compare donc, sur les **mêmes graines**,
la politique apprise et l'autopilote porté, et vérifie que l'écart
`politique − autopilote` ne se dégrade pas.

La politique évaluée est `policy.json` par défaut (la politique `seek`
entraînée par CEM, committée) - surchargeable par la variable d'environnement
`TRAINER_POLICY` pour tester un `nn_policy.json` / `ppo_policy.json` / tout
autre fichier reconnu par `policies.load_policy_file`.

    cd tools/trainer
    python3 -m unittest -v test_reward_gap
    TRAINER_POLICY=ppo_policy.json python3 -m unittest -v test_reward_gap
"""

from __future__ import annotations

import os
import random
import unittest

from autopilot_ref import autopilot_ref_policy
from eva_env import EvaSim
from evaluate import run_episode_sim, sim_comparison
from policies import load_policy_file

#: Distance d'éjection de référence (docs/AUTOENTRAINEMENT.md §5 sexies).
SPAWN_DIST = 300.0

#: Graines d'évaluation **hors entraînement** (les politiques du dépôt
#: s'entraînent sur les graines 1..8).
EVAL_SEEDS = [11, 12, 13, 14, 15]

#: Graines de la vérification de la référence (mesure documentée : ~943,8).
REFERENCE_SEEDS = [1, 2, 3, 4, 5, 6]

#: Récompense moyenne attendue de l'autopilote porté sur les graines de
#: référence : bande de tolérance autour des 943,8 documentés (le portage
#: mesure ~941 en simulateur).
REFERENCE_MEAN_MIN = 900.0
REFERENCE_MEAN_MAX = 990.0

#: Écart minimal toléré `politique − autopilote`. La politique `seek` CEM
#: committée fait mieux que l'autopilote (~975 contre ~941, soit +34) ; le
#: seuil laisse la marge d'un changement d'implémentation de politique, mais
#: refuse une régression franche (politique nettement en dessous de la
#: référence).
MIN_GAP = -50.0

POLICY_PATH = os.environ.get("TRAINER_POLICY", "policy.json")


def _evaluate(policy, seeds: list[int], timeout: float = 60.0) -> list[float]:
    """Récompenses d'une politique sur des graines (simulateur EVA)."""
    env = EvaSim()
    rng = random.Random(0)
    return [
        run_episode_sim(env, policy, seed, SPAWN_DIST, timeout, rng)["reward"]
        for seed in seeds
    ]


class RewardGapTest(unittest.TestCase):
    """Non-régression : la politique apprise ne doit pas décrocher de
    l'autopilote de référence."""

    def test_reference_autopilot_matches_the_game(self) -> None:
        """Le portage Python doit reproduire l'autopilote du jeu (sinon toute
        comparaison serait vide de sens) : récupération systématique et
        récompense moyenne dans la bande documentée."""
        rewards = _evaluate(autopilot_ref_policy(), REFERENCE_SEEDS)
        mean = sum(rewards) / len(rewards)
        self.assertTrue(
            REFERENCE_MEAN_MIN <= mean <= REFERENCE_MEAN_MAX,
            f"récompense moyenne de l'autopilote porté {mean:.1f} hors de "
            f"[{REFERENCE_MEAN_MIN}, {REFERENCE_MEAN_MAX}] (référence "
            f"documentée ~943,8) - le portage a-t-il dérivé ?",
        )
        self.assertEqual(
            sum(1 for r in rewards if r > 500.0), len(REFERENCE_SEEDS),
            f"l'autopilote porté doit récupérer le cosmonaute à chaque épisode "
            f"(récompenses : {[round(r, 1) for r in rewards]})",
        )

    def test_learned_policy_gap_against_autopilot(self) -> None:
        """L'écart de récompense de la politique apprise à l'autopilote reste
        dans la tolérance, et la politique réussit au moins un épisode."""
        if not os.path.exists(POLICY_PATH):
            self.skipTest(f"politique absente : {POLICY_PATH} "
                          f"(entraîner puis relancer, ou poser TRAINER_POLICY)")
        policy = load_policy_file(POLICY_PATH)
        cmp = sim_comparison(policy, EVAL_SEEDS, SPAWN_DIST)
        self.assertGreaterEqual(
            cmp["policy_ok"], 1,
            f"la politique {POLICY_PATH} doit réussir au moins un épisode "
            f"(récompenses : {[round(r['reward'], 1) for r in cmp['policy']]})",
        )
        self.assertGreaterEqual(
            cmp["gap"], MIN_GAP,
            f"écart à l'autopilote {cmp['gap']:+.1f} < {MIN_GAP:+.1f} : "
            f"régression de {POLICY_PATH} (politique {cmp['policy_mean']:.1f} "
            f"vs autopilote {cmp['reference_mean']:.1f})",
        )

    def test_autopilot_reference_beats_idle(self) -> None:
        """Garde-fou : l'autopilote de référence doit dominer nettement
        l'immobilité (sinon la comparaison ne mesure rien)."""
        idle = lambda obs: {"up": False, "down": False, "left": False,  # noqa: E731
                            "right": False, "fire": False}
        idle_mean = sum(_evaluate(idle, EVAL_SEEDS)) / len(EVAL_SEEDS)
        ref_mean = sum(_evaluate(autopilot_ref_policy(), EVAL_SEEDS)) / len(EVAL_SEEDS)
        self.assertGreater(
            ref_mean, idle_mean + 500.0,
            "l'autopilote de référence doit nettement dépasser l'immobilité",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
