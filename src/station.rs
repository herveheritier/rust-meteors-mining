//! Station - chargement de `assets/anneauStation.json`.
//!
//! Comme le vaisseau (`vaisseau.rs`), le cosmonaute (`cosmonaut.rs`) et le
//! portail (`warp_gate.rs`), la base est désormais un **mesh
//! « meshes-designer »** : un fichier JSON (liste de `planes` déjà triangulés,
//! `verts` + `faces`, sans couleur - l'anneau n'est pas peint dans l'éditeur)
//! embarqué au compile via `include_str!`. L'anneau y a été tracé sur
//! `station.png` avec les proportions du jeu : le mesh est mis à l'échelle
//! pour que son bord extérieur atteigne `STATION_OUTER_RADIUS` (~162, comme
//! l'ex-`STATION_MESH` de `shape.rs` - le bord intérieur, ~110, suit) puis
//! converti en une `Shape` + ses `Triangle` du modèle du jeu
//! (`geom.rs`/`shape.rs`) : une `Triangle` par face, axe y **retourné**
//! (éditeur y↑ → jeu y↓).
//!
//! Le mesh ne porte que la **géométrie** : le rendu est inchangé - anneau
//! texturé `station.png` dans le style TEXTURED (mapping radial de
//! `draw_textured_triangle`, teinte brûlée et fissures de dégâts), opacité
//! décroissante dans COLORED, arêtes rougies dans MESH. Les faces ne portent
//! pas de couleur (`t.color` reste 0) : le rendu lit la texture du style
//! TEXTURED, sinon la couleur de la forme (`shape.shape_color`).
//!
//! `build_station` construit la station posée au centre du monde (statique :
//! ni vitesse ni rotation). `generate.rs` (`create_station`) l'appelle
//! pendant `prepare` ; les météores l'endommagent triangle par triangle
//! (`game.rs`).

use serde::Deserialize;

use crate::config::{TEXTURE_STATION, WHOIAM_STATION};
use crate::geom::{Point, Triangle};
use crate::shape::{Shape, compute_shape_center, free_shape};

/// Rayon extérieur de l'anneau de la station (unités monde) : le mesh de
/// l'asset est mis à l'échelle pour que son bord extérieur atteigne cette
/// valeur - celle de l'ex-`STATION_MESH` de `shape.rs` (le bord intérieur,
/// ~110, suit : l'anneau a été tracé dans l'éditeur sur `station.png` avec
/// les mêmes proportions). Le mapping radial du rendu texturé
/// (`draw_textured_triangle`, bande `STATION_UV_R_INNER`..`STATION_UV_R_OUTER`)
/// et les constantes d'accostage (`STATION_INNER_RADIUS`) restent inchangés.
const STATION_OUTER_RADIUS: f64 = 162.0;

/// Fichier mesh embarqué au compile (`include_str!`) : chemin relatif à la
/// racine du projet. (L'outil de gestion n'a pas encore de carte « Station » :
/// la constante vit ici, pas dans `src/marketplace.rs`.)
const STATION_JSON: &str = include_str!("../assets/anneauStation.json");

/// Racine du fichier « meshes-designer » - seuls les plans portent le mesh.
#[derive(Deserialize)]
struct StationFile {
    planes: Vec<Plane>,
}

#[derive(Deserialize)]
struct Plane {
    /// Sommets du plan, en coordonnées de l'éditeur (y vers le haut).
    verts: Vec<[f64; 2]>,
    /// Triangles du plan, indices dans `verts`. Les faces de l'anneau ne sont
    /// pas peintes dans l'éditeur : elles ne portent pas de champ `color`
    /// (ignoré s'il apparaît) - le rendu texturé ne lit que la géométrie.
    faces: Vec<Face>,
}

#[derive(Deserialize)]
struct Face {
    /// Indices des 3 sommets de la face dans `verts`.
    v: [usize; 3],
}

