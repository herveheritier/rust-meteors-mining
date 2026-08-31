//! Génération des formes et initialisation du jeu.
//!
//! Portage des fonctions de génération de `meteorsMining.bas` :
//! `generateShape`, `createShape`, `createAlien`, `createStation`,
//! `createMineral`, `fireBullet` et `prepare`.

use ::rand::SeedableRng;
use ::rand_chacha::ChaCha12Rng;
use rand::Rng;
use std::f64::consts::TAU;

use crate::config::*;
use crate::geom::{generate_vertex_outside, Point, Triangle};
// génération des météores (taille, triangles, vitesse) et population :
// constantes de la carte « Météores & collisions » de l'outil de gestion
use crate::marketplace::{
    METEOR_SPIN_BASE, METEOR_SPIN_MAX, METEOR_VELOCITY_MAX, TRIANGLE_BASE_MAX,
    TRIANGLE_BASE_MIN, TRIANGLE_HEIGHT_MAX, TRIANGLE_HEIGHT_MIN, TRIANGLES_IN_SHAPE_MIN,
};
use crate::shape::*;
use crate::state::{default_elements, Element, GameState};

/// PRNG seedé pour la génération procédurale (`prepare`, étoiles…).
///
/// **Natif** : entropie système (`getrandom`). **Web** (wasm32-unknown-
/// unknown) : getrandom y exige la glue wasm-bindgen (`__wbg_*`) que le
/// chargeur web miniquad (`web/gl.js`) ne fournit pas - instancier le wasm
/// échouerait (`WebAssembly.instantiate: Import "__wbindgen_placeholder__"`,
/// voir le déploiement GitHub Pages). On seede donc depuis l'horloge
/// (`get_time`, ms) XOR un compteur : entropie largement suffisante pour un
/// monde de jeu, et le binaire wasm ne référence plus getrandom du tout.
pub fn seeded_rng() -> ChaCha12Rng {
    #[cfg(not(target_arch = "wasm32"))]
    {
        ChaCha12Rng::from_entropy()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // horloge + compteur : jamais le même départ deux fois dans la session
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEED_CTR: AtomicU64 = AtomicU64::new(0);
        let ctr = SEED_CTR.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        let ms = (macroquad::time::get_time() * 1000.0) as u64;
        ChaCha12Rng::seed_from_u64(ms ^ ctr)
    }
}

