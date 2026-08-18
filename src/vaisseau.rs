//! Vaisseau joueur — chargement de `assets/vaisseau.json`.
//!
//! Même format « meshes-designer » que `assets/cosmonaute.json` (voir
//! `cosmonaut.rs`) : une liste de `planes`, chacun étant une région
//! polygonale **déjà triangulée** — `verts` = sommets `[x, y]`, `faces` =
//! triangles avec leurs indices dans `verts` et une couleur RGBA (flottants
//! 0..1) par face. Les autres champs du fichier (`zoom`, `cx`, `cy`,
//! `grid`…) sont de l'état d'éditeur, ignorés.
//!
//! Le mesh remplace l'ancien triangle texturé du joueur (`vaisseau.png`) :
//! une `Triangle` du modèle du jeu (`geom.rs`/`shape.rs`) par face via
//! `Triangle::create`, couleur RGBA → ARGB 32 bits (`argb32`), axe y
//! **retourné** (l'éditeur travaille y vers le haut, le jeu y vers le bas)
//! et mise à l'échelle (`VAISSEAU_SCALE`). Le nez du vaisseau (+x éditeur)
//! reste à droite : c'est l'orientation 0 du jeu, celle du départ à quai.
//! `create_player_vaisseau` est appelé par `generate::prepare` — `shapes`
//! est vide, le vaisseau prend l'index 0 (`PLAYER_INDEX`).

use serde::Deserialize;

use crate::config::{argb32, TEXTURE_NONE, WHOIAM_PLAYER};
use crate::geom::{Point, Triangle};
use crate::shape::{compute_shape_center, free_shape, Shape};

/// Le fichier est embarqué dans le binaire (`include_str!`), comme les
/// textures et les sons : pas d'accès au système de fichiers au runtime.
pub const VAISSEAU_JSON: &str = include_str!("../assets/vaisseau.json");

/// Échelle du vaisseau : le mesh fait ~20,6 × 17,9 unités éditeur — la
/// taille de l'ancien triangle joueur (20 × 20, rayon 10) : échelle 1,0.
pub const VAISSEAU_SCALE: f64 = 1.0;

/// Couleur par défaut d'une face **sans champ `color`** dans le fichier :
/// l'éditeur « meshes-designer » n'exporte pas de couleur pour les faces
/// non peintes — gris clair neutre, opaque (sinon `serde` refuserait le
/// fichier entier à la première face sans couleur).
const DEFAULT_FACE_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

/// Racine du fichier « meshes-designer » — seuls les plans portent le mesh.
#[derive(Deserialize)]
struct VaisseauFile {
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
    /// Couleur RGBA de la face, flottants 0..1 — **optionnelle** : les faces
    /// non peintes de l'éditeur n'en portent pas, `DEFAULT_FACE_COLOR` est
    /// alors utilisée (au lieu de faire échouer le chargement du fichier).
    #[serde(default = "default_face_color")]
    color: [f32; 4],
}

/// Valeur par défaut de `Face::color` (voir `DEFAULT_FACE_COLOR`).
fn default_face_color() -> [f32; 4] {
    DEFAULT_FACE_COLOR
}

/// Nombre de faces du fichier embarqué (une `Triangle` par face) — exposé
/// pour les tests d'invariant du monde (`generate::prepare` construit le
/// vaisseau + la station : le compte de triangles total en découle).
#[cfg(test)]
pub fn vaisseau_face_count() -> usize {
    let file: VaisseauFile =
        serde_json::from_str(VAISSEAU_JSON).expect("assets/vaisseau.json : JSON invalide");
    file.planes.iter().map(|p| p.faces.len()).sum()
}

/// RGBA (flottants 0..1) → ARGB 32 bits au format QB64 (AARRGGBB).
fn rgba_to_argb(rgba: [f32; 4]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    argb32(byte(rgba[3]), byte(rgba[0]), byte(rgba[1]), byte(rgba[2]))
}

