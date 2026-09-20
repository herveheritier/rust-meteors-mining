#!/usr/bin/env python3
"""Afficheur **pixels** indépendant : une grille où chaque élément du jeu est
un petit bloc de couleur - la station, le vaisseau (avec son nez et sa flamme),
les météores, les minerais libérés et les balles - plus un panneau de suivi
(statistiques, courbes de fitness). Le monde torique (3 960 × 3 540 u) est
rendu à **10 u/pixel**, vu de la station (posée en 0, 0, au centre de la
grille) : l'anneau minier (450-1 200 u) tient entier à l'écran.

Deux usages :

- **seul** : rejouer des épisodes dans le simulateur (`ship_env.ShipSim`) - la
  loi portée de l'autopilote par défaut, ou une politique entraînée
  (`--policy ga_policy.json`) ;
- **branché sur l'entraînement** (`ga.py --view`) : à chaque génération du GA,
  l'épisode du **meilleur candidat** est rejoué pendant que les courbes
  best/moyenne avancent ; entre les générations défilent des épisodes de
  **candidats ordinaires échantillonnés** (`--view-sample`, lecture de
  moindre priorité) ; à la fin, l'épisode de **validation**. L'entraînement
  tourne dans un fil d'arrière-plan (la file d'événements le découple de
  l'écran : il n'attend jamais l'affichage).

Python **standard uniquement** : `tkinter` pour la fenêtre ; sans tkinter (ou
avec `--backend ansi`), le même affichage tombe dans le **terminal** (couleurs
ANSI 24 bits, vue suivie du vaisseau).

    python3 viewer.py                          # la loi du jeu, graine 1
    python3 viewer.py --policy ga_policy.json  # une politique entraînée
    python3 ga.py --view                       # le déroulement d'un entraînement
"""

from __future__ import annotations

import argparse
import math
import queue
import sys
import threading
import time
from typing import Any, Optional

from eva_env import episode_reward
from ship_env import (OUTCOME_DELIVERED, OUTCOME_DESTROYED, WORLD_H, WORLD_W,
                      ShipSim, episode_outcome)

# ── palette et grille de pixels ──────────────────────────────────────────────

#: Palette (index → hex) : chaque élément du jeu a sa couleur de pixel.
PALETTE = (
    "#0d1117",  # 0 fond (espace)
    "#8b95a7",  # 1 coque de la station
    "#4a5568",  # 2 liseré de la station / trappe
    "#ffd75e",  # 3 vaisseau
    "#ff8c42",  # 4 flamme de poussée
    "#a97b5c",  # 5 météore
    "#4fd8e8",  # 6 minerai
    "#ff5555",  # 7 balle
    "#7dd956",  # 8 courbe « meilleur »
    "#5f8fdd",  # 9 courbe « moyenne »
    "#2c3440",  # 10 axes des courbes
)

#: Échelle du rendu : 10 unités du monde par pixel de grille.
U_PER_PIXEL = 10.0
GRID_W = int(WORLD_W / U_PER_PIXEL)   # 396
GRID_H = int(WORLD_H / U_PER_PIXEL)   # 354

#: Grille des courbes de fitness (même palette).
CHART_W = 198
CHART_H = 72

#: Commande neutre (départ à quai, phases d'accostage : rien).
IDLE_CMD = {"up": False, "down": False, "left": False, "right": False,
            "fire": False}


def blank_grid() -> list[list[int]]:
    """Grille vide (fond)."""
    return [[0] * GRID_W for _ in range(GRID_H)]


def world_to_pixel(x: float, y: float) -> tuple[int, int]:
    """Monde → pixel : la station (0, 0) est au **centre** de la grille et le
    monde torique boucle aux bords."""
    px = int((x + WORLD_W / 2.0) / U_PER_PIXEL) % GRID_W
    py = int((y + WORLD_H / 2.0) / U_PER_PIXEL) % GRID_H
    return px, py


def plot(grid: list[list[int]], px: int, py: int, color: int) -> None:
    """Pose un pixel dans les bornes de la grille passée (le monde 396×354,
    la vue ANSI ou les courbes) - hors bornes : ignoré."""
    if 0 <= py < len(grid) and 0 <= px < len(grid[0]):
        grid[py][px] = color