/// Génère un météore (ex `generateShape`).
///
/// Construit un premier triangle puis ajoute des triangles sur les bords
/// libres (`choose_border_segment`) tant qu'ils sont valides
/// (`is_triangle_valid` et `is_vertex_in_shape`).
///
/// Renvoie l'index de la forme créée (réutilise une forme détruite ayant le
/// même nombre de triangles quand c'est possible).
// Signature historique (les paramètres de génération sont nombreux) :
// `#[allow]` ciblé plutôt qu'un regroupement en struct.
#[allow(clippy::too_many_arguments)]
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
        (127.0 + rng.r#gen::<f64>() * 128.0) as u32,
        (127.0 + rng.r#gen::<f64>() * 128.0) as u32,
        (127.0 + rng.r#gen::<f64>() * 128.0) as u32,
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
                hauteur_max as f64 - rng.r#gen::<f64>() * (hauteur_max as f64 - hauteur_min as f64),
            );
            if !is_vertex_in_shape(&shape, triangles, pt0) || cnt <= 0 {
                break;
            }
        }

        let mut t = Triangle::default();
        t.create(pt1, pt2, pt0);
        if is_triangle_valid(&shape, triangles, &t) {
            // 15 % des triangles contiennent un élément minéral
            if rng.r#gen::<f64>() > 0.85 {
                // ex `int(rnd * (ubound(elements) + 1))` : ubound = len-1, donc
                // valeurs 0..len-1 (0 = pas d'élément)
                t.element = (rng.r#gen::<f64>() * elements.len() as f64) as i32;
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

/// Vitesse de rotation d'un météore (rad/frame à 60 fps) : inversement
/// proportionnelle à la taille (nombre de triangles), plafonnée à
/// `METEOR_SPIN_MAX`. Un petit éclat tourne vite, un gros astéroïde avec
/// langueur (comportement réaliste des débris).
/// NB : le signe n'est pas géré ici — l'appelant multiplie par un ±1 aléatoire.
///
/// Remplace l'ancien 0.01–0.02 rad/frame fixe de l'original, sans lien avec
/// la taille.
pub fn meteor_spin(nbr: usize) -> f64 {
    if nbr == 0 {
        return 0.0;
    }
    (METEOR_SPIN_BASE * TRIANGLES_IN_SHAPE_MIN as f64 / nbr as f64).min(METEOR_SPIN_MAX)
}

/// Crée un météore à une position hors de la vue (ex `createShape`).
///
/// **Difficulté adaptative** (`difficulty.rs`) : au fil de la session, le
/// nombre de triangles (taille) et la vitesse maximale des météores
/// augmentent progressivement (vagues progressives).
pub fn create_shape(
    state: &GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    camera: Point,
    elements: &[Element],
    rng: &mut impl Rng,
) -> usize {
    let nbr = crate::difficulty::triangle_count(state, rng);
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
    let (x, y) = random_world_position(state, camera, rng);

    let shape = &mut shapes[shape_index];
    shape.who_i_am = WHOIAM_METEOR;
    shape.is_collider = true;
    shape.position = Point::new(x, y);
    shape.direction = TAU * rng.r#gen::<f64>();
    shape.velocity = crate::difficulty::meteor_velocity_max(state) * rng.r#gen::<f64>();
    shape.orientation = 0.0;
    // Tournoiement : vitesse de rotation inversement proportionnelle à la
    // taille (voir `meteor_spin`), signe aléatoire.
    shape.rotation = meteor_spin(nbr) * (1.0 - 2.0 * rng.r#gen::<f64>());
    shape.texture = TEXTURE_METEOR;
    shape.is_boss = false;
    // minerais contenus : un par triangle minéralisé (or/fer/eau) - la
    // quantité libérée en minerais si le météore est détruit par la collision
    // d'un autre météore (voir `release_meteor_minerals`)
    shape.minerals = (shape.first_triangle..=shape.last_triangle)
        .filter(|&i| triangles[i].element > 0)
        .count() as i32;
    compute_shape_center(shape, triangles);

    shape_index
}

/// Position aléatoire dans le monde, **hors de la vue actuelle** (ex la
/// boucle de positionnement de `createShape`) - partagée par les météores,
/// le météore spécial et les portails.
pub fn random_world_position(state: &GameState, camera: Point, rng: &mut impl Rng) -> (f64, f64) {
    loop {
        let x = WORLD_WIDTH * rng.r#gen::<f64>() + WORLD_MINX;
        let y = WORLD_HEIGHT * rng.r#gen::<f64>() + WORLD_MINY;
        let mut p = Point::new(x + camera.x, y + camera.y);
        p.normalize_world(&state.world);
        let inside_view =
            (p.x > 0.0 && p.x < VIEWPORT_WIDTH) || (p.y > 0.0 && p.y < VIEWPORT_HEIGHT);
        if !inside_view {
            break (x, y);
        }
    }
}

/// Crée le **météore spécial** (boss) : un gros astéroïde de
/// `BOSS_TRIANGLES` triangles (générés avec les bornes maximales), mis à
/// l'échelle `BOSS_SCALE` (plus de résistance : chaque triangle est une
/// « vie »), lent (vitesse réduite, rotation lente - `meteor_spin` plafonne
/// déjà pour les gros corps), avec une forte teneur minérale - y compris du
/// **PLATINUM** (`ELEMENT_PLATINUM`, le minerai rare) sur une partie des
/// triangles. Sa destruction rapporte un bonus de réputation et un
/// éparpillement de minerais (voir `game.rs`). Apparaît périodiquement
/// (`BOSS_SPAWN_INTERVAL`, `game.rs`) - un seul boss vivant à la fois.
pub fn create_boss_meteor(
    state: &GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    camera: Point,
    elements: &[Element],
    rng: &mut impl Rng,
) -> usize {
    let nbr = BOSS_TRIANGLES;
    let shape_index = generate_shape(
        shapes,
        triangles,
        nbr,
        TRIANGLE_BASE_MAX / 2,
        TRIANGLE_BASE_MAX,
        TRIANGLE_HEIGHT_MAX / 2,
        TRIANGLE_HEIGHT_MAX,
        elements,
        rng,
    );
    resize_shape(BOSS_SCALE, &mut shapes[shape_index], triangles);

    let (x, y) = random_world_position(state, camera, rng);
    let shape = &mut shapes[shape_index];
    shape.who_i_am = WHOIAM_METEOR;
    shape.is_collider = true;
    shape.is_boss = true;
    shape.position = Point::new(x, y);
    shape.direction = TAU * rng.r#gen::<f64>();
    shape.velocity = METEOR_VELOCITY_MAX * 0.6 * rng.r#gen::<f64>();
    shape.orientation = 0.0;
    shape.rotation = meteor_spin(nbr) * (1.0 - 2.0 * rng.r#gen::<f64>());
    shape.texture = TEXTURE_METEOR;
    // forte teneur minérale : les triangles générés ont déjà leur élément
    // (15 % par défaut) - on en ajoute sur les triangles restants, avec une
    // part de PLATINUM (1 triangle sur 8)
    let plat = ELEMENT_PLATINUM;
    for tri in triangles
        .iter_mut()
        .take(shape.last_triangle + 1)
        .skip(shape.first_triangle)
    {
        if tri.element <= 0 {
            tri.element = if rng.r#gen::<f64>() < 0.5 {
                plat
            } else {
                1 + (rng.r#gen::<f64>() * 3.0) as i32
            };
        }
    }
    shape.minerals = (shape.first_triangle..=shape.last_triangle)
        .filter(|&i| triangles[i].element > 0)
        .count() as i32;
    compute_shape_center(shape, triangles);

    shape_index
}

/// Crée un **portail de distorsion** (warp gate) à une position hors de la
/// vue : un anneau violet statique, indestructible, qui téléporte le
/// vaisseau qui le percute d'une fraction du monde (`WARP_JUMP_FRACTION`,
/// `game.rs` - le portail est consommé). Les météores rebondissent dessus
/// sans l'endommager (pas de choc élastique - `game.rs`).
pub fn create_warp_gate(
    state: &GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    camera: Point,
    rng: &mut impl Rng,
) -> usize {
    // anneau à 16 côtés (éventail glissant → 14 triangles)
    const GATE_MESH: Mesh = &[&[
        (30.0, 0.0),
        (24.0, 0.0),
        (27.7, -11.5),
        (22.2, -9.2),
        (21.2, -21.2),
        (17.0, -17.0),
        (11.5, -27.7),
        (9.2, -22.2),
        (0.0, -30.0),
        (0.0, -24.0),
        (-11.5, -27.7),
        (-9.2, -22.2),
        (-21.2, -21.2),
        (-17.0, -17.0),
        (-27.7, -11.5),
        (-22.2, -9.2),
        (-30.0, 0.0),
        (-24.0, 0.0),
        (-27.7, 11.5),
        (-22.2, 9.2),
        (-21.2, 21.2),
        (-17.0, 17.0),
        (-11.5, 27.7),
        (-9.2, 22.2),
        (0.0, 30.0),
        (0.0, 24.0),
        (11.5, 27.7),
        (9.2, 22.2),
        (21.2, 21.2),
        (17.0, 17.0),
        (27.7, 11.5),
        (22.2, 9.2),
        (30.0, 0.0),
        (24.0, 0.0),
    ]];
    let mut shape = Shape::default();
    let idx = meshes_to_shape(&mut shape, shapes, triangles, GATE_MESH);
    let (x, y) = random_world_position(state, camera, rng);
    let shape = &mut shapes[idx];
    shape.who_i_am = WHOIAM_WARP_GATE;
    shape.is_collider = true;
    shape.shape_color = 0xFFB04AFF; // violet néon
    shape.position = Point::new(x, y);
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.texture = TEXTURE_NONE;
    compute_shape_center(shape, triangles);
    idx
}

/// Crée une **mine** (consommable fabriqué, posée en vol - touche 3) : un
/// octogone rouge statique qui explose au contact d'un météore, détruisant
/// les triangles dans son rayon (`MINE_RADIUS`, `game.rs`).
pub fn create_mine(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>, position: Point) -> usize {
    const MINE_MESH: Mesh = &[&[
        (12.0, 0.0),
        (8.5, -8.5),
        (0.0, -12.0),
        (-8.5, -8.5),
        (-12.0, 0.0),
        (-8.5, 8.5),
        (0.0, 12.0),
        (8.5, 8.5),
        (12.0, 0.0),
        (8.5, -8.5),
    ]];
    let mut shape = Shape::default();
    let idx = meshes_to_shape(&mut shape, shapes, triangles, MINE_MESH);
    let shape = &mut shapes[idx];
    shape.who_i_am = WHOIAM_MINE;
    shape.is_collider = true;
    shape.shape_color = 0xFFFF5040;
    shape.position = position;
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.texture = TEXTURE_NONE;
    compute_shape_center(shape, triangles);
    idx
}

/// Libère les minerais d'un météore détruit par la collision d'un autre
/// météore : un minerai par unité de minerai, à la position du météore (ex
/// `createMineral` en boucle). Appelé par `game.rs` quand un météore meurt sous
/// un autre météore - le minerai n'est alors pas perdu (les triangles
/// minéralisés détruits par collision ne donnent pas de minerai, contrairement
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
        // un minerai par unité de minerai, à la position du météore : on
        // fabrique un triangle source factice (élément aléatoire) pour
        // réutiliser `create_mineral` - `center = centre du météore` fait
        // tomber le minerai sur la position du météore (rotation autour de
        // lui-même = lui-même)
        let source = Triangle {
            element: 1 + (rng.r#gen::<f64>() * 3.0) as i32, // 1..=3 (or/fer/eau)
            shape_index: meteor_index as i32,
            center,
            ..Triangle::default()
        };
        create_mineral(shapes, triangles, elements, &source, rng);
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
    // identique : la station est centrée sur (0,0)) - on le fait explicitement.
    compute_shape_center(&mut shapes[idx], triangles);
    // NB dérive volontaire de l'original : celui-ci forçait `radius = 36`,
    // bien plus petit que l'anneau visible (r ≈ 110-162). Le pré-filtre de
    // proximité de `game.rs` (`sum_radius`) laissait alors les météores
    // traverser l'anneau sans aucun test. On garde le rayon calculé (la
    // géométrie réelle) : c'est la détection de collision par triangles
    // (SAT) qui décide de la collision avec la base.
}

/// Crée un minerai à partir d.un triangle détruit (ex `createMineral`).
pub fn create_mineral(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &[Element],
    source_triangle: &Triangle,
    rng: &mut impl Rng,
) {
    let mut shape = Shape::default();
    let _idx = meshes_to_shape(&mut shape, shapes, triangles, MINERAL_MESH);
    for t in &mut triangles[shape.first_triangle..=shape.last_triangle] {
        t.element = source_triangle.element;
    }
    shape.who_i_am = WHOIAM_MINERAL;
    shape.is_collider = true;
    shape.element = source_triangle.element;
    // minerai libéré par un météore détruit : reste absorbable par un autre
    // météore (rejette explicitement le drapeau - le slot peut être réutilisé)
    shape.ejected_cargo = false;
    if shape.element < 1 || shape.element as usize >= elements.len() {
        eprintln!("createMineral: element hors limites: {}", shape.element);
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
        shapes[source_shape_index].direction + rng.r#gen::<f64>() * TAU / 4.0 - TAU / 8.0;
    shape.velocity = shapes[source_shape_index].velocity * rng.r#gen::<f64>() * 2.0 - 1.0;
    shape.orientation = shapes[source_shape_index].orientation;
    shape.rotation = shapes[source_shape_index].rotation;
    shape.center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    let id = shape.id as usize;
    shapes[id] = shape;
}

/// Crée un minerai d.élément donné à une **position imposée** (ex éjection de
/// la soute du vaisseau détruit - `eject_cargo_minerals`) : même mesh, même
/// élément et même couleur qu'`create_mineral`, mais position, direction et
/// vitesse données (au lieu d'être dérivées du triangle source).
fn create_mineral_at(
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    elements: &[Element],
    element: i32,
    position: Point,
    direction: f64,
    velocity: f64,
) {
    let mut shape = Shape::default();
    let _idx = meshes_to_shape(&mut shape, shapes, triangles, MINERAL_MESH);
    for t in &mut triangles[shape.first_triangle..=shape.last_triangle] {
        t.element = element;
    }
    shape.who_i_am = WHOIAM_MINERAL;
    shape.is_collider = true;
    shape.element = element;
    // minerai relâché de la soute au crash : marqueur `ejected_cargo` - il
    // suit les règles du monde (absorbé par les météores, ramassé par le
    // vaisseau) mais la station ne le détruit pas (il reste dans l'espace à
    // récupérer au retour du vaisseau reconstruit)
    shape.ejected_cargo = true;
    if element < 1 || element as usize >= elements.len() {
        eprintln!("createMineral: element hors limites: {}", element);
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
/// sont **rejetés autour** du crash - un minerai par unité, éparpillée dans
/// un cercle autour du vaisseau détruit (rayon `CARGO_EJECT_SPREAD`) avec une
/// petite vitesse de dérive aléatoire - et la soute est vidée. Le cosmonaute
/// EVA (jeu libre/Progression) ou le vaisseau ressuscité (Survival) peuvent
/// les ramasser à nouveau : le minerai n'est pas perdu avec le vaisseau.
/// Sans effet quand la soute est vide. Appelé par `game.rs` quand le
/// vaisseau meurt (tous scénarios).
pub fn eject_cargo_minerals(
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
    // morts) - les minerais jaillissent autour de lui
    let crash = shapes[PLAYER_INDEX].position;
    for e in 1..elements.len() {
        let count = elements[e].count;
        elements[e].count = 0;
        for _ in 0..count {
            // position éparpillée dans un cercle autour du crash + petite
            // vitesse de dérive : le chargement « se renverse » autour du
            // vaisseau détruit. La dérive reste **très lente** (2,5-8
            // unités/s) : les minerais s'éparpillent visuellement mais
            // **restent à portée** du vaisseau reconstruit quand le joueur
            // reviendra les récupérer (des minerais qui filent à 30 unités/s
            // deviendraient inrattrapables)
            let ang = rng.r#gen::<f64>() * TAU;
            let dist = CARGO_EJECT_SPREAD * rng.r#gen::<f64>();
            let pos = Point::new(crash.x + ang.cos() * dist, crash.y + ang.sin() * dist);
            create_mineral_at(
                shapes,
                triangles,
                elements,
                e as i32,
                pos,
                rng.r#gen::<f64>() * TAU,          // direction de dérive aléatoire
                0.04 + 0.10 * rng.r#gen::<f64>(), // vitesse : dérive lente (reste à ramasser)
            );
        }
    }
    state.player.cargo_qty = 0;
}

/// Tire une balle par emplacement de tir (`VAISSEAU_BULLET_SPAWNS` - les
/// positions en % de la boîte englobante sont converties en points locaux du
/// vaisseau par `vaisseau::vaisseau_bullet_spawns`, puis tournées avec le
/// vaisseau autour de son centre). Liste vide = un seul emplacement au centre
/// de rotation (comportement d'origine : ex `fireBullet`).
///
/// **Catalogue d'armes** (`VAISSEAU_WEAPONS`) : quand il est rempli, chaque
/// arme **possédée et armée** tire sa propre munition (mesh embarqué, échelle
/// et orientation) depuis son emplacement sur le vaisseau (`spawn_index` dans
/// `VAISSEAU_BULLET_SPAWNS` - liste contrainte). Le masque `fired` (produit
/// par `scenario::try_fire`, index du catalogue borné à `WEAPON_SLOTS`)
/// sélectionne les armes qui tirent : une arme sans munitions (ou non
/// possédée) ne tire pas, les autres continuent. Catalogue vide = tir
/// classique (une balle rouge par emplacement, repli) - il n'a lieu que si le
/// slot 0 (le canon classique) a tiré.
pub fn fire_bullet(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>, fired: &[bool; WEAPON_SLOTS]) {
    // cinématiques du vaisseau figées avant la boucle : `create_specific_shape`
    // / `create_ammo_shape` peuvent allouer dans `shapes` (le joueur reste à
    // l'index 0, mais un emprunt concurrent serait invalide)
    let px = shapes[PLAYER_INDEX].position.x;
    let py = shapes[PLAYER_INDEX].position.y;
    let cx = shapes[PLAYER_INDEX].center.x;
    let cy = shapes[PLAYER_INDEX].center.y;
    let orientation = shapes[PLAYER_INDEX].orientation;
    let velocity = shapes[PLAYER_INDEX].velocity;
    // catalogue d'armes : une munition par arme armée, depuis son emplacement
    let weapons = crate::vaisseau::vaisseau_weapons();
    if !weapons.is_empty() {
        let armed: Vec<(crate::marketplace::VaisseauWeapon, Point)> = weapons
            .into_iter()
            .enumerate()
            .filter(|(i, _)| fired.get(*i).copied().unwrap_or(false))
            .map(|(_, w)| w)
            .collect();
        fire_bullet_with(
            shapes,
            triangles,
            &armed,
            px,
            py,
            cx,
            cy,
            orientation,
            velocity,
        );
        return;
    }
    // tir classique (repli) : une balle par emplacement, seulement si le
    // canon classique (slot 0) a tiré
    if !fired[0] {
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
/// autour de `(cx, cy)` de `orientation` - comme les sommets du mesh,
/// `compute_real_positions`). Appelée par `fire_bullet` quand le catalogue
/// est rempli (les tests l'utilisent aussi avec des armes factices).
#[allow(clippy::too_many_arguments)]
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
    // (remplace l'ancien triangle texturé `vaisseau.png` - 35 faces, couleur
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
            rng.r#gen::<f64>() * WORLD_WIDTH * plan as f64,
            rng.r#gen::<f64>() * WORLD_HEIGHT * plan as f64,
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
        Shape {
            who_i_am: WHOIAM_METEOR,
            is_collider: true,
            first_triangle: 0,
            last_triangle: (n - 1).max(0) as usize,
            life: n,
            minerals: n,
            radius: 10.0,
            position: Point::new(0.0, 0.0),
            ..Shape::default()
        }
    }

    /// Triangle de test rattaché à la forme `shape_index`, avec un élément
    /// minéral (or) et une position `(x, y)`.
    fn test_mineral_triangle(id: i32, shape_index: i32, x: f64, y: f64) -> Triangle {
        let mut t = Triangle {
            id,
            shape_index,
            element: 1, // GOLD
            ..Triangle::default()
        };
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
        assert!((1..=8).contains(&n));
        for t in &triangles[shape.first_triangle..=shape.last_triangle] {
            assert_eq!(t.life, 1);
            assert_eq!(t.shape_index, idx as i32);
        }

        // nb d'arêtes libres = n + 2 (polygone simple triangulé, sans trou)
        get_border_segments(shape, &mut triangles);
        let mut border = 0;
        for t in &triangles[shape.first_triangle..=shape.last_triangle] {
            if t.a_shape_border {
                border += 1;
            }
            if t.b_shape_border {
                border += 1;
            }
            if t.c_shape_border {
                border += 1;
            }
        }
        assert_eq!(border, n + 2);
    }

    #[test]
    fn meteor_creation_counts_mineral_triangles_as_minerals() {
        // un météore créé contient un minerai par triangle minéralisé (or,
        // fer, eau - `element > 0`) : c.est la quantité libérée en minerais
        // quand deux météores se percutent et se détruisent
        let mut rng = seed();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let state = GameState::new();
        let elements = default_elements();
        let idx = create_shape(&state, &mut shapes, &mut triangles, Point::new(0.0, 0.0), &elements, &mut rng);

        let minerals = (shapes[idx].first_triangle..=shapes[idx].last_triangle)
            .filter(|&i| triangles[i].element > 0)
            .count() as i32;
        assert_eq!(shapes[idx].minerals, minerals);
        assert_eq!(shapes[idx].who_i_am, WHOIAM_METEOR);
    }

    #[test]
    fn meteor_spin_is_inversely_proportional_to_size() {
        // Tournoiement : |spin| décroît avec la taille (nbr), plafond METEOR_SPIN_MAX.
        use crate::marketplace::{METEOR_SPIN_BASE, METEOR_SPIN_MAX, TRIANGLES_IN_SHAPE_MIN};
        let small = meteor_spin(TRIANGLES_IN_SHAPE_MIN as usize);
        let big = meteor_spin(200);
        assert!((small - METEOR_SPIN_BASE).abs() < 1e-9);
        assert!(big < METEOR_SPIN_BASE, "gros astéroïde doit tourner plus lentement");
        assert!(small <= METEOR_SPIN_MAX);
        assert!(meteor_spin(0) == 0.0, "nbr=0 → pas de rotation (garde)");
    }

    #[test]
    fn create_shape_applies_spin_with_random_sign() {
        // create_shape applique meteor_spin avec un signe aléatoire :
        // |rotation| dans (0, METEOR_SPIN_MAX]. (Le nombre réel de triangles
        // peut être inférieur au nombre demandé — la génération saute les
        // triangles invalides — d'où l'absence d'égalité exacte.)
        use crate::marketplace::METEOR_SPIN_MAX;
        let elements = default_elements();
        let state = GameState::new();
        let mut rng = seed();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let idx = create_shape(&state, &mut shapes, &mut triangles, Point::new(0.0, 0.0), &elements, &mut rng);
        let rot = shapes[idx].rotation.abs();
        assert!(rot > 0.0, "le météore doit tourner");
        assert!(rot <= METEOR_SPIN_MAX + 1e-9);
    }

    #[test]
    fn meteor_spin_is_deterministic_with_fixed_seed() {
        // même seed → même rotation (génération procédurale reproductible)
        let elements = default_elements();
        let state = GameState::new();
        let mut r1 = seed();
        let mut s1 = Vec::new();
        let mut t1 = Vec::new();
        let i1 = create_shape(&state, &mut s1, &mut t1, Point::new(0.0, 0.0), &elements, &mut r1);
        let mut r2 = seed();
        let mut s2 = Vec::new();
        let mut t2 = Vec::new();
        let i2 = create_shape(&state, &mut s2, &mut t2, Point::new(0.0, 0.0), &elements, &mut r2);
        assert_eq!(s1[i1].rotation, s2[i2].rotation);
    }

    #[test]
    fn release_meteor_minerals_spawns_one_mineral_per_unit() {
        // un météore détruit par un autre météore libère ses minerais : une
        // minerai par unité, à sa position, et son compteur passe à 0
        let mut rng = seed();
        let mut shapes = vec![test_meteor(3)]; // 3 minerais, 3 triangles
        let mut triangles = Vec::new();
        for i in 0..3 {
            triangles.push(test_mineral_triangle(i, 0, i as f64, 0.0));
        }
        let elements = default_elements();

        release_meteor_minerals(&mut shapes, &mut triangles, &elements, 0, &mut rng);

        assert_eq!(shapes[0].minerals, 0);
        let minerals = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).count();
        assert_eq!(minerals, 3);
        // chaque minerai a un élément valide (1..=3) et se trouve près du météore
        for s in shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL) {
            assert!((1..=3).contains(&s.element));
            let d = (s.position.x - 0.0).hypot(s.position.y - 0.0);
            assert!(d < 50.0, "minerai trop loin du météore : {d}");
        }
    }

    #[test]
    fn release_meteor_minerals_without_minerals_is_a_noop() {
        // pas de minerai → aucun minerai, compteur inchangé (rien à libérer)
        let mut rng = seed();
        let mut shapes = vec![test_meteor(0)];
        let mut triangles = Vec::new();
        let elements = default_elements();

        release_meteor_minerals(&mut shapes, &mut triangles, &elements, 0, &mut rng);

        assert_eq!(shapes[0].minerals, 0);
        assert!(!shapes.iter().any(|s| s.who_i_am == WHOIAM_MINERAL));
    }

    #[test]
    fn eject_cargo_minerals_spawns_one_mineral_per_unit_around_the_crash() {
        // le vaisseau est détruit : les minerais de la soute sont rejetés en
        // minerais éparpillés autour du crash et la soute est vidée (le
        // cosmonaute EVA ou le vaisseau ressuscité pourront les ramasser)
        let mut rng = seed();
        // vaisseau joueur (index 0) au point du crash
        let player = Shape {
            who_i_am: WHOIAM_PLAYER,
            position: Point::new(100.0, 100.0),
            ..Shape::default()
        };
        let mut shapes = vec![player];
        let mut triangles = Vec::new();
        let mut elements = default_elements();
        elements[1].count = 2; // GOLD ×2
        elements[2].count = 1; // IRON ×1
        elements[3].count = 1; // WATER ×1
        let mut state = GameState::new();
        state.player.cargo_qty = 4;

        eject_cargo_minerals(&mut state, &mut shapes, &mut triangles, &mut elements, &mut rng);

        assert_eq!(state.player.cargo_qty, 0, "la soute doit être vidée");
        assert!(elements.iter().all(|e| e.count == 0), "les compteurs doivent être remis à zéro");
        let minerals: Vec<&Shape> = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).collect();
        assert_eq!(minerals.len(), 4, "un minerai par unité de la soute");
        // répartition par élément conservée (2 or, 1 fer, 1 eau) et minerais
        // éparpillées dans le cercle de rayon CARGO_EJECT_SPREAD autour du crash
        let mut gold = 0;
        for s in &minerals {
            assert!((1..=3).contains(&s.element), "élément de minerai invalide");
            if s.element == 1 {
                gold += 1;
            }
            let d = (s.position.x - 100.0).hypot(s.position.y - 100.0);
            assert!(d < CARGO_EJECT_SPREAD + 1.0, "minerai trop loin du crash : {d}");
            // dérive lente : les minerais doivent **rester à portée** du
            // cosmonaute EVA (sans frein) - pas de dérive qui les éloigne à
            // 30 unités/s (inrattrapables), juste un « renversement » visuel
            assert!(
                s.velocity <= 0.14,
                "minerai qui dérive trop vite ({} unités/frame) : inrattrapable par le cosmonaute",
                s.velocity
            );
        }
        assert_eq!(gold, 2);
    }

    #[test]
    fn eject_cargo_minerals_with_empty_cargo_is_a_noop() {
        // soute vide → aucun minerai rejeté, rien ne change
        let mut rng = seed();
        let player = Shape {
            who_i_am: WHOIAM_PLAYER,
            position: Point::new(100.0, 100.0),
            ..Shape::default()
        };
        let mut shapes = vec![player];
        let mut triangles = Vec::new();
        let mut elements = default_elements();
        let mut state = GameState::new();

        eject_cargo_minerals(&mut state, &mut shapes, &mut triangles, &mut elements, &mut rng);

        assert_eq!(state.player.cargo_qty, 0);
        assert!(!shapes.iter().any(|s| s.who_i_am == WHOIAM_MINERAL));
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
        // un petit rayon forcé (dérive volontaire - voir `create_station`).
        assert!(
            shapes[STATION_INDEX].radius >= 160.0,
            "rayon de la station {} trop petit pour couvrir l'anneau",
            shapes[STATION_INDEX].radius
        );
        assert_eq!(stars.len(), STARS_COUNT);
        // GOLD, IRON, WATER + PLATINUM (minerai rare du météore spécial)
        assert_eq!(elements.len(), 5);
        // positions initiales
        assert_eq!(shapes[PLAYER_INDEX].position, Point::new(0.0, 0.0));
        assert_eq!(shapes[STATION_INDEX].position, Point::new(0.0, 0.0));
    }

    #[test]
    fn fire_bullet_fires_one_bullet_per_spawn_rotated_with_ship() {
        // une balle par emplacement de tir (`VAISSEAU_BULLET_SPAWNS` - la
        // liste générée fait foi, 1 seule balle quand elle est vide), tournée
        // avec l'orientation du vaisseau (catalogue d'armes vide : tir classique)
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let pivot = shapes[PLAYER_INDEX].target_center;
        let spawns = crate::vaisseau::vaisseau_bullet_spawns();
        let weapons = crate::vaisseau::vaisseau_weapons();
        let bullets_before = shapes.len();
        fire_bullet(&mut shapes, &mut triangles, &[true; WEAPON_SLOTS]);
        // Le catalogue d'armes remplace le tir classique : une munition par
        // arme ; sans catalogue, une balle part de chaque emplacement.
        let expected_bullets = if weapons.is_empty() {
            spawns.len()
        } else {
            weapons.len()
        };
        assert_eq!(shapes.len(), bullets_before + expected_bullets);
        // toutes les nouvelles formes sont des projectiles
        for b in &shapes[bullets_before..] {
            assert_eq!(b.who_i_am, WHOIAM_BULLET);
        }
        if weapons.is_empty() && spawns.is_empty() {
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
        // l'arme (point local tourné avec le vaisseau) - mesh de la munition
        // à la place de la balle rouge
        let state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        // deux armes : une au nez (90 %, 50 % → +x) et une à l'arrière
        // (10 %, 50 % → −x) - meshes de munition à 2 faces colorées
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
        // emplacement tourné - et chaque munition porte ses faces colorées
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
