//! Réputation et rangs (scénarios à économie - extrait de `scenario.rs`) :
//! la précision de tir amplifie le gain, les minerais déchargés en rapportent
//! aussi ; chaque palier franchi annonce le rang au HUD et la remise du rang
//! courant s'applique à **tous** les coûts de la station (magasin,
//! ravitaillement, modes de déplacement). Seuils et noms : `PROGRESSION_RANKS`
//! de `marketplace.rs` (généré par l'outil de gestion).

use super::*;

// ─── Réputation et rangs ────────────────────────────────────────────────────

/// Précision de tir du joueur (0..1) : part de tirs **non perdus** - 1 = aucun
/// tir perdu (tous les tirs ont touché un astéroïde). Sans tir : 0. Sert au
/// gain de réputation (`on_meteor_destroyed`) et à la remise sur les coûts
/// (`current_discount`).
pub fn shooting_precision(state: &GameState) -> f64 {
    if state.bullets_fired > 0 {
        (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64).max(0.0)
    } else {
        0.0
    }
}

/// Réputation gagnée par un astéroïde détruit : le gain de base
/// (`reputation_per_asteroid`) est multiplié par `1 + poids × précision` - la
/// précision de tir (part de tirs non perdus) récompense donc les tirs
/// efficaces. Appelé par `game.rs` quand un météore meurt sous une balle.
pub fn on_meteor_destroyed(state: &mut GameState) {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return;
    }
    let precision = shooting_precision(state);
    let before = rank_at(s.ranks, state.resources.reputation);
    state.resources.reputation +=
        s.reputation_per_asteroid * (1.0 + s.reputation_precision_weight * precision);
    // un palier de réputation franchi débloque le rang suivant : annoncé au
    // HUD (ex « RANK UP: PILOT »)
    let after = rank_at(s.ranks, state.resources.reputation);
    if let (Some(after), Some(before)) = (after, before) {
        if after != before {
            state.send_message(&format!("RANK UP: {}", after.name));
        }
    }
}

/// Rang atteint pour une réputation donnée dans une table de rangs : le plus
/// haut palier dont le seuil est franchi - `None` si la table est vide (jeu
/// libre). Fonction pure (tests). La durée de vie du rang renvoyé est celle
/// de la table passée (`PROGRESSION_RANKS` est `'static`).
pub fn rank_at(ranks: &[ReputationRank], reputation: f64) -> Option<&ReputationRank> {
    ranks.iter().rev().find(|r| reputation >= r.threshold)
}

/// Nom du rang de réputation courant du scénario (dernier palier dont le
/// seuil est atteint), ou `None` si le scénario n'a pas de rangs - affiché au
/// HUD à côté du compteur de réputation.
pub fn current_rank(state: &GameState) -> Option<&'static str> {
    rank_at(scenario(state.scenario).ranks, state.resources.reputation).map(|r| r.name)
}

/// Remise (pourcentage 0..100) accordée sur les coûts de la station par la
/// réputation : celle du plus haut rang atteint (0 sans rang ou table vide).
/// Pure (tests).
pub fn reputation_discount(ranks: &[ReputationRank], reputation: f64) -> i32 {
    rank_at(ranks, reputation).map_or(0, |r| r.discount_percent.clamp(0, 100))
}

/// Coût après remise de réputation : `cost × (100 − remise) / 100`, arrondi à
/// l'entier inférieur (jamais négatif). Pure (tests).
pub fn discounted_cost(cost: i32, discount_percent: i32) -> i32 {
    (cost * (100 - discount_percent.clamp(0, 100))) / 100
}

/// Remise du scénario courant : la remise du rang atteint (`reputation_discount`),
/// **amplifiée par la précision de tir** - la remise est multipliée par
/// `1 + poids × précision` (voir `discount_precision_weight` de `Scenario`) et
/// bornée à 100 %. Sans rang ou poids nul, la précision ne change rien.
pub fn current_discount(state: &GameState) -> i32 {
    let s = scenario(state.scenario);
    let base = reputation_discount(s.ranks, state.resources.reputation);
    if base == 0 || s.discount_precision_weight <= 0.0 {
        return base;
    }
    let boosted = base as f64 * (1.0 + s.discount_precision_weight * shooting_precision(state));
    boosted.round().clamp(0.0, 100.0) as i32
}
