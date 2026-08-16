//! Constantes du jeu.
//!
//! Portage de `context_type.bas` : en QB64 ces valeurs étaient des champs de
//! `context_type`, ici elles deviennent des `const` Rust (elles sont fixes).
//! Seul l'état dynamique reste dans `GameState` (voir `src/state.rs`).
//!
//! Valeurs identiques à l'original — voir `docs/ANALYSE.md` §6.

use std::f64::consts::TAU as TAU_F64;

/// Taille de la fenêtre / de la vue.
pub const VIEWPORT_WIDTH: f64 = 960.0;
pub const VIEWPORT_HEIGHT: f64 = 540.0;

/// Marge autour de la vue = taille du monde hors écran.
pub const EXTERNAL_BORDER: f64 = 1500.0;

/// Dimensions du monde torique (= vue + 2×marge).
pub const WORLD_WIDTH: f64 = VIEWPORT_WIDTH + 2.0 * EXTERNAL_BORDER; // 3960
pub const WORLD_HEIGHT: f64 = VIEWPORT_HEIGHT + 2.0 * EXTERNAL_BORDER; // 3540
pub const WORLD_MINX: f64 = (VIEWPORT_WIDTH - WORLD_WIDTH) / 2.0; // -1500
pub const WORLD_MAXX: f64 = -WORLD_MINX + VIEWPORT_WIDTH; // 2460
pub const WORLD_MINY: f64 = (VIEWPORT_HEIGHT - WORLD_HEIGHT) / 2.0; // -1500
pub const WORLD_MAXY: f64 = -WORLD_MINY + VIEWPORT_HEIGHT; // 2040

/// Limites de dessin (monde − marge ± 100, élargies par le refactor du 15/08/2026).
pub const DRAW_MINX: f64 = WORLD_MINX + EXTERNAL_BORDER - 100.0; // -100
pub const DRAW_MAXX: f64 = WORLD_MAXX - EXTERNAL_BORDER + 100.0; // 1060
pub const DRAW_MINY: f64 = WORLD_MINY + EXTERNAL_BORDER - 100.0; // -100
pub const DRAW_MAXY: f64 = WORLD_MAXY - EXTERNAL_BORDER + 100.0; // 640

/// Étoiles de fond (parallaxe).
pub const STARS_COUNT: usize = 100_000;
pub const STARS_LAYERS: i32 = 15;

/// Plafond du nombre de météores.
pub const SHAPES_COUNT: i32 = 150;

/// Génération procédurale des météores.
pub const TRIANGLES_IN_SHAPE_MIN: i32 = 6;
pub const TRIANGLES_IN_SHAPE_MAX: i32 = 16;
pub const TRIANGLE_BASE_MIN: i32 = 15;
pub const TRIANGLE_BASE_MAX: i32 = 40;
pub const TRIANGLE_HEIGHT_MIN: i32 = 11;
pub const TRIANGLE_HEIGHT_MAX: i32 = 22;

/// FPS visé par la boucle QB64 (le rendu plafonne bien plus bas).
pub const ATTEMPT_FPS: i32 = 600;

/// Plein écran au démarrage.
pub const FULL_SCREEN: bool = false;

/// 2π (ex `TAU` global du jeu QB64).
pub const TAU: f64 = TAU_F64;

/// Identifiants `whoIam` des formes.
pub const WHOIAM_METEOR: i32 = 0;
pub const WHOIAM_BULLET: i32 = 1;
pub const WHOIAM_PLAYER: i32 = 2;
pub const WHOIAM_GEM: i32 = 3;
pub const WHOIAM_STATION: i32 = 4;
pub const WHOIAM_ALIEN: i32 = 5;

/// Modes de déplacement du vaisseau.
pub const MOVING_MODE_INERTIAL: i32 = 0;
pub const MOVING_MODE_4_WAYS: i32 = 1;
pub const MOVING_MODE_DIRECTIONAL: i32 = 2;

