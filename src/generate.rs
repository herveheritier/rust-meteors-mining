//! Génération des formes et initialisation du jeu.
//!
//! Portage des fonctions de génération de `meteorsMining.bas` :
//! `generateShape`, `createShape`, `createAlien`, `createStation`,
//! `createGem`, `fireBullet` et `prepare`.

use rand::Rng;
use std::f64::consts::TAU;

use crate::config::*;
use crate::geom::{generate_vertex_outside, Point, Triangle};
// génération des météores (taille, triangles, vitesse) et population :
// constantes de la carte « Météores & collisions » de l'outil de gestion
use crate::marketplace::{
    METEOR_VELOCITY_MAX, TRIANGLE_BASE_MAX, TRIANGLE_BASE_MIN, TRIANGLE_HEIGHT_MAX,
    TRIANGLE_HEIGHT_MIN, TRIANGLES_IN_SHAPE_MAX, TRIANGLES_IN_SHAPE_MIN,
};
use crate::shape::*;
use crate::state::{default_elements, Element, GameState};

/// Génère un météore (ex `generateShape`).
///
/// Construit un premier triangle puis ajoute des triangles sur les bords
/// libres (`choose_border_segment`) tant qu'ils sont valides
/// (`is_triangle_valid` et `is_vertex_in_shape`).
///
/// Renvoie l'index de la forme créée (réutilise une forme détruite ayant le
/// même nombre de triangles quand c'est possible).
pub fn generate_shape(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    nbr: usize,
    base_min: i32,
    base_max: i32,
    hauteur_min: i32,
    hauteur_max: i32,
    elements: &[Element],
    rng: &mut impl Rng,
) -> usize {
    let mut shape = Shape::default();

    // cherche une forme détruite réutilisable, sinon en alloue une nouvelle
    let (shape_index, reuse) = match free_shape(shapes, nbr) {
        Some(idx) => {
            shape = shapes[idx].clone();
            (idx, true)
        }
        None => {
            shapes.push(Shape::default());
            (shapes.len() - 1, false)
        }
    };

    // premier triangle
    let mut t = Triangle::default();
    t.generate(base_min, base_max, hauteur_min, hauteur_max, rng);
    t.shape_index = shape_index as i32;
    if reuse {
        t.id = shape.first_triangle as i32;
        triangles[shape.first_triangle] = t;
        shape.last_triangle = shape.first_triangle;
    } else {
        triangles.push(Triangle::default());
        let idx = triangles.len() - 1;
        t.id = idx as i32;
        triangles[idx] = t;
        shape.first_triangle = idx;
        shape.last_triangle = idx;
    }
    shape.id = shape_index as i32;
    shape.border_mask = 0;
    shape.border_len = 3;
    shape.shape_color = argb32(
        64,
        (127.0 + rng.gen::<f64>() * 128.0) as u32,
        (127.0 + rng.gen::<f64>() * 128.0) as u32,
        (127.0 + rng.gen::<f64>() * 128.0) as u32,
    );

    // triangles supplémentaires sur les bords libres
    let mut nbr = nbr;
    while nbr > 1 {
        let bs = choose_border_segment(&mut shape, rng);
        let p = bs % 3;
        let i = bs / 3;
        let tri = &triangles[shape.first_triangle + i];
        let (a, b, c) = (tri.a, tri.b, tri.c);
        let (pt1, pt2, pt3) = match p {
            0 => (a, b, c),
            1 => (b, c, a),
            _ => (c, a, b),
        };

        // tente (20 fois max) de placer un sommet hors de la forme
        let mut cnt = 20;
        let mut pt0;
        loop {
            cnt -= 1;
            pt0 = generate_vertex_outside(
                pt1,
                pt2,
                pt3,
                hauteur_max as f64 - rng.gen::<f64>() * (hauteur_max as f64 - hauteur_min as f64),
            );
            if !is_vertex_in_shape(&shape, triangles, pt0) || cnt <= 0 {
                break;
            }
        }

        let mut t = Triangle::default();
        t.create(pt1, pt2, pt0);
        if is_triangle_valid(&shape, triangles, &t) {
            // 15 % des triangles contiennent un élément minéral
            if rng.gen::<f64>() > 0.85 {
                // ex `int(rnd * (ubound(elements) + 1))` : ubound = len-1, donc
                // valeurs 0..len-1 (0 = pas d'élément)
                t.element = (rng.gen::<f64>() * elements.len() as f64) as i32;
            }
            t.shape_index = shape_index as i32;
            if reuse {
                shape.last_triangle += 1;
                t.id = shape.last_triangle as i32;
                triangles[shape.last_triangle] = t;
            } else {
                triangles.push(Triangle::default());
                let idx = triangles.len() - 1;
                t.id = idx as i32;
                triangles[idx] = t;
                shape.last_triangle = idx;
            }
            // arête commune au parent marquée utilisée : ajoute « 100 » au bitmask
            shape.border_mask |= 1 << shape.border_len;
            shape.border_len += 3;
        }
        nbr -= 1;
    }

    shape.life = (shape.last_triangle - shape.first_triangle + 1) as i32;
    shapes[shape_index] = shape;

    shape_index
}

