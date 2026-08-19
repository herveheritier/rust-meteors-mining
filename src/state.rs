//! État du jeu, joueur et éléments.
//!
//! Portage de `context_type.bas` (partie dynamique) et `player_type.bas` :
//! les constantes de `context_type` sont devenues des `const` dans
//! `src/config.rs` — seul l'état mutable reste ici.

use crate::config::{
    ATTEMPT_FPS, CARGO_SIZE, MOVING_MODE_COUNT, MOVING_MODE_DIRECTIONAL, PLAYER_INDEX, WORLD_HEIGHT,
    WORLD_MAXX, WORLD_MAXY, WORLD_MINX, WORLD_MINY, WORLD_WIDTH,
};
use crate::geom::{Point, World};
// population de météores : constante de la carte « Météores & collisions »
// de l'outil de gestion (src/marketplace.rs, généré)
use crate::marketplace::INITIAL_MAX_METEOR_SHAPES;
use crate::scenario::{Resources, ScenarioId};

/// Mode d'affichage (touche F — cycle) : fenêtré → plein écran zoomé → plein
/// écran natif → fenêtré.
///
/// - `Windowed` : fenêtre 960×540, rendu direct 1:1 (pas de render target).
/// - `Zoomed` : plein écran EWMH, vue 960×540 rendue dans une texture puis
///   étirée (le mode historique du port).
/// - `Native` : plein écran EWMH, rendu direct **à la définition réelle de
///   l'écran** (caméra zoomée), sans passage par un render target — un seul
///   passage de rendu, image plus nette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Windowed,
    Zoomed,
    Native,
}

