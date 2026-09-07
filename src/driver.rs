//! Interface de contrôle pour l'**auto-entraînement du pilote** : un système
//! extérieur (indépendant de l'application - un entraîneur RL, un script
//! Python…) pilote le cosmonaute en utilisant le jeu comme environnement.
//! Ce module est la **partie jeu** de cette interface : il expose
//!
//! - une **observation** complète de la situation (cinématique du pilote -
//!   vaisseau **ou** cosmonaute EVA -, station, ressources, objets proches),
//!   publiée à chaque frame par la boucle de jeu (`publish_state`) et servie
//!   par `GET /obs` ;
//! - un **canal d'actions** identique aux touches du joueur
//!   (↑/↓/←/→ + tir), lu par `input::player_controls` quand le pilote externe
//!   est **engagé** (`engaged`) : le système entraîné pilote le vaisseau
//!   quand il est intact et le cosmonaute EVA quand le vaisseau est détruit -
//!   comme le pilote automatique (`POST /cmd`) ;
//! - la bascule du **pilote automatique** de référence (`autopilot`), pour
//!   mesurer la progression de l'entraînement contre la ligne de base ;
//! - une **remise à zéro d'épisode** (`POST /reset`) : monde régénéré
//!   déterministe (graine), départ vaisseau à quai ou cosmonaute EVA éjecté
//!   à une position donnée - consommée par la boucle de jeu (`main.rs`).
//!
//! Architecture calquée sur `remote.rs` : serveur HTTP natif (hors wasm)
//! dans un thread dédié, état partagé (`Mutex`, section critique courte),
//! fonctions pures testables sans macroquad. Le port (`DRIVER_PORT` 8643)
//! est distinct de la télécommande (`REMOTE_PORT` 8642).
//!
//! # Protocole (localhost, JSON)
//!
//! - `GET /obs`      - dernière observation publiée (une par frame de jeu)
//! - `GET /obs`      - dernière observation publiée (une par frame de jeu)
//! - `POST /cmd`     - `{"up":bool,"down":..,"left":..,"right":..,"fire":..,
//!   "autopilot":bool?,"driver":bool?}` - actions de la frame + bascules du
//!   pilote automatique / pilote externe
//! - `POST /reset`   - `{"seed":u64,"target":"ship"|"eva","x":..,"y":..}`
//!   - nouvelle partie déterministe (consommée par le jeu)

use crate::config::{
    PLAYER_INDEX, STATION_INDEX, WHOIAM_ALIEN, WHOIAM_METEOR, WHOIAM_MINE, WHOIAM_MINERAL,
    WHOIAM_WARP_GATE,
};
use crate::geom::{wrapped_delta, Point, Triangle};
use crate::shape::Shape;
use crate::state::GameState;
use serde::Serialize;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use tiny_http::{Header, Method, Response, Server};

/// Port d'écoute de l'interface de contrôle (auto-entraînement) - le serveur
/// écoute sur loopback (l'interface sert un système d'entraînement local).
#[cfg(not(target_arch = "wasm32"))]
pub const DRIVER_PORT: u16 = 8643;

/// Cible d'un épisode d'auto-entraînement (`POST /reset`) : l'entité que le
/// pilote entraîné contrôle au départ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetTarget {
    /// Le vaisseau démarre **à quai** au centre de la station (partie neuve).
    Ship,
    /// Le vaisseau est détruit à `(x, y)` : le pilote entraîné contrôle le
    /// **cosmonaute EVA** éjecté (objectif : rejoindre la base).
    Eva,
}

/// Demande de remise à zéro d'un épisode (posée par `POST /reset`, consommée
/// par la boucle de jeu - voir `main.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EpisodeReset {
    /// Graine du monde (PRNG ChaCha12) : même graine → même partie.
    pub seed: u64,
    /// Entité pilotée au départ de l'épisode.
    pub target: ResetTarget,
    /// Position du crash (mode EVA, monde torique). Sans effet en mode
    /// vaisseau (départ à quai au centre de la station).
    pub x: f64,
    pub y: f64,
    /// Monde qui se peuple (météores générés au fil de l'épisode) ou monde
    /// figé (seul le contenu initial de la graine) - éteint par défaut pour
    /// un épisode déterministe ; à allumer pour entraîner la boucle complète
    /// de minage (les météores n'existent qu'après génération).
    pub auto_generate: bool,
}

/// Cinématique d'une entité pilotable (vaisseau ou cosmonaute EVA) :
/// position monde, vitesse (vectorielle **écran** en unités/s - y vers le
/// bas, convention du portage), orientation/direction/vitesse angulaire
/// brutes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Kinematic {
    pub x: f64,
    pub y: f64,
    /// Vecteur vitesse écran (unités/s) : `cos(direction)·velocity·60`.
    pub vx: f64,
    /// Vecteur vitesse écran (unités/s, y vers le bas) : `-sin·velocity·60`.
    pub vy: f64,
    /// Norme de la vitesse (unités/s).
    pub speed: f64,
    /// Direction du déplacement (radians, convention écran).
    pub direction: f64,
    /// Orientation du nez (radians).
    pub orientation: f64,
    /// Vitesse angulaire (radians/s, mode REALISTIC).
    pub rotation: f64,
}

