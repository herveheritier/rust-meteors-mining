//! État du jeu, joueur et éléments.
//!
//! Portage de `context_type.bas` (partie dynamique) et `player_type.bas` :
//! les constantes de `context_type` sont devenues des `const` dans
//! `src/config.rs` - seul l'état mutable reste ici.

use crate::config::{
    ATTEMPT_FPS, BOSS_SPAWN_INTERVAL, CARGO_SIZE, CRAFT_COUNT, MOVING_MODE_COUNT,
    MOVING_MODE_DIRECTIONAL, PLAYER_INDEX, WARP_GATE_SPAWN_INTERVAL, WEAPON_SLOTS, WORLD_HEIGHT,
    WORLD_MAXX, WORLD_MAXY, WORLD_MINX, WORLD_MINY, WORLD_WIDTH,
};
use crate::geom::{Point, World};
// population de météores : constante de la carte « Météores & collisions »
// de l'outil de gestion (src/marketplace.rs, généré)
use crate::marketplace::INITIAL_MAX_METEOR_SHAPES;
use crate::scenario::{RadarKind, Resources, ScenarioId};

/// Mode d'affichage (touche F - cycle) : fenêtré → plein écran zoomé → plein
/// écran natif → fenêtré.
///
/// - `Windowed` : fenêtre 960×540, rendu direct 1:1 (pas de render target).
/// - `Zoomed` : plein écran EWMH, vue 960×540 rendue dans une texture puis
///   étirée (le mode historique du port).
/// - `Native` : plein écran EWMH, rendu direct **à la définition réelle de
///   l'écran** (caméra zoomée), sans passage par un render target - un seul
///   passage de rendu, image plus nette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Windowed,
    Zoomed,
    Native,
}

/// Style de rendu des triangles (écran de paramétrage, touche O) :
/// - `Textured` - les textures (`_MapTriangle` de l'original) ;
/// - `Colored` - remplissage uni avec la couleur de l'élément / de la forme ;
/// - `Mesh` - fil de fer (arêtes seules).
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
    /// Compteur de frames du **jet latéral gauche** (touche ←, rotation) :
    /// même principe que `thrusted` - le jet est dessiné tant qu'il est négatif.
    pub rotate_left_thrusted: i32,
    /// Compteur de frames du **jet latéral droit** (touche →, rotation).
    pub rotate_right_thrusted: i32,
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
            rotate_left_thrusted: 0,
            rotate_right_thrusted: 0,
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
    /// l'original (seuls `color` et `count` servent) - conservés pour la
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
/// Index 1 = GOLD, 2 = IRON, 3 = WATER, 4 = PLATINUM (minerai rare du
/// météore spécial - `ELEMENT_PLATINUM`, valeur de 10 crédits).
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
        Element {
            id: 0,
            name: "PLATINUM".into(),
            color: 0xFFFFE040,
            count: 0,
        },
    ]
}

/// État dynamique du jeu (ex `context_type`, sans les constantes).
#[derive(Clone, Debug)]
/// Statistiques de session - récapitulatif affiché à l'écran GAME OVER (et
/// aux objets des tests) : temps de vol, distance parcourue, précision de
/// tir, triangles minéralisés détruits, accostages, valeur totale déchargée.
pub struct SessionStats {
    /// Temps de vol total (secondes, hors pause).
    pub flight_time: f64,
    /// Distance totale parcourue (unités monde).
    pub distance: f64,
    /// Triangles minéralisés (or/fer/eau/platine) détruits par des tirs.
    pub minerals_destroyed: i32,
    /// Valeur totale (crédits) de la cargaison déchargée à la station.
    pub cargo_value_unloaded: i32,
}

impl Default for SessionStats {
    fn default() -> Self {
        SessionStats {
            flight_time: 0.0,
            distance: 0.0,
            minerals_destroyed: 0,
            cargo_value_unloaded: 0,
        }
    }
}

