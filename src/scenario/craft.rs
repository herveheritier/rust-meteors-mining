//! Système de **fabrication** (onglet FABRICATION du magasin de la station) :
//! consommer les minerais de la soute (GOLD, IRON, WATER - `elements[1..=3]`)
//! pour fabriquer des **consommables** utilisables en vol :
//!
//! - **Bouclier temporaire** (`CRAFT_SHIELD`) - absorbe `TEMP_SHIELD_POINTS`
//!   impacts, dans tous les scénarios, jusqu'à épuisement ;
//! - **Boost de vitesse** (`CRAFT_BOOST`) - poussée × `BOOST_FACTOR` pendant
//!   `BOOST_DURATION` ;
//! - **Mine** (`CRAFT_MINE`) - explosif posé dans l'espace qui explose au
//!   contact d'un météore (rayon `MINE_RADIUS`).
//!
//! Fonctions pures testables sans macroquad (la pose de mine passe par
//! `generate::create_mine`, elle aussi sans macroquad).

use crate::config::{
    CRAFT_BOOST, CRAFT_BOOST_RECIPE, CRAFT_COUNT, CRAFT_MINE, CRAFT_MINE_RECIPE, CRAFT_SHIELD,
    CRAFT_SHIELD_RECIPE,
};
use crate::geom::Triangle;
use crate::state::{Element, GameState};

/// Une recette de fabrication : nom, ingrédients (quantités de GOLD, IRON,
/// WATER à prélever dans la soute) et description affichée (onglet
/// FABRICATION, tooltip).
#[derive(Clone, Copy, Debug)]
pub struct CraftSpec {
    /// Nom du consommable (magasin, HUD).
    pub name: &'static str,
    /// Ingrédients : (GOLD, IRON, WATER) prélevés dans la soute à chaque
    /// fabrication.
    pub ingredients: [i32; 3],
    /// Courte description (magasin, tooltip).
    pub description: &'static str,
}

/// Les trois recettes du système de fabrication (index `CRAFT_*`,
/// ingrédients définis dans `config.rs` - `CRAFT_*_RECIPE`).
pub const CRAFT_RECIPES: [CraftSpec; CRAFT_COUNT] = [
    CraftSpec {
        name: "BOUCLIER TEMPORAIRE",
        ingredients: CRAFT_SHIELD_RECIPE,
        description: "Absorbe 3 impacts de météore, tous scénarios",
    },
    CraftSpec {
        name: "BOOST DE VITESSE",
        ingredients: CRAFT_BOOST_RECIPE,
        description: "Poussée +50 % pendant 20 secondes",
    },
    CraftSpec {
        name: "MINE",
        ingredients: CRAFT_MINE_RECIPE,
        description: "Explose au contact d'un météore (rayon 130)",
    },
];

/// Recette `i` (index `CRAFT_*`).
pub fn craft_recipe(i: usize) -> &'static CraftSpec {
    &CRAFT_RECIPES[i.min(CRAFT_COUNT - 1)]
}

/// La soute contient-elle les ingrédients de la recette `i` ?
pub fn craft_affordable(_state: &GameState, elements: &[Element], i: usize) -> bool {
    let spec = craft_recipe(i);
    spec.ingredients
        .iter()
        .enumerate()
        .all(|(e, &need)| elements.get(e + 1).map_or(false, |el| el.count >= need))
}

/// Résultat d'une fabrication (`craft_consumable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftOutcome {
    /// Consommable fabriqué (ingrédients prélevés, ajouté à l'inventaire).
    Crafted(usize),
    /// Pas assez de minerais dans la soute.
    NotEnough,
}

/// Fabrique le consommable `i` (index `CRAFT_*`) : prélève les ingrédients
/// dans la soute (`elements[1..=3]`) et ajoute le consommable à l'inventaire
/// (`state.consumables`). La fabrication se fait **à la station** (onglet
/// FABRICATION du magasin) - le joueur décharge le minerai brut puis le
/// transforme en équipement. Pur (tests).
pub fn craft_consumable(state: &mut GameState, elements: &mut [Element], i: usize) -> CraftOutcome {
    let spec = craft_recipe(i);
    // vérifie que la soute couvre la recette avant de prélever quoi que ce soit
    if !craft_affordable(state, elements, i) {
        return CraftOutcome::NotEnough;
    }
    for (e, &need) in spec.ingredients.iter().enumerate() {
        if let Some(el) = elements.get_mut(e + 1) {
            el.count -= need;
        }
    }
    state.player.cargo_qty = state
        .player
        .cargo_qty
        .saturating_sub(spec.ingredients.iter().sum::<i32>());
    state.consumables[i] += 1;
    state.log_event(&format!("FABRICATION: {}", spec.name));
    CraftOutcome::Crafted(i)
}

/// Résultat de l'utilisation d'un consommable (`use_consumable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumableOutcome {
    /// Consommable utilisé et retiré de l'inventaire.
    Used,
    /// Aucun consommable de ce type en stock.
    None,
}

