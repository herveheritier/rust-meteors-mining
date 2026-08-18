//! Cosmonaute — chargement de `assets/cosmonaute.json`.
//!
//! Ce fichier est une exportation de l'éditeur « meshes-designer » (le format
//! de mesh du projet d'origine) : une liste de `planes`, chacun étant une
//! région polygonale **déjà triangulée** — `verts` = sommets `[x, y]`,
//! `faces` = triangles avec leurs indices dans `verts` et une couleur RGBA
//! (flottants 0..1) par face. Les autres champs du fichier (`zoom`, `cx`,
//! `cy`, `grid`…) sont de l'état d'éditeur, ignorés.
//!
//! Le mesh est converti en une `Shape` + ses `Triangle` du modèle du jeu
//! (`geom.rs`/`shape.rs`) : une `Triangle` par face via `Triangle::create`,
//! couleur RGBA → ARGB 32 bits (`argb32`), axe y **retourné** (l'éditeur
//! travaille y vers le haut, le jeu y vers le bas) et mise à l'échelle
//! (`COSMONAUTE_EVA_SCALE`). `create_eva_cosmonaut` construit le **pilote
//! éjecté** quand le vaisseau est détruit (taille vaisseau, ~26 unités) :
//! garé hors écran en bord de monde et téléporté par `game.rs`
//! (`activate_cosmonaut`/`rescue_cosmonaut`). Décoratif (`is_collider =
//! false`, jamais détruit), il n'est jamais affiché à côté de la base.

use serde::Deserialize;

use crate::config::{argb32, TEXTURE_NONE, WHOIAM_COSMONAUT};
use crate::geom::{Point, Triangle};
use crate::shape::{compute_shape_center, free_shape, Shape};

/// Le fichier est embarqué dans le binaire (`include_str!`), comme les
/// textures et les sons : pas d'accès au système de fichiers au runtime.
pub const COSMONAUTE_JSON: &str = include_str!("../assets/cosmonaute.json");

/// Échelle du cosmonaute EVA (pilote éjecté, vaisseau détruit) : ~17 unités
/// éditeur × 1,5 → ~26 unités monde — la taille du vaisseau, manœuvrable
/// dans la zone d'accostage (rayon 15).
pub const COSMONAUTE_EVA_SCALE: f64 = 1.5;

/// Poste « garé » du cosmonaute EVA : en bord de monde (coin sud-ouest), loin
/// de la caméra de départ — invisible tant que le vaisseau n'est pas détruit
/// (`game.rs` le téléporte au crash, puis le ramène ici après le secours).
pub const COSMONAUTE_EVA_PARK: Point = Point::new(-1400.0, -1400.0);

/// Amplitude (radians) du balancement des **bras** pendant la poussée
/// (~26° de part et d'autre — un remuement énergique).
const SWING_ARMS: f64 = 0.45;
/// Amplitude relative des **jambes** : plus courte que les bras (elles
/// s'agitent moins fort).
const SWING_LEGS_FACTOR: f64 = 0.65;
/// Pulsation (rad/s) du balancement : ~2,2 Hz.
const SWING_OMEGA: f64 = 14.0;
/// Vitesse de rattrapage de l'angle cible : la pose s'installe dès la poussée
/// et les membres **retombent au repos** (constante de temps ~1/14 s) quand
/// elle cesse.
const SWING_CHASE: f64 = 14.0;

/// Racine du fichier « meshes-designer » — seuls les plans portent le mesh.
#[derive(Deserialize)]
struct CosmonautFile {
    planes: Vec<Plane>,
}

#[derive(Deserialize)]
struct Plane {
    /// Sommets du plan, en coordonnées de l'éditeur (y vers le haut).
    verts: Vec<[f64; 2]>,
    /// Triangles du plan, indices dans `verts`.
    faces: Vec<Face>,
}

#[derive(Deserialize)]
struct Face {
    /// Indices des 3 sommets de la face dans `verts`.
    v: [usize; 3],
    /// Couleur RGBA de la face, flottants 0..1.
    color: [f32; 4],
}

/// RGBA (flottants 0..1) → ARGB 32 bits au format QB64 (AARRGGBB).
fn rgba_to_argb(rgba: [f32; 4]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    argb32(byte(rgba[3]), byte(rgba[0]), byte(rgba[1]), byte(rgba[2]))
}

