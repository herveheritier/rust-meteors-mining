//! Géométrie 2D de base.
//!
//! Portage de `point_type.bas`, `world_type.bas`, `segment_type.bas` et
//! `triangle_type.bas`. Toute la géométrie est en double précision (comme
//! QB64) et respecte la **convention écran : y croît vers le bas** (ne pas
//! « corriger » — voir `docs/PORTAGE.md` §6).

use rand::Rng;

use crate::config::TAU;

// ─── Point ───────────────────────────────────────────────────────────────────

/// Un point 2D (ex `point_type`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    /// Produit scalaire (ex `dotProduct`).
    pub fn dot(self, q: Point) -> f64 {
        self.x * q.x + self.y * q.y
    }

    /// Fait tourner `self` autour de `axe` de `angle` radians (ex `rotation`).
    pub fn rotate_around(&mut self, axe: Point, angle: f64) {
        let ax0 = self.x - axe.x;
        let ay0 = self.y - axe.y;
        self.x = ax0 * angle.cos() - ay0 * angle.sin() + axe.x;
        self.y = ax0 * angle.sin() + ay0 * angle.cos() + axe.y;
    }

    /// Reboucle la position dans le monde torique (ex `normalizeWorldPosition`).
    ///
    /// NB : comme l'original, ne gère qu'un seul passage de frontière par axe ;
    /// les positions ne s'éloignent jamais de plus d'un monde, cela suffit.
    pub fn normalize_world(&mut self, world: &World) {
        if self.x < world.minx {
            self.x = self.x - world.minx + world.maxx;
        }
        if self.x > world.maxx {
            self.x = self.x + world.minx - world.maxx;
        }
        if self.y < world.miny {
            self.y = self.y - world.miny + world.maxy;
        }
        if self.y > world.maxy {
            self.y = self.y + world.miny - world.maxy;
        }
    }

    /// Variante pour les plans de parallaxe (ex `normalizePlanPosition`).
    /// NB : pas utilisée par le rendu actuel (les étoiles gèrent leur propre
    /// parallaxe), mais couverte par un test unitaire — conservée pour la
    /// fidélité de l'API géométrique.
    #[allow(dead_code)]
    pub fn normalize_plan(&mut self, world: &World, plan: i32) {
        let plan = plan as f64;
        if self.x < world.minx * plan {
            self.x = self.x - world.minx * plan + world.maxx * plan;
        }
        if self.x > world.maxx * plan {
            self.x = self.x + world.minx * plan - world.maxx * plan;
        }
        if self.y < world.miny * plan {
            self.y = self.y - world.miny * plan + world.maxy * plan;
        }
        if self.y > world.maxy * plan {
            self.y = self.y + world.miny * plan - world.maxy * plan;
        }
    }

    /// Compare deux points à epsilon près (ex `arePointsEqual`).
    pub fn are_equal(self, other: Point) -> bool {
        const EPSILON: f64 = 0.0001;
        (self.x - other.x).abs() < EPSILON && (self.y - other.y).abs() < EPSILON
    }
}

/// Crée un sommet à l'extérieur du triangle : à distance `h` du milieu de
/// `[p1, p2]`, du côté opposé à `p3` (ex `generateVertexOutsideTriangle`).
pub fn generate_vertex_outside(p1: Point, p2: Point, p3: Point, h: f64) -> Point {
    // vecteur ab
    let v = Point::new(p2.x - p1.x, p2.y - p1.y);
    let l = v.x.hypot(v.y);
    if l == 0.0 {
        return Point::new(0.0, 0.0);
    }
    // normale unitaire à [p1,p2] : (-vy, vx)
    let mut u = Point::new(-v.y / l, v.x / l);
    // côté de la droite (p1,p2) où se trouve p3 : signe de (p3-p1) × v
    let side = (p3.x - p1.x) * v.y - (p3.y - p1.y) * v.x;
    // si p3 est du côté où la normale est négative, on inverse la normale
    // pour placer le point dans le demi-plan opposé à p3
    if side <= 0.0 {
        u = Point::new(-u.x, -u.y);
    }
    // milieu de ab comme base de mesure de la hauteur
    Point::new((p1.x + p2.x) / 2.0 + u.x * h, (p1.y + p2.y) / 2.0 + u.y * h)
}

// ─── Monde torique ───────────────────────────────────────────────────────────

