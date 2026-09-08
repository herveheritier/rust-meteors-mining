#!/usr/bin/env python3
"""Petit réseau de neurones en **Python standard uniquement** (aucune
dépendance - la convention de `tools/trainer/`) pour l'apprentissage par
**imitation de l'autopilote** (`imitate.py`).

- `obs_features(obs)` : transforme une observation JSON (`/obs`, ou celle des
  trajectoires du banc d'essai) en un **vecteur de features à taille fixe** -
  le réseau n'apprend que sur ce vecteur, jamais sur le JSON brut ;
- `MLP` : perceptron multicouche à une couche cachée (tanh) et sorties
  sigmoïdes (une par action : up/down/left/right/fire), entraîné en descente
  de gradient par lots avec **entropie croisée binaire** (le pilote a plusieurs
  boutons en même temps : c'est une classification multi-étiquettes) ;
- `save_nn` / `load_nn` : poids sérialisés en JSON (comme `policy.json`),
  rejouables par `evaluate.py --strategy nn --policy nn_policy.json`.
"""

from __future__ import annotations

import json
import math
import random
from typing import Any, Optional

# ── extraction de features ──────────────────────────────────────────────────

#: Version du format de features : si elle change (nouveaux champs, autre
#: normalisation), les politiques entraînées avec l'ancienne version ne sont
#: plus rejouables - le fichier de politique porte cette version.
FEATURES_VERSION = 2

#: Actions (boutons) prédits par le réseau - l'ordre définit l'index de sortie.
ACTIONS = ("up", "down", "left", "right", "fire")

#: Types d'objets proches encodés en one-hot (même libellé que l'observation).
NEARBY_KINDS = ("meteore", "minerai", "alien", "portail", "mine")

#: Nombre d'objets proches pris en compte (les plus proches) : le reste de la
#: liste est ignoré - couvre l'horizon utile de l'autopilote (tir, minage).
NEARBY_SLOTS = 6

#: Échelles de normalisation (le monde fait 3960×3540, vitesses ~centaines
#: d'unités/s, vie de forme ~quelques dizaines).
SCALE_DIST = 2000.0
SCALE_SPEED = 300.0
SCALE_RADIUS = 100.0
SCALE_LIFE = 50.0
SCALE_CREDITS = 5000.0


def _clamp(x: float, lo: float = -1.0, hi: float = 1.0) -> float:
    return min(max(x, lo), hi)


def _wrap_angle(a: float) -> float:
    """Ramène un angle dans ]−π, π] (comme le jeu)."""
    return (a + math.pi) % math.tau - math.pi


def _angle_feature(a: float) -> float:
    """Angle normalisé dans ]−1, 1] (invariant par tour complet)."""
    return _clamp((_wrap_angle(a) / math.pi))


def _pilot_features(body: dict[str, Any], station_dx: float, station_dy: float) -> list[float]:
    """Features d'un pilote (vaisseau ou EVA) : cinématique brute **plus** les
    grandeurs que l'autopilote calcule pour décider (viser la station) :

    - `aim`    : angle écran vers la station (l'orientation doit s'y aligner) ;
    - `err`    : erreur d'alignement `aim − orientation` (signe → tourner
      gauche/droite, |err| → seuil de poussée) ;
    - `v_along`: vitesse projetée sur la direction de la station (positive =
      on s'en rapproche - décide pousser ou contre-pousser) ;
    - `v_tang` : vitesse perpendiculaire (l'orbite autour de la base - la
      contre-poussée qui la tue).

    Fournir ces grandeurs dérivées rend l'imitation robuste en boucle fermée :
    le réseau n'a pas à ré-apprendre l'`atan2` et les projections - il apprend
    la **décision** (le seuil), pas le calcul.
    """
    aim = math.atan2(station_dy, station_dx)
    orientation = body.get("orientation", 0.0)
    vx = body.get("vx", 0.0)
    vy = body.get("vy", 0.0)
    speed = body.get("speed", 0.0)
    v_along = vx * math.cos(aim) + vy * math.sin(aim)
    v_tang = math.sqrt(max(0.0, speed * speed - v_along * v_along))
    return [
        body.get("x", 0.0) / SCALE_DIST,
        body.get("y", 0.0) / SCALE_DIST,
        _clamp(vx / SCALE_SPEED),
        _clamp(vy / SCALE_SPEED),
        _clamp(speed / SCALE_SPEED),
        _angle_feature(body.get("direction", 0.0)),
        _angle_feature(orientation),
        _clamp(body.get("rotation", 0.0) / math.tau),
        # grandeurs dérivées (celles que l'autopilote utilise)
        _angle_feature(aim),
        _clamp(_wrap_angle(aim - orientation) / math.pi),
        _clamp(v_along / SCALE_SPEED),
        _clamp(v_tang / SCALE_SPEED),
    ]


