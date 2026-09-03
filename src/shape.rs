//! Formes (assemblages de triangles).
//!
//! Portage de `shape_type.bas` : `Shape`, `free_shape`, `resolve_elastic_collision`,
//! `detect_collision`, `moving_shape`, `compute_real_positions`,
//! `compute_shape_center`, `get_border_segments`, `is_triangle_valid`,
//! `is_vertex_in_shape`, `choose_border_segment`, `meshes_to_shape`,
//! `resize_shape`, `create_specific_shape`.
//!
//! Les meshes (alien, minerai, balle) étaient des instructions `DATA` du code
//! QB64 (voir `docs/ASSETS.md` §3) ; ils deviennent des constantes ici (la
//! station est désormais chargée depuis `assets/anneauStation.json`, voir
//! `station.rs`).

use rand::Rng;
use std::f64::consts::TAU;

use crate::config::TEXTURE_NONE;
use crate::geom::{
    is_segment_shared, is_vertex_in_triangle, triangles_collide, Point, Segment, Triangle,
    SegmentIntersection, World,
};

// ─── Mesh ────────────────────────────────────────────────────────────────────

/// Un mesh : une liste de packs (éventails de points).
///
/// Chaque pack de `n` points produit `n-2` triangles : `(p1,p2,p3)`,
/// `(p2,p3,p4)`, … (format `meshesToShape`).
pub type Mesh = &'static [&'static [(f64, f64)]];

/// Alien - `reference/assets/gripper-meshes.bas` (4 packs : 16+16+5+8 → 37 triangles).
pub const ALIEN_MESH: Mesh = &[
    &[
        (170.0, 50.0),
        (140.0, 120.0),
        (110.0, 90.0),
        (130.0, 140.0),
        (60.0, 100.0),
        (10.0, 160.0),
        (20.0, 100.0),
        (-40.0, 110.0),
        (0.0, 80.0),
        (-60.0, 40.0),
        (0.0, 0.0),
        (-110.0, 40.0),
        (-140.0, 0.0),
        (-140.0, 110.0),
        (-170.0, 0.0),
        (-200.0, 0.0),
    ],
    &[
        (170.0, -50.0),
        (140.0, -120.0),
        (110.0, -90.0),
        (130.0, -140.0),
        (60.0, -100.0),
        (10.0, -160.0),
        (20.0, -100.0),
        (-40.0, -110.0),
        (0.0, -80.0),
        (-60.0, -40.0),
        (0.0, 0.0),
        (-110.0, -40.0),
        (-140.0, 0.0),
        (-140.0, -110.0),
        (-170.0, 0.0),
        (-200.0, 0.0),
    ],
    &[
        (-180.0, 40.0),
        (-200.0, 0.0),
        (-250.0, 40.0),
        (-290.0, 0.0),
        (-320.0, 40.0),
    ],
    &[
        (-180.0, -40.0),
        (-200.0, 0.0),
        (-250.0, -40.0),
        (-290.0, 0.0),
        (-320.0, -40.0),
        (-320.0, 40.0),
        (-370.0, -80.0),
        (-370.0, 80.0),
    ],
];

/// Minerai ramassable - `data 1,4` puis 4 points → 2 triangles (voir `createMineral`).
pub const MINERAL_MESH: Mesh = &[&[
    (2.0, -2.0),
    (-2.0, -2.0),
    (-2.0, 2.0),
    (2.0, 2.0),
]];

/// Balle - `data 1,-2,-2, -2,2, 2,0` → 1 triangle (voir `fireBullet`).
pub const BULLET_POINTS: &[(f64, f64)] = &[(-2.0, -2.0), (-2.0, 2.0), (2.0, 0.0)];

// ─── Shape ───────────────────────────────────────────────────────────────────

