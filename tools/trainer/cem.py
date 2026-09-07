#!/usr/bin/env python3
"""Entraînement par **méthode de la croix-entropie (CEM)** du contrôleur
« rentrer à la station » (`policies.seek`, 5 paramètres) sur la tâche EVA.

À chaque génération, des candidats sont tirés autour de la moyenne courante,
évalués sur des épisodes déterministes (graines) du **micro-simulateur**
(`eva_env.py`, même physique que le jeu, instantané), et les meilleurs
deviennent la nouvelle moyenne. La récompense favorise un retour **réussi
et maîtrisé** : +1000 pour la récupération, moins le temps passé, moins une
pénalité quand le cosmonaute entre trop vite dans le cercle d'accostage
(l'autopilote du jeu cherche exactement ce compromis).

La politique apprise (les paramètres) est écrite dans `policy.json` et peut
être rejouée - dans le simulateur comme dans la vraie partie - par
`evaluate.py --policy policy.json`.

Le simulateur permet des centaines d'épisodes en quelques secondes. Un
`--backend live` existe pour entraîner contre la **vraie partie** via
l'interface HTTP (`cargo run` requis) : chaque épisode y dure de vraies
secondes - réduisez le budget (`--pop 4 --gens 3 --seeds 1`).

    python3 cem.py                          # simulateur, budget par défaut
    python3 cem.py --init expert            # départ près de l'autopilote du jeu
    python3 cem.py --backend live           # contre la vraie partie (lent)
"""

from __future__ import annotations

import argparse
import json
import math
import random
import sys
from typing import Any, Optional

from client import DriverClient, die
from eva_env import EvaSim, episode_reward, spawn_position
from policies import SEEK_BOUNDS, SEEK_DEFAULTS, seek

#: Initialisations possibles de la moyenne CEM.
INIT_PRESETS: dict[str, dict[str, float]] = {
    # départ « naïf » : bande d'alignement lâche, plein gaz, aucun
    # ralentissement - le cosmonaute dérive et se met en orbite autour de la
    # base (échec quasi systématique : c'est la difficulté à apprendre)
    "naive": {"turn_db": 0.5, "thrust_db": 0.5, "cruise": 150.0, "slow_zone": 10.0, "band": 60.0},
    # départ « expert » : le réglage robuste de `policies.SEEK_DEFAULTS`
    "expert": dict(SEEK_DEFAULTS),
}

#: Écart-type initial par paramètre (fraction de l'étendue de recherche) :
#: large au départ (le bassin des épisodes réussis est étroit depuis un départ
#: naïf), puis la dispersion se resserre sur les meilleurs candidats.
SIGMA_RATIO = 0.6


def clip_params(p: dict[str, float]) -> dict[str, float]:
    return {k: min(max(v, SEEK_BOUNDS[k][0]), SEEK_BOUNDS[k][1]) for k, v in p.items()}


def evaluate_candidate(
    policy_params: dict[str, float],
    seeds: list[int],
    spawn_dist: float,
    timeout: float,
    sim: bool,
    client: Optional[DriverClient],
) -> float:
    """Récompense moyenne d'un candidat sur plusieurs épisodes (graines)."""
    total = 0.0
    for seed in seeds:
        policy = lambda obs: seek(obs, policy_params)  # noqa: E731
        if sim:
            x, y = spawn_position(seed, spawn_dist)
            env = EvaSim(seed=seed)
            obs = env.reset(seed, x, y)
            while not env.done and env.t < timeout:
                cmd = policy(obs)
                obs = env.step(cmd["up"], cmd["right"], cmd["left"])
            outcome = {
                "success": env.done,
                "seconds": env.t,
                "entry_speed": env.entry_speed if env.done else 0.0,
                "final_dist": obs["station_dist"],
            }
        else:
            outcome = run_live_episode(client, policy_params, seed, spawn_dist, timeout)
        total += episode_reward(None, outcome)
    return total / len(seeds)


