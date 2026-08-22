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
//!
//! **Composition des plans** : le mesh peut être composé par l'outil de
//! gestion — chaque plan du fichier est *toujours visible*
//! (`VAISSEAU_PLANES_ALWAYS`), *lié à une ligne d'atelier*
//! (`VAISSEAU_PLANE_LINKS`, visible à partir de son niveau, Progression) ou
//! *exclu* (jamais construit). Le vaisseau est construit avec les plans
//! visibles au niveau courant, dans une plage allouée à la **composition
//! maximale** (toujours + liés) : les achats d'extensions ne font que
//! révéler des plans déjà réservés (`rebuild_player_vaisseau`). Le pivot
//! (centre de rotation) est calculé sur la composition maximale, quel que
//! soit le niveau — acheter une extension ne décale jamais la rotation.
//! Listes vides = tous les plans (repli sûr).

use serde::Deserialize;

use crate::config::{argb32, PLAYER_INDEX, TEXTURE_NONE, WHOIAM_PLAYER};
use crate::geom::{Point, Triangle};
use crate::marketplace::{
    PlaneUpgradeLink, PlaneUpgradeTrack, VAISSEAU_BULLET_SPAWNS, VAISSEAU_CENTER_X_PERCENT,
    VAISSEAU_CENTER_Y_PERCENT, VAISSEAU_JSON, VAISSEAU_ORIENTATION_DEGREES, VAISSEAU_PLANE_LINKS,
    VAISSEAU_PLANES_ALWAYS, VAISSEAU_SCALE, VAISSEAU_THRUSTERS, VAISSEAU_WEAPONS, VaisseauThruster,
    VaisseauWeapon,
};
use crate::shape::{compute_real_positions, compute_shape_center, free_shape, Shape};
use crate::state::GameState;

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

/// Nombre total de faces du **catalogue d'armes** (chaque arme est toujours
/// visible, quel que soit le niveau d'atelier — ses faces s'ajoutent à celles
/// du vaisseau : l'arme est dessinée sur le vaisseau à son emplacement).
pub fn weapons_face_count() -> usize {
    VAISSEAU_WEAPONS
        .iter()
        .map(|w| {
            let file = parse_mesh(w.mesh);
            file.planes.iter().map(|p| p.faces.len()).sum::<usize>()
        })
        .sum()
}

/// Nombre de faces de la **composition maximale** du vaisseau (plans
/// toujours visibles + plans liés aux upgrades, quel que soit le niveau) +
/// faces du catalogue d'armes — la taille d'allocation du maillage : le
/// vaisseau est construit dans une plage assez grande pour toutes les
/// compositions possibles et toutes les armes, les achats d'extensions ne
/// faisant que révéler des plans déjà réservés. Exposé pour les tests
/// d'invariant du monde (`generate::prepare` construit le vaisseau + la
/// station : le compte de triangles total en découle).
#[cfg(test)]
pub fn vaisseau_face_count() -> usize {
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    let planes = file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| comp.get(*i).copied().unwrap_or(false))
        .map(|(_, p)| p.faces.len())
        .sum::<usize>();
    planes + weapons_face_count()
}

/// Nombre de faces **visibles aux niveaux d'atelier courants** (plans
/// toujours visibles + plans liés dont la ligne a atteint le niveau) +
/// faces du catalogue d'armes — la valeur de `life` du vaisseau construit
/// avec cet état. Exposé pour les tests d'invariant : un plan lié
/// (`VAISSEAU_PLANE_LINKS`) n'apparaît qu'à partir de son niveau, `life` ne
/// vaut `vaisseau_face_count()` (la composition maximale) qu'une fois toutes
/// les lignes montées.
#[cfg(test)]
pub fn vaisseau_visible_face_count(state: &GameState) -> usize {
    let file = vaisseau_file();
    let visible = plane_visibility(state);
    let planes = file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| visible.get(*i).copied().unwrap_or(false))
        .map(|(_, p)| p.faces.len())
        .sum::<usize>();
    planes + weapons_face_count()
}

/// RGBA (flottants 0..1) → ARGB 32 bits au format QB64 (AARRGGBB).
fn rgba_to_argb(rgba: [f32; 4]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    argb32(byte(rgba[3]), byte(rgba[0]), byte(rgba[1]), byte(rgba[2]))
}

/// Charge le fichier mesh embarqué (une fois, à chaque construction).
fn vaisseau_file() -> VaisseauFile {
    serde_json::from_str(VAISSEAU_JSON).expect("mesh du vaisseau : JSON invalide")
}

/// Parse un mesh « meshes-designer » embarqué (arme ou munition du catalogue
/// `VAISSEAU_WEAPONS`). Même format que le vaisseau : plans porteurs de
/// sommets et de faces colorées.
fn parse_mesh(json: &str) -> VaisseauFile {
    serde_json::from_str(json).expect("mesh du catalogue d'armes : JSON invalide")
}

/// Masque des plans **pouvant être visibles** (composition) : les plans
/// toujours visibles (`VAISSEAU_PLANES_ALWAYS`) et ceux liés à une ligne
/// d'atelier (`VAISSEAU_PLANE_LINKS`), quelle que soit la ligne atteinte.
/// Sert de boîte englobante au centre de rotation (le pivot reste stable
/// quand des plans apparaissent) et de taille d'allocation du maillage.
/// Listes vides = tous les plans (repli : composition non définie).
fn composition_mask(file: &VaisseauFile) -> Vec<bool> {
    let fallback = VAISSEAU_PLANES_ALWAYS.is_empty() && VAISSEAU_PLANE_LINKS.is_empty();
    (0..file.planes.len())
        .map(|i| {
            fallback
                || VAISSEAU_PLANES_ALWAYS.contains(&i)
                || VAISSEAU_PLANE_LINKS.iter().any(|l| l.plane_index == i)
        })
        .collect()
}

/// Masque de visibilité par plan pour des niveaux d'atelier donnés : les
/// plans toujours visibles, plus les plans liés dont la ligne a atteint le
/// niveau minimal. Un indice de plan hors bornes est ignoré (repli sûr :
/// jamais visible). Fonction pure (tests) — `plane_visibility` lit les
/// niveaux de l'état courant.
fn plane_visibility_with(
    always: &[usize],
    links: &[PlaneUpgradeLink],
    plane_count: usize,
    fuel_level: i32,
    ammo_level: i32,
    cargo_level: i32,
) -> Vec<bool> {
    let fallback = always.is_empty() && links.is_empty();
    let mut mask = vec![false; plane_count];
    for (i, m) in mask.iter_mut().enumerate() {
        if fallback || always.contains(&i) {
            *m = true;
        }
    }
    for link in links {
        if link.plane_index >= plane_count {
            continue; // indice de plan invalide : ignoré
        }
        let level = match link.track {
            PlaneUpgradeTrack::Fuel => fuel_level,
            PlaneUpgradeTrack::Ammo => ammo_level,
            PlaneUpgradeTrack::Cargo => cargo_level,
        };
        if level >= link.min_level {
            mask[link.plane_index] = true;
        }
    }
    mask
}

