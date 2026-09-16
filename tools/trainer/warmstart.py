#!/usr/bin/env python3
"""Amorce experte par **imitation avec perturbation des états de départ**.

C'est le correctif de l'échec documenté en Phase 3 (`docs/AUTOENTRAINEMENT.md`
§5 sexies) : la boucle fermée **gèle au premier état hors distribution** de
son amorce. Le clonage (et l'amorce PPO) imite l'expert sur **ses propres**
trajectoires, qui ne font que *traverser* certains états - typiquement l'état
« nez aligné sur la station, loin, au repos » : `seek` n'y passe qu'une frame
avant de pousser, il est sous-représenté dans les données (`p(↑) ≈ 0,35`), et
la politique n'ose ni pousser ni quitter l'état → épisode immobile (−200).

La correction : entraîner l'expert sur des états de départ **perturbés** -
position, orientation, vitesse - et **départ au repos aligné à plusieurs
distances** (l'état exact où la boucle fermée gelait), puis enregistrer tout
le déroulé de l'expert depuis ces départs. La politique apprend ainsi la
**décision de rattrapage** (pousser quand on est aligné, freiner quand on
arrive trop vite, casser une orbite) au lieu de la seule décision nominale.

L'expert est interchangeable :

- `seek` : le contrôleur paramétré de `policies.py` (hors ligne, toujours
  disponible) ;
- `autopilot` : le portage Python de l'autopilote du jeu
  (`autopilot_ref.py`) - l'expert de référence, celui que la politique doit
  dépasser.

Le rééquilibrage des classes est **adaptatif** (`balance`) : quand l'expert
passe l'essentiel de son temps à ne rien faire (clonage sur trajectoires
nominales), les pas rares sont dupliqués - sans quoi l'entropie croisée
converge vers une politique immobile ; quand l'amorce perturbée produit au
contraire surtout de l'action, aucune sur-pondération n'est appliquée.
"""

from __future__ import annotations

import math
import random
from typing import Any, Callable, Sequence

from eva_env import EvaSim, spawn_position, wrapped_dxdy
from nn import obs_features
from policies import EVA_ACTIONS

#: Un état de départ d'épisode : `(seed, x, y, orientation, vx, vy)` avec la
#: vitesse en unités/s (mêmes axes que l'observation `eva.vx/vy`).
Start = tuple[int, float, float, float, float, float]

#: Un pas d'imitation : `(features de l'observation, indice d'action)`.
Transition = tuple[list[float], int]


def action_index(cmd: dict[str, bool]) -> int:
    """Indice de `EVA_ACTIONS` correspondant à une commande `up/left/right`
    (lève une erreur sur une combinaison hors espace discret, ex. gauche et
    droite ensemble - l'expert n'en produit jamais)."""
    up = bool(cmd.get("up"))
    if cmd.get("left") and cmd.get("right"):
        raise ValueError(f"rotation gauche ET droite simultanées : {cmd}")
    if cmd.get("left"):
        turn = 0
    elif cmd.get("right"):
        turn = 1
    else:
        turn = 2
    for i, act in enumerate(EVA_ACTIONS):
        a_up = 1 if act.get("up") else 0
        a_turn = 0 if act.get("left") else (1 if act.get("right") else 2)
        if a_up == (1 if up else 0) and a_turn == turn:
            return i
    raise AssertionError(f"commande hors espace discret : {cmd}")


def make_expert(kind: str) -> Callable[[dict[str, Any]], dict[str, bool]]:
    """Expert étiqueteur : `seek` (contrôleur paramétré hors ligne) ou
    `autopilot` (portage de l'autopilote du jeu). Les deux renvoient une
    commande `up/left/right`."""
    if kind == "seek":
        from policies import seek

        return seek
    if kind == "autopilot":
        from autopilot_ref import autopilot_ref_policy

        return autopilot_ref_policy()
    raise ValueError(f"expert inconnu : {kind} (attendu : seek, autopilot)")