/// Construit la station à partir du mesh `STATION_JSON` : une `Triangle` par
/// face du fichier, mise à l'échelle pour couvrir l'anneau du jeu (bord
/// extérieur `STATION_OUTER_RADIUS`), axe y retourné (éditeur y↑ → jeu y↓) et
/// posée au centre du monde (les sommets sont en coordonnées locales, la forme
/// se dessine autour de sa position). La station est **statique** (direction,
/// vitesse, orientation et rotation nulles) et reste rendue par le chemin
/// texturé de l'ex-`STATION_MESH` (`TEXTURE_STATION`) : seule la géométrie
/// vient de l'asset. Renvoie l'index de la forme créée (réutilise une forme
/// détruite au même nombre de triangles quand c'est possible, comme
/// `warp_gate::build_warp_gate`).
pub fn build_station(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) -> usize {
    let file: StationFile =
        serde_json::from_str(STATION_JSON).expect("assets/anneauStation.json : JSON invalide");
    let nbr: usize = file.planes.iter().map(|p| p.faces.len()).sum();

    // boîte englobante du mesh dans le repère de l'éditeur (y vers le haut) -
    // son centre sert de pivot (l'anneau est symétrique : proche de (0,0))
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for plane in &file.planes {
        for v in &plane.verts {
            minx = minx.min(v[0]);
            miny = miny.min(v[1]);
            maxx = maxx.max(v[0]);
            maxy = maxy.max(v[1]);
        }
    }
    let center_editor = Point::new((minx + maxx) / 2.0, (miny + maxy) / 2.0);
    // échelle : le bord extérieur du mesh (plus grand rayon depuis le centre)
    // doit atteindre `STATION_OUTER_RADIUS` - le même anneau que l'ex-
    // `STATION_MESH`, le bord intérieur (~110) suit proportionnellement
    let outer_editor = file
        .planes
        .iter()
        .flat_map(|p| &p.verts)
        .map(|v| (v[0] - center_editor.x).hypot(v[1] - center_editor.y))
        .fold(0.0, f64::max);
    let scale = STATION_OUTER_RADIUS / outer_editor;
    // sommet éditeur (y↑) → repère local du jeu (y↓) : centré puis à l'échelle
    let pt = |v: [f64; 2]| {
        Point::new(
            (v[0] - center_editor.x) * scale,
            -(v[1] - center_editor.y) * scale,
        )
    };

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
    shape.who_i_am = WHOIAM_STATION;
    shape.is_collider = true;
    shape.shape_color = 0xFF808000;
    // rendu inchangé : anneau texturé `station.png` (mapping radial du style
    // TEXTURED) - le mesh de l'asset ne porte que la géométrie
    shape.texture = TEXTURE_STATION;
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
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }
    debug_assert_eq!(k, nbr);

    // centre cible, rayon et boîte englobante depuis les triangles vivants
    // (centre (0,0), rayon ~162 - la géométrie réelle décide des collisions,
    // comme l'ex-`create_station` : pas de rayon forcé)
    compute_shape_center(shape, triangles);
    shape_index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_builds_ring_from_anneau_asset() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = build_station(&mut shapes, &mut triangles);
        assert_eq!(idx, 0);
        let shape = &shapes[idx];
        assert_eq!(shape.who_i_am, WHOIAM_STATION);
        assert_eq!(shape.texture, TEXTURE_STATION);
        assert!(shape.is_collider);
        assert_eq!(shape.position, Point::new(0.0, 0.0));
        // une Triangle par face du fichier, toutes vivantes, ids séquentiels
        assert_eq!(
            shape.life as usize,
            shape.last_triangle - shape.first_triangle + 1
        );
        for (k, t) in triangles[shape.first_triangle..=shape.last_triangle]
            .iter()
            .enumerate()
        {
            assert_eq!(t.life, 1, "triangle {k} mort");
            assert_eq!(t.id as usize, shape.first_triangle + k);
            assert_eq!(t.shape_index, idx as i32);
            assert_eq!(t.color, 0); // géométrie seule : le rendu reste texturé
        }
        // anneau autour de l'origine : bord intérieur ~110, extérieur ~162
        // (unités monde, comme l'ex-STATION_MESH)
        let (mut min_r, mut max_r): (f64, f64) = (f64::MAX, 0.0);
        for t in &triangles[shape.first_triangle..=shape.last_triangle] {
            for p in [&t.a, &t.b, &t.c] {
                min_r = min_r.min(p.x.hypot(p.y));
                max_r = max_r.max(p.x.hypot(p.y));
            }
        }
        assert!(
            (100.0..115.0).contains(&min_r),
            "bord intérieur {min_r} hors de l'anneau"
        );
        assert!(
            (155.0..170.0).contains(&max_r),
            "bord extérieur {max_r} hors de l'anneau"
        );
        // le rayon de la forme couvre l'anneau (dérive volontaire : pas de
        // rayon forcé à 36 - `game.rs` s'appuie dessus pour le pré-filtre)
        assert!(
            shape.radius >= 160.0,
            "rayon de la station {} trop petit pour couvrir l'anneau",
            shape.radius
        );
    }
}
