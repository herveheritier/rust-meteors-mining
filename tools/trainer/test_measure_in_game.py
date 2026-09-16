#!/usr/bin/env python3
"""Tests du **protocole de mesure en boucle fermée dans le jeu**
(`measure_in_game.py`) : la logique de mesure doit être vérifiable sans lancer
la partie - c'est ce qui rend le protocole rejouable et non-régressable.

Trois verrous :

- `classify` (dénouement d'un pas) : le dénouement **explicite du jeu** prime,
  le garde-fou de temps simulé ne s'applique qu'à un épisode encore ouvert ;
- `run_episode` (la boucle) : elle doit **ignorer l'observation de l'épisode
  précédent** (l'`episode_id` doit avoir changé) et attendre que le **cerveau
  demandé** soit appliqué, puis s'arrêter au dénouement - testé contre un faux
  client, aucune partie requise ;
- le **vocabulaire des dénouements** est verrouillé contre `src/driver.rs` :
  les libellés rapportés sont ceux que le jeu publie, pas une invention du
  script (`délai` / `mur` sont bien les nôtres).

    python3 -m unittest -v test_measure_in_game
"""

from __future__ import annotations

import os
import unittest

from measure_in_game import (
    OUTCOME_DONE,
    OUTCOME_TIMEOUT,
    OUTCOME_WALL,
    classify,
    delivered_count,
    mean_seconds_delivered,
    run_episode,
)

HERE = os.path.dirname(os.path.abspath(__file__))

#: Source de vérité du vocabulaire (`EpisodeOutcome::label`).
DRIVER = os.path.normpath(os.path.join(HERE, os.pardir, os.pardir, "src", "driver.rs"))


def frame(
    frame_no: int,
    *,
    episode_id: int = 6,
    episode_t: float = 0.0,
    done: bool = False,
    outcome: str | None = None,
    learned_pilot: bool = True,
    steps: int = 0,
) -> dict:
    """Observation publiée par le jeu, réduite aux champs du protocole."""
    return {
        "frame": frame_no,
        "episode_id": episode_id,
        "episode_t": episode_t,
        "episode_done": done,
        "episode_outcome": outcome,
        "learned_pilot": learned_pilot,
        "episode_steps": steps,
        "episode_deliveries": 1 if outcome == "delivered" else 0,
        "episode_collected": 3 if outcome == "delivered" else 0,
    }


class FakeClient:
    """Client simulé : rejoue une liste d'observations prêtes (aucune partie).

    La dernière observation est répétée si la boucle en demande davantage -
    utile pour vérifier les garde-fous (une boucle qui ne se termine jamais).
    """

    def __init__(self, frames: list[dict]) -> None:
        self.frames = frames
        self.reads = 0
        self.resets: list[dict] = []
        self.commands: list[dict] = []

    def obs(self) -> dict:
        o = self.frames[min(self.reads, len(self.frames) - 1)]
        self.reads += 1
        return o

    def reset(self, **kw) -> None:
        self.resets.append(kw)

    def cmd(self, **kw) -> None:
        self.commands.append(kw)


#: Le protocole par défaut du script, resserré pour les tests.
KW = dict(target="ship", scenario="economy", sim_cap=10.0, wall_cap=5.0)


class ClassifyTest(unittest.TestCase):
    """Dénouement d'un pas (fonction pure)."""

    def test_open_episode_keeps_going(self) -> None:
        self.assertIsNone(classify(frame(1, episode_t=3.0), 10.0))

    def test_explicit_outcome_wins(self) -> None:
        """Un dénouement publié par le jeu est rapporté tel quel - même au-delà
        du garde-fou simulé (le jeu a tranché, le script ne réinterprète pas)."""
        o = frame(1, episode_t=99.0, done=True, outcome="delivered")
        self.assertEqual(classify(o, 10.0), "delivered")

    def test_sim_cap_guards_a_never_ending_episode(self) -> None:
        self.assertEqual(classify(frame(1, episode_t=10.5), 10.0), OUTCOME_TIMEOUT)

    def test_done_without_label_is_not_silently_dropped(self) -> None:
        o = frame(1, done=True, outcome=None)
        self.assertEqual(classify(o, 10.0), OUTCOME_DONE)


class VocabularyTest(unittest.TestCase):
    """Le vocabulaire rapporté est celui du jeu (non-régression de la source)."""

    def test_game_labels_exist_in_driver(self) -> None:
        with open(DRIVER, encoding="utf-8") as f:
            src = f.read()
        for label in ("delivered", "destroyed", "eva_recovered", "objectives_complete"):
            self.assertIn(f'"{label}"', src,
                          f"le libellé `{label}` n'existe plus dans src/driver.rs")

    def test_protocol_labels_are_ours(self) -> None:
        """`délai` / `mur` / `terminé` sont des états de **mesure** : ils ne
        doivent pas provenir du jeu (sinon le rapport serait ambigu)."""
        with open(DRIVER, encoding="utf-8") as f:
            src = f.read()
        for label in (OUTCOME_TIMEOUT, OUTCOME_WALL, OUTCOME_DONE):
            self.assertNotIn(f'"{label}"', src)


