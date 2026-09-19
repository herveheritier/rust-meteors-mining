#!/usr/bin/env python3
"""Amorce par **imitation avec départs perturbés** pour la **boucle de minage
du vaisseau** (cible `ship`) - le pendant vaisseau de `warmstart.py` (EVA).

Le diagnostic de la Phase 3 vaut aussi pour le vaisseau : le clonage pur n'a
jamais vu les états **hors distribution** que la politique visite en boucle
fermée (vaisseau en vol, nez désaligné, vitesse d'approche trop rapide, soute
entamée, réserves basses…), et la politique y gèle. La correction est la même :
partir de **départs perturbés** (le jeu, lui, démarre toujours le vaisseau
**à quai**), dérouler l'expert depuis chacun et étiqueter chaque pas visité.

L'environnement est le **micro-simulateur vaisseau** (`ship_env.py`) :
cinématique, tir, minage, économie, **départ** (rétraction des liens) et
**accostage** (animation, boîte, livraison) fidèles, champ minier **importé du
jeu** quand la graine est enregistrée (`fixtures/ship_mining_fields.json` -
même graine, même monde, cf. `validate_ship_env.py`), géométrie des météores
approchée par des cercles (la frontière hybride restante). L'expert est le
**portage Python de l'autopilote vaisseau** (`ship_autopilot_ref.py`), validé à
100 % contre la loi du jeu.
Aucun processus headless n'est nécessaire : la boucle fermée contre
l'autopilote est mesurable hors ligne (`sim_comparison_ship`).

    # amorce : départs perturbés + entraînement du réseau, puis mesure
    python3 ship_warmstart.py --output ship_warmstart_policy.json --measure-sim
"""

from __future__ import annotations

import argparse
import math
import random
import sys
from typing import Any, Callable, Optional, Sequence

from nn import MLP, OUTPUT_COUNT, action_target, obs_features, save_nn
from ship_env import (
    REAL_FIELDS,
    ShipSim,
    episode_outcome,
    episode_reward,
    wrapped_dxdy,
)
from ship_autopilot_ref import autopilot_ship_inputs

#: Un état de départ d'épisode vaisseau :
#: `(seed, x, y, orientation, vx, vy, cargo, fuel, ammo, credits)` - la vitesse
#: en unités/s, mêmes axes que l'observation (`ship.vx/vy`). `fuel`/`ammo`/`cargo`
#: `None` = plein / vide (départ nominal).
ShipStart = tuple[Optional[int], float, float, float, float, float, int,
                  Optional[float], Optional[int], int]

#: Un pas d'imitation : `(features de l'observation, cibles de l'action experte)`.
Transition = tuple[list[float], list[float]]

#: Expert étiqueteur : `obs → commande` (mêmes primitives que les touches).
EXPERT = "autopilot"


def expert_policy() -> Callable[[dict[str, Any]], dict[str, bool]]:
    """L'expert vaisseau : le portage Python de l'autopilote du jeu
    (`ship_autopilot_ref.py`) - la référence à battre."""
    return autopilot_ship_inputs


def is_rare(action: dict[str, bool]) -> bool:
    """Pas « rare » : une commande active (poussée, frein, rotation ou tir) -
    l'expert passe une grande partie de son temps à ne rien faire."""
    return bool(action.get("up") or action.get("down") or action.get("left")
                or action.get("right") or action.get("fire"))