def disc(grid: list[list[int]], px: float, py: float, radius: float,
         color: int) -> set[tuple[int, int]]:
    """Disque plein ; renvoie les cellules posées (pour l'effacement)."""
    cells: set[tuple[int, int]] = set()
    r = max(0, int(round(radius)))
    h, w = len(grid), len(grid[0])
    for dy in range(-r, r + 1):
        span = int(math.sqrt(r * r - dy * dy))
        for dx in range(-span, span + 1):
            x, y = int(px) + dx, int(py) + dy
            if 0 <= x < w and 0 <= y < h:
                grid[y][x] = color
                cells.add((x, y))
    return cells


def line(grid: list[list[int]], x0: int, y0: int, x1: int, y1: int,
         color: int) -> None:
    """Segment (Bresenham) - les courbes de fitness."""
    dx = abs(x1 - x0)
    dy = -abs(y1 - y0)
    sx = 1 if x0 < x1 else -1
    sy = 1 if y0 < y1 else -1
    err = dx + dy
    while True:
        plot(grid, x0, y0, color)
        if x0 == x1 and y0 == y1:
            return
        e2 = 2 * err
        if e2 >= dy:
            err += dy
            x0 += sx
        if e2 <= dx:
            err += dx
            y0 += sy


# ── rendu d'une observation (mêmes champs que `/obs` du jeu) ─────────────────

def meteor_map(obs: dict[str, Any]) -> dict[tuple[int, int], int]:
    """Météores vivants de l'observation, par **cellule** de grille (position
    absolue : le vaisseau + le delta torique publié) → rayon en pixels. Le
    champ minier est inerte : une cellule identifie un météore de façon stable."""
    ship = obs.get("ship", {})
    x0, y0 = float(ship.get("x", 0.0)), float(ship.get("y", 0.0))
    out: dict[tuple[int, int], int] = {}
    for n in obs.get("nearby", ()):
        if n.get("kind") != "meteore" or n.get("life", 1) <= 0:
            continue
        px, py = world_to_pixel((x0 + float(n["dx"])) % WORLD_W,
                                (y0 + float(n["dy"])) % WORLD_H)
        out[(px, py)] = max(1, int(round(float(n.get("radius", 0.0)) / U_PER_PIXEL)))
    return out


def draw_static(obs: dict[str, Any]) -> tuple[list[list[int]], frozenset[tuple[int, int]]]:
    """Calque **fixe** de l'épisode : fond + station + météores (inertes).
    Renvoie la grille et ses cellules non-fond (pour retoucher un météore
    détruit sans tout repeindre)."""
    base = blank_grid()
    cells: set[tuple[int, int]] = set()
    cx, cy = GRID_W // 2, GRID_H // 2
    r = max(1, int(round(float(obs.get("station_radius", 162.0)) / U_PER_PIXEL)))
    cells |= disc(base, cx, cy, r, 2)             # liseré
    cells |= disc(base, cx, cy, r - 1, 1)         # coque
    plot(base, cx, cy, 2)                          # la trappe (cercle de 15 u)
    cells.add((cx, cy))
    for (px, py), pr in meteor_map(obs).items():
        cells |= disc(base, px, py, pr, 5)
    return base, frozenset(cells)


def draw_dynamic(grid: list[list[int]], obs: dict[str, Any],
                 cmd: dict[str, bool]) -> set[tuple[int, int]]:
    """Éléments **mobiles** posés sur une copie du calque : minerais, balles,
    vaisseau (corps + nez), flamme si poussée. Renvoie les cellules peintes."""
    cells: set[tuple[int, int]] = set()
    ship = obs.get("ship", {})
    sx, sy = float(ship.get("x", 0.0)), float(ship.get("y", 0.0))
    o = float(ship.get("orientation", 0.0))
    for n in obs.get("nearby", ()):
        if n.get("kind") == "minerai" and n.get("life", 1) > 0:
            px, py = world_to_pixel((sx + float(n["dx"])) % WORLD_W,
                                    (sy + float(n["dy"])) % WORLD_H)
            plot(grid, px, py, 6)
            cells.add((px, py))
    for b in obs.get("bullets", ()):
        if b.get("life", 1) > 0:
            px, py = world_to_pixel((sx + float(b["dx"])) % WORLD_W,
                                    (sy + float(b["dy"])) % WORLD_H)
            plot(grid, px, py, 7)
            cells.add((px, py))
    if cmd.get("up") and not obs.get("docked"):
        # la flamme derrière le nez (l'observation publie le monde en y vers le bas)
        px, py = world_to_pixel(sx - math.cos(o) * 16.0, sy - math.sin(o) * 16.0)
        plot(grid, px, py, 4)
        cells.add((px, py))
    px, py = world_to_pixel(sx, sy)
    plot(grid, px, py, 3)
    cells.add((px, py))
    nx, ny = world_to_pixel(sx + math.cos(o) * 14.0, sy + math.sin(o) * 14.0)
    plot(grid, nx, ny, 3)
    cells.add((nx, ny))
    return cells


