#!/usr/bin/env python3
"""Apprentissage par **DAgger** (Dataset Aggregation) de la politique réseau de
neurones contre l'autopilote du jeu - la suite du clonage comportemental
(`imitate.py`), destinée à combler la **dérive de distribution** mesurée.

Le clonage pur apprend sur les seules trajectoires de l'autopilote : en boucle
fermée, la moindre erreur de la politique déplace la trajectoire hors de la
distribution apprise, et elle n'a jamais appris à s'en rattraper. DAgger
corrige cela en faisant jouer la **politique elle-même** (en boucle ouverte
dans le jeu, pas-à-pas HTTP) et en **étiquetant chaque état qu'elle visite**
avec l'action que prendrait l'autopilote sur cet état - le champ `expert` de
l'observation (`/obs`, calculé par `src/driver.rs` à chaque publication). Ces
(état, action experte) sont **agrégés** au jeu de données, la politique est
ré-entraînée, et on itère : à chaque itération elle apprend à se rattraper
sur les états qu'elle crée elle-même.

    # 1. (optionnel) amorcer le jeu de données avec les trajectoires de
    #    l'autopilote (nécessite un processus headless) :
    python3 bench.py --episodes 12 --target eva --trajectories
    # 2. DAgger : 3 itérations × 6 épisodes à départs variés (300/500/800 u) :
    python3 dagger.py \
        --init-trajectories /tmp/meteors_mining_headless_*/trajectories_*.jsonl \
        --iterations 3 --episodes 6 --target eva
    # 3. comparer la politique DAgger à l'autopilote (mêmes épisodes) :
    python3 evaluate.py --backend hybrid --strategy nn --policy dagger_policy.json

Chaque itération déploie la politique courante (aléatoire au départ, ou la
politique amorcée par `--init-policy`, ou le clone d'`imitate.py`) sur des
**départs variés** autour de la station (distances et directions tirées des
graines - c'est le régime hors distribution que le clonage pur ne tient pas),
récolte les états visités + l'action experte, puis ré-entraîne le MLP sur tout
le jeu de données agrégé. La séparation train/validation se fait **par
épisode** (graines), comme dans `imitate.py`.
"""

from __future__ import annotations

import argparse
import random
import sys
from typing import Any, Optional

from client import DriverClient, DriverError, die
from evaluate import EPISODE_TIMEOUT, run_bench_comparison, wait_frame
from eva_env import episode_reward, spawn_position
from imitate import format_accuracy, load_samples, split_by_seed
from nn import MLP, OUTPUT_COUNT, action_target, obs_features, save_nn
from policies import nn_policy, random_policy

#: Distances de départ (unités) des épisodes EVA, parcourues en cycle : varier
#: la distance d'éjection est le régime **hors distribution** du clonage pur
#: (mesuré : l'imitation ne généralise pas aux départs qu'elle n'a pas vus).
DEFAULT_SPAWN_DISTS = "300,500,800"


def expert_target(expert: dict[str, Any]) -> list[float]:
    """Cibles de l'action experte (l'autopilote du jeu, champ `expert` de
    l'observation) : sigmoïdes (up/down/fire) + un-seul de rotation -
    mêmes cibles que `imitate.py` (`nn.action_target`)."""
    return action_target(expert)