/// Crée un météore à une position hors de la vue (ex `createShape`).
pub fn create_shape(
    state: &GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    camera: Point,
    elements: &[Element],
    rng: &mut impl Rng,
) -> usize {
    let nbr = (TRIANGLES_IN_SHAPE_MIN as f64
        + (TRIANGLES_IN_SHAPE_MAX - TRIANGLES_IN_SHAPE_MIN) as f64 * rng.gen::<f64>())
        as usize;
    let shape_index = generate_shape(
        shapes,
        triangles,
        nbr,
        TRIANGLE_BASE_MIN,
        TRIANGLE_BASE_MAX,
        TRIANGLE_HEIGHT_MIN,
        TRIANGLE_HEIGHT_MAX,
        elements,
        rng,
    );

    // position aléatoire dans le monde, hors de la vue actuelle
    let (x, y) = loop {
        let x = WORLD_WIDTH * rng.gen::<f64>() + WORLD_MINX;
        let y = WORLD_HEIGHT * rng.gen::<f64>() + WORLD_MINY;
        let mut p = Point::new(x + camera.x, y + camera.y);
        p.normalize_world(&state.world);
        let inside_view =
            (p.x > 0.0 && p.x < VIEWPORT_WIDTH) || (p.y > 0.0 && p.y < VIEWPORT_HEIGHT);
        if !inside_view {
            break (x, y);
        }
    };

    let shape = &mut shapes[shape_index];
    shape.who_i_am = WHOIAM_METEOR;
    shape.is_collider = true;
    shape.position = Point::new(x, y);
    shape.direction = TAU * rng.gen::<f64>();
    shape.velocity = METEOR_VELOCITY_MAX * rng.gen::<f64>();
    shape.orientation = 0.0;
    shape.rotation = 0.01 - 0.02 * rng.gen::<f64>();
    shape.texture = TEXTURE_METEOR;
    // minerais contenus : un par triangle minéralisé (or/fer/eau) — la
    // quantité libérée en gemmes si le météore est détruit par la collision
    // d'un autre météore (voir `release_meteor_minerals`)
    shape.minerals = (shape.first_triangle..=shape.last_triangle)
        .filter(|&i| triangles[i].element > 0)
        .count() as i32;
    compute_shape_center(shape, triangles);

    shape_index
}

/// Libère les minerais d'un météore détruit par la collision d'un autre
/// météore : une gemme par unité de minerai, à la position du météore (ex
/// `createGem` en boucle). Appelé par `game.rs` quand un météore meurt sous
/// un autre météore — le minerai n'est alors pas perdu (les triangles
/// minéralisés détruits par collision ne donnent pas de gemme, contrairement
/// à ceux détruits par une balle).
pub fn release_meteor_minerals(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &[Element],
    meteor_index: usize,
    rng: &mut impl Rng,
) {
    let minerals = shapes[meteor_index].minerals;
    if minerals <= 0 {
        return;
    }
    shapes[meteor_index].minerals = 0;
    let center = shapes[meteor_index].center;
    for _ in 0..minerals {
        // une gemme par unité de minerai, à la position du météore : on
        // fabrique un triangle source factice (élément aléatoire) pour
        // réutiliser `create_gem` — `center = centre du météore` fait
        // tomber la gemme sur la position du météore (rotation autour de
        // lui-même = lui-même)
        let mut source = Triangle::default();
        source.element = 1 + (rng.gen::<f64>() * 3.0) as i32; // 1..=3 (or/fer/eau)
        source.shape_index = meteor_index as i32;
        source.center = center;
        create_gem(shapes, triangles, elements, &source, rng);
    }
}

/// Crée un alien (touche C, ex `createAlien`).
pub fn create_alien(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) {
    let mut shape = Shape::default();
    let _idx = meshes_to_shape(&mut shape, shapes, triangles, ALIEN_MESH);
    resize_shape(1.0 / 5.0, &mut shape, triangles);
    shape.who_i_am = WHOIAM_ALIEN;
    shape.is_collider = true;
    shape.show_all_parts = true;
    shape.shape_color = 0x80FFFF00;
    shape.position = Point::new(100.0, 100.0);
    shape.direction = 0.0;
    shape.velocity = 1.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.center = Point::new(0.0, 0.0);
    shape.target_center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    let id = shape.id as usize;
    shapes[id] = shape;
    compute_shape_center(&mut shapes[id], triangles);
}

