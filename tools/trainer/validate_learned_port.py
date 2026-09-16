#!/usr/bin/env python3
"""Valide le **portage Rust de la politique apprise** (`src/learned_pilot.rs`,
Phase 4) contre la **politique Python** (`nn.py` + poids embarqués) : sur une
vraie partie headless, on laisse le **jeu** jouer avec le réseau embarqué
(`POST /cmd {"autopilot": true, "learned_pilot": true}`) et on compare, **à
chaque pas publié**, le champ `learned` de l'observation (l'action que le
portage Rust a calculée sur cet état) à la commande calculée par la politique
Python sur la **même** observation.

C'est la mesure de fidélité symétrique de `validate_ship_port.py` (qui compare
le portage de la loi vaisseau au champ `expert`) : si le taux d'accord est de
100 % sur un épisode de minage complet, le portage est fidèle et la stratégie
déployée dans le jeu est bien celle qui a été entraînée.

`--record` écrit en plus le **fixture** rejoué hors-ligne par le test unitaire
Rust (`src/learned_pilot.rs`, `learned_pilot_windows.jsonl`) : une ligne JSON
par fenêtre `{"obs": …, "features": …, "action": …}` - l'observation (dont
`nearby` est **tronqué aux 6 slots** que les features lisent, pour garder le
fixture committable), les features de `nn.py::obs_features` et l'action de la
politique Python. Le test Rust vérifie les deux.

Nécessite un processus de jeu (`cargo run -- --headless` par défaut) :

    python3 validate_learned_port.py --seeds 7 21 --scenario economy --seconds 20
    python3 validate_learned_port.py --record fixtures/learned_pilot_windows.jsonl

Sortie : par graine, les pas comparés, le taux d'accord et la répartition des
désaccords par touche (utile pour diagnostiquer un écart).
"""

from __future__ import annotations

import argparse
import json
import os
import time
from typing import Any, Optional

from client import DriverClient, die
from nn import BULLET_SLOTS, NEARBY_SLOTS, load_nn, obs_features
from policies import nn_policy


class Reference:
    """Politique Python de référence (les **mêmes poids** que le jeu) et de quoi
    expliquer un désaccord : l'action vient de `policies.nn_policy` - la
    référence d'action, unique source de la règle gloutonne - et le réseau
    rechargé sert à afficher les probabilités de la couche de sortie au pas
    fautif (une rotation à probabilités quasi égales est le cas limite attendu).
    """

    def __init__(self, path: str) -> None:
        self.net = load_nn(path)
        self.policy = nn_policy(path)

    def action(self, obs: dict[str, Any]) -> dict[str, bool]:
        """Action de la politique Python (mêmes primitives que les touches)."""
        return {k: bool(self.policy(obs).get(k)) for k in KEYS}

    def detail(self, obs: dict[str, Any]) -> str:
        """Probabilités de sortie du réseau sur cette observation."""
        out = self.net.forward(obs_features(obs))
        sig = ", ".join(f"{v:.6f}" for v in out[:3])
        turn = ", ".join(f"{v:.6f}" for v in out[3:])
        return f"sigmoides(up,down,fire)=[{sig}] rotation(left,right,none)=[{turn}]"

#: Touches comparées (mêmes primitives que les touches du jeu).
KEYS = ("up", "down", "left", "right", "fire")

#: Politique embarquée dans le jeu (`include_str!` de `src/learned_pilot.rs`) -
#: chemin depuis `tools/trainer/` (le répertoire de travail habituel).
DEFAULT_POLICY = os.path.join("..", "..", "assets", "ship_pilot_policy.json")

#: Nombre de pas enregistrés par défaut dans le fixture (assez pour couvrir
#: décollage, minage, esquive et accostage sans alourdir le dépôt).
RECORD_LIMIT = 40


def _record_obs(obs: dict[str, Any]) -> dict[str, Any]:
    """Observation réduite au strict nécessaire des features : `nearby` et
    `bullets` sont tronqués aux `NEARBY_SLOTS` / `BULLET_SLOTS` premiers (les
    seuls lus par `obs_features`). Le reste est conservé tel quel, pour que le
    portage Rust reçoive exactement la même observation que la politique
    Python."""
    trimmed = dict(obs)
    trimmed["nearby"] = list(obs.get("nearby", []))[:NEARBY_SLOTS]
    trimmed["bullets"] = list(obs.get("bullets", []))[:BULLET_SLOTS]
    return trimmed