/// Construit le vaisseau joueur à partir de `assets/vaisseau.json` : une
/// `Triangle` par face du fichier, à la suite des triangles existants,
/// mise à l'échelle `VAISSEAU_SCALE` et posée à l'origine (le vaisseau
/// démarre au centre de la station, position 0,0 — le monde n'est pas
/// encore initialisé). Renvoie l'index de la forme créée (réutilise une
/// forme détruite au même nombre de triangles quand c'est possible, comme
/// `meshes_to_shape` — ici `shapes` est vide, l'index est 0).
pub fn create_player_vaisseau(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) -> usize {
    let file: VaisseauFile =
        serde_json::from_str(VAISSEAU_JSON).expect("assets/vaisseau.json : JSON invalide");
    let nbr = file.planes.iter().map(|p| p.faces.len()).sum();

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue — même schéma que `build_cosmonaut`
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
    shape.who_i_am = WHOIAM_PLAYER;
    shape.is_collider = true;
    shape.show_all_parts = true; // toujours visible (pas de culling)
    shape.texture = TEXTURE_NONE; // rendu en couleurs par face, sans texture
    shape.shape_color = 0x80FFFFFF; // repli (jamais utilisé : chaque face a sa couleur)
    shape.position = Point::new(0.0, 0.0);
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;

    let first = shape.first_triangle;
    let mut k = 0usize;
    for plane in &file.planes {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            // axe y retourné (éditeur y↑ → jeu y↓) + mise à l'échelle — le
            // nez (+x éditeur) reste à droite : orientation 0 du vaisseau
            let pt = |v: [f64; 2]| Point::new(v[0] * VAISSEAU_SCALE, -v[1] * VAISSEAU_SCALE);
            let mut t = Triangle::default();
            t.create(pt(plane.verts[i]), pt(plane.verts[j]), pt(plane.verts[l]));
            t.color = rgba_to_argb(face.color);
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }
    debug_assert_eq!(k, nbr);

    compute_shape_center(shape, triangles);
    // centre fixe : `moving_shape` fait converger `center` vers
    // `target_center` (÷100 par frame) — posés égaux, le vaisseau reste
    // centré sur sa position (rotation autour du centre géométrique, pas de
    // dérive au démarrage).
    shape.center = shape.target_center;
    shape_index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PLAYER_INDEX;

    #[test]
    fn vaisseau_has_all_faces_with_their_color() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_player_vaisseau(&mut shapes, &mut triangles);

        // une Triangle vivante par face du fichier, dans une seule forme — le
        // compte est dérivé du fichier (l'éditeur peut le ré-exporter)
        let nbr = vaisseau_face_count();
        assert_eq!(shapes.len(), 1);
        assert_eq!(idx, PLAYER_INDEX);
        let s = &shapes[idx];
        assert_eq!(s.who_i_am, WHOIAM_PLAYER);
        assert_eq!(s.life as usize, nbr);
        assert_eq!(s.last_triangle - s.first_triangle + 1, nbr);
        assert!(s.is_collider);
        assert_eq!(s.texture, TEXTURE_NONE);
        for i in s.first_triangle..=s.last_triangle {
            let t = &triangles[i];
            assert_eq!(t.life, 1);
            assert_eq!(t.shape_index, idx as i32);
            assert_ne!(t.color, 0, "chaque face doit porter sa couleur");
        }
        // plusieurs couleurs distinctes (fuselage gris, verrière bleue,
        // ailerons/tuyère…)
        let colors: std::collections::HashSet<u32> =
            (s.first_triangle..=s.last_triangle).map(|i| triangles[i].color).collect();
        assert!(colors.len() >= 3, "{} couleurs distinctes attendues", colors.len());
    }

    #[test]
    fn vaisseau_y_is_flipped_and_nose_points_right() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_player_vaisseau(&mut shapes, &mut triangles);
        let s = &shapes[0];

        // axe y retourné : les ailerons du haut de l'éditeur (y > 0) passent
        // en y négatif (haut d'écran) — la boîte englobante couvre ±~8,9
        assert!((s.top_left.y + 8.9).abs() < 0.6, "haut : {}", s.top_left.y);
        assert!((s.bottom_right.y - 8.9).abs() < 0.6, "bas : {}", s.bottom_right.y);
        // le nez (+x éditeur) reste à droite : orientation 0 du vaisseau
        assert!(s.bottom_right.x > 9.0, "nez à droite : {}", s.bottom_right.x);
        assert!(s.top_left.x < -10.0, "tuyère à gauche : {}", s.top_left.x);
        // échelle appliquée : ~20,6 × 17,9 unités monde
        assert!((s.width - 20.6).abs() < 1.0, "largeur {}", s.width);
        assert!((s.height - 17.85).abs() < 1.0, "hauteur {}", s.height);
        // posé au centre de la station, immobile, centre fixé (pas de dérive)
        assert_eq!(s.position, Point::new(0.0, 0.0));
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.orientation, 0.0);
        assert_eq!(s.center, s.target_center);
    }

    #[test]
    fn face_without_color_uses_default_color() {
        // une face sans champ `color` (non peinte dans l'éditeur) est
        // acceptée et reçoit la couleur par défaut — plus d'erreur
        // « missing field `color` » au chargement
        let file: VaisseauFile = serde_json::from_str(
            r#"{"planes":[{"verts":[[0.0,0.0],[10.0,0.0],[0.0,10.0]],"faces":[{"v":[0,1,2]}]}]}"#,
        )
        .expect("une face sans couleur doit être acceptée");
        let face = &file.planes[0].faces[0];
        assert_eq!(face.v, [0, 1, 2]);
        assert_eq!(face.color, DEFAULT_FACE_COLOR);
        // la couleur par défaut est opaque (alpha 1) et non nulle une fois
        // convertie en ARGB
        assert_eq!(rgba_to_argb(face.color) >> 24, 0xFF);
        assert_ne!(rgba_to_argb(face.color) & 0xFFFFFF, 0);
    }

    #[test]
    fn face_with_color_still_parses() {
        // une face qui porte sa couleur reste lue telle quelle
        let file: VaisseauFile = serde_json::from_str(
            r#"{"planes":[{"verts":[[0.0,0.0],[10.0,0.0],[0.0,10.0]],"faces":[{"v":[0,1,2],"color":[0.9,0.1,0.2,1.0]}]}]}"#,
        )
        .expect("face avec couleur acceptée");
        let face = &file.planes[0].faces[0];
        assert_eq!(face.color, [0.9, 0.1, 0.2, 1.0]);
    }

    #[test]
    fn real_vaisseau_file_loads_with_defaults_for_unpainted_faces() {
        // le fichier embarqué (ré-exporté de l'éditeur) contient des faces
        // sans couleur : il doit se charger sans erreur, toutes les faces
        // vivantes portant une couleur (la leur ou la couleur par défaut)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_player_vaisseau(&mut shapes, &mut triangles);
        let s = &shapes[idx];
        assert_eq!(s.life as usize, vaisseau_face_count());
        for i in s.first_triangle..=s.last_triangle {
            assert_ne!(triangles[i].color, 0, "chaque face doit porter une couleur");
        }
    }
}