def hud_lines(obs: dict[str, Any]) -> str:
    """Ligne d'état de l'épisode (texte sous la grille, pas des pixels)."""
    ship = obs.get("ship", {})
    outcome = obs.get("episode_outcome")
    if outcome == OUTCOME_DELIVERED:
        den = "livré ✔"
    elif outcome == OUTCOME_DESTROYED:
        den = "détruit ✘"
    elif obs.get("episode_done"):
        den = "délai dépassé"
    elif obs.get("docked"):
        den = "à quai"
    else:
        den = "en vol"
    return (f"t = {float(obs.get('episode_t', 0.0)):5.1f} s · {den} · "
            f"vitesse {float(ship.get('speed', 0.0)):5.1f} u/s · "
            f"soute {int(obs.get('cargo_qty', 0))}/{int(obs.get('cargo_cap', 5))} · "
            f"carburant {float(obs.get('fuel', 0.0)):3.0f}/{float(obs.get('fuel_cap', 0.0)):.0f} · "
            f"munitions {int(obs.get('ammo', 0))}/{int(obs.get('ammo_cap', 0))} · "
            f"météores détruits {int(obs.get('meteors_destroyed', 0))}")


# ── rejeu d'un épisode (le même moteur que `ga.run_sim_episode`) ─────────────

def record_episode(policy: Any, seed: int, timeout: float, *,
                   stride: int = 2) -> tuple[list[tuple[dict[str, Any], dict[str, bool]]],
                                             dict[str, Any]]:
    """Joue un épisode dans le simulateur en **enregistrant** un échantillon
    d'observations (1 frame sur `stride`, plus la dernière) : la même boucle
    que `ga.run_sim_episode`, mais qui regarde. Renvoie (échantillons
    `(obs, cmd)`, info de dénouement) - le rejeu est déterministe : c'est
    exactement l'épisode joué par l'entraînement."""
    sim = ShipSim(timeout=timeout)
    obs = sim.reset(seed)
    frames: list[tuple[dict[str, Any], dict[str, bool]]] = [(obs, dict(IDLE_CMD))]
    while not sim.done and sim.t < timeout:
        cmd = dict(policy(obs) or IDLE_CMD)
        obs = sim.step(cmd["up"], cmd["down"], cmd["left"], cmd["right"], cmd["fire"])
        if sim.frame % stride == 0 or sim.done:
            frames.append((obs, cmd))
    outcome = episode_outcome(sim)
    info = {"seed": seed, "outcome": outcome["outcome"],
            "reward": episode_reward(None, outcome), "seconds": outcome["seconds"],
            "success": outcome["success"]}
    return frames, info


# ── session d'affichage (file d'événements → écran) ──────────────────────────

