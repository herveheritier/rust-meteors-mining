#!/usr/bin/env python3
"""Client de l'interface d'auto-entraînement du pilote (côté jeu : `src/driver.rs`).

Le jeu expose un serveur HTTP localhost (port 8643 par défaut) :
- `GET  /obs`    observation de la dernière frame (JSON),
- `POST /cmd`    actions de la frame + bascules driver / autopilot,
- `POST /reset`  remise à zéro d'un épisode déterministe (graine).

Ce module est le pendant **indépendant de l'application** : un entraîneur
(simulateur, CEM, plus tard un réseau) pilote le jeu à travers ce protocole.
Python standard uniquement (`urllib`), aucune dépendance.

    from client import DriverClient
    c = DriverClient()
    c.reset(seed=42, target="eva", x=400.0, y=250.0)
    obs = c.wait_next_obs()
    c.cmd(up=True, right=True)
"""

from __future__ import annotations

import json
import time
import urllib.request
from typing import Any, Optional


class DriverError(RuntimeError):
    """Le serveur n'a pas répondu comme attendu (hors ligne, code HTTP, JSON illisible)."""


class DriverClient:
    """Accès au serveur de contrôle du jeu (`src/driver.rs`)."""

    def __init__(self, base_url: str = "http://127.0.0.1:8643/") -> None:
        self.base = base_url if base_url.endswith("/") else base_url + "/"
        if not self.base.startswith("http://"):
            raise DriverError(f"URL invalide (localhost uniquement) : {base_url}")

    # ── primitives HTTP ─────────────────────────────────────────────────────
    def get(self, path: str, timeout: float = 2.0) -> str:
        try:
            with urllib.request.urlopen(self.base + path.lstrip("/"), timeout=timeout) as r:
                return r.read().decode("utf-8", "replace")
        except OSError as e:  # connexion refusée, délai dépassé…
            raise DriverError(f"serveur injoignable ({self.base + path}) : {e}") from e

    def post(self, path: str, payload: dict[str, Any], timeout: float = 2.0) -> str:
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            self.base + path.lstrip("/"),
            data=data,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                if r.status != 200:
                    raise DriverError(f"{path} → HTTP {r.status}")
                return r.read().decode("utf-8", "replace")
        except OSError as e:
            raise DriverError(f"{path} échoué : {e}") from e

    # ── protocole ───────────────────────────────────────────────────────────
    def reachable(self, timeout: float = 1.0) -> bool:
        try:
            self.get("/", timeout=timeout)
            return True
        except DriverError:
            return False

    def obs(self) -> dict[str, Any]:
        """Dernière observation publiée (`GET /obs`)."""
        body = self.get("/obs")
        try:
            return json.loads(body)
        except json.JSONDecodeError as e:
            raise DriverError(f"réponse /obs illisible : {e}") from e

    def cmd(
        self,
        *,
        up: bool = False,
        down: bool = False,
        left: bool = False,
        right: bool = False,
        fire: bool = False,
        driver: Optional[bool] = None,
        autopilot: Optional[bool] = None,
    ) -> None:
        """Actions de la frame + bascules. `driver: true` engage le pilote
        externe (les actions ci-dessus pilotent) ; `autopilot: true` laisse
        l'ordinateur du jeu piloter (les actions sont alors ignorées)."""
        payload: dict[str, Any] = {"up": up, "down": down, "left": left, "right": right, "fire": fire}
        if driver is not None:
            payload["driver"] = driver
        if autopilot is not None:
            payload["autopilot"] = autopilot
        self.post("/cmd", payload)

    def reset(
        self,
        seed: int,
        target: str = "eva",
        x: float = 0.0,
        y: float = 0.0,
        auto_generate: bool = False,
        scenario: str = "free",
    ) -> None:
        """Remise à zéro d'un épisode déterministe (monde régénéré à la graine
        à la frame suivante). `target` : "eva" (vaisseau détruit à (x, y), le
        pilote est le cosmonaute EVA) ou "ship" (vaisseau à quai).
        `scenario` : "free" (défaut, aucune économie) ou "economy"
        (Progression - la boucle de minage du vaisseau : carburant, crédits,
        soute à livrer)."""
        self.post(
            "/reset",
            {"seed": int(seed), "target": target, "x": float(x), "y": float(y),
             "auto_generate": bool(auto_generate), "scenario": scenario},
        )

    def bench(
        self,
        episodes: int,
        seed: int = 1,
        target: str = "ship",
        x: float = 0.0,
        y: float = 0.0,
        auto_generate: bool = False,
        scenario: str = "free",
        max_steps: Optional[int] = None,
        trajectories: bool = False,
    ) -> None:
        """Demande un **banc d'essai en continu** (`POST /bench`) : le
        processus headless exécute `episodes` épisodes de bout en bout à
        pleine vitesse dans le processus (aucun aller-retour HTTP par pas -
        l'autopilote du jeu joue), puis sert le rapport par `bench_report()`.

        `target` : "eva" (vaisseau détruit à (x, y), le pilote est le
        cosmonaute EVA) ou "ship" (vaisseau à quai - scénario "economy" pour
        la boucle de minage). `max_steps` : garde-fou par épisode (défaut du
        serveur : 120 s de simulation). `trajectories` : enregistrer les
        déroulés (observation + action de l'autopilote à chaque pas) dans un
        fichier JSONL pour l'entraînement RL - le chemin est dans le rapport
        (`trajectory_file`)."""
        payload: dict[str, Any] = {
            "episodes": int(episodes),
            "seed": int(seed),
            "target": target,
            "x": float(x),
            "y": float(y),
            "auto_generate": bool(auto_generate),
            "scenario": scenario,
            "trajectories": bool(trajectories),
        }
        if max_steps is not None:
            payload["max_steps"] = int(max_steps)
        self.post("/bench", payload)

    def bench_report(self) -> dict[str, Any]:
        """Rapport du dernier banc d'essai (`GET /bench`), `{}` tant qu'aucun
        banc n'a été demandé ou terminé : déroulé par épisode (graine,
        dénouement, pas, secondes) + agrégats (cadence en épisodes/s, temps
        mur, répartition des dénouements)."""
        body = self.get("/bench")
        try:
            return json.loads(body)
        except json.JSONDecodeError as e:
            raise DriverError(f"réponse /bench illisible : {e}") from e

    def wait_bench(self, timeout: float = 120.0, poll: float = 0.01) -> dict[str, Any]:
        """Attend la fin du banc d'essai demandé et renvoie le rapport
        (poll `GET /bench` tant qu'aucun rapport n'est publié). Le temps mur
        dépend du nombre d'épisodes et de leur longueur simulée."""
        deadline = time.monotonic() + timeout
        while True:
            rep = self.bench_report()
            if rep:
                return rep
            if time.monotonic() > deadline:
                raise DriverError("banc d'essai trop long (délai dépassé)")
            time.sleep(poll)

    def load_trajectories(self, path: str) -> list[dict[str, Any]]:
        """Charge un fichier de trajectoires JSONL écrit par un banc d'essai
        (`trajectories` demandé, chemin dans le rapport) : une entrée par
        ligne - `episode` (bornes), `step` (observation + action de
        l'autopilote à ce pas) et `episode_end` (dénouement + récompense).
        Destiné à l'entraînement RL hors-ligne sur les décisions de la ligne
        de base."""
        events: list[dict[str, Any]] = []
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    events.append(json.loads(line))
        return events

    def wait_next_obs(self, last_frame: int = 0, timeout: float = 5.0, poll: float = 0.002) -> dict[str, Any]:
        """Attend la publication de la frame suivante (le jeu publie une
        observation par frame) et la renvoie. `last_frame` = frame déjà vue."""
        deadline = time.monotonic() + timeout
        while True:
            o = self.obs()
            if o.get("frame", 0) > last_frame:
                return o
            if time.monotonic() > deadline:
                raise DriverError("aucune nouvelle frame publiée (jeu en pause ?)")
            time.sleep(poll)


def die(message: str, hint: bool = True) -> None:
    """Message d'erreur propre quand le jeu n'est pas joignable."""
    print(f"✗ {message}")
    if hint:
        print("  Le jeu doit tourner avec l'interface d'auto-entraînement")
        print("  (`cargo run` - serveur sur http://127.0.0.1:8643/)")
        print("  ou en mode headless accéléré sans fenêtre : `cargo run -- --headless`.")
    raise SystemExit(1)