def run_live_episode(
    client: DriverClient,
    policy_params: dict[str, float],
    seed: int,
    spawn_dist: float,
    timeout: float,
) -> dict[str, Any]:
    """Un épisode contre la vraie partie (même déroulé que `evaluate.py`)."""
    x, y = spawn_position(seed, spawn_dist)
    client.reset(seed=seed, target="eva", x=x, y=y)
    client.cmd(driver=True)
    last = client.obs().get("frame", 0)
    obs = client.wait_next_obs(last, timeout=5.0)
    while obs.get("pilot") != "eva" or obs.get("station_dist", 0.0) < 15.0:
        last = obs.get("frame", 0)
        obs = client.wait_next_obs(last, timeout=5.0)
    t0 = obs.get("t", 0.0)
    entry_speed = 0.0
    while True:
        if obs.get("eva_recovery", 0.0) > 0.0:
            entry_speed = obs["eva"]["speed"]
            break
        if obs.get("t", t0) - t0 > timeout:
            break
        cmd = seek(obs, policy_params)
        client.cmd(up=cmd["up"], down=cmd["down"], left=cmd["left"],
                   right=cmd["right"], fire=cmd["fire"])
        last = obs.get("frame", 0)
        obs = client.wait_next_obs(last, timeout=5.0)
    seconds = obs.get("t", t0) - t0
    return {
        "success": obs.get("eva_recovery", 0.0) > 0.0,
        "seconds": max(0.0, seconds),
        "entry_speed": entry_speed,
        "final_dist": obs.get("station_dist", 0.0),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backend", choices=("sim", "live"), default="sim",
                    help="simulateur (défaut, instantané) ou vraie partie (lent)")
    ap.add_argument("--host", default="http://127.0.0.1:8643/")
    ap.add_argument("--init", choices=tuple(INIT_PRESETS), default="naive",
                    help="moyenne initiale : naïf (défaut) ou constantes de l'autopilote")
    ap.add_argument("--gens", type=int, default=10, help="nombre de générations")
    ap.add_argument("--pop", type=int, default=20, help="candidats par génération")
    ap.add_argument("--elites", type=int, default=6, help="meilleurs candidats retenus")
    ap.add_argument("--seeds", type=int, default=3, help="épisodes d'évaluation par candidat")
    ap.add_argument("--seed-base", type=int, default=1, help="graine des épisodes d'évaluation")
    ap.add_argument("--spawn-dist", type=float, default=300.0,
                    help="distance du crash au centre de la station")
    ap.add_argument("--timeout", type=float, default=90.0)
    ap.add_argument("--output", default="policy.json")
    args = ap.parse_args()

    if args.elites > args.pop:
        ap.error("--elites doit rester < --pop")

    client: Optional[DriverClient] = None
    if args.backend == "live":
        client = DriverClient(args.host)
        if not client.reachable():
            die("le jeu ne répond pas sur " + args.host)
        print("⚠  Épisode réel = vraies secondes ; réduisez --pop/--gens/--seeds.")

    params = sorted(SEEK_DEFAULTS)
    bounds = SEEK_BOUNDS
    mean = clip_params(dict(INIT_PRESETS[args.init]))
    # écart-type initial : fraction de l'étendue de chaque paramètre
    sigma = {k: (bounds[k][1] - bounds[k][0]) * SIGMA_RATIO for k in params}
    rng = random.Random(args.seed_base * 7919)  # graine de l'optimisation

    seeds = [args.seed_base + i for i in range(args.seeds)]
    print(f"CEM - politique `seek` ({', '.join(params)})")
    print(f"backend : {args.backend}   init : {args.init}   "
          f"{args.gens} générations × {args.pop} candidats × {args.seeds} graines")
    print(f"récompense : +1000 récupéré − 2 s/épisode − pénalité d'arrivée trop rapide\n")

    best: tuple[float, dict[str, float]] = (-1e18, mean)
    for gen in range(1, args.gens + 1):
        if gen == 1:
            # première génération : exploration uniforme de tout l'espace (le
            # bassin des politiques réussies est inconnu - un tirage gaussien
            # autour d'un départ naïf ne l'atteindrait pas)
            candidates = [
                clip_params({k: rng.uniform(bounds[k][0], bounds[k][1]) for k in params})
                for _ in range(args.pop)
            ]
        else:
            candidates = [
                clip_params({k: mean[k] + rng.gauss(0.0, sigma[k]) for k in params})
                for _ in range(args.pop)
            ]
        # élitisme : le meilleur candidat trouvé est réévalué à chaque
        # génération (la moyenne des élites d'une petite population peut
        # sinon « oublier » la meilleure politique entre deux générations)
        candidates.append(dict(best[1]))
        scored: list[tuple[float, dict[str, float]]] = []
        for cand in candidates:
            score = evaluate_candidate(cand, seeds, args.spawn_dist, args.timeout,
                                       args.backend == "sim", client)
            scored.append((score, cand))
        scored.sort(key=lambda t: t[0], reverse=True)
        elites = scored[: args.elites]
        if elites[0][0] > best[0]:
            best = elites[0]
        incumbent = elites[0][0]
        # nouvelle moyenne et dispersion : les meilleurs candidats
        new_mean = {k: sum(e[1][k] for e in elites) / len(elites) for k in params}
        new_sigma = {
            k: math.sqrt(sum((e[1][k] - new_mean[k]) ** 2 for e in elites) / len(elites))
            + 1e-6
            for k in params
        }
        # plancher d'exploration (proportion de l'étendue)
        for k in params:
            new_sigma[k] = max(new_sigma[k], (bounds[k][1] - bounds[k][0]) * 0.03)
        mean, sigma = new_mean, new_sigma
        fmt = " ".join(f"{k}={mean[k]:.3f}" for k in params)
        print(f"gén {gen:>2}   meilleur {incumbent:>8.1f}   "
              f"moyenne élite {sum(e[0] for e in elites) / len(elites):>8.1f}   "
              f"cumulé {best[0]:>8.1f}   μ {fmt}")

    score, pol = best
    print(f"\nMeilleure politique (récompense {score:.1f}) :")
    for k in params:
        print(f"  {k:<12} {pol[k]:.3f}   [bornes {bounds[k][0]:.2f}..{bounds[k][1]:.2f}]")

    payload = {"policy": "seek", "params": {k: round(pol[k], 4) for k in params},
               "task": "eva-return", "spawn_dist": args.spawn_dist,
               "reward": round(score, 2)}
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"Écrite dans {args.output} - rejouable par "
          f"`python3 evaluate.py --policy {args.output}`.")


if __name__ == "__main__":
    main()
