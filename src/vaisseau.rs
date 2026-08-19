//! Vaisseau joueur — chargement du mesh choisi dans `src/marketplace.rs`.
//!
//! Le fichier mesh (format « meshes-designer », voir `cosmonaut.rs`) est
//! embarqué au compile via `include_str!` : le **chemin** est la constante
//! `VAISSEAU_JSON` de `src/marketplace.rs`, un fichier **généré** par l'outil
//! de gestion `tools/marketplace-editor/index.html` — c'est lui qui choisit
//! l'asset (`assets/*.json`), l'échelle (`VAISSEAU_SCALE`), l'orientation
//! (`VAISSEAU_ORIENTATION_DEGREES`) et le centre de rotation
//! (`VAISSEAU_CENTER_X/Y_PERCENT`).
//!
//! Le mesh est converti en une `Shape` + ses `Triangle` du modèle du jeu
//! (`geom.rs`/`shape.rs`) : une `Triangle` par face via `Triangle::create`,
//! couleur RGBA → ARGB 32 bits (`argb32`), axe y **retourné** (l'éditeur
//! travaille y vers le haut, le jeu y vers le bas), mise à l'échelle
//! (`VAISSEAU_SCALE`) et rotation autour du centre de rotation choisi : le
//! mesh est tourné de `−VAISSEAU_ORIENTATION_DEGREES` (angle du nez du mesh
//! dans l'éditeur) pour ramener le nez sur +x — l'orientation 0 du jeu, celle
//! du départ à quai. Le **centre de rotation** (pivot, en % de la boîte
//! englobante du mesh) devient le centre de la forme (`target_center`) : le
//! vaisseau pivote autour de ce point dans le jeu.
//! `create_player_vaisseau` est appelé par `generate::prepare` — `shapes`
//! est vide, le vaisseau prend l'index 0 (`PLAYER_INDEX`).

use serde::Deserialize;

use crate::config::{argb32, TEXTURE_NONE, WHOIAM_PLAYER};
use crate::geom::{Point, Triangle};
use crate::marketplace::{
    VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT, VAISSEAU_JSON,
    VAISSEAU_ORIENTATION_DEGREES, VAISSEAU_SCALE,
};
use crate::shape::{compute_shape_center, free_shape, Shape};

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
        serde_json::from_str(VAISSEAU_JSON).expect("mesh du vaisseau : JSON invalide");
    file.planes.iter().map(|p| p.faces.len()).sum()
}

/// RGBA (flottants 0..1) → ARGB 32 bits au format QB64 (AARRGGBB).
fn rgba_to_argb(rgba: [f32; 4]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    argb32(byte(rgba[3]), byte(rgba[0]), byte(rgba[1]), byte(rgba[2]))
}

/// Construit le vaisseau joueur à partir des réglages de `src/marketplace.rs`
/// (fichier généré par l'outil de gestion) : mesh `VAISSEAU_JSON`, échelle
/// `VAISSEAU_SCALE`, orientation `VAISSEAU_ORIENTATION_DEGREES` et centre de
/// rotation `VAISSEAU_CENTER_X/Y_PERCENT`. Voir `build_vaisseau`.
pub fn create_player_vaisseau(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) -> usize {
    build_vaisseau(
        shapes,
        triangles,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    )
}

