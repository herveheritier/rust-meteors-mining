//! Cosmonaute - chargement de `assets/cosmonaute.json`.
//!
//! Ce fichier est une exportation de l'éditeur « meshes-designer » (le format
//! de mesh du projet d'origine) : une liste de `planes`, chacun étant une
//! région polygonale **déjà triangulée** - `verts` = sommets `[x, y]`,
//! `faces` = triangles avec leurs indices dans `verts` et une couleur RGBA
//! (flottants 0..1) par face. Les autres champs du fichier (`zoom`, `cx`,
//! `cy`, `grid`…) sont de l'état d'éditeur, ignorés.
//!
//! Le mesh est converti en une `Shape` + ses `Triangle` du modèle du jeu
//! (`geom.rs`/`shape.rs`) : une `Triangle` par face via `Triangle::create`,
//! couleur RGBA → ARGB 32 bits (`argb32`), axe y **retourné** (l'éditeur
//! travaille y vers le haut, le jeu y vers le bas), mise à l'échelle et
//! rotation autour du centre de rotation choisi - les constantes
//! `COSMONAUTE_EVA_SCALE`, `COSMONAUTE_ORIENTATION_DEGREES` et
//! `COSMONAUTE_CENTER_X/Y_PERCENT` de `src/marketplace.rs` (fichier généré
//! par l'outil de gestion). `create_eva_cosmonaut` construit le **pilote
//! éjecté** quand le vaisseau est détruit (taille vaisseau, ~26 unités) :
//! garé hors écran en bord de monde et téléporté par `game.rs`
//! (`activate_cosmonaut`/`rescue_cosmonaut`). Décoratif (`is_collider =
//! false`, jamais détruit), il n'est jamais affiché à côté de la base.

use serde::Deserialize;
use std::sync::OnceLock;

use crate::config::{argb32, TEXTURE_NONE, WHOIAM_COSMONAUT};
use crate::geom::{Point, Triangle};
use crate::marketplace::{
    COSMONAUTE_CENTER_X_PERCENT, COSMONAUTE_CENTER_Y_PERCENT, COSMONAUTE_EVA_SCALE,
    COSMONAUTE_JSON, COSMONAUTE_ORIENTATION_DEGREES, COSMONAUTE_PLANES, COSMONAUTE_THRUSTERS,
    CosmonautThruster,
};
use crate::shape::{compute_shape_center, free_shape, Shape};

/// Couleur par défaut d'une face **sans champ `color`** dans le fichier :
/// l'éditeur « meshes-designer » n'exporte pas de couleur pour les faces
/// non peintes - gris clair neutre, opaque (sinon `serde` refuserait le
/// fichier entier à la première face sans couleur).
const DEFAULT_FACE_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

/// Poste « garé » du cosmonaute EVA : en bord de monde (coin sud-ouest), loin
/// de la caméra de départ - invisible tant que le vaisseau n'est pas détruit
/// (`game.rs` le téléporte au crash, puis le ramène ici après le secours).
pub const COSMONAUTE_EVA_PARK: Point = Point::new(-1400.0, -1400.0);

/// Amplitude (radians) du balancement des **bras** pendant la poussée
/// (~26° de part et d'autre - un remuement énergique).
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
/// Inclinaison (radians) des membres quand le cosmonaute **tourne** sur
/// lui-même (réorientation, touches ←/→ en mode EVA) : les bras et les
/// jambes **basculent dans le sens du tour** (~20°, en plus du balancement
/// de poussée) tant que la rotation est demandée, puis retombent à la pose
/// d'origine quand elle s'arrête (voir `animate_eva_cosmonaut`).
const TURN_LEAN_ANGLE: f64 = 0.35;

