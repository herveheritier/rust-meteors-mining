#!/usr/bin/env python3
"""**Représentativité du micro-simulateur vaisseau** (`ship_env.py`) : le
simulateur rejoue-t-il, hors ligne, ce que le **jeu** a joué ?

C'est la mesure qui manquait : le simulateur était **hybride** (météores en
cercles, champ minier **synthétisé**, départ immédiat - le jeu retient le
vaisseau 1,5 s au décollage), si bien qu'il pouvait **désigner le mauvais
réseau** (documents §5 nonies bis : l'écart de récompense hors ligne était
*anti-corrélé* au résultat réel). Deux corrections le rendent représentatif :

1. le **monde** : le champ minier de la graine est **celui du jeu**,
   enregistré dans `fixtures/ship_mining_fields.json` (voir
   `measure_in_game.py --fields`) et importé par `ShipSim` ;
2. la **séquence d'épisode** : liens attachés au départ, rétraction de 1,5 s
   (entrées ignorées), animation d'accostage de 3 s avant la boîte, livraison
   datée là - comme `game.rs::update` / `docking.rs`.

Le fixture porte aussi les **dénouements du jeu** par graine (loi scriptée et
réseau embarqué) : la mesure se fait donc **sans processus de jeu**.

    cd tools/trainer
    python3 validate_ship_env.py                        # scripté vs jeu + réseau embarqué
    python3 validate_ship_env.py --seeds 1 2 3 --no-policy
    python3 validate_ship_env.py --json /tmp/repr.json

Sortie : par graine, le dénouement et le temps **simulé** du jeu et du
simulateur, puis l'**accord** des dénouements et un verdict. Un écart est
attendu - la géométrie reste des **cercles** au lieu de meshes et le ramassage
des minerais a une tolérance (`ship_env.py` documente la frontière) : la
mesure dit **combien** de graines tombent du même côté, pas si le monde est
identique au triangle près.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
from typing import Any, Callable, Optional

from policies import nn_policy
from ship_autopilot_ref import autopilot_ship_inputs
from ship_env import FIELD_FIXTURE, OUTCOME_DELIVERED, ShipSim, load_real_fields
from ship_warmstart import run_ship_episode

#: Départ **nominal du jeu** : vaisseau à quai au centre de la station, plein,
#: soute vide (`(seed, x, y, orientation, vx, vy, cargo, fuel, ammo, credits)`).
DOCKED_START = (0.0, 0.0, 0.0, 0.0, 0.0, 0, None, None, 0)

#: Part de graines dont le dénouement doit tomber du même côté (réussite ou
#: échec) pour que le simulateur soit déclaré **représentatif** sur ce cerveau.
#: 0,8 = 10 graines sur 12 : le seuil documenté, pas une garantie.
AGREEMENT_THRESHOLD = 0.8

#: Politique **embarquée dans le jeu** (le cerveau de la case LEARNED PILOT) -
#: mesurée en face de sa propre trace quand elle est disponible.
DEFAULT_POLICY = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                              "..", "..", "assets", "ship_pilot_policy.json")


def load_fixture(path: str) -> dict[str, Any]:
    """Fixture des champs réels et des dénouements du jeu
    (`measure_in_game.py --fields`). Absent → sortie en erreur explicite."""
    try:
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    except OSError as e:
        raise SystemExit(
            f"✗ fixture illisible : {path} ({e})\n"
            "  l'enregistrer depuis une partie réelle (jeu lancé en headless) :\n"
            "    cargo run --release -- --headless\n"
            "    python3 measure_in_game.py --seeds 1 2 3 4 5 6 7 8 9 10 11 12 \\\n"
            "        --fields fixtures/ship_mining_fields.json") from e


def sim_episode(seed: int, policy: Callable[[dict[str, Any]], dict[str, bool]],
                timeout: float) -> dict[str, Any]:
    """Un épisode du simulateur depuis le départ **du jeu** (à quai)."""
    start = (seed, *DOCKED_START)
    return run_ship_episode(ShipSim(timeout=timeout), policy, start, timeout)


def succeeded(result: Optional[dict[str, Any]]) -> Optional[bool]:
    """L'épisode est-il **réussi** (livraison) ? `None` si la graine n'a pas été
    mesurée pour ce cerveau (le fixture ne porte que ce qui a été joué)."""
    if not result:
        return None
    outcome = result.get("outcome")
    if outcome is None:
        return False  # garde-fou atteint (délai) : la boucle n'a pas bouclé
    return outcome == OUTCOME_DELIVERED


def agree(game: Optional[dict[str, Any]], sim: Optional[dict[str, Any]]) -> Optional[bool]:
    """Le jeu et le simulateur tombent-ils du même côté (réussi / échoué) ?"""
    g, s = succeeded(game), succeeded(sim)
    if g is None or s is None:
        return None
    return g == s


def _cell(result: Optional[dict[str, Any]]) -> str:
    """Cellule de tableau : dénouement et temps simulé (ou « — »)."""
    if result is None:
        return "—"
    outcome = result.get("outcome") or "délai"
    return f"{outcome} {result.get('seconds', 0.0):5.1f}s"


def representativity(fixture_path: str = FIELD_FIXTURE,
                     policy_path: Optional[str] = DEFAULT_POLICY,
                     seeds: Optional[list[int]] = None,
                     timeout: float = 150.0) -> dict[str, Any]:
    """Mesure complète, réutilisable : le jeu **et** le simulateur, graine par
    graine, pour la loi scriptée et pour le réseau embarqué.

    Renvoie `{"rows": [...], "verdict": {...}}` : chaque ligne porte les quatre
    dénouements, chaque verdict la part de graines du même côté (scripté,
    appris) et l'écart de temps sur les livraisons communes. `policy_path=None`
    mesure la loi scriptée seule (aucun réseau à charger).
    """
    fixture = load_fixture(fixture_path)
    recorded: dict[int, dict[str, Any]] = {}
    for key, entry in (fixture.get("seeds") or {}).items():
        try:
            recorded[int(key)] = entry
        except (TypeError, ValueError):
            continue
    real_field = load_real_fields(fixture_path)
    wanted = seeds or sorted(recorded)
    missing_field = [s for s in wanted if s not in real_field]

    policy: Optional[Callable[[dict[str, Any]], dict[str, bool]]] = None
    if policy_path:
        policy = nn_policy(policy_path)

    rows: list[dict[str, Any]] = []
    for seed in wanted:
        entry = recorded.get(seed, {})
        game_ref = entry.get("reference")
        game_pol = entry.get("learned") if policy else None
        sim_ref = sim_episode(seed, autopilot_ship_inputs, timeout)
        sim_pol = sim_episode(seed, policy, timeout) if policy else None
        rows.append({
            "seed": seed,
            "game_reference": game_ref, "sim_reference": sim_ref,
            "game_learned": game_pol, "sim_learned": sim_pol,
            "reference_agrees": agree(game_ref, sim_ref),
            "learned_agrees": agree(game_pol, sim_pol) if policy else None,
        })

    verdict: dict[str, Any] = {}
    for label, key, brain in (("scripté", "reference_agrees", "reference"),
                              ("appris", "learned_agrees", "learned")):
        judged = [r[key] for r in rows if r[key] is not None]
        if not judged:
            continue
        ok = sum(1 for a in judged if a)
        rate = ok / len(judged)
        # livraisons du cerveau, **du côté du jeu et du simulateur** : c'est ce
        # couple qui porte le **classement** (donc une éventuelle sélection).
        # Comptées sur les graines que le jeu a effectivement mesurées.
        measured = [r for r in rows if r[f"game_{brain}"] is not None]
        verdict[label] = {
            "agree": ok, "judged": len(judged), "rate": rate,
            "representative": rate >= AGREEMENT_THRESHOLD,
            "compared": len(measured),
            "game_delivered": sum(1 for r in measured if succeeded(r[f"game_{brain}"])),
            "sim_delivered": sum(1 for r in measured if succeeded(r[f"sim_{brain}"])),
        }
    delivered = [(r["game_reference"]["seconds"], r["sim_reference"]["seconds"])
                 for r in rows
                 if succeeded(r["game_reference"]) and succeeded(r["sim_reference"])]
    if delivered:
        verdict["mean_seconds_delta"] = statistics.fmean(abs(g - s) for g, s in delivered)
    verdict["missing_field"] = missing_field
    verdict["seeds"] = [r["seed"] for r in rows]
    return {"rows": rows, "verdict": verdict, "fixture": fixture_path,
            "policy": policy_path}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--fixture", default=FIELD_FIXTURE,
                    help="fixture des champs réels + dénouements du jeu "
                         "(défaut : fixtures/ship_mining_fields.json)")
    ap.add_argument("--seeds", type=int, nargs="+", default=None,
                    help="graines mesurées (défaut : celles du fixture)")
    ap.add_argument("--policy", default=DEFAULT_POLICY, metavar="PATH",
                    help="réseau mesuré en face de sa trace dans le jeu "
                         "(défaut : assets/ship_pilot_policy.json embarqué)")
    ap.add_argument("--no-policy", action="store_true",
                    help="ne mesurer que la loi scriptée (aucun réseau)")
    ap.add_argument("--timeout", type=float, default=150.0,
                    help="garde-fou de temps simulé d'un épisode hors ligne (s), "
                         "le même que le `--sim-cap` de la mesure dans le jeu")
    ap.add_argument("--json", metavar="PATH", default=None,
                    help="écrire le rapport machine")
    args = ap.parse_args()

    policy_path = args.policy if not args.no_policy else None
    try:
        report = representativity(args.fixture, policy_path, args.seeds, args.timeout)
    except (OSError, ValueError) as e:
        print(f"⚠ réseau illisible ({e}) : mesure de la loi scriptée seule")
        report = representativity(args.fixture, None, args.seeds, args.timeout)
    rows, verdict = report["rows"], report["verdict"]
    with_policy = report["policy"] is not None

    print(f"Représentativité du micro-simulateur vaisseau (fixture {report['fixture']})")
    print(f"  {len(rows)} graines · départ à quai · garde-fou {args.timeout:.0f} s "
          f"de temps simulé")
    if verdict["missing_field"]:
        print(f"  ⚠ graines sans champ enregistré (monde synthétisé, non "
              f"représentatif) : {verdict['missing_field']}")
    print("-" * 92)
    header = f"{'graine':>7} │ {'jeu : scripté':>20} {'sim : scripté':>20}"
    if with_policy:
        header += f" │ {'jeu : appris':>20} {'sim : appris':>20}"
    print(header)
    print("-" * 92)
    for r in rows:
        line = f"{r['seed']:>7} │ {_cell(r['game_reference']):>20} {_cell(r['sim_reference']):>20}"
        if with_policy:
            line += f" │ {_cell(r['game_learned']):>20} {_cell(r['sim_learned']):>20}"
        print(line)
    print("-" * 92)

    for label in ("scripté", "appris"):
        entry = verdict.get(label)
        if not entry:
            continue
        print(f"représentatif ({label}) : {entry['agree']}/{entry['judged']} graines "
              f"du même côté ({100.0 * entry['rate']:.0f} %)")
    if "mean_seconds_delta" in verdict:
        print(f"temps des livraisons communes : écart moyen |jeu − sim| "
              f"{verdict['mean_seconds_delta']:.1f} s")
    # Le **classement** est ce que la mesure sert à établir : le simulateur
    # désigne-t-il le cerveau que la partie préfère ?
    ref, pol = verdict.get("scripté"), verdict.get("appris")
    if with_policy and ref and pol:
        print(f"classement — livraisons du **jeu** : scripté {ref['game_delivered']}/"
              f"{ref['compared']} vs appris {pol['game_delivered']}/{pol['compared']} "
              f"· **simulateur** : {ref['sim_delivered']} vs {pol['sim_delivered']}")
        if (pol["game_delivered"] < ref["game_delivered"]
                and pol["sim_delivered"] > ref["sim_delivered"]):
            print("  ⚠ le simulateur préfère le cerveau que le jeu classe **dernier** "
                  "- ne pas sélectionner de politique sur cette mesure")
    for label in ("scripté", "appris"):
        entry = verdict.get(label)
        if entry and not entry["representative"]:
            print(f"  ⚠ le simulateur ne rejoue pas le {label} du jeu : ne pas "
                  f"sélectionner de politique sur cette mesure")
    print("  (géométrie des météores en cercles et tolérance de ramassage : la "
          "frontière hybride reste, voir ship_env.py)")

    if args.json:
        payload = {"fixture": args.fixture, "policy": report["policy"],
                   "timeout": args.timeout, "seeds": rows, "verdict": verdict}
        with open(args.json, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False, indent=2)
        print(f"rapport : {args.json}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