def perturbed_ship_starts(
    seeds: Sequence[int],
    dists: Sequence[float],
    rng: random.Random,
    *,
    dist_jitter: float = 0.0,
    angle_jitter: float = 0.0,
    speed_jitter: float = 0.0,
    cargo_starts: Sequence[int] = (),
    low_supply: bool = False,
    rest_aligned: int = 0,
) -> list[ShipStart]:
    """États de départ de l'entraînement, tirés des graines et distances.

    Chaque `(graine, distance)` produit :

    - le départ **nominal du jeu** : le vaisseau **à quai** au centre de la
      station, au repos, plein et soute vide (seul cas du jeu) ;
    - `rest_aligned` départs **au repos, nez aligné** sur la station à la
      distance donnée (l'état « aligné loin, à l'arrêt » où le clonage pur
      gèle) ;
    - un départ **tiré** : distance jittée ±`dist_jitter`, orientation décalée
      de ±`angle_jitter`, vitesse initiale tirée (±`speed_jitter`) ;
    - `cargo_starts` soutes de départ (vaisseau en cours de mission - la
      mission change : rentrer décharger) ;
    - `low_supply` : réserves basses (carburant/munitions), pour exercer la
      mission de ravitaillement de l'autopilote.

    Toutes les perturbations sont nulles par défaut : la fonction dégénère en
    départs nominaux (l'ancienne amorce).
    """
    starts: list[ShipStart] = []
    for seed in seeds:
        for dist in dists:
            # départ nominal du jeu : à quai au centre, au repos, plein
            starts.append((seed, 0.0, 0.0, 0.0, 0.0, 0.0, 0, None, None, 0))
            x, y = _position(seed, dist)
            aim = _aim_from_position(x, y)
            for _ in range(max(0, rest_aligned)):
                starts.append((seed, x, y, aim, 0.0, 0.0, 0, None, None, 0))
            if dist_jitter > 0.0 or angle_jitter > 0.0 or speed_jitter > 0.0:
                xj, yj = _position(seed, max(15.0, dist + rng.uniform(-dist_jitter, dist_jitter)))
                aim_j = _aim_from_position(xj, yj)
                orientation = aim_j + rng.uniform(-angle_jitter, angle_jitter)
                speed = rng.uniform(0.0, speed_jitter)
                v_angle = rng.uniform(0.0, math.tau)
                starts.append((seed, xj, yj, orientation,
                               math.cos(v_angle) * speed, math.sin(v_angle) * speed,
                               0, None, None, 0))
            for cargo in cargo_starts:
                starts.append((seed, x, y, aim, 0.0, 0.0, cargo, None, None, 0))
            if low_supply:
                starts.append((seed, x, y, aim, 0.0, 0.0, 0, 10.0, 2, 0))
    return starts


def _position(seed: int, dist: float) -> tuple[float, float]:
    """Position de départ autour de la station : distance donnée, direction
    tirée de la graine (déterministe, comme le crash EVA)."""
    rng = random.Random(seed)
    angle = rng.uniform(0.0, math.tau)
    return math.cos(angle) * dist, math.sin(angle) * dist


def _aim_from_position(x: float, y: float) -> float:
    """Angle écran de la station (centre 0,0 du simulateur) depuis `(x, y)`."""
    dx, dy = wrapped_dxdy(x, y, 0.0, 0.0)
    return math.atan2(dy, dx)


def default_ship_perturbation(spawn_dist: float, scale: float = 1.0) -> dict[str, Any]:
    """Perturbation par défaut : départs au repos alignés (l'état qui gèle),
    couronne de distances, orientations désalignées, vitesses initiales,
    soutes entamées et réserves basses. `scale = 0` = perturbation nulle
    (départs nominaux seulement)."""
    if scale <= 0.0:
        return {
            "dist_jitter": 0.0,
            "angle_jitter": 0.0,
            "speed_jitter": 0.0,
            "cargo_starts": (),
            "low_supply": False,
            "rest_aligned": 0,
        }
    return {
        "dist_jitter": spawn_dist * 0.5 * scale,
        "angle_jitter": math.radians(45.0) * scale,
        "speed_jitter": 40.0 * scale,
        "cargo_starts": (2, 4),
        "low_supply": True,
        "rest_aligned": 2,
    }


def balance_actions(
    transitions: Sequence[Transition],
    cmd_actions: Sequence[dict[str, bool]],
    rare_repeat: int = 4,
    max_samples: int = 6000,
    rng: Optional[random.Random] = None,
) -> list[Transition]:
    """Rééquilibre les classes en dupliquant les pas actifs (adaptatif : rien
    si les pas actifs sont déjà majoritaires), puis **plafonne** le jeu de
    données (`max_samples`) : le MLP en Python pur est lent - un jeu trop gros
    ne tient pas la passe sans rien apporter (les pas voisins sont quasi
    identiques)."""
    rare = [t for t, a in zip(transitions, cmd_actions) if is_rare(a)]
    common = [t for t, a in zip(transitions, cmd_actions) if not is_rare(a)]
    if not rare:
        out = list(common)
    else:
        repeat = max(1, min(rare_repeat, round(len(common) / len(rare))))
        out = list(common) + rare * repeat
    if len(out) > max_samples:
        rng = rng or random.Random(0)
        out = rng.sample(out, max_samples)
    return out