class TrainingSession:
    """Fenêtre d'affichage d'un entraînement (ou de rejeux). Les événements
    arrivent par une **file** depuis le fil d'arrière-plan (`push_*`) ; la
    boucle d'affichage reste au fil principal (tkinter l'exige). Sans tkinter
    (ou `--backend ansi`), le même protocole est rendu dans le terminal.

    Protocol des événements : `("status", texte)`, `("gen", gen, meilleur,
    moyenne|None, sigma|None, frames, info)`, `("candidat", gen, index,
    fitness, frames, info)`, `("result", texte, frames, info)`,
    `("error", texte)`, puis `None` (fin de l'entraînement)."""

    def __init__(self, *, gens: int = 0, zoom: int = 2, speed: int = 4,
                 backend: str = "auto",
                 title: str = "Afficheur pixels — vaisseau (GA)") -> None:
        self.gens = gens
        self.zoom = max(1, zoom)
        self.speed = max(1, speed)
        self.backend = backend
        self.title = title
        self.queue: "queue.Queue[Any]" = queue.Queue()
        self._idle = threading.Event()
        self._stopped = False
        # état partagé (écrit par le fil producteur via _apply_event)
        self._status = "en attente de l'entraînement…"
        self._result_text = ""
        self._last_sigma: Optional[float] = None
        self._history: list[tuple[int, float, Optional[float]]] = []
        self._current: Optional[dict[str, Any]] = None
        self._pending: Optional[tuple[Any, dict[str, Any], str]] = None
        #: lecture de moindre priorité : le dernier candidat ordinaire
        #: échantillonné (une génération qui arrive passe toujours devant)
        self._regular: Optional[tuple[Any, dict[str, Any], str]] = None
        # rendu (fil d'affichage seulement)
        self._paused = False
        self._chart_dirty = False
        self._need_repaint = False
        self._base = blank_grid()
        self._base_cells: set[tuple[int, int]] = set()
        self._meteors: dict[tuple[int, int], int] = {}
        self._prev_dyn: set[tuple[int, int]] = set()
        self._last_render = 0.0

    # ── producteur (fil d'entraînement) ─────────────────────────────────
    def push_status(self, text: str) -> None:
        self.queue.put(("status", text))

    def push_generation(self, gen: int, best: float, mean: Optional[float],
                        sigma: Optional[float], frames: Any, info: dict) -> None:
        self.queue.put(("gen", gen, best, mean, sigma, frames, info))

    def push_candidate(self, gen: int, index: int, score: float,
                       frames: Any, info: dict) -> None:
        """Épisode d'un candidat ordinaire (échantillonné) : lecture de
        moindre priorité, qu'une génération qui arrive interrompt toujours."""
        self.queue.put(("candidat", gen, index, score, frames, info))

    def push_result(self, text: str, frames: Any, info: dict) -> None:
        self.queue.put(("result", text, frames, info))

    def push_error(self, text: str) -> None:
        self.queue.put(("error", text))

    def close(self) -> None:
        """Fin de l'entraînement (la fenêtre reste ouverte)."""
        self.queue.put(None)

    def wait_idle(self, poll: float = 0.1) -> None:
        """Bloque le producteur jusqu'à la fin de la lecture en cours
        (le mode autonome rejoue les épisodes l'un après l'autre)."""
        while not self._idle.wait(poll):
            if self._stopped:
                return

    # ── consommation des événements (fil d'affichage) ───────────────────
    def _drain_queue(self) -> None:
        try:
            while True:
                ev = self.queue.get_nowait()
                self._apply_event(ev)
                if ev is None:
                    break
        except queue.Empty:
            pass

    def _apply_event(self, ev: Any) -> None:
        if ev is None:
            if "erreur" not in self._status:
                self._status = "entraînement terminé"
            return
        kind = ev[0]
        if kind == "status":
            self._status = ev[1]
        elif kind == "gen":
            _, gen, best, mean, sigma, frames, info = ev
            self._history.append((gen, best, mean))
            self._last_sigma = sigma
            self._chart_dirty = True
            label = f"génération {gen}" + (f"/{self.gens}" if self.gens else "")
            slot = (frames, info, label)
            if self._current is None:
                self._start(slot)
            else:
                self._pending = slot   # la plus récente gagne
        elif kind == "result":
            _, text, frames, info = ev
            self._result_text = text
            slot = (frames, info, "validation")
            if self._current is None:
                self._start(slot)
            else:
                self._pending = slot
        elif kind == "candidat":
            _, gen, index, score, frames, info = ev
            slot = (frames, info, f"candidat #{index} · fit {score:.1f}")
            if self._current is None:
                self._start(slot)
            else:
                self._regular = slot   # la plus récente gagne
        elif kind == "error":
            self._status = "erreur : " + ev[1]

    def _start(self, slot: tuple[Any, dict[str, Any], str]) -> None:
        frames, info, label = slot
        self._current = {"frames": frames, "idx": 0, "info": info, "label": label}
        obs0 = frames[0][0]
        self._base, cells = draw_static(obs0)
        self._base_cells = set(cells)
        self._meteors = meteor_map(obs0)
        self._prev_dyn = set()
        self._need_repaint = True

    def _update_static(self, obs: dict[str, Any]) -> set[tuple[int, int]]:
        """Retouche du calque fixe : un météore **détruit** est effacé (les
        minerais prennent sa place). Si `nearby` est tronqué (32 objets), on
        ne peut pas distinguer mort et hors liste : on garde alors les vus."""
        seen = meteor_map(obs)
        repaint: set[tuple[int, int]] = set()
        if len(obs.get("nearby", ())) < 32:
            for key, radius in self._meteors.items():
                if key not in seen:
                    cells = disc(self._base, key[0], key[1], radius, 0)
                    self._base_cells -= cells
                    repaint |= cells
            self._meteors = dict(seen)
        else:
            self._meteors.update(seen)
        return repaint

    def _stats_lines(self) -> list[str]:
        lines = [f"état : {self._status}"]
        if self._history:
            gen, best, mean = self._history[-1]
            total = f"/{self.gens}" if self.gens else ""
            lines.append(f"génération {gen}{total}")
            lines.append(f"meilleur {best:9.1f}")
            if mean is not None:
                lines.append(f"moyenne {mean:9.1f}")
            if self._last_sigma is not None:
                lines.append(f"σ mutation {self._last_sigma:.3f}")
        if self._result_text:
            lines.append(self._result_text)
        return lines

    # ── fenêtre tkinter ──────────────────────────────────────────────────
    def run(self, auto_quit_ms: Optional[int] = None) -> None:
        """Boucle d'affichage au **fil principal** ; les événements du fil
        d'entraînement sont consommés par `after` (jamais de tk ailleurs).
        `auto_quit_ms` ferme la fenêtre seule (tests / CI)."""
        if self.backend == "ansi":
            self._run_ansi(auto_quit_ms)
            return
        try:
            import tkinter as tk
        except ImportError:
            self._run_ansi(auto_quit_ms)
            return
        self._tk = tk
        root = tk.Tk()
        self._root = root
        root.title(self.title)
        self._build(root)
        root.protocol("WM_DELETE_WINDOW", self._on_close)
        if auto_quit_ms:
            root.after(auto_quit_ms, self._on_close)
        root.after(33, self._tick_gui)
        root.mainloop()
        self._stopped = True
        self._idle.set()

    def _build(self, root: Any) -> None:
        tk = self._tk
        z = self.zoom
        left = tk.Frame(root)
        left.pack(side="left", padx=8, pady=8)
        canvas = tk.Canvas(left, width=GRID_W * z, height=GRID_H * z,
                           bg=PALETTE[0], highlightthickness=0)
        canvas.pack()
        self._img = tk.PhotoImage(width=GRID_W * z, height=GRID_H * z)
        canvas.create_image(0, 0, image=self._img, anchor="nw")
        self._hud = tk.Label(left, font=("Courier", 10), anchor="w", text="…")
        self._hud.pack(fill="x")

        side = tk.Frame(root)
        side.pack(side="left", fill="y", padx=(0, 8), pady=8)
        self._stats = tk.Label(side, font=("Courier", 11), justify="left",
                               anchor="w", text="")
        self._stats.pack(fill="x")
        chart = tk.Canvas(side, width=CHART_W * z, height=CHART_H * z,
                          bg=PALETTE[0], highlightthickness=0)
        chart.pack(pady=(8, 0))
        self._chart_img = tk.PhotoImage(width=CHART_W * z, height=CHART_H * z)
        chart.create_image(0, 0, image=self._chart_img, anchor="nw")

        controls = tk.Frame(side)
        controls.pack(pady=(8, 0), fill="x")
        self._pause_btn = tk.Button(controls, text="Pause", width=9,
                                    command=self._toggle_pause)
        self._pause_btn.pack(side="left")
        tk.Button(controls, text="Passer »", width=9,
                  command=self._skip).pack(side="left", padx=4)
        tk.Button(controls, text="Quitter", width=9,
                  command=self._on_close).pack(side="left")
        speeds = tk.Frame(side)
        speeds.pack(pady=(6, 0), fill="x")
        tk.Label(speeds, text="vitesse").pack(side="left")
        self._speed_var = tk.IntVar(value=self.speed)
        for s in (1, 2, 4, 8):
            tk.Radiobutton(speeds, text=f"×{s}", value=s, variable=self._speed_var,
                           command=lambda: setattr(self, "speed",
                                                   self._speed_var.get())).pack(side="left")

        legend = tk.Canvas(side, width=230, height=8 * 17 + 4, highlightthickness=0)
        legend.pack(pady=(8, 0))
        entries = ((1, "station (+ trappe)"), (3, "vaisseau + nez"),
                   (4, "flamme de poussée"), (5, "météore"),
                   (6, "minerai libéré"), (7, "balle"),
                   (8, "fitness : meilleur"), (9, "fitness : moyenne"))
        for i, (color, text) in enumerate(entries):
            y = 4 + i * 17
            legend.create_rectangle(4, y, 16, y + 11, fill=PALETTE[color], width=0)
            legend.create_text(24, y + 5, text=text, anchor="w")

    def _on_close(self) -> None:
        self._stopped = True
        self._idle.set()
        self._root.destroy()

    def _toggle_pause(self) -> None:
        self._paused = not self._paused
        self._pause_btn.config(text="Reprendre" if self._paused else "Pause")

    def _skip(self) -> None:
        """Passe immédiatement à la lecture en attente (génération d'abord,
        sinon candidat ordinaire)."""
        slot = self._pending or self._regular
        if slot is None:
            return
        self._pending = None
        self._regular = None
        self._current = None
        self._idle.set()
        self._start(slot)

    def _finish_current(self) -> None:
        """Fin de la lecture en cours : la génération en attente passe devant
        le candidat ordinaire en attente ; sinon le plus récent des deux, ou
        la main rendue au producteur."""
        if self._pending is not None:
            slot = self._pending
            self._pending = None
            self._start(slot)
        elif self._regular is not None:
            slot = self._regular
            self._regular = None
            self._start(slot)
        else:
            self._current = None
            self._idle.set()

    def _blit(self, cells: set[tuple[int, int]], grid: list[list[int]]) -> None:
        z = self.zoom
        put = self._img.put
        for px, py in cells:
            put(PALETTE[grid[py][px]],
                to=(px * z, py * z, (px + 1) * z, (py + 1) * z))

    def _tick_gui(self) -> None:
        self._drain_queue()
        if self._need_repaint:
            self._need_repaint = False
            z = self.zoom
            self._img.put(PALETTE[0], to=(0, 0, GRID_W * z, GRID_H * z))
            self._blit(self._base_cells, self._base)
        if self._current is not None and not self._paused:
            cur = self._current
            cur["idx"] += self.speed
            obs, cmd = cur["frames"][min(cur["idx"], len(cur["frames"]) - 1)]
            erase = self._update_static(obs)
            grid = [row[:] for row in self._base]
            dyn = draw_dynamic(grid, obs, cmd)
            self._blit(erase | self._prev_dyn | dyn, grid)
            self._prev_dyn = dyn
            self._hud.config(text=f"{cur['label']} · graine {cur['info'].get('seed')} — "
                                  + hud_lines(obs))
            if cur["idx"] >= len(cur["frames"]):
                self._finish_current()
        self._stats.config(text="\n".join(self._stats_lines()))
        if self._chart_dirty:
            self._chart_dirty = False
            self._redraw_chart()
        try:
            self._root.after(33, self._tick_gui)
        except self._tk.TclError:  # fenêtre fermée
            pass

    def _redraw_chart(self) -> None:
        z = self.zoom
        g = [[0] * CHART_W for _ in range(CHART_H)]
        hist = self._history
        if hist:
            vals = []
            for _, best, mean in hist:
                vals.append(best)
                if mean is not None:
                    vals.append(mean)
            lo, hi = min(vals), max(vals)
            if hi - lo < 1e-9:
                hi = lo + 1.0
            pad = (hi - lo) * 0.1
            lo, hi = lo - pad, hi + pad
            x0, x1 = 6, CHART_W - 3
            y0, y1 = CHART_H - 4, 3

            def px(i: int) -> int:
                return x0 + int(round((x1 - x0) * i / max(1, len(hist) - 1)))

            def py(v: float) -> int:
                return y0 + int(round((y1 - y0) * (v - lo) / (hi - lo)))

            for x in range(x0 - 2, x1 + 2):
                g[CHART_H - 2][x] = 10
            for y in range(y1 - 1, y0 + 2):
                g[y][2] = 10

            def serie(idx: int, color: int) -> None:
                prev: Optional[tuple[int, int]] = None
                for i, entry in enumerate(hist):
                    v = entry[1 + idx]
                    if v is None:
                        continue
                    cur = (px(i), py(v))
                    if prev is None:
                        plot(g, cur[0], cur[1], color)
                    else:
                        line(g, prev[0], prev[1], cur[0], cur[1], color)
                    prev = cur
                if prev is not None:
                    plot(g, prev[0], prev[1], color)
                    plot(g, prev[0] + 1, prev[1], color)

            serie(0, 8)
            serie(1, 9)
        self._chart_img.put(PALETTE[0], to=(0, 0, CHART_W * z, CHART_H * z))
        put = self._chart_img.put
        for row_i in range(CHART_H):
            row = g[row_i]
            for col_i in range(CHART_W):
                if row[col_i]:
                    put(PALETTE[row[col_i]],
                        to=(col_i * z, row_i * z, (col_i + 1) * z, (row_i + 1) * z))

    # ── repli terminal (ANSI 24 bits) ────────────────────────────────────
    def _run_ansi(self, auto_quit_ms: Optional[int]) -> None:
        """Boucle de secours **terminal** (sans tkinter, ou `--backend ansi`) :
        vue suivie du vaisseau à 16 u/cellule, ~20 lectures/s."""
        import shutil
        size = shutil.get_terminal_size((110, 44))
        cols, rows = min(99, max(41, size.columns - 2)), 40
        deadline = None if auto_quit_ms is None else time.monotonic() + auto_quit_ms / 1000.0
        self._last_render = 0.0
        out = sys.stdout
        out.write("\x1b[?25l\x1b[2J")  # cache le curseur, efface une fois
        try:
            while not self._stopped and (deadline is None or time.monotonic() < deadline):
                self._drain_queue()
                now = time.monotonic()
                if self._current is None or now - self._last_render < 1.0 / 20.0:
                    time.sleep(0.01)
                    continue
                self._last_render = now
                cur = self._current
                cur["idx"] += self.speed
                obs, cmd = cur["frames"][min(cur["idx"], len(cur["frames"]) - 1)]
                out.write("\x1b[H" + grid_to_ansi(render_ansi(obs, cmd, cols, rows))
                          + "\x1b[0m\n")
                tail = self._stats_lines()
                tail.append(f"{cur['label']} · graine {cur['info'].get('seed')} — "
                            + hud_lines(obs))
                for ln in tail:
                    out.write("\x1b[K" + ln + "\x1b[0m\n")
                out.flush()
                if cur["idx"] >= len(cur["frames"]):
                    self._finish_current()
        finally:
            out.write("\x1b[?25h\x1b[0m\n")
            out.flush()
        self._stopped = True
        self._idle.set()