/// Une forme : un assemblage de triangles (ex `shape_type`).
#[derive(Clone, Debug)]
pub struct Shape {
    pub id: i32,
    pub first_triangle: usize,
    pub last_triangle: usize,
    /// `pointsUsageIndicator` en bitmask : bit `i` = bord `i` déjà utilisé
    /// (bords de l'éventail partagés ou consommés par la génération).
    /// `u128` : un météore de `n` triangles a `3n` bords, et la difficulté
    /// monte à `TRIANGLES_IN_SHAPE_MAX × 2` = 32 triangles (96 bords) - un
    /// `u64` débordait (repli silencieux du décalage en release, collision
    /// de bits qui accélérait l'épuisement des bords libres).
    pub border_mask: u128,
    /// Nombre de bits suivis (= 3 × nombre de triangles de la forme).
    pub border_len: usize,
    pub position: Point,
    pub width: f64,
    pub height: f64,
    pub top_left: Point,
    pub bottom_right: Point,
    pub center: Point,
    pub target_center: Point,
    pub radius: f64,
    pub direction: f64,
    pub velocity: f64,
    pub orientation: f64,
    pub rotation: f64,
    /// Couleur ARGB 32 bits au format QB64 (AARRGGBB).
    pub shape_color: u32,
    /// Handle de texture (constantes `TEXTURE_*` de `config.rs` ; 0 = aucune).
    pub texture: i32,
    pub is_collider: bool,
    pub life: i32,
    pub element: i32,
    /// Quantité de minerai contenue dans un météore (1 par triangle
    /// minéralisé au départ, +1 par minerai absorbé) : libérée en minerais à la
    /// position du météore quand il est détruit par la collision d'un autre
    /// météore. 0 hors météores.
    pub minerals: i32,
    /// Minerai **relâché de la soute** du vaisseau détruit
    /// (`eject_cargo_minerals`) : marqueur du relâchement au crash - il suit
    /// les règles du monde (absorbé par le météore qui le percute, ramassé par
    /// le vaisseau par collision), mais la **station** ne le détruit pas (il
    /// reste dans l'espace près de la base, à récupérer au retour du vaisseau
    /// reconstruit). `false` pour les minerais libérés par un météore détruit
    /// (`create_gem`), détruits eux par la station.
    pub ejected_cargo: bool,
    /// Angle (radians) de balancement actuel des membres du cosmonaute EVA :
    /// poursuit la cible oscillante pendant la poussée et décroît vers 0 au
    /// repos (voir `cosmonaut::animate_eva_cosmonaut`). 0 pour les autres
    /// formes.
    pub anim_angle: f64,
    pub show_all_parts: bool,
    pub who_i_am: i32,
    /// Météore **spécial** (boss - `generate::create_boss_meteor`) : plus
    /// gros, plus résistant (plus de triangles), libère du PLATINUM et
    /// rapporte un bonus de réputation à sa destruction. `false` pour toutes
    /// les autres formes.
    pub is_boss: bool,
}

impl Default for Shape {
    fn default() -> Self {
        Shape {
            id: 0,
            first_triangle: 0,
            last_triangle: 0,
            border_mask: 0,
            border_len: 0,
            position: Point::default(),
            width: 0.0,
            height: 0.0,
            top_left: Point::default(),
            bottom_right: Point::default(),
            center: Point::default(),
            target_center: Point::default(),
            radius: 0.0,
            direction: 0.0,
            velocity: 0.0,
            orientation: 0.0,
            rotation: 0.0,
            shape_color: 0,
            texture: TEXTURE_NONE,
            is_collider: false,
            life: 0,
            element: 0,
            minerals: 0,
            ejected_cargo: false,
            anim_angle: 0.0,
            show_all_parts: false,
            who_i_am: 0,
            is_boss: false,
        }
    }
}

/// Trouve une forme détruite avec exactement `nbr` triangles (ex `freeShape`).
///
/// NB : la recherche démarre à l'index 1 comme l'original - le joueur
/// (`shapes[0]`) n'est jamais réutilisé.
pub fn free_shape(shapes: &[Shape], nbr: usize) -> Option<usize> {
    if shapes.len() > 3 {
        for (i, s) in shapes.iter().enumerate().skip(1) {
            if s.life > 0 {
                continue;
            }
            if s.last_triangle - s.first_triangle + 1 == nbr {
                return Some(i);
            }
        }
    }
    None
}

