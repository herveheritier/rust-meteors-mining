//! Portail de distorsion - chargement de `assets/portail.json`.
//!
//! Le portail (warp gate) est désormais un **mesh « meshes-designer »** comme
//! le vaisseau et le cosmonaute : un fichier JSON (liste de `planes` déjà
//! triangulés, `verts` + `faces` avec couleur RGBA) embarqué au compile via
//! `WARP_GATE_JSON` (src/marketplace.rs, fichier généré par l'outil de
//! gestion) et paramétré par son échelle (`WARP_GATE_SCALE`), son orientation
//! (`WARP_GATE_ORIENTATION_DEGREES`), son centre de rotation
//! (`WARP_GATE_CENTER_X/Y_PERCENT`) et sa composition de plans
//! (`WARP_GATE_PLANES`). Le mesh est converti en une `Shape` + ses `Triangle`
//! du modèle du jeu (`geom.rs`/`shape.rs`) exactement comme le cosmonaute
//! (`cosmonaut.rs`) : une `Triangle` par face, couleur RGBA → ARGB 32 bits,
//! axe y **retourné** (éditeur y↑ → jeu y↓), mise à l'échelle et rotation
//! autour du centre de rotation choisi.
//!
//! `build_warp_gate` construit le portail posé à `position` (statique : ni
//! vitesse ni rotation). `generate.rs` (`create_warp_gate`) le pose hors de
//! la vue à intervalle régulier ; `game.rs` s'occupe des interactions
//! (téléportation du vaisseau qui le traverse, rebond des météores).

use serde::Deserialize;

use crate::config::{argb32, TEXTURE_NONE, WHOIAM_WARP_GATE};
use crate::geom::{Point, Triangle};
use crate::marketplace::{
    WARP_GATE_CENTER_X_PERCENT, WARP_GATE_CENTER_Y_PERCENT, WARP_GATE_JSON,
    WARP_GATE_ORIENTATION_DEGREES, WARP_GATE_PLANES, WARP_GATE_SCALE,
};
use crate::shape::{compute_shape_center, free_shape, Shape};

/// Violet néon du portail (0xFFB04AFF) - repli pour une face sans champ
/// `color` dans le fichier (l'éditeur n'exporte pas de couleur pour les faces
/// non peintes) : sans elle, `serde` refuserait le fichier entier à la
/// première face sans couleur.
const DEFAULT_FACE_COLOR: [f32; 4] = [0.69, 0.29, 1.0, 1.0];

/// Racine du fichier « meshes-designer » - seuls les plans portent le mesh.
#[derive(Deserialize)]
struct WarpGateFile {
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
    /// Couleur RGBA de la face, flottants 0..1 - **optionnelle** : les faces
    /// non peintes de l'éditeur n'en portent pas, `DEFAULT_FACE_COLOR` est
    /// alors utilisée.
    #[serde(default = "default_face_color")]
    color: [f32; 4],
}

/// Valeur par défaut de `Face::color` (voir `DEFAULT_FACE_COLOR`).
fn default_face_color() -> [f32; 4] {
    DEFAULT_FACE_COLOR
}

/// RGBA (flottants 0..1) → ARGB 32 bits au format QB64 (AARRGGBB).
fn rgba_to_argb(rgba: [f32; 4]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    argb32(byte(rgba[3]), byte(rgba[0]), byte(rgba[1]), byte(rgba[2]))
}

/// Masque des plans construits (composition) : les plans listés dans
/// `WARP_GATE_PLANES` (indices dans le fichier mesh). **Liste vide = tous les
/// plans** (repli : composition non définie).
fn warp_gate_visible_mask(file: &WarpGateFile) -> Vec<bool> {
    if WARP_GATE_PLANES.is_empty() {
        vec![true; file.planes.len()]
    } else {
        (0..file.planes.len())
            .map(|i| WARP_GATE_PLANES.contains(&i))
            .collect()
    }
}

