//! Débris (éclats de météore et particules d'impact sur la base).
//!
//! Portage de `garbage_type.bas` : `Garbage`, `generate_garbages`,
//! `moving_garbage` (le dessin `draw_garbage` viendra avec le rendu, Phase 2).

use rand::Rng;
use std::f64::consts::TAU;

use crate::config::{STATION_IMPACT_DEBRIS_COLOR, STATION_IMPACT_DEBRIS_SPEED};
use crate::marketplace::{GARBAGE_PER_TRIANGLE, GARBAGE_SPIN};
use crate::geom::{Point, Triangle};
use crate::shape::Shape;

/// Un débris (ex `garbage_type`).
///
/// NB : `radius` et `orientation` sont posés mais jamais lus - c'est
/// aussi le cas dans l'original (`garbage_type.bas`, le `circle` du rayon est
/// commenté) ; conservés pour la fidélité du modèle de données.
/// `angle` : phase de **rotation propre** du débris (tournoiement réaliste,
/// dérive volontaire de l'original qui le laissait à 0) - avancée par
/// `moving_garbage`, lue par `render::draw_garbage`.
#[derive(Clone, Copy, Debug)]
pub struct Garbage {
    pub position: Point,
    #[allow(dead_code)]
    pub radius: f64,
    pub direction: f64,
    pub velocity: f64,
    #[allow(dead_code)]
    pub orientation: f64,
    /// Phase de rotation propre du débris (rad) : avancée à
    /// `spin_rate` rad/s par `moving_garbage`.
    pub angle: f64,
    /// Vitesse angulaire propre (rad/s, signe fixé à la génération).
    pub spin_rate: f64,
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
            spin_rate: 0.0,
            life: 0,
            rgba_color: 0xFFFFFFFF,
        }
    }
}

/// Génère 12 débris blancs à partir d'un triangle détruit (ex
/// `generateGarbages`).
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
    generate_garbages_with(garbages, t, shapes, rng, 0xFFFFFFFF, None);
}

/// Particules d'un **impact sur la base** : éclats éjectés par le triangle
/// de la station percuté par un météore (branche `WHOIAM_STATION` de
/// `game.rs`). Couleur rouille dédiée (`STATION_IMPACT_DEBRIS_COLOR`) et
/// **vitesse d'éjection propre** (`STATION_IMPACT_DEBRIS_SPEED`) : la
/// station est immobile, les éclats jaillissent du point d'impact (les
/// débris de météore, eux, héritent de la vitesse de leur forme).
pub fn generate_station_impact_garbages(
    garbages: &mut Vec<Garbage>,
    t: &Triangle,
    shapes: &[Shape],
    rng: &mut impl Rng,
) {
    generate_garbages_with(
        garbages,
        t,
        shapes,
        rng,
        STATION_IMPACT_DEBRIS_COLOR,
        Some(STATION_IMPACT_DEBRIS_SPEED),
    );
}