/// Écho du scope de contrôleur aérien (radar ATC - `render::draw_atc_radar`) :
/// la position **figée** d'une forme sur le scope, telle que peinte au
/// **dernier passage du balayage**. Entre deux passages, l'écho ne bouge pas :
/// il reste à cette position et s'estompe progressivement (persistance
/// décroissante) jusqu'au prochain rafraîchissement - un écho ne bouge que
/// quand le balayage est repassé dessus. `age` (radians de balayage écoulés
/// depuis le dernier rafraîchissement) pilote le fondu.
#[derive(Debug, Clone, Copy, Default)]
pub struct RadarEcho {
    /// Position de l'écho sur le scope, relative à son centre (px écran).
    pub x: f32,
    /// Position de l'écho sur le scope, relative à son centre (px écran).
    pub y: f32,
    /// Âge de l'écho en radians de balayage depuis son dernier rafraîchissement.
    pub age: f32,
}

/// Situation d'accostage du vaisseau, pour les **messages d'aide au pilote**
/// (`docking::docking`) : le message n'est envoyé qu'au **changement** de
/// situation (front montant - pas à chaque frame). La vitesse est jugée sur
/// **tout le rayon de la base** (comme la mire, rouge→vert - pas seulement
/// dans le petit cercle d'accostage au centre) : « SLOW DOWN » quand le
/// vaisseau est trop rapide pour accoster, « IN RANGE » quand il ralentit
/// assez, « ZONE LEFT » s'il ressort de la base sans accoster. Ne vaut que
/// lors du **retour à la base** (`docking_guide` actif) : en vol libre ou à
/// quai, aucune aide n'est envoyée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockHint {
    /// À quai, en vol libre, ou guide d'accostage coupé : pas de message.
    #[default]
    Docked,
    /// Dans la base (guide actif) mais trop rapide pour accoster.
    TooFast,
    /// Dans la base (guide actif) et presque immobile : l'accostage peut se
    /// terminer dès que le vaisseau atteint la zone au centre.
    InRange,
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
    /// Nombre total d'accostages réussis à la station (incrémenté à chaque
    /// animation d'accostage démarrée) - utilisé par les objectifs DAG.
    pub docking_count: i32,
    pub bullets_fired: i32,
    pub bullets_lost: i32,
    pub moving_mode: i32,
    /// Scénario actif (choisi à l'écran titre, touche N) - voir `scenario.rs`.
    pub scenario: ScenarioId,
    /// Version de radar de bord **active** - une seule à la fois : la minimap
    /// classique ou le scope de contrôleur aérien (`RadarKind`, choisi au
    /// magasin de la station, onglet ÉQUIPEMENT). En scénario à économie, la
    /// version doit avoir été achetée pour être sélectionnable
    /// (`scenario::radar_kind_available` - la version effective est
    /// `scenario::active_radar_kind`).
    pub radar_kind: RadarKind,
    /// Échos du scope de contrôleur aérien (radar ATC), indexés par index de
    /// forme (`shapes`) : positions **figées** des formes entre deux passages
    /// du balayage + âge du fondu (`RadarEcho`) - un écho ne bouge que quand
    /// le balayage le rafraîchit (voir `render::draw_atc_radar`).
    pub radar_echoes: Vec<RadarEcho>,
    /// Ressources économiques du scénario (carburant, munitions, minerais,
    /// réputation) - ignorées en jeu libre (`has_economy` = false).
    pub resources: Resources,
    /// Modes de déplacement débloqués (index historique `MOVING_MODE_*`) : en
    /// Progression, seuls les modes dont le coût configuré (outil) est nul
    /// sont débloqués au départ (REALISTIC par défaut, et INERTIAL s'il est
    /// paramétré gratuit) ; les modes payants (INERTIAL 15, 4 WAYS 30,
    /// DIRECTIONAL 45 par défaut) s'achètent en minerais au magasin de la
    /// station (bouton SHOP de la boîte DOCK STATION) ; en jeu libre, tous
    /// sont débloqués.
    pub unlocked_modes: [bool; MOVING_MODE_COUNT as usize],
    /// Coût du dernier ravitaillement refusé au magasin (0 = aucun) : évite
    /// de répéter « NOT ENOUGH MINERALS » à chaque clic sur les lignes FUEL /
    /// AMMO sans assez de minerais (`scenario::buy_fuel_qty` /
    /// `scenario::buy_ammo_qty`).
    pub supplies_shortage_cost: i32,
    /// Quantité de carburant sélectionnée sur le curseur du magasin (section
    /// RAVITAILLEMENT, ligne FUEL) : achetée par clic sur la ligne - bornée
    /// au manque du réservoir et à ce que les minerais permettent
    /// (`scenario::clamp_shop_quantities`).
    pub shop_fuel_qty: f64,
    /// Quantités de munitions sélectionnées sur les curseurs du magasin (une
    /// par arme du catalogue, ligne AMMO de l'arme possédée) - achetées par
    /// clic sur la ligne, bornées comme `shop_fuel_qty`.
    pub shop_ammo_qty: [f64; WEAPON_SLOTS],
    /// Curseur du magasin en cours de glisser (`Some(0)` = carburant,
    /// `Some(1 + i)` = munitions de l'arme `i`) - `None` = aucun. La valeur
    /// suit le pointeur tant que le bouton est maintenu (`game.rs`).
    pub shop_drag: Option<usize>,
    /// Onglet actif du magasin de la station (bouton SHOP de la boîte DOCK
    /// STATION) : RAVITAILLEMENT, ÉQUIPEMENT, ATELIER ou MODE DE VOL
    /// (`SHOP_TAB_*`, défaut = RAVITAILLEMENT).
    pub shop_tab: u8,
    /// Dernier retour d'action du magasin (achat confirmé ou refus) affiché
    /// en bas de la fenêtre - vide si rien à afficher. Texte + drapeau
    /// succès/échec (`shop_feedback_ok`) pour la couleur (vert / rouge).
    pub shop_feedback: String,
    /// `true` = succès (vert), `false` = refus (rouge) pour
    /// `shop_feedback`.
    pub shop_feedback_ok: bool,
    /// Pause (touche P) : gèle déplacements et collisions, mais pas le rendu
    /// ni l'input (voir `docs/PORTAGE.md` §6).
    pub paused: bool,
    /// Fin de partie (scénario Survival, dernière vie perdue) : le monde est
    /// gelé, le HUD affiche GAME OVER et seule la touche ESC (quitter) reste
    /// active. Remis à faux par `scenario::apply_start` (nouvelle partie).
    pub game_over: bool,
    /// Invulnérabilité restante en secondes (scénario Survival, après un
    /// respawn) : les impacts sont absorbés sans toucher au bouclier - le
    /// vaisseau clignote. Décrémentée à chaque frame (`game.rs`), remise à
    /// `respawn_invulnerability` par `scenario::player_hit`.
    pub invulnerable: f64,
    /// Mode d'affichage (touche F, cycle) - local de `mainLoop` devenu champ
    /// d'état : fenêtré → plein écran zoomé → plein écran natif (voir
    /// `ViewMode`).
    pub view_mode: ViewMode,
    /// Génération automatique des météores (touche A, ex `autoGenerateShape%`).
    pub auto_generate: bool,
    /// Style de rendu des triangles (écran de paramétrage) - voir
    /// `RenderStyle`.
    pub render_style: RenderStyle,
    /// Index dans `WINDOW_SIZES` de la définition de fenêtre choisie (écran
    /// de paramétrage) : 0 = 960×540 (défaut), 1 = 1280×720, etc.
    pub window_size: i32,
    /// Anticrénelage MSAA 4× (écran de paramétrage) : appliqué à la **création
    /// de la fenêtre** (macroquad ne permet pas de le changer à chaud) - la
    /// Anticrénelage MSAA 4× (écran de paramétrage) : appliqué à la **création
    /// de la fenêtre** (macroquad ne permet pas de le changer à chaud) - la
    /// valeur prend effet au prochain lancement.
    pub antialias: bool,
    /// Interface tactile affichée et active (case TOUCH UI de l'écran de
    /// paramétrage, persistée - clé `touch_ui`) : joystick virtuel bas-gauche
    /// et bouton de tir bas-droite (`touch.rs`). Masquée (et inopérante) quand
    /// le réglage est éteint - le jeu se pilote alors au clavier seul.
    pub touch_ui: bool,
    /// Sauvegarde la **position du vaisseau** à la sortie (touche ESC, case
    /// SAVE POSITION de l'écran de paramétrage, clé `save_position`) : au
    /// lancement suivant, le vaisseau repart de la dernière position (au lieu
    /// du centre de la station) - débarqué, sans liens d'accostage, comme une
    /// position initiale de scénario custom (`initial_ship_*`).
    pub save_position: bool,
    /// Étoiles du fond **agrandies** (case STARS 3x3 de l'écran de
    /// paramétrage, clé `stars_big`) : chaque étoile est dessinée en 3×3 px
    /// au lieu de 1×1 - pour les écrans (ou les réglages de l'OS) où le
    /// champ d'étoiles 1×1 est peu visible. Éteinte par défaut (1×1).
    pub stars_big: bool,
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
    /// `DOCK_ANIMATION_DURATION` - le monde continue de tourner (voir
    /// `docking::advance_dock_animation` et `render::draw_docking_line`).
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
    /// intérieur de l'anneau pendant `DOCK_RETRACT_DURATION` - le monde
    /// continue de tourner (voir `docking::advance_dock_retract` et
    /// `render::draw_docking_line`).
    pub dock_retract: f64,
    /// Liens d'accostage **attachés à quai** : vrai au lancement et après un
    /// respawn (le vaisseau démarre à la station) - les 4 traits néon sont
    /// tendus jusqu'au vaisseau au centre, la mire est cachée. Dès que le
    /// joueur donne une commande de mouvement, `docking::release_links` les
    /// rétracte (comme au départ après CLOSE) puis le vaisseau est libre.
    pub dock_links: bool,
    /// Guide d'accostage (la mire) **affiché lors du retour à la base** : vrai
    /// quand le vaisseau a quitté la base puis a **recroisé sa limite
    /// extérieure en entrant** (voir `docking::update_docking_guide`) - jamais
    /// pendant qu'il quitte l'accostage ni à quai. Faux dès qu'il accoste
    /// (`docking::docking`) ou quitte la base (`docking::release_links`).
    pub docking_guide: bool,
    /// Le vaisseau était **hors de la limite extérieure** de la base à la
    /// frame précédente : détection du franchissement **en entrant** (retour)
    /// par `docking::update_docking_guide` (front montant de la distance).
    pub dock_was_outside: bool,
    /// Situation d'accostage courante (`DockHint`) : l'état précédent permet
    /// à `docking::docking` de n'envoyer les messages d'aide au pilote qu'au
    /// changement de situation (front montant).
    pub dock_hint: DockHint,
    /// Prochain **bip de proximité** de l'accostage (heure `get_time()`, s) :
    /// les messages clignotants au-dessus du vaisseau sont accompagnés de
    /// bips d'autant plus rapprochés que le vaisseau est près du centre de la
    /// station (voir `docking::update_dock_approach`). Mis à jour à chaque
    /// frame tant que le guide d'accostage est actif.
    pub dock_approach_beep_at: f64,
    /// Son « accostage réussi » déjà émis pour l'animation d'accostage en
    /// cours : le bip distinct est joué une seule fois au moment où le
    /// vaisseau est **capturé** (front montant de `dock_anim`) - remis à faux
    /// quand l'animation se termine (aucun son après l'accostage).
    pub dock_approach_ok_sounded: bool,
    /// Vaisseau détruit, le joueur contrôle le **cosmonaute EVA éjecté** : son
    /// seul objectif est de rejoindre la base (zone d'accostage au centre) où
    /// il est secouru et le vaisseau reconstruit (voir `eva::rescue_cosmonaut`).
    /// Pendant ce temps la caméra, la mire et le HUD suivent le cosmonaute.
    pub cosmonaut_active: bool,
    /// Index de la forme « cosmonaute EVA » dans `shapes` (-1 tant qu'elle
    /// n'est pas créée par `main.rs`).
    pub eva_cosmonaut: i32,
    /// Récupération du cosmonaute EVA en cours (secondes restantes, 0 =
    /// aucune) : vaisseau détruit, il a rejoint la zone d'accostage - un
    /// cordon jaillit de l'anneau jusqu'à lui et le ramène sur l'anneau
    /// pendant `EVA_RECOVERY_DURATION` (le monde continue de tourner, voir
    /// `eva::advance_eva_recovery` et `render::draw_eva_recovery_cable`).
    pub eva_recovery: f64,
    /// Position de **départ de la traction** du cordon de récupération :
    /// posée au déclenchement puis mise à jour pendant le déploiement (le
    /// cosmonaute continue sur son élan) - interpolée vers
    /// `eva_recovery_to_pos`, le point de l'anneau où le cordon le ramène,
    /// une fois le cordon tendu.
    pub eva_recovery_from_pos: Point,
    /// Point de l'anneau (rayon `STATION_INNER_RADIUS`) où le cordon ramène
    /// le cosmonaute - dans sa direction au début de la traction.
    pub eva_recovery_to_pos: Point,
    /// Fondu enchaîné de la récupération en cours (secondes restantes, 0 =
    /// aucune) : le cosmonaute sur l'anneau s'efface pendant que le vaisseau
    /// reconstruit apparaît au centre de la station, liens attachés, pendant
    /// `EVA_CROSSFADE_DURATION` (voir `eva::advance_eva_crossfade`).
    pub eva_crossfade: f64,
    /// Boîte de choix DOCK STATION ouverte (accostage) - ex la boucle
    /// bloquante de `windowUtils_choiceBox` : tant qu'elle est ouverte, le
    /// jeu est gelé et seuls les clics sur UNLOAD / SHOP / CLOSE sont traités
    /// (UNLOAD garde la boîte ouverte ; le carburant et les munitions
    /// s'achètent au magasin, bouton SHOP).
    pub dock_box: bool,
    /// Magasin de la station ouvert (bouton SHOP de la boîte DOCK STATION) :
    /// choix des modes de déplacement (sélection gratuite ou déblocage contre
    /// minerais) et, en scénario à économie, achats d'extensions (réservoir,
    /// chargeur, soute) - le jeu est gelé tant qu'il est affiché ; CLOSE
    /// revient à la boîte DOCK STATION (toujours accosté).
    pub shop_box: bool,
    /// Fenêtre d'aide ouverte (touche S, ex `help` de windowUtils) : le jeu
    /// est gelé tant qu'elle est affichée (bouton CLOSE).
    pub help_box: bool,
    /// Écran de paramétrage ouvert (touche O) : options audio et graphiques.
    /// Le jeu est gelé tant qu'il est affiché. (Le mode de déplacement se
    /// choisit désormais au magasin de la station - bouton SHOP de la boîte
    /// DOCK STATION.)
    pub settings_box: bool,
    /// État de pause avant l'ouverture de l'écran de paramétrage : ouvrir les
    /// options (touche O, ou entrée RÉGLAGES du panneau COMMANDES) met le jeu
    /// en pause, et cet état est restauré à la fermeture (le jeu reprend où
    /// il en était - pause déjà active ou jeu en cours).
    pub settings_pause_prev: bool,
    /// Code PIN de la télécommande HTTP (vide = aucune protection - n'importe
    /// qui sur le réseau local peut piloter le vaisseau). Saisi dans l'écran
    /// de paramétrage (ligne REMOTE PIN) et persisté (clé `remote_pin`) -
    /// exigé par le `POST /cmd` du serveur (`remote.rs`).
    pub remote_pin: String,
    /// Saisie du PIN de la télécommande en cours dans l'écran de paramétrage :
    /// les chiffres du clavier remplissent `settings_pin_buffer` (4 max),
    /// ENTRÉE valide, ÉCHAP annule.
    pub settings_pin_edit: bool,
    /// Pin en cours de saisie dans l'écran de paramétrage (chiffres tapés,
    /// avant validation par ENTRÉE).
    pub settings_pin_buffer: String,
    /// Affiche les données de debug des formes (touche D, ex `showData%`).
    pub show_data: bool,
    /// Affiche les informations de debug (touche I, ex `showInfo%`).
    pub show_info: bool,
    /// Dernier keycode pressé (affiché par le mode I, ex `keycode = inp(96)`).
    pub last_keycode: i32,
    /// État de la touche F à la frame précédente (front montant de
    /// `is_key_down` - voir `input::f_pressed`) : détecte la pression même
    /// quand macroquad l'a avalée comme « répétition » (relâchement perdu
    /// pendant la bascule plein écran).
    pub f_was_down: bool,
    // messages (ex sendMessage/drawMessage, voir mainLoop.bas)
    pub message_delay: f64,
    pub message: String,
    pub message_queue: String,
    pub message1: String,
    pub message2: String,
    /// Suivi des objectifs DAG du scénario custom en cours (vide pour les
    /// scénarios built-in sans objectifs) - voir `objective_tracker.rs`.
    pub objective_tracker: crate::objective_tracker::ObjectiveTracker,
    /// Position initiale X du vaisseau (scénarios custom,appliquée après
    /// `create_player_vaisseau`).
    pub initial_ship_x: f64,
    /// Position initiale Y du vaisseau.
    pub initial_ship_y: f64,
    /// Orientation initiale du vaisseau en degrés (0 = droite).
    pub initial_ship_orientation: f64,
    /// Vitesse initiale du vaisseau (0 = immobile).
    pub initial_ship_velocity: f64,
    /// Crédits **gagnés cumulés** depuis le départ de la partie (crédits
    /// déchargés à la station + récompenses d'objectifs DAG) - indépendant des
    /// dépenses : le score composite (`scenario::composite_score`) s'appuie
    /// sur ce total, pas sur le solde courant (`resources.credits`).
    pub credits_earned: i32,
    /// Record (high-score) du scénario courant, restauré depuis le fichier de
    /// config (clé `highscore_<index>`) par `load_progression` - affiché à
    /// l'écran titre dans la ligne `[ SAVE : … ]`. Mis à jour quand le score
    /// courant le dépasse (voir `scenario::maybe_update_high_score`).
    pub high_score: i32,/// Annonce « NEW RECORD » déjà émise pour la session courante (voir
