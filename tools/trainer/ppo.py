#!/usr/bin/env python3
"""Apprentissage par renforcement **PPO** (Proximal Policy Optimization) sur
l'observation complète - la tâche « le cosmonaute EVA rentre à la station »,
dans le micro-simulateur (`eva_env.py`, même physique que le jeu, instantané).

Pourquoi PPO et pas DQN ? La tâche a un **horizon long** (~1100 pas pour
rentrer de 300 unités) : avec γ = 0,99, le +1000 de la récupération
n'atteint jamais les états de départ (0,99¹¹⁰⁰ ≈ 0) et le Q-learning
bootstrapé diverge (essayé, écart-type des hyperparamètres). PPO est
**on-policy** : chaque rollout propage le retour réel jusqu'au départ,
l'amorce experte garantit une politique déjà réussie que le clip protège des
mises à jour destructrices, et le réseau de valeur fournit la ligne de base
sans bootstrap max.

Le réseau (`PolicyNet`) partage une couche cachée tanh entre trois têtes,
**factorisées comme les décisions de l'autopilote** (c'est la structure
éprouvée de `nn.py` - une softmax conjointe sur les 6 combinaisons plafonne
à ~95 % d'exactitude et dérive en boucle fermée) :

- **poussée** : sigmoïde (↑ ou rien) ;
- **rotation** : softmax mutuellement exclusive {←, →, rien} ;
- **valeur** : scalaire (la ligne de base des avantages).

La politique conjointe est le produit `π(↑)·π(←/→/rien)`, et l'entraînement :

1. **amorce experte** : imitation supervisée (entropie croisée sur les deux
   têtes) du contrôleur `seek` (le réglage robuste de l'entraînement CEM,
   qui réussit la tâche) - la politique démarre déjà gagnante ;
2. **itérations PPO** : rollouts de la politique courante (échantillonnage
   softmax), retours actualisés (γ = 0,9995), avantages `G − V` normalisés
   par lot, puis quelques époques de l'objectif **clipé**
   `min(ρ·A, clip(ρ, 1−ε, 1+ε)·A)` (le rapport des probabilités est borné -
   une mise à jour ne peut pas dégrader la politique d'un coup) + régression
   de la valeur + petit bonus d'entropie (l'exploration) ;
3. **mesure** : épisodes déterministes (action la plus probable) sur des
   graines hors entraînement, récompense `episode_reward` (mêmes règles que
   le jeu).

La politique apprise est écrite dans `ppo_policy.json` et rejouée - dans le
simulateur comme dans la vraie partie - par
`evaluate.py --strategy ppo --policy ppo_policy.json`.

Référence à battre (simulateur, graines 11..15) : `seek` ≈ **893** (5/5,
entrée ~48 u/s - la pénalité d'arrivée trop rapide lui coûte ~90 points) ;
un freinage appris plus franc (entrée ≤ 30 u/s) vaut ~985. L'autopilote du
jeu (≈ 944 en conditions réelles) est la barre suivante.

    python3 ppo.py                        # budget par défaut (~100 itérations)
    python3 ppo.py --iters 200            # plus long, meilleure convergence
"""

from __future__ import annotations

import argparse
import json
import math
import random
from typing import Any, Optional

from eva_env import EvaSim, episode_reward, spawn_position
from nn import FEATURES_VERSION, feature_size, obs_features
from policies import EVA_ACTIONS

# ── récompense (mêmes règles que le jeu, `eva_env.episode_reward`) ──────────
TIME_COST_PER_SECOND = 2.0     # −2 s⁻¹ (coût de temps de l'épisode)
RECOVERY_REWARD = 1000.0       # +1000 à la récupération
ENTRY_SPEED_LIMIT = 30.0       # entrée trop rapide dans le cercle d'accostage
ENTRY_OVERSHOOT_PENALTY = 5.0  # pénalité par u/s au-dessus de la limite

