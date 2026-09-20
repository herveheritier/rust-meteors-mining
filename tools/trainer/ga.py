#!/usr/bin/env python3
"""Algorithme **génétique sélectif** de l'autopilote vaisseau : une population
de pilotes candidats est évaluée en boucle fermée puis sélectionnée (élitisme +
tournoi), croisée et mutée jusqu'à converger - l'équivalent génétique de
`cem.py` (qui n'a ni croisement ni population large), pour la **boucle de
minage** du vaisseau (décoller → miner → décharger).

Deux **génomes** sont proposés :

- **A - loi paramétrée** (`--genome law`) : les constantes de conduite de la
  loi portée de l'autopilote du jeu (`ship_autopilot_ref.py`) deviennent des
  gènes bornés. La structure de la loi est conservée (missions, visée, tir) -
  le GA cherche **son meilleur réglage**, viable dès la génération 0 (le
  réglage du jeu est semé dans la population initiale).
- **B - réseau** (`--genome nn`) : les poids du MLP (`nn.py`, le même format
  que `assets/ship_pilot_policy.json`) sont aplatis en gènes continus. La
  population initiale est faite de **perturbations gaussiennes d'un réseau
  existant** (amorce d'imitation recommandée : `ship_warmstart.py` ou
  `imitate.py`) : le GA affine en boucle fermée là où le clonage échoue.

La **fitness** d'un candidat est la moyenne des récompenses d'épisodes
(`ship_env.episode_outcome`, les règles du jeu) sur K graines, moins
`lambda * écart-type` : un pilote qui réussit 2 graines sur 3 très vite doit
perdre contre celui qui réussit les 3. Les graines de **validation** sont
disjointes et ne servent qu'à la mesure finale (anti-surapprentissage).

La politique gagnante est écrite en `ga_policy.json` (génome A) ou en
`nn_policy.json` compatible `load_nn` (génome B) - rejouable par
`evaluate.py --strategy ga --policy ...`, comparable à l'autopilote porté
(`sim_comparison_ship`), déployable en jeu pour B via
`ship_warmstart.py`-style (`assets/ship_pilot_policy.json`, touche Y).

    python3 ga.py                              # génome A, simulateur (rapide)
    python3 ga.py --view                       # idem, avec l'afficheur pixels
    python3 ga.py --genome nn --warmstart nn_policy.json
    python3 ga.py --backend live               # contre la vraie partie (headless)

Python **standard uniquement** - le simulateur fait quelques ms par épisode,
une génération entière se joue en quelques secondes.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics
import sys
from typing import Any, Callable, Optional, Sequence

# le simulateur et la loi portée sont les mêmes pour tout l'entraîneur
from ship_env import ShipSim, episode_outcome
from eva_env import episode_reward
import ship_autopilot_ref as law

Policy = Callable[[dict[str, Any]], dict[str, bool]]

# ── génome A : les gènes de la loi paramétrée ────────────────────────────────

#: Bornes de recherche (min, max) par gène - étendues volontairement plus
#: larges que les valeurs du jeu (unités/frame pour les vitesses, rad pour les
#: tolérances d'alignement, unités pour les rayons). Les valeurs du jeu
#: (`LAW_DEFAULTS`) tiennent dedans : le réglage actuel est un membre possible.
LAW_BOUNDS: dict[str, tuple[float, float]] = {
    "CRUISE_SPEED": (0.8, 3.5),
    "ATTACK_STANDOFF": (20.0, 140.0),
    "STATION_GUARD_RADIUS": (120.0, 400.0),
    "FIRE_RANGE": (120.0, 320.0),
    "AIM_TOLERANCE": (0.05, 0.45),
    "THRUST_DEADBAND": (0.05, 0.45),
    "TURN_DEADBAND": (0.03, 0.35),
    "PATROL_RADIUS": (40.0, 240.0),
    "DOCK_SLOW_ZONE": (30.0, 180.0),
    "LOW_SUPPLY_RATIO": (0.1, 0.6),
    "MINERAL_CLEARANCE": (60.0, 220.0),
    "AVOID_RADIUS": (180.0, 520.0),
    "AVOID_CLEARANCE": (40.0, 160.0),
    "AVOID_TIME": (0.6, 2.6),
    "DODGE_BRAKE_SPEED": (0.0, 1.5),
}

#: Les constantes **non gènes** de la loi restent à leur valeur du jeu
#: (retenue de feu : le comportement est structurel, pas un réglage).
LAW_DEFAULTS: dict[str, float] = {
    "CRUISE_SPEED": law.CRUISE_SPEED,
    "ATTACK_STANDOFF": law.ATTACK_STANDOFF,
    "STATION_GUARD_RADIUS": law.STATION_GUARD_RADIUS,
    "FIRE_RANGE": law.FIRE_RANGE,
    "AIM_TOLERANCE": law.AIM_TOLERANCE,
    "THRUST_DEADBAND": law.THRUST_DEADBAND,
    "TURN_DEADBAND": law.TURN_DEADBAND,
    "PATROL_RADIUS": law.PATROL_RADIUS,
    "DOCK_SLOW_ZONE": law.DOCK_SLOW_ZONE,
    "LOW_SUPPLY_RATIO": law.LOW_SUPPLY_RATIO,
    "MINERAL_CLEARANCE": law.MINERAL_CLEARANCE,
    "AVOID_RADIUS": law.AVOID_RADIUS,
    "AVOID_CLEARANCE": law.AVOID_CLEARANCE,
    "AVOID_TIME": law.AVOID_TIME,
    "DODGE_BRAKE_SPEED": law.DODGE_BRAKE_SPEED,
}


def clip_law(gene: dict[str, float]) -> dict[str, float]:
    """Ramène chaque gène dans ses bornes (les tirages croisés/mutations)."""
    return {k: min(max(v, LAW_BOUNDS[k][0]), LAW_BOUNDS[k][1]) for k, v in gene.items()}


def make_law_policy(gene: dict[str, float]) -> Policy:
    """Politique de la loi portée **recâblée** sur les gènes : les constantes
    de module de `ship_autopilot_ref` sont substituées le temps de la décision
    (et restaurées ensuite) - aucune copie de la loi, une seule source."""
    restored: dict[str, float] = {}

    def policy(obs: dict[str, Any]) -> dict[str, bool]:
        for k, v in gene.items():
            restored[k] = getattr(law, k)
            setattr(law, k, v)
        try:
            return law.autopilot_ship_inputs(obs)
        finally:
            for k, v in restored.items():
                setattr(law, k, v)

    return policy


# ── génome B : les poids du MLP ──────────────────────────────────────────────

NN_GENOME_LABEL = "nn"


def nn_flat_layers(inputs: int, hidden: int, outputs: int) -> list[tuple[str, int, int]]:
    """Dimensions des blocs de poids, dans l'ordre d'aplatissement."""
    return [("w1", inputs, hidden), ("b1", 1, hidden),
            ("w2", hidden, outputs), ("b2", 1, outputs)]


def nn_flatten(net: Any) -> list[float]:
    """Aplatit les poids d'un MLP en génome continu."""
    g: list[float] = []
    for v in net.w1:
        g.extend(v)
    g.extend(net.b1)
    for row in net.w2:
        g.extend(row)
    g.extend(net.b2)
    return g