/// Masque de visibilité des plans du vaisseau aux niveaux d'atelier courants
/// (`state.resources.fuel_level` / `ammo_level` / `cargo_level` — Progression ;
/// 0 ailleurs, les plans liés restent alors cachés).
fn plane_visibility(state: &GameState) -> Vec<bool> {
    plane_visibility_with(
        VAISSEAU_PLANES_ALWAYS,
        VAISSEAU_PLANE_LINKS,
        vaisseau_file().planes.len(),
        state.resources.fuel_level,
        state.resources.ammo_level,
        state.resources.cargo_level,
    )
}

/// Emplacements de départ des projectiles (`VAISSEAU_BULLET_SPAWNS`) en
/// **points locaux du vaisseau** (repère des sommets du mesh transformé —
/// l'axe x local = le nez du vaisseau à l'orientation 0) : chaque emplacement
/// en % de la boîte englobante de la **composition maximale** est converti
/// par la même transformation que les sommets (pivot en %, rotation
/// `−orientation`, échelle, axe y retourné). Liste vide = un seul emplacement
/// au pivot (le centre de rotation — comportement d'origine). `fire_bullet`
/// (generate.rs) part de ces points, tournés avec le vaisseau.
pub fn vaisseau_bullet_spawns() -> Vec<Point> {
    vaisseau_bullet_spawns_with(
        VAISSEAU_BULLET_SPAWNS,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    )
}