# ── repli terminal : vue suivie du vaisseau ──────────────────────────────────

def render_ansi(obs: dict[str, Any], cmd: dict[str, bool], cols: int = 99,
                rows: int = 40, u: float = 16.0) -> list[list[int]]:
    """Vue **centrée sur le vaisseau** pour le terminal : une cellule = `u`
    unités, le monde torique boucle par les deltas (la vue ~±800 u reste loin
    du raccord). Même palette que la fenêtre."""
    ship = obs.get("ship", {})
    sx, sy = float(ship.get("x", 0.0)), float(ship.get("y", 0.0))
    o = float(ship.get("orientation", 0.0))
    g = [[0] * cols for _ in range(rows)]
    cx, cy = cols // 2, rows // 2

    def cell(wx: float, wy: float) -> tuple[int, int]:
        dx = ((wx - sx + WORLD_W / 2.0) % WORLD_W) - WORLD_W / 2.0
        dy = ((wy - sy + WORLD_H / 2.0) % WORLD_H) - WORLD_H / 2.0
        return int(round(dx / u)) + cx, int(round(dy / u)) + cy

    scx, scy = cell(float(obs.get("station_x", 0.0)), float(obs.get("station_y", 0.0)))
    sr = float(obs.get("station_radius", 162.0)) / u
    disc(g, scx, scy, sr, 2)
    disc(g, scx, scy, max(0.0, sr - 1.0), 1)
    plot(g, scx, scy, 2)
    for n in obs.get("nearby", ()):
        if n.get("life", 1) <= 0:
            continue
        px, py = cell(sx + float(n["dx"]), sy + float(n["dy"]))
        if n.get("kind") == "meteore":
            disc(g, px, py, max(1.0, float(n.get("radius", 0.0)) / u), 5)
        elif n.get("kind") == "minerai":
            plot(g, px, py, 6)
    for b in obs.get("bullets", ()):
        px, py = cell(sx + float(b["dx"]), sy + float(b["dy"]))
        plot(g, px, py, 7)
    if cmd.get("up") and not obs.get("docked"):
        px, py = cell(sx - math.cos(o) * u * 1.5, sy - math.sin(o) * u * 1.5)
        plot(g, px, py, 4)
    plot(g, cx, cy, 3)
    px, py = cell(sx + math.cos(o) * u * 1.5, sy + math.sin(o) * u * 1.5)
    plot(g, px, py, 3)
    return g


