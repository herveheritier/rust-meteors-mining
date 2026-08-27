//! Atelier d'amélioration du vaisseau (magasin de la station, onglet
//! ATELIER - extrait de `scenario.rs`) : capacités par niveau et coût des
//! extensions (réservoir `FUEL_UPGRADE_TRACK`, chargeur
//! `AMMO_UPGRADE_TRACK`, soute `CARGO_UPGRADE_TRACK`), lignes affichées au
//! magasin et achat - persistés avec la progression (`super::progression`).

use super::*;

// ─── Atelier d'amélioration du vaisseau ─────────────────────────────────────

/// Capacité d'une ligne d'amélioration au niveau `level` (0 = base) : base +
/// bonus des extensions achetées, niveau borné au nombre d'extensions.
/// Fonction pure (tests).
pub fn track_capacity(track: &UpgradeTrack, level: i32) -> i32 {
    let level = level.clamp(0, track.tiers.len() as i32);
    track.base + track.tiers.iter().take(level as usize).map(|t| t.bonus).sum::<i32>()
}

/// Prochaine extension d'une ligne (`None` = niveau max atteint).
pub fn next_upgrade(track: &UpgradeTrack, level: i32) -> Option<&ShipUpgrade> {
    track.tiers.get(level.clamp(0, track.tiers.len() as i32) as usize)
}

/// Capacité maximale du réservoir de carburant (base + extensions achetées).
pub fn fuel_capacity(state: &GameState) -> f64 {
    track_capacity(&scenario(state.scenario).fuel_upgrades, state.resources.fuel_level) as f64
}

/// Capacité maximale du chargeur de munitions (base + extensions achetées).
pub fn ammo_capacity(state: &GameState) -> i32 {
    track_capacity(&scenario(state.scenario).ammo_upgrades, state.resources.ammo_level)
}

/// Capacité maximale de la soute (base + extensions achetées).
pub fn cargo_capacity(state: &GameState) -> i32 {
    track_capacity(&scenario(state.scenario).cargo_upgrades, state.resources.cargo_level)
}

/// Ligne d'affichage d'une amélioration de l'atelier : libellé, capacité
/// actuelle et prochaine extension (`None` = au max) - pour l'écran atelier
/// (`shop_render::draw_shop_box`).
pub struct UpgradeLine {
    /// Libellé de la ligne (ex « FUEL TANK »).
    pub label: &'static str,
    /// Capacité actuelle.
    pub capacity: i32,
    /// Prochaine extension (nom, coût, bonus) - `None` = niveau max.
    pub next: Option<ShipUpgrade>,
}

/// Ligne d'affichage d'une amélioration pour l'atelier (voir `UpgradeLine`).
pub fn upgrade_line(state: &GameState, track: UpgradeTrackId) -> UpgradeLine {
    let s = scenario(state.scenario);
    let (upgrades, level, capacity) = match track {
        UpgradeTrackId::Fuel => (
            s.fuel_upgrades,
            state.resources.fuel_level,
            fuel_capacity(state) as i32,
        ),
        UpgradeTrackId::Ammo => (s.ammo_upgrades, state.resources.ammo_level, ammo_capacity(state)),
        UpgradeTrackId::Cargo => (s.cargo_upgrades, state.resources.cargo_level, cargo_capacity(state)),
    };
    let mut next = next_upgrade(&upgrades, level).copied();
    // le coût affiché à l'atelier est le coût réellement payé (remisé)
    if let Some(u) = &mut next {
        u.cost = discounted_cost(u.cost, current_discount(state));
    }
    UpgradeLine {
        label: upgrades.label,
        capacity,
        next,
    }
}

/// Résultat d'un achat à l'atelier (`buy_upgrade`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeOutcome {
    /// Ligne déjà au niveau maximum (ou pas d'atelier hors économie).
    Maxed,
    /// Extension achetée (coût en crédits déduit, niveau +1).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète la prochaine extension d'une ligne à l'atelier de la station : paie
/// en crédits et fait passer la ligne au niveau suivant - les réservoirs
/// montent à la nouvelle capacité (plein inclus) et la soute s'agrandit
/// immédiatement. Hors scénario à économie (pas d'atelier) ou ligne au max :
/// sans effet (`Maxed`). Appelé par le magasin (bouton SHOP de la
/// boîte DOCK STATION).
pub fn buy_upgrade(state: &mut GameState, track: UpgradeTrackId) -> UpgradeOutcome {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return UpgradeOutcome::Maxed;
    }
    let (upgrades, level) = match track {
        UpgradeTrackId::Fuel => (s.fuel_upgrades, state.resources.fuel_level),
        UpgradeTrackId::Ammo => (s.ammo_upgrades, state.resources.ammo_level),
        UpgradeTrackId::Cargo => (s.cargo_upgrades, state.resources.cargo_level),
    };
    let next = match next_upgrade(&upgrades, level) {
        Some(u) => u,
        None => return UpgradeOutcome::Maxed,
    };
    // la réputation remise les coûts de la station (atelier, ravitaillement,
    // modes) : le prix affiché et payé est le coût remisé
    let cost = discounted_cost(next.cost, current_discount(state));
    if state.resources.credits < cost {
        state.send_message(&format!(
            "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
            next.name, cost
        ));
        return UpgradeOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    match track {
        UpgradeTrackId::Fuel => {
            state.resources.fuel_level += 1;
            state.resources.fuel = fuel_capacity(state); // plein à la nouvelle capacité
        }
        UpgradeTrackId::Ammo => {
            state.resources.ammo_level += 1;
            // chargeur agrandi : chaque arme possédée passe à la nouvelle
            // capacité, pleine (les armes non possédées restent à 0)
            for i in 0..weapon_slot_count() {
                if weapon_owned(state, i) {
                    state.resources.weapon_ammo[i] = ammo_capacity(state);
                }
            }
        }
        UpgradeTrackId::Cargo => {
            state.resources.cargo_level += 1;
            state.player.cargo_size = cargo_capacity(state);
        }
    }
    state.send_message(&format!("{} PURCHASED: -{} CREDITS", next.name, cost));
    UpgradeOutcome::Purchased(cost)
}
