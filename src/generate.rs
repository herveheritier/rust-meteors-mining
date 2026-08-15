//! Génération des formes et initialisation du jeu.
//!
//! Portage des fonctions de génération de `meteorsMining.bas` :
//! `generateShape`, `createShape`, `createAlien`, `createStation`,
//! `createGem`, `fireBullet` et `prepare`.

use rand::Rng;
use std::f64::consts::TAU;

use crate::config::*;
use crate::geom::{generate_vertex_outside, Point, Triangle};
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
    compute_shape_center(shape, triangles);

    shape_index
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
    shapes[idx].radius = STATION_RADIUS;
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

/// Tire une balle depuis le vaisseau (ex `fireBullet`).
pub fn fire_bullet(shapes: &mut Vec<Shape>, triangles: &mut Vec<Triangle>) {
    let mut shape = Shape::default();
    let _idx = create_specific_shape(&mut shape, shapes, triangles, BULLET_POINTS);
    shape.who_i_am = WHOIAM_BULLET;
    shape.is_collider = true;
    shape.shape_color = 0xFFFF0000;
    shape.position = Point::new(
        shapes[PLAYER_INDEX].position.x + shapes[PLAYER_INDEX].target_center.x,
        shapes[PLAYER_INDEX].position.y + shapes[PLAYER_INDEX].target_center.y,
    );
    shape.direction = -shapes[PLAYER_INDEX].orientation;
    shape.velocity = shapes[PLAYER_INDEX].velocity + 2.0;
    shape.orientation = shapes[PLAYER_INDEX].orientation;
    shape.rotation = 0.0;
    shape.center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    let id = shape.id as usize;
    shapes[id] = shape;
}

/// Initialise le monde : éléments, vaisseau joueur, étoiles, station
/// (ex `prepare`).
pub fn prepare(
    _state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    stars: &mut Vec<Point>,
    elements: &mut Vec<Element>,
    rng: &mut impl Rng,
) {
    // éléments (indice 0 factice, 1=GOLD, 2=IRON, 3=WATER)
    *elements = default_elements();

    // vaisseau joueur (shapes[0])
    let mut shape = Shape::default();
    let mut t = Triangle::default();
    t.create(
        Point::new(PLAYER_POINTS[0].0, PLAYER_POINTS[0].1),
        Point::new(PLAYER_POINTS[1].0, PLAYER_POINTS[1].1),
        Point::new(PLAYER_POINTS[2].0, PLAYER_POINTS[2].1),
    );
    t.id = 0;
    t.element = 0;
    t.shape_index = 0;
    triangles.push(t);
    shape.life = 1;
    shape.first_triangle = 0;
    shape.last_triangle = 0;
    shape.id = 0;
    shape.who_i_am = WHOIAM_PLAYER;
    shape.show_all_parts = true;
    shape.is_collider = true;
    shape.shape_color = 0x80FFFFFF;
    shape.texture = TEXTURE_PLAYER;
    shape.position = Point::new(0.0, 0.0);
    shape.direction = 0.0;
    shape.velocity = 0.0;
    shape.orientation = 0.0;
    shape.rotation = 0.0;
    shape.center = Point::new(0.0, 0.0);
    shape.target_center = Point::new(0.0, 0.0);
    shape.radius = 10.0;
    shapes.push(shape);
    compute_shape_center(&mut shapes[PLAYER_INDEX], triangles);

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
        assert_eq!(shapes[PLAYER_INDEX].life, 1);
        assert_eq!(shapes[STATION_INDEX].who_i_am, WHOIAM_STATION);
        // joueur (1) + station (34 emplacements)
        assert_eq!(triangles.len(), 1 + 34);
        assert_eq!(stars.len(), STARS_COUNT);
        assert_eq!(elements.len(), 4);
        // positions initiales
        assert_eq!(shapes[PLAYER_INDEX].position, Point::new(0.0, 0.0));
        assert_eq!(shapes[STATION_INDEX].position, Point::new(0.0, 0.0));
    }
}
