#!/usr/bin/env python3
"""Évalue des stratégies de pilotage sur la tâche « le cosmonaute EVA rentre
à la station » : chaque épisode remet à zéro le monde (graine déterministe),
le vaisseau « explose » à `spawn_dist` de la station et le pilote évalué
doit ramener le cosmonaute dans le cercle d'accostage pour être récupéré.

Mesure la **ligne de base** de l'auto-entraînement :
- `idle`      : ne fait rien (le cosmonaute ne dérive pas) ;
- `random`    : actions tirées au hasard - le hasard ramène-t-il à la base ?
- `seek`      : le contrôleur homing paramétré de `policies.py` (défauts =
  réglage robuste, départ `expert` de l'entraînement) ;
- `autopilot` : l'autopilote **du jeu** (`POST /cmd {"autopilot": true}`) -
  la référence absolue (jeu réel uniquement, pas de simulateur).

`--backend sim` (défaut) utilise le micro-simulateur (`eva_env.py`, instantané,
mêmes lois physiques) ; `--backend live` pilote la **vraie partie** par
l'interface HTTP (`cargo run` requis). Un `--policy` entraîné remplace les
paramètres par défaut de `seek`.

    python3 evaluate.py --backend sim --strategy seek --episodes 5
    python3 evaluate.py --backend live --strategy autopilot --episodes 3
"""

from __future__ import annotations

import argparse
import json
import random
import sys
import time
from typing import Any, Optional

from client import DriverClient, DriverError, die
from eva_env import EvaSim, episode_reward, spawn_position
from policies import LIVE_AUTOPILOT, SIM_STRATEGIES, policy_for, seek

EPISODE_TIMEOUT = 60.0  # secondes avant de déclarer l'épisode perdu


def wait_frame(client: DriverClient, last_frame: int, paced: bool, timeout: float = 5.0) -> dict[str, Any]:
    """Attend la publication de la frame suivante et la renvoie. Quand le
    pilote externe est engagé (`paced`), chaque pas de l'épisode est déclenché
    par une commande - dans le **mode headless** (pas-à-pas accéléré), il faut
    donc poster une commande (même vide) pour faire avancer d'un pas. Contre
    la partie temps réel, ces commandes vides sont sans effet (elles ne
    changent aucune action)."""
    deadline = time.monotonic() + timeout
    while True:
        if paced:
            client.cmd()  # un pas par commande (mode headless)
        o = client.obs()
        if o.get("frame", 0) > last_frame:
            return o
        if time.monotonic() > deadline:
            raise DriverError("aucune nouvelle frame publiée (jeu en pause ?)")
        time.sleep(0.002)


def run_episode_sim(
    env: EvaSim,
    policy,
    seed: int,
    spawn_dist: float,
    timeout: float,
    rng: random.Random,
) -> dict[str, Any]:
    """Un épisode EVA dans le micro-simulateur : succès quand le cosmonaute
    entre dans le cercle d'accostage (récupération), échec au délai."""
    x, y = spawn_position(seed, spawn_dist)
    obs = env.reset(seed, x, y)
    while not env.done and env.t < timeout:
        obs = env.step(**{k: v for k, v in policy(obs).items() if k in ("up", "right", "left")})
    return {
        "success": env.done,
        "seconds": env.t,
        "final_dist": env.obs()["station_dist"],
        "entry_speed": env.entry_speed if env.done else 0.0,
        "reward": 0.0,  # rempli par l'appelant
    }