/// Style de rendu des triangles (écran de paramétrage, touche O) :
/// - `Textured` — les textures (`_MapTriangle` de l'original) ;
/// - `Colored` — remplissage uni avec la couleur de l'élément / de la forme ;
/// - `Mesh` — fil de fer (arêtes seules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStyle {
    Textured,
    Colored,
    Mesh,
}

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
    /// Scénario actif (choisi à l'écran titre, touche N) — voir `scenario.rs`.
    pub scenario: ScenarioId,
    /// Ressources économiques du scénario (carburant, munitions, minerais,
    /// réputation) — ignorées en jeu libre (`has_economy` = false).
    pub resources: Resources,
    /// Modes de déplacement débloqués (index `MOVING_MODE_*`) : en
    /// Progression, INERTIAL est gratuit et les autres s'achètent en minerais
    /// dans l'écran de paramétrage (touche O) ; en jeu libre, tous sont
    /// débloqués.
    pub unlocked_modes: [bool; MOVING_MODE_COUNT as usize],
    /// Coût du dernier ravitaillement refusé à la station (0 = aucun) : évite
    /// de répéter « NOT ENOUGH MINERALS » à chaque clic sur REFUEL/REARM sans
    /// assez de minerais (`scenario::purchase_supplies`).
    pub supplies_shortage_cost: i32,
    /// Pause (touche P) : gèle déplacements et collisions, mais pas le rendu
    /// ni l'input (voir `docs/PORTAGE.md` §6).
    pub paused: bool,
    /// Fin de partie (scénario Survival, dernière vie perdue) : le monde est
    /// gelé, le HUD affiche GAME OVER et seule la touche ESC (quitter) reste
    /// active. Remis à faux par `scenario::apply_start` (nouvelle partie).
    pub game_over: bool,
    /// Invulnérabilité restante en secondes (scénario Survival, après un
    /// respawn) : les impacts sont absorbés sans toucher au bouclier — le
    /// vaisseau clignote. Décrémentée à chaque frame (`game.rs`), remise à
    /// `respawn_invulnerability` par `scenario::player_hit`.
    pub invulnerable: f64,
    /// Mode d'affichage (touche F, cycle) — local de `mainLoop` devenu champ
    /// d'état : fenêtré → plein écran zoomé → plein écran natif (voir
    /// `ViewMode`).
    pub view_mode: ViewMode,
    /// Génération automatique des météores (touche A, ex `autoGenerateShape%`).
    pub auto_generate: bool,
    /// Style de rendu des triangles (écran de paramétrage) — voir
    /// `RenderStyle`.
    pub render_style: RenderStyle,
    /// Index dans `WINDOW_SIZES` de la définition de fenêtre choisie (écran
    /// de paramétrage) : 0 = 960×540 (défaut), 1 = 1280×720, etc.
    pub window_size: i32,
    /// Anticrénelage MSAA 4× (écran de paramétrage) : appliqué à la **création
    /// de la fenêtre** (macroquad ne permet pas de le changer à chaud) — la
    /// valeur prend effet au prochain lancement.
    pub antialias: bool,
    /// Valeur d'anticrénelage effectivement appliquée par la fenêtre au
    /// lancement (`Conf.sample_count`). Si `antialias` en diffère, un
    /// redémarrage est nécessaire (bouton RESTART de l'écran de paramétrage).
    pub antialias_applied: bool,
    /// Nombre max de météores : 15 au départ, +1 par météore détruit (M4),
    /// plafonné à `SHAPES_COUNT` (ex `maxMeteorShapes%`).
    pub max_meteor_shapes: i32,
    /// Animation d'accostage en cours (secondes restantes, 0 = aucune) :
    /// avant d'ouvrir la boîte DOCK STATION, le vaisseau pivote vers la droite
    /// (orientation 0) tout en se recentrant au centre de la station, pendant
    /// `DOCK_ANIMATION_DURATION` — le monde est gelé (voir
    /// `game::advance_dock_animation` et `render::draw_docking_line`).
    pub dock_anim: f64,
    /// Position du vaisseau au début de l'animation d'accostage (interpolée
    /// vers le centre de la station).
    pub dock_anim_from_pos: Point,
    /// Orientation du vaisseau au début de l'animation d'accostage (interpolée
    /// vers 0 = pointe vers la droite).
    pub dock_anim_from_orient: f64,
    /// Rétraction des liens d'accostage en cours (secondes restantes, 0 =
    /// aucune) : au départ (bouton CLOSE de la boîte DOCK STATION), le vaisseau
    /// reste au centre et les 4 traits néon se rétractent vers le bord
    /// intérieur de l'anneau pendant `DOCK_RETRACT_DURATION` — le monde est
    /// gelé (voir `game::advance_dock_retract` et
    /// `render::draw_docking_line`).
    pub dock_retract: f64,
    /// Liens d'accostage **attachés à quai** : vrai au lancement et après un
    /// respawn (le vaisseau démarre à la station) — les 4 traits néon sont
    /// tendus jusqu'au vaisseau au centre, la mire est cachée. Dès que le
    /// joueur donne une commande de mouvement, `game::release_links` les
    /// rétracte (comme au départ après CLOSE) puis le vaisseau est libre.
    pub dock_links: bool,
    /// Guide d'accostage (la mire) **affiché lors du retour à la base** : vrai
    /// quand le vaisseau a quitté la base puis a **recroisé sa limite
    /// extérieure en entrant** (voir `game::update_docking_guide`) — jamais
    /// pendant qu'il quitte l'accostage ni à quai. Faux dès qu'il accoste
    /// (`game::docking`) ou quitte la base (`game::release_links`).
    pub docking_guide: bool,
    /// Le vaisseau était **hors de la limite extérieure** de la base à la
    /// frame précédente : détection du franchissement **en entrant** (retour)
    /// par `game::update_docking_guide` (front montant de la distance).
    pub dock_was_outside: bool,
    /// Vaisseau détruit, le joueur contrôle le **cosmonaute EVA éjecté** : son
    /// seul objectif est de rejoindre la base (zone d'accostage au centre) où
    /// il est secouru et le vaisseau reconstruit (voir `game::rescue_cosmonaut`).
    /// Pendant ce temps la caméra, la mire et le HUD suivent le cosmonaute.
    pub cosmonaut_active: bool,
    /// Index de la forme « cosmonaute EVA » dans `shapes` (-1 tant qu'elle
    /// n'est pas créée par `main.rs`).
    pub eva_cosmonaut: i32,
    /// Récupération du cosmonaute EVA en cours (secondes restantes, 0 =
    /// aucune) : vaisseau détruit, il a rejoint la zone d'accostage — un
    /// cordon jaillit de l'anneau jusqu'à lui et le ramène sur l'anneau
    /// pendant `EVA_RECOVERY_DURATION` (monde gelé, voir
    /// `game::advance_eva_recovery` et `render::draw_eva_recovery_cable`).
    pub eva_recovery: f64,
    /// Position du cosmonaute au début de la récupération (interpolée vers
    /// `eva_recovery_to_pos`, le point de l'anneau où le cordon le ramène).
    pub eva_recovery_from_pos: Point,
    /// Point de l'anneau (rayon `STATION_INNER_RADIUS`) où le cordon ramène
    /// le cosmonaute — dans sa direction au moment de la récupération.
    pub eva_recovery_to_pos: Point,
    /// Fondu enchaîné de la récupération en cours (secondes restantes, 0 =
    /// aucune) : le cosmonaute sur l'anneau s'efface pendant que le vaisseau
    /// reconstruit apparaît au centre de la station, liens attachés, pendant
    /// `EVA_CROSSFADE_DURATION` (voir `game::advance_eva_crossfade`).
    pub eva_crossfade: f64,
    /// Boîte de choix DOCK STATION ouverte (accostage) — ex la boucle
    /// bloquante de `windowUtils_choiceBox` : tant qu'elle est ouverte, le
    /// jeu est gelé et seuls les clics sur UNLOAD / REFUEL/REARM / UPGRADES /
    /// CLOSE sont traités (UNLOAD et REFUEL/REARM gardent la boîte ouverte).
    pub dock_box: bool,
    /// Atelier d'amélioration du vaisseau ouvert (bouton UPGRADES de la boîte
    /// DOCK STATION, scénario à économie) : achats d'extensions (réservoir,
    /// chargeur, soute) contre minerais — le jeu est gelé tant qu'il est
    /// affiché ; CLOSE revient à la boîte DOCK STATION (toujours accosté).
    pub workshop_box: bool,
    /// Fenêtre d'aide ouverte (touche S, ex `help` de windowUtils) : le jeu
    /// est gelé tant qu'elle est affichée (bouton CLOSE).
    pub help_box: bool,
    /// Écran de paramétrage ouvert (touche O) : choix du mode de déplacement
    /// du vaisseau (INERTIAL / 4 WAYS / DIRECTIONAL). Le jeu est gelé tant
    /// qu'il est affiché ; à la fermeture, un message HUD signale le mode
    /// activé s'il a été modifié.
    pub settings_box: bool,
    /// Mode de déplacement à l'ouverture de l'écran de paramétrage : s'il
    /// diffère de `moving_mode` à la fermeture, le changement est annoncé au
    /// HUD.
    pub settings_previous_mode: i32,
    /// Index du radio-bouton sous le focus clavier (flèches ↑/↓ + Entrée) de
    /// l'écran de paramétrage, borné à `[0, MOVING_MODE_COUNT-1]`. Recentré
    /// sur le mode courant à l'ouverture et à chaque clic.
    pub settings_focus: i32,
    /// Affiche les données de debug des formes (touche D, ex `showData%`).
    pub show_data: bool,
    /// Affiche les informations de debug (touche I, ex `showInfo%`).
    pub show_info: bool,
    /// Dernier keycode pressé (affiché par le mode I, ex `keycode = inp(96)`).
    pub last_keycode: i32,
    /// État de la touche F à la frame précédente (front montant de
    /// `is_key_down` — voir `game::f_pressed`) : détecte la pression même
    /// quand macroquad l'a avalée comme « répétition » (relâchement perdu
    /// pendant la bascule plein écran).
    pub f_was_down: bool,
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
            scenario: ScenarioId::FreePlay,
            resources: Resources::default(),
            unlocked_modes: [true; MOVING_MODE_COUNT as usize],
            supplies_shortage_cost: 0,
            paused: false,
            game_over: false,
            invulnerable: 0.0,
            view_mode: ViewMode::Windowed,
            auto_generate: true,
            render_style: RenderStyle::Textured,
            window_size: 0,
            antialias: false,
            antialias_applied: false,
            max_meteor_shapes: INITIAL_MAX_METEOR_SHAPES,
            dock_anim: 0.0,
            dock_anim_from_pos: Point::new(0.0, 0.0),
            dock_anim_from_orient: 0.0,
            dock_retract: 0.0,
            dock_links: true, // le vaisseau démarre à quai, liens attachés
            docking_guide: false, // pas encore revenu à la base
            dock_was_outside: false,
            cosmonaut_active: false,
            eva_cosmonaut: -1, // créé par main.rs au démarrage
            eva_recovery: 0.0,
            eva_recovery_from_pos: Point::new(0.0, 0.0),
            eva_recovery_to_pos: Point::new(0.0, 0.0),
            eva_crossfade: 0.0,
            dock_box: false,
            workshop_box: false,
            help_box: false,
            settings_box: false,
            settings_previous_mode: MOVING_MODE_DIRECTIONAL,
            settings_focus: MOVING_MODE_DIRECTIONAL,
            show_data: false,
            show_info: false,
            last_keycode: 0,
            f_was_down: false,
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