/// Utilise le consommable `i` (index `CRAFT_*`) en vol :
/// - SHIELD : ajoute `TEMP_SHIELD_POINTS` au bouclier temporaire (plafonné à
///   9 points) ;
/// - BOOST : relance le minuteur de boost (`BOOST_DURATION`) ;
/// - MINE : pose une mine à la position du vaisseau (`generate::create_mine`).
/// Le consommable est retiré de l'inventaire. Touches 1/2/3 (`game.rs`).
pub fn use_consumable(
    state: &mut GameState,
    shapes: &mut Vec<crate::shape::Shape>,
    triangles: &mut Vec<Triangle>,
    i: usize,
) -> ConsumableOutcome {
    if i >= CRAFT_COUNT || state.consumables[i] <= 0 {
        return ConsumableOutcome::None;
    }
    state.consumables[i] -= 1;
    match i {
        CRAFT_SHIELD => {
            state.temp_shield = (state.temp_shield + crate::config::TEMP_SHIELD_POINTS).min(9.0);
            state.send_message(&format!("TEMPORARY SHIELD: {:.0} POINTS", state.temp_shield));
        }
        CRAFT_BOOST => {
            state.boost_timer = crate::config::BOOST_DURATION;
            state.send_message("SPEED BOOST ENGAGED");
        }
        CRAFT_MINE => {
            let pos = shapes[crate::config::PLAYER_INDEX].position;
            crate::generate::create_mine(shapes, triangles, pos);
            state.send_message("MINE DEPLOYED");
        }
        _ => {}
    }
    state.log_event(&format!("UTILISÉ: {}", craft_recipe(i).name));
    ConsumableOutcome::Used
}

/// Multiplicateur de poussée courant (boost de vitesse actif ?) - lu par
/// `input::player_controls` pour amplifier l'accélération.
pub fn boost_factor(state: &GameState) -> f64 {
    if state.boost_timer > 0.0 {
        crate::config::BOOST_FACTOR
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cargo(elements: &mut [Element], gold: i32, iron: i32, water: i32) {
        elements[1].count = gold;
        elements[2].count = iron;
        elements[3].count = water;
    }

    #[test]
    fn craft_consumes_ingredients_and_adds_to_inventory() {
        let mut state = GameState::new();
        let mut elements = crate::state::default_elements();
        cargo(&mut elements, 0, 3, 2);
        state.player.cargo_qty = 5;

        // bouclier : 0 GOLD, 2 IRON, 1 WATER
        assert!(craft_affordable(&state, &elements, CRAFT_SHIELD));
        assert_eq!(
            craft_consumable(&mut state, &mut elements, CRAFT_SHIELD),
            CraftOutcome::Crafted(CRAFT_SHIELD)
        );
        assert_eq!(state.consumables[CRAFT_SHIELD], 1);
        assert_eq!(elements[2].count, 1); // IRON 3 → 1
        assert_eq!(elements[3].count, 1); // WATER 2 → 1
        assert_eq!(state.player.cargo_qty, 2); // 3 ingrédients prélevés
    }

    #[test]
    fn craft_refuses_when_cargo_is_short() {
        let mut state = GameState::new();
        let mut elements = crate::state::default_elements();
        cargo(&mut elements, 0, 1, 0); // pas assez d'IRON ni de WATER
        state.player.cargo_qty = 1;

        assert!(!craft_affordable(&state, &elements, CRAFT_SHIELD));
        assert_eq!(
            craft_consumable(&mut state, &mut elements, CRAFT_SHIELD),
            CraftOutcome::NotEnough
        );
        // rien n'a été prélevé
        assert_eq!(state.consumables[CRAFT_SHIELD], 0);
        assert_eq!(elements[2].count, 1);
        assert_eq!(state.player.cargo_qty, 1);
    }

    #[test]
    fn shield_consumable_grants_temp_shield() {
        let mut state = GameState::new();
        state.consumables[CRAFT_SHIELD] = 1;
        let mut shapes = vec![crate::shape::Shape::default()];
        let mut triangles = Vec::new();
        assert_eq!(
            use_consumable(&mut state, &mut shapes, &mut triangles, CRAFT_SHIELD),
            ConsumableOutcome::Used
        );
        assert_eq!(state.consumables[CRAFT_SHIELD], 0);
        assert_eq!(state.temp_shield, crate::config::TEMP_SHIELD_POINTS);
        // sans stock : rien ne se passe
        assert_eq!(
            use_consumable(&mut state, &mut shapes, &mut triangles, CRAFT_SHIELD),
            ConsumableOutcome::None
        );
        assert_eq!(state.temp_shield, crate::config::TEMP_SHIELD_POINTS);
    }

    #[test]
    fn mine_consumable_deploys_a_mine_at_ship_position() {
        let mut state = GameState::new();
        state.consumables[CRAFT_MINE] = 1;
        let mut shapes = vec![crate::shape::Shape {
            position: crate::geom::Point::new(123.0, 45.0),
            ..crate::shape::Shape::default()
        }];
        let mut triangles = Vec::new();
        assert_eq!(
            use_consumable(&mut state, &mut shapes, &mut triangles, CRAFT_MINE),
            ConsumableOutcome::Used
        );
        let mine = shapes.iter().find(|s| s.who_i_am == crate::config::WHOIAM_MINE);
        assert!(mine.is_some(), "une mine doit être posée");
        assert_eq!(mine.unwrap().position, crate::geom::Point::new(123.0, 45.0));
    }

    #[test]
    fn boost_factor_active_only_while_timer_runs() {
        let mut state = GameState::new();
        assert_eq!(boost_factor(&state), 1.0);
        state.boost_timer = 5.0;
        assert_eq!(boost_factor(&state), crate::config::BOOST_FACTOR);
    }
}
