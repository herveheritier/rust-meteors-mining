#!/usr/bin/env python3
"""Apprentissage **hors-ligne par imitation** de l'autopilote du jeu sur les
**trajectoires JSONL** du banc d'essai (`bench.py --trajectories`, ou le champ
`trajectory_file` du rapport `/bench`).

Chaque pas d'un épisode est un exemple supervisé : l'**observation** (au
format `/obs`, telle que publiée par le processus headless) en entrée,
l'**action** que l'autopilote du jeu a prise ce pas-là en cible. Le script
entraîne un petit réseau de neurones (`nn.MLP`, Python standard uniquement)
à reproduire ces décisions - un clonage comportemental (behavioral cloning) -
puis écrit la politique dans un fichier JSON rejouable par
`evaluate.py --strategy nn --policy nn_policy.json` (backend live ou hybride,
pour la tâche EVA comme pour la boucle de minage du vaisseau).

    # 1. produire des trajectoires (nécessite un processus headless) :
    python3 bench.py --episodes 30 --target eva --trajectories
    # 2. entraîner hors-ligne sur le fichier écrit (chemin dans le rapport) :
    python3 imitate.py --trajectories /tmp/meteors_mining_headless_*/trajectories_1_30_eva_free.jsonl
    # 3. rejouer la politique entraînée contre l'autopilote (mêmes épisodes) :
    python3 evaluate.py --backend hybrid --strategy nn --policy nn_policy.json --target eva --episodes 5

La séparation train/validation se fait **par épisode** (graines) et non par
pas : les pas d'un même épisode sont fortement corrélés (un clonage qui
mémoriserait des pas voisins gonflerait artificiellement son exactitude).
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from typing import Any

from client import DriverClient, die
from nn import MLP, ACTIONS, OUTPUT_COUNT, action_target, obs_features, save_nn


#: Fraction des épisodes retenue pour la validation (les derniers par graine).
VAL_FRACTION = 0.2


def load_samples(
    paths: list[str],
    stride: int = 3,
) -> tuple[list[int], dict[int, list[tuple[list[float], list[float]]]]]:
    """Charge les trajectoires JSONL et regroupe les exemples (obs, action)
    par graine d'épisode. Chaque exemple = features de l'observation + cibles
    binaires des actions (le réseau prédit chaque bouton indépendamment).

    `stride` sous-échantillonne les pas d'un épisode (un pas sur `stride`) :
    les pas voisins sont quasi identiques (la physique avance de 1/60 s et
    l'autopilote ne change d'action que rarement) - entraîner sur tous les
    pas coûte `stride` fois plus cher pour le même apprentissage."""
    by_seed: dict[int, list[tuple[list[float], list[float]]]] = {}
    for path in paths:
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                ev = json.loads(line)
                if ev.get("event") != "step":
                    continue
                # un pas sur `stride` seulement (les premiers pas comptent
                # toujours - c'est là que l'autopilote décide le plus)
                if int(ev.get("step", 0)) % stride != 0:
                    continue
                obs = ev.get("obs", {})
                action = ev.get("action", {})
                x = obs_features(obs)
                y = action_target(action)
                by_seed.setdefault(int(ev.get("seed", 0)), []).append((x, y))
    seeds = sorted(by_seed)
    if not seeds:
        raise SystemExit("✗ aucune trajectoire trouvée (--trajectories doit pointer "
                         "vers un fichier JSONL écrit par un bench avec --trajectories)")
    return seeds, by_seed


def split_by_seed(
    seeds: list[int],
    by_seed: dict[int, list[tuple[list[float], list[float]]]],
    val_fraction: float,
) -> tuple[list[list[float]], list[list[float]], list[list[float]], list[list[float]]]:
    """Train / validation par épisode : les derniers `val_fraction` épisodes
    (par graine) forment la validation, le reste l'entraînement."""
    n_val = max(1, round(len(seeds) * val_fraction))
    val_seeds = set(seeds[-n_val:])
    X_tr, Y_tr, X_va, Y_va = [], [], [], []
    for seed in seeds:
        for x, y in by_seed[seed]:
            if seed in val_seeds:
                X_va.append(x)
                Y_va.append(y)
            else:
                X_tr.append(x)
                Y_tr.append(y)
    return X_tr, Y_tr, X_va, Y_va


