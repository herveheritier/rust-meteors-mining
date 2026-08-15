//! État du jeu, joueur et éléments.
//!
//! Portage de `context_type.bas` (partie dynamique) et `player_type.bas` :
//! les constantes de `context_type` sont devenues des `const` dans
//! `src/config.rs` — seul l'état mutable reste ici.

use crate::config::{
    ATTEMPT_FPS, CARGO_SIZE, FULL_SCREEN, INITIAL_MAX_METEOR_SHAPES, MOVING_MODE_DIRECTIONAL,
    PLAYER_INDEX, WORLD_HEIGHT, WORLD_MAXX, WORLD_MAXY, WORLD_MINX, WORLD_MINY, WORLD_WIDTH,
};
use crate::geom::World;

/// Le joueur (ex `player_type`).
#[derive(Clone, Debug)]
pub struct Player {
    /// NB : le jeu passe par la constante `PLAYER_INDEX` (`shapes[0]`) ; le
    /// champ est conservé pour la fidélité à `player_type.shapeIndex`.
    #[allow(dead_code)]
    pub shape_index: usize,
    pub thrust: f64,
    /// Compteur de frames de poussée avant : négatif tant que la flamme est
    /// affichée (-5 au départ, +1 par frame jusqu'à 0), comme l'original.
    pub thrusted: i32,
    /// Idem pour la poussée arrière.
    pub revert_thrusted: i32,
    /// Cooldown de tir en secondes restantes (0 = prêt à tirer).
    pub fire: f64,
    pub cargo_size: i32,
    pub cargo_qty: i32,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            shape_index: PLAYER_INDEX,
            thrust: 0.0,
            thrusted: 0,
            revert_thrusted: 0,
            fire: 0.0,
            cargo_size: CARGO_SIZE,
            cargo_qty: 0,
        }
    }
}

/// Un élément minéral (ex `element_type`).
///
/// NB : l'original lit ses `DATA` en sens inverse, si bien que
/// `elements[1] = GOLD`, `elements[2] = IRON`, `elements[3] = WATER`
/// (voir `docs/ASSETS.md` §3). L'index 0 est un élément factice jamais utilisé
/// (les éléments valides d'un triangle vont de 1 à 3).
#[derive(Clone, Debug)]
pub struct Element {
    /// NB : `id`/`name` ne sont jamais lus, ni dans le port ni dans
    /// l'original (seuls `color` et `count` servent) — conservés pour la
    /// fidélité à `element_type`.
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub name: String,
    /// Couleur ARGB 32 bits au format QB64 (AARRGGBB).
    pub color: u32,
    pub count: i32,
}

/// Construit le tableau des éléments (ex `prepare`, données `elements:`).
pub fn default_elements() -> Vec<Element> {
    vec![
        Element {
            id: 0,
            name: String::new(),
            color: 0,
            count: 0,
        },
        Element {
            id: 2,
            name: "GOLD".into(),
            color: 0xFFD0D010,
            count: 0,
        },
        Element {
            id: 1,
            name: "IRON".into(),
            color: 0xFFC0C0C0,
            count: 0,
        },
        Element {
            id: 0,
            name: "WATER".into(),
            color: 0xFF8080FF,
            count: 0,
        },
    ]
}

/// État dynamique du jeu (ex `context_type`, sans les constantes).
#[derive(Clone, Debug)]
pub struct GameState {
    pub world: World,
    pub player: Player,
    /// FPS mesurés (affichés au HUD, utilisés par les formules de l'original).
    pub fps: i32,
    pub player_at_station: i32,
    pub player_enter_station: i32,
    pub meteors_destroyed: i32,
    pub bullets_fired: i32,
    pub bullets_lost: i32,
    pub moving_mode: i32,
    /// Pause (touche P) : gèle déplacements et collisions, mais pas le rendu
    /// ni l'input (voir `docs/PORTAGE.md` §6).
    pub paused: bool,
    /// Plein écran (touche F) — local de `mainLoop` devenu champ d'état.
    pub fullscreen: bool,
    /// Génération automatique des météores (touche A, ex `autoGenerateShape%`).
    pub auto_generate: bool,
    /// Nombre max de météores : 15 au départ, +1 par météore détruit (M4),
    /// plafonné à `SHAPES_COUNT` (ex `maxMeteorShapes%`).
    pub max_meteor_shapes: i32,
    /// Boîte de choix DOCK STATION ouverte (accostage) — ex la boucle
    /// bloquante de `windowUtils_choiceBox` : tant qu'elle est ouverte, le
    /// jeu est gelé et seuls les clics sur UNLOAD/CLOSE sont traités.
    pub dock_box: bool,
    /// Fenêtre d'aide ouverte (touche S, ex `help` de windowUtils) : le jeu
    /// est gelé tant qu'elle est affichée (bouton CLOSE).
    pub help_box: bool,
    /// Affiche les données de debug des formes (touche D, ex `showData%`).
    pub show_data: bool,
    /// Affiche les informations de debug (touche I, ex `showInfo%`).
    pub show_info: bool,
    /// Dernier keycode pressé (affiché par le mode I, ex `keycode = inp(96)`).
    pub last_keycode: i32,
    // messages (ex sendMessage/drawMessage, voir mainLoop.bas)
    pub message_delay: f64,
    pub message: String,
    pub message_queue: String,
    pub message1: String,
    pub message2: String,
}

impl GameState {
    /// Ex `defineWorld` appliqué aux constantes du jeu.
    pub fn new() -> Self {
        GameState {
            world: World::define(
                WORLD_WIDTH,
                WORLD_HEIGHT,
                WORLD_MINY,
                WORLD_MINX,
                WORLD_MAXY,
                WORLD_MAXX,
            ),
            player: Player::default(),
            fps: ATTEMPT_FPS,
            player_at_station: -1,
            player_enter_station: 0,
            meteors_destroyed: 0,
            bullets_fired: 0,
            bullets_lost: 0,
            moving_mode: MOVING_MODE_DIRECTIONAL,
            paused: false,
            fullscreen: FULL_SCREEN,
            auto_generate: true,
            max_meteor_shapes: INITIAL_MAX_METEOR_SHAPES,
            dock_box: false,
            help_box: false,
            show_data: false,
            show_info: false,
            last_keycode: 0,
            message_delay: 0.0,
            message: String::new(),
            message_queue: String::new(),
            message1: String::new(),
            message2: String::new(),
        }
    }

    /// Met un message en file d'attente (ex `sendMessage`).
    pub fn send_message(&mut self, message: &str) {
        if self.message_queue.is_empty() && self.message.is_empty() {
            self.message_delay = -1.0;
        } else {
            self.message_delay = 0.5;
        }
        self.message_queue.push_str(message);
        self.message_queue.push('/');
    }
}

impl Default for GameState {
    fn default() -> Self {
        GameState::new()
    }
}