#: Prime d'apprentissage : fermer la distance vers la station paie par
#: unité. Sans elle, quand les rollouts échouent (fréquent au début), le
#: « moins mauvais échec » est l'immobilité (−200) et PPO y converge au lieu
#: d'apprendre à s'approcher - la prime donne un gradient vers la station
#: même dans les épisodes perdus. La mesure finale reste `episode_reward`
#: (sans prime, mêmes règles que le jeu).
PROGRESS_GAIN = 0.1

GAMMA = 0.9995                 # horizon long : le +1000 atteint le départ
ADVANTAGE_CLIP = 2.0           # bornage des avantages normalisés (PPO)

#: Classes de rotation (l'ordre définit les indices de la tête softmax).
TURN_CLASSES = ("left", "right", "none")


def action_parts(a: int) -> tuple[int, int]:
    """Décompose une action discrète `EVA_ACTIONS[a]` en `(up, turn)` où
    `up` ∈ {0, 1} et `turn` est l'indice dans `TURN_CLASSES` - la politique
    factorisée décide des deux indépendamment."""
    cmd = EVA_ACTIONS[a]
    up = 1 if cmd.get("up") else 0
    if cmd.get("left"):
        turn = 0
    elif cmd.get("right"):
        turn = 1
    else:
        turn = 2
    return up, turn


class PolicyNet:
    """MLP entrée → cachée (tanh) → trois têtes : poussée (sigmoïde),
    rotation (softmax {←, →, rien}) et valeur (scalaire). La politique est le
    produit des deux têtes - même factorisation que l'autopilote et que le
    MLP d'imitation (`nn.py`), qui atteint ~98 % là où une softmax conjointe
    plafonne à ~95 % et dérive en boucle fermée."""

    def __init__(self, inputs: int, hidden: int, rng: Optional[random.Random] = None) -> None:
        self.inputs = inputs
        self.hidden = hidden
        self.turn = len(TURN_CLASSES)
        rng = rng or random.Random()

        def init(fan_in: int, fan_out: int) -> list[list[float]]:
            limit = math.sqrt(6.0 / (fan_in + fan_out))
            return [[rng.uniform(-limit, limit) for _ in range(fan_out)] for _ in range(fan_in)]

        self.w1 = init(inputs, hidden)
        self.b1 = [0.0] * hidden
        self.wu = init(hidden, 1)          # tête poussée (sigmoïde)
        self.bu = [0.0] * 1
        self.wt = init(hidden, self.turn)  # tête rotation (softmax)
        self.bt = [0.0] * self.turn
        self.wv = init(hidden, 1)          # tête valeur
        self.bv = [0.0] * 1

    def hidden_of(self, x: list[float]) -> list[float]:
        return [math.tanh(sum(x[i] * self.w1[i][j] for i in range(self.inputs)) + self.b1[j])
                for j in range(self.hidden)]

    def forward(self, x: list[float]) -> tuple[float, list[float], float]:
        """`(p_up, p_turn, valeur)` pour une entrée - les probabilités des
        deux têtes factorisées."""
        h = self.hidden_of(x)
        zu = sum(h[j] * self.wu[j][0] for j in range(self.hidden)) + self.bu[0]
        p_up = 1.0 / (1.0 + math.exp(-zu))
        zt = [sum(h[j] * self.wt[j][k] for j in range(self.hidden)) + self.bt[k]
              for k in range(self.turn)]
        m = max(zt)
        exp = [math.exp(zt[k] - m) for k in range(self.turn)]
        s = sum(exp)
        p_turn = [e / s for e in exp]
        v = sum(h[j] * self.wv[j][0] for j in range(self.hidden)) + self.bv[0]
        return p_up, p_turn, v

    def sample(self, p_up: float, p_turn: list[float], rng: random.Random) -> tuple[int, float]:
        """Action tirée selon la politique (produit des deux têtes) +
        log-probabilité du tirage."""
        up = 1 if rng.random() < p_up else 0
        r = rng.random()
        acc = 0.0
        turn = self.turn - 1
        for k, p in enumerate(p_turn):
            acc += p
            if r <= acc:
                turn = k
                break
        # clamp : la sigmoïde peut saturer à 0/1 après saturation des logits
        logp = math.log(max(p_up if up else 1.0 - p_up, 1e-12)) + math.log(max(p_turn[turn], 1e-12))
        # retrouve l'indice dans EVA_ACTIONS pour la paire (up, turn)
        for a, cmd in enumerate(EVA_ACTIONS):
            if (1 if cmd.get("up") else 0) == up and action_parts(a)[1] == turn:
                return a, logp
        raise AssertionError("paire (up, turn) sans action discrète")  # couvre les 6 combinaisons

    def greedy(self, x: list[float]) -> int:
        """Action déterministe (évaluation) : la plus probable du produit des
        deux têtes (le maximum conjoint = maxima séparés, les têtes étant
        indépendantes)."""
        p_up, p_turn, _ = self.forward(x)
        up = 1 if p_up >= 0.5 else 0
        turn = max(range(self.turn), key=lambda k: p_turn[k])
        for a, cmd in enumerate(EVA_ACTIONS):
            if (1 if cmd.get("up") else 0) == up and action_parts(a)[1] == turn:
                return a
        raise AssertionError("paire (up, turn) sans action discrète")