def obs_features(obs: dict[str, Any]) -> list[float]:
    """Vecteur de features à taille fixe d'une observation JSON.

    Les champs manquants valent zéro (tolérant : une observation de
    micro-simulateur EVA n'a pas d'objets proches ni d'économie, un épisode
    vaisseau n'a pas d'EVA active…). L'ordre des features est stable - c'est
    lui qui définit l'entrée du réseau.
    """
    ship = obs.get("ship", {})
    eva = obs.get("eva", {})
    station_dx = obs.get("station_dx", 0.0)
    station_dy = obs.get("station_dy", 0.0)
    f: list[float] = [
        # ── contexte global ─────────────────────────────────────────────────
        1.0 if obs.get("eva_active") else 0.0,          # pilote EVA ?
        1.0 if obs.get("docked") else 0.0,              # accosté / déchargement
        1.0 if obs.get("economy") else 0.0,             # scénario à économie
        station_dx / SCALE_DIST,
        station_dy / SCALE_DIST,
        obs.get("station_dist", 0.0) / SCALE_DIST,
        # ── cinématique du vaisseau (brute + dérivée vers la station) ───────
        *_pilot_features(ship, station_dx, station_dy),
        # ── cinématique du cosmonaute EVA (brute + dérivée) ─────────────────
        *_pilot_features(eva, station_dx, station_dy),
        # ── économie (boucle de minage du vaisseau) ─────────────────────────
        obs.get("fuel", 0.0) / max(obs.get("fuel_cap", 0.0), 1.0),
        obs.get("ammo", 0) / max(obs.get("ammo_cap", 0), 1),
        obs.get("credits", 0) / SCALE_CREDITS,
        obs.get("cargo_qty", 0) / max(obs.get("cargo_cap", 0), 1),
    ]
    # ── objets proches (les plus proches, slots fixes) ──────────────────────
    nearby = obs.get("nearby", [])[: NEARBY_SLOTS]
    for o in nearby:
        kind = o.get("kind", "")
        for k in NEARBY_KINDS:
            f.append(1.0 if kind == k else 0.0)
        f.extend(
            [
                _clamp(o.get("dx", 0.0) / SCALE_DIST),
                _clamp(o.get("dy", 0.0) / SCALE_DIST),
                _clamp(o.get("dist", 0.0) / SCALE_DIST, 0.0, 1.0),
                _clamp(o.get("vx", 0.0) / SCALE_SPEED),
                _clamp(o.get("vy", 0.0) / SCALE_SPEED),
                _clamp(o.get("radius", 0.0) / SCALE_RADIUS, 0.0, 1.0),
                _clamp(o.get("life", 0.0) / SCALE_LIFE, 0.0, 1.0),
            ]
        )
    # slots restants : zéros (pas d'objet à cette distance)
    for _ in range(NEARBY_SLOTS - len(nearby)):
        f.extend([0.0] * (len(NEARBY_KINDS) + 7))
    return f


def feature_size() -> int:
    """Taille du vecteur de features (fixe, dérivée des constantes ci-dessus)."""
    return len(obs_features({}))


# ── le réseau ───────────────────────────────────────────────────────────────

