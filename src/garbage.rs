//! Débris (éclats de météore).
//!
//! Portage de `garbage_type.bas` : `Garbage`, `generate_garbages`,
//! `moving_garbage` (le dessin `draw_garbage` viendra avec le rendu, Phase 2).

use rand::Rng;
use std::f64::consts::TAU;

use crate::marketplace::GARBAGE_PER_TRIANGLE;
use crate::geom::{Point, Triangle};
use crate::shape::Shape;

/// Un débris (ex `garbage_type`).
///
/// NB : `radius`, `orientation` et `angle` sont posés mais jamais lus - c'est
/// aussi le cas dans l'original (`garbage_type.bas`, le `circle` du rayon est
/// commenté) ; conservés pour la fidélité du modèle de données.
#[derive(Clone, Copy, Debug)]
pub struct Garbage {
    pub position: Point,
    #[allow(dead_code)]
    pub radius: f64,
    pub direction: f64,
    pub velocity: f64,
    #[allow(dead_code)]
    pub orientation: f64,
    #[allow(dead_code)]
    pub angle: f64,
    pub life: i32,
    /// Couleur ARGB 32 bits au format QB64 (AARRGGBB).
    pub rgba_color: u32,
}

impl Default for Garbage {
    fn default() -> Self {
        Garbage {
            position: Point::default(),
            radius: 0.0,
            direction: 0.0,
            velocity: 0.0,
            orientation: 0.0,
            angle: 0.0,
            life: 0,
            rgba_color: 0xFFFFFFFF,
        }
    }
}

/// Génère 12 débris à partir d'un triangle détruit (ex `generateGarbages`).
///
/// Les débris morts (`life = 0`) sont réutilisés avant d'étendre le tableau.
/// NB : l'original démarrait avec un tableau « vide » contenant un slot à
/// l'index -1 ; on part ici d'un `Vec` vide, le comportement observé est
/// identique (une particule de plus au premier tir, sans impact).
pub fn generate_garbages(
    garbages: &mut Vec<Garbage>,
    t: &Triangle,
    shapes: &[Shape],
    rng: &mut impl Rng,
) {
    let shape_velocity = shapes[t.shape_index as usize].velocity;
    for _ in 0..GARBAGE_PER_TRIANGLE {
        let g = Garbage {
            position: t.real_center,
            radius: rng.r#gen::<f64>() * 2.0,
            direction: rng.r#gen::<f64>() * TAU,
            velocity: shape_velocity * (1.0 + rng.r#gen::<f64>() * 3.0),
            orientation: rng.r#gen::<f64>() * TAU,
            life: ((rng.r#gen::<f64>() * 255.0) as i32) / 7,
            rgba_color: 0xFFFFFFFF,
            ..Default::default()
        };
        // cherche un slot mort à réutiliser
        let mut reused = None;
        for (idx, existing) in garbages.iter().enumerate() {
            if existing.life == 0 {
                reused = Some(idx);
                break;
            }
        }
        match reused {
            Some(idx) => garbages[idx] = g,
            None => garbages.push(g),
        }
    }
}

/// Déplace un débris (ex `movingGarbage`), `dt` en secondes.
///
/// NB : `life` est un compteur de frames (décrémenté par frame, comme
/// l'original) ; seule la position utilise `dt`.
pub fn moving_garbage(g: &mut Garbage, dt: f64) {
    if g.life == 0 {
        return;
    }
    g.life -= 1;
    g.position.x += g.direction.cos() * 60.0 * g.velocity * dt;
    g.position.y -= g.direction.sin() * 60.0 * g.velocity * dt;
}