def cap_dataset(
    X: Sequence[list[float]],
    Y: Sequence[list[float]],
    cap: int,
    rng: random.Random,
) -> tuple[list[list[float]], list[list[float]]]:
    """Plafonne un jeu (X, Y) en échantillonnant uniformément - le MLP en
    Python pur est lent, et les pas voisins sont quasi identiques."""
    if cap <= 0 or len(X) <= cap:
        return list(X), list(Y)
    idx = rng.sample(range(len(X)), cap)
    return [X[i] for i in idx], [Y[i] for i in idx]


def ship_dagger_transitions(
    policy: Callable[[dict[str, Any]], dict[str, bool]],
    starts: Sequence[ShipStart],
    timeout: float,
    expert: Optional[Callable[[dict[str, Any]], dict[str, bool]]] = None,
    stride: int = 1,
    by_seed: Optional[dict[int, list[Transition]]] = None,
    stall_steps: int = 0,
) -> dict[int, list[Transition]]:
    """Une itération **DAgger** (Dataset Aggregation) : la **politique** joue
    l'épisode elle-même (en boucle fermée dans le simulateur), et chaque état
    qu'elle visite est étiqueté par l'action de l'**expert** sur cet état.

    C'est le remède à la **dérive de distribution** : le clonage n'apprend que
    sur les états de l'expert ; dès qu'une erreur l'en écarte, la politique
    n'a aucun label pour s'en rattraper et **gèle** (mesuré : le réseau se
    gare à ~400 u de la station et n'en sort plus). Les états qu'elle crée
    elle-même, étiquetés par l'expert, sont **agrégés** au jeu de données
    avant le ré-entraînement - elle apprend à se rattraper là où elle tombe.

    `stride` sous-échantillonne les pas (la trappe est un point fixe : les pas
    y sont quasi identiques) ; les transitions sont **ajoutées** à `by_seed`,
    qui est renvoyé (clé = graine de l'épisode).

    `stall_steps` (0 = désactivé) **coupe l'épisode** après ce nombre de pas
    consécutifs où la politique ne commande **rien** : la trappe mesurée est
    exactement cet état (toutes les sorties à `False`), et y rejouer des
    milliers de pas ne produit aucun état nouveau - cela ne coûte que du temps
    de simulateur (~1,3 ms le pas en Python pur, soit ~10 min par itération).
    Le seuil doit rester **large** devant la séquence d'accostage (1,5 s de
    rétraction + 3 s d'animation, entrées ignorées) et devant un simple
    vol balistique : 10 s (600 pas) ne coupe que ce qui est réellement figé.
    """
    expert = expert or expert_policy()
    out = by_seed if by_seed is not None else {}
    env = ShipSim()
    for seed, x, y, orientation, vx, vy, cargo, fuel, ammo, credits in starts:
        key = seed if seed is not None else 0
        obs = env.reset(key, x, y, orientation, vx, vy,
                        cargo=cargo, fuel=fuel, ammo=ammo, credits=credits)
        step = 0
        idle = 0
        while not env.done and env.t < timeout:
            if step % stride == 0:
                out.setdefault(key, []).append(
                    (obs_features(obs), action_target(expert(obs))))
            cmd = policy(obs)
            if stall_steps and not any(cmd.values()):
                idle += 1
                if idle >= stall_steps:
                    break
            else:
                idle = 0
            step += 1
            obs = env.step(cmd["up"], cmd["down"], cmd["left"], cmd["right"],
                           cmd["fire"])
    return out


