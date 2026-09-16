#!/usr/bin/env python3
"""Valide le **portage Python de l'autopilote vaisseau** (`ship_autopilot_ref.py`)
contre la loi du jeu : sur une vraie partie headless, on engage l'autopilote du
jeu et on compare, **à chaque frame**, le champ `expert` de l'observation
(l'action que prend l'autopilote du jeu sur cet état) à la commande calculée
par le portage sur la **même** observation.

C'est la mesure de fidélité symétrique du portage EVA (`autopilot_ref.py`) :
si le taux d'accord est élevé sur un épisode complet, le portage est une
référence hors-ligne valable (mesure d'écart, expert étiqueteur d'imitation).

Nécessite un processus de jeu (`cargo run -- --headless` par défaut) :

    python3 validate_ship_port.py --seeds 7 21 --scenario economy --seconds 20

Sortie : par graine, le nombre de pas comparés, le taux d'accord global et la
répartition des désaccords par touche (utile pour diagnostiquer un écart).
"""

from __future__ import annotations

import argparse
import time

from client import DriverClient, die
from ship_autopilot_ref import autopilot_ship_inputs

KEYS = ("up", "down", "left", "right", "fire")


def compare_episode(
    client: DriverClient, seed: int, scenario: str, seconds: float
) -> tuple[int, int, dict[str, int], str | None]:
    """Rejoue un épisode piloté par l'autopilote du jeu et compare les commandes.

    Renvoie `(pas comparés, pas identiques, désaccords par touche, dénouement)`.
    Le premier pas après le `POST /reset` peut encore porter la piste de
    l'épisode précédent (`episode_done` verrouillé) : on l'ignore en attendant
    que l'épisode courant soit ouvert."""
    client.reset(seed=seed, target="ship", scenario=scenario)
    client.cmd(autopilot=True)
    seen = match = 0
    mism = {k: 0 for k in KEYS}
    last = 0
    outcome: str | None = None
    deadline = time.monotonic() + seconds
    # attendre l'ouverture de l'épisode (piste remise à zéro)
    while time.monotonic() < deadline:
        o = client.obs()
        if o.get("frame", 0) > last and not o.get("episode_done"):
            break
        last = o.get("frame", 0)
        time.sleep(0.002)
    while time.monotonic() < deadline:
        o = client.obs()
        if o.get("frame", 0) <= last:
            continue
        last = o["frame"]
        if o.get("pilot") != "vaisseau":
            break
        expert = o.get("expert", {})
        port = autopilot_ship_inputs(o)
        seen += 1
        if all(bool(expert.get(k)) == bool(port.get(k)) for k in KEYS):
            match += 1
        else:
            for k in KEYS:
                if bool(expert.get(k)) != bool(port.get(k)):
                    mism[k] += 1
        if o.get("episode_done"):
            outcome = o.get("episode_outcome")
            break
    return seen, match, mism, outcome


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--seeds", type=int, nargs="+", default=[7, 21, 42],
                    help="graines des épisodes comparés")
    ap.add_argument("--scenario", default="economy",
                    help="règles de l'épisode (economy = boucle de minage du vaisseau)")
    ap.add_argument("--seconds", type=float, default=20.0,
                    help="durée maximale d'un épisode comparé (s mur)")
    args = ap.parse_args()

    client = DriverClient(args.host)
    if not client.reachable():
        die("le jeu ne répond pas sur " + args.host)

    print(f"Validation du port vaisseau contre le jeu réel (scénario {args.scenario})")
    print("-" * 66)
    total_seen = total_match = 0
    for seed in args.seeds:
        seen, match, mism, outcome = compare_episode(client, seed, args.scenario, args.seconds)
        total_seen += seen
        total_match += match
        rate = 100.0 * match / max(1, seen)
        print(f"graine {seed:>4} : {match}/{seen} pas identiques ({rate:.1f}%) "
              f"· dénouement {outcome}")
        discord = {k: v for k, v in mism.items() if v}
        if discord:
            print(f"             désaccords : {discord}")
    print("-" * 66)
    print(f"total : {total_match}/{total_seen} pas identiques "
          f"({100.0 * total_match / max(1, total_seen):.1f}%)")


if __name__ == "__main__":
    main()