def format_accuracy(acc: float) -> str:
    return f"{acc * 100.0:5.1f} %"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--trajectories", nargs="+", required=True, metavar="PATH",
                    help="fichier(s) JSONL de trajectoires du bench (chemin dans le rapport /bench)")
    ap.add_argument("--output", default="nn_policy.json", metavar="PATH",
                    help="politique entraînée (sortie, rejouable par evaluate.py)")
    ap.add_argument("--stride", type=int, default=3,
                    help="sous-échantillonne les pas des épisodes (un pas sur N) : les "
                         "pas voisins sont redondants - accélère l'entraînement")
    ap.add_argument("--hidden", type=int, default=24, help="neurones de la couche cachée")
    ap.add_argument("--epochs", type=int, default=120, help="époques d'entraînement max (arrêt précoce)")
    ap.add_argument("--lr", type=float, default=0.1, help="taux d'apprentissage")
    ap.add_argument("--batch", type=int, default=64, help="taille des mini-lots")
    ap.add_argument("--noise", type=float, default=0.0,
                    help="bruit gaussien ajouté aux entrées pendant l'entraînement "
                         "(régularisation possible contre la dérive en boucle fermée - "
                         "peut dégrader, tester)")
    ap.add_argument("--val-fraction", type=float, default=VAL_FRACTION,
                    help="fraction des épisodes réservée à la validation (par graine)")
    ap.add_argument("--seed", type=int, default=0, help="graine du mélange (reproductibilité)")
    ap.add_argument("--evaluate", action="store_true",
                    help="après l'entraînement, comparer la politique à l'autopilote "
                         "en mode hybride (nécessite un processus headless)")
    ap.add_argument("--episodes", type=int, default=5, help="épisodes de l'évaluation hybride")
    ap.add_argument("--target", choices=("ship", "eva"), default="eva")
    ap.add_argument("--scenario", choices=("free", "economy"), default="free")
    ap.add_argument("--host", default="http://127.0.0.1:8643/")
    args = ap.parse_args()

    seeds, by_seed = load_samples(args.trajectories, stride=args.stride)
    X_tr, Y_tr, X_va, Y_va = split_by_seed(seeds, by_seed, args.val_fraction)
    n_steps = sum(len(v) for v in by_seed.values())
    print(f"Trajectoires : {len(args.trajectories)} fichier(s), {len(seeds)} épisodes "
          f"(graines {seeds[0]}..{seeds[-1]}), {n_steps} pas retenus (stride {args.stride})")
    print(f"Entraînement : {len(X_tr)} pas · validation : {len(X_va)} pas "
          f"(séparation par épisode)")
    if not X_tr or not X_va:
        raise SystemExit("✗ pas assez d'épisodes pour séparer train/validation "
                         "(augmentez --episodes du bench ou baissez --val-fraction)")

    rng = random.Random(args.seed)
    net = MLP(len(X_tr[0]), args.hidden, OUTPUT_COUNT, rng)
    print(f"Réseau : {net.inputs} entrées → {net.hidden} cachées → {net.outputs} sorties "
          f"({', '.join(ACTIONS)})")
    print(f"Entraînement : {args.epochs} époques max, lr {args.lr}, lots de {args.batch}…")
    for name, X, Y in (("train", X_tr, Y_tr), ("val", X_va, Y_va)):
        s = net.summary(X, Y)
        print(f"  départ : perte {s['loss']:7.4f} · exactitude {format_accuracy(s['accuracy'])} ({name})")
    res = net.train(X_tr, Y_tr, epochs=args.epochs, lr=args.lr,
                    batch_size=args.batch, noise=args.noise, rng=rng)
    s_tr = net.summary(X_tr, Y_tr)
    s_va = net.summary(X_va, Y_va)
    print(f"  fin    : perte {s_tr['loss']:7.4f} · exactitude "
          f"{format_accuracy(s_tr['accuracy'])} (train) · "
          f"{format_accuracy(s_va['accuracy'])} (val) · "
          f"{res.get('epochs_done', args.epochs)} époques")
    print("\nExactitude par action (validation) :")
    for a, acc in zip(ACTIONS, s_va["per_action"]):
        print(f"  {a:<6} {format_accuracy(acc)}")

    meta = {
        "source": args.trajectories,
        "episodes": len(seeds),
        "steps": n_steps,
        "stride": args.stride,
        "val_seeds": seeds[-max(1, round(len(seeds) * args.val_fraction)) :],
        "train_accuracy": round(s_tr["accuracy"], 4),
        "val_accuracy": round(s_va["accuracy"], 4),
        "val_loss": round(s_va["loss"], 4),
        "epochs_done": res.get("epochs_done", args.epochs),
        "noise": args.noise,
        "seed": args.seed,
        "target": args.target,
        "scenario": args.scenario,
    }
    save_nn(args.output, net, meta)
    print(f"\nPolitique écrite dans {args.output} - rejouable par "
          f"`python3 evaluate.py --backend hybrid --strategy nn "
          f"--policy {args.output} --target {args.target} --episodes {args.episodes}`.")

    if args.evaluate:
        from evaluate import run_bench_comparison
        from nn import SIGMOID_OUTPUTS

        client = DriverClient(args.host)
        if not client.reachable():
            die("le jeu headless ne répond pas sur " + args.host)
        x = 300.0 if args.target == "eva" else 0.0
        y = 0.0

        def policy(obs: dict[str, Any]) -> dict[str, bool]:
            out = net.forward(obs_features(obs))
            cmd = {"up": out[0] >= 0.5, "down": out[1] >= 0.5, "fire": out[2] >= 0.5}
            turn = net.turn_action(out)
            cmd["left"] = turn == "left"
            cmd["right"] = turn == "right"
            return cmd

        run_bench_comparison(client, policy, 1, args.episodes, args.target, x, y,
                             False, args.scenario, 60.0)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)