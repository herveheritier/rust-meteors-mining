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

/// Titre de la fenêtre (utilisé par la fenêtre macroquad ET pour retrouver
/// la fenêtre en X11 lors du plein écran EWMH — voir `src/x11.rs`).
pub const WINDOW_TITLE: &str = "Meteors Mining (Rust port)";

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

/// 2π (ex `TAU` global du jeu QB64).
pub const TAU: f64 = TAU_F64;

/// Identifiants `whoIam` des formes.
pub const WHOIAM_METEOR: i32 = 0;
pub const WHOIAM_BULLET: i32 = 1;
pub const WHOIAM_PLAYER: i32 = 2;
pub const WHOIAM_GEM: i32 = 3;
pub const WHOIAM_STATION: i32 = 4;
pub const WHOIAM_ALIEN: i32 = 5;
/// Cosmonaute décoratif chargé depuis `assets/cosmonaute.json` (export
/// « meshes-designer », voir `cosmonaut.rs`) : jamais détruit, aucun collider.
pub const WHOIAM_COSMONAUT: i32 = 6;

/// Modes de déplacement du vaisseau.
pub const MOVING_MODE_INERTIAL: i32 = 0;
pub const MOVING_MODE_4_WAYS: i32 = 1;
pub const MOVING_MODE_DIRECTIONAL: i32 = 2;
/// Nombre de modes de déplacement (bornes du focus clavier de l'écran de
/// paramétrage).
pub const MOVING_MODE_COUNT: i32 = 3;

/// Libellé d'affichage d'un mode de déplacement (écran de paramétrage,
/// message HUD d'activation).
pub fn moving_mode_label(mode: i32) -> &'static str {
    match mode {
        MOVING_MODE_INERTIAL => "INERTIAL",
        MOVING_MODE_4_WAYS => "4 WAYS",
        MOVING_MODE_DIRECTIONAL => "DIRECTIONAL",
        _ => "?",
    }
}

/// Styles de rendu des triangles (écran de paramétrage, touche O).
pub const RENDER_STYLE_TEXTURED: i32 = 0;
pub const RENDER_STYLE_COLORED: i32 = 1;
pub const RENDER_STYLE_MESH: i32 = 2;
pub const RENDER_STYLE_COUNT: i32 = 3;

/// Libellé d'affichage d'un style de rendu (écran de paramétrage).
pub fn render_style_label(style: i32) -> &'static str {
    match style {
        RENDER_STYLE_TEXTURED => "TEXTURED",
        RENDER_STYLE_COLORED => "COLORED",
        RENDER_STYLE_MESH => "MESH",
        _ => "?",
    }
}

/// Modes d'affichage de la fenêtre (écran de paramétrage, touche O ; mêmes
/// valeurs que `ViewMode` de `state.rs`, en entiers pour la persistance).
pub const WINDOW_MODE_WINDOWED: i32 = 0;
pub const WINDOW_MODE_ZOOMED: i32 = 1;
pub const WINDOW_MODE_NATIVE: i32 = 2;
pub const WINDOW_MODE_COUNT: i32 = 3;

/// Libellé d'affichage d'un mode d'affichage (écran de paramétrage, message
/// HUD de la touche F).
pub fn window_mode_label(mode: i32) -> &'static str {
    match mode {
        WINDOW_MODE_WINDOWED => "WINDOWED",
        WINDOW_MODE_ZOOMED => "ZOOMED",
        WINDOW_MODE_NATIVE => "NATIVE",
        _ => "?",
    }
}

/// Définitions de fenêtre proposées (écran de paramétrage, touche O) :
/// (largeur, hauteur). La vue 960×540 est rendue 1:1 dans une fenêtre à la
/// définition native et étirée (letterbox) dans toute fenêtre plus grande.
pub const WINDOW_SIZES: [(i32, i32); 4] = [(960, 540), (1280, 720), (1600, 900), (1920, 1080)];

/// Libellé d'affichage d'une définition de fenêtre (écran de paramétrage).
pub fn window_size_label(index: i32) -> String {
    match WINDOW_SIZES.get(index as usize) {
        Some(&(w, h)) => format!("{}x{}", w, h),
        None => "?".to_string(),
    }
}

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
/// Rayon (unités monde) de la zone d'accostage de la station : le vaisseau
/// accoste quand son **centre** entre dans ce cercle (vérification circulaire
/// dans `docking`). Élargie par rapport à l'original (5 px) pour dépasser le
/// rayon du vaisseau (10) — la zone est affichée par la mire au centre de la
/// station (voir `render::draw_docking_marker`).
pub const STATION_DOCK_DISTANCE: f64 = 15.0;