/// Construit le portail (warp gate) à partir du mesh `WARP_GATE_JSON` : une
/// `Triangle` par face du fichier, mise à l'échelle `WARP_GATE_SCALE`, tournée
/// de `−WARP_GATE_ORIENTATION_DEGREES` autour du pivot
/// `WARP_GATE_CENTER_X/Y_PERCENT` (position en **pourcentage de la boîte
/// englobante** du mesh, 50/50 = centre géométrique) et posée à `position`
/// (les sommets sont en coordonnées locales, la forme se dessine autour de sa
/// position). Le portail est **statique** (direction, vitesse, orientation et
/// rotation nulles - contrairement aux météores qui dérivent et tournent) et
/// **indestructible** (`is_collider`, rendu uni violet). Renvoie l'index de la
/// forme créée (réutilise une forme détruite au même nombre de triangles
/// quand c'est possible, comme `build_cosmonaut`).
pub fn build_warp_gate(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    position: Point,
) -> usize {
    let file: WarpGateFile =
        serde_json::from_str(WARP_GATE_JSON).expect("assets/portail.json : JSON invalide");
    // composition des plans (`WARP_GATE_PLANES`) : un plan exclu n'est ni
    // construit ni alloué
    let visible = warp_gate_visible_mask(&file);
    let nbr: usize = file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(_, p)| p.faces.len())
        .sum();

    // boîte englobante du mesh dans le repère de l'éditeur (y vers le haut) -
    // sert à situer le centre de rotation en pourcentage (même schéma que
    // `build_cosmonaut`)
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    for (i, plane) in file.planes.iter().enumerate() {
        if !visible[i] {
            continue;
        }
        for v in &plane.verts {
            minx = minx.min(v[0]);
            miny = miny.min(v[1]);
            maxx = maxx.max(v[0]);
            maxy = maxy.max(v[1]);
        }
    }
    // centre de rotation : pivot en % de la boîte englobante (50/50 = centre)
    let pivot_editor = Point::new(
        minx + WARP_GATE_CENTER_X_PERCENT / 100.0 * (maxx - minx),
        miny + WARP_GATE_CENTER_Y_PERCENT / 100.0 * (maxy - miny),
    );
    // orientation : angle de l'avant du mesh dans l'éditeur (degrés, sens
    // trigonométrique : 0 = à droite, +90 = en haut) - le mesh est tourné de
    // −orientation autour du pivot
    let angle = -WARP_GATE_ORIENTATION_DEGREES.to_radians();
    let (sin_a, cos_a) = angle.sin_cos();
    // sommet éditeur (y↑) → repère local du jeu (y↓) : rotation autour du
    // pivot, mise à l'échelle, axe y retourné
    let pt = |v: [f64; 2]| {
        let dx = v[0] - pivot_editor.x;
        let dy = v[1] - pivot_editor.y;
        Point::new(
            (pivot_editor.x + dx * cos_a - dy * sin_a) * WARP_GATE_SCALE,
            -(pivot_editor.y + dx * sin_a + dy * cos_a) * WARP_GATE_SCALE,
        )
    };

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue - même schéma que `build_cosmonaut`
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
    shape.who_i_am = WHOIAM_WARP_GATE;
    shape.is_collider = true; // les météores rebondissent dessus (`game.rs`)
    shape.texture = TEXTURE_NONE; // rendu en couleurs par face, sans texture
    shape.shape_color = 0xFFB04AFF; // violet néon - repli (chaque face a sa couleur)
    shape.position = position;
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;

    let first = shape.first_triangle;
    let mut k = 0usize;
    for plane in file.planes.iter().enumerate().filter(|(i, _)| visible[*i]).map(|(_, p)| p) {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            // rotation autour du pivot + axe y retourné (éditeur y↑ → jeu y↓)
            // + mise à l'échelle (closure `pt` définie plus haut)
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
    shape_index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WHOIAM_WARP_GATE;

    #[test]
    fn warp_gate_mesh_builds_a_live_large_static_portal() {
        // le portail est construit depuis `assets/portail.json` (mesh) : il
        // est **vivant** (autant de triangles que de faces), **agrandi** de
        // `WARP_GATE_SCALE` (rayon ≈ 30 × 3 = 90), **posé** à la position
        // demandée et **statique** (rien ne le fait dériver ni tourner)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = build_warp_gate(&mut shapes, &mut triangles, Point::new(320.0, 0.0));

        let s = &shapes[idx];
        assert_eq!(s.who_i_am, WHOIAM_WARP_GATE);
        assert!(s.is_collider, "le portail est un collider (les météores rebondissent dessus)");
        assert!(s.life > 0, "portail vivant depuis son mesh");
        assert_eq!(s.life as usize, triangles.len(), "une Triangle par face du mesh");
        assert_eq!(s.position, Point::new(320.0, 0.0));
        assert_eq!(s.direction, 0.0);
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        // dimension apparente : anneau r 30 (mesh) × `WARP_GATE_SCALE`
        // (échelle paramétrée dans l'outil de gestion) → rayon visuel
        // 30 × scale, vérifié sur la boîte englobante locale ; le champ
        // `radius` (métrique de collision) inclut la hauteur des triangles et
        // reste supérieur au rayon visuel
        let expected = 30.0 * WARP_GATE_SCALE;
        let half_w = (s.bottom_right.x - s.top_left.x) / 2.0;
        let half_h = (s.bottom_right.y - s.top_left.y) / 2.0;
        assert!(
            (half_w - expected).abs() < 5.0 && (half_h - expected).abs() < 5.0,
            "anneau r 30 × scale {} → demi-largeur ≈ {expected}, obtenu x {half_w} y {half_h}",
            WARP_GATE_SCALE
        );
        assert!(s.radius > half_w, "le rayon de collision englobe la hauteur des triangles");
        assert!(s.life > 0 && shapes[idx].texture == TEXTURE_NONE);
        // toutes les faces du mesh portent une couleur propre (le repli
        // `shape_color` n'est jamais utilisé)
        for t in &triangles {
            assert_ne!(t.color, 0, "chaque face du portail a sa couleur");
        }
    }
}