/// Options de compilation ($LET du code QB64).
///
/// NB : `SHOW_INFOS`/`SHOW_RADIUS`/`SHOW_DEBUG` correspondent aux bascules de
/// debug devenues **runtime** dans le port (touches I/D) ; `NO_MUSIC`
/// documente l'absence de musique. Conservées telles quelles pour la
/// fidélité à la référence.
#[allow(dead_code)]
pub const SHOW_INFOS: bool = false;
pub const SHOW_GLOBAL_MAP: bool = true;
#[allow(dead_code)]
pub const SHOW_RADIUS: bool = false;
#[allow(dead_code)]
pub const SHOW_DEBUG: bool = false;
#[allow(dead_code)]
pub const NO_MUSIC: bool = true;

/// Indices fixes dans le tableau des formes : `shapes[0]` = joueur,
/// `shapes[1]` = station (dur en dur dans `mainLoop`).
pub const PLAYER_INDEX: usize = 0;
pub const STATION_INDEX: usize = 1;

/// Handles de texture (index dans la table des assets ; 0 = aucune).
/// NB : `TEXTURE_ORANGE` (ex `txtr&`, `orange2.png`) n'est utilisée que dans
/// un `_MapTriangle` commenté de l'original — conservée pour la fidélité.
pub const TEXTURE_NONE: i32 = 0;
#[allow(dead_code)]
pub const TEXTURE_ORANGE: i32 = 1;
pub const TEXTURE_METEOR: i32 = 2;
pub const TEXTURE_PLAYER: i32 = 3;
pub const TEXTURE_STATION: i32 = 4;

// ─── Constantes de gameplay dérivées (ANALYSE.md §6) ─────────────────────────

/// Accélération du joueur (par seconde de jeu : `60*0.05/fps` → `0.05*60*dt`).
pub const PLAYER_ACCELERATION: f64 = 0.05;
/// Rotation du joueur (rad/s : `60*(TAU/210)/fps` → `(TAU/210)*60*dt`).
pub const PLAYER_ROTATION_SPEED: f64 = TAU / 210.0;
/// Cooldown de tir en secondes (`fps/3` frames à 60 FPS = 1/3 s).
pub const PLAYER_FIRE_COOLDOWN: f64 = 1.0 / 3.0;
/// Cargo du joueur.
pub const CARGO_SIZE: i32 = 5;
/// Nombre initial de météores max, +1 par météore détruit, plafonné à SHAPES_COUNT.
pub const INITIAL_MAX_METEOR_SHAPES: i32 = 15;
/// Vitesse maximale des météores (`2*rnd`).
pub const METEOR_VELOCITY_MAX: f64 = 2.0;
/// Distance d'accostage de la station.
pub const STATION_DOCK_DISTANCE: f64 = 5.0;
/// Rayon de la station (forcé après calcul du centre).
pub const STATION_RADIUS: f64 = 36.0;
/// Mapping UV de la station : `station.png` est un anneau fin (bord intérieur
/// UV ~0.34, extérieur ~0.5) plus étroit que la bande du mesh (rayon
/// 90-163). À l'échelle normale (÷320), les dents cardinales (rayon 160 →
/// UV 0.0/0.5) tombent sur le pixel vide du bord de la texture (défauts à
/// droite et en bas) ; augmenter l'échelle pousse les creux (rayon 110) sous
/// le bord intérieur (nouveaux trous). On utilise donc un mapping radial :
/// la bande du mesh [90, 163] est compressée dans la bande pleine de la
/// texture [0.36, 0.48] → anneau complet. Dérive volontaire de l'original,
/// dont la station était dégradée par le bug `computeShapeCenter shapes(0)`
/// (sa largeur n'était jamais calculée → ratio UV divisé par 0).
pub const STATION_UV_R_INNER: f64 = 90.0;
pub const STATION_UV_R_OUTER: f64 = 163.0;
pub const STATION_UV_INNER: f64 = 0.36;
pub const STATION_UV_OUTER: f64 = 0.48;
/// Débris générés par triangle détruit.
pub const GARBAGE_PER_TRIANGLE: usize = 12;

/// Construit une couleur ARGB 32 bits au format QB64 (AARRGGBB).
pub const fn argb32(a: u32, r: u32, g: u32, b: u32) -> u32 {
    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}
