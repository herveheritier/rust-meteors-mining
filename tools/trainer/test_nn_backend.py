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

import json
import os
import random
import tempfile
import unittest

from nn import (ACTIONS, MLP, OUTPUT_COUNT, SIGMOID_ACTIONS, TURN_ACTIONS,
                action_target, load_nn, save_nn)

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


class ActionTargetTest(unittest.TestCase):
    """Le **format des cibles** (`action_target`) : trois sigmoïdes
    `(up, down, fire)` puis un un-seul de rotation `(left, right, none)`.

    Bug corrigé (format v8) : `action_target` découpait `ACTIONS[:SIGMOID_OUTPUTS]`
    et `ACTIONS` range `left`/`right` **avant** `fire` - la cible des sigmoïdes
    était donc `(up, down, left)`. Le **tir**, seule action qui libère les
    minerais, était absent de l'apprentissage : le réseau lisait « tire » là où
    il avait appris « tourne à gauche ». Ce verrou est là pour que le format ne
    dérive plus jamais en silence - il coûtait la boucle de minage entière.
    """

    def test_sigmoids_are_up_down_fire(self) -> None:
        self.assertEqual(SIGMOID_ACTIONS, ("up", "down", "fire"))
        self.assertEqual(TURN_ACTIONS, ("left", "right", "none"))
        self.assertEqual(action_target({"fire": True}), [0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
        self.assertEqual(action_target({"up": True, "down": True}),
                         [1.0, 1.0, 0.0, 0.0, 0.0, 1.0])

    def test_left_is_not_duplicated_in_the_sigmoids(self) -> None:
        """`left` n'a **que** sa classe de rotation : la cible v8 l'encodait
        deux fois (une sigmoïde *et* la tête de rotation), ce qui faisait
        hériter la tête « tir » de la valeur de virage à gauche."""
        y = action_target({"left": True})
        self.assertEqual(y[:3], [0.0, 0.0, 0.0], "une sigmoïde porte un virage")
        self.assertEqual(y[3:], [1.0, 0.0, 0.0])
        self.assertEqual(action_target({"left": True, "fire": True}),
                         [0.0, 0.0, 1.0, 1.0, 0.0, 0.0])

    def test_the_target_decodes_back_to_the_action(self) -> None:
        """Invariant central : relire la cible avec le **décodeur du jeu**
        (`learned_pilot.rs`, `policies.py` : seuil 0,5 puis classe de rotation
        la plus probable) doit rendre l'action d'origine - sans quoi le réseau
        apprend une action et le jeu en exécute une autre."""
        for combo in range(32):
            action = {k: bool(combo & (1 << i))
                      for i, k in enumerate(("up", "down", "left", "right", "fire"))}
            if action["left"] and action["right"]:
                continue  # l'expert ne tourne jamais des deux côtés à la fois
            y = action_target(action)
            back = {k: y[i] >= 0.5 for i, k in enumerate(SIGMOID_ACTIONS)}
            turn = max(range(3), key=lambda k: y[3 + k])
            back["left"], back["right"] = turn == 0, turn == 1
            self.assertEqual(back, action, f"action non reconstructible : {action}")

    def test_the_trained_policy_decodes_back_to_the_experts_action(self) -> None:
        """Bout en bout : le réseau apprend les cibles, mais c'est le
        **décodeur du jeu** qui agit. On compare donc les actions **rejouées par
        le jeu** à celles de l'expert : en v8 la sortie lue comme « tir » avait
        appris la sigmoïde d'un virage à gauche, et les deux divergeaient."""
        rng = random.Random(7)
        keys = ("up", "down", "left", "right", "fire")
        X: list[list[float]] = []
        actions: list[dict] = []
        for _ in range(SAMPLES):
            x = [rng.uniform(-1.0, 1.0) for _ in range(8)]
            action = {"up": x[0] > 0.0, "down": x[1] > 0.0, "fire": x[2] > 0.2,
                      "left": x[3] > 0.3, "right": x[3] < -0.3}
            X.append(x)
            actions.append(action)
        net = MLP(8, 8, OUTPUT_COUNT, rng)
        net.train(X, [action_target(a) for a in actions], epochs=200, lr=0.1,
                  batch_size=32, patience=25, rng=rng, backend="python")
        same = 0
        for x, want in zip(X, actions):
            out = net.forward(x)
            got = {k: out[i] >= 0.5 for i, k in enumerate(SIGMOID_ACTIONS)}
            turn = net.turn_action(out)
            got["left"], got["right"] = turn == "left", turn == "right"
            same += got == want
        self.assertGreater(same / len(X), 0.95,
                           f"le jeu n'exécute pas l'action apprise "
                           f"({same / len(X) * 100:.1f} % d'accord)")

    def test_a_policy_trained_on_another_layout_is_refused(self) -> None:
        """Un fichier de poids entraîné sur l'ancien format doit être **refusé**
        (et non rejoué de travers) : le format des cibles est écrit dans
        l'artefact."""
        net, _ = fit("python")
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "policy.json")
            save_nn(path, net)
            load_nn(path)  # le format courant passe
            with open(path, encoding="utf-8") as f:
                payload = json.load(f)
            self.assertEqual(payload["sigmoid_actions"], list(SIGMOID_ACTIONS))
            for wrong in (["up", "down", "left"], None):
                payload["sigmoid_actions"] = wrong
                with open(path, "w", encoding="utf-8") as f:
                    json.dump(payload, f)
                with self.assertRaises(ValueError, msg=f"format v8 accepté ({wrong})"):
                    load_nn(path)


if __name__ == "__main__":
    unittest.main()