/// Construit le cosmonaute EVA — le pilote contrôlé quand le vaisseau est
/// détruit (`game.rs`) : petit, garé hors écran jusqu'à l'éjection. Garé, il
/// est cullé (hors limites de dessin) ; une fois éjecté, la caméra le suit
/// donc il est toujours affiché.
pub fn create_eva_cosmonaut(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) -> usize {
    build_cosmonaut(shapes, triangles, COSMONAUTE_EVA_SCALE, COSMONAUTE_EVA_PARK)
}

/// Construit une forme « cosmonaute » à partir de `assets/cosmonaute.json` :
/// une `Triangle` par face du fichier, à la suite des triangles existants,
/// mise à l'échelle `scale` et posée à `position` (les sommets sont en
/// coordonnées locales, la forme se dessine autour de sa position). Garé, il
/// n'est pas dessiné (culling, `show_all_parts = false`) ; éjecté, la caméra
/// le suit donc il est toujours affiché. Renvoie l'index de la forme créée
/// (réutilise une forme détruite au même nombre de triangles quand c'est
/// possible, comme `meshes_to_shape`).
fn build_cosmonaut(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    scale: f64,
    position: Point,
) -> usize {
    let file: CosmonautFile =
        serde_json::from_str(COSMONAUTE_JSON).expect("assets/cosmonaute.json : JSON invalide");
    let nbr = file.planes.iter().map(|p| p.faces.len()).sum();

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue — même schéma que `meshes_to_shape`
    let shape_index = match free_shape(shapes, nbr) {
        Some(idx) => idx,
        None => {
            let idx = shapes.len();
            shapes.push(Shape::default());
            triangles.resize(triangles.len() + nbr, Triangle::default());
            shapes[idx].first_triangle = triangles.len() - nbr;
            shapes[idx].last_triangle = triangles.len() - 1;
            idx
        }
    };

    let shape = &mut shapes[shape_index];
    shape.id = shape_index as i32;
    shape.life = nbr as i32;
    shape.who_i_am = WHOIAM_COSMONAUT;
    shape.is_collider = false; // décoratif : jamais de collision
    shape.show_all_parts = false; // garé : cullé (éjecté, la caméra le suit)
    shape.texture = TEXTURE_NONE; // rendu en couleurs par face, sans texture
    shape.shape_color = 0xFFFFFFFF; // repli (jamais utilisé : chaque face a sa couleur)
    shape.position = position;
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;

    // classification des membres par plan (repère de l'éditeur, y vers le
    // haut) : les **bras** sont les plans extérieurs du haut (|x| grand, y > 0),
    // les **jambes** les plans du bas (y < 0 — sous le buste) ; l'articulation
    // de chaque membre est son sommet le plus proche du centre du personnage
    // (l'épaule, la hanche). Servira à l'animation de la poussée
    // (`animate_eva_cosmonaut` : bras et jambes qui s'agitent).
    let mut total = (0.0, 0.0);
    let mut verts_count = 0usize;
    for plane in &file.planes {
        for v in &plane.verts {
            total.0 += v[0];
            total.1 += v[1];
            verts_count += 1;
        }
    }
    let figure_center = Point::new(total.0 / verts_count as f64, total.1 / verts_count as f64);
    let limbs: Vec<(i32, Point)> = file
        .planes
        .iter()
        .map(|plane| {
            let mut cx = 0.0;
            let mut cy = 0.0;
            for v in &plane.verts {
                cx += v[0];
                cy += v[1];
            }
            let pc = Point::new(cx / plane.verts.len() as f64, cy / plane.verts.len() as f64);
            let limb = if pc.y < -2.0 {
                2 // jambes : sous le buste
            } else if pc.x.abs() > 1.5 && pc.y > 2.0 {
                1 // bras : à l'extérieur, en haut
            } else {
                0
            };
            // articulation = sommet du plan le plus proche du centre
            let pivot = if limb == 0 {
                Point::default()
            } else {
                let mut best: Option<(Point, f64)> = None;
                for v in &plane.verts {
                    let p = Point::new(v[0], v[1]);
                    let d = (p.x - figure_center.x).hypot(p.y - figure_center.y);
                    if best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((p, d));
                    }
                }
                best.map(|(p, _)| p).unwrap_or_default()
            };
            (limb, pivot)
        })
        .collect();

    let first = shape.first_triangle;
    let mut k = 0usize;
    for (plane, &(limb, pivot)) in file.planes.iter().zip(limbs.iter()) {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            // axe y retourné (éditeur y↑ → jeu y↓) + mise à l'échelle
            let pt = |v: [f64; 2]| Point::new(v[0] * scale, -v[1] * scale);
            let mut t = Triangle::default();
            t.create(pt(plane.verts[i]), pt(plane.verts[j]), pt(plane.verts[l]));
            t.color = rgba_to_argb(face.color);
            // membre + articulation (repère local, y retourné + échelle)
            t.limb = limb;
            t.pivot = Point::new(pivot.x * scale, -pivot.y * scale);
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }
    debug_assert_eq!(k, nbr);

    compute_shape_center(shape, triangles);
    // centre fixe : `moving_shape` fait converger `center` vers
    // `target_center` (÷100 par frame) — posés égaux, le cosmonaute reste
    // immobile à sa position (vitesse et rotation nulles).
    shape.center = shape.target_center;
    shape_index
}

