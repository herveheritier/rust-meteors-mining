//! Difficulté adaptative (vagues progressives).
//!
//! La difficulté croît avec le **temps de session** (`state.session_time`) :
//! à chaque palier de `DIFFICULTY_RAMP_SECONDS` (2 min), les météores
//! deviennent plus rapides, plus gros (plus de triangles), plus nombreux et
//! plus denses - particulièrement utile en Survival, où la session dure.
//! Toutes les fonctions sont **pures** (testables sans macroquad) : la
//! boucle de jeu (`game.rs`) et la génération (`generate.rs`) les appellent.

use crate::config::DIFFICULTY_RAMP_SECONDS;
use crate::marketplace::{METEOR_VELOCITY_MAX, TRIANGLES_IN_SHAPE_MAX, TRIANGLES_IN_SHAPE_MIN};
use crate::state::GameState;

/// Palier de difficulté courant (0 au départ) : nombre de fois que
/// `DIFFICULTY_RAMP_SECONDS` se sont écoulés depuis le début de la partie.
pub fn level(state: &GameState) -> i32 {
    (state.session_time / DIFFICULTY_RAMP_SECONDS) as i32
}

/// Multiplicateur de la **vitesse maximale** des météores : +15 % par palier,
/// plafonné à ×2 (au-delà, le jeu deviendrait injouable).
pub fn velocity_multiplier(state: &GameState) -> f64 {
    (1.0 + 0.15 * level(state) as f64).min(2.0)
}

/// Vitesse maximale effective des météores au palier courant.
pub fn meteor_velocity_max(state: &GameState) -> f64 {
    METEOR_VELOCITY_MAX * velocity_multiplier(state)
}

/// Nombre maximal de triangles par météore au palier courant : le plafond
/// grimpe de 2 triangles par palier, borné à `TRIANGLES_IN_SHAPE_MAX × 2`
/// (les gros astéroïdes des vagues tardives).
pub fn triangle_count_max(state: &GameState) -> i32 {
    (TRIANGLES_IN_SHAPE_MAX + 2 * level(state)).min(TRIANGLES_IN_SHAPE_MAX * 2)
}

/// Nombre de triangles demandé pour un météore courant (bornes du palier).
pub fn triangle_count(state: &GameState, rng: &mut impl rand::Rng) -> usize {
    let min = TRIANGLES_IN_SHAPE_MIN as f64;
    let max = triangle_count_max(state) as f64;
    (min + (max - min) * rng.r#gen::<f64>()) as usize
}

/// Probabilité (par frame) de génération automatique d'un météore : 5 % au
/// départ, +1 % par palier, plafonnée à 15 % (les vagues s'accélèrent).
pub fn spawn_chance(state: &GameState) -> f64 {
    (0.05 + 0.01 * level(state) as f64).min(0.15)
}

/// Bonus de **population** (nombre maximal de météores) accordé par la
/// difficulté : +10 par palier (en plus du +1 par météore détruit).
pub fn max_meteors_bonus(state: &GameState) -> i32 {
    10 * level(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GameState;

    fn state_at(seconds: f64) -> GameState {
        let mut s = GameState::new();
        s.session_time = seconds;
        s
    }

    #[test]
    fn level_ramps_with_session_time() {
        assert_eq!(level(&state_at(0.0)), 0);
        assert_eq!(level(&state_at(DIFFICULTY_RAMP_SECONDS - 0.1)), 0);
        assert_eq!(level(&state_at(DIFFICULTY_RAMP_SECONDS)), 1);
        assert_eq!(level(&state_at(6.0 * DIFFICULTY_RAMP_SECONDS)), 6);
    }

    #[test]
    fn velocity_multiplier_increases_then_caps() {
        let s0 = state_at(0.0);
        let s1 = state_at(DIFFICULTY_RAMP_SECONDS);
        let s10 = state_at(10.0 * DIFFICULTY_RAMP_SECONDS);
        assert_eq!(velocity_multiplier(&s0), 1.0);
        assert!((velocity_multiplier(&s1) - 1.15).abs() < 1e-9);
        // plafond ×2 (palier 6 = 1.9, palier 7 = 2.05 → 2.0)
        assert_eq!(velocity_multiplier(&s10), 2.0);
        assert!(meteor_velocity_max(&s1) > METEOR_VELOCITY_MAX);
    }

    #[test]
    fn triangle_count_stays_in_bounds() {
        use ::rand::SeedableRng;
        use ::rand_chacha::ChaCha12Rng;
        let mut rng = ChaCha12Rng::seed_from_u64(1);
        let s = state_at(4.0 * DIFFICULTY_RAMP_SECONDS);
        for _ in 0..100 {
            let n = triangle_count(&s, &mut rng) as i32;
            assert!(n >= TRIANGLES_IN_SHAPE_MIN);
            assert!(n <= triangle_count_max(&s));
        }
        // le plafond monte avec le palier, borné au double du plafond d'origine
        assert!(triangle_count_max(&state_at(0.0)) == TRIANGLES_IN_SHAPE_MAX);
        assert!(triangle_count_max(&state_at(10.0 * DIFFICULTY_RAMP_SECONDS)) == TRIANGLES_IN_SHAPE_MAX * 2);
    }

    #[test]
    fn spawn_chance_and_population_grow() {
        assert_eq!(spawn_chance(&state_at(0.0)), 0.05);
        assert!((spawn_chance(&state_at(DIFFICULTY_RAMP_SECONDS)) - 0.06).abs() < 1e-9);
        assert_eq!(spawn_chance(&state_at(20.0 * DIFFICULTY_RAMP_SECONDS)), 0.15);
        assert_eq!(max_meteors_bonus(&state_at(0.0)), 0);
        assert_eq!(max_meteors_bonus(&state_at(3.0 * DIFFICULTY_RAMP_SECONDS)), 30);
    }
}