/// Un objet proche du pilote (météore, alien, minerai…) avec sa position et
/// sa vitesse **relatives** au pilote (deltas toriques), pour un entraîneur
/// qui n'aurait pas à gérer le repliement du monde.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NearbyObject {
    /// Libellé du type (`meteore`, `alien`, `minerai`, `mine`, `portail`).
    pub kind: String,
    /// Position relative au pilote (dx, dy - delta torique le plus court).
    pub dx: f64,
    pub dy: f64,
    /// Distance au pilote (unités monde).
    pub dist: f64,
    /// Vitesse relative (unités/s, deltas toriques des vitesses écran).
    pub vx: f64,
    pub vy: f64,
    /// Rayon de la forme.
    pub radius: f64,
    /// Vie restante (triangles vivants pour un météore).
    pub life: i32,
}

/// Observation complète publiée à chaque frame - le « coup d'œil » que le
/// système d'auto-entraînement reçoit pour choisir ses actions. Sérialisée
/// en JSON (`GET /obs`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Observation {
    /// Compteur de frames du serveur d'interface (incrémenté à chaque
    /// `publish_state`) : permet à l'entraîneur de détecter un nouveau pas.
    pub frame: u64,
    /// Temps de partie (s).
    pub t: f64,
    /// Entité **contrôlée** : `"vaisseau"` ou `"eva"` (le pilote entraîné
    /// suit la même règle que le joueur / le pilote automatique).
    pub pilot: String,
    /// Cinématique du vaisseau (toujours présent ; contrôlé hors mode EVA).
    pub ship: Kinematic,
    /// Cinématique du cosmonaute EVA (présent ; contrôlé en mode EVA).
    pub eva: Kinematic,
    /// `true` quand le pilote contrôlé est le cosmonaute EVA.
    pub eva_active: bool,
    /// Position de la station (au centre du monde) et distance du pilote
    /// vers elle (delta torique).
    pub station_x: f64,
    pub station_y: f64,
    pub station_radius: f64,
    /// Delta torique pilote → station.
    pub station_dx: f64,
    pub station_dy: f64,
    pub station_dist: f64,
    /// Vaisseau à quai (liens d'accostage attachés, ou animation
    /// accostage/rétraction en cours).
    pub docked: bool,
    pub dock_anim: f64,
    pub dock_retract: f64,
    pub dock_box: bool,
    /// Récupération du cosmonaute EVA en cours / fondu enchaîné du secours.
    pub eva_recovery: f64,
    pub eva_crossfade: f64,
    pub paused: bool,
    pub game_over: bool,
    pub autopilot: bool,
    pub driver_engaged: bool,
    /// Scénario à économie (carburant/munitions/crédits) - sinon jeu libre.
    pub economy: bool,
    pub fuel: f64,
    pub fuel_cap: f64,
    pub ammo: i32,
    pub ammo_cap: i32,
    pub credits: i32,
    /// Soute (emplacements occupés / capacité) - le remplissage pilote la
    /// mission « rentrer décharger » de la boucle complète.
    pub cargo_qty: i32,
    pub cargo_cap: i32,
    /// Compteurs utiles aux récompenses de l'entraîneur.
    pub meteors_destroyed: i32,
    pub score: i32,
    /// Objets proches du pilote (≤ `MAX_NEARBY_OBJECTS`, les plus proches).
    pub nearby: Vec<NearbyObject>,
}

/// Nombre maximal d'objets rapportés dans une observation (les plus proches
/// du pilote) : garde le JSON léger tout en couvrant l'horizon de jeu utile.
pub const MAX_NEARBY_OBJECTS: usize = 32;

/// Libellé stable du type d'une forme pour l'observation.
fn kind_label(who: i32) -> String {
    match who {
        WHOIAM_METEOR => "meteore".to_string(),
        WHOIAM_MINERAL => "minerai".to_string(),
        WHOIAM_ALIEN => "alien".to_string(),
        WHOIAM_WARP_GATE => "portail".to_string(),
        WHOIAM_MINE => "mine".to_string(),
        _ => format!("forme-{who}"),
    }
}

/// Cinématique écran (unités/s) d'une forme : la vitesse est stockée par
/// frame dans le modèle (×60 par seconde) et la direction suit l'axe écran
/// (y vers le bas - `-sin` pour la composante verticale, voir `thrust_vector`).
fn kinematic(s: &Shape) -> Kinematic {
    Kinematic {
        x: s.position.x,
        y: s.position.y,
        vx: s.direction.cos() * s.velocity * 60.0,
        vy: -s.direction.sin() * s.velocity * 60.0,
        speed: s.velocity * 60.0,
        direction: s.direction,
        orientation: s.orientation,
        rotation: s.rotation,
    }
}

