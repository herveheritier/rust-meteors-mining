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
- `nn`        : le **réseau de neurones** entraîné hors-ligne par imitation de
  l'autopilote (`imitate.py` - `--policy nn_policy.json` requis) ;
- `ppo`       : la **politique PPO** entraînée par renforcement sur
  l'observation complète (`ppo.py` - `--policy ppo_policy.json` requis) ;
- `autopilot` : l'autopilote **du jeu** (`POST /cmd {"autopilot": true}`) -
  la référence absolue (jeu réel uniquement, pas de simulateur) ;
- `autopilot_sim` : le **portage Python** de cet autopilote
  (`autopilot_ref.py`) - la même référence, mais **en simulateur** (aucun
  processus de jeu). `--reference` compare la politique évaluée à ce
  portage sur les mêmes graines et affiche l'**écart de récompense**.

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
from policies import LIVE_AUTOPILOT, SIM_STRATEGIES, nn_policy, policy_for, ppo_policy, seek

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
    entre dans le cercle d'accostage (récupération), échec au délai.

    L'état interne des politiques qui en ont un (l'autopilote porté,
    `autopilot_ref`) est réinitialisé au départ de l'épisode."""
    if hasattr(policy, "reset"):
        policy.reset()  # type: ignore[attr-defined]
    x, y = spawn_position(seed, spawn_dist)
    obs = env.reset(seed, x, y)
    while not env.done and env.t < timeout:
        obs = env.step(**{k: v for k, v in policy(obs).items() if k in ("up", "right", "left")})
    return {
        "seed": seed,
        "success": env.done,
        "seconds": env.t,
        "final_dist": env.obs()["station_dist"],
        "entry_speed": env.entry_speed if env.done else 0.0,
        "reward": episode_reward(None, {
            "success": env.done,
            "seconds": env.t,
            "entry_speed": env.entry_speed if env.done else 0.0,
            "final_dist": env.obs()["station_dist"],
        }),
    }


def sim_comparison(
    policy,
    seeds: list[int],
    spawn_dist: float = 300.0,
    timeout: float = EPISODE_TIMEOUT,
    reference=None,
) -> dict[str, Any]:
    """Compare une politique à la **référence** (par défaut l'autopilote du
    jeu porté en Python, `autopilot_ref.py`) sur les **mêmes graines**, dans
    le micro-simulateur - l'écart de récompense hors-ligne, sans processus
    headless. C'est la mesure du test de non-régression et de `dagger.py`.

    Renvoie les déroulés des deux côtés, les moyennes et l'**écart**
    `politique − référence` (positif = la politique fait mieux)."""
    if reference is None:
        from autopilot_ref import autopilot_ref_policy

        reference = autopilot_ref_policy()
    env = EvaSim()
    rng = random.Random(0)
    pol = [run_episode_sim(env, policy, s, spawn_dist, timeout, rng) for s in seeds]
    ref = [run_episode_sim(env, reference, s, spawn_dist, timeout, rng) for s in seeds]
    p_mean = sum(r["reward"] for r in pol) / len(pol)
    r_mean = sum(r["reward"] for r in ref) / len(ref)
    return {
        "seeds": list(seeds),
        "spawn_dist": spawn_dist,
        "policy": pol,
        "reference": ref,
        "policy_mean": p_mean,
        "reference_mean": r_mean,
        "gap": p_mean - r_mean,
        "policy_ok": sum(1 for r in pol if r["success"]),
        "reference_ok": sum(1 for r in ref if r["success"]),
    }