/// Anime les membres du cosmonaute EVA : les **bras et les jambes s'agitent**
/// (bascule de leurs triangles autour de leurs articulations, `Triangle.limb`/
/// `pivot`) tant qu'il pousse, puis **retombent au repos** — l'angle cible
/// oscille pendant la poussée et vaut 0 sinon (`Shape.anim_angle` rattrape la
/// cible : la pose s'installe à la montée, retombe doucement à l'arrêt).
/// `time` est l'horloge (ex `get_time()`), `dt` le pas de la frame ; les
/// sommets locaux sont tournés en place, `moving_shape` recalcule les
/// positions réelles dans la foulée. Sans effet (ni coût) une fois au repos.
pub fn animate_eva_cosmonaut(
    shape: &mut Shape,
    triangles: &mut [Triangle],
    thrusting: bool,
    time: f64,
    dt: f64,
) {
    // cible : oscillation pendant la poussée, 0 au repos
    let target = if thrusting { SWING_ARMS * (time * SWING_OMEGA).sin() } else { 0.0 };
    // rattrapage lissé : la pose s'installe à la poussée et retombe au repos
    let step = (target - shape.anim_angle) * (1.0 - (-dt * SWING_CHASE).exp());
    shape.anim_angle += step;
    if step.abs() < 1e-9 {
        return;
    }
    for i in shape.first_triangle..=shape.last_triangle {
        let t = &mut triangles[i];
        let amp = match t.limb {
            1 => step,                         // bras : balancement complet
            2 => step * SWING_LEGS_FACTOR,     // jambes : plus court
            _ => continue,                     // buste, tête : immobiles
        };
        // rotation des sommets autour de l'articulation (repère local) puis
        // centre recalculé
        let pivot = t.pivot;
        t.a.rotate_around(pivot, amp);
        t.b.rotate_around(pivot, amp);
        t.c.rotate_around(pivot, amp);
        t.center = Point::new((t.a.x + t.b.x + t.c.x) / 3.0, (t.a.y + t.b.y + t.c.y) / 3.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eva_cosmonaut_has_all_faces_with_their_color() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_eva_cosmonaut(&mut shapes, &mut triangles);

        // 7 plans, 91 faces → 91 triangles vivants dans une seule forme
        assert_eq!(shapes.len(), 1);
        let s = &shapes[idx];
        assert_eq!(s.who_i_am, WHOIAM_COSMONAUT);
        assert_eq!(s.life, 91);
        assert_eq!(s.last_triangle - s.first_triangle + 1, 91);
        assert!(!s.is_collider);
        for i in s.first_triangle..=s.last_triangle {
            let t = &triangles[i];
            assert_eq!(t.life, 1);
            assert_eq!(t.shape_index, idx as i32);
            assert_ne!(t.color, 0, "chaque face doit porter sa couleur");
        }
        // les couleurs du fichier : la combinaison claire et la visière sombre
        let colors: std::collections::HashSet<u32> =
            (s.first_triangle..=s.last_triangle).map(|i| triangles[i].color).collect();
        assert!(colors.len() >= 2, "{} couleurs distinctes attendues", colors.len());
    }

    #[test]
    fn eva_cosmonaut_y_is_flipped_and_scaled() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[0];

        // y retourné : la tête (y éditeur ≈ +8.5) passe en y négatif (haut
        // d'écran), les pieds (y ≈ -8.5) en y positif (bas d'écran)
        assert!(s.top_left.y < 0.0, "tête en haut : {}", s.top_left.y);
        assert!(s.bottom_right.y > 0.0, "pieds en bas : {}", s.bottom_right.y);
        // échelle appliquée : ~17 unités éditeur × 1,5 → ~26 unités monde
        assert!((s.height - COSMONAUTE_EVA_SCALE * 17.1).abs() < 1.0, "hauteur {}", s.height);
        // largeur ~13,2 unités éditeur × 1,5
        assert!(s.width > COSMONAUTE_EVA_SCALE * 12.0, "largeur {}", s.width);
        // NB : `target_center` est le centroïde des centres de triangles (pas
        // le centre géométrique — comportement du jeu, `compute_shape_center`) :
        // le cosmonaute se dessine autour de sa position, rotation nulle.
        assert!(s.target_center.x.abs() < 10.0, "centre x {}", s.target_center.x);
    }

    #[test]
    fn eva_cosmonaut_limbs_are_identified_with_their_pivot() {
        // les plans extérieurs du haut sont des bras, ceux du bas des jambes —
        // chacun avec son articulation (le reste du corps est fixe) : 32 bras
        // (2×16) + 20 jambes (2×10) + 39 fixes (buste/casque/visière)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[0];
        let mut arms = 0;
        let mut legs = 0;
        let mut fixed = 0;
        for i in s.first_triangle..=s.last_triangle {
            let t = &triangles[i];
            match t.limb {
                1 => {
                    arms += 1;
                    assert_ne!(t.pivot, Point::default(), "bras sans articulation");
                }
                2 => {
                    legs += 1;
                    assert_ne!(t.pivot, Point::default(), "jambe sans articulation");
                }
                _ => fixed += 1,
            }
        }
        assert_eq!(arms, 32);
        assert_eq!(legs, 20);
        assert_eq!(fixed, 39);
    }

    #[test]
    fn eva_cosmonaut_limbs_swing_during_thrust_and_settle() {
        // pendant la poussée, bras et jambes s'agitent (leurs sommets bougent
        // autour des articulations) ; le buste reste fixe ; quand la poussée
        // cesse, les membres retombent à la pose d'origine
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[0];
        let mut arm_tri = None;
        let mut leg_tri = None;
        let mut fixed_tri = None;
        for i in s.first_triangle..=s.last_triangle {
            match triangles[i].limb {
                1 if arm_tri.is_none() => arm_tri = Some(i),
                2 if leg_tri.is_none() => leg_tri = Some(i),
                0 if fixed_tri.is_none() => fixed_tri = Some(i),
                _ => {}
            }
        }
        let (arm, leg, fixed) = (arm_tri.unwrap(), leg_tri.unwrap(), fixed_tri.unwrap());
        let arm_rest = triangles[arm].center;
        let leg_rest = triangles[leg].center;
        let fixed_rest = triangles[fixed].a;

        // poussée : les membres s'agitent, le buste ne bouge pas
        let mut shape = shapes[0].clone();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        for _ in 0..120 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, true, t, dt);
        }
        assert!(
            (triangles[arm].center.x - arm_rest.x).abs() > 0.3
                || (triangles[arm].center.y - arm_rest.y).abs() > 0.3,
            "le bras doit s'agiter (centre {:?} → {:?})",
            arm_rest,
            triangles[arm].center
        );
        assert!(
            (triangles[leg].center.x - leg_rest.x).abs() > 0.3
                || (triangles[leg].center.y - leg_rest.y).abs() > 0.3,
            "la jambe doit s'agiter"
        );
        assert_eq!(triangles[fixed].a, fixed_rest, "le buste reste immobile");

        // repos : les membres retombent à la pose d'origine
        for _ in 0..600 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, t, dt);
        }
        assert!(
            (triangles[arm].center.x - arm_rest.x).abs() < 0.1
                && (triangles[arm].center.y - arm_rest.y).abs() < 0.1,
            "le bras doit revenir au repos ({:?})",
            triangles[arm].center
        );
        assert!(
            (triangles[leg].center.x - leg_rest.x).abs() < 0.1
                && (triangles[leg].center.y - leg_rest.y).abs() < 0.1,
            "la jambe doit revenir au repos"
        );
        assert_eq!(triangles[fixed].a, fixed_rest);
    }

    #[test]
    fn eva_cosmonaut_is_parked_off_screen_and_immobile() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[idx];

        // garé en bord de monde (hors écran au lancement) : ni visible ni
        // collidable — son seul objectif est de rejoindre la base une fois
        // éjecté
        assert_eq!(s.position, COSMONAUTE_EVA_PARK);
        assert!(!s.is_collider);
        assert!(!s.show_all_parts); // cullé tant qu'il est garé
        // immobile : vitesse, rotation et orientation nulles
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.orientation, 0.0);
    }
}