def nn_unflatten(net: Any, genome: Sequence[float]) -> None:
    """Écrit un génome aplati dans les poids d'un MLP (dimensions préservées)."""
    i = 0
    for row in net.w1:
        for j in range(len(row)):
            row[j] = genome[i]
            i += 1
    for j in range(len(net.b1)):
        net.b1[j] = genome[i]
        i += 1
    for row in net.w2:
        for j in range(len(row)):
            row[j] = genome[i]
            i += 1
    for j in range(len(net.b2)):
        net.b2[j] = genome[i]
        i += 1


def make_nn_policy(net: Any) -> Policy:
    """Politique du réseau : mêmes têtes que `policies.nn_policy` (sigmoïdes
    up/down/fire puis rotation softmax mutuellement exclusive)."""

    from nn import SIGMOID_OUTPUTS

    def policy(obs: dict[str, Any]) -> dict[str, bool]:
        out = net.forward(obs_features_cache(obs))
        cmd = {"up": False, "down": False, "left": False, "right": False, "fire": False}
        for a, v in zip(("up", "down", "fire"), out[:SIGMOID_OUTPUTS]):
            cmd[a] = v >= 0.5
        turn = ("left", "right", "none")[
            max(range(3), key=lambda k: out[SIGMOID_OUTPUTS + k])]
        cmd["left"] = turn == "left"
        cmd["right"] = turn == "right"
        return cmd

    return policy


