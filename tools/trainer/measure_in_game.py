#!/usr/bin/env python3
"""Protocole de mesure **en boucle fermée dans le jeu** (Phase 4) : ce que vaut
la **stratégie apprise embarquée** (`assets/ship_pilot_policy.json`, rejouée
par `src/learned_pilot.rs`) face à la **loi scriptée** de l'autopilote, sur les
**mêmes épisodes**.

C'est la mesure que ni le micro-simulateur (`ship_env.py`) ni le portage
(`validate_learned_port.py`) ne remplacent : ici c'est le **jeu lui-même** qui
joue (physique réelle, météores réels, collisions par triangles, économie
complète), un épisode par cerveau, mêmes graines.

    # 1. compiler puis lancer le jeu headless (les poids sont embarqués à la
    #    compilation : re-entraîner = recompiler)
    cargo build --release
    cargo run --release -- --headless          # port 8643 par défaut
    # 2. mesurer, depuis tools/trainer/
    python3 measure_in_game.py --seeds 1 2 3 4 5 6
    python3 measure_in_game.py --seeds 1 2 --only learned --json /tmp/measure.json

### Protocole (rejouable)

Pour chaque graine, **deux épisodes identiques** (même graine, même cible,
même scénario) sont enchaînés :

1. `POST /reset` sur la graine, puis `POST /cmd {"autopilot": true}` avec
   `learned_pilot` à **faux** (loi scriptée) ou **vrai** (réseau embarqué) ;
2. on attend que le `POST /reset` ouvre une **nouvelle piste** - l'`episode_id`
   de l'observation doit changer - **et** que le cerveau demandé soit appliqué.
   Deux pièges évités : l'observation publiée avant que la remise à zéro soit
   consommée appartient encore à l'épisode précédent (une livraison y serait
   comptée deux fois) ; et le champ `learned` est **neutre** tant que la
   bascule n'a pas été consommée (un pas d'amorçage serait comparé pour rien,
   cf. `validate_learned_port.py`) ;
3. on suit les frames publiées jusqu'à ce que l'épisode rende un **dénouement
   explicite** (`episode_done` : `delivered` / `destroyed` / `eva_recovered` /
   `objectives_complete`, cf. `src/driver.rs::advance_episode`) ;
4. si l'épisode ne se termine pas de lui-même (le cerveau appris, mesuré, ne
   livre pas), un **garde-fou de temps simulé** (`--sim-cap`, défaut 150 s) le
   classe `délai` ; un second garde-fou **mural** (`--wall-cap`, défaut 120 s)
   protège d'un jeu qui ne publie plus de frame (classe `mur`).

L'épisode **economy** cible `ship` se termine donc à la livraison, à la
destruction du vaisseau ou sur le garde-fou - pas besoin de surveiller la
cinématique (la condition de livraison est celle du jeu, pas une heuristique du
script). Le temps rapporté est le **temps simulé** (`episode_t`), comparable
d'une machine à l'autre.

Sortie : un tableau graine par graine (dénouement des deux cerveaux + temps
simulé + pas), puis les totaux. `--json` écrit le même rapport en machine.

Nécessite un processus de jeu lancé avec l'interface de contrôle (les poids du
réseau sont **embarqués dans le binaire** : `src/learned_pilot.rs`).
"""

from __future__ import annotations

import argparse
import json
import statistics
import time
from typing import Any, Optional

from client import DriverClient, die

#: Dénouements publiés par le jeu (`EpisodeOutcome::label`, `src/driver.rs`) -
#: verrouillés contre la source par `test_measure_in_game.py`.
OUTCOME_DELIVERED = "delivered"
OUTCOME_DESTROYED = "destroyed"
OUTCOME_EVA = "eva_recovered"
OUTCOME_OBJECTIVES = "objectives_complete"

#: Dénouements propres à la mesure : le jeu n'en publie aucun.
OUTCOME_TIMEOUT = "délai"   # garde-fou de temps simulé (l'épisode n'a pas bouclé)
OUTCOME_WALL = "mur"        # garde-fou mural (plus aucune frame publiée)
OUTCOME_DONE = "terminé"    # `episode_done` sans libellé (ne devrait pas arriver)