def ship_expert_transitions(
    seeds: Sequence[int],
    dists: Sequence[float],
    rng: random.Random,
    timeout: float,
    stride: int = 1,
    rare_repeat: int = 4,
    balance_cap: int = 6000,
    expert: Optional[Callable[[dict[str, Any]], dict[str, bool]]] = None,
    **perturb: Any,
) -> list[Transition]:
    """Amorce complète : départs perturbés, déroulé de l'expert, rééquilibrage
    des classes. `balance_cap` plafonne les pas conservés par graine (le
    rééquilibrage duplique les pas actifs : sans plafond, une graine pèse plus
    que les autres et la validation par graine devient biaisée)."""
    starts = perturbed_ship_starts(seeds, dists, rng, **perturb)
    expert = expert or expert_policy()
    # les commandes sont recalculées pour le rééquilibrage : on les conserve
    # en parallèle des transitions (elles ne sont pas dans le vecteur cible)
    transitions: list[Transition] = []
    actions: list[dict[str, bool]] = []
    env = ShipSim()
    for seed, x, y, orientation, vx, vy, cargo, fuel, ammo, credits in starts:
        obs = env.reset(seed if seed is not None else 0, x, y, orientation, vx, vy,
                        cargo=cargo, fuel=fuel, ammo=ammo, credits=credits)
        step = 0
        while not env.done and env.t < timeout:
            cmd = expert(obs)
            if step % stride == 0:
                transitions.append((obs_features(obs), action_target(cmd)))
                actions.append(cmd)
            step += 1
            obs = env.step(cmd["up"], cmd["down"], cmd["left"], cmd["right"], cmd["fire"])
    return balance_actions(transitions, actions, rare_repeat,
                           max_samples=balance_cap, rng=rng)


# ── mesure en boucle fermée contre l'autopilote ─────────────────────────────

def docked_ship_starts(seeds: Sequence[int]) -> list[ShipStart]:
    """Départs **du jeu** : le vaisseau est **à quai** (liens attachés, vitesse
    nulle, soute vide) - c'est la distribution de départ de la boucle de minage,
    celle que mesurent les épisodes de comparaison, donc celle sur laquelle
    DAgger doit rouler pour visiter les états que la politique crée en partie."""
    return [(s, 0.0, 0.0, 0.0, 0.0, 0.0, 0, None, None, 0) for s in seeds]


def run_ship_episode(
    env: ShipSim,
    policy: Callable[[dict[str, Any]], dict[str, bool]],
    start: ShipStart,
    timeout: float,
) -> dict[str, Any]:
    """Un épisode du simulateur piloté par `policy` depuis `start`."""
    seed, x, y, orientation, vx, vy, cargo, fuel, ammo, credits = start
    obs = env.reset(seed if seed is not None else 0, x, y, orientation, vx, vy,
                    cargo=cargo, fuel=fuel, ammo=ammo, credits=credits)
    while not env.done and env.t < timeout:
        cmd = policy(obs)
        obs = env.step(cmd["up"], cmd["down"], cmd["left"], cmd["right"], cmd["fire"])
    outcome = episode_outcome(env)
    outcome["seed"] = seed
    outcome["reward"] = episode_reward(None, outcome)
    return outcome


def sim_comparison_ship(
    policy: Callable[[dict[str, Any]], dict[str, bool]],
    seeds: Sequence[int],
    timeout: float = 150.0,
    reference: Optional[Callable[[dict[str, Any]], dict[str, bool]]] = None,
) -> dict[str, Any]:
    """Compare une politique à l'autopilote vaisseau porté, **mêmes épisodes**,
    dans le micro-simulateur (écart de récompense hors ligne, sans headless).

    Chaque épisode part **à quai** (le départ du jeu) : c'est la boucle de
    minage complète (décoller → miner → décharger) que mesurent les deux
    politiques."""
    reference = reference or autopilot_ship_inputs
    starts = docked_ship_starts(seeds)
    env = ShipSim()
    pol = [run_ship_episode(env, policy, st, timeout) for st in starts]
    ref = [run_ship_episode(env, reference, st, timeout) for st in starts]
    p_mean = sum(r["reward"] for r in pol) / len(pol)
    r_mean = sum(r["reward"] for r in ref) / len(ref)
    return {
        "seeds": list(seeds),
        "policy": pol,
        "reference": ref,
        "policy_mean": p_mean,
        "reference_mean": r_mean,
        "gap": p_mean - r_mean,
        "policy_ok": sum(1 for r in pol if r["success"]),
        "reference_ok": sum(1 for r in ref if r["success"]),
    }