/// Crée la station au centre du monde (ex `createStation`).
pub fn create_station(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) {
    let mut shape = Shape::default();
    let idx = meshes_to_shape(&mut shape, shapes, triangles, STATION_MESH);
    resize_shape(1.0, &mut shape, triangles);
    shape.who_i_am = WHOIAM_STATION;
    shape.is_collider = true;
    shape.shape_color = 0xFF808000;
    shape.texture = TEXTURE_STATION;
    shape.position = Point::new(0.0, 0.0);
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shapes[idx] = shape;
    // NB : l'original recalcule (par bug) le centre du joueur `shapes[0]` ici ;
    // l'intention est clairement de calculer celui de la station (résultat
    // identique : la station est centrée sur (0,0)) — on le fait explicitement.
    compute_shape_center(&mut shapes[idx], triangles);
    // NB dérive volontaire de l'original : celui-ci forçait `radius = 36`,
    // bien plus petit que l'anneau visible (r ≈ 110-162). Le pré-filtre de
    // proximité de `game.rs` (`sum_radius`) laissait alors les météores
    // traverser l'anneau sans aucun test. On garde le rayon calculé (la
    // géométrie réelle) : c'est la détection de collision par triangles
    // (SAT) qui décide de la collision avec la base.
}

/// Crée une gemme à partir d'un triangle détruit (ex `createGem`).
pub fn create_gem(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &[Element],
    source_triangle: &Triangle,
    rng: &mut impl Rng,
) {
    let mut shape = Shape::default();
    let _idx = meshes_to_shape(&mut shape, shapes, triangles, GEM_MESH);
    for i in shape.first_triangle..=shape.last_triangle {
        triangles[i].element = source_triangle.element;
    }
    shape.who_i_am = WHOIAM_GEM;
    shape.is_collider = true;
    shape.element = source_triangle.element;
    // gemme libérée par un météore détruit : reste absorbable par un autre
    // météore (rejette explicitement le drapeau — le slot peut être réutilisé)
    shape.ejected_cargo = false;
    if shape.element < 1 || shape.element as usize >= elements.len() {
        eprintln!("createGem: element hors limites: {}", shape.element);
    } else {
        shape.shape_color = elements[shape.element as usize].color;
    }
    let source_shape_index = source_triangle.shape_index as usize;
    shape.life = (shape.last_triangle - shape.first_triangle + 1) as i32;
    // position = centre local du triangle source, tourné puis translaté
    let mut center = source_triangle.center;
    center.rotate_around(shapes[source_shape_index].center, shapes[source_shape_index].orientation);
    shape.position = Point::new(
        center.x + shapes[source_shape_index].position.x,
        center.y + shapes[source_shape_index].position.y,
    );
    shape.direction =
        shapes[source_shape_index].direction + rng.gen::<f64>() * TAU / 4.0 - TAU / 8.0;
    shape.velocity = shapes[source_shape_index].velocity * rng.gen::<f64>() * 2.0 - 1.0;
    shape.orientation = shapes[source_shape_index].orientation;
    shape.rotation = shapes[source_shape_index].rotation;
    shape.center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    let id = shape.id as usize;
    shapes[id] = shape;
}

/// Crée une gemme d'élément donné à une **position imposée** (ex éjection de
/// la soute du vaisseau détruit — `eject_cargo_gems`) : même mesh, même
/// élément et même couleur qu'`create_gem`, mais position, direction et
/// vitesse données (au lieu d'être dérivées du triangle source).
fn create_gem_at(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &[Element],
    element: i32,
    position: Point,
    direction: f64,
    velocity: f64,
) {
    let mut shape = Shape::default();
    let _idx = meshes_to_shape(&mut shape, shapes, triangles, GEM_MESH);
    for i in shape.first_triangle..=shape.last_triangle {
        triangles[i].element = element;
    }
    shape.who_i_am = WHOIAM_GEM;
    shape.is_collider = true;
    shape.element = element;
    // gemme de soute (rejetée au crash) : les météores ne l'absorbent pas —
    // elle doit rester ramassable par le cosmonaute / le vaisseau ressuscité
    shape.ejected_cargo = true;
    if element < 1 || element as usize >= elements.len() {
        eprintln!("createGem: element hors limites: {}", element);
    } else {
        shape.shape_color = elements[element as usize].color;
    }
    shape.life = (shape.last_triangle - shape.first_triangle + 1) as i32;
    shape.position = position;
    shape.direction = direction;
    shape.velocity = velocity;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    let id = shape.id as usize;
    shapes[id] = shape;
}