#: Cerveaux mesurés : `False` = loi scriptée de l'autopilote, `True` = réseau
#: embarqué (case LEARNED PILOT).
BRAINS: tuple[tuple[str, bool], ...] = (("scripté", False), ("appris", True))


def classify(obs: dict[str, Any], sim_cap: float) -> Optional[str]:
    """Dénouement d'un pas d'observation, ou `None` tant que l'épisode continue.

    Fonction pure (aucun accès réseau) pour rester testable : le dénouement
    explicite du jeu prime, le garde-fou de temps simulé ne s'applique qu'à un
    épisode encore ouvert.
    """
    if obs.get("episode_done"):
        return obs.get("episode_outcome") or OUTCOME_DONE
    if obs.get("episode_t", 0.0) >= sim_cap:
        return OUTCOME_TIMEOUT
    return None


def run_episode(
    client: DriverClient,
    seed: int,
    learned: bool,
    *,
    target: str,
    scenario: str,
    sim_cap: float,
    wall_cap: float,
) -> dict[str, Any]:
    """Joue **un** épisode et renvoie son résultat mesuré.

    `learned` : cerveau de l'autopilote (`False` loi scriptée, `True` réseau
    embarqué). Le résultat porte la graine, le cerveau, le dénouement, le temps
    **simulé**, les pas et la livraison (`episode_deliveries` / `episode_collected`).
    """
    # l'id de l'épisode courant *avant* la demande : la piste ouverte à la
    # consommation du `POST /reset` aura un id strictement supérieur - c'est le
    # signal que l'observation lue appartient bien au nouvel épisode.
    before = client.obs().get("episode_id", 0)
    client.reset(seed=seed, target=target, scenario=scenario)
    client.cmd(autopilot=True, learned_pilot=learned)

    deadline = time.monotonic() + wall_cap
    last = 0
    obs: dict[str, Any] = {}
    while time.monotonic() < deadline:
        obs = client.obs()
        if (obs.get("frame", 0) > last
                and obs.get("episode_id", 0) != before
                and bool(obs.get("learned_pilot")) == learned):
            break
        last = obs.get("frame", last)
        time.sleep(0.002)
    else:
        return _result(seed, learned, OUTCOME_WALL, obs)

    while time.monotonic() < deadline:
        obs = client.obs()
        if obs.get("frame", 0) <= last:
            time.sleep(0.001)
            continue
        last = obs["frame"]
        if "learned_pilot" not in obs:
            die("le jeu ne publie pas le champ `learned_pilot` : binaire trop "
                "ancien (recompiler avec src/learned_pilot.rs)")
        outcome = classify(obs, sim_cap)
        if outcome is not None:
            return _result(seed, learned, outcome, obs)
    return _result(seed, learned, OUTCOME_WALL, obs)


def _result(
    seed: int, learned: bool, outcome: str, obs: dict[str, Any]
) -> dict[str, Any]:
    """Résultat d'épisode normalisé (temps simulé, pas, compteurs de livraison)."""
    return {
        "seed": seed,
        "brain": "appris" if learned else "scripté",
        "learned": learned,
        "outcome": outcome,
        "seconds": obs.get("episode_t", 0.0),
        "steps": obs.get("episode_steps", 0),
        "deliveries": obs.get("episode_deliveries", 0),
        "collected": obs.get("episode_collected", 0),
    }


def delivered_count(results: list[dict[str, Any]]) -> int:
    """Nombre d'épisodes livrés (le succès de la boucle de minage)."""
    return sum(1 for r in results if r["outcome"] == OUTCOME_DELIVERED)


def mean_seconds_delivered(results: list[dict[str, Any]]) -> Optional[float]:
    """Temps simulé moyen des épisodes **livrés** (`None` si aucun)."""
    done = [r["seconds"] for r in results if r["outcome"] == OUTCOME_DELIVERED]
    return statistics.fmean(done) if done else None