/// Construit l'observation de la frame à partir de l'état du jeu et des
/// formes. **Pure** (aucune macroquad) - testable sans fenêtre comme
/// `autopilot_inputs`. Tolérante aux vecteurs courts (défense) : les champs
/// manquants valent zéro / vide plutôt qu'un `panic`.
#[allow(clippy::too_many_lines)]
pub fn observe(state: &GameState, shapes: &[Shape]) -> Observation {
    // entité contrôlée : le vaisseau normalement, le cosmonaute EVA quand le
    // vaisseau est détruit (`cosmonaut_active`) - même règle que le joueur et
    // le pilote automatique (`input::pilot_index`)
    let eva_active = state.cosmonaut_active;
    let ship = shapes.get(PLAYER_INDEX).map(kinematic).unwrap_or_default();
    let eva_idx = if state.eva_cosmonaut >= 0 { state.eva_cosmonaut as usize } else { shapes.len() };
    let eva = shapes.get(eva_idx).map(kinematic).unwrap_or_default();
    // référence du pilote pour les distances / objets proches (position seule)
    let pilot = if eva_active {
        shapes
            .get(eva_idx)
            .map(|s| s.position)
            .unwrap_or_else(|| shapes.first().map(|s| s.position).unwrap_or_default())
    } else {
        shapes.first().map(|s| s.position).unwrap_or_default()
    };
    // référence de la station pour distance / deltas toriques
    let station = shapes.get(STATION_INDEX);
    let (station_x, station_y, station_radius) = station
        .map(|s| (s.position.x, s.position.y, s.radius))
        .unwrap_or((0.0, 0.0, 0.0));
    let delta = wrapped_delta(pilot, Point::new(station_x, station_y), &state.world);

    // objets proches (météores, minerais, aliens, mines, portails) - triés
    // par distance croissante au pilote, plafonnés à `MAX_NEARBY_OBJECTS`
    let mut nearby: Vec<NearbyObject> = shapes
        .iter()
        .filter(|s| {
            s.life > 0
                && (s.who_i_am == WHOIAM_METEOR
                    || s.who_i_am == WHOIAM_MINERAL
                    || s.who_i_am == WHOIAM_ALIEN
                    || s.who_i_am == WHOIAM_WARP_GATE
                    || s.who_i_am == WHOIAM_MINE)
        })
        .map(|s| {
            let d = wrapped_delta(pilot, s.position, &state.world);
            NearbyObject {
                kind: kind_label(s.who_i_am),
                dx: d.x,
                dy: d.y,
                dist: d.x.hypot(d.y),
                vx: s.direction.cos() * s.velocity * 60.0,
                vy: -s.direction.sin() * s.velocity * 60.0,
                radius: s.radius,
                life: s.life,
            }
        })
        .collect();
    nearby.sort_by(|a, b| a.dist.total_cmp(&b.dist));
    nearby.truncate(MAX_NEARBY_OBJECTS);

    let economy = crate::scenario::has_economy(state);
    let docked = state.dock_links || state.dock_anim > 0.0 || state.dock_retract > 0.0;
    Observation {
        frame: 0, // posé par `publish_state` (le serveur compte ses frames)
        t: state.session_time,
        pilot: if eva_active { "eva".to_string() } else { "vaisseau".to_string() },
        ship,
        eva,
        eva_active,
        station_x,
        station_y,
        station_radius,
        station_dx: delta.x,
        station_dy: delta.y,
        station_dist: delta.x.hypot(delta.y),
        docked,
        dock_anim: state.dock_anim,
        dock_retract: state.dock_retract,
        dock_box: state.dock_box,
        eva_recovery: state.eva_recovery,
        eva_crossfade: state.eva_crossfade,
        paused: state.paused,
        game_over: state.game_over,
        autopilot: state.autopilot,
        driver_engaged: false, // posé par `publish_state` (état partagé)
        economy,
        fuel: crate::scenario::fuel_capacity(state).min(state.resources.fuel),
        fuel_cap: crate::scenario::fuel_capacity(state),
        ammo: crate::scenario::total_ammo(state),
        ammo_cap: crate::scenario::total_ammo_capacity(state),
        credits: state.resources.credits,
        cargo_qty: state.player.cargo_qty,
        cargo_cap: crate::scenario::cargo_capacity(state),
        meteors_destroyed: state.meteors_destroyed,
        score: state.meteors_destroyed,
        nearby,
    }
}

// ─── état partagé + serveur HTTP (natif uniquement - hors wasm) ─────────────

