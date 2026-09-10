#!/usr/bin/env python3
"""Banc d'essai **en continu** contre le processus headless : demande au
processus (`POST /bench`) d'exécuter `episodes` épisodes de bout en bout à
**pleine vitesse dans le processus** (aucun aller-retour HTTP par pas -
l'autopilote du jeu joue chaque épisode jusqu'à sa terminaison explicite),
puis affiche le rapport (`GET /bench`) : cadence réelle en épisodes/s, temps
mur, répartition des dénouements et déroulé par épisode.

C'est l'accélération au-delà du pas-à-pas HTTP : le coût d'un épisode se
réduit au temps de la physique (l'autopilote de référence file à des
centaines d'épisodes/s pour la tâche EVA).

    python3 bench.py --episodes 200 --target eva          # tâche EVA
    python3 bench.py --episodes 20 --target ship --scenario economy  # boucle de minage

Nécessite un processus headless en cours : `cargo run --release -- --headless`
(port 8643 par défaut). Le banc d'essai peut aussi être lancé au démarrage :
`cargo run --release -- --headless --bench N [--target eva] [--scenario economy]`.
"""

from __future__ import annotations

import argparse
import sys
from typing import Any, Optional

from client import DriverClient, die


def print_report(rep: dict[str, Any], episodes: int, seed: int) -> None:
    print(f"\nBanc d'essai en continu : {rep.get('episodes', episodes)} épisodes "
          f"(graines {seed}..{seed + episodes - 1})")
    print("-" * 78)
    print(f"temps mur : {rep.get('wall_seconds', 0.0):.2f} s   "
          f"cadence : {rep.get('episodes_per_second', 0.0):.0f} épisodes/s")
    print(f"dénouements : livrés {rep.get('delivered', 0)} · secourus EVA "
          f"{rep.get('eva_recovered', 0)} · détruits {rep.get('destroyed', 0)} "
          f"· délais (garde-fou) {rep.get('timed_out', 0)}")
    print(f"temps de simulation moyen : {rep.get('mean_seconds', 0.0):.1f} s · "
          f"récompense moyenne : {rep.get('mean_reward', 0.0):.1f}")
    if rep.get("trajectory_file"):
        print(f"trajectoires (RL) : {rep['trajectory_file']}")
    print("-" * 78)
    print(f"{'graine':>7} {'dénouement':>14} {'pas':>7} {'secondes':>9} {'récompense':>11} "
          f"{'objectifs':>9}")
    for r in rep.get("results", []):
        outcome = r.get("outcome") or "delai"
        obj = f"{r.get('objectives_completed', 0)}/{r.get('objectives_total', 0)}" if r.get("objectives_total", 0) else "-"
        print(f"{r.get('seed', 0):>7} {outcome:>14} {r.get('steps', 0):>7} "
              f"{r.get('seconds', 0.0):>9.1f} {r.get('reward', 0.0):>11.1f} {obj:>9}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--episodes", type=int, default=200, help="nombre d'épisodes à enchaîner")
    ap.add_argument("--seed", type=int, default=1, help="graine du premier épisode (les suivants +1)")
    ap.add_argument("--target", choices=("ship", "eva"), default="eva",
                    help="entité pilotée (vaisseau à quai ou cosmonaute EVA éjecté)")
    ap.add_argument("--scenario", default="free",
                    help="règles de l'épisode : free (défaut), economy (boucle de minage "
                         "du vaisseau) ou l'id d'un scénario à objectifs (ex. "
                         "campaign_prospector - missions DAG de l'éditeur)")
    ap.add_argument("--x", type=float, default=0.0, help="position du crash (mode eva)")
    ap.add_argument("--y", type=float, default=0.0)
    ap.add_argument("--auto-generate", action="store_true",
                    help="monde vivant (météores générés au fil de l'épisode)")
    ap.add_argument("--max-steps", type=int, default=None,
                    help="garde-fou par épisode en pas (défaut serveur : 120 s de simulation)")
    ap.add_argument("--trajectories", action="store_true",
                    help="enregistrer les déroulés (obs+action par pas) dans un fichier JSONL "
                         "pour l'entraînement RL (chemin dans le rapport)")
    ap.add_argument("--wait", type=float, default=120.0, help="délai d'attente du rapport (s)")
    args = ap.parse_args()

    client = DriverClient(args.host)
    if not client.reachable():
        die("le jeu headless ne répond pas sur " + args.host)
    # mode EVA : le crash est posé à 300 unités à l'est par défaut (le même
    # départ que `evaluate.py`) - un crash en (0, 0), centre de la station,
    # serait récupéré au premier pas (épisodes triviaux)
    if args.target == "eva" and args.x == 0.0 and args.y == 0.0:
        args.x, args.y = 300.0, 0.0
    print(f"Demande : {args.episodes} épisodes, cible {args.target}, "
          f"scénario {args.scenario} (graines {args.seed}..{args.seed + args.episodes - 1})")
    client.bench(
        episodes=args.episodes,
        seed=args.seed,
        target=args.target,
        x=args.x,
        y=args.y,
        auto_generate=args.auto_generate,
        scenario=args.scenario,
        max_steps=args.max_steps,
        trajectories=args.trajectories,
    )
    rep = client.wait_bench(timeout=args.wait)
    if not rep:
        die("aucun rapport de banc d'essai (le processus headless exécute-t-il le lot ?)")
    print_report(rep, args.episodes, args.seed)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)