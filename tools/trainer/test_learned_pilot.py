#!/usr/bin/env python3
"""Test de non-régression de la **stratégie apprise embarquée** dans le jeu
(Phase 4) : la politique déployée (`assets/ship_pilot_policy.json`, compilée
dans le binaire par `src/learned_pilot.rs`) et le **fixture de fidélité** que
rejoue le test unitaire Rust.

Ce test verrouille le maillon Python de la chaîne :

    nn.py (features + réseau)  ==  fixture  ==  portage Rust

- l'asset embarqué doit être un réseau `nn` de la **bonne version de features**
  et de la bonne taille d'entrée (`feature_size()`) ;
- sur chaque fenêtre enregistrée (`fixtures/learned_pilot_windows.jsonl`, issue
  d'une **vraie partie** par `validate_learned_port.py --record`), les features
  de `nn.py` doivent être **exactement** celles du fixture et l'action de la
  politique Python **exactement** celle enregistrée.

Conséquence voulue : ré-entraîner la politique ou modifier les features **casse
ce test** tant que le fixture n'est pas ré-enregistré - impossible de déployer
des poids qui ne correspondent plus à ce que le Rust exécute.

    python3 -m unittest -v test_learned_pilot
"""

from __future__ import annotations

import json
import os
import unittest

from nn import feature_size, load_nn, obs_features
from policies import nn_policy

HERE = os.path.dirname(os.path.abspath(__file__))

#: Politique embarquée dans le binaire (`src/learned_pilot.rs`) : `../..`
#: depuis `tools/trainer/` remonte à la racine du projet.
POLICY = os.path.normpath(
    os.path.join(HERE, os.pardir, os.pardir, "assets", "ship_pilot_policy.json")
)

#: Fenêtres de fidélité : observation, features de `nn.py`, action de la
#: politique Python (`validate_learned_port.py --record`).
FIXTURE = os.path.join(HERE, "fixtures", "learned_pilot_windows.jsonl")

#: Touches comparées.
KEYS = ("up", "down", "left", "right", "fire")


def load_windows() -> list[dict]:
    """Fenêtres du fixture (une ligne JSON par fenêtre)."""
    with open(FIXTURE, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


class LearnedPilotTest(unittest.TestCase):
    """Cohérence de la politique déployée et du fixture de fidélité."""

    def test_embedded_policy_matches_the_trainer(self) -> None:
        """L'asset embarqué doit être rejouable par `nn.py` : bon type, bonne
        version de features (sinon `load_nn` refuse) et bonne taille d'entrée."""
        net = load_nn(POLICY)
        self.assertEqual(
            net.inputs,
            feature_size(),
            f"la politique embarquée attend {net.inputs} features, "
            f"nn.py en produit {feature_size()} (re-entraîner)",
        )
        # 3 sigmoïdes (up/down/fire) + 3 classes de rotation (left/right/none)
        self.assertEqual(net.outputs, 6)

    def test_fixture_features_match_the_trainer(self) -> None:
        """Les features enregistrées sont **exactement** celles de `nn.py` : le
        fixture est la preuve que le Rust reçoit le même vecteur."""
        windows = load_windows()
        self.assertGreaterEqual(len(windows), 10, "fixture de fidélité trop pauvre")
        for i, w in enumerate(windows):
            self.assertEqual(
                obs_features(w["obs"]),
                w["features"],
                f"fenêtre {i} : features divergentes (re-enregistrer le fixture)",
            )

    def test_fixture_action_is_the_python_policy(self) -> None:
        """L'action de chaque fenêtre doit être celle de la **politique Python**
        (et non du portage Rust) : c'est elle que le test unitaire Rust compare
        à ses propres décisions - la chaîne Rust == Python passe par là."""
        policy = nn_policy(POLICY)
        for i, w in enumerate(load_windows()):
            got = {k: bool(policy(w["obs"]).get(k)) for k in KEYS}
            want = {k: bool(w["action"].get(k)) for k in KEYS}
            self.assertEqual(got, want, f"fenêtre {i} : action de référence divergente")


if __name__ == "__main__":
    unittest.main()