/// Choc élastique entre deux formes (ex `resolveElasticCollision`).
pub fn resolve_elastic_collision(a: &mut Shape, b: &mut Shape) {
    // positions relatives
    let d = Point::new(b.position.x - a.position.x, b.position.y - a.position.y);
    let dist = d.x.hypot(d.y);

    // pas de collision (pas de recouvrement)
    if dist >= a.radius + b.radius {
        return;
    }

    // normale unitaire (de A vers B)
    let (nx, ny) = if dist == 0.0 {
        (1.0, 0.0)
    } else {
        (d.x / dist, d.y / dist)
    };

    // vitesses polaires → cartésiennes
    let ax = a.velocity * a.direction.cos();
    let ay = a.velocity * a.direction.sin();
    let bx = b.velocity * b.direction.cos();
    let by = b.velocity * b.direction.sin();

    // projections sur la normale et la tangente
    let va_n = ax * nx + ay * ny;
    let vb_n = bx * nx + by * ny;
    let tx = -ny;
    let ty = nx;
    let va_t = ax * tx + ay * ty;
    let vb_t = bx * tx + by * ty;

    // choc élastique 1D sur la composante normale
    // NB : masses = nombre de triangles SANS +1, fidèle à l'original
    let ma = (a.last_triangle - a.first_triangle) as f64;
    let mb = (b.last_triangle - b.first_triangle) as f64;
    let va_n_after = (va_n * (ma - mb) + 2.0 * mb * vb_n) / (ma + mb);
    let vb_n_after = (vb_n * (mb - ma) + 2.0 * ma * va_n) / (ma + mb);

    // recomposition cartésienne
    let ax_after = va_n_after * nx + va_t * tx;
    let ay_after = va_n_after * ny + va_t * ty;
    let bx_after = vb_n_after * nx + vb_t * tx;
    let by_after = vb_n_after * ny + vb_t * ty;

    // cartésiennes → polaires (direction ramenée dans [0, TAU[)
    a.velocity = ax_after.hypot(ay_after);
    a.direction = if a.velocity == 0.0 {
        0.0
    } else {
        let d = ay_after.atan2(ax_after);
        if d < 0.0 {
            d + TAU
        } else {
            d
        }
    };

    b.velocity = bx_after.hypot(by_after);
    b.direction = if b.velocity == 0.0 {
        0.0
    } else {
        let d = by_after.atan2(bx_after);
        if d < 0.0 {
            d + TAU
        } else {
            d
        }
    };
}

/// Détecte la collision entre deux formes : AABB par triangle puis SAT.
/// Pose les indicateurs `collid`/`collid_by` des deux côtés (ex `detectCollision`).
/// `index_a`/`index_b` sont les indices des formes dans le tableau `shapes` ;
/// `collid_by` stocke l'**index** (pas le type) pour pouvoir identifier la
/// forme précise qui a causé la collision (ex. quelle balle a touché quel
/// météore).
pub fn detect_collision(
    shape_a: &Shape,
    shape_b: &Shape,
    index_a: usize,
    index_b: usize,
    triangles: &mut [Triangle],
) -> bool {
    let mut res = false;
    for i in shape_a.first_triangle..=shape_a.last_triangle {
        if triangles[i].life == 0 {
            continue;
        }
        for j in shape_b.first_triangle..=shape_b.last_triangle {
            if triangles[j].life == 0 {
                continue;
            }
            // AABB
            if triangles[j].real_max.x < triangles[i].real_min.x
                || triangles[j].real_min.x > triangles[i].real_max.x
                || triangles[j].real_max.y < triangles[i].real_min.y
                || triangles[j].real_min.y > triangles[i].real_max.y
            {
                continue;
            }
            // SAT
            if triangles_collide(&triangles[j], &triangles[i]) {
                triangles[i].collid = true;
                triangles[i].collid_by = index_b as i32;
                triangles[j].collid = true;
                triangles[j].collid_by = index_a as i32;
                res = true;
            }
        }
    }
    res
}