class MLP:
    """Perceptron multicouche : entrée → cachée (tanh) → sorties (sigmoïdes).

    Une seule couche cachée suffit pour imiter les décisions de l'autopilote
    (missions, visée, seuils) ; l'entraînement est une descente de gradient
    par lots avec entropie croisée binaire et élan (`momentum`).
    """

    def __init__(self, inputs: int, hidden: int, outputs: int, rng: Optional[random.Random] = None) -> None:
        self.inputs = inputs
        self.hidden = hidden
        self.outputs = outputs
        rng = rng or random.Random()
        # initialisation de Xavier : variances adaptées aux tailles des couches
        def init(fan_in: int, fan_out: int) -> list[list[float]]:
            limit = math.sqrt(6.0 / (fan_in + fan_out))
            return [[rng.uniform(-limit, limit) for _ in range(fan_out)] for _ in range(fan_in)]

        self.w1 = init(inputs, hidden)   # inputs × hidden
        self.b1 = [0.0] * hidden
        self.w2 = init(hidden, outputs)  # hidden × outputs
        self.b2 = [0.0] * outputs

    # ── propagation ─────────────────────────────────────────────────────────
    def forward(self, x: list[float]) -> list[float]:
        """Sorties sigmoïdes (probabilités par action) pour une entrée."""
        h = [math.tanh(sum(x[i] * self.w1[i][j] for i in range(self.inputs)) + self.b1[j])
             for j in range(self.hidden)]
        return [1.0 / (1.0 + math.exp(-(sum(h[j] * self.w2[j][k] for j in range(self.hidden)) + self.b2[k])))
                for k in range(self.outputs)]

    def predict(self, obs: dict[str, Any]) -> list[float]:
        """Sorties pour une observation JSON (chemin court pour `policies`)."""
        return self.forward(obs_features(obs))

    # ── entraînement ────────────────────────────────────────────────────────
    def train(
        self,
        X: list[list[float]],
        Y: list[list[float]],
        epochs: int = 50,
        lr: float = 0.1,
        momentum: float = 0.9,
        batch_size: int = 64,
        patience: int = 15,
        noise: float = 0.0,
        rng: Optional[random.Random] = None,
    ) -> dict[str, float]:
        """Descente de gradient par lots (mini-lots) sur (X, Y).

        `X` : vecteurs de features, `Y` : cibles binaires (1.0/0.0) par
        action. Renvoie un résumé : perte finale, exactitude globale
        (toutes les actions justes), exactitude par action, et le nombre
        d'époques réellement effectuées (arrêt précoce si la perte ne
        diminue plus).
        """
        rng = rng or random.Random()
        n = len(X)
        # élan (momentum) : mêmes dimensions que les poids
        v_w1 = [[0.0] * self.hidden for _ in range(self.inputs)]
        v_b1 = [0.0] * self.hidden
        v_w2 = [[0.0] * self.outputs for _ in range(self.hidden)]
        v_b2 = [0.0] * self.outputs

        def bce_loss(y: float, p: float) -> float:
            p = min(max(p, 1e-9), 1.0 - 1e-9)
            return -(y * math.log(p) + (1.0 - y) * math.log(1.0 - p))

        best_loss = float("inf")
        stagnant = 0
        epochs_done = 0
        best_w1 = [row[:] for row in self.w1]
        best_b1 = list(self.b1)
        best_w2 = [row[:] for row in self.w2]
        best_b2 = list(self.b2)
        for epoch in range(epochs):
            # une époque = un passage complet en mini-lots mélangés
            idx = list(range(n))
            rng.shuffle(idx)
            epoch_loss = 0.0
            for start in range(0, n, batch_size):
                batch = idx[start : start + batch_size]
                # gradients accumulés sur le lot
                g_w1 = [[0.0] * self.hidden for _ in range(self.inputs)]
                g_b1 = [0.0] * self.hidden
                g_w2 = [[0.0] * self.outputs for _ in range(self.hidden)]
                g_b2 = [0.0] * self.outputs
                for i in batch:
                    x = X[i]
                    y = Y[i]
                    # bruit gaussien sur l'entrée (régularisation) : le réseau
                    # doit répondre juste aussi aux observations **légèrement
                    # hors distribution** - en boucle fermée, chaque erreur
                    # déplace la trajectoire et c'est exactement ce régime qui
                    # décide (la dérive de distribution du clonage)
                    if noise > 0.0:
                        x = [v + rng.gauss(0.0, noise) for v in x]
                    # ── avant ──
                    h = [math.tanh(sum(x[a] * self.w1[a][j] for a in range(self.inputs)) + self.b1[j])
                         for j in range(self.hidden)]
                    out = [1.0 / (1.0 + math.exp(-(sum(h[j] * self.w2[j][k] for j in range(self.hidden)) + self.b2[k])))
                           for k in range(self.outputs)]
                    for k in range(self.outputs):
                        epoch_loss += bce_loss(y[k], out[k])
                    # ── arrière : erreur de sortie = p − y (BCE + sigmoïde) ──
                    d_out = [out[k] - y[k] for k in range(self.outputs)]
                    # dérivée de tanh : 1 − h²
                    d_h = [0.0] * self.hidden
                    for j in range(self.hidden):
                        d_h[j] = (1.0 - h[j] * h[j]) * sum(d_out[k] * self.w2[j][k] for k in range(self.outputs))
                    for k in range(self.outputs):
                        g_b2[k] += d_out[k]
                        for j in range(self.hidden):
                            g_w2[j][k] += d_out[k] * h[j]
                    for j in range(self.hidden):
                        g_b1[j] += d_h[j]
                        for a in range(self.inputs):
                            g_w1[a][j] += d_h[j] * x[a]
                # ── mise à jour avec élan (moyenne sur le lot) ──
                scale = lr / len(batch)
                for a in range(self.inputs):
                    for j in range(self.hidden):
                        v_w1[a][j] = momentum * v_w1[a][j] + scale * g_w1[a][j]
                        self.w1[a][j] -= v_w1[a][j]
                for j in range(self.hidden):
                    v_b1[j] = momentum * v_b1[j] + scale * g_b1[j]
                    self.b1[j] -= v_b1[j]
                for j in range(self.hidden):
                    for k in range(self.outputs):
                        v_w2[j][k] = momentum * v_w2[j][k] + scale * g_w2[j][k]
                        self.w2[j][k] -= v_w2[j][k]
                for k in range(self.outputs):
                    v_b2[k] = momentum * v_b2[k] + scale * g_b2[k]
                    self.b2[k] -= v_b2[k]
            loss = epoch_loss / n
            if loss < best_loss - 1e-5:
                best_loss = loss
                stagnant = 0
                best_w1 = [row[:] for row in self.w1]
                best_b1 = list(self.b1)
                best_w2 = [row[:] for row in self.w2]
                best_b2 = list(self.b2)
            else:
                stagnant += 1
                if stagnant >= patience:
                    break  # arrêt précoce : plus de progrès
            epochs_done += 1
        # restaure les poids de la meilleure époque (l'arrêt précoce évite de
        # finir sur une époque de sur-apprentissage)
        self.w1, self.b1, self.w2, self.b2 = best_w1, best_b1, best_w2, best_b2
        summary = self.summary(X, Y)
        summary["epochs_done"] = epochs_done
        return summary

    def summary(self, X: list[list[float]], Y: list[list[float]]) -> dict[str, float]:
        """Exactitudes sur (X, Y) : globale (toutes les actions justes) et par
        action, plus la perte BCE moyenne."""
        n = len(X)
        loss = 0.0
        per_action = [0.0] * self.outputs
        exact = 0
        for x, y in zip(X, Y):
            out = self.forward(x)
            for k in range(self.outputs):
                p = min(max(out[k], 1e-9), 1.0 - 1e-9)
                loss += -(y[k] * math.log(p) + (1.0 - y[k]) * math.log(1.0 - p))
                per_action[k] += 1.0 if (out[k] >= 0.5) == (y[k] >= 0.5) else 0.0
            if all((out[k] >= 0.5) == (y[k] >= 0.5) for k in range(self.outputs)):
                exact += 1
        return {
            "loss": loss / n,
            "accuracy": exact / n,
            "per_action": [pa / n for pa in per_action],
        }