/// Construit la forme « vaisseau » à partir du mesh `VAISSEAU_JSON` : une
/// `Triangle` par face du fichier, à la suite des triangles existants, mise à
/// l'échelle `scale`, tournée de `−orientation_degrees` autour du pivot
/// `center_percent` (position en **pourcentage de la boîte englobante** du
/// mesh, 50/50 = centre géométrique) et posée à l'origine (le vaisseau
/// démarre au centre de la station, position 0,0 — le monde n'est pas encore
/// initialisé). Le pivot devient le centre de la forme : le vaisseau pivote
/// autour de lui dans le jeu (rotation des triangles autour de
/// `shape.center`). Renvoie l'index de la forme créée (réutilise une forme
/// détruite au même nombre de triangles quand c'est possible, comme
/// `meshes_to_shape` — ici `shapes` est vide, l'index est 0).
fn build_vaisseau(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> usize {
    let file: VaisseauFile =
        serde_json::from_str(VAISSEAU_JSON).expect("mesh du vaisseau : JSON invalide");
    let nbr = file.planes.iter().map(|p| p.faces.len()).sum();

    // boîte englobante du mesh dans le repère de l'éditeur (y vers le haut) —
    // sert à situer le centre de rotation en pourcentage
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    for plane in &file.planes {
        for v in &plane.verts {
            minx = minx.min(v[0]);
            miny = miny.min(v[1]);
            maxx = maxx.max(v[0]);
            maxy = maxy.max(v[1]);
        }
    }
    // centre de rotation : pivot en % de la boîte englobante (50/50 = centre)
    let pivot_editor = Point::new(
        minx + center_percent.x / 100.0 * (maxx - minx),
        miny + center_percent.y / 100.0 * (maxy - miny),
    );
    // orientation : angle du nez du mesh dans l'éditeur (degrés, sens
    // trigonométrique : 0 = à droite, +90 = en haut) — le mesh est tourné de
    // −orientation autour du pivot pour ramener le nez sur +x (l'orientation 0
    // du jeu, celle du départ à quai)
    let angle = -orientation_degrees.to_radians();
    let (sin_a, cos_a) = angle.sin_cos();
    // sommet éditeur (y↑) → repère local du jeu (y↓) : rotation autour du
    // pivot, mise à l'échelle, axe y retourné
    let pt = |v: [f64; 2]| {
        let dx = v[0] - pivot_editor.x;
        let dy = v[1] - pivot_editor.y;
        Point::new(
            (pivot_editor.x + dx * cos_a - dy * sin_a) * scale,
            -(pivot_editor.y + dx * sin_a + dy * cos_a) * scale,
        )
    };
    // le pivot dans le repère local du jeu (le vaisseau pivote autour de lui)
    let pivot = Point::new(pivot_editor.x * scale, -pivot_editor.y * scale);

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
    // centre de rotation : le pivot choisi (src/marketplace.rs), pas le
    // centroïde des faces — le vaisseau pivote autour de ce point. `moving_shape`
    // fait converger `center` vers `target_center` (÷100 par frame) — posés
    // égaux, le vaisseau reste stable dès le départ (pas de dérive).
    shape.target_center = pivot;
    shape.center = pivot;
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

        // attentes dérivées du fichier embarqué et de l'échelle réglée dans
        // `src/marketplace.rs` (`VAISSEAU_SCALE` — modifiable via l'outil de
        // gestion) : le test vérifie les invariants (axe y retourné, nez à
        // droite, échelle appliquée), pas une valeur figée qui casserait à
        // chaque réglage de l'échelle.
        let file: VaisseauFile =
            serde_json::from_str(VAISSEAU_JSON).expect("mesh du vaisseau : JSON invalide");
        let mut minx = f64::INFINITY;
        let mut miny = f64::INFINITY;
        let mut maxx = f64::NEG_INFINITY;
        let mut maxy = f64::NEG_INFINITY;
        for pl in &file.planes {
            for v in &pl.verts {
                minx = minx.min(v[0]);
                miny = miny.min(v[1]);
                maxx = maxx.max(v[0]);
                maxy = maxy.max(v[1]);
            }
        }
        let scale = VAISSEAU_SCALE;
        let tol = 0.05 + 0.001 * (maxx - minx) * scale;
        // axe y retourné : le haut de l'éditeur (y > 0) passe en y négatif
        // (haut d'écran) — la boîte englobante couvre ±maxy × échelle
        assert!((s.top_left.y + maxy * scale).abs() < tol, "haut : {}", s.top_left.y);
        assert!((s.bottom_right.y - maxy * scale).abs() < tol, "bas : {}", s.bottom_right.y);
        // le nez (+x éditeur) reste à droite : orientation 0 du vaisseau
        assert!(
            (s.bottom_right.x - maxx * scale).abs() < tol,
            "nez à droite : {}",
            s.bottom_right.x
        );
        assert!(
            (s.top_left.x - minx * scale).abs() < tol,
            "tuyère à gauche : {}",
            s.top_left.x
        );
        // échelle appliquée : largeur/hauteur = bbox de l'éditeur × VAISSEAU_SCALE
        assert!(
            (s.width - (maxx - minx) * scale).abs() < tol,
            "largeur {:.2}",
            s.width
        );
        assert!(
            (s.height - (maxy - miny) * scale).abs() < tol,
            "hauteur {:.2}",
            s.height
        );
        // posé au centre de la station, immobile, centre fixé (pas de dérive)
        assert_eq!(s.position, Point::new(0.0, 0.0));
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.orientation, 0.0);
        assert_eq!(s.center, s.target_center);
    }

    #[test]
    fn vaisseau_scale_is_applied_to_the_mesh() {
        // échelle 50 % : moitié de la taille par défaut (le mesh ~20,6 × 17,9
        // unités éditeur devient ~10,3 × 8,9), le nez reste à droite
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(&mut shapes, &mut triangles, 0.5, 0.0, Point::new(50.0, 50.0));
        let s = &shapes[0];
        assert!((s.width - 10.3).abs() < 0.5, "largeur {:.2}", s.width);
        assert!((s.height - 8.95).abs() < 0.5, "hauteur {:.2}", s.height);
        assert!(s.bottom_right.x > 4.5, "nez à droite : {}", s.bottom_right.x);
    }

    #[test]
    fn vaisseau_orientation_rotates_the_mesh_around_the_pivot() {
        // orientation 90 = le nez du mesh est « vers le haut » dans l'éditeur :
        // le mesh est tourné de −90° autour du centre de rotation — le nez du
        // mesh actuel (qui pointe à droite, orientation réelle 0) passe donc
        // vers le bas (dans le repère du jeu) et la boîte pivote de 90°
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(&mut shapes, &mut triangles, 1.0, 90.0, Point::new(50.0, 50.0));
        let s = &shapes[0];
        // le nez (max +x éditeur) finit en +y jeu (bas d'écran)…
        assert!(s.bottom_right.y > 9.0, "nez en bas : {}", s.bottom_right.y);
        // …et la tuyère (min −x éditeur) en −y (haut d'écran) : hauteur ≈
        // l'ancienne largeur (~21), largeur ≈ l'ancienne hauteur (~17,8)
        assert!(s.top_left.y < -9.0, "tuyère en haut : {}", s.top_left.y);
        assert!(s.height > s.width, "boîte pivotée : {} > {}", s.height, s.width);
        assert!((s.height - 21.1).abs() < 1.0, "hauteur {:.2}", s.height);
        assert!((s.width - 17.85).abs() < 1.0, "largeur {:.2}", s.width);
        // le centre de rotation reste le centre de la boîte englobante
        let cx = (s.top_left.x + s.bottom_right.x) / 2.0;
        let cy = (s.top_left.y + s.bottom_right.y) / 2.0;
        assert!((s.target_center.x - cx).abs() < 1e-9, "centre x {}", s.target_center.x);
        assert!((s.target_center.y - cy).abs() < 1e-9, "centre y {}", s.target_center.y);
    }

    #[test]
    fn vaisseau_rotation_center_moves_with_the_bbox_percentage() {
        // centre de rotation 0 %/0 % = coin haut-gauche de la boîte englobante
        // (repère éditeur, y↑) : dans le jeu (y↓), le pivot est le coin
        // bas-gauche de la boîte — le vaisseau s'étend à droite et vers le bas
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(&mut shapes, &mut triangles, 1.0, 0.0, Point::new(0.0, 0.0));
        let s = &shapes[0];
        let expected = Point::new(s.top_left.x, s.bottom_right.y);
        assert!(
            (s.target_center.x - expected.x).abs() < 1e-9
                && (s.target_center.y - expected.y).abs() < 1e-9,
            "pivot {:?} attendu {:?}",
            s.target_center,
            expected
        );
        // centre 100 %/100 % = coin bas-droit de l'éditeur → haut-droit du jeu
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(&mut shapes, &mut triangles, 1.0, 0.0, Point::new(100.0, 100.0));
        let s = &shapes[0];
        let expected = Point::new(s.bottom_right.x, s.top_left.y);
        assert!(
            (s.target_center.x - expected.x).abs() < 1e-9
                && (s.target_center.y - expected.y).abs() < 1e-9,
            "pivot {:?} attendu {:?}",
            s.target_center,
            expected
        );
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