/// Déplace et fait tourner une forme (ex `movingShape`).
///
/// `dt` en secondes : la formule QB64 `60*valeur/fps` devient `valeur*60*dt`
/// (équivalent à 60 FPS - voir `docs/PORTAGE.md` §6).
pub fn moving_shape(shape: &mut Shape, triangles: &mut [Triangle], world: &World, dt: f64) {
    shape.position.x += shape.direction.cos() * 60.0 * shape.velocity * dt;
    shape.position.y -= shape.direction.sin() * 60.0 * shape.velocity * dt;
    shape.position.normalize_world(world);
    shape.center.x += (shape.target_center.x - shape.center.x) / 100.0;
    shape.center.y += (shape.target_center.y - shape.center.y) / 100.0;
    // `rotation` stocke des rad/s (voir `meteor_spin`) - pas de conversion
    // ×60 : ce facteur ne s'applique qu'aux valeurs par frame de l'original.
    shape.orientation += shape.rotation * dt;
    for t in &mut triangles[shape.first_triangle..=shape.last_triangle] {
        compute_real_positions(t, shape.position, shape.center, shape.orientation);
    }
}

/// Position monde d'un triangle à partir de la position/centre/orientation de
/// la forme (ex `computeRealPositions`) : rotation des sommets locaux autour
/// de `axe` puis translation par `p` ; recalcule l'AABB monde.
pub fn compute_real_positions(t: &mut Triangle, p: Point, axe: Point, angle: f64) {
    let mut a = Point::new(t.a.x + t.position.x, t.a.y + t.position.y);
    let mut b = Point::new(t.b.x + t.position.x, t.b.y + t.position.y);
    let mut c = Point::new(t.c.x + t.position.x, t.c.y + t.position.y);
    let mut center = Point::new(t.center.x + t.position.x, t.center.y + t.position.y);
    a.rotate_around(axe, angle);
    b.rotate_around(axe, angle);
    c.rotate_around(axe, angle);
    center.rotate_around(axe, angle);
    t.real_a = Point::new(p.x + a.x, p.y + a.y);
    t.real_b = Point::new(p.x + b.x, p.y + b.y);
    t.real_c = Point::new(p.x + c.x, p.y + c.y);
    t.real_center = Point::new(p.x + center.x, p.y + center.y);
    t.real_min = Point::new(
        t.real_a.x.min(t.real_b.x).min(t.real_c.x),
        t.real_a.y.min(t.real_b.y).min(t.real_c.y),
    );
    t.real_max = Point::new(
        t.real_a.x.max(t.real_b.x).max(t.real_c.x),
        t.real_a.y.max(t.real_b.y).max(t.real_c.y),
    );
}

/// Calcule le centre cible, le rayon et la boîte englobante d'une forme à
/// partir de ses triangles vivants (ex `computeShapeCenter`).
pub fn compute_shape_center(shape: &mut Shape, triangles: &[Triangle]) {
    if shape.life <= 0 {
        return;
    }
    let slice = &triangles[shape.first_triangle..=shape.last_triangle];
    let mut d = 0i32;
    let mut x = 0.0;
    let mut y = 0.0;
    for tri in slice {
        if tri.life <= 0 {
            continue;
        }
        d += 1;
        x += (tri.a.x + tri.b.x + tri.c.x) / 3.0;
        y += (tri.a.y + tri.b.y + tri.c.y) / 3.0;
    }
    let p = Point::new(x / d as f64, y / d as f64);
    shape.target_center = p;

    let mut radius: f64 = 0.0;
    for tri in slice {
        if tri.life <= 0 {
            continue;
        }
        let h = (tri.center.x - shape.target_center.x)
            .hypot(tri.center.y - shape.target_center.y)
            + tri.hauteur;
        radius = radius.max(h);
    }
    shape.radius = radius;

    // boîte englobante
    shape.top_left = Point::new(f64::MAX, f64::MAX);
    shape.bottom_right = Point::new(f64::MIN, f64::MIN);
    for tri in slice {
        if tri.life <= 0 {
            continue;
        }
        let minx = tri.a.x.min(tri.b.x).min(tri.c.x);
        if shape.top_left.x > minx {
            shape.top_left.x = minx;
        }
        let miny = tri.a.y.min(tri.b.y).min(tri.c.y);
        if shape.top_left.y > miny {
            shape.top_left.y = miny;
        }
        let maxx = tri.a.x.max(tri.b.x).max(tri.c.x);
        if shape.bottom_right.x < maxx {
            shape.bottom_right.x = maxx;
        }
        let maxy = tri.a.y.max(tri.b.y).max(tri.c.y);
        if shape.bottom_right.y < maxy {
            shape.bottom_right.y = maxy;
        }
    }
    shape.width = shape.bottom_right.x - shape.top_left.x;
    shape.height = shape.bottom_right.y - shape.top_left.y;
}