# ── récompense de pas (règles du jeu réparties par frame) ───────────────────

def step_reward(prev_dist: float, dist: float, done: bool, entry_speed: float) -> float:
    r = -TIME_COST_PER_SECOND / 60.0 + PROGRESS_GAIN * (prev_dist - dist)
    if done:
        r += RECOVERY_REWARD - max(0.0, entry_speed - ENTRY_SPEED_LIMIT) * ENTRY_OVERSHOOT_PENALTY
    return r


def discounted_returns(rewards: list[float], gamma: float) -> list[float]:
    """Retours actualisés `G_t = Σ_k γ^k r_{t+k}` (calculés en remontant)."""
    rets = [0.0] * len(rewards)
    acc = 0.0
    for t in range(len(rewards) - 1, -1, -1):
        acc = rewards[t] + gamma * acc
        rets[t] = acc
    return rets


# ── amorce experte : imiter le contrôleur `seek` ────────────────────────────

def imitate_expert(net: PolicyNet, transitions: list[tuple[list[float], int]],
                   epochs: int, lr: float, momentum: float,
                   rng: random.Random) -> None:
    """Entropie croisée supervisée sur les actions de l'expert (têtes
    poussée + rotation, indépendantes). La politique démarre déjà gagnante -
    PPO n'aura qu'à affiner (freinage, vitesse d'entrée)."""
    v_w1 = [[0.0] * net.hidden for _ in range(net.inputs)]
    v_b1 = [0.0] * net.hidden
    v_wu = [[0.0] * 1 for _ in range(net.hidden)]
    v_bu = [0.0] * 1
    v_wt = [[0.0] * net.turn for _ in range(net.hidden)]
    v_bt = [0.0] * net.turn
    for epoch in range(epochs):
        idx = list(range(len(transitions)))
        rng.shuffle(idx)
        for start in range(0, len(idx), 64):
            batch = idx[start:start + 64]
            g_w1 = [[0.0] * net.hidden for _ in range(net.inputs)]
            g_b1 = [0.0] * net.hidden
            g_wu = [[0.0] * 1 for _ in range(net.hidden)]
            g_bu = [0.0] * 1
            g_wt = [[0.0] * net.turn for _ in range(net.hidden)]
            g_bt = [0.0] * net.turn
            for i in batch:
                x, a = transitions[i]
                up, turn = action_parts(a)
                h = net.hidden_of(x)
                # poussée (BCE) : d = σ(z) − cible
                zu = sum(h[j] * net.wu[j][0] for j in range(net.hidden)) + net.bu[0]
                p_up = 1.0 / (1.0 + math.exp(-zu))
                d_up = p_up - up
                g_bu[0] += d_up
                for j in range(net.hidden):
                    g_wu[j][0] += d_up * h[j]
                # rotation (softmax CE) : d = p − 1_{turn}
                zt = [sum(h[j] * net.wt[j][k] for j in range(net.hidden)) + net.bt[k]
                      for k in range(net.turn)]
                m = max(zt)
                exp = [math.exp(zt[k] - m) for k in range(net.turn)]
                s = sum(exp)
                p_turn = [e / s for e in exp]
                for k in range(net.turn):
                    d = p_turn[k] - (1.0 if k == turn else 0.0)
                    g_bt[k] += d
                    for j in range(net.hidden):
                        g_wt[j][k] += d * h[j]
                # arrière vers la couche cachée (les deux têtes)
                for j in range(net.hidden):
                    d_h = (1.0 - h[j] * h[j]) * (
                        d_up * net.wu[j][0]
                        + sum((p_turn[k] - (1.0 if k == turn else 0.0)) * net.wt[j][k]
                              for k in range(net.turn))
                    )
                    g_b1[j] += d_h
                    for i in range(net.inputs):
                        g_w1[i][j] += d_h * x[i]
            scale = lr / len(batch)
            for i in range(net.inputs):
                for j in range(net.hidden):
                    v_w1[i][j] = momentum * v_w1[i][j] + scale * g_w1[i][j]
                    net.w1[i][j] -= v_w1[i][j]
            for j in range(net.hidden):
                v_b1[j] = momentum * v_b1[j] + scale * g_b1[j]
                net.b1[j] -= v_b1[j]
            for j in range(net.hidden):
                v_wu[j][0] = momentum * v_wu[j][0] + scale * g_wu[j][0]
                net.wu[j][0] -= v_wu[j][0]
            v_bu[0] = momentum * v_bu[0] + scale * g_bu[0]
            net.bu[0] -= v_bu[0]
            for j in range(net.hidden):
                for k in range(net.turn):
                    v_wt[j][k] = momentum * v_wt[j][k] + scale * g_wt[j][k]
                    net.wt[j][k] -= v_wt[j][k]
            for k in range(net.turn):
                v_bt[k] = momentum * v_bt[k] + scale * g_bt[k]
                net.bt[k] -= v_bt[k]