def grid_to_ansi(grid: list[list[int]]) -> str:
    """Grille → texte ANSI 24 bits (fond coloré, plages compressées)."""
    rgb = [tuple(int(h[i:i + 2], 16) for i in (1, 3, 5)) for h in PALETTE]
    out: list[str] = []
    for row in grid:
        parts: list[str] = []
        i = 0
        while i < len(row):
            j = i
            while j < len(row) and row[j] == row[i]:
                j += 1
            r, g_, b = rgb[row[i]]
            parts.append(f"\x1b[48;2;{r};{g_};{b}m" + " " * (j - i))
            i = j
        out.append("".join(parts) + "\x1b[0m")
    return "\n".join(out)


# ── point d'entrée autonome ──────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--policy", default=None, metavar="PATH",
                    help="politique à rejouer (JSON ; défaut : la loi portée "
                         "de l'autopilote du jeu)")
    ap.add_argument("--seed", type=int, default=1, help="graine du premier épisode")
    ap.add_argument("--episodes", type=int, default=0,
                    help="nombre d'épisodes (0 = boucle infinie sur la graine)")
    ap.add_argument("--timeout", type=float, default=120.0, help="délai d'un épisode (s)")
    ap.add_argument("--zoom", type=int, default=2, help="taille d'un pixel à l'écran")
    ap.add_argument("--speed", type=int, default=4, choices=(1, 2, 4, 8),
                    help="vitesse de lecture (1 = temps réel)")
    ap.add_argument("--backend", choices=("auto", "gui", "ansi"), default="auto",
                    help="fenêtre tkinter (auto) ou terminal ANSI")
    ap.add_argument("--quit-after", type=int, default=None, metavar="MS",
                    help="ferme l'affichage après MS ms (tests / CI)")
    args = ap.parse_args()

    if args.policy:
        import policies
        policy = policies.load_policy_file(args.policy)
        label = args.policy
    else:
        import ship_autopilot_ref as law
        policy = law.autopilot_ship_inputs
        label = "loi portée (autopilote du jeu)"

    session = TrainingSession(zoom=args.zoom, speed=args.speed,
                              backend=args.backend,
                              title=f"Afficheur pixels — {label}")

    def work() -> None:
        i = 0
        while args.episodes <= 0 or i < args.episodes:
            frames, info = record_episode(policy, args.seed + i, args.timeout)
            session.push_generation(i, info["reward"], None, None, frames, info)
            session.push_status(f"rejeu : {label}")
            session.wait_idle()
            i += 1
        session.close()

    threading.Thread(target=work, daemon=True, name="viewer-rejeu").start()
    session.run(auto_quit_ms=args.quit_after)


if __name__ == "__main__":
    main()
