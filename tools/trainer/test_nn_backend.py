#!/usr/bin/env python3
"""Test de non-régression des **deux moteurs d'entraînement** de `nn.py::MLP`.

Le chemin `python` (défaut) est la référence : **aucune dépendance**, c'est lui
qui doit rester rejouable partout, CI comprise. Le chemin `numpy`, **optionnel**,
sert à entraîner les réseaux larges et les gros jeux de données que le Python
pur ne tient pas (mesuré : ~33 × plus rapide) ; il doit faire **le même**
apprentissage (même perte, même arrêt précoce) et produire des poids
sérialisables et rechargeables.

Ce verrou compte parce que le chemin `numpy` est sur le **chemin critique du
déploiement** : l'artefact embarqué dans le jeu (`assets/ship_pilot_policy.json`)
est entraîné avec lui (64 cachés, 24 000 pas, `ship_warmstart.py` par défaut).

    python3 -m unittest -v test_nn_backend
"""

from __future__ import annotations

import os
import random
import tempfile
import unittest

from nn import MLP, OUTPUT_COUNT, action_target, load_nn, save_nn

try:
    import numpy  # noqa: F401
    HAVE_NUMPY = True
except ImportError:  # pragma: no cover - dépend de l'environnement
    HAVE_NUMPY = False

#: Taille du jeu de données de test : apprenable jusqu'à l'exactitude, donc les
#: deux moteurs doivent y arriver - on ne compare pas des poids (l'ordre des
#: flottants diffère), mais la **capacité à apprendre**.
SAMPLES = 400


def make_dataset(seed: int = 3) -> tuple[list[list[float]], list[list[float]]]:
    """Jeu **déterministe** : des seuils sur les entrées décident des trois
    sigmoïdes (`up`/`down`/`fire`) et de la rotation, encodés par
    `action_target` (les vraies cibles de l'entraîneur)."""
    rng = random.Random(seed)
    X: list[list[float]] = []
    Y: list[list[float]] = []
    for _ in range(SAMPLES):
        x = [rng.uniform(-1.0, 1.0) for _ in range(8)]
        action = {
            "up": x[0] > 0.0,
            "down": x[1] > 0.0,
            "fire": x[2] > 0.2,
            "left": x[3] > 0.3,
            "right": x[3] < -0.3,
        }
        X.append(x)
        Y.append(action_target(action))
    return X, Y


def fit(backend: str) -> tuple[MLP, dict]:
    """Entraîne un petit réseau sur le jeu de test avec le moteur demandé."""
    X, Y = make_dataset()
    rng = random.Random(0)
    net = MLP(8, 8, OUTPUT_COUNT, rng)
    res = net.train(X, Y, epochs=60, lr=0.1, batch_size=32, patience=15,
                    rng=rng, backend=backend)
    return net, res


class BackendTest(unittest.TestCase):
    """Le moteur par défaut reste sans dépendance ; l'optionnel apprend autant."""

    def test_python_backend_is_the_default_and_needs_nothing(self) -> None:
        """Sans `backend`, `train` n'importe **rien** et se déclare `python`."""
        _, res = fit("python")
        self.assertEqual(res["backend"], "python")
        self.assertGreater(res["epochs_done"], 0)
        self.assertGreater(res["accuracy"], 0.9, "le jeu de test est apprenable")

    @unittest.skipUnless(HAVE_NUMPY, "numpy absent (le chemin Python pur reste testé)")
    def test_numpy_backend_learns_as_well_as_python(self) -> None:
        """Le chemin vectorisé doit atteindre le même niveau d'apprentissage :
        s'il régressait silencieusement, l'artefact embarqué en hériterait."""
        _, py = fit("python")
        _, np_ = fit("numpy")
        self.assertEqual(np_["backend"], "numpy")
        self.assertGreater(np_["accuracy"], 0.9,
                           f"numpy n'apprend pas (exactitude {np_['accuracy']:.3f})")
        self.assertAlmostEqual(np_["accuracy"], py["accuracy"], delta=0.10)

    @unittest.skipUnless(HAVE_NUMPY, "numpy absent")
    def test_numpy_weights_round_trip_through_save_nn(self) -> None:
        """Les poids du chemin numpy sont des **listes Python** : `save_nn` doit
        les écrire (un tableau numpy ferait échouer `json.dump`) et `load_nn`
        doit les relire à l'identique."""
        net, res = fit("numpy")
        X, _ = make_dataset()
        before = [net.forward(x) for x in X[:20]]
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "policy.json")
            save_nn(path, net)
            reloaded = load_nn(path)
        self.assertEqual(reloaded.hidden, net.hidden)
        for a, b in zip(before, (reloaded.forward(x) for x in X[:20])):
            self.assertEqual(a, b, "les poids rechargés diffèrent")
        self.assertGreater(res["accuracy"], 0.9)


if __name__ == "__main__":
    unittest.main()