/// Le monde torique (ex `world_type`).
/// NB : `width`/`height` ne sont jamais relus (la géométrie passe par
/// `minx`/`maxx`/`miny`/`maxy`) ; conservés pour la fidélité à
/// `defineWorld(world, width, height, ...)`.
#[derive(Clone, Copy, Debug)]
pub struct World {
    #[allow(dead_code)]
    pub width: f64,
    #[allow(dead_code)]
    pub height: f64,
    pub minx: f64,
    pub maxx: f64,
    pub miny: f64,
    pub maxy: f64,
}

impl World {
    /// Ex `defineWorld(world, width, height, upper, left, bottom, right)`.
    pub fn define(width: f64, height: f64, upper: f64, left: f64, bottom: f64, right: f64) -> Self {
        World {
            width,
            height,
            minx: left,
            maxx: right,
            miny: upper,
            maxy: bottom,
        }
    }
}

// ─── Segment ─────────────────────────────────────────────────────────────────

/// Un segment (ex `segment_type`).
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub a: Point,
    pub b: Point,
}

/// Résultat de `checkSegmentsIntersect` :
/// - `Crossing` : intersection propre ou chevauchement colinéaire (valeur QB64 -1) ;
/// - `SharedVertex` : un sommet est commun aux deux segments (valeur QB64 -3) ;
/// - `None` : aucune intersection (valeur QB64 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentIntersection {
    None,
    Crossing,
    SharedVertex,
}

impl Segment {
    /// Teste l'intersection entre deux segments (ex `checkSegmentsIntersect`).
    ///
    /// NB : un sommet partagé renvoie `SharedVertex` et **pas** `Crossing` —
    /// c'est ce qui permet à `isTriangleValid` d'accepter l'arête commune entre
    /// un nouveau triangle et le triangle parent.
    pub fn intersects(&self, other: &Segment) -> SegmentIntersection {
        const EPSILON: f64 = 0.001;

        // boîtes englobantes
        let x1min = self.a.x.min(self.b.x);
        let x1max = self.a.x.max(self.b.x);
        let y1min = self.a.y.min(self.b.y);
        let y1max = self.a.y.max(self.b.y);
        let x2min = other.a.x.min(other.b.x);
        let x2max = other.a.x.max(other.b.x);
        let y2min = other.a.y.min(other.b.y);
        let y2max = other.a.y.max(other.b.y);

        if x1max < x2min || x2max < x1min || y1max < y2min || y2max < y1min {
            return SegmentIntersection::None;
        }

        // sommet partagé
        if ((self.a.x - other.a.x).abs() < EPSILON && (self.a.y - other.a.y).abs() < EPSILON)
            || ((self.a.x - other.b.x).abs() < EPSILON && (self.a.y - other.b.y).abs() < EPSILON)
            || ((self.b.x - other.a.x).abs() < EPSILON && (self.b.y - other.a.y).abs() < EPSILON)
            || ((self.b.x - other.b.x).abs() < EPSILON && (self.b.y - other.b.y).abs() < EPSILON)
        {
            return SegmentIntersection::SharedVertex;
        }

        // vecteurs
        let dx1 = self.b.x - self.a.x;
        let dy1 = self.b.y - self.a.y;
        let dx2 = other.b.x - other.a.x;
        let dy2 = other.b.y - other.a.y;
        let det = dx1 * dy2 - dy1 * dx2;

        if det.abs() < EPSILON {
            // parallèles ou colinéaires
            if ((other.a.x - self.a.x) * dy1 - (other.a.y - self.a.y) * dx1).abs() < EPSILON {
                // colinéaires — test de chevauchement
                if (x1min <= x2max && x2min <= x1max) && (y1min <= y2max && y2min <= y1max) {
                    return SegmentIntersection::Crossing;
                }
            }
            return SegmentIntersection::None;
        }

        // paramètres d'intersection
        let t1 = ((other.a.x - self.a.x) * dy2 - (other.a.y - self.a.y) * dx2) / det;
        let t2 = ((other.a.x - self.a.x) * dy1 - (other.a.y - self.a.y) * dx1) / det;
        if t1 >= 0.0 && t1 <= 1.0 && t2 >= 0.0 && t2 <= 1.0 {
            SegmentIntersection::Crossing
        } else {
            SegmentIntersection::None
        }
    }
}

// ─── Triangle ────────────────────────────────────────────────────────────────