/// Calcule les bords libres (segments non partagés) de la forme
/// (ex `getBorderSegments`).
///
/// NB : à appeler uniquement quand la forme change (l'original la recalcule à
/// chaque frame dans `drawShape`, inutile - voir `docs/PORTAGE.md` §6).
pub fn get_border_segments(shape: &Shape, triangles: &mut [Triangle]) {
    // les segments de chaque triangle vivant sont lus depuis la tranche
    // avant de muter les drapeaux (emprunt séparé)
    let range = shape.first_triangle..=shape.last_triangle;
    let segs: Vec<[Segment; 3]> = triangles[range.clone()]
        .iter()
        .map(|t| {
            [
                Segment { a: t.a, b: t.b },
                Segment { a: t.b, b: t.c },
                Segment { a: t.c, b: t.a },
            ]
        })
        .collect();

    for (i, s) in segs.iter().enumerate() {
        let ti = shape.first_triangle + i;
        if triangles[ti].life <= 0 {
            continue;
        }
        for (j, seg) in s.iter().enumerate() {
            let mut shared = false;
            // une arête commune avec un autre triangle n'est pas un bord libre
            for (k, other) in triangles[range.clone()].iter().enumerate() {
                let k = shape.first_triangle + k;
                if k == ti || other.life <= 0 {
                    continue;
                }
                shared = is_segment_shared(seg, other);
                if shared {
                    break;
                }
            }
            if !shared {
                let t = &mut triangles[ti];
                match j {
                    0 => t.a_shape_border = true,
                    1 => t.b_shape_border = true,
                    _ => t.c_shape_border = true,
                }
            }
        }
    }
}

/// Vérifie qu'un nouveau triangle peut être ajouté à la forme sans recouvrir
/// un autre triangle (ex `isTriangleValid`).
///
/// NB : comme l'original, aucun test de vie des triangles existants - seule
/// l'intersection des segments compte ; l'arête commune avec le parent
/// renvoie `SharedVertex` et est donc acceptée.
pub fn is_triangle_valid(shape: &Shape, triangles: &[Triangle], triangle: &Triangle) -> bool {
    let s2 = [
        Segment {
            a: triangle.a,
            b: triangle.b,
        },
        Segment {
            a: triangle.b,
            b: triangle.c,
        },
        Segment {
            a: triangle.c,
            b: triangle.a,
        },
    ];

    for tri in &triangles[shape.first_triangle..=shape.last_triangle] {
        let s1 = [
            Segment { a: tri.a, b: tri.b },
            Segment { a: tri.b, b: tri.c },
            Segment { a: tri.c, b: tri.a },
        ];
        for a in &s1 {
            for b in &s2 {
                if a.intersects(b) == SegmentIntersection::Crossing {
                    return false;
                }
            }
        }
    }
    true
}

/// Vérifie si un sommet est à l'intérieur de la forme (ex `isVertexInnerShape`).
pub fn is_vertex_in_shape(shape: &Shape, triangles: &[Triangle], vertex: Point) -> bool {
    triangles[shape.first_triangle..=shape.last_triangle]
        .iter()
        .any(|t| is_vertex_in_triangle(t, vertex))
}