# ── rollout ─────────────────────────────────────────────────────────────────

def collect_episode(net: PolicyNet, env: EvaSim, seed: int, spawn_dist: float,
                    timeout: float, rng: random.Random) -> list[tuple[list[float], int, float, float]]:
    """Un épisode complet (graine déterministe) sous la politique courante :
    `(s, a, logp(a), r)` par pas. Le tirage softmax est l'exploration."""
    x, y = spawn_position(seed, spawn_dist)
    obs = env.reset(seed, x, y)
    traj: list[tuple[list[float], int, float, float]] = []
    prev_dist = obs["station_dist"]
    while not env.done and env.t < timeout:
        s = obs_features(obs)
        p_up, p_turn, _ = net.forward(s)
        a, logp = net.sample(p_up, p_turn, rng)
        cmd = EVA_ACTIONS[a]
        env.step(cmd.get("up", False), cmd.get("right", False), cmd.get("left", False))
        obs = env.obs()
        r = step_reward(prev_dist, obs["station_dist"], env.done, env.entry_speed)
        traj.append((s, a, logp, r))
        prev_dist = obs["station_dist"]
    return traj


def evaluate_greedy(net: PolicyNet, seeds: list[int], spawn_dist: float,
                    timeout: float) -> dict[str, float]:
    """Évaluation déterministe (action la plus probable) sur des graines
    **hors entraînement** : récompense `episode_reward` (mêmes règles que le
    jeu), taux de réussite, temps et vitesse d'entrée moyens."""
    env = EvaSim()
    rewards: list[float] = []
    entries: list[float] = []
    ok = 0
    for seed in seeds:
        x, y = spawn_position(seed, spawn_dist)
        obs = env.reset(seed, x, y)
        while not env.done and env.t < timeout:
            a = net.greedy(obs_features(obs))
            cmd = EVA_ACTIONS[a]
            env.step(cmd.get("up", False), cmd.get("right", False), cmd.get("left", False))
            obs = env.obs()
        rewards.append(episode_reward(None, {
            "success": env.done,
            "seconds": env.t,
            "entry_speed": env.entry_speed if env.done else 0.0,
            "final_dist": obs["station_dist"],
        }))
        if env.done:
            ok += 1
            entries.append(env.entry_speed)
    return {
        "ok": float(ok),
        "total": float(len(seeds)),
        "mean_reward": sum(rewards) / len(rewards),
        "best_reward": max(rewards),
        "mean_entry": (sum(entries) / len(entries)) if entries else 0.0,
    }