def print_sim_comparison(cmp: dict[str, Any], policy_label: str = "politique") -> None:
    """Rapport lisible de `sim_comparison` (une ligne par graine + moyennes)."""
    print(f"\nComparaison hors-ligne (simulateur) : {policy_label} vs autopilote du jeu")
    print(f"Départ à {cmp['spawn_dist']:.0f} u · graines "
          f"{cmp['seeds'][0]}..{cmp['seeds'][-1]}")
    print("-" * 66)
    print(f"{'graine':>7} {'politique':>18} {'autopilote':>18}")
    for p, r in zip(cmp["policy"], cmp["reference"]):
        p_out = "récupéré" if p["success"] else "échec"
        r_out = "récupéré" if r["success"] else "échec"
        print(f"{p['seed']:>7} {p_out:>10} {p['reward']:>7.1f} "
              f"{r_out:>10} {r['reward']:>7.1f}")
    print("-" * 66)
    print(f"{policy_label:>7} : {cmp['policy_ok']}/{len(cmp['policy'])} réussis · "
          f"récompense moyenne {cmp['policy_mean']:.1f}")
    print(f"autopilote : {cmp['reference_ok']}/{len(cmp['reference'])} réussis · "
          f"récompense moyenne {cmp['reference_mean']:.1f}")
    sign = "+" if cmp["gap"] >= 0 else ""
    print(f"écart (politique − autopilote) : {sign}{cmp['gap']:.1f}")


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
        # scénario à objectifs (Phase 2) : bonus cumulé des complétions de
        # l'épisode (0 hors épisode à objectifs - mêmes règles que le jeu)
        "objective_bonus": obs.get("objective_bonus", 0.0),
    }


def run_episode_live_episode(
    client: DriverClient,
    policy,
    seed: int,
    target: str,
    x: float,
    y: float,
    auto_generate: bool,
    scenario: str,
    timeout: float,
) -> dict[str, Any]:
    """Un épisode dans la **vraie partie** (interface HTTP) piloté par la
    politique externe (`driver` engagé), sur les **mêmes épisodes que le
    banc d'essai** (même graine, même cible, même position, même scénario) :
    le mode hybride compare ainsi la politique externe à l'autopilote du jeu
    sur des épisodes identiques. Fin = terminaison explicite de l'épisode
    (`episode_done` / `episode_outcome`, disponible dans l'observation) ou
    délai."""
    client.reset(seed=seed, target=target, x=x, y=y,
                 auto_generate=auto_generate, scenario=scenario)
    client.cmd(driver=True)  # le pilote externe (évalué) pilote
    paced = True  # un pas par commande (mode headless) : il faut piloter
    last = client.obs().get("frame", 0)
    obs = wait_frame(client, last, paced)
    t0 = obs.get("t", 0.0)
    entry_speed = 0.0
    outcome = None
    while True:
        if obs.get("episode_done", False):
            # terminaison explicite (delivered / eva_recovered / destroyed)
            outcome = obs.get("episode_outcome")
            if obs.get("eva_recovery", 0.0) > 0.0:
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
        obs = client.wait_next_obs(last, timeout=5.0)
    seconds = obs.get("t", t0) - t0
    return {
        "success": outcome in ("delivered", "eva_recovered", "objectives_complete"),
        "outcome": outcome,
        "seconds": max(0.0, seconds),
        "final_dist": obs.get("station_dist", 0.0),
        "entry_speed": entry_speed,
        # scénario à objectifs (Phase 2) : bonus cumulé des complétions de
        # l'épisode (0 hors épisode à objectifs - mêmes règles que le jeu)
        "objective_bonus": obs.get("objective_bonus", 0.0),
    }


