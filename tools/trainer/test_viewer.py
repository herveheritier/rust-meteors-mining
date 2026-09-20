#!/usr/bin/env python3
"""Tests de l'**afficheur pixels** (`viewer.py`) : conversions monde → pixel,
calque fixe + éléments mobiles, rejeu déterministe d'un épisode, session
d'affichage et repli terminal ANSI - tout **hors écran** (aucune fenêtre,
tkinter non requis).

    python3 -m unittest test_viewer -v
"""

from __future__ import annotations

import contextlib
import io
import unittest
from typing import Any

import ga
import viewer
from ship_env import WORLD_H, WORLD_W, ShipSim


def obs_view(**ship_extra: Any) -> dict[str, Any]:
    """Observation minimale pour le rendu (vaisseau au centre de la station)."""
    ship = {"x": 0.0, "y": 0.0, "speed": 0.0, "orientation": 0.0}
    ship.update(ship_extra)
    return {"ship": ship, "station_x": 0.0, "station_y": 0.0,
            "station_radius": 162.0, "docked": False, "episode_done": False,
            "episode_outcome": None, "episode_t": 0.0, "cargo_qty": 0,
            "cargo_cap": 5, "fuel": 100.0, "fuel_cap": 100.0, "ammo": 30,
            "ammo_cap": 30, "meteors_destroyed": 0, "nearby": [], "bullets": []}


def meteor(dx: float, dy: float, radius: float = 30.0, life: int = 4) -> dict[str, Any]:
    """Objet proche « météore » au format de l'observation (relatif au vaisseau)."""
    return {"kind": "meteore", "dx": dx, "dy": dy, "dist": (dx * dx + dy * dy) ** 0.5,
            "radius": radius, "life": life, "center_x": 0.0, "center_y": 0.0,
            "vx": 0.0, "vy": 0.0}


def idle(**over: bool) -> dict[str, bool]:
    cmd = dict(viewer.IDLE_CMD)
    cmd.update(over)
    return cmd