/// État partagé entre le thread du serveur (requêtes `/cmd`, `/reset`,
/// `/obs`) et la boucle de jeu (application des actions, publication de
/// l'observation à chaque frame).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct Shared {
    /// Actions de la frame courante (pilotage externe, mêmes primitives que
    /// les touches).
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub fire: bool,
    /// Pilote externe **engagé** : quand `true`, `input::player_controls`
    /// consomme ces actions à la place du clavier / pilote automatique.
    pub engaged: bool,
    /// Demande de bascule du pilote automatique de référence (consommée par
    /// la boucle de jeu, qui possède `state.autopilot`).
    pub autopilot_req: Option<bool>,
    /// Dernière observation publiée par la boucle de jeu.
    pub obs: Option<Observation>,
    /// Prochaine remise à zéro d'épisode demandée (consommée par `main.rs`).
    pub reset_req: Option<EpisodeReset>,
    /// Serveur démarré : publication active (`publish_state`).
    pub started: bool,
    /// Compteur de frames du serveur (incrémenté à chaque publication).
    frame: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Shared {
    /// État initial - `const` : sert de valeur au niveau `static STATE`.
    const fn new() -> Self {
        Shared {
            up: false,
            down: false,
            left: false,
            right: false,
            fire: false,
            engaged: false,
            autopilot_req: None,
            obs: None,
            reset_req: None,
            started: false,
            frame: 0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Shared {
    fn default() -> Self {
        Shared::new()
    }
}

/// État partagé (std `Mutex` const - valable en `static`).
#[cfg(not(target_arch = "wasm32"))]
static STATE: Mutex<Shared> = Mutex::new(Shared::new());

/// Démarre le serveur de contrôle sur `DRIVER_PORT` dans un thread dédié et
/// renvoie l'URL (localhost - l'interface sert le système d'auto-entraînement
/// local). En cas d'échec (port occupé…), renvoie l'erreur - le jeu continue
/// sans interface d'entraînement. Sur wasm : toujours une erreur.
pub fn start() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        start_on(DRIVER_PORT)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("interface d'auto-entrainement indisponible sur wasm".to_string())
    }
}

/// Démarre l'interface sur un port donné (0 = port éphémère choisi par le
/// système - utilisé par les tests pour ne pas entrer en conflit avec une
/// instance du jeu ouverte sur `DRIVER_PORT`).
#[cfg(not(target_arch = "wasm32"))]
fn start_on(port: u16) -> Result<String, String> {
    let server = Server::http(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
    let bound = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        #[cfg(unix)]
        tiny_http::ListenAddr::Unix(_) => port,
    };
    let url = format!("http://127.0.0.1:{bound}/");
    STATE.lock().unwrap().started = true;
    std::thread::spawn(move || serve(server));
    Ok(url)
}

/// Boucle du thread serveur : traite chaque requête HTTP (observation,
/// commandes, remise à zéro). Le serveur vit aussi longtemps que le
/// processus - aucune fermeture propre nécessaire à la sortie du jeu.
#[cfg(not(target_arch = "wasm32"))]
fn serve(server: tiny_http::Server) {
    for mut request in server.incoming_requests() {
        match (request.method(), request.url()) {
            (&Method::Get, "/") => respond(
                request,
                "text/plain; charset=utf-8",
                "Meteors Mining - interface d'auto-entrainement du pilote.\n\
                 GET /obs    observation de la frame courante (JSON)\n\
                 POST /cmd   actions {up,down,left,right,fire} + bascules driver/autopilot\n\
                 POST /reset episode {seed,target:\"ship\"|\"eva\",x,y}\n",
            ),
            (&Method::Get, "/obs") => {
                let body = STATE
                    .lock()
                    .unwrap()
                    .obs
                    .as_ref()
                    .map(|o| serde_json::to_string(o).unwrap_or_else(|_| "{}".to_string()))
                    .unwrap_or_else(|| "{}".to_string());
                respond(request, "application/json", &body);
            }
            (&Method::Post, "/cmd") => {
                let mut body = Vec::new();
                read_body(&mut request, &mut body);
                let body = String::from_utf8_lossy(&body);
                if apply_cmd(&body) {
                    respond(request, "text/plain", "ok");
                } else {
                    respond_status(request, 400, "bad json");
                }
            }
            (&Method::Post, "/reset") => {
                let mut body = Vec::new();
                read_body(&mut request, &mut body);
                let body = String::from_utf8_lossy(&body);
                if apply_reset(&body) {
                    respond(request, "text/plain", "ok");
                } else {
                    respond_status(request, 400, "bad json");
                }
            }
            _ => {
                // favicon.ico et tout le reste : 404
                let _ = request
                    .respond(Response::from_string("not found".to_string()).with_status_code(404));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn respond(request: tiny_http::Request, content_type: &str, body: &str) {
    let header = Header::from_bytes(b"Content-Type", content_type.as_bytes()).unwrap();
    let _ = request.respond(Response::from_string(body.to_string()).with_header(header));
}

#[cfg(not(target_arch = "wasm32"))]
fn respond_status(request: tiny_http::Request, code: u16, body: &str) {
    let header = Header::from_bytes(b"Content-Type", b"text/plain").unwrap();
    let _ = request
        .respond(Response::from_string(body.to_string()).with_status_code(code).with_header(header));
}

/// Taille maximale (octets) du corps d'un `POST /cmd` ou `/reset`. Les actions
/// et bascules représentent quelques dizaines d'octets - 4 Ko laissent une
/// marge confortable tout en rejetant les corps démesurés.
#[cfg(not(target_arch = "wasm32"))]
const MAX_CMD_BODY: usize = 4096;

#[cfg(not(target_arch = "wasm32"))]
fn read_body(request: &mut tiny_http::Request, body: &mut Vec<u8>) {
    let mut buf = [0u8; 512];
    loop {
        let n = request.as_reader().read(&mut buf).unwrap_or(0);
        if n == 0 || body.len() >= MAX_CMD_BODY {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
}

/// Pilote externe **engagé** ? (`false` sur wasm - interface inactive.)
pub fn engaged() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().engaged
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn up() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().up
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn down() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().down
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn left() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().left
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn right() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().right
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

pub fn fire() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().fire
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// Applique les demandes de bascule du pilote automatique posées par
/// `POST /cmd` (la boucle de jeu possède `state.autopilot` - seul elle peut
/// le modifier).
pub fn sync_autopilot(state: &mut GameState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut g = STATE.lock().unwrap();
        if let Some(req) = g.autopilot_req.take() {
            state.autopilot = req;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = state;
    }
}

/// Publie l'observation de la frame courante (appelé par la boucle de jeu à
/// chaque frame - voir `main.rs`). Sans effet si l'interface n'est pas
/// démarrée (ni sur wasm). `frame` du serveur incrémenté à chaque
/// publication : le système d'entraînement détecte ainsi les nouveaux pas.
pub fn publish_state(state: &GameState, shapes: &[Shape]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut g = STATE.lock().unwrap();
        if !g.started {
            return;
        }
        let mut obs = observe(state, shapes);
        g.frame += 1;
        obs.frame = g.frame;
        obs.driver_engaged = g.engaged;
        g.obs = Some(obs);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (state, shapes);
    }
}

/// Prend la prochaine remise à zéro d'épisode demandée (`POST /reset`), si
/// une est en attente - consommée par la boucle de jeu (`main.rs`).
pub fn take_reset() -> Option<EpisodeReset> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        STATE.lock().unwrap().reset_req.take()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// Applique un corps `POST /cmd` à l'état partagé. Renvoie `false` si le
/// corps n'est pas du JSON.
#[cfg(not(target_arch = "wasm32"))]
fn apply_cmd(body: &str) -> bool {
    apply_cmd_to(&mut STATE.lock().unwrap(), body)
}

/// Applique un corps `POST /cmd` à un état (pur - testable) : actions de la
/// frame, engagement du pilote externe (`driver`) et demande de bascule du
/// pilote automatique (`autopilot`). Un corps illisible est ignoré (l'état
/// reste inchangé). Le pilote externe et le pilote automatique sont
/// mutuellement exclusifs : demander l'un relâche l'autre.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply_cmd_to(s: &mut Shared, body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let get = |k: &str, cur: bool| v.get(k).and_then(|x| x.as_bool()).unwrap_or(cur);
    s.up = get("up", s.up);
    s.down = get("down", s.down);
    s.left = get("left", s.left);
    s.right = get("right", s.right);
    s.fire = get("fire", s.fire);
    if let Some(d) = v.get("driver").and_then(|x| x.as_bool()) {
        s.engaged = d;
        if d {
            s.autopilot_req = Some(false); // driver prend la main
        }
    }
    if let Some(a) = v.get("autopilot").and_then(|x| x.as_bool()) {
        s.autopilot_req = Some(a);
        if a {
            s.engaged = false; // autopilot reprend la main
        }
    }
    true
}

/// Applique un corps `POST /reset` à l'état partagé. Renvoie `false` si le
/// corps n'est pas du JSON.
#[cfg(not(target_arch = "wasm32"))]
fn apply_reset(body: &str) -> bool {
    apply_reset_to(&mut STATE.lock().unwrap(), body)
}

/// Applique un corps `POST /reset` à un état (pur - testable) : pose la
/// demande de remise à zéro d'épisode. Corps illisible → `false`.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply_reset_to(s: &mut Shared, body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let seed = v.get("seed").and_then(|x| x.as_u64()).unwrap_or(0);
    let target = match v.get("target").and_then(|x| x.as_str()) {
        Some("eva") => ResetTarget::Eva,
        _ => ResetTarget::Ship,
    };
    let x = v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let y = v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let auto_generate = v.get("auto_generate").and_then(|x| x.as_bool()).unwrap_or(false);
    s.reset_req = Some(EpisodeReset { seed, target, x, y, auto_generate });
    true
}

// ─── remise à zéro d'un épisode (consommée par `main.rs`) ───────────────────
// Régénère un monde **déterministe** (graine) puis installe la situation de
// départ demandée : vaisseau à quai (mode Ship) ou vaisseau détruit avec le
// cosmonaute EVA éjecté à `(x, y)` (mode Eva). Le scénario repart en jeu
// libre (règles de départ, progression non chargée) : chaque épisode
// s'entraîne sur la même base, sans dépendre de la sauvegarde du joueur.

/// Remise à zéro complète d'un épisode : monde neuf (même graine → mêmes
/// formes), vaisseau reconstruit à quai, puis mise en place de la cible.
/// Appelée par la boucle de jeu (`main.rs`) quand `POST /reset` a posé une
/// demande. Les éléments et les étoiles sont réinitialisés par
/// `generate::prepare` (comme au lancement) - même graine → même monde.
#[allow(clippy::too_many_arguments)]
pub fn reset_episode(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    garbages: &mut Vec<crate::garbage::Garbage>,
    elements: &mut Vec<crate::state::Element>,
    stars: &mut Vec<Point>,
    rng: &mut rand_chacha::ChaCha12Rng,
    req: EpisodeReset,
) {
    use ::rand::SeedableRng;
    use crate::scenario::{apply_start, start_mode, ScenarioId};
    // graine déterministe : même seed → même monde (génération + formes)
    *rng = rand_chacha::ChaCha12Rng::seed_from_u64(req.seed);
    // monde neuf : formes/triangles/débris/étoiles vidés
    shapes.clear();
    triangles.clear();
    garbages.clear();
    elements.clear();
    stars.clear();
    // scénario de référence pour l'entraînement : jeu libre (aucune économie)
    // - règles de départ réappliquées, progression non chargée
    state.scenario = ScenarioId::FreePlay;
    apply_start(state);
    state.resources = crate::scenario::Resources::default();
    state.player.cargo_size = crate::scenario::cargo_capacity(state);
    state.max_meteor_shapes = crate::marketplace::INITIAL_MAX_METEOR_SHAPES;
    // mode de déplacement de référence (DIRECTIONAL - le défaut du jeu libre)
    state.moving_mode = start_mode(ScenarioId::FreePlay);
    state.unlocked_modes = [true; crate::config::MOVING_MODE_COUNT as usize];
    // monde stable pour l'épisode : la génération automatique est éteinte -
    // le contenu du monde ne dépend que de la graine (déterministe), pas du
    // rythme des frames (l'entraîneur décide s'il veut un monde qui se peuple)
    state.auto_generate = false;
    // monde régénéré : vaisseau + station + étoiles + éléments
    crate::generate::prepare(state, shapes, triangles, stars, elements, rng);
    // cosmonaute EVA recréé (sa forme a été vidée avec le monde) et garé
    state.eva_cosmonaut = crate::cosmonaut::create_eva_cosmonaut(shapes, triangles) as i32;
    // vaisseau reconstruit à quai au centre de la station (coque + liens)
    crate::eva::respawn_player(state, shapes, triangles);
    // état d'épisode propre : pas de pause, de boîtes ni d'animations
    // résiduelles de la partie précédente
    state.paused = false;
    state.game_over = false;
    state.cosmonaut_active = false; // (le mode EVA ci-dessous le réactive)
    state.dock_box = false;
    state.shop_box = false;
    state.help_box = false;
    state.settings_box = false;
    state.log_box = false;
    state.commands_box = false;
    state.briefing_box = false;
    state.dock_anim = 0.0;
    state.dock_retract = 0.0;
    state.eva_recovery = 0.0;
    state.eva_crossfade = 0.0;
    state.eva_recovery_from_pos = Point::new(0.0, 0.0);
    state.eva_recovery_to_pos = Point::new(0.0, 0.0);
    state.cosmonaut_turn = 0;
    state.docking_guide = false;
    state.dock_was_outside = false;
    state.invulnerable = 0.0;
    state.radar_echoes.clear();
    state.station_scratches.clear();
    state.credits_earned = 0;
    state.high_score = 0;
    state.score_record_announced = false;
    state.message_delay = 0.0;
    state.message = String::new();
    state.message_queue = String::new();
    state.message1 = String::new();
    state.message2 = String::new();

    // cible : le cosmonaute EVA (vaisseau détruit à `(x, y)`)
    if req.target == ResetTarget::Eva {
        // position du crash dans le monde torique (normalisée)
        let world = &state.world;
        let x = req.x.rem_euclid(world.width);
        let y = req.y.rem_euclid(world.height);
        // le vaisseau « explose » à cet endroit : coque tuée (comme une
        // collision), puis le cosmonaute est éjecté sur place. L'épisode ne
        // démarre pas à quai : le pilote est le cosmonaute, en vol près du
        // crash (`activate_cosmonaut` l'y téléporte et allume la mire)
        let ship = &mut shapes[PLAYER_INDEX];
        ship.position = Point::new(x, y);
        ship.life = 0;
        for t in &mut triangles[ship.first_triangle..=ship.last_triangle] {
            t.life = 0;
        }
        state.dock_links = false;
        state.player_at_station = 0;
        crate::eva::activate_cosmonaut(state, shapes, triangles);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::shape::Shape;
    use crate::state::GameState;
    use ::rand::SeedableRng;
    use rand_chacha::ChaCha12Rng;

    /// Petite scène : vaisseau à `(px, py)` orienté à `o`, station au centre
    /// (le monde de `GameState::new` - torique, même repliement qu'en jeu).
    fn scene(px: f64, py: f64, o: f64) -> (GameState, Vec<Shape>) {
        let state = GameState::new();
        let shapes = vec![
            Shape {
                position: Point::new(px, py),
                orientation: o,
                direction: 0.0,
                velocity: 0.0,
                life: 1,
                who_i_am: crate::config::WHOIAM_PLAYER,
                is_collider: true,
                ..Shape::default()
            },
            Shape {
                position: Point::new(0.0, 0.0),
                radius: 162.0,
                life: 1,
                who_i_am: crate::config::WHOIAM_STATION,
                ..Shape::default()
            },
        ];
        (state, shapes)
    }

    #[test]
    fn observation_reports_the_ship_pilot_and_station() {
        let (state, shapes) = scene(300.0, 0.0, 0.0);
        let obs = observe(&state, &shapes);
        assert_eq!(obs.pilot, "vaisseau");
        assert!(!obs.eva_active);
        assert_eq!(obs.ship.x, 300.0);
        assert!((obs.station_dist - 300.0).abs() < 1e-6);
        assert!((obs.station_dx + 300.0).abs() < 1e-9, "delta pilote → station");
        assert!(!obs.economy);
        // vaisseau à quai au lancement (GameState::new)
        assert!(obs.docked);
    }

    #[test]
    fn observation_tracks_nearby_meteors_only_live() {
        let (state, mut shapes) = scene(0.0, 0.0, 0.0);
        shapes.push(Shape {
            position: Point::new(120.0, 0.0),
            radius: 20.0,
            life: 8,
            who_i_am: WHOIAM_METEOR,
            is_collider: true,
            ..Shape::default()
        });
        // météore mort : exclu de l'observation
        shapes.push(Shape {
            position: Point::new(-200.0, 0.0),
            radius: 20.0,
            life: 0,
            who_i_am: WHOIAM_METEOR,
            ..Shape::default()
        });
        let obs = observe(&state, &shapes);
        assert_eq!(obs.nearby.len(), 1);
        let m = &obs.nearby[0];
        assert_eq!(m.kind, "meteore");
        assert!((m.dx - 120.0).abs() < 1e-9);
        assert_eq!(m.life, 8);
    }

    #[test]
    fn observation_switches_to_the_eva_cosmonaut_when_active() {
        let (mut state, shapes) = scene(300.0, 0.0, 0.0);
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 2;
        let mut shapes = shapes;
        shapes.push(Shape {
            position: Point::new(200.0, 100.0),
            life: 91,
            who_i_am: crate::config::WHOIAM_COSMONAUT,
            ..Shape::default()
        });
        let obs = observe(&state, &shapes);
        assert_eq!(obs.pilot, "eva");
        assert!(obs.eva_active);
        assert!((obs.eva.x - 200.0).abs() < 1e-9);
        assert!((obs.station_dist - (200.0f64 * 200.0 + 100.0 * 100.0).sqrt()).abs() < 1e-6);
    }

    #[test]
    fn observation_is_stable_when_vectors_are_short() {
        // pas de panique avec des vecteurs vides (défense)
        let state = GameState::new();
        let obs = observe(&state, &[]);
        assert_eq!(obs.pilot, "vaisseau");
        assert!(obs.nearby.is_empty());
    }

    #[test]
    fn cmd_updates_actions_and_toggles() {
        let mut s = Shared::new();
        assert!(apply_cmd_to(
            &mut s,
            r#"{"up":true,"down":false,"left":true,"fire":true,"driver":true}"#
        ));
        assert!(s.up && s.left && s.fire);
        assert!(!s.down && !s.right);
        assert!(s.engaged, "driver:true engage le pilote externe");
        assert_eq!(s.autopilot_req, Some(false), "driver prend la main sur autopilot");

        // autopilot:true est demandé et désengage le pilote externe
        let mut s = Shared::new();
        assert!(apply_cmd_to(&mut s, r#"{"autopilot":true}"#));
        assert_eq!(s.autopilot_req, Some(true));
        assert!(!s.engaged, "autopilot et driver sont mutuellement exclusifs");

        // un corps illisible est refusé sans rien changer
        let mut s = Shared::new();
        s.up = true;
        assert!(!apply_cmd_to(&mut s, "pas du json"));
        assert!(s.up, "l'état reste inchangé après un refus");
    }

    #[test]
    fn reset_parses_seed_target_and_position() {
        let mut s = Shared::new();
        assert!(apply_reset_to(&mut s, r#"{"seed":42,"target":"eva","x":600.0,"y":200.0}"#));
        let req = s.reset_req.expect("la demande doit être posée");
        assert_eq!(req.seed, 42);
        assert_eq!(req.target, ResetTarget::Eva);
        assert_eq!((req.x, req.y), (600.0, 200.0));

        // défauts : vaisseau, graine 0, position 0, génération automatique éteinte
        let mut s = Shared::new();
        assert!(apply_reset_to(&mut s, r#"{}"#));
        let req = s.reset_req.expect("la demande doit être posée");
        assert_eq!(req.target, ResetTarget::Ship);
        assert_eq!(req.seed, 0);
        assert!(!req.auto_generate);
        // auto_generate:true est retenu
        let mut s = Shared::new();
        assert!(apply_reset_to(&mut s, r#"{"auto_generate":true}"#));
        assert!(s.reset_req.unwrap().auto_generate);
    }

    #[test]
    fn state_json_serializes_observation() {
        let (state, shapes) = scene(0.0, 0.0, 0.0);
        let obs = observe(&state, &shapes);
        let json = serde_json::to_string(&obs).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("pilot").and_then(|x| x.as_str()), Some("vaisseau"));
        assert!(parsed.get("station_dist").is_some());
        assert!(parsed.get("nearby").and_then(|x| x.as_array()).is_some());
    }

    /// Bout en bout (sans fenêtre, socket local) : le serveur sert
    /// l'observation et accepte une commande et une remise à zéro.
    #[test]
    fn server_serves_obs_and_accepts_cmd_and_reset() {
        use std::io::{Read, Write};
        use std::sync::Mutex;
        static SERIAL: Mutex<()> = Mutex::new(()); // état partagé global
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());

        let url = start_on(0).expect("le serveur doit démarrer");
        let host_port = url.trim_end_matches('/').trim_start_matches("http://").to_string();
        let conn = |req_head: &str, body: &str| -> String {
            let mut stream = std::net::TcpStream::connect(&host_port).expect("connexion");
            write!(
                stream,
                "{req_head}\r\nHost: {host_port}\r\nConnection: close\r\n\r\n{body}"
            )
            .unwrap();
            let mut resp = String::new();
            stream.read_to_string(&mut resp).unwrap();
            resp
        };

        // la racine est servie
        let page = conn("GET / HTTP/1.1", "");
        assert!(page.contains("200 OK"), "{page}");
        assert!(page.contains("auto-entrainement"));

        // POST /cmd accepté
        let body = r#"{"up":true,"driver":true}"#;
        let ok = conn(
            &format!(
                "POST /cmd HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}",
                body.len()
            ),
            body,
        );
        assert!(ok.contains("200 OK"), "{ok}");
        assert!(engaged(), "driver doit être engagé");

        // POST /reset accepté
        let body = r#"{"seed":7,"target":"eva"}"#;
        let ok = conn(
            &format!(
                "POST /reset HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}",
                body.len()
            ),
            body,
        );
        assert!(ok.contains("200 OK"), "{ok}");
        assert_eq!(take_reset().map(|r| (r.seed, r.target)), Some((7, ResetTarget::Eva)));

        // une observation publiée est servie en JSON
        let (state, shapes) = scene(0.0, 0.0, 0.0);
        publish_state(&state, &shapes);
        let obs = conn("GET /obs HTTP/1.1", "");
        assert!(obs.contains("200 OK"), "{obs}");
        assert!(obs.contains("\"pilot\":\"vaisseau\""), "{obs}");

        // remise à zéro de l'état partagé (les tests suivants s'exécutent
        // dans le même processus)
        STATE.lock().unwrap().up = false;
        STATE.lock().unwrap().engaged = false;
    }

    /// Le mode EVA de la remise à zéro éjecte le cosmonaute au crash.
    #[test]
    fn reset_episode_eva_ejects_the_cosmonaut() {
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut garbages = Vec::new();
        let mut elements = Vec::new();
        let mut stars = Vec::new();
        let mut rng = ChaCha12Rng::seed_from_u64(1);
        reset_episode(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            EpisodeReset { seed: 1, target: ResetTarget::Eva, x: 400.0, y: 250.0, auto_generate: false },
        );
        assert!(state.cosmonaut_active, "le cosmonaute doit être éjecté");
        assert_eq!(state.eva_cosmonaut, 2, "index de la forme EVA après régénération");
        assert_eq!(shapes[PLAYER_INDEX].life, 0, "vaisseau détruit au crash");
        let eva = &shapes[state.eva_cosmonaut as usize];
        assert_eq!(eva.who_i_am, crate::config::WHOIAM_COSMONAUT);
        assert!((eva.position.x - 400.0).abs() < 1e-6, "cosmonaute au crash");
        assert!((eva.position.y - 250.0).abs() < 1e-6, "cosmonaute au crash");
    }

    /// Le mode vaisseau démarre à quai, coque intacte, contrôle au vaisseau.
    #[test]
    fn reset_episode_ship_starts_docked() {
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut garbages = Vec::new();
        let mut elements = Vec::new();
        let mut stars = Vec::new();
        let mut rng = ChaCha12Rng::seed_from_u64(2);
        reset_episode(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            EpisodeReset { seed: 2, target: ResetTarget::Ship, x: 0.0, y: 0.0, auto_generate: false },
        );
        assert!(!state.cosmonaut_active, "le vaisseau est piloté");
        assert!(state.dock_links, "à quai (liens attachés)");
        assert!(shapes[PLAYER_INDEX].life > 0, "coque intacte");
        assert!(shapes[PLAYER_INDEX].position.x.abs() < 1e-6, "au centre de la station");
    }
}