def print_ship_comparison(cmp: dict[str, Any], policy_label: str = "politique") -> None:
    """Rapport lisible de `sim_comparison_ship`."""
    print(f"\nComparaison hors-ligne (simulateur vaisseau) : {policy_label} vs autopilote")
    print(f"Boucle de minage complète · graines "
          f"{cmp['seeds'][0]}..{cmp['seeds'][-1]}")
    print("-" * 66)
    print(f"{'graine':>7} {'politique':>18} {'autopilote':>18}")
    for p, r in zip(cmp["policy"], cmp["reference"]):
        p_out = p["outcome"] or "délai"
        r_out = r["outcome"] or "délai"
        print(f"{p['seed']:>7} {p_out:>10} {p['reward']:>7.1f} "
              f"{r_out:>10} {r['reward']:>7.1f}")
    print("-" * 66)
    print(f"{policy_label:>10} : {cmp['policy_ok']}/{len(cmp['policy'])} livraisons · "
          f"récompense moyenne {cmp['policy_mean']:.1f}")
    print(f"autopilote : {cmp['reference_ok']}/{len(cmp['reference'])} livraisons · "
          f"récompense moyenne {cmp['reference_mean']:.1f}")
    sign = "+" if cmp["gap"] >= 0 else ""
    print(f"écart (politique − autopilote) : {sign}{cmp['gap']:.1f}")