# ── mise à jour PPO ─────────────────────────────────────────────────────────

def ppo_update(
    net: PolicyNet,
    batch: list[tuple[list[float], int, float, float, float]],
    lr: float,
    momentum: float,
    clip_eps: float,
    ent_coef: float,
    val_coef: float,
    velocities: dict[str, Any],
) -> None:
    """Une passe sur un mini-lot : objectif PPO clipé (le rapport `ρ` des
    probabilités conjointes est borné à `[1−ε, 1+ε]` - une mise à jour ne
    peut pas dégrader la politique d'un coup), régression de la valeur (MSE
    sur les retours), bonus d'entropie (l'exploration), tout en descente de
    gradient avec élan (`velocities` persiste entre les lots)."""
    g_w1 = [[0.0] * net.hidden for _ in range(net.inputs)]
    g_b1 = [0.0] * net.hidden
    g_wu = [[0.0] * 1 for _ in range(net.hidden)]
    g_bu = [0.0] * 1
    g_wt = [[0.0] * net.turn for _ in range(net.hidden)]
    g_bt = [0.0] * net.turn
    g_wv = [[0.0] * 1 for _ in range(net.hidden)]
    g_bv = [0.0] * 1
    for s, a, logp_old, adv, ret in batch:
        up, turn = action_parts(a)
        h = net.hidden_of(s)
        zu = sum(h[j] * net.wu[j][0] for j in range(net.hidden)) + net.bu[0]
        p_up = 1.0 / (1.0 + math.exp(-zu))
        zt = [sum(h[j] * net.wt[j][k] for j in range(net.hidden)) + net.bt[k]
              for k in range(net.turn)]
        m = max(zt)
        exp = [math.exp(zt[k] - m) for k in range(net.turn)]
        ssum = sum(exp)
        p_turn = [e / ssum for e in exp]
        v = sum(h[j] * net.wv[j][0] for j in range(net.hidden)) + net.bv[0]
        logp = (math.log(max(p_up if up else 1.0 - p_up, 1e-12))
                + math.log(max(p_turn[turn], 1e-12)))
        ratio = math.exp(logp - logp_old)
        # objectif clipé : min(ρ·A, clip(ρ)·A)
        unclipped = ratio * adv
        clipped = max(min(ratio, 1.0 + clip_eps), 1.0 - clip_eps) * adv
        pol_gain = unclipped if unclipped < clipped else clipped
        # ∇(−L_pol) par rapport aux logits des deux têtes : −gain·dlogp/dz
        d_up = -pol_gain * (up - p_up)          # dlog σ/dz = up − σ
        d_turn = [-pol_gain * ((1.0 if k == turn else 0.0) - p_turn[k])
                  for k in range(net.turn)]
        # entropie : H = H(↑) + H(←/→/rien) ; dH_up/dz = p_up·(H_up + log p_up)
        h_up = -(p_up * math.log(max(p_up, 1e-12)) + (1.0 - p_up) * math.log(max(1.0 - p_up, 1e-12)))
        h_turn = -sum(p * math.log(max(p, 1e-12)) for p in p_turn)
        d_up += ent_coef * p_up * (h_up + math.log(max(p_up, 1e-12)))
        for k in range(net.turn):
            d_turn[k] += ent_coef * p_turn[k] * (h_turn + math.log(max(p_turn[k], 1e-12)))
        # valeur : MSE (ret − v)² → d = 2·(v − ret)
        d_val = 2.0 * (v - ret)
        # gradients cumulés sur les têtes
        g_bu[0] += d_up
        for j in range(net.hidden):
            g_wu[j][0] += d_up * h[j]
        for k in range(net.turn):
            g_bt[k] += d_turn[k]
            for j in range(net.hidden):
                g_wt[j][k] += d_turn[k] * h[j]
        g_bv[0] += val_coef * d_val
        for j in range(net.hidden):
            g_wv[j][0] += val_coef * d_val * h[j]
        # arrière vers la couche cachée (les trois têtes)
        for j in range(net.hidden):
            d_h = (1.0 - h[j] * h[j]) * (
                d_up * net.wu[j][0]
                + sum(d_turn[k] * net.wt[j][k] for k in range(net.turn))
                + val_coef * d_val * net.wv[j][0]
            )
            g_b1[j] += d_h
            for i in range(net.inputs):
                g_w1[i][j] += d_h * s[i]
    scale = lr / len(batch)
    vw1, vb1 = velocities["w1"], velocities["b1"]
    vwu, vbu = velocities["wu"], velocities["bu"]
    vwt, vbt = velocities["wt"], velocities["bt"]
    vwv, vbv = velocities["wv"], velocities["bv"]
    for i in range(net.inputs):
        for j in range(net.hidden):
            vw1[i][j] = momentum * vw1[i][j] + scale * g_w1[i][j]
            net.w1[i][j] -= vw1[i][j]
    for j in range(net.hidden):
        vb1[j] = momentum * vb1[j] + scale * g_b1[j]
        net.b1[j] -= vb1[j]
    for j in range(net.hidden):
        vwu[j][0] = momentum * vwu[j][0] + scale * g_wu[j][0]
        net.wu[j][0] -= vwu[j][0]
    vbu[0] = momentum * vbu[0] + scale * g_bu[0]
    net.bu[0] -= vbu[0]
    for j in range(net.hidden):
        for k in range(net.turn):
            vwt[j][k] = momentum * vwt[j][k] + scale * g_wt[j][k]
            net.wt[j][k] -= vwt[j][k]
    for k in range(net.turn):
        vbt[k] = momentum * vbt[k] + scale * g_bt[k]
        net.bt[k] -= vbt[k]
    for j in range(net.hidden):
        vwv[j][0] = momentum * vwv[j][0] + scale * g_wv[j][0]
        net.wv[j][0] -= vwv[j][0]
    vbv[0] = momentum * vbv[0] + scale * g_bv[0]
    net.bv[0] -= vbv[0]