/// Le vaisseau vient d'être détruit : les minerais collectés dans la soute
/// sont **rejetés autour** du crash — une gemme par minerai, éparpillée dans
/// un cercle autour du vaisseau détruit (rayon `CARGO_EJECT_SPREAD`) avec une
/// petite vitesse de dérive aléatoire — et la soute est vidée. Le cosmonaute
/// EVA (jeu libre/Progression) ou le vaisseau ressuscité (Survival) peuvent
/// les ramasser à nouveau : le minerai n'est pas perdu avec le vaisseau.
/// Sans effet quand la soute est vide. Appelé par `game.rs` quand le
/// vaisseau meurt (tous scénarios).
pub fn eject_cargo_gems(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &mut [Element],
    rng: &mut impl Rng,
) {
    if state.player.cargo_qty <= 0 {
        return;
    }
    // position du crash : le vaisseau détruit reste sur place (triangles
    // morts) — les gemmes jaillissent autour de lui
    let crash = shapes[PLAYER_INDEX].position;
    for e in 1..elements.len() {
        let count = elements[e].count;
        elements[e].count = 0;
        for _ in 0..count {
            // position éparpillée dans un cercle autour du crash + petite
            // vitesse de dérive : le chargement « se renverse » autour du
            // vaisseau détruit
            let ang = rng.gen::<f64>() * TAU;
            let dist = CARGO_EJECT_SPREAD * rng.gen::<f64>();
            let pos = Point::new(crash.x + ang.cos() * dist, crash.y + ang.sin() * dist);
            create_gem_at(
                shapes,
                triangles,
                elements,
                e as i32,
                pos,
                rng.gen::<f64>() * TAU,            // direction de dérive aléatoire
                0.15 + 0.35 * rng.gen::<f64>(), // vitesse : lent (facile à ramasser)
            );
        }
    }
    state.player.cargo_qty = 0;
}

/// Tire une balle par emplacement de tir (`VAISSEAU_BULLET_SPAWNS` — les
/// positions en % de la boîte englobante sont converties en points locaux du
/// vaisseau par `vaisseau::vaisseau_bullet_spawns`, puis tournées avec le
/// vaisseau autour de son centre). Liste vide = un seul emplacement au centre
/// de rotation (comportement d'origine : ex `fireBullet`).
///
/// **Catalogue d'armes** (`VAISSEAU_WEAPONS`) : quand il est rempli, chaque
/// arme tire sa propre munition (mesh embarqué, échelle et orientation) depuis
/// son emplacement sur le vaisseau (`spawn_index` dans `VAISSEAU_BULLET_SPAWNS`
/// — liste contrainte) ; toutes les armes tirent ensemble. Catalogue vide =
/// tir classique (une balle rouge par emplacement, repli).
pub fn fire_bullet(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) {
    // cinématiques du vaisseau figées avant la boucle : `create_specific_shape`
    // / `create_ammo_shape` peuvent allouer dans `shapes` (le joueur reste à
    // l'index 0, mais un emprunt concurrent serait invalide)
    let px = shapes[PLAYER_INDEX].position.x;
    let py = shapes[PLAYER_INDEX].position.y;
    let cx = shapes[PLAYER_INDEX].center.x;
    let cy = shapes[PLAYER_INDEX].center.y;
    let orientation = shapes[PLAYER_INDEX].orientation;
    let velocity = shapes[PLAYER_INDEX].velocity;
    // catalogue d'armes : une munition par arme, depuis son emplacement
    let weapons = crate::vaisseau::vaisseau_weapons();
    if !weapons.is_empty() {
        fire_bullet_with(
            shapes,
            triangles,
            &weapons,
            px,
            py,
            cx,
            cy,
            orientation,
            velocity,
        );
        return;
    }
    for spawn in crate::vaisseau::vaisseau_bullet_spawns() {
        // point local du vaisseau → position monde : tourné autour du centre
        // (comme les sommets du mesh, `compute_real_positions`) puis translaté
        let mut local = spawn;
        local.rotate_around(Point::new(cx, cy), orientation);
        let mut shape = Shape::default();
        let _idx = create_specific_shape(&mut shape, shapes, triangles, BULLET_POINTS);
        shape.who_i_am = WHOIAM_BULLET;
        shape.is_collider = true;
        shape.shape_color = 0xFFFF0000;
        shape.position = Point::new(px + local.x, py + local.y);
        shape.direction = -orientation;
        shape.velocity = velocity + 2.0;
        shape.orientation = orientation;
        shape.rotation = 0.0;
        shape.center = Point::new(0.0, 0.0);
        shape.radius = 10.0;
        let id = shape.id as usize;
        shapes[id] = shape;
    }
}