/// Implémentation commune : couleur et vitesse d'éjection paramétrables
/// (`None` = vitesse héritée de la forme du triangle).
fn generate_garbages_with(
    garbages: &mut Vec<Garbage>,
    t: &Triangle,
    shapes: &[Shape],
    rng: &mut impl Rng,
    rgba_color: u32,
    ejection_velocity: Option<f64>,
) {
    let shape_velocity = shapes[t.shape_index as usize].velocity;
    let base_velocity = ejection_velocity.unwrap_or(shape_velocity);
    for _ in 0..GARBAGE_PER_TRIANGLE {
        let g = Garbage {
            position: t.real_center,
            radius: rng.r#gen::<f64>() * 2.0,
            direction: rng.r#gen::<f64>() * TAU,
            velocity: base_velocity * (1.0 + rng.r#gen::<f64>() * 3.0),
            orientation: rng.r#gen::<f64>() * TAU,
            // tournoiement propre : phase et vitesse angulaire aléatoires,
            // signe aléatoire (les éclats tournent dans les deux sens)
            angle: rng.r#gen::<f64>() * TAU,
            spin_rate: GARBAGE_SPIN * (1.0 - 2.0 * rng.r#gen::<f64>()),
            life: ((rng.r#gen::<f64>() * 255.0) as i32) / 7,
            rgba_color,
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

/// Déplace un débris et le fait tourner (ex `movingGarbage`), `dt` en secondes.
///
/// NB : `life` est un compteur de frames (décrémenté par frame, comme
/// l'original) ; la position et la phase de rotation utilisent `dt`.
pub fn moving_garbage(g: &mut Garbage, dt: f64) {
    if g.life == 0 {
        return;
    }
    g.life -= 1;
    g.position.x += g.direction.cos() * 60.0 * g.velocity * dt;
    g.position.y -= g.direction.sin() * 60.0 * g.velocity * dt;
    g.angle += g.spin_rate * dt;
    if g.angle >= TAU || g.angle < 0.0 {
        g.angle %= TAU;
        if g.angle < 0.0 {
            g.angle += TAU;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::rand::SeedableRng;

    #[test]
    fn moving_garbage_advances_rotation() {
        // débris vivant : la phase de rotation avance de spin_rate·dt, la
        // direction est conservée (translation indépendante de la rotation)
        let mut g = Garbage {
            direction: 0.0,
            velocity: 1.0,
            angle: 0.0,
            spin_rate: GARBAGE_SPIN,
            life: 10,
            ..Default::default()
        };
        moving_garbage(&mut g, 1.0 / 60.0);
        let expected = GARBAGE_SPIN / 60.0;
        assert!((g.angle - expected).abs() < 1e-9);
        assert_eq!(g.life, 9);
        // la position a avancé selon la direction (translation)
        assert!(g.position.x > 0.0);
        assert_eq!(g.position.y, 0.0);
    }

    #[test]
    fn moving_garbage_negative_spin_wraps_into_tau() {
        // vitesse de rotation négative : la phase ne passe pas en négatif,
        // elle revient dans [0, TAU)
        let mut g = Garbage {
            direction: 0.0,
            velocity: 0.0,
            angle: 0.0,
            spin_rate: -GARBAGE_SPIN,
            life: 10,
            ..Default::default()
        };
        moving_garbage(&mut g, 1.0 / 60.0);
        assert!(g.angle >= 0.0 && g.angle < TAU);
        // équivalent -GARBAGE_SPIN/60 + TAU
        let expected = TAU - GARBAGE_SPIN / 60.0;
        assert!((g.angle - expected).abs() < 1e-9);
    }

    #[test]
    fn moving_garbage_dead_is_a_noop() {
        let mut g = Garbage {
            life: 0,
            spin_rate: GARBAGE_SPIN,
            ..Default::default()
        };
        moving_garbage(&mut g, 1.0 / 60.0);
        assert_eq!(g.angle, 0.0);
        assert_eq!(g.life, 0);
    }

    #[test]
    fn generated_garbages_have_spin() {
        // generate_garbages pose la phase et une vitesse de rotation
        // non nulle, les deux signes présents (sur 12+ débris)
        let mut rng = ::rand_chacha::ChaCha12Rng::seed_from_u64(42);
        let mut shapes = Vec::new();
        let meteor = Shape { velocity: 1.0, ..Default::default() };
        shapes.push(meteor);
        let mut t = Triangle::default();
        t.create(
            crate::geom::Point::new(0.0, 0.0),
            crate::geom::Point::new(10.0, 0.0),
            crate::geom::Point::new(5.0, 8.0),
        );
        t.shape_index = 0;
        let mut garbages = Vec::new();
        generate_garbages(&mut garbages, &t, &shapes, &mut rng);
        assert_eq!(garbages.len(), GARBAGE_PER_TRIANGLE);
        let mut pos = 0;
        let mut neg = 0;
        for g in &garbages {
            assert!(g.spin_rate.abs() > 0.0);
            assert!((0.0..TAU).contains(&g.angle));
            if g.spin_rate > 0.0 { pos += 1; } else { neg += 1; }
        }
        assert!(pos > 0 && neg > 0, "les deux signes de rotation doivent apparaître");
    }

    #[test]
    fn station_impact_garbages_are_colored_and_ejected() {
        // les particules d'un impact sur la base portent la couleur dédiée
        // (éclats rouille) et une vitesse d'éjection propre : la station est
        // immobile (velocity 0), sans elle elles resteraient sur place
        let mut rng = ::rand_chacha::ChaCha12Rng::seed_from_u64(42);
        let mut shapes = Vec::new();
        let station = Shape {
            velocity: 0.0,
            ..Default::default()
        };
        shapes.push(station);
        let mut t = Triangle::default();
        t.create(
            crate::geom::Point::new(0.0, 0.0),
            crate::geom::Point::new(10.0, 0.0),
            crate::geom::Point::new(5.0, 8.0),
        );
        t.shape_index = 0;
        let mut garbages = Vec::new();
        generate_station_impact_garbages(&mut garbages, &t, &shapes, &mut rng);
        assert_eq!(garbages.len(), GARBAGE_PER_TRIANGLE);
        for g in &garbages {
            assert_eq!(g.rgba_color, STATION_IMPACT_DEBRIS_COLOR);
            assert!(g.velocity > 0.0, "éclat sans vitesse d'éjection");
        }
    }
}