def obs_features_cache(obs: dict[str, Any]) -> list[float]:
    """Features d'observation (import local : nn.py est lourd à charger)."""
    from nn import obs_features

    return obs_features(obs)


# ── évaluation : la fitness d'un candidat ────────────────────────────────────

def run_sim_episode(env: ShipSim, policy: Policy, seed: int, timeout: float) -> dict[str, Any]:
    """Un épisode de boucle de minage dans le micro-simulateur : départ à quai
    (le départ du jeu), politique obs → commande jusqu'au dénouement."""
    obs = env.reset(seed)
    while not env.done and env.t < timeout:
        cmd = policy(obs)
        obs = env.step(cmd["up"], cmd["down"], cmd["left"], cmd["right"], cmd["fire"])
    outcome = episode_outcome(env)
    outcome["seed"] = seed
    outcome["reward"] = episode_reward(None, outcome)
    return outcome


def fitness(
    policy: Policy,
    seeds: Sequence[int],
    timeout: float,
    robustness: float,
    env: Optional[ShipSim] = None,
) -> float:
    """Fitness d'un candidat : moyenne des récompenses sur les graines moins
    `robustness × écart-type` (0 = moyenne seule, 1 = pénalité forte de
    variance). Un épisode court et réussi vaut plus qu'un épisode qui traîne."""
    if env is None:
        env = ShipSim()
    rewards = [run_sim_episode(env, policy, s, timeout)["reward"] for s in seeds]
    mean = sum(rewards) / len(rewards)
    if robustness <= 0.0 or len(rewards) < 2:
        return mean
    return mean - robustness * statistics.pstdev(rewards)


def evaluate_candidate(
    genome: dict[str, Any],
    kind: str,
    seeds: Sequence[int],
    timeout: float,
    robustness: float,
    env: Optional[ShipSim] = None,
) -> tuple[float, Policy]:
    """Construit la politique d'un candidat et renvoie (fitness, politique)."""
    if kind == "law":
        policy = make_law_policy(clip_law(genome))
    else:
        policy = make_nn_policy(genome["net"])
    return fitness(policy, seeds, timeout, robustness, env), policy


# ── l'algorithme génétique ───────────────────────────────────────────────────

def tournament(scores: Sequence[tuple[float, Any]], k: int, rng: random.Random) -> tuple[float, Any]:
    """Sélection par tournoi : le meilleur de `k` candidats tirés au hasard."""
    contenders = rng.sample(list(scores), min(k, len(scores)))
    return max(contenders, key=lambda t: t[0])


def uniform_crossover(a: dict[str, Any], b: dict[str, Any], rng: random.Random) -> dict[str, Any]:
    """Croisement uniforme gène à gène (chaque gène vient de a ou de b, 50/50)
    - pour le génome A (dictionnaire de gènes nommés)."""
    return {k: (a[k] if rng.random() < 0.5 else b[k]) for k in a}


def mutate_law(gene: dict[str, float], sigma_ratio: float, rng: random.Random) -> dict[str, float]:
    """Mutation gaussienne bornée : σ = `sigma_ratio` × étendue du gène."""
    out = {}
    for k, v in gene.items():
        lo, hi = LAW_BOUNDS[k]
        out[k] = min(max(v + rng.gauss(0.0, (hi - lo) * sigma_ratio), lo), hi)
    return out