/// Un triangle (ex `triangle_type`).
///
/// Géométrie locale (`a`, `b`, `c`, `center`…) définie dans le repère de la
/// forme ; géométrie monde (`real_*`) calculée par `compute_real_positions`.
#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub id: i32,
    pub position: Point,
    pub angle: f64,
    pub hauteur: f64,
    pub demibase: Point,
    pub a: Point,
    pub b: Point,
    pub c: Point,
    pub center: Point,
    pub real_a: Point,
    pub real_b: Point,
    pub real_c: Point,
    pub real_center: Point,
    pub real_min: Point,
    pub real_max: Point,
    pub collid: bool,
    pub collid_by: i32,
    pub life: i32,
    pub shape_index: i32,
    pub element: i32,
    /// Couleur ARGB 32 bits (AARRGGBB) **par face** — 0 = couleur de la forme
    /// (`shape.shape_color`) ou de l'élément. Posée par le cosmonaute
    /// (`cosmonaut.rs`, export « meshes-designer ») dont chaque face porte sa
    /// propre couleur ; inutilisée par les formes procédurales.
    pub color: u32,
    /// Membre animé du cosmonaute EVA : 0 = aucun (reste fixe), 1 = bras,
    /// 2 = jambe — bascule autour de `pivot` pendant la poussée (voir
    /// `cosmonaut::animate_eva_cosmonaut`). 0 pour toutes les formes
    /// procédurales.
    pub limb: i32,
    /// Pivot (articulation) du membre, en coordonnées **locales** de la forme :
    /// rotation des sommets du triangle autour de ce point pendant l'animation.
    pub pivot: Point,
    pub a_shape_border: bool,
    pub b_shape_border: bool,
    pub c_shape_border: bool,
    /// Position de base dans la texture (ex `textureBasePosition`) — posée
    /// mais non relue (le rendu texturé du port ne s'en sert pas).
    #[allow(dead_code)]
    pub texture_base_position: i32,
}

impl Default for Triangle {
    fn default() -> Self {
        Triangle {
            id: 0,
            position: Point::default(),
            angle: 0.0,
            hauteur: 0.0,
            demibase: Point::default(),
            a: Point::default(),
            b: Point::default(),
            c: Point::default(),
            center: Point::default(),
            real_a: Point::default(),
            real_b: Point::default(),
            real_c: Point::default(),
            real_center: Point::default(),
            real_min: Point::default(),
            real_max: Point::default(),
            collid: false,
            collid_by: 0,
            life: 0,
            shape_index: 0,
            element: 0,
            color: 0,
            limb: 0,
            pivot: Point::default(),
            a_shape_border: false,
            b_shape_border: false,
            c_shape_border: false,
            texture_base_position: 0,
        }
    }
}

impl Triangle {
    /// Crée un triangle à partir de 3 sommets (ex `createTriangle`).
    pub fn create(&mut self, p1: Point, p2: Point, p3: Point) {
        self.a = p1;
        self.b = p2;
        self.c = p3;
        self.position = Point::new(0.0, 0.0);
        self.angle = (self.b.y - self.a.y).atan2(self.b.x - self.a.x);
        self.hauteur = ((self.b.x - self.a.x) * (self.a.y - self.c.y)
            - (self.b.y - self.a.y) * (self.a.x - self.c.x))
        .abs()
            / (self.b.x - self.a.x).hypot(self.b.y - self.a.y);
        self.demibase = Point::new((self.b.x - self.a.x) / 2.0, (self.b.y - self.a.y) / 2.0);
        self.center = Point::new(
            (self.a.x + self.b.x + self.c.x) / 3.0,
            (self.a.y + self.b.y + self.c.y) / 3.0,
        );
        self.life = 1;
    }

    /// Génère un nouveau triangle aléatoire (ex `generateTriangle`).
    ///
    /// NB : `t.b.y = -sin(angle)*bas` — le signe moins fait partie de la
    /// convention d'écran de l'original, ne pas « corriger ».
    pub fn generate(
        &mut self,
        base_min: i32,
        base_max: i32,
        hauteur_min: i32,
        hauteur_max: i32,
        rng: &mut impl Rng,
    ) {
        let bas = base_max as f64 - rng.gen::<f64>() * (base_max as f64 - base_min as f64);
        let angle = rng.gen::<f64>() * TAU;
        self.a = Point::new(0.0, 0.0);
        self.b = Point::new(angle.cos() * bas, -angle.sin() * bas);
        let hauteur =
            hauteur_max as f64 - rng.gen::<f64>() * (hauteur_max as f64 - hauteur_min as f64);
        self.demibase = Point::new(self.b.x / 2.0, self.b.y / 2.0);
        self.c = Point::new(
            self.demibase.x + (angle + TAU / 4.0).cos() * hauteur,
            self.demibase.y - (angle + TAU / 4.0).sin() * hauteur,
        );
        self.center = Point::new(
            (self.a.x + self.b.x + self.c.x) / 3.0,
            (self.a.y + self.b.y + self.c.y) / 3.0,
        );
        self.life = 1;
    }
}