/// Variante de `fire_bullet` pour le catalogue d'armes : une munition par
/// arme, depuis le point local de son emplacement (le point est tourné ici
/// autour de `(cx, cy)` de `orientation` — comme les sommets du mesh,
/// `compute_real_positions`). Appelée par `fire_bullet` quand le catalogue
/// est rempli (les tests l'utilisent aussi avec des armes factices).
fn fire_bullet_with(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    weapons: &[(crate::marketplace::VaisseauWeapon, Point)],
    px: f64,
    py: f64,
    cx: f64,
    cy: f64,
    orientation: f64,
    velocity: f64,
) {
    for (weapon, local) in weapons {
        // point local du vaisseau → position monde : tourné autour du centre
        // (comme les sommets du mesh, `compute_real_positions`)
        let mut local = *local;
        local.rotate_around(Point::new(cx, cy), orientation);
        let idx = crate::vaisseau::create_ammo_shape(
            shapes,
            triangles,
            weapon.ammo_mesh,
            weapon.ammo_scale,
            weapon.ammo_orientation_degrees,
        );
        let shape = &mut shapes[idx];
        shape.who_i_am = WHOIAM_BULLET;
        shape.is_collider = true;
        shape.position = Point::new(px + local.x, py + local.y);
        shape.direction = -orientation;
        shape.velocity = velocity + 2.0;
        shape.orientation = orientation;
        shape.rotation = 0.0;
        shape.center = Point::new(0.0, 0.0);
        shape.radius = shape.radius.max(6.0);
    }
}