/// `scenario::maybe_update_high_score`) : l'annonce n'est envoyée qu'une
/// fois, au premier dépassement d'un record enregistré non nul - sans ce
/// drapeau, chaque point gagné ensuite repasserait pour un nouveau record.
/// Remis à faux par `apply_start` (nouvelle partie) et réarmé par
/// `load_progression` quand un record enregistré est restauré.
pub score_record_announced: bool,
    /// Temps de partie écoulé (secondes, hors pause) - moteur de la
    /// **difficulté adaptative** (`difficulty.rs`) : à chaque palier
    /// (`DIFFICULTY_RAMP_SECONDS`), la vitesse, la densité et la population
    /// des météores augmentent progressivement.
    pub session_time: f64,
    /// Statistiques de session (récapitulatif affiché à l'écran GAME OVER).
    pub session_stats: SessionStats,
    /// Journal de bord : les `EVENT_LOG_LEN` derniers événements (tirs,
    /// minerais, accostages, achats…), consultables via la touche L.
    pub event_log: Vec<String>,
    /// Journal de bord affiché (touche L) : panneau au-dessus du monde.
    pub log_box: bool,
    /// Panneau COMMANDES affiché (bouton COMMANDES du HUD, interface
    /// tactile) : liste les commandes (touches) activables au moment de
    /// l'ouverture - un clic sur une entrée exécute la commande, ESC ou
    /// l'entrée FERMER referme le panneau.
    pub commands_box: bool,
    /// Briefing pré-partie affiché (scénarios custom avec objectifs) :
    /// résumé des objectifs DAG, des contraintes et un conseil avant le
    /// lancement - fermé par ENTRÉE / ÉCHAP / clic.
    pub briefing_box: bool,
    /// Défilement du briefing (px, borné par `hud::briefing_scroll_max`) :
    /// l'ascenseur vertical apparaît quand le contenu dépasse la zone
    /// visible du panneau (molette, flèches haut/bas, PgPréc/PgSuiv,
    /// ou saisie/déplacement du curseur à la souris).
    pub briefing_scroll: f32,
    /// Saisie du curseur de l'ascenseur en cours : `Some(anchor)` = le bouton
    /// gauche est maintenu sur la piste, `anchor` étant la position verticale
    /// du point de préhension dans le curseur (0 = haut, `thumb_h` = bas).
    /// Le déplacement de la souris fait bouger le défilement (`hud.rs`).
    pub briefing_drag_anchor: Option<f32>,
    /// Consommables fabriqués à la station (onglet FABRICATION) : index
    /// `CRAFT_*` - bouclier temporaire, boost de vitesse, mines. Utilisés en
    /// vol (touches 1/2/3).
    pub consumables: [i32; CRAFT_COUNT],
    /// Bouclier temporaire actif (points d'impacts absorbés, consommable
    /// SHIELD) : absorbe les impacts comme le bouclier Survival, dans tous
    /// les scénarios, jusqu'à épuisement.
    pub temp_shield: f64,
    /// Boost de vitesse actif (secondes restantes, consommable BOOST) : la
    /// poussée est multipliée par `BOOST_FACTOR` tant qu'il est positif.
    pub boost_timer: f64,
    /// Décompte avant l'apparition du prochain **météore spécial** (boss).
    pub boss_timer: f64,
    /// Décompte avant la pose du prochain **portail** (warp gate).
    pub warp_timer: f64,
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
            docking_count: 0,
            bullets_fired: 0,
            bullets_lost: 0,
            moving_mode: MOVING_MODE_DIRECTIONAL,
            scenario: ScenarioId::FreePlay,
            radar_kind: RadarKind::Minimap,
            radar_echoes: Vec::new(),
            resources: Resources::default(),
            unlocked_modes: [true; MOVING_MODE_COUNT as usize],
            supplies_shortage_cost: 0,
            shop_fuel_qty: 0.0,
            shop_ammo_qty: [0.0; WEAPON_SLOTS],
            shop_drag: None,
            shop_tab: crate::config::SHOP_TAB_SUPPLIES,
            shop_feedback: String::new(),
            shop_feedback_ok: true,
            paused: false,
            game_over: false,
            invulnerable: 0.0,
            view_mode: ViewMode::Windowed,
            auto_generate: true,
            render_style: RenderStyle::Textured,
            window_size: 0,
            antialias: false,
            antialias_applied: false,
            touch_ui: true, // interface tactile affichée par défaut
            save_position: false, // option SAVE POSITION éteinte par défaut
            stars_big: false, // étoiles du fond en 1×1 par défaut
            max_meteor_shapes: INITIAL_MAX_METEOR_SHAPES,
            dock_anim: 0.0,
            dock_anim_from_pos: Point::new(0.0, 0.0),
            dock_anim_from_orient: 0.0,
            dock_retract: 0.0,
            dock_links: true, // le vaisseau démarre à quai, liens attachés
            docking_guide: false, // pas encore revenu à la base
            dock_was_outside: false,
            dock_hint: DockHint::Docked, // à quai au lancement : pas d'aide
            dock_approach_beep_at: 0.0, // premier bip dès l'entrée dans la base
            dock_approach_ok_sounded: false,
            cosmonaut_active: false,
            eva_cosmonaut: -1, // créé par main.rs au démarrage
            eva_recovery: 0.0,
            eva_recovery_from_pos: Point::new(0.0, 0.0),
            eva_recovery_to_pos: Point::new(0.0, 0.0),
            eva_crossfade: 0.0,
            dock_box: false,
            shop_box: false,
            help_box: false,
            settings_box: false,
            settings_pause_prev: false,
            remote_pin: String::new(),
            settings_pin_edit: false,
            settings_pin_buffer: String::new(),
            show_data: false,
            show_info: false,
            last_keycode: 0,
            f_was_down: false,
            message_delay: 0.0,
            message: String::new(),
            message_queue: String::new(),
            message1: String::new(),
            message2: String::new(),
            objective_tracker: crate::objective_tracker::ObjectiveTracker::default(),
            initial_ship_x: 0.0,
            initial_ship_y: 0.0,
            initial_ship_orientation: 0.0,
            initial_ship_velocity: 0.0,
            credits_earned: 0,
            high_score: 0,
            score_record_announced: false,
            session_time: 0.0,
            session_stats: SessionStats::default(),
            event_log: Vec::new(),
            log_box: false,
            commands_box: false,
            briefing_box: false,
            briefing_scroll: 0.0,
            briefing_drag_anchor: None,
            consumables: [0; CRAFT_COUNT],
            temp_shield: 0.0,
            boost_timer: 0.0,
            boss_timer: BOSS_SPAWN_INTERVAL,
            warp_timer: WARP_GATE_SPAWN_INTERVAL,
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

    /// Ajoute un événement au **journal de bord** (les `EVENT_LOG_LEN`
    /// derniers, consultables via la touche L) - le plus récent en tête.
    pub fn log_event(&mut self, message: &str) {
        self.event_log.insert(0, message.to_string());
        self.event_log.truncate(crate::config::EVENT_LOG_LEN);
    }

    /// Remet à zéro les statistiques de session et le journal de bord
    /// (nouvelle partie - `apply_start`).
    pub fn reset_session(&mut self) {
        self.session_time = 0.0;
        self.session_stats = SessionStats::default();
        self.event_log.clear();
        self.consumables = [0; CRAFT_COUNT];
        self.temp_shield = 0.0;
        self.boost_timer = 0.0;
        self.boss_timer = crate::config::BOSS_SPAWN_INTERVAL;
        self.warp_timer = crate::config::WARP_GATE_SPAWN_INTERVAL;
    }
}

impl Default for GameState {
    fn default() -> Self {
        GameState::new()
    }
}