/// SAT : test de collision entre deux triangles en coordonnées monde
/// (ex `trianglesCollide`).
pub fn triangles_collide(a: &Triangle, b: &Triangle) -> bool {
    const INF: f64 = 1e308;
    const EPS: f64 = 1e-9;

    let verts_a = [a.real_a, a.real_b, a.real_c];
    let verts_b = [b.real_a, b.real_b, b.real_c];

    // projection sur les 6 axes (3 arêtes de chaque triangle)
    for t in 0..2 {
        let verts = if t == 0 { &verts_a } else { &verts_b };
        for i in 0..3 {
            let p1 = verts[i];
            let p2 = verts[(i + 1) % 3];
            let edge = Point::new(p2.x - p1.x, p2.y - p1.y);
            let axis = Point::new(-edge.y, edge.x);

            let mut min_a = INF;
            let mut max_a = -INF;
            for j in 0..3 {
                let proj = verts_a[j].dot(axis);
                min_a = min_a.min(proj);
                max_a = max_a.max(proj);
            }
            let mut min_b = INF;
            let mut max_b = -INF;
            for j in 0..3 {
                let proj = verts_b[j].dot(axis);
                min_b = min_b.min(proj);
                max_b = max_b.max(proj);
            }
            if max_a < min_b - EPS || max_b < min_a - EPS {
                return false;
            }
        }
    }
    true
}

/// Vérifie si le segment `s` est une arête du triangle `t`
/// (ex `isSegmentShared`).
pub fn is_segment_shared(s: &Segment, t: &Triangle) -> bool {
    (s.a.are_equal(t.a) && s.b.are_equal(t.b))
        || (s.a.are_equal(t.b) && s.b.are_equal(t.c))
        || (s.a.are_equal(t.c) && s.b.are_equal(t.a))
        || (s.b.are_equal(t.a) && s.a.are_equal(t.b))
        || (s.b.are_equal(t.b) && s.a.are_equal(t.c))
        || (s.b.are_equal(t.c) && s.a.are_equal(t.a))
}