# ── sérialisation (même esprit que policy.json / nn_policy.json) ────────────

def save_ppo(path: str, net: PolicyNet, meta: Optional[dict[str, Any]] = None) -> None:
    payload: dict[str, Any] = {
        "policy": "ppo",
        "features_version": FEATURES_VERSION,
        "inputs": net.inputs,
        "hidden": net.hidden,
        "action_list": [dict(a) for a in EVA_ACTIONS],
        "w1": net.w1,
        "b1": net.b1,
        "wu": net.wu,
        "bu": net.bu,
        "wt": net.wt,
        "bt": net.bt,
        "wv": net.wv,
        "bv": net.bv,
    }
    if meta:
        payload["meta"] = meta
    with open(path, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
        f.write("\n")


def load_ppo(path: str) -> tuple[PolicyNet, list[dict[str, bool]]]:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("policy") != "ppo":
        raise ValueError(f"{path} n'est pas une politique PPO")
    if data.get("features_version") != FEATURES_VERSION:
        raise ValueError(
            f"{path} : version de features {data.get('features_version')} "
            f"≠ {FEATURES_VERSION} attendue (re-entraîner avec ppo.py)"
        )
    net = PolicyNet(data["inputs"], data["hidden"])
    net.w1 = data["w1"]
    net.b1 = data["b1"]
    net.wu = data["wu"]
    net.bu = data["bu"]
    net.wt = data["wt"]
    net.bt = data["bt"]
    net.wv = data["wv"]
    net.bv = data["bv"]
    return net, [dict(a) for a in data["action_list"]]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--iters", type=int, default=100,
                    help="nombre d'itérations (chacune = `--rollouts` épisodes + PPO)")
    ap.add_argument("--rollouts", type=int, default=4,
                    help="épisodes collectés par itération (graines cyclées)")
    ap.add_argument("--epochs", type=int, default=2,
                    help="époques PPO par itération")
    ap.add_argument("--updates", type=int, default=32,
                    help="mini-lots ré-échantillonnés par époque (borne le coût "
                         "Python pur : on ne passe pas tout le lot à chaque époque)")
    ap.add_argument("--batch", type=int, default=128, help="taille des mini-lots PPO")
    ap.add_argument("--lr", type=float, default=0.02, help="taux d'apprentissage")
    ap.add_argument("--momentum", type=float, default=0.9)
    ap.add_argument("--hidden", type=int, default=24, help="neurones de la couche cachée")
    ap.add_argument("--clip", type=float, default=0.2, help="bornage du rapport de probabilités (ε)")
    ap.add_argument("--entropy", type=float, default=0.02, help="coefficient du bonus d'entropie")
    ap.add_argument("--value-coef", type=float, default=0.5, help="coefficient de la régression de valeur")
    ap.add_argument("--warmup", type=int, default=25,
                    help="épisodes de l'expert (`seek`) pour l'amorce supervisée")
    ap.add_argument("--imitate-epochs", type=int, default=20,
                    help="époques de l'amorce supervisée (données équilibrées)")
    ap.add_argument("--seeds", type=int, nargs="+", default=list(range(1, 9)),
                    help="graines d'entraînement (cyclées ; par défaut 1..8)")
    ap.add_argument("--eval-seeds", type=int, nargs="+", default=list(range(11, 16)),
                    help="graines d'évaluation hors entraînement (par défaut 11..15)")
    ap.add_argument("--eval-every", type=int, default=5, help="évaluation toutes les N itérations")
    ap.add_argument("--spawn-dist", type=float, default=300.0,
                    help="distance du crash au centre de la station (unités)")
    ap.add_argument("--timeout", type=float, default=60.0, help="délai maximal d'un épisode (s)")
    ap.add_argument("--seed", type=int, default=0, help="graine de l'optimisation (tirage)")
    ap.add_argument("--output", default="ppo_policy.json")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    net = PolicyNet(feature_size(), args.hidden, rng)
    env = EvaSim()

    print(f"PPO - poussée (sigmoïde) × rotation (softmax) sur l'observation "
          f"complète ({feature_size()} features)")
    print(f"{args.iters} itérations × {args.rollouts} épisodes · γ {GAMMA} · "
          f"lr {args.lr} · clip {args.clip} · entropie {args.entropy}")
    print(f"récompense : −2 s⁻¹, +1000 récupéré, pénalité d'entrée > "
          f"{ENTRY_SPEED_LIMIT} u/s\n")

    # 1) amorce experte : imiter `seek` sur ses propres épisodes (même
    # contrôle que le rollout, mais les actions viennent de l'expert)
    from policies import seek
    transitions: list[tuple[list[float], int]] = []
    for k in range(args.warmup):
        seed = args.seeds[k % len(args.seeds)]
        x, y = spawn_position(seed, args.spawn_dist)
        obs = env.reset(seed, x, y)
        while not env.done and env.t < args.timeout:
            cmd = seek(obs)
            s = obs_features(obs)
            a = None
            for i, act in enumerate(EVA_ACTIONS):
                if all(cmd.get(b, False) == act.get(b, False) for b in ("up", "left", "right")):
                    a = i
                    break
            if a is None:
                raise ValueError(f"action de `seek` hors espace discret : {cmd}")
            transitions.append((s, a))
            c = EVA_ACTIONS[a]
            env.step(c.get("up", False), c.get("right", False), c.get("left", False))
            obs = env.obs()
    # équilibre des classes : ~80 % des pas de `seek` sont « ne rien faire »
    # (la majorité domine l'entropie croisée et l'imitation finit immobile) -
    # les pas rares (pousser, tourner) sont dupliqués pour peser autant
    def is_rare(t: tuple[list[float], int]) -> bool:
        up, turn = action_parts(t[1])
        return up == 1 or turn != 2
    rare = [t for t in transitions if is_rare(t)]
    common = [t for t in transitions if not is_rare(t)]
    balanced = common + rare * 4
    imitate_expert(net, balanced, args.imitate_epochs, 0.1, 0.9, rng)
    print(f"amorce experte : {args.warmup} épisodes de `seek` imités "
          f"({len(transitions)} pas dont {len(rare)} rares ×4, "
          f"{args.imitate_epochs} époques)\n")

    # 2) itérations PPO
    best: dict[str, float] = {"mean_reward": -1e18}
    velocities = {
        "w1": [[0.0] * net.hidden for _ in range(net.inputs)],
        "b1": [0.0] * net.hidden,
        "wu": [[0.0] * 1 for _ in range(net.hidden)],
        "bu": [0.0] * 1,
        "wt": [[0.0] * net.turn for _ in range(net.hidden)],
        "bt": [0.0] * net.turn,
        "wv": [[0.0] * 1 for _ in range(net.hidden)],
        "bv": [0.0] * 1,
    }
    for it in range(1, args.iters + 1):
        # collecte : épisodes sous la politique courante
        data: list[tuple[list[float], int, float, float]] = []
        for k in range(args.rollouts):
            seed = args.seeds[(it * args.rollouts + k) % len(args.seeds)]
            data += collect_episode(net, env, seed, args.spawn_dist, args.timeout, rng)
        # retours actualisés puis avantages G − V normalisés par lot
        rewards = [r for (_, _, _, r) in data]
        rets = discounted_returns(rewards, GAMMA)
        advs = [0.0] * len(data)
        for i, (s, a, logp, r) in enumerate(data):
            _, _, v = net.forward(s)
            advs[i] = rets[i] - v
        mean_a = sum(advs) / len(advs)
        std_a = math.sqrt(max(1e-9, sum((x - mean_a) ** 2 for x in advs) / len(advs)))
        advs = [max(-ADVANTAGE_CLIP, min(ADVANTAGE_CLIP, (x - mean_a) / std_a)) for x in advs]
        batch = [(s, a, logp, adv, ret) for (s, a, logp, _), adv, ret in zip(data, advs, rets)]
        # époques PPO : chaque époque re-échantillonne `--updates` mini-lots
        # (le coût Python pur est borné - pas besoin de passer tout le lot)
        for _ in range(args.epochs):
            for _ in range(args.updates):
                idx = rng.sample(range(len(batch)), args.batch)
                mb = [batch[i] for i in idx]
                ppo_update(net, mb, args.lr, args.momentum, args.clip,
                           args.entropy, args.value_coef, velocities)
        # mesure en boucle fermée sur des graines hors entraînement
        if it % args.eval_every == 0:
            ev = evaluate_greedy(net, args.eval_seeds, args.spawn_dist, args.timeout)
            marker = ""
            if ev["mean_reward"] > best["mean_reward"]:
                best = ev
                marker = " ← meilleur"
                save_ppo(args.output, net, meta={
                    "task": "eva-return", "iters": it,
                    "eval": {k: round(v, 1) for k, v in ev.items()},
                })
            print(f"itér {it:>3}  éval {int(ev['ok'])}/{int(ev['total'])}  "
                  f"récomp. {ev['mean_reward']:>8.1f}  entrée {ev['mean_entry']:>5.1f} u/s  "
                  f"cumulé {best['mean_reward']:>8.1f}{marker}", flush=True)

    ev = evaluate_greedy(net, args.eval_seeds, args.spawn_dist, args.timeout)
    print(f"\nFinal - {int(ev['ok'])}/{int(ev['total'])} épisodes réussis (graines "
          f"{args.eval_seeds[0]}..{args.eval_seeds[-1]}), "
          f"récompense moyenne {ev['mean_reward']:.1f}, entrée {ev['mean_entry']:.1f} u/s")
    print(f"Meilleur en cours d'entraînement : {best['mean_reward']:.1f} "
          f"(sauvé dans {args.output})")
    print(f"Référence `seek` : ~893 (entrée ~48 u/s) - viser entrée ≤ "
          f"{ENTRY_SPEED_LIMIT} u/s pour ~985.")


if __name__ == "__main__":
    main()