#!/usr/bin/env python3
"""Tests hors-ligne du **portage de l'autopilote vaisseau**
(`ship_autopilot_ref.py`) : sur des observations synthétiques (même format
JSON que `/obs`), vérifier les décisions clés de la loi - cap sur la station,
conduite 4 WAYS, retour soute pleine, attaque prioritaire, retenue de feu.

La **fidélité** au jeu se mesure contre la vraie partie
(`validate_ship_port.py`, qui exige `cargo run -- --headless`) ; ces tests-ci
sont la garde de non-régression hors-ligne (stdlib `unittest`, aucun jeu).

    cd tools/trainer
    python3 -m unittest -v test_ship_port
"""

from __future__ import annotations

import unittest

from ship_autopilot_ref import autopilot_ship_inputs

MOVING_MODE_INERTIAL = 0
MOVING_MODE_4_WAYS = 1
MOVING_MODE_DIRECTIONAL = 2
MOVING_MODE_REALISTIC = 3


def ship(**kw: float) -> dict[str, float]:
    """Cinématique de vaisseau (défauts : au repos, centre nul)."""
    base = {"x": 0.0, "y": 0.0, "vx": 0.0, "vy": 0.0, "speed": 0.0,
            "direction": 0.0, "orientation": 0.0, "rotation": 0.0,
            "center_x": 0.0, "center_y": 0.0}
    base.update(kw)
    return base


def obj(kind: str, dx: float, dy: float, **kw: float) -> dict[str, float]:
    """Objet proche (météore/minerai/balle) au format `NearbyObject`."""
    base = {"kind": kind, "dx": dx, "dy": dy, "dist": (dx * dx + dy * dy) ** 0.5,
            "vx": 0.0, "vy": 0.0, "radius": 20.0, "life": 5,
            "center_x": 0.0, "center_y": 0.0}
    base.update(kw)
    return base


def obs(ship_kin: dict[str, float], **kw) -> dict:
    """Observation minimale au format `/obs` (pilote = vaisseau)."""
    base = {
        "pilot": "vaisseau",
        "ship": ship_kin,
        "station_x": 0.0,
        "station_y": 0.0,
        "station_radius": 162.0,
        "station_dx": -ship_kin["x"],
        "station_dy": -ship_kin["y"],
        "station_dist": (ship_kin["x"] ** 2 + ship_kin["y"] ** 2) ** 0.5,
        "moving_mode": MOVING_MODE_DIRECTIONAL,
        "economy": False,
        "fuel": 0.0,
        "fuel_cap": 0.0,
        "ammo": 0,
        "ammo_cap": 0,
        "credits": 0,
        "cargo_qty": 0,
        "cargo_cap": 5,
        "supplies_affordable": False,
        "nearby": [],
        "bullets": [],
    }
    base.update(kw)
    return base


class ShipAutopilotPortTest(unittest.TestCase):
    def test_directional_pushes_toward_the_station_when_aligned(self) -> None:
        # vaisseau à l'est de la station, nez à l'ouest (orienté vers elle)
        import math

        o = obs(ship(x=300.0, orientation=math.pi))
        cmd = autopilot_ship_inputs(o)
        self.assertTrue(cmd["up"], "aligné et en dessous de la croisière : pousse")
        self.assertFalse(cmd["left"] or cmd["right"], "déjà aligné : ne tourne pas")

    def test_directional_turns_toward_the_station_when_misaligned(self) -> None:
        # nez à l'est (orientation 0) alors que la station est à l'ouest
        o = obs(ship(x=300.0, orientation=0.0))
        cmd = autopilot_ship_inputs(o)
        self.assertFalse(cmd["up"], "nez désaligné : les gaz sont coupés")
        self.assertTrue(cmd["left"] or cmd["right"], "doit tourner vers la station")

    def test_four_ways_pushes_right_toward_an_east_target(self) -> None:
        o = obs(ship(x=0.0, y=0.0), moving_mode=MOVING_MODE_4_WAYS,
                station_dx=300.0, station_dy=0.0, station_dist=300.0)
        cmd = autopilot_ship_inputs(o)
        self.assertTrue(cmd["right"], "cible à l'est : poussée → de l'écran")

    def test_full_cargo_returns_to_the_station(self) -> None:
        import math

        # nez à l'est (à l'opposé de la station) alors que la soute est pleine
        o = obs(ship(x=300.0, orientation=0.0), cargo_qty=5, cargo_cap=5)
        cmd = autopilot_ship_inputs(o)
        self.assertTrue(cmd["left"] or cmd["right"],
                        "soute pleine : rentrer accoster (se réorienter)")
        self.assertNotEqual(cmd["up"] and cmd["down"], True)

    def test_hostile_threatening_the_station_is_attacked_first(self) -> None:
        # météore à 200 u de la station (< 240) : mission prioritaire
        o = obs(ship(x=0.0, y=0.0, orientation=0.0),
                nearby=[obj("meteore", 100.0, 0.0, life=8)])
        cmd = autopilot_ship_inputs(o)
        self.assertTrue(cmd["fire"], "hostile à portée et aligné : tir")
        self.assertTrue(cmd["up"], "s'approche de la distance de tir")

    def test_hold_fire_on_a_target_finished_by_bullets_in_flight(self) -> None:
        # cible quasi détruite (life 1) et balle en vol dans le rayon : retenue
        dead = obj("meteore", 100.0, 0.0, life=1)
        bullet = obj("balle", 90.0, 0.0, life=1)
        o = obs(ship(x=0.0, y=0.0, orientation=0.0), nearby=[dead], bullets=[bullet])
        self.assertFalse(autopilot_ship_inputs(o)["fire"],
                         "balle en vol sur une cible quasi détruite : retenue de feu")
        # sans la balle, le tir reprend
        o2 = obs(ship(x=0.0, y=0.0, orientation=0.0), nearby=[dead], bullets=[])
        self.assertTrue(autopilot_ship_inputs(o2)["fire"])

    def test_low_supplies_in_economy_returns_to_dock(self) -> None:
        import math

        o = obs(ship(x=300.0, orientation=0.0), economy=True, fuel=0.0,
                fuel_cap=100.0, supplies_affordable=True)
        cmd = autopilot_ship_inputs(o)
        self.assertTrue(cmd["left"] or cmd["right"], "réserves basses : rentrer")

    def test_neutral_when_the_ship_is_not_piloting(self) -> None:
        o = obs(ship())
        o["pilot"] = "eva"
        cmd = autopilot_ship_inputs(o)
        self.assertFalse(any(cmd.values()), "plus le vaisseau qui pilote : rien à faire")


if __name__ == "__main__":
    unittest.main(verbosity=2)