def run_episode_live(
    client: DriverClient,
    strategy: str,
    policy,
    seed: int,
    spawn_dist: float,
    timeout: float,
) -> dict[str, Any]:
    """Un épisode EVA dans la **vraie partie** (interface HTTP) : succès dès
    que le jeu déclenche la récupération (`eva_recovery > 0` - même instant
    que l'entrée dans le cercle du simulateur)."""
    x, y = spawn_position(seed, spawn_dist)
    client.reset(seed=seed, target="eva", x=x, y=y)
    if strategy == LIVE_AUTOPILOT:
        client.cmd(autopilot=True)  # l'ordinateur du jeu pilote
        paced = False  # l'autopilote joue seul : les frames s'enchaînent
    else:
        client.cmd(driver=True)  # le pilote externe (évalué) pilote
        paced = True  # un pas par commande (mode headless) : il faut piloter
    last = client.obs().get("frame", 0)
    obs = wait_frame(client, last, paced)
    # attendre que l'épisode soit réellement en place (cosmonaute à l'écran)
    while obs.get("pilot") != "eva" or obs.get("station_dist", 0.0) < 15.0:
        last = obs.get("frame", 0)
        obs = wait_frame(client, last, paced)
    t0 = obs.get("t", 0.0)
    entry_speed = 0.0
    while True:
        if obs.get("eva_recovery", 0.0) > 0.0:
            # récupération déclenchée : épisode réussi (le reste est animé)
            entry_speed = obs["eva"]["speed"]
            break
        if obs.get("t", t0) - t0 > timeout:
            break
        if policy is not None:
            cmd = policy(obs)
            client.cmd(
                up=cmd["up"], down=cmd["down"], left=cmd["left"],
                right=cmd["right"], fire=cmd["fire"],
            )
        last = obs.get("frame", 0)
        # la commande ci-dessus a déclenché le pas (mode headless) : il ne
        # reste qu'à attendre la frame publiée
        obs = client.wait_next_obs(last, timeout=5.0)
    seconds = obs.get("t", t0) - t0
    return {
        "success": obs.get("eva_recovery", 0.0) > 0.0,
        "seconds": max(0.0, seconds),
        "final_dist": obs.get("station_dist", 0.0),
        "entry_speed": entry_speed,
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backend", choices=("sim", "live"), default="sim",
                    help="simulateur (défaut, instantané) ou vraie partie (serveur du jeu)")
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--strategy", default="seek", choices=SIM_STRATEGIES + (LIVE_AUTOPILOT,))
    ap.add_argument("--policy", default=None, metavar="policy.json",
                    help="paramètres d'un `seek` entraîné (sortie de cem.py)")
    ap.add_argument("--episodes", type=int, default=5)
    ap.add_argument("--seed", type=int, default=1, help="graine du premier épisode (les suivants +1)")
    ap.add_argument("--spawn-dist", type=float, default=300.0,
                    help="distance du crash au centre de la station (unités)")
    ap.add_argument("--timeout", type=float, default=EPISODE_TIMEOUT)
    args = ap.parse_args()

    params: Optional[dict[str, float]] = None
    if args.strategy == "seek" and args.policy:
        with open(args.policy, encoding="utf-8") as f:
            data = json.load(f)
        params = data.get("params")
        print(f"Politique chargée : {data.get('policy', 'seek')} "
              f"(paramètres de l'entraînement)")

    # politique évaluée (None = l'autopilote du jeu pilote lui-même)
    rng = random.Random(args.seed)
    if args.strategy == LIVE_AUTOPILOT and args.backend == "sim":
        ap.error("`autopilot` vit dans le jeu - utilisez --backend live")
    if args.strategy == "seek":
        policy = lambda obs: seek(obs, params)  # noqa: E731 - paramètres entraînés ou défauts
    elif args.strategy == LIVE_AUTOPILOT:
        policy = None
    else:
        policy = policy_for(args.strategy, rng)

    if args.backend == "sim":
        env = EvaSim()
        results = [run_episode_sim(env, policy, args.seed + i, args.spawn_dist,
                                   args.timeout, rng) for i in range(args.episodes)]
    else:
        client = DriverClient(args.host)
        if not client.reachable():
            die("le jeu ne répond pas sur " + args.host)
        results = [run_episode_live(client, args.strategy, policy, args.seed + i,
                                    args.spawn_dist, args.timeout)
                   for i in range(args.episodes)]

    # rapport
    print(f"\nTâche : cosmonaute EVA → station (départ à {args.spawn_dist:.0f} u)")
    print(f"Stratégie : {args.strategy}   backend : {args.backend}   "
          f"{args.episodes} épisodes (graines {args.seed}..{args.seed + args.episodes - 1})")
    print("-" * 78)
    print(f"{'épisode':>8} {'graine':>7} {'succès':>7} {'temps (s)':>10} "
          f"{'dist fin.':>9} {'vitesse fin.':>12} {'récompense':>10}")
    for i, (r, ep) in enumerate(zip(results, range(args.episodes))):
        r = dict(r)
        r["reward"] = episode_reward(None, r)
        print(f"{i + 1:>8} {args.seed + ep:>7} {str(r['success']):>7} "
              f"{r['seconds']:>10.1f} {r['final_dist']:>9.0f} "
              f"{r['entry_speed']:>12.1f} {r['reward']:>10.1f}")
    print("-" * 78)
    ok = sum(1 for r in results if r["success"])
    mean_t = sum(r["seconds"] for r in results if r["success"]) / max(1, ok)
    mean_r = sum(episode_reward(None, r) for r in results) / len(results)
    print(f"{ok}/{len(results)} épisodes réussis   temps moyen : {mean_t:.1f} s   "
          f"récompense moyenne : {mean_r:.1f}")
    if ok == 0:
        print("Aucun succès : le délai est-il assez long (--timeout) ? Le départ "
              "assez proche (--spawn-dist) ?")


if __name__ == "__main__":
    main()