def roll_episode(
    client: DriverClient,
    policy,
    seed: int,
    target: str,
    x: float,
    y: float,
    scenario: str,
    timeout: float,
    stride: int = 1,
) -> tuple[list[tuple[list[float], list[float]]], dict[str, Any]]:
    """Un épisode de la politique courante dans la **vraie partie** (mode
    headless pas-à-pas) : chaque état visité est étiqueté avec l'action
    experte (`obs["expert"]`), puis la politique agit. Renvoie les exemples
    (features, action experte) et le dénouement de l'épisode.

    Le pas-à-pas est le même que `evaluate.py` : une commande (même vide)
    déclenche la publication de la frame suivante quand le pilote externe est
    engagé ; contre la partie temps réel, ces commandes n'ont pas d'effet de
    cadence. L'état étiqueté est celui que la politique s'apprête à traiter :
    on échantillonne **avant** d'envoyer la commande."""
    client.reset(seed=seed, target=target, x=x, y=y, scenario=scenario)
    client.cmd(driver=True)  # le pilote externe (évalué) pilote
    paced = True  # un pas par commande (mode headless)
    last = client.obs().get("frame", 0)
    obs = wait_frame(client, last, paced)
    # attendre que l'épisode soit réellement en place : le pilote attendu est
    # à l'écran. La garde `station_dist >= 15` ne vaut que pour l'EVA (éjecté
    # loin de la station) - un épisode vaisseau démarre **à quai** (distance
    # 0, le vaisseau ne se déverrouille pas tout seul : c'est la politique qui
    # doit le faire, une fois engagée).
    while obs.get("pilot") != ("eva" if target == "eva" else "vaisseau") \
            or (target == "eva" and obs.get("station_dist", 0.0) < 15.0):
        last = obs.get("frame", 0)
        obs = wait_frame(client, last, paced)
    t0 = obs.get("t", 0.0)
    samples: list[tuple[list[float], list[float]]] = []
    outcome = None
    entry_speed = 0.0
    step_i = 0
    while True:
        if obs.get("episode_done", False):
            # terminaison explicite (delivered / eva_recovered / destroyed) -
            # l'état terminal n'a pas d'action à apprendre
            outcome = obs.get("episode_outcome")
            if obs.get("eva_recovery", 0.0) > 0.0:
                entry_speed = obs["eva"].get("speed", 0.0)
            break
        if obs.get("t", t0) - t0 > timeout:
            break
        # étiquette experte de l'état visité, puis action de la politique.
        # Sous-échantillonnage (`stride`, comme `imitate.load_samples`) : les
        # pas voisins sont quasi identiques - les premiers pas comptent
        # toujours (c'est là que la politique dérive le plus)
        if step_i % stride == 0:
            samples.append((obs_features(obs), expert_target(obs.get("expert", {}))))
        step_i += 1
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
    return samples, {
        "success": outcome in ("delivered", "eva_recovered", "objectives_complete"),
        "outcome": outcome,
        "seconds": max(0.0, seconds),
        "entry_speed": entry_speed,
        "final_dist": obs.get("station_dist", 0.0),
        # scénario à objectifs (Phase 2) : bonus cumulé des complétions de
        # l'épisode (0 hors épisode à objectifs - mêmes règles que le jeu)
        "objective_bonus": obs.get("objective_bonus", 0.0),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--iterations", type=int, default=3, help="itérations DAgger (rollouts + ré-entraînement)")
    ap.add_argument("--episodes", type=int, default=6, help="épisodes déployés par itération")
    ap.add_argument("--seed", type=int, default=1, help="graine du premier épisode (les suivants +1, graines uniques par itération)")
    ap.add_argument("--target", choices=("ship", "eva"), default="eva",
                    help="entité pilotée (eva : cosmonaute éjecté - défaut ; ship : boucle de minage)")
    ap.add_argument("--scenario", default="free",
                    help="règles de l'épisode : free (défaut), economy (boucle de minage "
                         "du vaisseau) ou l'id d'un scénario à objectifs (ex. "
                         "campaign_prospector - missions DAG de l'éditeur)")
    ap.add_argument("--spawn-dists", default=DEFAULT_SPAWN_DISTS, metavar="D1,D2,..",
                    help="distances d'éjection EVA parcourues en cycle (unités)")
    ap.add_argument("--timeout", type=float, default=EPISODE_TIMEOUT, help="délai d'un épisode (s)")
    ap.add_argument("--init-trajectories", nargs="+", metavar="PATH",
                    help="amorce le jeu de données avec les trajectoires de l'autopilote (sortie de bench.py --trajectories)")
    ap.add_argument("--init-policy", default=None, metavar="nn_policy.json",
                    help="politique de départ (sortie d'imitate.py) ; sans lui, itération 0 = aléatoire")
    ap.add_argument("--output", default="dagger_policy.json", metavar="PATH",
                    help="politique entraînée (sortie, rejouable par evaluate.py --strategy nn)")
    # hyper-paramètres du MLP (mêmes défauts qu'imitate.py)
    ap.add_argument("--stride", type=int, default=3,
                    help="sous-échantillonne les pas des épisodes (un pas sur N)")
    ap.add_argument("--hidden", type=int, default=24, help="neurones de la couche cachée")
    ap.add_argument("--epochs", type=int, default=120, help="époques d'entraînement max (arrêt précoce)")
    ap.add_argument("--lr", type=float, default=0.1, help="taux d'apprentissage")
    ap.add_argument("--batch", type=int, default=64, help="taille des mini-lots")
    ap.add_argument("--noise", type=float, default=0.0,
                    help="bruit gaussien ajouté aux entrées pendant l'entraînement (régularisation)")
    ap.add_argument("--val-fraction", type=float, default=0.2,
                    help="fraction des épisodes réservée à la validation (par graine)")
    ap.add_argument("--rng-seed", type=int, default=0, help="graine du mélange (reproductibilité)")
    ap.add_argument("--evaluate", action="store_true",
                    help="après l'entraînement, comparer la politique à l'autopilote en mode hybride")
    ap.add_argument("--eval-episodes", type=int, default=5, help="épisodes de l'évaluation hybride")
    args = ap.parse_args()

    client = DriverClient(args.host)
    if not client.reachable():
        die("le jeu ne répond pas sur " + args.host)

    dists = [float(d) for d in args.spawn_dists.split(",") if d.strip()]
    if not dists:
        ap.error("--spawn-dists doit contenir au moins une distance")

    # jeu de données agrégé : exemples (features, action experte) par graine
    seeds: list[int] = []
    by_seed: dict[int, list[tuple[list[float], list[float]]]] = {}
    if args.init_trajectories:
        seeds, by_seed = load_samples(args.init_trajectories, stride=args.stride)
        print(f"Amorce : {len(args.init_trajectories)} fichier(s) de trajectoires, "
              f"{len(seeds)} épisodes ({sum(len(v) for v in by_seed.values())} pas)")

    # politique de départ : le clone d'imitate.py s'il est fourni, sinon
    # aléatoire (l'itération 0 collecte alors des états très hors distribution
    # - c'est le but : DAgger apprend à s'en rattraper)
    rng = random.Random(args.rng_seed)
    policy = nn_policy(args.init_policy) if args.init_policy else random_policy(rng)
    print(f"Politique de départ : {'clone ' + args.init_policy if args.init_policy else 'aléatoire'}"
          f" · {args.iterations} itérations × {args.episodes} épisodes · "
          f"cible {args.target} · départs {args.spawn_dists} u")

    summary: list[dict[str, Any]] = []
    for it in range(args.iterations):
        # déploiement : graines uniques par itération (aucun chevauchement
        # entre le jeu de données d'itérations différentes), départs variés
        outcomes: list[dict[str, Any]] = []
        for i in range(args.episodes):
            seed = args.seed + it * args.episodes + i
            dist = dists[i % len(dists)]
            if args.target == "eva":
                x, y = spawn_position(seed, dist)
            else:
                x, y = 0.0, 0.0
            samples, outcome = roll_episode(
                client, policy, seed, args.target, x, y, args.scenario,
                args.timeout, args.stride,
            )
            by_seed.setdefault(seed, []).extend(samples)
            outcomes.append(outcome)
        if seed not in seeds:
            seeds = sorted(by_seed)  # toutes les graines, y compris amorce

        ok = sum(1 for o in outcomes if o["success"])
        mean_t = sum(o["seconds"] for o in outcomes if o["success"]) / max(1, ok)
        mean_r = sum(episode_reward(None, o) for o in outcomes) / len(outcomes)
        print(f"\nItération {it + 1}/{args.iterations} : {ok}/{len(outcomes)} épisodes réussis "
              f"(temps moyen {mean_t:.1f} s · récompense moyenne {mean_r:.1f}) "
              f"· {sum(len(v) for v in by_seed.values())} pas agrégés au total")

        # ré-entraînement sur le jeu de données agrégé (train/validation par
        # épisode, comme imitate.py)
        X_tr, Y_tr, X_va, Y_va = split_by_seed(seeds, by_seed, args.val_fraction)
        if not X_tr or not X_va:
            raise SystemExit("✗ pas assez d'épisodes pour séparer train/validation "
                             "(augmentez --episodes ou --init-trajectories)")
        net = MLP(len(X_tr[0]), args.hidden, OUTPUT_COUNT, rng)
        res = net.train(X_tr, Y_tr, epochs=args.epochs, lr=args.lr,
                        batch_size=args.batch, noise=args.noise, rng=rng)
        s_tr = net.summary(X_tr, Y_tr)
        s_va = net.summary(X_va, Y_va)
        print(f"  réseau : {net.inputs} entrées → {net.hidden} cachées → {net.outputs} sorties · "
              f"exactitude train {format_accuracy(s_tr['accuracy'])} · "
              f"val {format_accuracy(s_va['accuracy'])} · "
              f"{res.get('epochs_done', args.epochs)} époques")
        summary.append({
            "iteration": it + 1,
            "successes": ok, "episodes": len(outcomes),
            "mean_seconds": mean_t, "mean_reward": mean_r,
            "steps": sum(len(v) for v in by_seed.values()),
            "train_accuracy": s_tr["accuracy"], "val_accuracy": s_va["accuracy"],
        })
        # la politique suivante est le réseau fraîchement entraîné (mêmes
        # conventions que `policies.nn_policy` : sigmoïdes + rotation softmax)
        from nn import SIGMOID_OUTPUTS

        def policy(obs: dict[str, Any]) -> dict[str, bool]:
            out = net.forward(obs_features(obs))
            cmd = {"up": out[0] >= 0.5, "down": out[1] >= 0.5, "fire": out[2] >= 0.5}
            turn = net.turn_action(out)
            cmd["left"] = turn == "left"
            cmd["right"] = turn == "right"
            return cmd

    meta = {
        "method": "dagger",
        "iterations": args.iterations,
        "episodes_per_iteration": args.episodes,
        "target": args.target,
        "scenario": args.scenario,
        "spawn_dists": dists,
        "init_trajectories": args.init_trajectories or None,
        "init_policy": args.init_policy,
        "episodes": len(seeds),
        "steps": sum(len(v) for v in by_seed.values()),
        "val_seeds": seeds[-max(1, round(len(seeds) * args.val_fraction)):],
        "per_iteration": summary,
        "noise": args.noise,
        "seed": args.rng_seed,
    }
    save_nn(args.output, net, meta)
    print(f"\nPolitique DAgger écrite dans {args.output} - rejouable par "
          f"`python3 evaluate.py --backend hybrid --strategy nn "
          f"--policy {args.output} --target {args.target} --episodes N`.")

    if args.evaluate:
        print("\nÉvaluation hybride contre l'autopilote (mêmes épisodes)…")
        x = 300.0 if args.target == "eva" else 0.0
        y = 0.0
        run_bench_comparison(client, policy, 1, args.eval_episodes, args.target,
                             x, y, False, args.scenario, args.timeout)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)