def validate_episode(
    client: DriverClient,
    reference: Reference,
    seed: int,
    scenario: str,
    seconds: float,
    record: Optional[list[dict[str, Any]]] = None,
    record_stride: int = 0,
) -> tuple[int, int, dict[str, int], str | None]:
    """Rejoue un épisode piloté par le **réseau embarqué du jeu** et compare ses
    décisions à celles de la politique Python.

    Renvoie `(pas comparés, pas identiques, désaccords par touche, dénouement)`.
    """
    client.reset(seed=seed, target="ship", scenario=scenario)
    client.cmd(autopilot=True, learned_pilot=True)
    seen = match = 0
    mism = {k: 0 for k in KEYS}
    last = 0
    outcome: str | None = None
    deadline = time.monotonic() + seconds
    # attendre l'ouverture de l'épisode (l'observation précédente peut encore
    # porter la piste verrouillée de l'épisode d'avant) **et** l'application de
    # la stratégie apprise : le `POST /cmd` est une requête séparée du `POST
    # /reset`, et tant que le jeu n'a pas consommé la bascule le champ `learned`
    # est **neutre** (le portage ne publie une action que si le cerveau conduit)
    # - comparer ces pas d'amorçage ferait échouer la mesure pour rien.
    while time.monotonic() < deadline:
        o = client.obs()
        if (o.get("frame", 0) > last and not o.get("episode_done")
                and o.get("learned_pilot")):
            break
        last = o.get("frame", 0)
        time.sleep(0.002)
    while time.monotonic() < deadline:
        o = client.obs()
        if o.get("frame", 0) <= last or not o.get("learned_pilot"):
            last = o.get("frame", last)
            continue
        last = o["frame"]
        if o.get("pilot") != "vaisseau":
            break
        if "learned" not in o:
            die("le jeu ne publie pas le champ `learned` : binaire trop ancien "
                "(recompiler avec src/learned_pilot.rs)")
        ours = reference.action(o)
        theirs = {k: bool(o["learned"].get(k)) for k in KEYS}
        seen += 1
        if all(ours[k] == theirs[k] for k in KEYS):
            match += 1
        else:
            for k in KEYS:
                if ours[k] != theirs[k]:
                    mism[k] += 1
            # pas fautif : qui, quoi, et l'état de la couche de sortie - sans
            # quoi un écart d'un pas sur des dizaines de milliers serait
            # indiagnostiquable
            diff = {k: ("Python" if ours[k] else "jeu") for k in KEYS if ours[k] != theirs[k]}
            print(f"             · pas {o.get('frame')} ({seen}e comparé) : {diff} · "
                  f"{reference.detail(o)}")
        if record is not None and record_stride > 0 and seen % record_stride == 0:
            if len(record) < RECORD_LIMIT:
                obs = _record_obs(o)
                record.append(
                    {"obs": obs, "features": obs_features(obs), "action": ours}
                )
        if o.get("episode_done"):
            outcome = o.get("episode_outcome")
            break
    return seen, match, mism, outcome


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", default="http://127.0.0.1:8643/", help="URL de l'interface du jeu")
    ap.add_argument("--policy", default=DEFAULT_POLICY,
                    help="politique embarquée dans le jeu (référence Python)")
    ap.add_argument("--seeds", type=int, nargs="+", default=[7, 21, 42],
                    help="graines des épisodes comparés")
    ap.add_argument("--scenario", default="economy",
                    help="règles de l'épisode (economy = boucle de minage du vaisseau)")
    ap.add_argument("--seconds", type=float, default=25.0,
                    help="durée maximale d'un épisode comparé (s mur)")
    ap.add_argument("--record", metavar="PATH", default=None,
                    help="écrire le fixture rejoué par le test unitaire Rust "
                         "(fenêtres obs/features/action)")
    args = ap.parse_args()

    # la référence Python est la politique **embarquée** : on recharge les
    # mêmes poids (et on vérifie au passage que le fichier est rejouable)
    reference = Reference(args.policy)
    client = DriverClient(args.host)
    if not client.reachable():
        die("le jeu ne répond pas sur " + args.host)

    print(f"Validation du portage de la politique apprise (scénario {args.scenario})")
    print("-" * 66)
    record: list[dict[str, Any]] = [] if args.record else None
    total_seen = total_match = 0
    for seed in args.seeds:
        # enregistrer au fil de l'eau : un pas sur ~40 pour étaler les fenêtres
        # sur la trajectoire (décollage, minage, accostage)
        seen, match, mism, outcome = validate_episode(
            client, reference, seed, args.scenario, args.seconds,
            record=record, record_stride=37)
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

    if record is not None:
        with open(args.record, "w", encoding="utf-8") as f:
            for w in record:
                f.write(json.dumps(w, ensure_ascii=False, separators=(",", ":")))
                f.write("\n")
        print(f"fixture : {len(record)} fenêtres écrites dans {args.record}")


if __name__ == "__main__":
    main()