def mutate_nn(genome: Sequence[float], sigma: float, rate: float, rng: random.Random) -> list[float]:
    """Mutation du génome réseau : chaque poids mute avec probabilité `rate`
    d'un bruit gaussien σ (les bornes, elles, sont infinies)."""
    return [v + rng.gauss(0.0, sigma) if rng.random() < rate else v for v in genome]


def run_ga(
    kind: str,
    population: list[Any],
    train_seeds: Sequence[int],
    *,
    timeout: float,
    robustness: float,
    gens: int,
    elites: int,
    tournament_k: int,
    sigma_ratio: float,
    sigma_min_ratio: float,
    rng: random.Random,
    progress: Optional[Callable[[int, float, float], None]] = None,
    on_generation: Optional[Callable[[int, float, Any, float, float], None]] = None,
    on_candidate: Optional[Callable[[int, int, float, Any], None]] = None,
) -> tuple[Any, float]:
    """Le moteur génétique, **génome-agnostique** : la population est une liste
    de génomes (dict de gènes pour `law`, dict {"net": MLP} pour `nn`).

    Boucle : évaluer → élites (copiées telles quelles) → tournoi + croisement
    + mutation → nouvelle génération. Le σ de mutation décroît linéairement
    vers `sigma_min_ratio` (règle des 1/5 simplifiée : l'exploration se
    resserre quand la population converge).

    Renvoie (meilleur génome, sa fitness). `on_generation`, si fourni, est
    appelé après chaque évaluation de population - génération 0 comprise -
    avec (génération, meilleur score, meilleur génome, moyenne, σ) : c'est la
    prise de l'afficheur (`--view`, `viewer.py`), elle ne touche pas à la
    sélection. `on_candidate`, lui, est appelé après **chaque** candidat
    évalué avec (génération, index dans la génération, fitness, génome) :
    l'afficheur y échantillonne les épisodes des candidats ordinaires."""
    scored: list[tuple[float, Any]] = []
    env = ShipSim()
    for index, genome in enumerate(population):
        s, _ = evaluate_candidate(genome, kind, train_seeds, timeout, robustness, env)
        scored.append((s, genome))
        if on_candidate is not None:
            on_candidate(0, index, s, genome)
    best_score, best_genome = max(scored, key=lambda t: t[0])
    if on_generation is not None:
        on_generation(0, best_score, best_genome,
                      sum(s for s, _ in scored) / len(scored), sigma_ratio)

    for gen in range(1, gens + 1):
        sigma = sigma_min_ratio + (sigma_ratio - sigma_min_ratio) * (gens - gen) / max(1, gens)
        # élitisme : les meilleurs passent intacts (la sélection ne doit jamais
        # « oublier » la meilleure politique trouvée - comme l'élite de cem.py)
        ranked = sorted(scored, key=lambda t: t[0], reverse=True)
        elites_g = [g for _, g in ranked[:elites]]
        # re-scorer les élites avec le σ courant (la fitness ne change pas :
        # mêmes graines, même politique - la re-scoring sert au suivi)
        if progress is not None:
            progress(gen, ranked[0][0], sum(s for s, _ in ranked) / len(ranked))
        # nouvelle population : élites + enfants de tournoi
        children: list[Any] = list(elites_g)
        while len(children) < len(population):
            p1 = tournament(scored, tournament_k, rng)[1]
            p2 = tournament(scored, tournament_k, rng)[1]
            if kind == "law":
                child = mutate_law(uniform_crossover(p1, p2, rng), sigma, rng)
            else:
                g1, g2 = nn_flatten(p1["net"]), nn_flatten(p2["net"])
                flat = [(g1[i] if rng.random() < 0.5 else g2[i]) for i in range(len(g1))]
                flat = mutate_nn(flat, sigma * 0.3, 0.25, rng)
                net = _clone_net(p1["net"])  # clone AVANT d'écrire (jamais muter le parent)
                nn_unflatten(net, flat)
                child = {"net": net}
            children.append(child)
        # re-évaluation de la génération
        scored = []
        for index, genome in enumerate(children):
            s, _ = evaluate_candidate(genome, kind, train_seeds, timeout, robustness, env)
            scored.append((s, genome))
            if on_candidate is not None:
                on_candidate(gen, index, s, genome)
        if scored[0][0] > best_score:
            best_score = scored[0][0]
            best_genome = scored[0][1]
        if on_generation is not None:
            top = max(scored, key=lambda t: t[0])
            on_generation(gen, top[0], top[1],
                          sum(s for s, _ in scored) / len(scored), sigma)

    return best_genome, best_score