/// Sélectionne un bord libre (bit 0 du bitmask) et le marque utilisé
/// (ex `chooseBorderSegment`).
///
/// NB : équivalent au balayage cyclique de la chaîne `pointsUsageIndicator`
/// de l'original, mais sur un bitmask - voir `docs/PORTAGE.md` §4.
///
/// **Jamais bloquant** : le balayage est borné à un tour complet - si aucun
/// bord n'est libre (forme « saturée », placement invalides répétés), renvoie
/// `None` et l'appelant arrête d'ajouter des triangles. L'ancien `loop`
/// infini gelait le jeu (CPU 100 %) quand les bords libres tombaient à zéro
/// pendant la génération d'un météore.
pub fn choose_border_segment(shape: &mut Shape, rng: &mut impl Rng) -> Option<usize> {
    let len = shape.border_len;
    let l = len + 1;
    let mut i = (rng.r#gen::<f64>() * l as f64) as usize;
    for _ in 0..l {
        if i < len && shape.border_mask & (1 << i) == 0 {
            shape.border_mask |= 1 << i;
            return Some(i);
        }
        i = (i + 1) % l;
    }
    None
}

/// Construit une forme à partir d'un mesh d'éventails (ex `meshesToShape`).
///
/// NB : l'original alloue `points_qty` emplacements de triangles (somme des
/// tailles des packs) alors qu'il n'en crée que `points_qty - 2×nbPacks` -
/// fidélité conservée (les emplacements restants restent morts, `life = 0`).
pub fn meshes_to_shape(
    shape: &mut Shape,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    mesh: Mesh,
) -> usize {
    let mut points_qty: usize = 0;
    for pack in mesh {
        points_qty += pack.len();
    }

    let shape_index = match free_shape(shapes, points_qty) {
        Some(idx) => {
            *shape = shapes[idx].clone();
            idx
        }
        None => {
            shapes.push(Shape::default());
            let idx = shapes.len() - 1;
            triangles.resize(triangles.len() + points_qty, Triangle::default());
            shape.last_triangle = triangles.len() - 1;
            shape.first_triangle = triangles.len() - points_qty;
            idx
        }
    };

    shape.id = shape_index as i32;
    shape.life = points_qty as i32;

    let mut remaining = points_qty;
    for pack in mesh {
        // triangles consécutifs (p1,p2,p3), (p2,p3,p4), … : les deux
        // premiers points glissent (`p1 = p2: p2 = p3` de l'original). NB :
        // un éventail fixe depuis `pack[0]` remplirait le trou des anneaux
        // (la station) - l'original fait bien glisser `p1`.
        let mut p1 = Point::new(pack[0].0, pack[0].1);
        let mut p2 = Point::new(pack[1].0, pack[1].1);
        for point in &pack[2..] {
            let p3 = Point::new(point.0, point.1);
            let mut t = Triangle::default();
            t.create(p1, p2, p3);
            t.shape_index = shape_index as i32;
            remaining -= 1;
            t.id = (shape.last_triangle - remaining) as i32;
            triangles[t.id as usize] = t;
            p1 = p2;
            p2 = p3;
        }
    }
    shape_index
}

/// Crée une forme à partir d'une liste plate de sommets (3 par triangle)
/// (ex `createSpecificShape`).
pub fn create_specific_shape(
    shape: &mut Shape,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    points: &[(f64, f64)],
) -> usize {
    let nbr = points.len() / 3;
    let shape_index = match free_shape(shapes, nbr) {
        Some(idx) => {
            *shape = shapes[idx].clone();
            idx
        }
        None => {
            shapes.push(Shape::default());
            let idx = shapes.len() - 1;
            triangles.resize(triangles.len() + nbr, Triangle::default());
            shape.last_triangle = triangles.len() - 1;
            shape.first_triangle = triangles.len() - nbr;
            idx
        }
    };

    shape.id = shape_index as i32;
    shape.life = nbr as i32;

    for k in 0..nbr {
        let p1 = Point::new(points[3 * k].0, points[3 * k].1);
        let p2 = Point::new(points[3 * k + 1].0, points[3 * k + 1].1);
        let p3 = Point::new(points[3 * k + 2].0, points[3 * k + 2].1);
        let mut t = Triangle::default();
        t.create(p1, p2, p3);
        t.shape_index = shape_index as i32;
        t.id = (shape.last_triangle - nbr + 1 + k) as i32;
        triangles[t.id as usize] = t;
    }
    shape_index
}

/// Redimensionne tous les triangles d'une forme (ex `resizeShape`).
pub fn resize_shape(resize_factor: f64, shape: &mut Shape, triangles: &mut [Triangle]) {
    for t in &mut triangles[shape.first_triangle..=shape.last_triangle] {
        t.a.x *= resize_factor;
        t.a.y *= resize_factor;
        t.b.x *= resize_factor;
        t.b.y *= resize_factor;
        t.c.x *= resize_factor;
        t.c.y *= resize_factor;
        t.center.x = (t.a.x + t.b.x + t.c.x) / 3.0;
        t.center.y = (t.a.y + t.b.y + t.c.y) / 3.0;
    }
    compute_shape_center(shape, triangles);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TAU, WHOIAM_BULLET, WHOIAM_METEOR};

    fn test_world() -> World {
        World::define(1000.0, 1000.0, -500.0, -500.0, 500.0, 500.0)
    }

    #[test]
    fn moving_shape_applies_velocity() {
        let mut shape = Shape {
            direction: 0.0,
            velocity: 1.0,
            ..Shape::default()
        };
        let mut t = Triangle {
            id: 0,
            shape_index: 0,
            ..Triangle::default()
        };
        t.create(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 8.0),
        );
        let mut triangles = vec![t];
        shape.first_triangle = 0;
        shape.last_triangle = 0;

        // direction 0 → déplacement +x de 60*1*(1/60) = 1 ; y : -sin(0) = 0
        moving_shape(&mut shape, &mut triangles, &test_world(), 1.0 / 60.0);
        assert!((shape.position.x - 1.0).abs() < 1e-9);
        assert_eq!(shape.position.y, 0.0);
        // le triangle suit la forme
        assert_eq!(triangles[0].real_a, Point::new(1.0, 0.0));
    }

    #[test]
    fn elastic_collision_head_on_swaps_velocities() {
        let mut a = Shape::default();
        let mut b = Shape::default();
        a.position = Point::new(0.0, 0.0);
        b.position = Point::new(9.0, 0.0); // dist 9 < rayon 5+5
        a.radius = 5.0;
        b.radius = 5.0;
        a.first_triangle = 0;
        a.last_triangle = 2; // 3 triangles (masse identique)
        b.first_triangle = 3;
        b.last_triangle = 5;
        a.velocity = 2.0;
        a.direction = 0.0; // vers +x
        b.velocity = 2.0;
        b.direction = TAU / 2.0; // vers -x

        resolve_elastic_collision(&mut a, &mut b);

        // masses égales → les vitesses s'échangent
        assert!((a.velocity - 2.0).abs() < 1e-9);
        assert!((a.direction - TAU / 2.0).abs() < 1e-9);
        assert!((b.velocity - 2.0).abs() < 1e-9);
        assert!(b.direction.abs() < 1e-9 || (b.direction - TAU).abs() < 1e-9);
    }

    #[test]
    fn detect_collision_sets_collid_flags() {
        let mut ta = Triangle::default();
        ta.create(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 10.0),
        );
        ta.life = 1;
        ta.id = 0;
        ta.shape_index = 0;
        ta.real_a = ta.a;
        ta.real_b = ta.b;
        ta.real_c = ta.c;
        ta.real_min = Point::new(0.0, 0.0);
        ta.real_max = Point::new(10.0, 10.0);

        let mut tb = Triangle::default();
        tb.create(
            Point::new(5.0, 5.0),
            Point::new(15.0, 5.0),
            Point::new(5.0, 15.0),
        );
        tb.life = 1;
        tb.id = 1;
        tb.shape_index = 1;
        tb.real_a = tb.a;
        tb.real_b = tb.b;
        tb.real_c = tb.c;
        tb.real_min = Point::new(5.0, 5.0);
        tb.real_max = Point::new(15.0, 15.0);

        let mut triangles = vec![ta, tb];
        let mut a = Shape::default();
        let mut b = Shape::default();
        a.first_triangle = 0;
        a.last_triangle = 0;
        b.first_triangle = 1;
        b.last_triangle = 1;
        a.who_i_am = WHOIAM_METEOR;
        b.who_i_am = WHOIAM_BULLET;

        assert!(detect_collision(&a, &b, 0, 1, &mut triangles));
        assert!(triangles[0].collid);
        assert_eq!(triangles[0].collid_by, 1); // index de la forme B (balle)
        assert!(triangles[1].collid);
        assert_eq!(triangles[1].collid_by, 0); // index de la forme A (météore)
    }

    #[test]
    fn free_shape_reuses_dead_shape_with_same_triangle_count() {
        // joueur (index 0, vivant), station (index 1, vivante), forme morte
        // de 8 triangles (index 2), forme morte de 3 triangles (index 3)
        let shapes = vec![
            Shape {
                life: 1,
                ..Shape::default()
            },
            Shape {
                life: 34,
                ..Shape::default()
            },
            Shape {
                life: 0,
                first_triangle: 2,
                last_triangle: 9,
                ..Shape::default()
            },
            Shape {
                life: 0,
                first_triangle: 10,
                last_triangle: 12,
                ..Shape::default()
            },
        ];

        assert_eq!(free_shape(&shapes, 8), Some(2));
        assert_eq!(free_shape(&shapes, 3), Some(3));
        assert_eq!(free_shape(&shapes, 7), None);
    }

    /// Petit anneau d'essai (éventail glissant fermé, comme l'ex-`STATION_MESH`
    /// de la station) : bords r 10/6, les deux derniers points répètent les
    /// deux premiers pour fermer l'anneau (18 points → 16 triangles sur 18
    /// emplacements).
    const TEST_RING_MESH: Mesh = &[&[
        (10.0, 0.0),
        (6.0, 0.0),
        (7.07, -7.07),
        (4.24, -4.24),
        (0.0, -10.0),
        (0.0, -6.0),
        (-7.07, -7.07),
        (-4.24, -4.24),
        (-10.0, 0.0),
        (-6.0, 0.0),
        (-7.07, 7.07),
        (-4.24, 4.24),
        (0.0, 10.0),
        (0.0, 6.0),
        (7.07, 7.07),
        (4.24, 4.24),
        (10.0, 0.0),
        (6.0, 0.0),
    ]];

    #[test]
    fn ring_mesh_builds_16_triangles_on_18_slots() {
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut shape = Shape::default();
        let idx = meshes_to_shape(&mut shape, &mut shapes, &mut triangles, TEST_RING_MESH);
        assert_eq!(idx, 0);
        assert_eq!(shape.life, 18); // points_qty (fidèle à l'original)
        let alive = triangles[shape.first_triangle..=shape.last_triangle]
            .iter()
            .filter(|t| t.life > 0)
            .count();
        assert_eq!(alive, 16);
        // ids séquentiels, partant de first_triangle
        assert_eq!(
            triangles[shape.first_triangle].id as usize,
            shape.first_triangle
        );
        assert_eq!(
            triangles[shape.first_triangle + 15].id as usize,
            shape.first_triangle + 15
        );
    }

    #[test]
    fn mesh_triangles_slide_consecutively_like_the_original() {
        // L'original fait glisser p1 et p2 (`p1 = p2: p2 = p3`) : le triangle
        // k doit être (pack[k], pack[k+1], pack[k+2]). Un éventail fixe depuis
        // pack[0] remplirait le trou de l'anneau (la station) - test de garde.
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut shape = Shape::default();
        meshes_to_shape(&mut shape, &mut shapes, &mut triangles, TEST_RING_MESH);
        for k in 0..16 {
            let t = &triangles[shape.first_triangle + k];
            let (ax, ay) = TEST_RING_MESH[0][k];
            let (bx, by) = TEST_RING_MESH[0][k + 1];
            let (cx, cy) = TEST_RING_MESH[0][k + 2];
            assert_eq!((t.a.x, t.a.y), (ax, ay), "sommet a du triangle {k}");
            assert_eq!((t.b.x, t.b.y), (bx, by), "sommet b du triangle {k}");
            assert_eq!((t.c.x, t.c.y), (cx, cy), "sommet c du triangle {k}");
        }
    }
}