/// Variante pure de `vaisseau_bullet_spawns` (tests) : emplacements en % de
/// la boîte englobante de la composition → points locaux du vaisseau, avec
/// les réglages donnés. Même transformation que les sommets du mesh.
fn vaisseau_bullet_spawns_with(
    spawns: &[(f64, f64)],
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> Vec<Point> {
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    let (minx, miny, maxx, maxy) = composition_bbox(&file, &comp);
    let (pivot, pt) =
        mesh_transform(&file, &comp, scale, orientation_degrees, center_percent);
    if spawns.is_empty() {
        return vec![pivot]; // repli : un seul emplacement au centre de rotation
    }
    spawns
        .iter()
        .map(|(x, y)| {
            let ex = minx + x / 100.0 * (maxx - minx);
            let ey = miny + y / 100.0 * (maxy - miny);
            pt([ex, ey])
        })
        .collect()
}

/// Propulseurs des éjections de gaz (`VAISSEAU_THRUSTERS`) : chaque
/// propulseur + son **point local** sur le vaisseau — sa `position` (en % de
/// la boîte englobante de la composition) convertie par la même
/// transformation que les sommets (pivot en %, rotation `−orientation`,
/// échelle, axe y retourné). Les meshes sont écrits dans le maillage du
/// vaisseau (`write_vaisseau`) et le gaz sort des points (src/main.rs).
/// Liste vide = repli : pas de propulseur (le gaz classique sort du centre
/// de rotation).
pub fn vaisseau_thrusters() -> Vec<(VaisseauThruster, Point)> {
    vaisseau_thrusters_with(
        VAISSEAU_THRUSTERS,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    )
}

/// Variante pure de `vaisseau_thrusters` (tests) : propulseur + point local
/// de sa position. Liste vide → aucune.
fn vaisseau_thrusters_with(
    thrusters: &[VaisseauThruster],
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> Vec<(VaisseauThruster, Point)> {
    if thrusters.is_empty() {
        return Vec::new();
    }
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    let (minx, miny, maxx, maxy) = composition_bbox(&file, &comp);
    let (_pivot, pt) =
        mesh_transform(&file, &comp, scale, orientation_degrees, center_percent);
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

/// Catalogue d'armes du vaisseau : chaque arme (`VAISSEAU_WEAPONS`) et son
/// **point local** sur le vaisseau — l'emplacement `spawn_index` de l'arme
/// (index dans `VAISSEAU_BULLET_SPAWNS`, liste contrainte) converti en point
/// local comme les emplacements de tir. Liste vide = tir classique (une balle
/// par emplacement, repli). `fire_bullet` (generate.rs) part de ces points
/// pour poser les armes sur le vaisseau et tirer leur munition.
pub fn vaisseau_weapons() -> Vec<(VaisseauWeapon, Point)> {
    vaisseau_weapons_with(
        VAISSEAU_WEAPONS,
        VAISSEAU_BULLET_SPAWNS,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    )
}

/// Variante pure de `vaisseau_weapons` (tests) : chaque arme + le point local
/// de son emplacement (`spawn_index` dans `spawns`). Un index hors bornes ou
/// une liste vide d'emplacements retombe sur le pivot (repli sûr).
fn vaisseau_weapons_with(
    weapons: &[VaisseauWeapon],
    spawns: &[(f64, f64)],
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> Vec<(VaisseauWeapon, Point)> {
    if weapons.is_empty() {
        return Vec::new();
    }
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    let (minx, miny, maxx, maxy) = composition_bbox(&file, &comp);
    let (pivot, pt) =
        mesh_transform(&file, &comp, scale, orientation_degrees, center_percent);
    // points locaux des emplacements (même conversion que les emplacements de
    // tir — une liste vide n'a qu'un emplacement : le pivot)
    let spawn_locals: Vec<Point> = if spawns.is_empty() {
        vec![pivot]
    } else {
        spawns
            .iter()
            .map(|(x, y)| {
                let ex = minx + x / 100.0 * (maxx - minx);
                let ey = miny + y / 100.0 * (maxy - miny);
                pt([ex, ey])
            })
            .collect()
    };
    weapons
        .iter()
        .map(|w| {
            let local = spawn_locals.get(w.spawn_index).copied().unwrap_or(pivot);
            (*w, local)
        })
        .collect()
}

/// Boîte englobante de la composition (repère de l'éditeur, y vers le haut) :
/// `(minx, miny, maxx, maxy)` — sert à situer le centre de rotation et les
/// emplacements de tir en pourcentage.
fn composition_bbox(file: &VaisseauFile, comp: &[bool]) -> (f64, f64, f64, f64) {
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    for (i, plane) in file.planes.iter().enumerate() {
        if !comp.get(i).copied().unwrap_or(false) {
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

/// Transforme un sommet éditeur → repère local du jeu : rotation de
/// `−orientation_degrees` autour du pivot (positionné en % de la boîte
/// englobante de la **composition**, 50/50 = centre), mise à l'échelle
/// `scale`, axe y retourné (éditeur y↑ → jeu y↓). Renvoie le pivot dans le
/// repère du jeu (le centre de rotation de la forme — `target_center`).
fn mesh_transform(
    file: &VaisseauFile,
    comp: &[bool],
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
) -> (Point, impl Fn([f64; 2]) -> Point) {
    let (minx, miny, maxx, maxy) = composition_bbox(file, comp);
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
    let pt = move |v: [f64; 2]| {
        let dx = v[0] - pivot_editor.x;
        let dy = v[1] - pivot_editor.y;
        Point::new(
            (pivot_editor.x + dx * cos_a - dy * sin_a) * scale,
            -(pivot_editor.y + dx * sin_a + dy * cos_a) * scale,
        )
    };
    // le pivot dans le repère local du jeu (le vaisseau pivote autour de lui)
    (Point::new(pivot_editor.x * scale, -pivot_editor.y * scale), pt)
}

/// Écrit les triangles des plans visibles de `file` dans la plage réservée
/// de la forme `shape_index` (`first_triangle..=last_triangle`, taille de la
/// composition maximale + armes) : une `Triangle` par face, compactées en
/// tête de plage, puis les meshes des armes du catalogue (`weapons`, chacun
/// avec son point local) ; `life` = nombre écrit. Les plans non visibles
/// laissent leurs triangles morts (l'invariant du jeu : `life` = triangles
/// vivants). Les propulseurs, eux, ne font **pas** partie du maillage : leur
/// mesh est dessiné dynamiquement (scintillant) quand ils tirent
/// (`thruster_mesh_triangles` + `render::draw_thruster_gas`, src/main.rs).
fn write_vaisseau(
    file: &VaisseauFile,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    shape_index: usize,
    pt: &impl Fn([f64; 2]) -> Point,
    visible: &[bool],
    weapons: &[(VaisseauWeapon, Point)],
) {
    let first = shapes[shape_index].first_triangle;
    let mut k = 0usize;
    for (pi, plane) in file.planes.iter().enumerate() {
        if !visible.get(pi).copied().unwrap_or(false) {
            continue;
        }
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
    for (weapon, spawn) in weapons {
        k = write_weapon(weapon, *spawn, shapes, triangles, shape_index, k);
    }
    shapes[shape_index].life = k as i32;
}

/// Triangles (repère local du vaisseau) du mesh d'un **propulseur** — la
/// flamme du gaz d'éjection, dessinée **seulement quand le propulseur tire**
/// (src/main.rs + `render::draw_thruster_gas`). Même transformation que
/// l'ancien `write_thruster` : échelle `thruster.scale`, rotation de
/// `−thruster.orientation_degrees` autour du centre de sa boîte englobante,
/// axe y retourné, puis pivot posé sur `spawn` (point local du vaisseau).
/// Chaque sommet : `[x, y]` dans le repère local du vaisseau (la couleur
/// appliquée est celle configurée — `thruster.color` — teinte par le rendu).
pub fn thruster_mesh_triangles(
    thruster: &VaisseauThruster,
    spawn: Point,
) -> Vec<([f64; 2], [f64; 2], [f64; 2])> {
    let file = parse_mesh(thruster.mesh);
    let comp = vec![true; file.planes.len()]; // toute la flamme visible
    let (pivot, pt) = mesh_transform(
        &file,
        &comp,
        thruster.scale,
        thruster.orientation_degrees,
        Point::new(50.0, 50.0),
    );
    // translation : le pivot du propulseur posé sur l'emplacement
    let dx = spawn.x - pivot.x;
    let dy = spawn.y - pivot.y;
    let mut out = Vec::new();
    for plane in &file.planes {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            let p1 = pt(plane.verts[i]);
            let p2 = pt(plane.verts[j]);
            let p3 = pt(plane.verts[l]);
            out.push((
                [p1.x + dx, p1.y + dy],
                [p2.x + dx, p2.y + dy],
                [p3.x + dx, p3.y + dy],
            ));
        }
    }
    out
}

/// Écrit le mesh d'une arme du catalogue dans la plage du vaisseau, à partir
/// de l'index `k` : le mesh est transformé (échelle `weapon.scale`, rotation
/// de `−weapon.orientation_degrees` autour du **centre de sa boîte
/// englobante** (pivot 50/50), axe y retourné) puis translaté pour que le
/// pivot de l'arme soit posé sur `spawn` (point local du vaisseau). Les
/// triangles sont dans le repère local du vaisseau : ils tournent avec lui.
/// Renvoie l'index du prochain triangle écrit.
fn write_weapon(
    weapon: &VaisseauWeapon,
    spawn: Point,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    shape_index: usize,
    k: usize,
) -> usize {
    let file = parse_mesh(weapon.mesh);
    let comp = vec![true; file.planes.len()]; // toute l'arme visible
    let (pivot, pt) = mesh_transform(
        &file,
        &comp,
        weapon.scale,
        weapon.orientation_degrees,
        Point::new(50.0, 50.0),
    );
    // translation : le pivot de l'arme posé sur l'emplacement
    let dx = spawn.x - pivot.x;
    let dy = spawn.y - pivot.y;
    let first = shapes[shape_index].first_triangle;
    let mut k = k;
    for plane in &file.planes {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            let mut t = Triangle::default();
            let p1 = pt(plane.verts[i]);
            let p2 = pt(plane.verts[j]);
            let p3 = pt(plane.verts[l]);
            t.create(
                Point::new(p1.x + dx, p1.y + dy),
                Point::new(p2.x + dx, p2.y + dy),
                Point::new(p3.x + dx, p3.y + dy),
            );
            t.color = rgba_to_argb(face.color);
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }
    k
}

/// Construit la forme « munition » d'une arme du catalogue à partir du mesh
/// embarqué `ammo_mesh` (une `Triangle` par face, couleur RGBA → ARGB) :
/// échelle `ammo_scale`, rotation de `−ammo_orientation_degrees` autour du
/// centre de la boîte englobante (le mesh de la munition est dessiné nez en
/// avant) et axe y retourné. La forme est posée à l'origine (le point de
/// départ est `shape.position`, tourné avec le vaisseau par `fire_bullet`).
/// Renvoie l'index de la forme créée.
pub fn create_ammo_shape(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    ammo_mesh: &str,
    ammo_scale: f64,
    ammo_orientation_degrees: f64,
) -> usize {
    let file = parse_mesh(ammo_mesh);
    let comp = vec![true; file.planes.len()]; // toute la munition visible
    let nbr: usize = file.planes.iter().map(|p| p.faces.len()).sum();
    let (pivot, pt) = mesh_transform(
        &file,
        &comp,
        ammo_scale,
        ammo_orientation_degrees,
        Point::new(50.0, 50.0),
    );

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue — même schéma que `build_vaisseau`
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
    shape.who_i_am = 0; // posé par `fire_bullet` (WHOIAM_BULLET)
    shape.is_collider = true;
    shape.show_all_parts = true;
    shape.texture = TEXTURE_NONE;
    shape.shape_color = 0xFFFF0000; // repli (chaque face a sa couleur)
    shape.position = Point::new(0.0, 0.0);
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.target_center = Point::new(0.0, 0.0);
    shape.center = Point::new(0.0, 0.0);

    let first = shapes[shape_index].first_triangle;
    let mut k = 0usize;
    for plane in &file.planes {
        for face in &plane.faces {
            let [i, j, l] = face.v;
            let mut t = Triangle::default();
            // centrée sur le pivot : la forme tourne autour de son centre
            let p1 = pt(plane.verts[i]);
            let p2 = pt(plane.verts[j]);
            let p3 = pt(plane.verts[l]);
            t.create(
                Point::new(p1.x - pivot.x, p1.y - pivot.y),
                Point::new(p2.x - pivot.x, p2.y - pivot.y),
                Point::new(p3.x - pivot.x, p3.y - pivot.y),
            );
            t.color = rgba_to_argb(face.color);
            t.shape_index = shape_index as i32;
            t.id = (first + k) as i32;
            triangles[first + k] = t;
            k += 1;
        }
    }

    let shape = &mut shapes[shape_index];
    compute_shape_center(shape, triangles);
    shape.target_center = Point::new(0.0, 0.0);
    shape.center = Point::new(0.0, 0.0);
    shape_index
}

/// Construit le vaisseau joueur à partir des réglages de `src/marketplace.rs`
/// (fichier généré par l'outil de gestion) : mesh `VAISSEAU_JSON`, échelle
/// `VAISSEAU_SCALE`, orientation `VAISSEAU_ORIENTATION_DEGREES`, centre de
/// rotation `VAISSEAU_CENTER_X/Y_PERCENT` et **composition des plans** aux
/// niveaux d'atelier courants (les plans liés aux upgrades apparaissent à
/// partir de leur niveau — 0 au lancement, la progression chargée avant
/// `prepare`). Voir `build_vaisseau`.
pub fn create_player_vaisseau(
    state: &GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
) -> usize {
    build_vaisseau(
        shapes,
        triangles,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
        &plane_visibility(state),
        &weapons_mask(state),
    )
}

/// Armes du catalogue **possédées** (masque par index, `scenario::weapon_owned`
/// — hors économie toutes les armes sont équipées ; en Progression seules
/// celles achetées au magasin, les armes de base à coût nul étant équipées
/// d'office). Le mesh d'une arme non possédée n'est pas construit sur le
/// vaisseau (elle ne tire pas non plus).
fn weapons_mask(state: &GameState) -> Vec<bool> {
    (0..VAISSEAU_WEAPONS.len())
        .map(|i| crate::scenario::weapon_owned(state, i))
        .collect()
}

/// Construit la forme « vaisseau » : une `Triangle` par face des plans
/// visibles (masque `visible`), à la suite des triangles existants, mise à
/// l'échelle `scale`, tournée de `−orientation_degrees` autour du pivot
/// `center_percent` (position en **pourcentage de la boîte englobante de la
/// composition**, 50/50 = centre géométrique) et posée à l'origine (le
/// vaisseau démarre au centre de la station, position 0,0 — le monde n'est
/// pas encore initialisé). Seules les armes du catalogue dont le masque
/// `weapons_mask` (index de `VAISSEAU_WEAPONS`) est `true` — les armes
/// **possédées** — sont dessinées sur le vaisseau. La plage allouée couvre la
/// **composition maximale** (plans toujours visibles + liés, et **toutes**
/// les armes du catalogue) : une reconstruction ultérieure
/// (`rebuild_player_vaisseau`) ne révèle que des plans/armes déjà réservés.
/// Le pivot devient le centre de la forme : le vaisseau pivote autour de lui
/// dans le jeu (rotation des triangles autour de `shape.center`). Renvoie
/// l'index de la forme créée (réutilise une forme détruite au même nombre de
/// triangles quand c'est possible, comme `meshes_to_shape` — ici `shapes`
/// est vide, l'index est 0).
fn build_vaisseau(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    scale: f64,
    orientation_degrees: f64,
    center_percent: Point,
    visible: &[bool],
    weapons_mask: &[bool],
) -> usize {
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    // nombre de triangles alloués : la composition maximale (toujours + liés)
    // + toutes les armes du catalogue (les propulseurs ne font pas partie du
    // maillage : leur mesh est dessiné dynamiquement quand ils tirent)
    let max_faces: usize = file
        .planes
        .iter()
        .enumerate()
        .filter(|(i, _)| comp.get(*i).copied().unwrap_or(false))
        .map(|(_, p)| p.faces.len())
        .sum::<usize>()
        + weapons_face_count();
    let (pivot, pt) =
        mesh_transform(&file, &comp, scale, orientation_degrees, center_percent);
    // seules les armes possédées sont dessinées (masque par index du
    // catalogue — les armes à acheter n'apparaissent qu'après l'achat)
    let weapons: Vec<(VaisseauWeapon, Point)> =
        vaisseau_weapons_with(VAISSEAU_WEAPONS, VAISSEAU_BULLET_SPAWNS, scale, orientation_degrees, center_percent)
            .into_iter()
            .enumerate()
            .filter(|(i, _)| weapons_mask.get(*i).copied().unwrap_or(false))
            .map(|(_, w)| w)
            .collect();

    // emplacement de la forme : réutilise un slot mort au même nombre de
    // triangles, sinon alloue — même schéma que `build_cosmonaut`
    let shape_index = match free_shape(shapes, max_faces) {
        Some(idx) => idx,
        None => {
            let idx = shapes.len();
            shapes.push(Shape::default());
            triangles.resize(triangles.len() + max_faces, Triangle::default());
            shapes[idx].first_triangle = triangles.len() - max_faces;
            shapes[idx].last_triangle = triangles.len() - 1;
            idx
        }
    };

    let shape = &mut shapes[shape_index];
    shape.id = shape_index as i32;
    shape.life = max_faces as i32;
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

    write_vaisseau(
        &file,
        shapes,
        triangles,
        shape_index,
        &pt,
        visible,
        &weapons,
    );

    let shape = &mut shapes[shape_index];
    compute_shape_center(shape, triangles);
    // centre de rotation : le pivot choisi (src/marketplace.rs), pas le
    // centroïde des faces — le vaisseau pivote autour de ce point. `moving_shape`
    // fait converger `center` vers `target_center` (÷100 par frame) — posés
    // égaux, le vaisseau reste stable dès le départ (pas de dérive).
    shape.target_center = pivot;
    shape.center = pivot;
    shape_index
}

/// Reconstruit le vaisseau joueur **en place** avec la composition courante
/// (niveaux d'atelier : les plans liés aux upgrades apparaissent à partir de
/// leur niveau) — le maillage est réécrit dans la plage allouée au lancement
/// (index `PLAYER_INDEX`, taille de la composition maximale) et les
/// cinématiques (position, orientation, vitesse, centre de rotation) sont
/// préservées : seuls les triangles changent. Appelé après un achat
/// d'extension à l'atelier (Progression) et au respawn — le pivot est calculé
/// sur la composition maximale, il ne bouge donc jamais quand des plans
/// apparaissent.
pub fn rebuild_player_vaisseau(
    state: &GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
) {
    let file = vaisseau_file();
    let comp = composition_mask(&file);
    let visible = plane_visibility(state);
    let (pivot, pt) = mesh_transform(
        &file,
        &comp,
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    );
    // seules les armes possédées sont dessinées (une arme achetée apparaît
    // à la reconstruction — `buy_weapon_and_save` côté jeu)
    let mask = weapons_mask(state);
    let weapons: Vec<(VaisseauWeapon, Point)> =
        vaisseau_weapons_with(VAISSEAU_WEAPONS, VAISSEAU_BULLET_SPAWNS, VAISSEAU_SCALE, VAISSEAU_ORIENTATION_DEGREES, Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT))
            .into_iter()
            .enumerate()
            .filter(|(i, _)| mask.get(*i).copied().unwrap_or(false))
            .map(|(_, w)| w)
            .collect();
    // reconstruction en place : toute la plage réservée est tuée puis
    // réécrite (les triangles des plans non visibles restent morts)
    let shape = &shapes[PLAYER_INDEX];
    for i in shape.first_triangle..=shape.last_triangle {
        triangles[i].life = 0;
    }
    write_vaisseau(
        &file,
        shapes,
        triangles,
        PLAYER_INDEX,
        &pt,
        &visible,
        &weapons,
    );

    let shape = &mut shapes[PLAYER_INDEX];
    compute_shape_center(shape, triangles);
    shape.target_center = pivot;
    shape.center = pivot;
    // positions réelles recalculées avec les cinématiques courantes (le
    // vaisseau est à quai pendant l'atelier ; le respawn vient de le poser)
    for i in shape.first_triangle..=shape.last_triangle {
        if triangles[i].life > 0 {
            compute_real_positions(
                &mut triangles[i],
                shape.position,
                shape.center,
                shape.orientation,
            );
        }
    }
}

/// Mesh de munition de test (2 faces colorées — pointe vers +x) pour les
/// tests du catalogue d'armes (`fire_bullet_with` de generate.rs).
#[cfg(test)]
pub fn vaisseau_test_ammo_mesh() -> &'static str {
    r#"{"planes":[{"verts":[[-4.0,-2.0],[-4.0,2.0],[4.0,2.0],[4.0,-2.0]],"faces":[
        {"v":[0,1,2],"color":[1.0,0.2,0.2,1.0]},
        {"v":[0,2,3],"color":[0.2,1.0,0.2,1.0]}
    ]}]}"#
}

/// Deux armes de test (catalogue factice) : une au nez (90 %, 50 %) et une à
/// l'arrière (10 %, 50 %) — utilisées par `fire_bullet_with` de generate.rs.
#[cfg(test)]
pub fn vaisseau_test_weapons() -> Vec<VaisseauWeapon> {
    vec![
        VaisseauWeapon {
            name: "CANON AVANT",
            mesh: vaisseau_test_ammo_mesh(),
            scale: 1.0,
            orientation_degrees: 0.0,
            spawn_index: 0,
            ammo_mesh: vaisseau_test_ammo_mesh(),
            ammo_scale: 1.0,
            ammo_orientation_degrees: 0.0,
            cost: 0,
            ammo_price: 1,
            ammo_pack: 5,
        },
        VaisseauWeapon {
            name: "CANON ARRIÈRE",
            mesh: vaisseau_test_ammo_mesh(),
            scale: 1.0,
            orientation_degrees: 0.0,
            spawn_index: 1,
            ammo_mesh: vaisseau_test_ammo_mesh(),
            ammo_scale: 1.0,
            ammo_orientation_degrees: 0.0,
            cost: 0,
            ammo_price: 1,
            ammo_pack: 5,
        },
    ]
}

/// Points locaux des deux armes de test (emplacements 90/50 et 10/50 en % de
/// la boîte englobante) — mêmes réglages que le vaisseau réel.
#[cfg(test)]
pub fn vaisseau_test_weapon_locals() -> Vec<Point> {
    vaisseau_weapons_with(
        &vaisseau_test_weapons(),
        &[(90.0, 50.0), (10.0, 50.0)],
        VAISSEAU_SCALE,
        VAISSEAU_ORIENTATION_DEGREES,
        Point::new(VAISSEAU_CENTER_X_PERCENT, VAISSEAU_CENTER_Y_PERCENT),
    )
    .into_iter()
    .map(|(_, p)| p)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaisseau_has_all_faces_with_their_color() {
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_player_vaisseau(&state, &mut shapes, &mut triangles);

        // une Triangle vivante par face **visible** du fichier, dans une
        // seule forme — le compte est dérivé du fichier et de la composition
        // aux niveaux courants (les plans liés aux upgrades n'apparaissent
        // qu'à partir de leur niveau : au niveau 0, seuls les plans
        // « toujours visibles » sont construits). La plage allouée, elle,
        // couvre la composition maximale (`vaisseau_face_count`).
        let nbr = vaisseau_visible_face_count(&state);
        assert_eq!(shapes.len(), 1);
        assert_eq!(idx, PLAYER_INDEX);
        let s = &shapes[idx];
        assert_eq!(s.who_i_am, WHOIAM_PLAYER);
        assert_eq!(s.life as usize, nbr);
        assert!(s.last_triangle - s.first_triangle + 1 >= nbr);
        assert!(s.is_collider);
        assert_eq!(s.texture, TEXTURE_NONE);
        let mut seen = 0;
        for i in s.first_triangle..=s.last_triangle {
            let t = &triangles[i];
            if t.life > 0 {
                seen += 1;
                assert_eq!(t.shape_index, idx as i32);
                assert_ne!(t.color, 0, "chaque face visible doit porter sa couleur");
            }
        }
        assert_eq!(seen, nbr);
        // plusieurs couleurs distinctes (fuselage gris, verrière bleue,
        // ailerons/tuyère…)
        let colors: std::collections::HashSet<u32> =
            (s.first_triangle..=s.last_triangle)
                .filter(|&i| triangles[i].life > 0)
                .map(|i| triangles[i].color)
                .collect();
        assert!(colors.len() >= 3, "{} couleurs distinctes attendues", colors.len());
    }

    #[test]
    fn ascii_munition_mesh_parses_and_points_forward() {
        // le mesh généré depuis l'art ASCII `assets/asciiart-fr.txt`
        // (tools/ascii-art-to-mesh.py) est compatible avec le parseur du
        // catalogue : plans/sommets/faces valides, indices dans les bornes,
        // couleurs portées — et, comme toute munition, le **nez** du missile
        // pointe vers +x (convention du catalogue : la munition est dessinée
        // nez en avant, `ammo_orientation_degrees = 0`)
        let json = include_str!("../assets/missileWeapon.json");
        let file = parse_mesh(json);
        assert!(!file.planes.is_empty());
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        for plane in &file.planes {
            assert!(!plane.faces.is_empty());
            for face in &plane.faces {
                assert!(face.v.iter().all(|&i| i < plane.verts.len()));
                assert_ne!(rgba_to_argb(face.color), 0);
            }
            for v in &plane.verts {
                min_x = min_x.min(v[0]);
                max_x = max_x.max(v[0]);
            }
        }
        // le nez (le haut de l'art) est ramené sur +x : le mesh part de
        // l'origine et le corps s'étend vers la droite
        assert!(min_x >= -0.001, "le nez doit être à l'origine (x={min_x})");
        assert!(max_x > 10.0, "le corps doit s'étendre vers +x (x={max_x})");

        // la forme « munition » se construit comme au tir (`fire_bullet` →
        // `create_ammo_shape`) : une Triangle vivante par face, chaque face
        // portant sa couleur
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_ammo_shape(&mut shapes, &mut triangles, json, 1.0, 0.0);
        let expected: usize = file.planes.iter().map(|p| p.faces.len()).sum();
        assert_eq!(shapes[idx].life as usize, expected);
        let mut colored = 0;
        for t in &triangles[shapes[idx].first_triangle..=shapes[idx].last_triangle] {
            if t.life > 0 {
                colored += 1;
                assert_ne!(t.color, 0);
            }
        }
        assert_eq!(colored, expected);
    }

    #[test]
    fn vaisseau_y_is_flipped_and_nose_points_right() {
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let s = &shapes[0];

        // Le maillage du joueur contient la composition du vaisseau et les
        // armes du catalogue ; la boîte finale peut donc être plus grande que
        // celle de `assets/vaisseau.json`. Vérifier que le mesh de base reste
        // bien présent dans cette boîte, avec l'axe y retourné et le nez à
        // droite, sans figer les extensions ajoutées par le catalogue.
        let file = vaisseau_file();
        let comp = composition_mask(&file);
        let (minx, miny, maxx, maxy) = composition_bbox(&file, &comp);
        let scale = VAISSEAU_SCALE;
        let tol = 0.05 + 0.001 * (maxx - minx) * scale;
        assert!(s.top_left.y <= -maxy * scale + tol, "haut : {}", s.top_left.y);
        assert!(s.bottom_right.y >= -miny * scale - tol, "bas : {}", s.bottom_right.y);
        assert!(s.bottom_right.x >= maxx * scale - tol, "nez à droite : {}", s.bottom_right.x);
        assert!(s.top_left.x <= minx * scale + tol, "tuyère à gauche : {}", s.top_left.x);
        assert!(s.width + tol >= (maxx - minx) * scale, "largeur {:.2}", s.width);
        assert!(s.height + tol >= (maxy - miny) * scale, "hauteur {:.2}", s.height);
        // posé au centre de la station, immobile, centre fixé (pas de dérive)
        assert_eq!(s.position, Point::new(0.0, 0.0));
        assert_eq!(s.velocity, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.orientation, 0.0);
        assert_eq!(s.center, s.target_center);
    }

    #[test]
    fn vaisseau_scale_is_applied_to_the_mesh() {
        // toutes les parties de la composition, y compris les armes, suivent
        // l'échelle : 50 % doit produire une boîte deux fois plus petite que
        // la même composition à 100 %.
        let state = GameState::new();
        let mut small_shapes = Vec::new();
        let mut small_triangles = Vec::new();
        build_vaisseau(
            &mut small_shapes,
            &mut small_triangles,
            0.5,
            0.0,
            Point::new(50.0, 50.0),
            &plane_visibility(&state),
            &weapons_mask(&state),
        );
        let mut full_shapes = Vec::new();
        let mut full_triangles = Vec::new();
        build_vaisseau(
            &mut full_shapes,
            &mut full_triangles,
            1.0,
            0.0,
            Point::new(50.0, 50.0),
            &plane_visibility(&state),
            &weapons_mask(&state),
        );
        let small = &small_shapes[0];
        let full = &full_shapes[0];
        // Les armes ont leur propre échelle et ne suivent pas celle du
        // vaisseau : on compare uniquement les faces de base, écrites en tête
        // de la plage de triangles.
        let file = vaisseau_file();
        let comp = composition_mask(&file);
        let visible = plane_visibility(&state);
        let base_faces: usize = file
            .planes
            .iter()
            .enumerate()
            .filter(|(i, _)| visible.get(*i).copied().unwrap_or(false) && comp.get(*i).copied().unwrap_or(false))
            .map(|(_, p)| p.faces.len())
            .sum();
        let base_bbox = |shape: &Shape, triangles: &[Triangle]| {
            let mut minx = f64::INFINITY;
            let mut maxx = f64::NEG_INFINITY;
            let mut miny = f64::INFINITY;
            let mut maxy = f64::NEG_INFINITY;
            for i in shape.first_triangle..shape.first_triangle + base_faces {
                for p in [triangles[i].a, triangles[i].b, triangles[i].c] {
                    minx = minx.min(p.x);
                    maxx = maxx.max(p.x);
                    miny = miny.min(p.y);
                    maxy = maxy.max(p.y);
                }
            }
            (maxx - minx, maxy - miny)
        };
        let (small_width, small_height) = base_bbox(small, &small_triangles);
        let (full_width, full_height) = base_bbox(full, &full_triangles);
        assert!((small_width / full_width - 0.5).abs() < 1e-9);
        assert!((small_height / full_height - 0.5).abs() < 1e-9);
        assert!(small.bottom_right.x > 0.0, "nez à droite : {}", small.bottom_right.x);
    }

    #[test]
    fn vaisseau_orientation_rotates_the_mesh_around_the_pivot() {
        // orientation 90 = le nez du mesh est « vers le haut » dans l'éditeur :
        // le mesh est tourné de −90° autour du centre de rotation — le nez du
        // mesh actuel (qui pointe à droite, orientation réelle 0) passe donc
        // vers le bas (dans le repère du jeu) et la boîte pivote de 90°
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(
            &mut shapes,
            &mut triangles,
            1.0,
            90.0,
            Point::new(50.0, 50.0),
            &plane_visibility(&state),
            &weapons_mask(&state),
        );
        let s = &shapes[0];
        // le nez finit en bas dans le repère du jeu et la boîte est tournée
        // (toutes les armes suivent aussi la rotation du vaisseau).
        assert!(s.bottom_right.y > 0.0, "nez en bas : {}", s.bottom_right.y);
        assert!(s.top_left.y < 0.0, "tuyère en haut : {}", s.top_left.y);
        assert!(s.width > s.height, "boîte pivotée : {} > {}", s.width, s.height);
        let file = vaisseau_file();
        let comp = composition_mask(&file);
        let (pivot, _) = mesh_transform(&file, &comp, 1.0, 90.0, Point::new(50.0, 50.0));
        assert!((s.target_center.x - pivot.x).abs() < 1e-9, "pivot x {}", s.target_center.x);
        assert!((s.target_center.y - pivot.y).abs() < 1e-9, "pivot y {}", s.target_center.y);
    }

    #[test]
    fn vaisseau_rotation_center_moves_with_the_bbox_percentage() {
        // centre de rotation 0 %/0 % = coin haut-gauche de la boîte englobante
        // (repère éditeur, y↑) : dans le jeu (y↓), le pivot est le coin
        // bas-gauche de la boîte — le vaisseau s'étend à droite et vers le bas
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        build_vaisseau(
            &mut shapes,
            &mut triangles,
            1.0,
            0.0,
            Point::new(0.0, 0.0),
            &plane_visibility(&state),
            &weapons_mask(&state),
        );
        let s = &shapes[0];
        let file = vaisseau_file();
        let comp = composition_mask(&file);
        let (expected, _) = mesh_transform(&file, &comp, 1.0, 0.0, Point::new(0.0, 0.0));
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
        build_vaisseau(
            &mut shapes,
            &mut triangles,
            1.0,
            0.0,
            Point::new(100.0, 100.0),
            &plane_visibility(&state),
            &weapons_mask(&state),
        );
        let s = &shapes[0];
        let (expected, _) = mesh_transform(&file, &comp, 1.0, 0.0, Point::new(100.0, 100.0));
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
        // **visibles** portant une couleur (la leur ou la couleur par défaut)
        // — un plan lié à une ligne d'atelier (`VAISSEAU_PLANE_LINKS`) n'est
        // pas construit aux niveaux 0 : seules les faces vivantes comptent
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let s = &shapes[idx];
        assert_eq!(s.life as usize, vaisseau_visible_face_count(&state));
        let mut visible = 0;
        for i in s.first_triangle..=s.last_triangle {
            if triangles[i].life > 0 {
                visible += 1;
                assert_ne!(triangles[i].color, 0, "chaque face visible doit porter une couleur");
            }
        }
        assert_eq!(visible, s.life as usize);
    }

    #[test]
    fn plane_visibility_follows_upgrade_levels() {
        // 4 plans : 0-1 toujours visibles, 2 lié au réservoir (niveau 1),
        // 3 lié à la soute (niveau 2) — les niveaux font apparaître les plans
        let links = [
            PlaneUpgradeLink {
                plane_index: 2,
                track: PlaneUpgradeTrack::Fuel,
                min_level: 1,
            },
            PlaneUpgradeLink {
                plane_index: 3,
                track: PlaneUpgradeTrack::Cargo,
                min_level: 2,
            },
        ];
        let always = [0usize, 1];
        // niveau 0 : seuls les plans toujours visibles
        let m0 = plane_visibility_with(&always, &links, 4, 0, 0, 0);
        assert_eq!(m0, vec![true, true, false, false]);
        // réservoir au niveau 1 : le plan 2 apparaît
        let m1 = plane_visibility_with(&always, &links, 4, 1, 0, 0);
        assert_eq!(m1, vec![true, true, true, false]);
        // soute au niveau 2 : le plan 3 apparaît (et 2 reste visible)
        let m2 = plane_visibility_with(&always, &links, 4, 1, 0, 2);
        assert_eq!(m2, vec![true, true, true, true]);
        // indices hors bornes ignorés (repli sûr), listes vides = tous
        let mbad = plane_visibility_with(&always, &[PlaneUpgradeLink {
            plane_index: 9,
            track: PlaneUpgradeTrack::Ammo,
            min_level: 1,
        }], 4, 5, 5, 5);
        assert_eq!(mbad, vec![true, true, false, false]);
        let mall = plane_visibility_with(&[], &[], 4, 0, 0, 0);
        assert_eq!(mall, vec![true, true, true, true]);
    }

    #[test]
    fn rebuild_player_vaisseau_preserves_kinematics() {
        // la reconstruction en place réécrit le maillage dans la plage
        // réservée sans toucher aux cinématiques (position, orientation,
        // vitesse) ni au centre de rotation — les triangles vivants sont
        // ceux de la composition courante
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let s = &mut shapes[PLAYER_INDEX];
        s.position = Point::new(42.0, -17.0);
        s.orientation = 0.7;
        s.velocity = 1.5;
        let pivot = s.target_center;
        let alive_before = (s.first_triangle..=s.last_triangle)
            .filter(|&i| triangles[i].life > 0)
            .count();

        rebuild_player_vaisseau(&state, &mut shapes, &mut triangles);

        let s = &shapes[PLAYER_INDEX];
        assert_eq!(s.position, Point::new(42.0, -17.0));
        assert_eq!(s.orientation, 0.7);
        assert_eq!(s.velocity, 1.5);
        assert_eq!(s.target_center, pivot);
        // même composition (niveaux inchangés) : autant de triangles vivants
        let alive_after = (s.first_triangle..=s.last_triangle)
            .filter(|&i| triangles[i].life > 0)
            .count();
        assert_eq!(alive_after, alive_before);
        assert_eq!(alive_after, vaisseau_visible_face_count(&state));
        // les positions réelles sont recalculées (le vaisseau n'est pas à
        // l'origine dans ce test)
        let t = &triangles[s.first_triangle];
        assert!((t.real_a.x - s.position.x).abs() > 0.0);
    }

    #[test]
    fn bullet_spawns_map_percents_to_local_points() {
        // la liste générée (`VAISSEAU_BULLET_SPAWNS`) fait foi : le nombre
        // d'emplacements est celui de la liste, et chaque point est dans la
        // boîte englobante du vaisseau (indépendant de l'échelle/orientation)
        let (pivot,) = {
            let state = GameState::new();
            let mut shapes = Vec::new();
            let mut triangles = Vec::new();
            create_player_vaisseau(&state, &mut shapes, &mut triangles);
            (shapes[PLAYER_INDEX].target_center,)
        };
        let spawns = vaisseau_bullet_spawns();
        if VAISSEAU_BULLET_SPAWNS.is_empty() {
            // repli (liste vide) : un seul emplacement au pivot — le centre
            // de rotation, soit `target_center`
            assert_eq!(spawns.len(), 1);
            assert!(
                (spawns[0].x - pivot.x).abs() < 1e-9 && (spawns[0].y - pivot.y).abs() < 1e-9,
                "repli : emplacement au centre de rotation {:?} ≠ pivot {:?}",
                spawns[0],
                pivot
            );
        } else {
            assert_eq!(spawns.len(), VAISSEAU_BULLET_SPAWNS.len());
            // chaque emplacement reste dans la boîte englobante (aucun point
            // ne s'échappe — la conversion % → point local est bornée)
            let file = vaisseau_file();
            let comp = composition_mask(&file);
            let (minx, miny, maxx, maxy) = composition_bbox(&file, &comp);
            for s in &spawns {
                assert!(
                    s.x >= minx * VAISSEAU_SCALE && s.x <= maxx * VAISSEAU_SCALE,
                    "emplacement hors bbox x : {:?}",
                    s
                );
                assert!(
                    s.y >= -maxy * VAISSEAU_SCALE && s.y <= -miny * VAISSEAU_SCALE,
                    "emplacement hors bbox y : {:?}",
                    s
                );
            }
        }

        // deux emplacements : (90, 50) = nez (+x local, à droite de la bbox)
        // et (10, 50) = arrière (−x local) — le centre de rotation (50, 50)
        // est le pivot (l'origine du repère local). Propriétés indépendantes
        // de l'échelle/orientation réglées dans l'outil : même %y → même
        // ordonnée locale, et l'ordre gauche/droite est conservé.
        let pts = vaisseau_bullet_spawns_with(
            &[(90.0, 50.0), (50.0, 50.0), (10.0, 50.0)],
            1.0,
            0.0,
            Point::new(50.0, 50.0),
        );
        assert_eq!(pts.len(), 3);
        // même %y : les trois points sont alignés horizontalement (même y
        // local) ; 90 % est à droite de (50,50) (nez), 10 % à gauche (arrière)
        assert!(
            (pts[0].y - pts[2].y).abs() < 1e-9,
            "même y% → même ordonnée locale : {:?}",
            pts
        );
        assert!(
            pts[0].x > pts[1].x && pts[2].x < pts[1].x,
            "90 % nez à droite, 10 % arrière à gauche : {:?}",
            pts
        );
    }

    #[test]
    fn thrusters_follow_the_4_keys_order_and_sides() {
        // `VAISSEAU_THRUSTERS` : 4 propulseurs ordonnés ↑ (arrière, -x local),
        // ↓ (avant, +x local), ← et → (flancs) — chaque `position` (en % de la
        // boîte englobante, valeurs libres : négatives et > 100 % possibles)
        // convertie comme les emplacements de tir ; liste vide = repli
        // (aucun propulseur).
        let pts = vaisseau_thrusters_with(
            VAISSEAU_THRUSTERS,
            1.0,
            0.0,
            Point::new(50.0, 50.0),
        );
        assert_eq!(pts.len(), 4);
        assert_eq!(pts[0].0.name, "ARRIÈRE");
        assert_eq!(pts[1].0.name, "AVANT");
        // pivot = centre de rotation (50/50) : l'origine du repère local
        let p0 = &pts[0].1; // ↑ arrière
        let p1 = &pts[1].1; // ↓ avant
        let p2 = &pts[2].1; // ← GAUCHE
        let p3 = &pts[3].1; // → DROITE
        assert!(p0.x < 0.0 && p1.x > 0.0, "↑ derrière, ↓ devant : {:?} {:?}", p0, p1);
        assert!(
            (p0.y - p1.y).abs() < 1e-9,
            "↑ et ↓ sur l'axe central (même y) : {:?} {:?}",
            p0,
            p1
        );
        // les flancs sont des positions libres (l'outil les déplace) : le
        // réglage courant place GAUCHE à gauche (-y local) et DROITE à droite
        // (+y local). Le jeu croise ensuite ces deux propulseurs pour produire
        // le couple correspondant à la touche de rotation.
        assert!(
            p2.y < 0.0 && p3.y > 0.0,
            "GAUCHE à gauche (-y), DROITE à droite (+y) : {:?} {:?}",
            p2,
            p3
        );
        // liste vide = repli
        assert!(vaisseau_thrusters_with(&[], 1.0, 0.0, Point::new(50.0, 50.0)).is_empty());
    }

    #[test]
    fn thruster_mesh_triangles_are_local_to_the_spawn() {
        // le mesh d'un propulseur (la flamme du gaz) est transformé dans le
        // repère local du vaisseau, pivot posé sur le point local du
        // propulseur : les sommets restent groupés autour de ce point (la
        // flamme fait ~4,3 unités éditeur × son échelle) — prêts pour
        // `draw_thruster_gas` (src/main.rs).
        let pts = vaisseau_thrusters_with(
            VAISSEAU_THRUSTERS,
            1.0,
            0.0,
            Point::new(50.0, 50.0),
        );
        for (t, spawn) in &pts {
            let tris = thruster_mesh_triangles(t, *spawn);
            assert!(!tris.is_empty(), "{} : une flamme attendue", t.name);
            for (a, b, c) in &tris {
                for p in [a, b, c] {
                    let d = (p[0] - spawn.x).hypot(p[1] - spawn.y);
                    assert!(d < 20.0, "{} : sommet à {:.2} du point local", t.name, d);
                }
            }
        }
    }
}