def _clone_net(net: Any) -> Any:
    """Copie profonde légère d'un MLP (poids clonés)."""
    clone = type(net)(net.inputs, net.hidden, net.outputs)
    clone.w1 = [row[:] for row in net.w1]
    clone.b1 = list(net.b1)
    clone.w2 = [row[:] for row in net.w2]
    clone.b2 = list(net.b2)
    return clone


# ── sérialisation ────────────────────────────────────────────────────────────

def save_law_policy(path: str, gene: dict[str, float], score: float, seeds: Sequence[int]) -> None:
    """Écrit `ga_policy.json` : gènes + récompense + graines d'entraînement."""
    payload = {
        "policy": "ga-law",
        "task": "ship-mining",
        "params": {k: round(v, 4) for k, v in gene.items()},
        "reward": round(score, 2),
        "train_seeds": list(seeds),
    }
    with open(path, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")


def save_nn_ga(path: str, net: Any, score: float, seeds: Sequence[int]) -> None:
    """Écrit la politique réseau gagnante (format `save_nn` : chargeable par
    `policies.nn_policy`, et déployable en jeu comme l'amorce d'imitation)."""
    from nn import save_nn

    net_meta = {"source": "ga", "reward": round(score, 2), "train_seeds": list(seeds)}
    save_nn(path, net, meta=net_meta)
    # save_nn écrit déjà toutes les clés attendues par load_nn ; rien d'autre
    # à faire - les constantes importées ci-dessus documentent le contrat.


# ── afficheur (`--view`) ─────────────────────────────────────────────────────────

def build_population(args: argparse.Namespace, rng: random.Random) -> list[Any]:
    """Population initiale : pour la loi, le réglage du jeu + tirages uniformes
    dans les bornes (l'espace est inconnu, la génération 0 explore tout) ; pour
    le réseau, l'amorce + perturbations gaussiennes de ses poids."""
    if args.genome == "law":
        population: list[Any] = [dict(LAW_DEFAULTS)]
        for _ in range(args.pop - 1):
            population.append(clip_law({k: rng.uniform(*LAW_BOUNDS[k]) for k in LAW_BOUNDS}))
        return population
    from nn import MLP, OUTPUT_COUNT, load_nn, feature_size

    if args.warmstart:
        base = load_nn(args.warmstart)
    else:
        base = MLP(feature_size(), args.hidden, OUTPUT_COUNT, rng)
        print("⚠ génome nn sans amorce : réseau initialisé au hasard - la "
              "recherche part de zéro et peut ne pas démarrer (voir "
              "ship_warmstart.py / imitate.py)")
    weights = nn_flatten(base)
    scale = max(abs(v) for v in weights) if weights else 1.0
    population = []
    for i in range(args.pop):
        net = _clone_net(base)
        if i == 0:
            perturbed = list(weights)
        else:
            perturbed = [v + rng.gauss(0.0, scale * args.sigma * 0.3) for v in weights]
        nn_unflatten(net, perturbed)
        population.append({"net": net})
    return population


def finalize(args: argparse.Namespace, best_genome: Any,
             best_score: float) -> tuple[str, float]:
    """Mesure de validation (graines disjointes - jamais vues par la sélection)
    puis écriture de la politique gagnante ; renvoie (chemin, récompense)."""
    best_policy = make_law_policy(clip_law(best_genome)) if args.genome == "law" \
        else make_nn_policy(best_genome["net"])
    val = fitness(best_policy, args.val_seeds, args.timeout, 0.0)
    print(f"\nMeilleure politique : fitness entraînement {best_score:.1f} · "
          f"récompense validation {val:.1f}")

    out = args.output or ("ga_policy.json" if args.genome == "law" else "nn_policy_ga.json")
    if args.genome == "law":
        save_law_policy(out, clip_law(best_genome), best_score, args.seeds)
    else:
        save_nn_ga(out, best_genome["net"], best_score, args.seeds)
    print(f"Écrite dans {out} - rejouable par "
          f"`python3 evaluate.py --strategy ga --policy {out}` "
          f"(génome law : comparer à l'autopilote porté avec "
          f"`python3 ship_warmstart.py --measure-sim`)")
    return out, val


def _view_training(args: argparse.Namespace, population: list[Any], rng: random.Random,
                   report: Callable[[int, float, float], None]) -> None:
    """Entraîne dans un **fil d'arrière-plan** et laisse le fil principal à
    l'afficheur pixels (tkinter l'exige) : chaque génération pousse ses
    statistiques et le rejeu de l'épisode de son meilleur candidat (première
    graine d'entraînement - déterministe, c'est exactement l'épisode de la
    fitness), des candidats ordinaires échantillonnés passent entre les
    générations (`--view-sample`, rejeu rapide en lecture de moindre
    priorité), et la fin pousse l'épisode de validation (première graine
    disjointe). L'écran n'attend jamais l'entraînement, l'entraînement
    n'attend jamais l'écran."""
    import threading

    import viewer as aff  # import local : l'afficheur reste indépendant

    session = aff.TrainingSession(gens=args.gens)

    def replay_policy(genome: Any) -> Policy:
        if args.genome == "law":
            return make_law_policy(clip_law(genome))
        return make_nn_policy(genome["net"])

    def on_generation(gen: int, best: float, genome: Any, mean: float,
                      sigma: float) -> None:
        frames, info = aff.record_episode(replay_policy(genome),
                                          args.seeds[0], args.timeout)
        session.push_generation(gen, best, mean, sigma, frames, info)

    def on_candidate(gen: int, index: int, score: float, genome: Any) -> None:
        # épisodes des candidats ordinaires, échantillonnés : rejeu rapide
        # (échantillonnage plus lâche) sur une des graines évaluées, en
        # lecture de moindre priorité - une génération qui arrive passe
        # toujours devant
        if args.view_sample <= 0 or (index + 1) % args.view_sample != 0:
            return
        seed = args.seeds[index % len(args.seeds)]
        frames, info = aff.record_episode(replay_policy(genome), seed,
                                          args.timeout, stride=6)
        session.push_candidate(gen, index, score, frames, info)

    def report_view(gen: int, best: float, mean: float) -> None:
        report(gen, best, mean)
        session.push_status(f"génération {gen} : évaluation de {args.pop} candidats…")

    def work() -> None:
        try:
            best_genome, best_score = run_ga(
                args.genome, population, args.seeds,
                timeout=args.timeout, robustness=args.robustness,
                gens=args.gens, elites=args.elites, tournament_k=args.tournament,
                sigma_ratio=args.sigma, sigma_min_ratio=args.sigma_min, rng=rng,
                progress=report_view, on_generation=on_generation,
                on_candidate=on_candidate,
            )
            out, val = finalize(args, best_genome, best_score)
            frames, info = aff.record_episode(replay_policy(best_genome),
                                              args.val_seeds[0], args.timeout)
            session.push_result(f"terminé · validation {val:.1f} · politique : {out}",
                                frames, info)
        except BaseException as exc:  # l'afficheur doit le montrer, pas le taire
            import traceback
            traceback.print_exc()
            session.push_error(f"{type(exc).__name__}: {exc}")
        finally:
            session.close()

    threading.Thread(target=work, daemon=True, name="ga-entrainement").start()
    session.run()


# ── point d'entrée ───────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--genome", choices=("law", "nn"), default="law",
                    help="génome : `law` = les constantes de la loi portée "
                         "(rapide, viable dès la génération 0) ; `nn` = les "
                         "poids du MLP (amorce par réseau d'imitation requis)")
    ap.add_argument("--backend", choices=("sim", "live"), default="sim",
                    help="simulateur (défaut, instantané) ou vraie partie "
                         "headless (lent, un aller-retour HTTP par pas)")
    ap.add_argument("--host", default="http://127.0.0.1:8643/")
    ap.add_argument("--gens", type=int, default=15, help="générations")
    ap.add_argument("--pop", type=int, default=24, help="candidats par génération")
    ap.add_argument("--elites", type=int, default=4, help="meilleurs conservés intacts")
    ap.add_argument("--tournament", type=int, default=3, help="taille du tournoi")
    ap.add_argument("--sigma", type=float, default=0.25,
                    help="dispersion de mutation initiale (fraction de l'étendue)")
    ap.add_argument("--sigma-min", type=float, default=0.04,
                    help="dispersion de mutation finale (convergence)")
    ap.add_argument("--seeds", type=int, nargs="+", default=[1, 2, 3],
                    help="graines d'entraînement (déterministes)")
    ap.add_argument("--val-seeds", type=int, nargs="+", default=[11, 12],
                    help="graines de validation (disjointes de l'entraînement)")
    ap.add_argument("--timeout", type=float, default=90.0, help="délai d'un épisode (s)")
    ap.add_argument("--robustness", type=float, default=0.5,
                    help="poids de la pénalité d'écart-type de la fitness")
    ap.add_argument("--warmstart", default=None, metavar="PATH",
                    help="génome nn : politique d'imitation de départ "
                         "(ship_warmstart_policy.json / nn_policy.json)")
    ap.add_argument("--hidden", type=int, default=64, help="neurones cachés (génome nn, sans amorce)")
    ap.add_argument("--output", default=None, metavar="PATH",
                    help="politique gagnante (défaut ga_policy.json ou nn_policy_ga.json)")
    ap.add_argument("--rng-seed", type=int, default=0, help="graine du GA (reproductible)")
    ap.add_argument("--view", action="store_true",
                    help="afficheur pixels (`viewer.py`) : à chaque génération, "
                         "courbes de fitness + rejeu de l'épisode du meilleur "
                         "candidat ; à la fin, l'épisode de validation")
    ap.add_argument("--view-sample", type=int, default=4, metavar="N",
                    help="avec --view : montre aussi 1 épisode de candidat "
                         "ordinaire sur N (0 = aucun)")
    args = ap.parse_args()

    rng = random.Random(args.rng_seed * 7919 + 13)
    population = build_population(args, rng)

    def report(gen: int, best: float, mean: float) -> None:
        print(f"gén {gen:>3}   meilleur {best:>9.1f}   moyenne {mean:>9.1f}")

    print(f"GA sélectif - génome {args.genome} · backend {args.backend}")
    print(f"{args.gens} générations × {args.pop} candidats × {len(args.seeds)} graines "
          f"(validation : {args.val_seeds})")

    if args.backend == "live":
        print("✗ backend live non implémenté dans cette version : le pas-à-pas "
              "HTTP coûte ~1000× le simulateur (voir cem.py pour la mécanique).")
        sys.exit(2)

    if args.view:
        _view_training(args, population, rng, report)
        return

    best_genome, best_score = run_ga(
        args.genome, population, args.seeds,
        timeout=args.timeout, robustness=args.robustness,
        gens=args.gens, elites=args.elites, tournament_k=args.tournament,
        sigma_ratio=args.sigma, sigma_min_ratio=args.sigma_min, rng=rng,
        progress=report,
    )
    finalize(args, best_genome, best_score)


if __name__ == "__main__":
    main()