/// Vérifie si un sommet est à l'intérieur du triangle (barycentrique,
/// ex `isVertexInnerTriangle`).
pub fn is_vertex_in_triangle(t: &Triangle, vertex: Point) -> bool {
    // vecteurs
    let v0 = Point::new(t.c.x - t.a.x, t.c.y - t.a.y);
    let v1 = Point::new(t.b.x - t.a.x, t.b.y - t.a.y);
    let v2 = Point::new(vertex.x - t.a.x, vertex.y - t.a.y);

    // produits scalaires
    let dot00 = v0.x * v0.x + v0.y * v0.y;
    let dot01 = v0.x * v1.x + v0.y * v1.y;
    let dot02 = v0.x * v2.x + v0.y * v2.y;
    let dot11 = v1.x * v1.x + v1.y * v1.y;
    let dot12 = v1.x * v2.x + v1.y * v2.y;

    // coordonnées barycentriques
    let mut inv_denom = dot00 * dot11 - dot01 * dot01;
    if inv_denom.abs() < 1e-12 {
        return false; // colinéaire
    }
    inv_denom = 1.0 / inv_denom;
    let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
    let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;

    // le point est dedans si u >= 0, v >= 0 et u + v <= 1
    u >= 0.0 - 1e-12 && v >= 0.0 - 1e-12 && u + v <= 1.0 + 1e-12
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> World {
        World::define(100.0, 100.0, -50.0, -50.0, 50.0, 50.0)
    }

    #[test]
    fn normalize_world_wraps_on_each_side() {
        let w = test_world();
        let mut p = Point::new(60.0, 0.0); // > maxx (50)
        p.normalize_world(&w);
        assert_eq!(p.x, -40.0); // 60 + minx - maxx = 60 - 50 - 50
        let mut p = Point::new(-60.0, 0.0); // < minx (-50)
        p.normalize_world(&w);
        assert_eq!(p.x, 40.0); // -60 - minx + maxx = -60 + 50 + 50
        let mut p = Point::new(0.0, 60.0);
        p.normalize_world(&w);
        assert_eq!(p.y, -40.0);
        let mut p = Point::new(0.0, -60.0);
        p.normalize_world(&w);
        assert_eq!(p.y, 40.0);
    }

    #[test]
    fn normalize_plan_scales_the_world() {
        let w = test_world();
        let mut p = Point::new(150.0, 0.0); // plan 2 → bornes ±100
        p.normalize_plan(&w, 2);
        assert_eq!(p.x, -50.0); // 150 - 100 - 100
        let mut p = Point::new(-150.0, 0.0);
        p.normalize_plan(&w, 2);
        assert_eq!(p.x, 50.0);
    }

    #[test]
    fn rotation_uses_screen_convention() {
        let mut p = Point::new(1.0, 0.0);
        p.rotate_around(Point::new(0.0, 0.0), TAU / 4.0);
        assert!((p.x - 0.0).abs() < 1e-12 && (p.y - 1.0).abs() < 1e-12);
        let mut p = Point::new(2.0, 0.0);
        p.rotate_around(Point::new(1.0, 0.0), TAU / 4.0);
        assert!((p.x - 1.0).abs() < 1e-12 && (p.y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn generate_vertex_outside_is_opposite_to_p3() {
        // triangle p1=(0,0) p2=(10,0) p3=(5,5) : le sommet doit être sous ab (y < 0)
        let p = generate_vertex_outside(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 5.0),
            3.0,
        );
        assert_eq!(p.x, 5.0);
        assert!((p.y - -3.0).abs() < 1e-12);
    }

    #[test]
    fn segments_crossing_and_not() {
        let a = Segment {
            a: Point::new(0.0, 0.0),
            b: Point::new(10.0, 10.0),
        };
        let b = Segment {
            a: Point::new(0.0, 10.0),
            b: Point::new(10.0, 0.0),
        };
        assert_eq!(a.intersects(&b), SegmentIntersection::Crossing);

        let c = Segment {
            a: Point::new(0.0, 20.0),
            b: Point::new(10.0, 30.0),
        };
        assert_eq!(a.intersects(&c), SegmentIntersection::None);
    }

    #[test]
    fn segments_sharing_a_vertex() {
        let a = Segment {
            a: Point::new(0.0, 0.0),
            b: Point::new(10.0, 0.0),
        };
        let b = Segment {
            a: Point::new(10.0, 0.0),
            b: Point::new(10.0, 10.0),
        };
        assert_eq!(a.intersects(&b), SegmentIntersection::SharedVertex);
    }

    #[test]
    fn segments_collinear_overlap() {
        let a = Segment {
            a: Point::new(0.0, 0.0),
            b: Point::new(10.0, 0.0),
        };
        let b = Segment {
            a: Point::new(5.0, 0.0),
            b: Point::new(15.0, 0.0),
        };
        assert_eq!(a.intersects(&b), SegmentIntersection::Crossing);
        let c = Segment {
            a: Point::new(20.0, 0.0),
            b: Point::new(30.0, 0.0),
        };
        assert_eq!(a.intersects(&c), SegmentIntersection::None);
    }

    #[test]
    fn vertex_inside_and_outside_triangle() {
        let mut t = Triangle::default();
        t.create(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 10.0),
        );
        assert!(is_vertex_in_triangle(&t, Point::new(1.0, 1.0)));
        assert!(!is_vertex_in_triangle(&t, Point::new(8.0, 8.0)));
        assert!(!is_vertex_in_triangle(&t, Point::new(-1.0, 1.0)));
    }

    #[test]
    fn triangles_collide_sat() {
        let mut a = Triangle::default();
        a.create(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(0.0, 10.0),
        );
        // recopie en coordonnées monde
        a.real_a = a.a;
        a.real_b = a.b;
        a.real_c = a.c;

        let mut b = Triangle::default();
        b.create(
            Point::new(5.0, 5.0),
            Point::new(15.0, 5.0),
            Point::new(5.0, 15.0),
        );
        b.real_a = b.a;
        b.real_b = b.b;
        b.real_c = b.c;
        assert!(triangles_collide(&a, &b));

        let mut c = Triangle::default();
        c.create(
            Point::new(50.0, 50.0),
            Point::new(60.0, 50.0),
            Point::new(50.0, 60.0),
        );
        c.real_a = c.a;
        c.real_b = c.b;
        c.real_c = c.c;
        assert!(!triangles_collide(&a, &c));
    }

    #[test]
    fn triangle_create_sets_derived_geometry() {
        let mut t = Triangle::default();
        t.create(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(5.0, 8.0),
        );
        assert_eq!(t.life, 1);
        assert!((t.angle - 0.0).abs() < 1e-12);
        assert!((t.hauteur - 8.0).abs() < 1e-12);
        assert_eq!(t.center, Point::new(5.0, 8.0 / 3.0));
        assert_eq!(t.demibase, Point::new(5.0, 0.0));
    }
}