# ── sérialisation (même esprit que policy.json) ─────────────────────────────

def save_nn(path: str, net: MLP, meta: Optional[dict[str, Any]] = None) -> None:
    """Écrit les poids + métadonnées (dont la version des features) en JSON."""
    payload: dict[str, Any] = {
        "policy": "nn",
        "features_version": FEATURES_VERSION,
        "inputs": net.inputs,
        "hidden": net.hidden,
        "outputs": net.outputs,
        "actions": list(ACTIONS),
        "w1": net.w1,
        "b1": net.b1,
        "w2": net.w2,
        "b2": net.b2,
    }
    if meta:
        payload["meta"] = meta
    with open(path, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")


def load_nn(path: str) -> MLP:
    """Recharge un réseau depuis un fichier JSON (sortie de `save_nn`)."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("policy") != "nn":
        raise ValueError(f"{path} n'est pas une politique réseau de neurones")
    if data.get("features_version") != FEATURES_VERSION:
        raise ValueError(
            f"{path} : version de features {data.get('features_version')} "
            f"≠ {FEATURES_VERSION} attendue (re-entraîner avec imitate.py)"
        )
    net = MLP(data["inputs"], data["hidden"], data["outputs"])
    net.w1 = data["w1"]
    net.b1 = data["b1"]
    net.w2 = data["w2"]
    net.b2 = data["b2"]
    return net