class RunEpisodeTest(unittest.TestCase):
    """La boucle de mesure contre un faux client."""

    def test_waits_for_the_new_episode_and_the_requested_brain(self) -> None:
        """Les frames de l'épisode **précédent** (id inchangé) ne sont pas
        mesurées : leurs compteurs piégés (une livraison à 999 s) ne doivent
        jamais ressortir dans le résultat."""
        frames = [
            frame(100, episode_id=5, done=True, outcome="delivered", episode_t=999.0,
                  learned_pilot=False),
            frame(101, episode_id=5, done=True, outcome="delivered", episode_t=999.0,
                  learned_pilot=False),
            frame(102, episode_id=5, done=True, outcome="delivered", episode_t=999.0,
                  learned_pilot=False),
            # nouvel épisode : le cerveau appris est bien appliqué
            frame(103, episode_id=6, episode_t=1.0, learned_pilot=True, steps=60),
            frame(104, episode_id=6, episode_t=12.0, done=True, outcome="delivered",
                  steps=720),
        ]
        c = FakeClient(frames)
        r = run_episode(c, seed=3, learned=True, **KW)
        self.assertEqual(r["outcome"], "delivered")
        self.assertEqual(r["seconds"], 12.0, "l'épisode précédent a fui dans la mesure")
        self.assertEqual(r["deliveries"], 1)
        self.assertEqual(c.resets, [{"seed": 3, "target": "ship", "scenario": "economy"}])
        self.assertEqual(c.commands, [{"autopilot": True, "learned_pilot": True}])

    def test_waits_for_the_scripted_brain_when_learned_was_left_on(self) -> None:
        """La bascule n'est pas instantanée : une frame encore jouée par le
        réseau (case restée allumée) ne doit pas être attribuée à la loi scriptée."""
        frames = [
            frame(100, episode_id=5, learned_pilot=True),
            frame(101, episode_id=6, learned_pilot=True, episode_t=0.5),   # bascule non consommée
            frame(102, episode_id=6, learned_pilot=False, episode_t=1.0),
            frame(103, episode_id=6, learned_pilot=False, episode_t=4.0, done=True,
                  outcome="destroyed"),
        ]
        c = FakeClient(frames)
        r = run_episode(c, seed=1, learned=False, **KW)
        self.assertEqual(r["outcome"], "destroyed")
        self.assertEqual(r["seconds"], 4.0)
        self.assertEqual(c.commands, [{"autopilot": True, "learned_pilot": False}])

    def test_sim_cap_ends_a_policy_that_never_delivers(self) -> None:
        """C'est le cas mesuré : le réseau embarqué ne boucle pas - l'épisode
        doit être classé `délai` au garde-fou simulé, pas attendu indéfiniment."""
        frames = [frame(100, episode_id=5, learned_pilot=True)]
        frames += [frame(100 + i, episode_id=6, episode_t=float(i),
                         learned_pilot=True, steps=i) for i in range(1, 12)]
        c = FakeClient(frames)
        r = run_episode(c, seed=2, learned=True, **KW)
        self.assertEqual(r["outcome"], OUTCOME_TIMEOUT)
        self.assertEqual(r["seconds"], 10.0)

    def test_wall_guard_when_no_new_episode_opens(self) -> None:
        """Jeu muet (ou remise à zéro jamais consommée) : le garde-fou mural
        rend `mur` au lieu de tourner sans fin."""
        c = FakeClient([frame(100, episode_id=5, learned_pilot=False)])
        r = run_episode(c, seed=1, learned=True, target="ship", scenario="economy",
                        sim_cap=10.0, wall_cap=0.05)
        self.assertEqual(r["outcome"], OUTCOME_WALL)


class TotalsTest(unittest.TestCase):
    """Agrégats du rapport."""

    RESULTS = [
        {"outcome": "delivered", "seconds": 20.0},
        {"outcome": "delivered", "seconds": 40.0},
        {"outcome": OUTCOME_TIMEOUT, "seconds": 150.0},
    ]

    def test_delivered_count(self) -> None:
        self.assertEqual(delivered_count(self.RESULTS), 2)

    def test_mean_seconds_only_over_deliveries(self) -> None:
        """Le temps moyen porte sur les épisodes **livrés** : mêler les délais
        gonflerait la moyenne d'un cerveau qui n'a rien livré."""
        self.assertEqual(mean_seconds_delivered(self.RESULTS), 30.0)

    def test_mean_without_a_delivery_is_none(self) -> None:
        self.assertIsNone(mean_seconds_delivered(self.RESULTS[2:]))


if __name__ == "__main__":
    unittest.main()