/// Initialise le monde : éléments, vaisseau joueur, étoiles, station
/// (ex `prepare`).
pub fn prepare(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    stars: &mut Vec<Point>,
    elements: &mut Vec<Element>,
    rng: &mut impl Rng,
) {
    // éléments (indice 0 factice, 1=GOLD, 2=IRON, 3=WATER)
    *elements = default_elements();

    // vaisseau joueur (shapes[0]) : mesh coloré de `assets/vaisseau.json`
    // (remplace l'ancien triangle texturé `vaisseau.png` — 35 faces, couleur
    // par face, nez vers la droite = orientation 0 du départ à quai). La
    // progression chargée avant `prepare` (niveaux d'atelier) détermine la
    // composition des plans visibles (`create_player_vaisseau` lit l'état).
    crate::vaisseau::create_player_vaisseau(state, shapes, triangles);

    // étoiles : 100 000, positions étirées par leur plan de parallaxe
    // (plan = (i mod 15) + 1, avec i 1-based comme l'original)
    stars.clear();
    stars.reserve(STARS_COUNT);
    for i in 0..STARS_COUNT {
        let plan = ((i as i32 + 1) % STARS_LAYERS) + 1;
        stars.push(Point::new(
            rng.gen::<f64>() * WORLD_WIDTH * plan as f64,
            rng.gen::<f64>() * WORLD_HEIGHT * plan as f64,
        ));
    }

    // station (shapes[1])
    create_station(shapes, triangles);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::rand::SeedableRng;
    use ::rand_chacha::ChaCha12Rng;

    fn seed() -> ChaCha12Rng {
        ChaCha12Rng::seed_from_u64(42)
    }

    fn generate_one(
        rng: &mut impl Rng,
        shapes: &mut Vec<Shape>,
        triangles: &mut Vec<Triangle>,
    ) -> usize {
        let elements = default_elements();
        generate_shape(
            shapes,
            triangles,
            8,
            TRIANGLE_BASE_MIN,
            TRIANGLE_BASE_MAX,
            TRIANGLE_HEIGHT_MIN,
            TRIANGLE_HEIGHT_MAX,
            &elements,
            rng,
        )
    }

    /// Météore de test avec `n` triangles consécutifs et `n` minerais.
    fn test_meteor(n: i32) -> Shape {
        let mut s = Shape::default();
        s.who_i_am = WHOIAM_METEOR;
        s.is_collider = true;
        s.first_triangle = 0;
        s.last_triangle = (n - 1).max(0) as usize;
        s.life = n;
        s.minerals = n;
        s.radius = 10.0;
        s.position = Point::new(0.0, 0.0);
        s
    }

    /// Triangle de test rattaché à la forme `shape_index`, avec un élément
    /// minéral (or) et une position `(x, y)`.
    fn test_mineral_triangle(id: i32, shape_index: i32, x: f64, y: f64) -> Triangle {
        let mut t = Triangle::default();
        t.id = id;
        t.shape_index = shape_index;
        t.element = 1; // GOLD
        t.create(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0));
        t.position = Point::new(x, y);
        t.real_a = Point::new(t.a.x + x, t.a.y + y);
        t.real_b = Point::new(t.b.x + x, t.b.y + y);
        t.real_c = Point::new(t.c.x + x, t.c.y + y);
        t.real_center = Point::new(t.center.x + x, t.center.y + y);
        t
    }

    #[test]
    fn generate_shape_produces_a_valid_shape() {
        let mut rng = seed();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = generate_one(&mut rng, &mut shapes, &mut triangles);

        let shape = &shapes[idx];
        // plage cohérente : life = nombre de triangles vivants
        let n = shape.last_triangle - shape.first_triangle + 1;
        assert_eq!(shape.life as usize, n);
        assert!(n >= 1 && n <= 8);
        for i in shape.first_triangle..=shape.last_triangle {
            assert_eq!(triangles[i].life, 1);
            assert_eq!(triangles[i].shape_index, idx as i32);
        }

        // nb d'arêtes libres = n + 2 (polygone simple triangulé, sans trou)
        get_border_segments(shape, &mut triangles);
        let mut border = 0;
        for i in shape.first_triangle..=shape.last_triangle {
            if triangles[i].a_shape_border {
                border += 1;
            }
            if triangles[i].b_shape_border {
                border += 1;
            }
            if triangles[i].c_shape_border {
                border += 1;
            }
        }
        assert_eq!(border, n + 2);
    }

    #[test]
    fn meteor_creation_counts_mineral_triangles_as_minerals() {
        // un météore créé contient un minerai par triangle minéralisé (or,
        // fer, eau — `element > 0`) : c'est la quantité libérée en gemmes
        // quand deux météores se percutent et se détruisent
        let mut rng = seed();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut state = GameState::new();
        let elements = default_elements();
        let idx = create_shape(&state, &mut shapes, &mut triangles, Point::new(0.0, 0.0), &elements, &mut rng);

        let minerals = (shapes[idx].first_triangle..=shapes[idx].last_triangle)
            .filter(|&i| triangles[i].element > 0)
            .count() as i32;
        assert_eq!(shapes[idx].minerals, minerals);
        assert_eq!(shapes[idx].who_i_am, WHOIAM_METEOR);
    }

    #[test]
    fn release_meteor_minerals_spawns_one_gem_per_mineral() {
        // un météore détruit par un autre météore libère ses minerais : une
        // gemme par unité, à sa position, et son compteur passe à 0
        let mut rng = seed();
        let mut shapes = vec![test_meteor(3)]; // 3 minerais, 3 triangles
        let mut triangles = Vec::new();
        for i in 0..3 {
            triangles.push(test_mineral_triangle(i as i32, 0, i as f64, 0.0));
        }
        let mut elements = default_elements();

        release_meteor_minerals(&mut shapes, &mut triangles, &elements, 0, &mut rng);

        assert_eq!(shapes[0].minerals, 0);
        let gems = shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM).count();
        assert_eq!(gems, 3);
        // chaque gemme a un élément valide (1..=3) et se trouve près du météore
        for s in shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM) {
            assert!((1..=3).contains(&s.element));
            let d = (s.position.x - 0.0).hypot(s.position.y - 0.0);
            assert!(d < 50.0, "gemme trop loin du météore : {d}");
        }
    }

    #[test]
    fn release_meteor_minerals_without_minerals_is_a_noop() {
        // pas de minerai → aucune gemme, compteur inchangé (rien à libérer)
        let mut rng = seed();
        let mut shapes = vec![test_meteor(0)];
        let mut triangles = Vec::new();
        let mut elements = default_elements();

        release_meteor_minerals(&mut shapes, &mut triangles, &elements, 0, &mut rng);

        assert_eq!(shapes[0].minerals, 0);
        assert!(!shapes.iter().any(|s| s.who_i_am == WHOIAM_GEM));
    }

    #[test]
    fn eject_cargo_gems_spawns_one_gem_per_mineral_around_the_crash() {
        // le vaisseau est détruit : les minerais de la soute sont rejetés en
        // gemmes éparpillées autour du crash et la soute est vidée (le
        // cosmonaute EVA ou le vaisseau ressuscité pourront les ramasser)
        let mut rng = seed();
        // vaisseau joueur (index 0) au point du crash
        let mut player = Shape::default();
        player.who_i_am = WHOIAM_PLAYER;
        player.position = Point::new(100.0, 100.0);
        let mut shapes = vec![player];
        let mut triangles = Vec::new();
        let mut elements = default_elements();
        elements[1].count = 2; // GOLD ×2
        elements[2].count = 1; // IRON ×1
        elements[3].count = 1; // WATER ×1
        let mut state = GameState::new();
        state.player.cargo_qty = 4;

        eject_cargo_gems(&mut state, &mut shapes, &mut triangles, &mut elements, &mut rng);

        assert_eq!(state.player.cargo_qty, 0, "la soute doit être vidée");
        assert!(elements.iter().all(|e| e.count == 0), "les compteurs doivent être remis à zéro");
        let gems: Vec<&Shape> = shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM).collect();
        assert_eq!(gems.len(), 4, "une gemme par minerai de la soute");
        // répartition par élément conservée (2 or, 1 fer, 1 eau) et gemmes
        // éparpillées dans le cercle de rayon CARGO_EJECT_SPREAD autour du crash
        let mut gold = 0;
        for s in &gems {
            assert!((1..=3).contains(&s.element), "élément de gemme invalide");
            if s.element == 1 {
                gold += 1;
            }
            let d = (s.position.x - 100.0).hypot(s.position.y - 100.0);
            assert!(d < CARGO_EJECT_SPREAD + 1.0, "gemme trop loin du crash : {d}");
        }
        assert_eq!(gold, 2);
    }

    #[test]
    fn eject_cargo_gems_with_empty_cargo_is_a_noop() {
        // soute vide → aucune gemme rejetée, rien ne change
        let mut rng = seed();
        let mut player = Shape::default();
        player.who_i_am = WHOIAM_PLAYER;
        player.position = Point::new(100.0, 100.0);
        let mut shapes = vec![player];
        let mut triangles = Vec::new();
        let mut elements = default_elements();
        let mut state = GameState::new();

        eject_cargo_gems(&mut state, &mut shapes, &mut triangles, &mut elements, &mut rng);

        assert_eq!(state.player.cargo_qty, 0);
        assert!(!shapes.iter().any(|s| s.who_i_am == WHOIAM_GEM));
    }

    #[test]
    fn generate_shape_is_deterministic_with_fixed_seed() {
        let elements = default_elements();

        let mut rng1 = seed();
        let mut shapes1 = Vec::new();
        let mut triangles1 = Vec::new();
        generate_shape(
            &mut shapes1,
            &mut triangles1,
            8,
            TRIANGLE_BASE_MIN,
            TRIANGLE_BASE_MAX,
            TRIANGLE_HEIGHT_MIN,
            TRIANGLE_HEIGHT_MAX,
            &elements,
            &mut rng1,
        );

        let mut rng2 = seed();
        let mut shapes2 = Vec::new();
        let mut triangles2 = Vec::new();
        generate_shape(
            &mut shapes2,
            &mut triangles2,
            8,
            TRIANGLE_BASE_MIN,
            TRIANGLE_BASE_MAX,
            TRIANGLE_HEIGHT_MIN,
            TRIANGLE_HEIGHT_MAX,
            &elements,
            &mut rng2,
        );

        let s1 = &shapes1[0];
        let s2 = &shapes2[0];
        assert_eq!(s1.first_triangle, s2.first_triangle);
        assert_eq!(s1.last_triangle, s2.last_triangle);
        for i in s1.first_triangle..=s1.last_triangle {
            assert_eq!(triangles2[i].a, triangles1[i].a);
            assert_eq!(triangles2[i].b, triangles1[i].b);
            assert_eq!(triangles2[i].c, triangles1[i].c);
            assert_eq!(triangles2[i].element, triangles1[i].element);
        }
    }

    #[test]
    fn prepare_builds_player_station_stars_elements() {
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut stars = Vec::new();
        let mut elements = Vec::new();
        let mut rng = seed();
        prepare(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut stars,
            &mut elements,
            &mut rng,
        );

        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes[PLAYER_INDEX].who_i_am, WHOIAM_PLAYER);
        // vaisseau mesh : une Triangle vivante par face **visible** aux
        // niveaux courants (les plans liés aux upgrades n'apparaissent qu'à
        // partir de leur niveau) ; la plage allouée, elle, couvre la
        // composition maximale (`vaisseau_face_count`)
        let player_faces = crate::vaisseau::vaisseau_visible_face_count(&state);
        assert_eq!(shapes[PLAYER_INDEX].life as usize, player_faces);
        assert_eq!(shapes[STATION_INDEX].who_i_am, WHOIAM_STATION);
        // joueur (plage maximale) + station (66 emplacements)
        assert_eq!(triangles.len(), crate::vaisseau::vaisseau_face_count() + 66);
        // le rayon de la station couvre l'anneau visible (r ≈ 110-162) : la
        // collision est décidée par la détection de triangles (SAT), pas par
        // un petit rayon forcé (dérive volontaire — voir `create_station`).
        assert!(
            shapes[STATION_INDEX].radius >= 160.0,
            "rayon de la station {} trop petit pour couvrir l'anneau",
            shapes[STATION_INDEX].radius
        );
        assert_eq!(stars.len(), STARS_COUNT);
        assert_eq!(elements.len(), 4);
        // positions initiales
        assert_eq!(shapes[PLAYER_INDEX].position, Point::new(0.0, 0.0));
        assert_eq!(shapes[STATION_INDEX].position, Point::new(0.0, 0.0));
    }

    #[test]
    fn fire_bullet_fires_one_bullet_per_spawn_rotated_with_ship() {
        // une balle par emplacement de tir (`VAISSEAU_BULLET_SPAWNS` — la
        // liste générée fait foi, 1 seule balle quand elle est vide), tournée
        // avec l'orientation du vaisseau (catalogue d'armes vide : tir classique)
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let pivot = shapes[PLAYER_INDEX].target_center;
        let spawns = crate::vaisseau::vaisseau_bullet_spawns();
        let bullets_before = shapes.len();
        fire_bullet(&mut shapes, &mut triangles);
        assert_eq!(shapes.len(), bullets_before + spawns.len());
        // toutes les balles sont des projectiles (WHOIAM_BULLET) et la
        // première part du centre de rotation quand la liste est vide
        for b in &shapes[bullets_before..] {
            assert_eq!(b.who_i_am, WHOIAM_BULLET);
        }
        if spawns.is_empty() {
            // repli : la balle part du pivot (le vaisseau est à l'origine)
            let bullet = &shapes[bullets_before];
            assert!(
                (bullet.position.x - pivot.x).abs() < 1e-9
                    && (bullet.position.y - pivot.y).abs() < 1e-9,
                "liste vide → tir au centre de rotation, position {:?} (pivot {:?})",
                bullet.position,
                pivot
            );
        }
    }

    #[test]
    fn fire_bullet_with_weapons_fires_one_ammo_per_weapon_at_its_spawn() {
        // catalogue d'armes : une munition par arme, depuis l'emplacement de
        // l'arme (point local tourné avec le vaisseau) — mesh de la munition
        // à la place de la balle rouge
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        // deux armes : une au nez (90 %, 50 % → +x) et une à l'arrière
        // (10 %, 50 % → −x) — meshes de munition à 2 faces colorées
        let ammo = crate::vaisseau::vaisseau_test_ammo_mesh();
        let mut weapons = crate::vaisseau::vaisseau_test_weapons();
        for w in weapons.iter_mut() {
            w.ammo_mesh = ammo;
        }
        let mut locals = crate::vaisseau::vaisseau_test_weapon_locals();
        let cx = shapes[PLAYER_INDEX].center.x;
        let cy = shapes[PLAYER_INDEX].center.y;
        let orientation = shapes[PLAYER_INDEX].orientation;
        for local in locals.iter_mut() {
            local.rotate_around(Point::new(cx, cy), orientation);
        }
        let expected_locals = locals.clone();
        let weapons: Vec<(crate::marketplace::VaisseauWeapon, Point)> =
            weapons.into_iter().zip(locals).collect();
        let before = shapes.len();
        let px = shapes[PLAYER_INDEX].position.x;
        let py = shapes[PLAYER_INDEX].position.y;
        fire_bullet_with(
            &mut shapes,
            &mut triangles,
            &weapons,
            px,
            py,
            cx,
            cy,
            orientation,
            0.0,
        );
        // une munition par arme (2 armes → 2 formes), chacune à son
        // emplacement tourné — et chaque munition porte ses faces colorées
        let spawned: Vec<&Shape> = shapes[before..].iter().collect();
        assert_eq!(spawned.len(), 2);
        for (i, s) in spawned.iter().enumerate() {
            assert_eq!(s.who_i_am, WHOIAM_BULLET);
            assert_eq!(s.life, 2, "munition à 2 faces");
            let expected = expected_locals[i];
            assert!(
                (s.position.x - (px + expected.x)).abs() < 1e-6
                    && (s.position.y - (py + expected.y)).abs() < 1e-6,
                "munition {} : position {:?} ≠ attendue {:?}",
                i,
                s.position,
                Point::new(px + expected.x, py + expected.y)
            );
            // couleurs par face (la munition n'est pas une balle rouge unie)
            let colors: std::collections::HashSet<u32> = (s.first_triangle..=s.last_triangle)
                .filter(|&i| triangles[i].life > 0)
                .map(|i| triangles[i].color)
                .collect();
            assert_eq!(colors.len(), 2, "2 couleurs de faces distinctes");
        }
    }
}