def run_bench_comparison(
    client: DriverClient,
    policy,
    seed: int,
    episodes: int,
    target: str,
    x: float,
    y: float,
    auto_generate: bool,
    scenario: str,
    timeout: float,
) -> None:
    """Mode **hybride** : l'autopilote du jeu joue d'abord le lot **en
    continu dans le processus** (`POST /bench` - centaines d'épisodes/s, la
    mesure de la Phase 2), puis la politique externe rejoue les **mêmes
    épisodes** pas à pas (HTTP) - comparaison épisode par épisode sur des
    épisodes identiques (même graine, même cible, même position, même
    scénario)."""
    # 1) ligne de base : banc d'essai en continu (autopilote du jeu)
    client.bench(episodes=episodes, seed=seed, target=target, x=x, y=y,
                 auto_generate=auto_generate, scenario=scenario)
    bench = client.wait_bench()
    if not bench:
        die("aucun rapport de banc d'essai (le processus headless exécute-t-il le lot ?)")
    # 2) politique externe sur les mêmes épisodes (pas-à-pas HTTP)
    results = [
        run_episode_live_episode(client, policy, seed + i, target, x, y,
                                 auto_generate, scenario, timeout)
        for i in range(episodes)
    ]
    # 3) rapport comparé
    print(f"\nMode hybride : politique externe vs autopilote du jeu - mêmes épisodes")
    print(f"Cible : {target} · scénario : {scenario} · départ ({x:.0f}, {y:.0f}) · "
          f"graines {seed}..{seed + episodes - 1}")
    print("-" * 78)
    print(f"Autopilote (bench en continu) : {bench.get('episodes', episodes)} épisodes "
          f"en {bench.get('wall_seconds', 0.0):.2f} s mur → "
          f"{bench.get('episodes_per_second', 0.0):.0f} épisodes/s · "
          f"récompense moyenne {bench.get('mean_reward', 0.0):.1f}")
    bench_map = {r["seed"]: r for r in bench.get("results", [])}
    print(f"{'graine':>7} {'autopilote':>26} {'politique':>26}")
    print(f"{'':>7} {'dénouement':>12} {'récomp.':>9} {'dénouement':>12} {'récomp.':>9}")
    for i in range(episodes):
        b = bench_map.get(seed + i, {})
        r = results[i]
        b_outcome = b.get("outcome") or "delai"
        p_outcome = r.get("outcome") or "delai"
        print(f"{seed + i:>7} {b_outcome:>12} {b.get('reward', 0.0):>9.1f} "
              f"{p_outcome:>12} {episode_reward(None, r):>9.1f}")
    print("-" * 78)
    p_ok = sum(1 for r in results if r["success"])
    p_mean = sum(episode_reward(None, r) for r in results) / max(1, len(results))
    b_ok = sum(
        1
        for r in bench.get("results", [])
        if r.get("outcome") in ("delivered", "eva_recovered", "objectives_complete")
    )
    print(f"Autopilote : {b_ok}/{bench.get('episodes', episodes)} réussis · "
          f"récompense moyenne {bench.get('mean_reward', 0.0):.1f}")
    print(f"Politique  : {p_ok}/{len(results)} réussis · récompense moyenne {p_mean:.1f} · "
          f"temps moyen {sum(r['seconds'] for r in results if r['success']) / max(1, p_ok):.1f} s")
    if bench.get("trajectory_file"):
        print(f"Trajectoires de l'autopilote (RL) : {bench['trajectory_file']}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backend", choices=("sim", "live", "hybrid"), default="sim",
                    help="simulateur (défaut, instantané), vraie partie pas-à-pas (live) "
                         "ou hybride (bench autopilote + politique externe sur les mêmes épisodes)")
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--strategy", default="seek", choices=SIM_STRATEGIES + (LIVE_AUTOPILOT,))
    ap.add_argument("--policy", default=None, metavar="policy.json",
                    help="politique entraînée : paramètres d'un `seek` (sortie de cem.py) "
                         "ou poids d'un réseau `nn`/`ppo` (sortie d'imitate.py / ppo.py)")
    ap.add_argument("--episodes", type=int, default=5)
    ap.add_argument("--seed", type=int, default=1, help="graine du premier épisode (les suivants +1)")
    ap.add_argument("--spawn-dist", type=float, default=300.0,
                    help="distance du crash au centre de la station (unités)")
    ap.add_argument("--target", choices=("ship", "eva"), default="eva",
                    help="entité pilotée (vaisseau à quai ou cosmonaute EVA éjecté)")
    ap.add_argument("--scenario", default="free",
                    help="règles de l'épisode : free (défaut), economy (boucle de minage "
                         "du vaisseau) ou l'id d'un scénario à objectifs (ex. "
                         "campaign_prospector - missions DAG de l'éditeur)")
    ap.add_argument("--x", type=float, default=None, help="position du crash (mode hybride, défaut 300)")
    ap.add_argument("--y", type=float, default=None, help="position du crash (mode hybride, défaut 0)")
    ap.add_argument("--auto-generate", action="store_true",
                    help="monde vivant (météores générés au fil de l'épisode)")
    ap.add_argument("--timeout", type=float, default=EPISODE_TIMEOUT)
    ap.add_argument("--reference", action="store_true",
                    help="backend sim : comparer aussi à l'autopilote du jeu **porté en "
                         "Python** (autopilot_ref.py) sur les mêmes graines - l'écart de "
                         "récompense, sans processus headless")
    args = ap.parse_args()

    params: Optional[dict[str, float]] = None
    nn_net_path: Optional[str] = None
    ppo_path: Optional[str] = None
    if args.policy:
        with open(args.policy, encoding="utf-8") as f:
            data = json.load(f)
        if data.get("policy") == "nn":
            nn_net_path = args.policy
            print(f"Politique chargée : réseau de neurones (imitation, {args.policy})")
        elif data.get("policy") == "ppo":
            ppo_path = args.policy
            print(f"Politique chargée : politique PPO (renforcement, {args.policy})")
        else:
            params = data.get("params")
            print(f"Politique chargée : {data.get('policy', 'seek')} "
                  f"(paramètres de l'entraînement)")

    # politique évaluée (None = l'autopilote du jeu pilote lui-même)
    rng = random.Random(args.seed)
    if args.strategy == LIVE_AUTOPILOT and args.backend in ("sim", "hybrid"):
        ap.error("`autopilot` vit dans le jeu - utilisez --backend live "
                 "(ou, en hybride, comparez-le à une politique externe)")
    if args.strategy == "seek":
        policy = lambda obs: seek(obs, params)  # noqa: E731 - paramètres entraînés ou défauts
    elif args.strategy == "nn":
        if nn_net_path is None:
            ap.error("--strategy nn exige --policy (sortie d'imitate.py : nn_policy.json)")
        policy = nn_policy(nn_net_path)
    elif args.strategy == "ppo":
        if ppo_path is None:
            ap.error("--strategy ppo exige --policy (sortie de ppo.py : ppo_policy.json)")
        policy = ppo_policy(ppo_path)
    elif args.strategy == LIVE_AUTOPILOT:
        policy = None
    else:
        policy = policy_for(args.strategy, rng)

    if args.backend == "sim":
        env = EvaSim()
        results = [run_episode_sim(env, policy, args.seed + i, args.spawn_dist,
                                   args.timeout, rng) for i in range(args.episodes)]
    elif args.backend == "hybrid":
        client = DriverClient(args.host)
        if not client.reachable():
            die("le jeu ne répond pas sur " + args.host)
        # mode hybride : mêmes épisodes pour l'autopilote (bench en continu)
        # et la politique externe (pas-à-pas) - position de départ fixe
        x = 300.0 if args.x is None else args.x
        y = 0.0 if args.y is None else args.y
        run_bench_comparison(client, policy, args.seed, args.episodes,
                             args.target, x, y, args.auto_generate, args.scenario,
                             args.timeout)
        return
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
    if args.backend == "sim" and args.reference:
        cmp = sim_comparison(policy,
                             list(range(args.seed, args.seed + args.episodes)),
                             args.spawn_dist, args.timeout)
        print_sim_comparison(cmp, args.strategy)
    if ok == 0:
        print("Aucun succès : le délai est-il assez long (--timeout) ? Le départ "
              "assez proche (--spawn-dist) ?")


if __name__ == "__main__":
    main()