def perturbed_starts(
    seeds: Sequence[int],
    dists: Sequence[float],
    rng: random.Random,
    *,
    dist_jitter: float = 0.0,
    angle_jitter: float = 0.0,
    speed_jitter: float = 0.0,
    rest_aligned: int = 0,
    radial_speeds: Sequence[float] = (),
    tangential_speeds: Sequence[float] = (),
) -> list[Start]:
    """États de départ, tirés des graines et distances.

    Chaque `(graine, distance)` produit :

    - le départ **nominal du jeu** (orientation 0, au repos) ;
    - `rest_aligned` départs **au repos, nez aligné** sur la station (l'état
      précis où la boucle fermée du clonage pur gelait) ;
    - un départ **tiré** : distance jittée ±`dist_jitter`, orientation décalée
      de ±`angle_jitter` rad autour de la visée, vitesse initiale tirée en
      direction et en norme (±`speed_jitter` unités/s) ;
    - les approches **radiales** (`radial_speeds`, unités/s le long de la
      visée : positive = **vers** la station, l'approche trop rapide que
      l'expert freine) ;
    - les vitesses **tangentielles** (`tangential_speeds`, unités/s
      perpendiculaires à la visée : l'orbite que l'expert casse).

    Toutes les perturbations sont nulles par défaut : la fonction dégénère
    alors en départs nominaux (l'ancienne amorce, sur les seules trajectoires
    de l'expert).
    """
    starts: list[Start] = []
    for seed in seeds:
        for dist in dists:
            x, y = spawn_position(seed, dist)
            # départ nominal du jeu : orientation 0 (est), au repos
            starts.append((seed, x, y, 0.0, 0.0, 0.0))
            # visée de la station depuis cette position (monde torique)
            aim = _aim_from_position(x, y)
            for _ in range(max(0, rest_aligned)):
                starts.append((seed, x, y, aim, 0.0, 0.0))
            # départ tiré (hors distribution) - seulement si un jitter existe,
            # pour que la perturbation nulle reproduise l'ancien jeu de données
            if dist_jitter > 0.0 or angle_jitter > 0.0 or speed_jitter > 0.0:
                xj, yj = spawn_position(
                    seed, max(15.0, dist + rng.uniform(-dist_jitter, dist_jitter)))
                aim_j = _aim_from_position(xj, yj)
                orientation = aim_j + rng.uniform(-angle_jitter, angle_jitter)
                speed = rng.uniform(0.0, speed_jitter)
                v_angle = rng.uniform(0.0, math.tau)
                starts.append((seed, xj, yj, orientation,
                               math.cos(v_angle) * speed, math.sin(v_angle) * speed))
            # approches radiales (freinage) et tangentielles (orbites)
            for v in radial_speeds:
                starts.append((seed, x, y, aim,
                               math.cos(aim) * v, math.sin(aim) * v))
            for v in tangential_speeds:
                starts.append((seed, x, y, aim,
                               -math.sin(aim) * v, math.cos(aim) * v))
    return starts


def _aim_from_position(x: float, y: float) -> float:
    """Angle écran de la station (centre 0,0 du simulateur) depuis `(x, y)` -
    la visée que le cosmonaute doit prendre pour pousser vers la station."""
    dx, dy = wrapped_dxdy(x, y, 0.0, 0.0)
    return math.atan2(dy, dx)