/// Rayon (unités monde) de ramassage des gemmes par le **cosmonaute EVA**
/// (vaisseau détruit) : il est non-collider, les gemmes le traversent — il
/// les ramasse par proximité (voir `game::eva_collect_gems`) et les rapporte
/// à la station. Rayon du cosmonaute (~13) + marge généreuse.
pub const EVA_PICKUP_RADIUS: f64 = 20.0;

/// Durée (secondes) de l'animation d'accostage avant l'ouverture de la boîte
/// DOCK STATION : le monde est gelé, le vaisseau pivote vers la droite
/// (orientation 0) tout en se recentrant exactement au centre de la station,
/// et 4 traits néon relient le bord intérieur de la station aux côtés du
/// vaisseau (voir `render::draw_docking_line` et
/// `game::advance_dock_animation`).
pub const DOCK_ANIMATION_DURATION: f64 = 3.0;

/// Durée (secondes) de la rétraction des liens d'accostage au départ
/// (bouton CLOSE de la boîte DOCK STATION) : le monde est gelé, le vaisseau
/// reste au centre, les 4 traits néon se rétractent du vaisseau vers le bord
/// intérieur de l'anneau, puis le vaisseau est libre (voir
/// `render::draw_docking_line` et `game::advance_dock_retract`).
pub const DOCK_RETRACT_DURATION: f64 = 1.5;

/// Rayon intérieur (unités monde) de l'anneau de la station (bord intérieur
/// du mesh, r = 110) : point de départ du trait d'accostage pendant
/// l'animation (voir `render::draw_docking_line`).
pub const STATION_INNER_RADIUS: f64 = 110.0;

/// Vitesse maximale (unités/s) du vaisseau pour que l'accostage se **termine**
/// (la boîte DOCK STATION ne s'ouvre que si le vaisseau est presque immobile
/// dans la zone — voir `docking`).
pub const STATION_DOCK_SPEED: f64 = 0.5;

/// Vitesse à partir de laquelle l'approche est jugée « mauvaise » : la mire
/// d'accostage est entièrement **rouge** (qualité 0) à cette vitesse ou au
/// delà, et passe progressivement au **vert** (qualité 1) à mesure que le
/// vaisseau ralentit — la qualité est interpolée sur **tout le rayon de la
/// station** (distance au centre) et sur la vitesse, voir
/// `render::draw_docking_marker` et `docking_approach_quality`.
pub const DOCK_APPROACH_FULL_RED_SPEED: f64 = 3.0;
/// Mapping UV de la station : `station.png` est un anneau fin (bord intérieur
/// UV ~0.34, extérieur ~0.5) plus étroit que la bande du mesh (rayon
/// 110-162). Un mapping d'échelle simple ferait tomber le bord de l'anneau
/// sur le pixel vide de la texture (défauts à droite et en bas). On utilise
/// donc un mapping radial : la bande du mesh, incluse dans [90, 163], est
/// compressée dans la bande pleine de la texture [0.36, 0.48] → anneau
/// complet. Dérive volontaire de l'original, dont la station était dégradée
/// par le bug `computeShapeCenter shapes(0)` (sa largeur n'était jamais
/// calculée → ratio UV divisé par 0).
pub const STATION_UV_R_INNER: f64 = 90.0;
pub const STATION_UV_R_OUTER: f64 = 163.0;
pub const STATION_UV_INNER: f64 = 0.36;
pub const STATION_UV_OUTER: f64 = 0.48;
/// Débris générés par triangle détruit.
pub const GARBAGE_PER_TRIANGLE: usize = 12;

/// Zoom avant appliqué à la texture des météores (`meteor_surface_tile.jpg`,)
/// par rapport à la formule d'origine (`ratio = tw / larger`, une tuile par
/// météore). Sans zoom, la tuile 1254 px est compressée dans la taille du
/// météore → détail sub-pixel, rendu « zoom arrière » (bruit gris). Avec ce
/// facteur, le motif de roche occupe `METEOR_TEXTURE_ZOOM×` plus d'écran
/// (chaque météore affiche la région centrale 1/M de la tuile).
pub const METEOR_TEXTURE_ZOOM: f64 = 4.0;

/// Construit une couleur ARGB 32 bits au format QB64 (AARRGGBB).
pub const fn argb32(a: u32, r: u32, g: u32, b: u32) -> u32 {
    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}