def print_report(rows: list[tuple[int, dict[str, Any], dict[str, Any]]],
                 target: str, scenario: str, sim_cap: float) -> None:
    """Tableau graine par graine + totaux par cerveau."""
    print(f"\nMesure en boucle fermée DANS LE JEU "
          f"(cible {target}, scénario {scenario}, garde-fou simulé {sim_cap:.0f} s)")
    print("  « scripté » = loi de l'autopilote · « appris » = réseau embarqué "
          "(LEARNED PILOT)")
    print("-" * 78)
    print(f"{'graine':>7} {'scripté':>31} {'appris':>35}")
    for seed, ref, pol in rows:
        print(f"{seed:>7} {_cell(ref):>31} {_cell(pol):>35}")
    print("-" * 78)
    for name, learned in BRAINS:
        got = [p if learned else r for _, r, p in rows]
        if not got:
            continue
        mean = mean_seconds_delivered(got)
        tail = f" · temps moyen livré {mean:.1f} s" if mean is not None else ""
        print(f"{name:<8}: {delivered_count(got)}/{len(got)} livrés{tail}")
    print("  (le temps est le temps **simulé** de l'épisode, `episode_t`)")


def _cell(r: dict[str, Any]) -> str:
    """Cellule de tableau : dénouement, temps simulé et pas."""
    return f"{r['outcome']} {r['seconds']:5.1f}s ({r['steps']} pas)"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--seeds", type=int, nargs="+", default=[1, 2, 3, 4, 5, 6],
                    help="graines des épisodes mesurés (les deux cerveaux par graine)")
    ap.add_argument("--target", choices=("ship", "eva"), default="ship",
                    help="entité pilotée (boucle de minage du vaisseau par défaut)")
    ap.add_argument("--scenario", default="economy",
                    help="règles de l'épisode (economy = boucle de minage, qui "
                         "se termine à la livraison)")
    ap.add_argument("--only", choices=("both", "scripted", "learned"), default="both",
                    help="ne mesurer qu'un cerveau (le « scripté » seul est un "
                         "banc de la loi du jeu, le « appris » seul la stratégie déployée)")
    ap.add_argument("--sim-cap", type=float, default=150.0,
                    help="garde-fou de temps simulé par épisode (s) - au-delà, "
                         "l'épisode est classé « délai »")
    ap.add_argument("--wall-cap", type=float, default=120.0,
                    help="garde-fou mural par épisode (s) - au-delà, « mur »")
    ap.add_argument("--json", metavar="PATH", default=None,
                    help="écrire le rapport machine (graines, cerveaux, totaux)")
    args = ap.parse_args()

    client = DriverClient(args.host)
    if not client.reachable():
        die("le jeu ne répond pas sur " + args.host)
    if "learned_pilot" not in client.obs():
        die("l'observation ne publie pas `learned_pilot` : binaire trop ancien "
            "(reconstruire avec `cargo build --release`)")

    brains = [b for b in BRAINS if args.only == "both"
              or (args.only == "learned") == b[1]]
    # un épisode par graine et par cerveau : les deux colonnes partagent la graine
    results: dict[bool, list[dict[str, Any]]] = {learned: [] for _, learned in brains}
    for seed in args.seeds:
        for name, learned in brains:
            print(f"graine {seed} · {name}…", flush=True)
            results[learned].append(run_episode(
                client, seed, learned,
                target=args.target, scenario=args.scenario,
                sim_cap=args.sim_cap, wall_cap=args.wall_cap,
            ))

    if len(brains) == 2:
        print_report(list(zip(args.seeds, results[False], results[True])),
                     args.target, args.scenario, args.sim_cap)
    else:
        name, learned = brains[0]
        got = results[learned]
        for r in got:
            print(f"graine {r['seed']:>4} · {name:<8} : {_cell(r)}")
        mean = mean_seconds_delivered(got)
        tail = f" · temps moyen livré {mean:.1f} s" if mean is not None else ""
        print(f"{name} : {delivered_count(got)}/{len(got)} livrés{tail}")

    if args.json:
        report = {
            "target": args.target,
            "scenario": args.scenario,
            "sim_cap": args.sim_cap,
            "wall_cap": args.wall_cap,
            "episodes": [r for _, learned in brains for r in results[learned]],
            "totals": {
                name: {
                    "delivered": delivered_count(results[learned]),
                    "episodes": len(results[learned]),
                    "mean_seconds_delivered": mean_seconds_delivered(results[learned]),
                }
                for name, learned in brains
            },
        }
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(report, f, ensure_ascii=False, indent=2)
        print(f"rapport : {args.json}")


if __name__ == "__main__":
    main()