/// Racine du fichier « meshes-designer » - seuls les plans portent le mesh.
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
    /// Couleur RGBA de la face, flottants 0..1 - **optionnelle** : les faces
    /// non peintes de l'éditeur n'en portent pas, `DEFAULT_FACE_COLOR` est
    /// alors utilisée (au lieu de faire échouer le chargement du fichier).
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
/// `COSMONAUTE_PLANES` (indices dans le fichier mesh). **Liste vide = tous
/// les plans** (repli : composition non définie). Un plan exclu n'est ni
/// construit ni animé (bras/jambes).
fn cosmonaut_visible_mask(file: &CosmonautFile) -> Vec<bool> {
    if COSMONAUTE_PLANES.is_empty() {
        vec![true; file.planes.len()]
    } else {
        (0..file.planes.len())
            .map(|i| COSMONAUTE_PLANES.contains(&i))
            .collect()
    }
}

/// Charge le fichier mesh embarqué (une seule fois, en cache `OnceLock`) -
/// même schéma que `vaisseau::vaisseau_file`.
fn cosmonaut_file() -> &'static CosmonautFile {
    static COSMONAUTE_FILE: OnceLock<CosmonautFile> = OnceLock::new();
    COSMONAUTE_FILE.get_or_init(|| {
        serde_json::from_str(COSMONAUTE_JSON).expect("assets/cosmonaute.json : JSON invalide")
    })
}

/// Boîte englobante de la composition (repère de l'éditeur, y vers le haut) :
/// `(minx, miny, maxx, maxy)` - sert à situer le centre de rotation et les
/// éjections de gaz en pourcentage (même schéma que `vaisseau::composition_bbox`).
fn cosmonaut_bbox(file: &CosmonautFile, visible: &[bool]) -> (f64, f64, f64, f64) {
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    for (i, plane) in file.planes.iter().enumerate() {
        if !visible.get(i).copied().unwrap_or(false) {
            continue;
        }
        for v in &plane.verts {
            minx = minx.min(v[0]);
            miny = miny.min(v[1]);
            maxx = maxx.max(v[0]);
            maxy = maxy.max(v[1]);
        }
    }
    (minx, miny, maxx, maxy)
}

/// Éjection de gaz du cosmonaute EVA (`COSMONAUTE_THRUSTERS`) : chaque
/// propulseur + son **point local** sur le cosmonaute - sa `position` (en %
/// de la boîte englobante de la composition) convertie par la même
/// transformation que les sommets (pivot en %, rotation `−orientation`,
/// échelle, axe y retourné - voir `build_cosmonaut`). Le gaz sort de ce
/// point, tourné avec la figure (src/main.rs). **Liste vide = repli** : pas
/// de propulseur configuré, la flamme classique sur le dos
/// (`render::draw_cosmonaut_thruster`).
pub fn cosmonaut_thrusters() -> Vec<(CosmonautThruster, Point)> {
    cosmonaut_thrusters_with(
        COSMONAUTE_THRUSTERS,
        COSMONAUTE_EVA_SCALE,
        COSMONAUTE_ORIENTATION_DEGREES,
        Point::new(COSMONAUTE_CENTER_X_PERCENT, COSMONAUTE_CENTER_Y_PERCENT),
    )
}