def collect_expert(
    starts: Sequence[Start],
    expert: Callable[[dict[str, Any]], dict[str, bool]],
    timeout: float,
    stride: int = 1,
) -> list[Transition]:
    """Déroule l'expert depuis chaque état de départ et étiquette chaque pas.

    Renvoie `(features(obs), action experte)` pour tout l'épisode, arrêté à la
    récupération ou au délai. `stride` sous-échantillonne les pas (les pas
    voisins sont quasi identiques) - le premier pas est toujours conservé.
    """
    env = EvaSim()
    transitions: list[Transition] = []
    for seed, x, y, orientation, vx, vy in starts:
        if hasattr(expert, "reset"):
            expert.reset()  # type: ignore[attr-defined]
        obs = env.reset(seed, x, y, orientation, vx, vy)
        step = 0
        while not env.done and env.t < timeout:
            if step % stride == 0:
                transitions.append((obs_features(obs), action_index(expert(obs))))
            step += 1
            cmd = expert(obs)
            obs = env.step(cmd["up"], cmd["right"], cmd["left"])
    return transitions


def is_rare(t: Transition) -> bool:
    """Pas « rare » : une action (pousser ou tourner) - la majorité des pas
    d'un expert est « ne rien faire »."""
    act = EVA_ACTIONS[t[1]]
    return bool(act.get("up")) or bool(act.get("left")) or bool(act.get("right"))


def balance(transitions: Sequence[Transition], rare_repeat: int = 4) -> list[Transition]:
    """Rééquilibre les classes en dupliquant les pas rares.

    Sans cela, quand l'expert passe l'essentiel de son temps à ne rien faire
    (le cas d'un clonage sur trajectoires nominales : ~80 % d'inactivité),
    l'entropie croisée se concentre sur la classe majoritaire et l'imitation
    converge vers une politique immobile. La duplication est **adaptative** :
    elle vise ~50/50 sans jamais dépasser `rare_repeat`, et ne fait rien si
    les pas rares sont déjà majoritaires (l'amorce perturbée, elle, produit
    surtout de l'action - la sur-pondérer serait nuisible). `rare_repeat = 1`
    désactive le rééquilibrage.
    """
    rare = [t for t in transitions if is_rare(t)]
    common = [t for t in transitions if not is_rare(t)]
    if not rare:
        return list(common)
    repeat = max(1, min(rare_repeat, round(len(common) / len(rare))))
    return list(common) + rare * repeat


def expert_transitions(
    expert_kind: str,
    seeds: Sequence[int],
    dists: Sequence[float],
    rng: random.Random,
    timeout: float,
    stride: int = 1,
    rare_repeat: int = 4,
    **perturb: Any,
) -> list[Transition]:
    """Amorce complète : génère les départs perturbés, déroule l'expert et
    rééquilibre les classes. Point d'entrée utilisé par `ppo.py`."""
    starts = perturbed_starts(seeds, dists, rng, **perturb)
    expert = make_expert(expert_kind)
    transitions = collect_expert(starts, expert, timeout, stride)
    return balance(transitions, rare_repeat)


def default_perturbation(spawn_dist: float, scale: float = 1.0) -> dict[str, Any]:
    """Perturbation par défaut autour d'une distance de départ : des départs
    au repos alignés (l'état qui gelait), une couronne de distances voisines,
    des orientations désalignées, des vitesses d'approche trop rapides et des
    orbites - le régime « hors distribution » que la boucle fermée visite.

    `scale = 0` renvoie la perturbation nulle (départs nominaux seulement) -
    l'ancien comportement de l'amorce, pour mesurer l'apport."""
    if scale <= 0.0:
        return {
            "dist_jitter": 0.0,
            "angle_jitter": 0.0,
            "speed_jitter": 0.0,
            "rest_aligned": 0,
            "radial_speeds": (),
            "tangential_speeds": (),
        }
    return {
        "dist_jitter": spawn_dist * 0.5 * scale,
        "angle_jitter": math.radians(50.0) * scale,
        "speed_jitter": 40.0 * scale,
        "rest_aligned": 2,
        # régimes physiques (unités/s) : approche trop rapide à freiner et
        # orbite à casser - non mis à l'échelle (ce sont des situations, pas
        # des amplitudes de bruit)
        "radial_speeds": (60.0, 90.0),
        "tangential_speeds": (40.0, 70.0),
    }