class GeometryTest(unittest.TestCase):
    def test_the_station_sits_at_the_center_of_the_grid(self) -> None:
        px, py = viewer.world_to_pixel(0.0, 0.0)
        self.assertEqual((px, py), (viewer.GRID_W // 2, viewer.GRID_H // 2))

    def test_the_torus_wraps_at_the_borders(self) -> None:
        self.assertEqual(viewer.world_to_pixel(WORLD_W - 1.0, WORLD_H - 1.0),
                         viewer.world_to_pixel(-1.0, -1.0), "le monde boucle")

    def test_plot_ignores_out_of_bounds_cells(self) -> None:
        grid = viewer.blank_grid()
        viewer.plot(grid, -5, 0, 3)
        viewer.plot(grid, 0, viewer.GRID_H, 3)
        self.assertEqual(grid, viewer.blank_grid(), "aucun effet de bord")

    def test_disc_and_line_paint_the_expected_cells(self) -> None:
        grid = viewer.blank_grid()
        cells = viewer.disc(grid, 10, 10, 2, 5)
        self.assertEqual(len(cells), 13, "disque de rayon 2 = 1+3+5+3+1 cellules")
        line_cells = {(x, 0) for x in range(4)}
        grid2 = viewer.blank_grid()
        viewer.line(grid2, 0, 0, 3, 0, 7)
        self.assertEqual({(col, row) for row in range(len(grid2))
                          for col in range(len(grid2[row]))
                          if grid2[row][col] == 7}, line_cells)


class StaticLayerTest(unittest.TestCase):
    def test_the_static_layer_holds_the_station_and_the_meteors(self) -> None:
        obs = obs_view()
        obs["nearby"] = [meteor(100.0, 0.0)]
        base, cells = viewer.draw_static(obs)
        cx, cy = viewer.GRID_W // 2, viewer.GRID_H // 2
        self.assertEqual(base[cy][cx], 2, "la trappe est le pixel central")
        self.assertEqual(base[cy - 15][cx], 1, "la coque entoure le liseré")
        mx, my = viewer.world_to_pixel(100.0, 0.0)
        self.assertEqual(base[my][mx], 5, "météore au rayon observé")
        self.assertIn((mx, my), cells)

    def test_meteor_positions_are_absolute_not_relative(self) -> None:
        obs = obs_view(x=100.0)
        obs["nearby"] = [meteor(-100.0, 0.0)]  # absolu : (0, 0), la station
        self.assertEqual(list(viewer.meteor_map(obs)),
                         [(viewer.GRID_W // 2, viewer.GRID_H // 2)])


class DynamicLayerTest(unittest.TestCase):
    def test_minerals_bullets_ship_and_nose_are_drawn(self) -> None:
        obs = obs_view()
        obs["nearby"] = [{"kind": "minerai", "dx": 50.0, "dy": 0.0, "dist": 50.0,
                          "life": 1, "radius": 10.0}]
        obs["bullets"] = [{"kind": "balle", "dx": -30.0, "dy": 0.0, "dist": 30.0,
                           "life": 1, "radius": 10.0}]
        grid = viewer.blank_grid()
        cells = viewer.draw_dynamic(grid, obs, idle())
        cx, cy = viewer.GRID_W // 2, viewer.GRID_H // 2
        self.assertEqual(grid[cy][cx + 5], 6, "minerai à 50 u à l'est")
        self.assertEqual(grid[cy][cx - 3], 7, "balle à 30 u à l'ouest")
        self.assertEqual(grid[cy][cx], 3, "vaisseau au centre de la vue")
        self.assertEqual(grid[cy][cx + 1], 3, "nez à +14 u (pixel suivant)")
        self.assertGreaterEqual(len(cells), 4)

    def test_the_flame_only_follows_thrust(self) -> None:
        cx, cy = viewer.GRID_W // 2, viewer.GRID_H // 2

        def flame_cells(cmd: dict[str, bool]) -> set[int]:
            grid = viewer.blank_grid()
            viewer.draw_dynamic(grid, obs_view(), cmd)
            return {c for row in grid for c in row if c == 4}

        self.assertFalse(flame_cells(idle()), "pas de flamme sans poussée")
        self.assertTrue(flame_cells(idle(up=True)), "flamme à la poussée")
        self.assertEqual(self.grid_of(obs_view(), idle(up=True))[cy][cx], 3,
                         "le corps du vaisseau reste visible sous la flamme")

    def grid_of(self, obs: dict[str, Any], cmd: dict[str, bool]) -> list[list[int]]:
        grid = viewer.blank_grid()
        viewer.draw_dynamic(grid, obs, cmd)
        return grid


class RecordEpisodeTest(unittest.TestCase):
    def test_the_recorded_episode_matches_the_simulator(self) -> None:
        policy = ga.make_law_policy(dict(ga.LAW_DEFAULTS))
        frames, info = viewer.record_episode(policy, 3, 150.0)
        reference = ga.run_sim_episode(ShipSim(), policy, 3, 150.0)
        self.assertGreater(len(frames), 50, "l'épisode est échantillonné")
        self.assertTrue(frames[0][0]["docked"], "départ à quai")
        self.assertTrue(frames[-1][0]["episode_done"], "dernière image = dénouement")
        self.assertEqual(info["reward"], reference["reward"],
                         "le rejeu est exactement l'épisode de la fitness")

    def test_the_law_completes_the_mining_loop(self) -> None:
        _, info = viewer.record_episode(ga.make_law_policy(dict(ga.LAW_DEFAULTS)),
                                        3, 150.0)
        self.assertTrue(info["success"], "livraison en 150 s (graine 3)")


class SessionTest(unittest.TestCase):
    def test_events_update_the_session_state(self) -> None:
        session = viewer.TrainingSession(gens=5)
        session.push_status("évaluation…")
        frames = [(obs_view(), idle())]
        info = {"seed": 1, "reward": 950.0, "success": True}
        session.push_generation(1, 950.0, 800.0, 0.2, frames, info)
        session.close()
        session._drain_queue()
        self.assertEqual(session._history, [(1, 950.0, 800.0)])
        self.assertEqual(session._current["label"], "génération 1/5")
        text = "\n".join(session._stats_lines())
        self.assertIn("meilleur", text)
        self.assertIn("950.0", text)
        self.assertIn("entraînement terminé", text)

    def test_the_newest_generation_wins_while_one_is_playing(self) -> None:
        session = viewer.TrainingSession()
        frames = [(obs_view(), idle())]
        session.push_generation(1, 900.0, 800.0, 0.2, frames, {"seed": 1})
        session.push_generation(2, 940.0, 810.0, 0.2, frames, {"seed": 1})
        session._drain_queue()
        self.assertEqual(session._current["label"], "génération 1")
        self.assertEqual(session._pending[2], "génération 2",
                         "la génération suivante attend son tour")
        session._finish_current()
        self.assertEqual(session._current["label"], "génération 2")
        session._finish_current()
        self.assertIsNone(session._current)
        self.assertTrue(session._idle.is_set(), "le producteur est rendu")

    def test_a_candidate_plays_now_and_the_next_generation_waits(self) -> None:
        session = viewer.TrainingSession()
        frames = [(obs_view(), idle())]
        session.push_candidate(1, 5, 412.5, frames, {"seed": 1})
        session.push_generation(1, 940.0, 810.0, 0.2, frames, {"seed": 1})
        session._drain_queue()
        self.assertEqual(session._current["label"], "candidat #5 · fit 412.5",
                         "le candidat ordinaire joue tout de suite")
        self.assertEqual(session._pending[2], "génération 1",
                         "…et la génération attend son tour")
        session._finish_current()
        self.assertEqual(session._current["label"], "génération 1")
        session._finish_current()
        self.assertIsNone(session._current)

    def test_the_featured_lane_passes_before_the_candidate_lane(self) -> None:
        session = viewer.TrainingSession()
        frames = [(obs_view(), idle())]
        session.push_generation(1, 940.0, 810.0, 0.2, frames, {"seed": 1})
        session.push_candidate(1, 5, 412.5, frames, {"seed": 1})
        session.push_generation(2, 950.0, 820.0, 0.2, frames, {"seed": 1})
        session._drain_queue()
        self.assertEqual(session._current["label"], "génération 1")
        self.assertEqual(session._pending[2], "génération 2")
        self.assertIsNotNone(session._regular)
        session._finish_current()
        self.assertEqual(session._current["label"], "génération 2",
                         "une génération passe toujours devant un candidat")
        self.assertIsNotNone(session._regular, "le candidat attend encore")
        session._finish_current()
        self.assertEqual(session._current["label"], "candidat #5 · fit 412.5")

    def test_the_newest_candidate_wins_over_an_older_one(self) -> None:
        session = viewer.TrainingSession()
        frames = [(obs_view(), idle())]
        session.push_generation(1, 940.0, 810.0, 0.2, frames, {"seed": 1})
        session.push_candidate(1, 3, 100.0, frames, {"seed": 1})
        session.push_candidate(1, 7, 200.0, frames, {"seed": 2})
        session._drain_queue()
        self.assertEqual(session._current["label"], "génération 1")
        self.assertEqual(session._regular[2], "candidat #7 · fit 200.0",
                         "le candidat ordinaire le plus récent gagne")
        session._skip()
        self.assertEqual(session._current["label"], "candidat #7 · fit 200.0",
                         "« Passer » enchaîne le candidat en attente")

    def test_a_destroyed_meteor_is_erased_from_the_static_layer(self) -> None:
        obs = obs_view()
        obs["nearby"] = [meteor(100.0, 0.0)]
        session = viewer.TrainingSession()
        session._start(([(obs, idle())], {"seed": 1, "reward": 0.0}, "génération 0"))
        mx, my = viewer.world_to_pixel(100.0, 0.0)
        self.assertEqual(session._base[my][mx], 5)
        gone = obs_view()  # le météore n'est plus dans nearby
        repaint = session._update_static(gone)
        self.assertIn((mx, my), repaint, "les cellules du météore sont repeintes")
        self.assertEqual(session._base[my][mx], 0, "…au fond du ciel")
        self.assertEqual(session._meteors, {})

    def test_ansi_session_runs_and_stops_cleanly(self) -> None:
        session = viewer.TrainingSession(backend="ansi")
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            session.run(auto_quit_ms=150)
        self.assertTrue(session._stopped, "l'auto-quit ferme la boucle ANSI")


class AnsiFallbackTest(unittest.TestCase):
    def test_the_view_centers_the_ship_and_shows_the_station(self) -> None:
        grid = viewer.render_ansi(obs_view(), idle(), cols=41, rows=15)
        cx, cy = 41 // 2, 15 // 2
        self.assertEqual(grid[cy][cx], 3, "vaisseau au centre (vue suivie)")
        self.assertEqual(grid[cy][cx + 5], 1, "coque de la station proche")

    def test_a_meteor_far_away_is_invisible_from_the_view(self) -> None:
        obs = obs_view()
        obs["nearby"] = [meteor(1500.0, 1500.0)]
        grid = viewer.render_ansi(obs, idle(), cols=41, rows=15)
        flat = [c for row in grid for c in row]
        self.assertNotIn(5, flat, "hors de la vue de ±640 u")

    def test_grid_to_ansi_encodes_24_bit_colors(self) -> None:
        text = viewer.grid_to_ansi([[0, 3], [0, 0]])
        self.assertIn("\x1b[48;2;13;17;23m", text, "fond #0d1117")
        self.assertIn("\x1b[48;2;255;215;94m", text, "vaisseau #ffd75e")


if __name__ == "__main__":
    unittest.main()