def split_by_seed(seeds: Sequence[int], by_seed: dict[int, list[Transition]],
                  val_fraction: float) -> tuple[list, ...]:
    """Sépare train/validation **par graine** (comme `imitate.py`)."""
    ordered = sorted(seeds)
    n_val = max(1, round(len(ordered) * val_fraction))
    val = ordered[-n_val:]
    train = ordered[:-n_val] or ordered[:1]
    X_tr: list[list[float]] = []
    Y_tr: list[list[float]] = []
    X_va: list[list[float]] = []
    Y_va: list[list[float]] = []
    for s, rows in by_seed.items():
        xs = X_va if s in val else X_tr
        ys = Y_va if s in val else Y_tr
        for x, y in rows:
            xs.append(x)
            ys.append(y)
    return X_tr, Y_tr, X_va, Y_va


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seeds", type=int, nargs="+", default=list(range(1, 13)),
                    help="graines des départs (défaut 1..12)")
    ap.add_argument("--spawn-dists", default="300,600,900,1200", metavar="D1,D2,..",
                    help="distances de départ autour de la station (unités)")
    ap.add_argument("--scale", type=float, default=1.0,
                    help="échelle de la perturbation (0 = départs nominaux)")
    ap.add_argument("--timeout", type=float, default=90.0, help="délai d'un déroulé (s)")
    ap.add_argument("--stride", type=int, default=4,
                    help="sous-échantillonne les pas (un sur N) - le déroulé de l'expert "
                         "coûte le même temps quel que soit N, seul le jeu de données change")
    ap.add_argument("--balance-cap", type=int, default=20000,
                    help="pas conservés par graine après rééquilibrage (défaut 20000)")
    ap.add_argument("--hidden", type=int, default=64, help="neurones de la couche cachée")
    ap.add_argument("--epochs", type=int, default=300, help="époques d'entraînement max")
    ap.add_argument("--patience", type=int, default=25,
                    help="époques sans progrès avant arrêt précoce")
    ap.add_argument("--noise", type=float, default=0.0,
                    help="écart-type du bruit d'entrée (régularisation) : un réseau "
                         "qui atteint 100 %% sur le simulateur sature ses sorties et "
                         "devient cassant hors distribution (mesuré dans le jeu)")
    ap.add_argument("--lr", type=float, default=0.1, help="taux d'apprentissage")
    ap.add_argument("--batch", type=int, default=64, help="taille des mini-lots")
    ap.add_argument("--train-cap", type=int, default=24000,
                    help="taille maximale du jeu d'entraînement (échantillonné)")
    ap.add_argument("--backend", choices=("python", "numpy", "auto"), default="auto",
                    help="moteur d'entraînement : python (sans dépendance), numpy "
                         "(vectorisé, exige numpy) ou auto (numpy si disponible)")
    ap.add_argument("--val-fraction", type=float, default=0.25,
                    help="fraction des graines réservée à la validation")
    ap.add_argument("--dagger-iterations", type=int, default=0,
                    help="itérations DAgger dans le simulateur (0 = clonage seul) : la "
                         "politique joue, l'expert étiquette les états qu'elle visite, "
                         "on ré-entraîne sur le jeu agrégé")
    ap.add_argument("--dagger-starts", choices=("dock", "perturbed", "both"),
                    default="dock",
                    help="distribution de départ des roulages DAgger : `dock` (défaut) = les "
                         "départs **du jeu** à quai, où la politique suit **sa propre** "
                         "trajectoire jusqu'à ses points fixes - c'est la seule façon de "
                         "visiter les états qu'elle crée en partie (mesuré : partant à 600 u, "
                         "les roulages ne tombaient jamais sur la trappe de l'évaluation) ; "
                         "`perturbed` = départs écartés `--dagger-dists` ; `both`")
    ap.add_argument("--dagger-dists", default=None, metavar="D1,D2,..",
                    help="distances de départ des roulages **DAgger** (défaut : celles de "
                         "l'amorce). Les états de dérive apparaissent à toute distance : "
                         "une seule distance suffit et divise le coût par le nombre de "
                         "distances - le roulage d'une politique qui cale va jusqu'au "
                         "délai de l'épisode (~1,3 ms par pas en Python pur)")
    ap.add_argument("--dagger-stride", type=int, default=16,
                    help="sous-échantillonnage des pas DAgger (la trappe est un point fixe)")
    ap.add_argument("--dagger-stall", type=int, default=600,
                    help="pas consécutifs sans aucune commande avant de couper un roulage "
                         "DAgger (0 = désactivé) : la trappe est un point fixe, la rejouer "
                         "des milliers de pas ne produit aucun état nouveau")
    ap.add_argument("--dagger-mix", type=float, default=0.5,
                    help="part maximale du jeu d'entraînement occupée par les états DAgger")
    ap.add_argument("--rng-seed", type=int, default=0, help="graine du mélange / perturbations")
    ap.add_argument("--output", default="ship_warmstart_policy.json", metavar="PATH",
                    help="politique entraînée (sortie)")
    ap.add_argument("--measure-sim", action="store_true",
                    help="mesurer la boucle fermée contre l'autopilote porté (mêmes graines)")
    ap.add_argument("--measure-seeds", type=int, nargs="+", default=list(range(1, 7)),
                    help="graines de la mesure hors-ligne (défaut 1..6)")
    ap.add_argument("--measure-timeout", type=float, default=150.0,
                    help="délai d'un épisode de mesure (s)")
    args = ap.parse_args()

    dists = [float(d) for d in args.spawn_dists.split(",") if d.strip()]
    rng = random.Random(args.rng_seed)
    perturb = default_ship_perturbation(dists[0] if dists else 600.0, args.scale)

    # le simulateur devient **représentatif** quand la graine a son champ
    # minier enregistré (`fixtures/ship_mining_fields.json`) : il rejoue alors
    # le monde du jeu, et les états visités (donc l'imitation) portent sur la
    # même géométrie que la partie. Sans champ, le monde est **synthétisé**.
    without_field = [s for s in args.seeds if s not in REAL_FIELDS]
    if without_field:
        print(f"  ⚠ graines sans champ réel enregistré (monde **synthétisé**, "
              f"non représentatif) : {without_field}")
        print(f"    enregistrer les champs depuis une partie : measure_in_game.py "
              f"--seeds {without_field[0]} --fields fixtures/ship_mining_fields.json")

    # déroulé de l'expert depuis chaque départ, transitions agrégées par graine
    by_seed: dict[int, list[Transition]] = {}
    for seed in args.seeds:
        transitions = ship_expert_transitions(
            [seed], dists, rng, args.timeout, stride=args.stride,
            balance_cap=args.balance_cap, **perturb)
        by_seed[seed] = transitions
    print(f"Déroulé de l'expert terminé ({len(args.seeds)} graines)")
    total = sum(len(v) for v in by_seed.values())
    print(f"Amorce : {len(args.seeds)} graines × {len(dists)} distances · "
          f"{total} pas étiquetés (expert autopilote vaisseau)")

    X_tr, Y_tr, X_va, Y_va = split_by_seed(args.seeds, by_seed, args.val_fraction)
    if not X_tr or not X_va:
        raise SystemExit("✗ pas assez de graines pour séparer train/validation "
                         "(augmentez --seeds)")
    # plafonne l'ensemble d'entraînement (le chemin Python pur est lent : sans
    # numpy, ~17 s par époque et par 6 000 pas à 64 cachés)
    X_tr, Y_tr = cap_dataset(X_tr, Y_tr, args.train_cap, rng)
    print(f"  jeu d'entraînement : {len(X_tr)} pas · validation {len(X_va)} pas "
          f"(backend {args.backend})")
    # les réglages par défaut sont dimensionnés pour le moteur vectorisé : sur
    # le Python pur (~100 × plus lent), ils prennent des heures. Mieux vaut le
    # dire avant que de laisser croire à un blocage.
    if args.backend != "numpy":
        effective = args.backend
        if effective == "auto":
            try:
                import numpy  # noqa: F401
            except ImportError:
                effective = "python"
            else:
                effective = "numpy"
        if effective == "python" and len(X_tr) * args.epochs * args.hidden > 5_000_000:
            print("  ⚠ moteur Python pur : ces réglages peuvent prendre des heures "
                  "- installer numpy (backend numpy/auto), ou réduire "
                  "--hidden / --epochs / --train-cap")
    net = MLP(len(X_tr[0]), args.hidden, OUTPUT_COUNT, rng)
    res = net.train(X_tr, Y_tr, epochs=args.epochs, lr=args.lr,
                    batch_size=args.batch, patience=args.patience,
                    noise=args.noise, rng=rng, backend=args.backend)
    s_tr = net.summary(X_tr, Y_tr)
    s_va = net.summary(X_va, Y_va)
    print(f"  réseau : {net.inputs} entrées → {net.hidden} cachées → {net.outputs} sorties · "
          f"exactitude train {s_tr['accuracy'] * 100:.1f} % · "
          f"val {s_va['accuracy'] * 100:.1f} % · "
          f"{res.get('epochs_done', args.epochs)} époques "
          f"(moteur {res.get('backend', 'python')})")

    def policy(obs: dict[str, Any]) -> dict[str, bool]:
        out = net.forward(obs_features(obs))
        cmd = {"up": out[0] >= 0.5, "down": out[1] >= 0.5, "fire": out[2] >= 0.5}
        turn = net.turn_action(out)
        cmd["left"] = turn == "left"
        cmd["right"] = turn == "right"
        return cmd

    meta = {
        "method": "warmstart_ship",
        "seeds": args.seeds,
        "spawn_dists": dists,
        "scale": args.scale,
        "expert": EXPERT,
        "timeout": args.timeout,
        "stride": args.stride,
        "balance_cap": args.balance_cap,
        "steps": total,
        "hidden": args.hidden,
        "backend": res.get("backend", "python"),
        "train_steps": len(X_tr),
        "val_steps": len(X_va),
        "train_cap": args.train_cap,
        "patience": args.patience,
        "noise": args.noise,
        "epochs_done": res.get("epochs_done", args.epochs),
        "train_accuracy": s_tr["accuracy"],
        "val_accuracy": s_va["accuracy"],
        "rng_seed": args.rng_seed,
        "dagger_iterations": args.dagger_iterations,
        "dagger_dists": None,
        "dagger_starts": args.dagger_starts,
        "dagger_stride": args.dagger_stride,
        "dagger_stall": args.dagger_stall,
        "dagger_mix": args.dagger_mix,
        "dagger_iterations_done": 0,
        "dagger_states_added": 0,
    }

    # Point de reprise : l'amorce est déjà une politique exploitable, on
    # l'écrit **avant** les itérations DAgger (les roulages durent des dizaines
    # de minutes et un run interrompu ne doit pas tout perdre).
    save_nn(args.output, net, meta)
    print(f"\nAmorce écrite dans {args.output}")
    if args.measure_sim:
        cmp = sim_comparison_ship(policy, list(args.measure_seeds),
                                  timeout=args.measure_timeout)
        print_ship_comparison(cmp, "amorce (clonage seul)")

    # ── itérations DAgger : la politique joue, l'expert la corrige ──────────
    # Le clonage seul ne tient pas la boucle fermée : la politique dérive vers
    # des états que l'expert n'a jamais montrés (mesuré : elle se gare à ~400 u
    # de la station, soute vide, et n'en sort plus - la tête de rotation
    # choisit « none » avec une confiance de 1.0 alors que l'expert tourne).
    dagger_dists = dists
    if args.dagger_dists:
        dagger_dists = [float(d) for d in args.dagger_dists.split(",") if d.strip()]
    meta["dagger_dists"] = dagger_dists
    added_total = 0
    for it in range(1, args.dagger_iterations + 1):
        starts = []
        if args.dagger_starts in ("dock", "both"):
            starts += docked_ship_starts(args.seeds)
        if args.dagger_starts in ("perturbed", "both"):
            starts += perturbed_ship_starts(args.seeds, dagger_dists, rng, **perturb)
        dagger: dict[int, list[Transition]] = {}
        ship_dagger_transitions(policy, starts, args.timeout, stride=args.dagger_stride,
                                by_seed=dagger, stall_steps=args.dagger_stall)
        # la politique piétine : ses états s'accumulent sans diversité (la
        # trappe est un point fixe). On plafonne l'apport DAgger à une part du
        # jeu d'entraînement, graine par graine, pour ne pas noyer l'amorce.
        budget = max(1, round(args.train_cap * args.dagger_mix / len(args.seeds)))
        added = 0
        for seed, rows in dagger.items():
            if len(rows) > budget:
                rows = [rows[i] for i in rng.sample(range(len(rows)), budget)]
            by_seed.setdefault(seed, []).extend(rows)
            added += len(rows)
        added_total += added
        X_tr, Y_tr, X_va, Y_va = split_by_seed(args.seeds, by_seed, args.val_fraction)
        X_tr, Y_tr = cap_dataset(X_tr, Y_tr, args.train_cap, rng)
        res = net.train(X_tr, Y_tr, epochs=args.epochs, lr=args.lr,
                        batch_size=args.batch, patience=args.patience,
                        noise=args.noise, rng=rng, backend=args.backend)
        s_tr = net.summary(X_tr, Y_tr)
        s_va = net.summary(X_va, Y_va)
        print(f"  DAgger {it}/{args.dagger_iterations} : {len(starts)} départs joués par "
              f"la politique · {added} états corrigés · jeu {len(X_tr)} pas · "
              f"exactitude train {s_tr['accuracy'] * 100:.1f} % · "
              f"val {s_va['accuracy'] * 100:.1f} %")
        if args.measure_sim:
            cmp = sim_comparison_ship(policy, list(args.measure_seeds),
                                      timeout=args.measure_timeout)
            print_ship_comparison(cmp, f"DAgger {it}")
        # point de reprise après chaque itération : le run reprend ici (et
        # l'artefact sur le disque est toujours celui que la mesure vient de
        # qualifier)
        meta.update({
            "steps": total + added_total,
            "train_steps": len(X_tr),
            "val_steps": len(X_va),
            "epochs_done": res.get("epochs_done", args.epochs),
            "train_accuracy": s_tr["accuracy"],
            "val_accuracy": s_va["accuracy"],
            "dagger_iterations_done": it,
            "dagger_states_added": added_total,
        })
        save_nn(args.output, net, meta)
        print(f"  politique écrite dans {args.output} (reprise DAgger {it} "
              f"consignée dans les métadonnées)")

    save_nn(args.output, net, meta)
    print(f"\nPolitique écrite dans {args.output}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