/// Variante pure de `cosmonaut_thrusters` (tests) : propulseur + point local
/// de sa position pour des réglages **explicites** (échelle, orientation,
/// centre de rotation en % de la boîte englobante). Liste vide → aucune.
fn cosmonaut_thrusters_with(
    thrusters: &[CosmonautThruster],
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> Vec<(CosmonautThruster, Point)> {
    if thrusters.is_empty() {
        return Vec::new();
    }
    let file = cosmonaut_file();
    let visible = cosmonaut_visible_mask(file);
    let (minx, miny, maxx, maxy) = cosmonaut_bbox(file, &visible);
    // centre de rotation : pivot en % de la boîte englobante (50/50 = centre)
    let pivot_editor = Point::new(
        minx + center_percent.x / 100.0 * (maxx - minx),
        miny + center_percent.y / 100.0 * (maxy - miny),
    );
    // orientation : angle de l'avant du mesh dans l'éditeur (degrés, sens
    // trigonométrique : 0 = à droite, +90 = en haut) - le mesh est tourné de
    // −orientation autour du pivot pour ramener l'avant sur +x (l'orientation
    // 0 du jeu, celle du départ)
    let angle = -orientation_degrees.to_radians();
    let (sin_a, cos_a) = angle.sin_cos();
    // sommet éditeur (y↑) → repère local du jeu (y↓) : rotation autour du
    // pivot, mise à l'échelle, axe y retourné (même `pt` que `build_cosmonaut`)
    let pt = |v: [f64; 2]| {
        let dx = v[0] - pivot_editor.x;
        let dy = v[1] - pivot_editor.y;
        Point::new(
            (pivot_editor.x + dx * cos_a - dy * sin_a) * scale,
            -(pivot_editor.y + dx * sin_a + dy * cos_a) * scale,
        )
    };
    thrusters
        .iter()
        .map(|t| {
            let (x, y) = t.position;
            let ex = minx + x / 100.0 * (maxx - minx);
            let ey = miny + y / 100.0 * (maxy - miny);
            (*t, pt([ex, ey]))
        })
        .collect()
}

/// Construit le cosmonaute EVA - le pilote contrôlé quand le vaisseau est
/// détruit (`game.rs`) : petit, garé hors écran jusqu'à l'éjection. Garé, il
/// est cullé (hors limites de dessin) ; une fois éjecté, la caméra le suit
/// donc il est toujours affiché.
pub fn create_eva_cosmonaut(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) -> usize {
    build_cosmonaut(
        shapes,
        triangles,
        COSMONAUTE_EVA_SCALE,
        COSMONAUTE_ORIENTATION_DEGREES,
        Point::new(COSMONAUTE_CENTER_X_PERCENT, COSMONAUTE_CENTER_Y_PERCENT),
        COSMONAUTE_EVA_PARK,
    )
}

/// Construit une forme « cosmonaute » à partir du mesh `COSMONAUTE_JSON` :
/// une `Triangle` par face du fichier, à la suite des triangles existants,
/// mise à l'échelle `scale`, tournée de `−orientation_degrees` autour du
/// pivot `center_percent` (position en **pourcentage de la boîte englobante**
/// du mesh, 50/50 = centre géométrique) et posée à `position` (les sommets
/// sont en coordonnées locales, la forme se dessine autour de sa position).
/// Garé, il n'est pas dessiné (culling, `show_all_parts = false`) ; éjecté,
/// la caméra le suit donc il est toujours affiché. Renvoie l'index de la
/// forme créée (réutilise une forme détruite au même nombre de triangles
/// quand c'est possible, comme `meshes_to_shape`).
fn build_cosmonaut(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
    position: Point,
) -> usize {
    let file = cosmonaut_file();
    // composition des plans (`COSMONAUTE_PLANES`) : un plan exclu n'est ni
    // construit ni animé - la boîte englobante (centre de rotation) et
    // l'allocation ne portent que sur les plans retenus
    let visible = cosmonaut_visible_mask(file);
    let nbr: usize = file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(_, p)| p.faces.len())
        .sum();

    // boîte englobante du mesh dans le repère de l'éditeur (y vers le haut) -
    // sert à situer le centre de rotation en pourcentage (même schéma que
    // `build_vaisseau`)
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
        minx + center_percent.x / 100.0 * (maxx - minx),
        miny + center_percent.y / 100.0 * (maxy - miny),
    );
    // orientation : angle de l'avant du mesh dans l'éditeur (degrés, sens
    // trigonométrique : 0 = à droite, +90 = en haut) - le mesh est tourné de
    // −orientation autour du pivot pour ramener l'avant sur +x (l'orientation
    // 0 du jeu, celle du départ)
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
    // le pivot dans le repère local du jeu (le cosmonaute pivote autour de lui)
    let pivot = Point::new(pivot_editor.x * scale, -pivot_editor.y * scale);

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue - même schéma que `meshes_to_shape`
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
    // les **jambes** les plans du bas (y < 0 - sous le buste) ; l'articulation
    // de chaque membre est son sommet le plus proche du centre du personnage
    // (l'épaule, la hanche). Servira à l'animation de la poussée
    // (`animate_eva_cosmonaut` : bras et jambes qui s'agitent).
    let mut total = (0.0, 0.0);
    let mut verts_count = 0usize;
    for (i, plane) in file.planes.iter().enumerate() {
        if !visible[i] {
            continue;
        }
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
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(_, plane)| {
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
                    if best.is_none_or(|(_, bd)| d < bd) {
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
    for (plane, &(limb, pivot)) in file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| visible[*i])
        .map(|(_, p)| p)
        .zip(limbs.iter())
    {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            // rotation autour du pivot + axe y retourné (éditeur y↑ → jeu y↓)
            // + mise à l'échelle (closure `pt` définie plus haut)
            let mut t = Triangle::default();
            t.create(pt(plane.verts[i]), pt(plane.verts[j]), pt(plane.verts[l]));
            t.color = rgba_to_argb(face.color);
            // membre + articulation (repère local, même transformation que
            // les sommets : la rotation est appliquée au pivot aussi)
            t.limb = limb;
            t.pivot = pt([pivot.x, pivot.y]);
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }
    debug_assert_eq!(k, nbr);

    compute_shape_center(shape, triangles);
    // centre de rotation : le pivot choisi (src/marketplace.rs), pas le
    // centroïde des faces - le cosmonaute pivote autour de ce point dans le
    // jeu (tours pendant la poussée). `moving_shape` fait converger `center`
    // vers `target_center` (÷100 par frame) - posés égaux, le cosmonaute
    // reste stable dès le départ (pas de dérive).
    shape.target_center = pivot;
    shape.center = pivot;
    shape_index
}

/// Anime les membres du cosmonaute EVA : les **bras et les jambes s'agitent**
/// (bascule de leurs triangles autour de leurs articulations, `Triangle.limb`/
/// `pivot`) tant qu'il pousse et, quand il **se réoriente** (`turn` ≠ 0),
/// **basculent dans le sens du tour** - l'inclinaison de réorientation
/// (`TURN_LEAN_ANGLE` × le sens) s'ajoute à l'oscillation de poussée dans la
/// cible poursuivie. Quand la poussée cesse et/ou que la rotation s'arrête,
/// les membres **retombent au repos** (`Shape.anim_angle` rattrape la cible :
/// la pose s'installe à la montée, retombe doucement à l'arrêt).
/// `turn` est le sens de rotation demandé (-1, 0, +1 : +1 = orientation
/// croissante, touche →), `time` l'horloge (ex `get_time()`), `dt` le pas de
/// la frame ; les sommets locaux sont tournés en place, `moving_shape`
/// recalcule les positions réelles dans la foulée. Sans effet (ni coût) une
/// fois au repos.
pub fn animate_eva_cosmonaut(
    shape: &mut Shape,
    triangles: &mut [Triangle],
    thrusting: bool,
    turn: i32,
    time: f64,
    dt: f64,
) {
    // cibles cumulées : inclinaison de réorientation (constante, dans le sens
    // du tour) + oscillation de la poussée ; 0 (repos) quand rien n'est demandé
    let lean = TURN_LEAN_ANGLE * turn as f64;
    let swing = if thrusting { SWING_ARMS * (time * SWING_OMEGA).sin() } else { 0.0 };
    let target = lean + swing;
    // rattrapage lissé : la pose s'installe à la poussée et retombe au repos
    let step = (target - shape.anim_angle) * (1.0 - (-dt * SWING_CHASE).exp());
    shape.anim_angle += step;
    if step.abs() < 1e-9 {
        return;
    }
    for t in &mut triangles[shape.first_triangle..=shape.last_triangle] {
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
        for t in &triangles[s.first_triangle..=s.last_triangle] {
            assert_eq!(t.life, 1);
            assert_eq!(t.shape_index, idx as i32);
            assert_ne!(t.color, 0, "chaque face doit porter sa couleur");
        }
        // les couleurs du fichier : la combinaison claire et la visière sombre
        let colors: std::collections::HashSet<u32> =
            triangles[s.first_triangle..=s.last_triangle].iter().map(|t| t.color).collect();
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
        // le centre géométrique - comportement du jeu, `compute_shape_center`) :
        // le cosmonaute se dessine autour de sa position, rotation nulle.
        assert!(s.target_center.x.abs() < 10.0, "centre x {}", s.target_center.x);
    }

    #[test]
    fn eva_cosmonaut_limbs_are_identified_with_their_pivot() {
        // les plans extérieurs du haut sont des bras, ceux du bas des jambes -
        // chacun avec son articulation (le reste du corps est fixe) : 32 bras
        // (2×16) + 20 jambes (2×10) + 39 fixes (buste/casque/visière)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[0];
        let mut arms = 0;
        let mut legs = 0;
        let mut fixed = 0;
        for t in &triangles[s.first_triangle..=s.last_triangle] {
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
        for (i, t) in triangles[s.first_triangle..=s.last_triangle].iter().enumerate() {
            let i = s.first_triangle + i;
            match t.limb {
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
            animate_eva_cosmonaut(&mut shape, &mut triangles, true, 0, t, dt);
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
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, 0, t, dt);
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
    fn eva_cosmonaut_limbs_lean_into_the_turn_and_settle() {
        // réorientation (←/→, `turn` ≠ 0) : les membres **basculent dans le
        // sens du tour** (un bras pivote autour de son articulation : tour +1 →
        // bascule +, tour -1 → bascule opposée), puis retombent à la pose
        // d'origine quand la rotation s'arrête - le buste reste fixe
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[0];
        let mut arm_tri = None;
        let mut fixed_tri = None;
        for (i, t) in triangles[s.first_triangle..=s.last_triangle].iter().enumerate() {
            let i = s.first_triangle + i;
            match t.limb {
                1 if arm_tri.is_none() => arm_tri = Some(i),
                0 if fixed_tri.is_none() => fixed_tri = Some(i),
                _ => {}
            }
        }
        let (arm, fixed) = (arm_tri.unwrap(), fixed_tri.unwrap());
        let arm_rest = triangles[arm].center;
        let pivot = triangles[arm].pivot;
        let fixed_rest = triangles[fixed].a;
        let dt = 1.0 / 60.0;
        // bras au repos : un vecteur non nul depuis son articulation (l'angle
        // de bascule se lit par le produit vectoriel v_repos × v_actuel)
        let v_rest = Point::new(arm_rest.x - pivot.x, arm_rest.y - pivot.y);
        assert!(v_rest.x.abs() + v_rest.y.abs() > 1.0, "bras étendu : {:?}", v_rest);
        let cross = |p: Point| v_rest.x * (p.y - pivot.y) - v_rest.y * (p.x - pivot.x);
        let lean = |v: Point| (v.x - pivot.x).hypot(v.y - pivot.y) > 0.5;

        // tour +1 (touche →) : le bras bascule dans le sens + autour de son
        // articulation (produit vectoriel > 0) et le buste reste immobile
        let mut shape = shapes[0].clone();
        let mut t = 0.0;
        for _ in 0..240 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, 1, t, dt);
        }
        let p_pos = triangles[arm].center;
        assert!(lean(p_pos) && cross(p_pos) > 0.5, "tour +1 : bascule + ({:?})", p_pos);
        assert_eq!(triangles[fixed].a, fixed_rest, "le buste reste immobile");

        // la rotation s'arrête : les membres reviennent à la pose d'origine
        for _ in 0..300 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, 0, t, dt);
        }
        assert!(
            (triangles[arm].center.x - arm_rest.x).abs() < 0.1
                && (triangles[arm].center.y - arm_rest.y).abs() < 0.1,
            "le bras doit revenir au repos après le tour ({:?})",
            triangles[arm].center
        );
        assert_eq!(triangles[fixed].a, fixed_rest);

        // tour -1 (touche ←) : bascule opposée (produit vectoriel < 0)
        let mut shape = shapes[0].clone();
        for _ in 0..240 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, -1, t, dt);
        }
        let p_neg = triangles[arm].center;
        assert!(lean(p_neg) && cross(p_neg) < -0.5, "tour -1 : bascule - ({:?})", p_neg);
        assert_eq!(triangles[fixed].a, fixed_rest);

        // arrêt : retour à la pose d'origine (mêmes membres, même buste)
        for _ in 0..300 {
            t += dt;
            animate_eva_cosmonaut(&mut shape, &mut triangles, false, 0, t, dt);
        }
        assert!(
            (triangles[arm].center.x - arm_rest.x).abs() < 0.1
                && (triangles[arm].center.y - arm_rest.y).abs() < 0.1,
            "le bras doit revenir au repos après le tour inverse ({:?})",
            triangles[arm].center
        );
        assert_eq!(triangles[fixed].a, fixed_rest);
    }

    #[test]
    fn eva_cosmonaut_orientation_rotates_the_mesh_around_the_pivot() {
        // orientation 90 = l'avant du mesh est « vers le haut » dans
        // l'éditeur : le mesh est tourné de −90° autour du centre de rotation
        // - le personnage (debout dans l'éditeur) se couche sur le côté et la
        // boîte pivote de 90° (la largeur ≈ l'ancienne hauteur, ~17,1)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_cosmonaut(&mut shapes, &mut triangles, 1.0, 90.0, Point::new(50.0, 50.0), Point::default());
        let s = &shapes[0];
        assert!(s.width > s.height, "boîte pivotée : {} > {}", s.width, s.height);
        // le centre de rotation reste le centre de la boîte englobante
        let cx = (s.top_left.x + s.bottom_right.x) / 2.0;
        let cy = (s.top_left.y + s.bottom_right.y) / 2.0;
        assert!((s.target_center.x - cx).abs() < 1e-9, "centre x {}", s.target_center.x);
        assert!((s.target_center.y - cy).abs() < 1e-9, "centre y {}", s.target_center.y);
    }

    #[test]
    fn eva_cosmonaut_rotation_center_moves_with_the_bbox_percentage() {
        // centre de rotation 0 %/0 % = coin haut-gauche de la boîte englobante
        // (repère éditeur, y↑) : dans le jeu (y↓), le pivot est le coin
        // bas-gauche de la boîte - le cosmonaute s'étend à droite et vers le bas
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_cosmonaut(&mut shapes, &mut triangles, 1.0, 0.0, Point::new(0.0, 0.0), Point::default());
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
        build_cosmonaut(&mut shapes, &mut triangles, 1.0, 0.0, Point::new(100.0, 100.0), Point::default());
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
    fn eva_cosmonaut_is_parked_off_screen_and_immobile() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_eva_cosmonaut(&mut shapes, &mut triangles);
        let s = &shapes[idx];

        // garé en bord de monde (hors écran au lancement) : ni visible ni
        // collidable - son seul objectif est de rejoindre la base une fois
        // éjecté
        assert_eq!(s.position, COSMONAUTE_EVA_PARK);
        assert!(!s.is_collider);
        assert!(!s.show_all_parts); // cullé tant qu'il est garé
        // immobile : vitesse, rotation et orientation nulles
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.orientation, 0.0);
    }

    #[test]
    fn cosmonaut_thrusters_map_percent_to_local_points() {
        // la position d'un propulseur (en % de la boîte englobante, repère
        // éditeur y↑) est convertie par la **même transformation que les
        // sommets du mesh** (rotation autour du centre de rotation, mise à
        // l'échelle, axe y retourné - `build_cosmonaut`) : le gaz sort du
        // point local visé, tourné avec la figure. Testé sur des réglages
        // **explicites** (données synthétiques) pour rester valable quel que
        // soit le réglage enregistré depuis l'outil.
        let mk = |position: (f64, f64)| CosmonautThruster {
            name: "DOS",
            position,
            ejection_angle_degrees: 180.0,
            color: 0xFFFF9020,
        };
        let file = cosmonaut_file();
        let visible = cosmonaut_visible_mask(file);
        let (minx, miny, maxx, maxy) = cosmonaut_bbox(file, &visible);
        // échelle 1, orientation 0, pivot au coin 0 %/0 % : pt = identité +
        // axe y retourné. 0 %/0 % (coin haut-gauche de l'éditeur) → (minx, -miny)
        let pts = cosmonaut_thrusters_with(&[mk((0.0, 0.0))], 1.0, 0.0, Point::new(0.0, 0.0));
        assert_eq!(pts.len(), 1);
        let p = pts[0].1;
        assert!(
            (p.x - minx).abs() < 1e-9 && (p.y + miny).abs() < 1e-9,
            "0 %/0 % → {:?} attendu ({}, {})",
            p,
            minx,
            -miny
        );
        // 100 %/100 % (coin bas-droit de l'éditeur) → (maxx, -maxy)
        let pts = cosmonaut_thrusters_with(&[mk((100.0, 100.0))], 1.0, 0.0, Point::new(0.0, 0.0));
        let p = pts[0].1;
        assert!(
            (p.x - maxx).abs() < 1e-9 && (p.y + maxy).abs() < 1e-9,
            "100 %/100 % → {:?} attendu ({}, {})",
            p,
            maxx,
            -maxy
        );
        // rotation autour du pivot : orientation 90 (l'avant du mesh vers le
        // haut est tourné de −90°). Le bord droit (100 %/50 %) passe au-dessus
        // du pivot : point (cx, w/2 − cy) avec w = largeur de la boîte.
        let (cx, cy, w) = (
            minx + (maxx - minx) / 2.0,
            miny + (maxy - miny) / 2.0,
            maxx - minx,
        );
        let pts = cosmonaut_thrusters_with(&[mk((100.0, 50.0))], 1.0, 90.0, Point::new(50.0, 50.0));
        let p = pts[0].1;
        assert!(
            (p.x - cx).abs() < 1e-9 && (p.y - (w / 2.0 - cy)).abs() < 1e-9,
            "rotation 90° → {:?} attendu ({}, {})",
            p,
            cx,
            w / 2.0 - cy
        );
        // l'échelle s'applique au point comme aux sommets : au centre
        // (50/50) avec pivot (50/50), le point est le pivot local du mesh
        // (centre de rotation), quelle que soit l'orientation
        let pts = cosmonaut_thrusters_with(
            &[mk((50.0, 50.0))],
            COSMONAUTE_EVA_SCALE,
            30.0,
            Point::new(50.0, 50.0),
        );
        let p = pts[0].1;
        assert!(
            (p.x - cx * COSMONAUTE_EVA_SCALE).abs() < 1e-9
                && (p.y + cy * COSMONAUTE_EVA_SCALE).abs() < 1e-9,
            "centre → {:?}",
            p
        );
        // liste vide = repli (aucun propulseur)
        assert!(cosmonaut_thrusters_with(&[], 1.0, 0.0, Point::new(0.0, 0.0)).is_empty());
        // données réelles (`COSMONAUTE_THRUSTERS`) : un point par propulseur
        // configuré (vide si la liste est vide) - indépendant des valeurs
        assert_eq!(cosmonaut_thrusters().len(), COSMONAUTE_THRUSTERS.len());
    }
}
