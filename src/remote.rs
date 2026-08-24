//! Télécommande HTTP : le jeu héberge un petit serveur web local - un
//! téléphone (ou tout appareil du réseau local) ouvre la page servie par le
//! jeu et **pilote le vaisseau** (D-pad de boutons ▲▼◀▶ + bouton de tir, les
//! mêmes commandes que les flèches/Shift) ; la page affiche aussi l'état du
//! jeu en direct.
//!
//! - `GET /`      - page de contrôle (HTML/JS embarqué, sans fichier externe)
//! - `POST /cmd/` - commandes `{"up":0|1,"down":..,"left":..,"right":..,"fire":..}`
//! - `GET /state` - état du jeu en JSON (FPS, carburant, munitions, crédits,
//!   réputation, vies/bouclier, pause…)
//!
//! Le serveur tourne dans un thread dédié (`start`, appelé au lancement par
//! `main.rs`), sans authentification - réseau local uniquement. Les commandes
//! reçues sont lues par la boucle de jeu à chaque frame (`up()/down()/…`,
//! combinées au clavier et au tactile dans `game.rs`) ; l'état y est publié à
//! chaque frame (`publish_state`). La section critique (`STATE`) n'est jamais
//! tenue pendant une requête réseau : le verrou est court des deux côtés.
//!
use crate::state::GameState;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use macroquad::prelude::info;
#[cfg(not(target_arch = "wasm32"))]
use tiny_http::{Header, Method, Response, Server};

/// Port d'écoute du serveur de contrôle (le jeu écoute sur toutes les
/// interfaces : joignable depuis le réseau local).
#[cfg(not(target_arch = "wasm32"))]
pub const REMOTE_PORT: u16 = 8642;

/// État partagé entre le thread du serveur (requêtes `/cmd` et `/state`) et
/// la boucle de jeu (lecture des commandes, publication de l'état du jeu).
#[derive(Debug, Clone)]
#[cfg(not(target_arch = "wasm32"))]
pub struct RemoteState {
    /// Commandes reçues du téléphone (joystick + tir).
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub fire: bool,
    // ── snapshot de l'état du jeu, publié à chaque frame ──
    pub fps: i32,
    pub paused: bool,
    pub game_over: bool,
    /// Vaisseau à quai (liens d'accostage attachés).
    pub docked: bool,
    /// Scénario à économie actif (carburant/munitions/crédits affichés).
    pub economy: bool,
    /// Scénario Survival actif (vies/bouclier affichés).
    pub survival: bool,
    pub fuel: f64,
    pub fuel_cap: f64,
    pub ammo: i32,
    pub ammo_cap: i32,
    pub credits: i32,
    pub reputation: i32,
    /// Rang courant du scénario à économie (ex CADET → PILOT → ACE).
    pub rank: Option<&'static str>,
    pub lives: i32,
    pub shield: f64,
    /// Compteur de météores détruits (jeu libre - le « score »).
    pub score: i32,
    /// Code PIN exigé par le `POST /cmd` (vide = aucune protection) : copié
    /// depuis `state.remote_pin` à chaque `publish_state` - la page de
    /// contrôle affiche une saisie de PIN et l'envoie avec chaque commande.
    pub pin: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl RemoteState {
    /// État initial - `const` : sert de valeur au niveau `static STATE` (std ne
    /// permet que des expressions constantes dans les statiques).
    const fn new() -> Self {
        RemoteState {
            up: false,
            down: false,
            left: false,
            right: false,
            fire: false,
            fps: 0,
            paused: false,
            game_over: false,
            docked: false,
            economy: false,
            survival: false,
            fuel: 0.0,
            fuel_cap: 0.0,
            ammo: 0,
            ammo_cap: 0,
            credits: 0,
            reputation: 0,
            rank: None,
            lives: 0,
            shield: 0.0,
            score: 0,
            pin: String::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for RemoteState {
    fn default() -> Self {
        RemoteState::new()
    }
}

/// État partagé (std `Mutex` const - valable en `static`).
#[cfg(not(target_arch = "wasm32"))]
static STATE: Mutex<RemoteState> = Mutex::new(RemoteState::new());

/// URL de la page de contrôle (remplie par `start`) - affichée par l'écran de
/// paramétrage (`render.rs`) pour que le joueur la retrouve à tout moment.
#[cfg(not(target_arch = "wasm32"))]
static URL: Mutex<Option<String>> = Mutex::new(None);

/// URL de la page de contrôle (ex `http://192.168.1.42:8642/`), `None` si le
/// serveur n'a pas démarré - toujours `None` sur wasm (télécommande
/// désactivée : pas de réseau dans le bac à sable navigateur).
pub fn url() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        URL.lock().unwrap().clone()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// Commande « poussée avant » reçue du téléphone (`false` sur wasm).
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

/// Commande « poussée arrière / frein » reçue du téléphone (`false` sur wasm).
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

/// Commande « rotation gauche » reçue du téléphone (`false` sur wasm).
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

/// Commande « rotation droite » reçue du téléphone (`false` sur wasm).
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

/// Commande « tir » reçue du téléphone (`false` sur wasm).
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

/// Démarre le serveur de contrôle dans un thread dédié et renvoie l'URL à
/// ouvrir sur le téléphone (ex `http://192.168.1.42:8642/`). Le serveur
/// écoute sur toutes les interfaces (`0.0.0.0`) - joignable depuis le réseau
/// local ou un hotspot créé sur le PC.
///
/// En cas d'échec (port occupé…), renvoie l'erreur - le jeu continue sans
/// télécommande. Sur wasm : toujours une erreur (télécommande désactivée).
pub fn start() -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let server = Server::http(format!("0.0.0.0:{}", REMOTE_PORT)).map_err(|e| e.to_string())?;
        let ip = lan_ip().unwrap_or_else(|| "localhost".to_string());
        info!("Remote control listening on {ip}");
        let url = format!("http://{}:{}/", ip, REMOTE_PORT);
        *URL.lock().unwrap() = Some(url.clone());
        std::thread::spawn(move || serve(server));
        Ok(url)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("télécommande indisponible sur wasm".to_string())
    }
}

/// Boucle du thread serveur : traite chaque requête HTTP (page, commandes,
/// état). Le serveur vit aussi longtemps que le processus - aucune
/// fermeture propre nécessaire à la sortie du jeu.
#[cfg(not(target_arch = "wasm32"))]
fn serve(server: Server) {
    for mut request in server.incoming_requests() {
        match (request.method(), request.url()) {
            (&Method::Get, "/") => respond(request, "text/html; charset=utf-8", PAGE),
            (&Method::Get, "/state") => {
                let body = state_json();
                respond(request, "application/json", &body);
            }
            (&Method::Post, "/cmd") => {
                // corps borné (4 Ko suffisent largement pour 5 booléens +
                // un PIN) : une requête démesurée est rejetée sans lecture
                // complète (`Read::take` exige une taille concrète - la
                // lecture est bornée à la main)
                let mut body = Vec::new();
                let mut buf = [0u8; 512];
                loop {
                    let n = request.as_reader().read(&mut buf).unwrap_or(0);
                    if n == 0 || body.len() >= MAX_CMD_BODY as usize {
                        break;
                    }
                    body.extend_from_slice(&buf[..n]);
                }
                let body = String::from_utf8_lossy(&body);
                if apply_cmd(&body) {
                    respond(request, "text/plain", "ok");
                } else {
                    // PIN manquant / incorrect ou corps illisible : refus
                    let header =
                        Header::from_bytes(b"Content-Type", b"text/plain").unwrap();
                    let _ = request.respond(
                        Response::from_string("forbidden".to_string())
                            .with_status_code(401)
                            .with_header(header),
                    );
                }
            }
            _ => {
                // favicon.ico et tout le reste : 404
                let _ = request.respond(
                    Response::from_string("not found".to_string())
                        .with_status_code(404),
                );
            }
        }
    }
}

/// Répond à une requête avec un corps et un type de contenu (200).
#[cfg(not(target_arch = "wasm32"))]
fn respond(request: tiny_http::Request, content_type: &str, body: &str) {
    let header = Header::from_bytes(b"Content-Type", content_type.as_bytes()).unwrap();
    let _ = request.respond(Response::from_string(body.to_string()).with_header(header));
}

/// Taille maximale (octets) du corps d'un `POST /cmd` : 5 booléens + un PIN
/// de 4 chiffres = quelques dizaines d'octets - 4 Ko laissent une marge
/// confortable tout en rejetant les corps démesurés.
#[cfg(not(target_arch = "wasm32"))]
const MAX_CMD_BODY: u64 = 4096;

/// Applique des commandes reçues (`POST /cmd`, JSON) à un état - un corps
/// illisible est ignoré (l'état reste inchangé). Pur (testable sans global).
///
/// Renvoie `false` si la commande est refusée : PIN incorrect (la
/// télécommande est protégée par `s.pin`) ou corps illisible.
#[cfg(not(target_arch = "wasm32"))]
fn apply_cmd_to(s: &mut RemoteState, body: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    // PIN : le serveur exige `pin` dans le corps quand un code est configuré
    // (vide = aucune protection)
    let expected = &s.pin;
    let given = v.get("pin").and_then(|x| x.as_str()).unwrap_or("");
    if !expected.is_empty() && given != expected {
        return false;
    }
    let get = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    s.up = get("up");
    s.down = get("down");
    s.left = get("left");
    s.right = get("right");
    s.fire = get("fire");
    true
}

/// Applique des commandes reçues à l'état partagé (`POST /cmd`). Renvoie
/// `false` si la commande est refusée (PIN incorrect ou corps illisible).
#[cfg(not(target_arch = "wasm32"))]
fn apply_cmd(body: &str) -> bool {
    apply_cmd_to(&mut STATE.lock().unwrap(), body)
}

/// Snapshot de l'état du jeu à publier (champs du HUD + ressources). Pur
/// (testable sans global). NB : les commandes (`up/down/…`) ne sont **pas**
/// touchées - seul le `POST /cmd` les modifie.
#[cfg(not(target_arch = "wasm32"))]
fn snapshot(state: &GameState) -> RemoteState {
    let economy = crate::scenario::has_economy(state);
    let mut s = RemoteState::new();
    s.fps = state.fps;
    s.paused = state.paused;
    s.game_over = state.game_over;
    s.docked = state.dock_links;
    s.economy = economy;
    s.survival = crate::scenario::has_survival(state);
    s.fuel = state.resources.fuel;
    s.fuel_cap = crate::scenario::fuel_capacity(state);
    s.ammo = crate::scenario::total_ammo(state);
    s.ammo_cap = crate::scenario::total_ammo_capacity(state);
    s.credits = state.resources.credits;
    s.reputation = if economy {
        state.resources.reputation as i32
    } else {
        state.meteors_destroyed
    };
    s.rank = crate::scenario::current_rank(state);
    s.lives = state.resources.lives;
    s.shield = state.resources.shield;
    s.score = state.meteors_destroyed;
    s.pin = state.remote_pin.clone();
    s
}

/// Publie l'état du jeu courant dans l'état partagé (`GET /state` le lira).
/// Appelé à chaque frame par `game::update`. Les commandes reçues du
/// téléphone (`up/down/left/right/fire`) sont préservées : seuls les champs
/// HUD et ressources sont mis à jour, pour que les boutons du téléphone
/// restent « enfoncés » entre deux envois `POST /cmd`. Sans effet sur wasm.
pub fn publish_state(state: &GameState) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut guard = STATE.lock().unwrap();
        // conserver les commandes reçues du téléphone
        let (up, down, left, right, fire) =
            (guard.up, guard.down, guard.left, guard.right, guard.fire);
        *guard = snapshot(state);
        guard.up = up;
        guard.down = down;
        guard.left = left;
        guard.right = right;
        guard.fire = fire;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = state;
    }
}

/// Sérialise un snapshot en JSON (`GET /state`) pour la page de contrôle.
#[cfg(not(target_arch = "wasm32"))]
fn state_json_from(s: &RemoteState) -> String {
    serde_json::json!({
        "fps": s.fps,
        "paused": s.paused,
        "game_over": s.game_over,
        "docked": s.docked,
        "economy": s.economy,
        "survival": s.survival,
        "fuel": s.fuel,
        "fuel_cap": s.fuel_cap,
        "ammo": s.ammo,
        "ammo_cap": s.ammo_cap,
        "credits": s.credits,
        "reputation": s.reputation,
        "rank": s.rank,
        "lives": s.lives,
        "shield": s.shield,
        "score": s.score,
        "pin_required": !s.pin.is_empty(),
    })
    .to_string()
}

/// Sérialise l'état partagé en JSON (`GET /state`).
#[cfg(not(target_arch = "wasm32"))]
fn state_json() -> String {
    state_json_from(&STATE.lock().unwrap())
}

/// Adresse IP locale (celle de la route par défaut) : astuce std - connecter
/// une socket UDP ne **jamais** envoyée (bind local uniquement) renseigne
/// l'adresse locale choisie pour joindre 8.8.8.8. `None` si indisponible.
#[cfg(not(target_arch = "wasm32"))]
fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip().to_string())
}



/// Page de contrôle servie sur `GET /` : **bouton de tir bas-gauche** +
/// **D-pad** à 4 boutons directionnels (▲▼◀▶, bas-droite - mêmes commandes
/// que les flèches), et panneau d'état en direct (rafraîchi toutes les
/// 250 ms).
/// Événements Pointer (unifie doigt et souris, multi-touch : chaque bouton
/// capture son propre doigt), commandes envoyées par `POST /cmd` au plus
/// toutes les 33 ms quand un état a changé.
#[cfg(not(target_arch = "wasm32"))]
const PAGE: &str = r##"<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>Meteors Mining - Remote</title>
<style>
  html, body { margin: 0; height: 100%; overflow: hidden; touch-action: none;
    user-select: none; -webkit-user-select: none; background: #0a0e1a;
    color: #eee; font-family: monospace; }
  #pad { position: fixed; right: 16vw; bottom: 13vh; width: 178px; height: 178px; }
  .dir { position: absolute; width: 62px; height: 62px; border-radius: 50%;
    border: 2px solid rgba(255,255,255,.35); background: rgba(255,255,255,.10);
    color: rgba(255,255,255,.85); font-size: 24px; font-family: monospace;
    display: flex; align-items: center; justify-content: center; }
  .dir.pressed { background: rgba(255,255,255,.5); border-color: #fff; color: #fff; }
  #up { left: 58px; top: 0; }
  #left { left: 0; top: 58px; }
  #right { left: 116px; top: 58px; }
  #down { left: 58px; top: 116px; }
  #fire { position: fixed; left: 16vw; bottom: 12vh; width: 110px; height: 110px;
    margin: -55px -55px 0 0; border-radius: 50%; border: 3px solid rgba(255,130,130,.7);
    background: rgba(255,60,60,.22); color: #fff; font-weight: bold; font-size: 16px;
    font-family: monospace; display: flex; align-items: center; justify-content: center; }
  #fire.pressed { background: rgba(255,60,60,.6); border-color: #fff; }
  #hud { position: fixed; top: 10px; left: 10px; right: 10px; font-size: 13px;
    line-height: 1.4; white-space: pre-wrap; pointer-events: none; text-shadow: 0 0 4px #000; }
  #pinbox { position: fixed; inset: 0; background: rgba(0,0,0,.75);
    display: flex; align-items: center; justify-content: center; }
  #pinbox.hidden { display: none; }
  .pin-card { background: #141b2e; border: 2px solid rgba(255,255,255,.35);
    border-radius: 8px; padding: 18px 22px; text-align: center; font-family: monospace; }
  .pin-card h2 { margin: 0 0 10px; font-size: 15px; letter-spacing: 1px; }
  .pin-card input { width: 110px; padding: 8px; font-size: 22px; text-align: center;
    background: #0a0e1a; color: #fff; border: 1px solid rgba(255,255,255,.4);
    border-radius: 4px; letter-spacing: 6px; }
  .pin-card button { margin-top: 10px; padding: 8px 22px; font-size: 15px;
    background: rgba(90,200,120,.85); color: #062; border: none; border-radius: 4px;
    font-weight: bold; cursor: pointer; font-family: monospace; }
  .pin-card .hint { margin-top: 10px; font-size: 11px; color: rgba(255,255,255,.55); }
  .pin-card .bad { color: #ff8080; font-size: 11px; min-height: 14px; }
</style>
</head>
<body>
<div id="pad">
  <div id="up" class="dir">▲</div>
  <div id="left" class="dir">◀</div>
  <div id="right" class="dir">▶</div>
  <div id="down" class="dir">▼</div>
</div>
<div id="fire">FIRE</div>
<div id="hud"></div>
<div id="pinbox" class="hidden">
  <div class="pin-card">
    <h2>REMOTE PIN</h2>
    <input id="pin" type="password" inputmode="numeric" maxlength="4" placeholder="••••" autocomplete="off">
    <div><button id="pinok">OK</button></div>
    <div class="bad" id="pinbad"></div>
    <div class="hint">Code à choisir dans le jeu - écran O, ligne REMOTE PIN</div>
  </div>
</div>
<script>
"use strict";
const fire = document.getElementById('fire');
const hud = document.getElementById('hud');
const pinbox = document.getElementById('pinbox');
const pinInput = document.getElementById('pin');
const pinBad = document.getElementById('pinbad');
const cmd = { up:false, down:false, left:false, right:false, fire:false };
let dirty = false;
// PIN mémorisé dans le navigateur (localStorage) : envoyé avec chaque
// commande - une protection vide = aucune exigence
let pin = localStorage.getItem('mm_pin') || '';

// saisie du PIN : affichée quand le jeu l'exige, le code soumis est retenu
// pour les commandes suivantes (et mémorisé pour la prochaine visite)
document.getElementById('pinok').addEventListener('click', () => {
  pin = pinInput.value.trim();
  if (pin.length > 0 && !/^[0-9]{1,4}$/.test(pin)) {
    pinBad.textContent = '4 chiffres maximum';
    return;
  }
  pinBad.textContent = '';
  localStorage.setItem('mm_pin', pin);
  pinbox.classList.add('hidden');
  dirty = true;
});

// un bouton directionnel : pressé → commande active (maintenue tant que le
// doigt reste dessus, capture du pointeur = pas de perte en glissant)
function bind(btn, key) {
  btn.addEventListener('pointerdown', e => {
    btn.setPointerCapture(e.pointerId);
    btn.classList.add('pressed');
    cmd[key] = true;
    dirty = true;
  });
  const up = () => { btn.classList.remove('pressed'); cmd[key] = false; dirty = true; };
  btn.addEventListener('pointerup', up);
  btn.addEventListener('pointercancel', up);
}
bind(document.getElementById('up'), 'up');
bind(document.getElementById('down'), 'down');
bind(document.getElementById('left'), 'left');
bind(document.getElementById('right'), 'right');

fire.addEventListener('pointerdown', e => {
  fire.setPointerCapture(e.pointerId);
  fire.classList.add('pressed');
  cmd.fire = true;
  dirty = true;
});
const fireUp = () => { fire.classList.remove('pressed'); cmd.fire = false; dirty = true; };
fire.addEventListener('pointerup', fireUp);
fire.addEventListener('pointercancel', fireUp);

// envoi des commandes : au plus toutes les 33 ms, seulement si un état a changé
setInterval(() => {
  if (!dirty) return;
  dirty = false;
  const body = JSON.stringify(cmd);
  fetch('/cmd', { method: 'POST', headers: { 'Content-Type': 'application/json' },
                  // le PIN est joint à chaque commande (le serveur le refuse
                  // sinon) - rejet 401 = reprompt sur la page
                  body: pin ? JSON.stringify({ ...cmd, pin }) : body })
    .then(r => { if (r.status === 401) { pinbox.classList.remove('hidden'); pinBad.textContent = 'PIN refusé'; } })
    .catch(() => {});
}, 33);

// état du jeu en direct (HUD du téléphone), toutes les 250 ms
setInterval(async () => {
  try {
    const s = await (await fetch('/state')).json();
    // le jeu exige un PIN : la saisie est affichée (elle reste invisible
    // tant que la télécommande n'est pas protégée)
    if (s.pin_required) {
      pinbox.classList.remove('hidden');
    } else {
      pinbox.classList.add('hidden');
      pinBad.textContent = '';
    }
    const lines = [];
    if (s.economy) {
      lines.push('FUEL ' + s.fuel.toFixed(0) + '/' + s.fuel_cap +
                 '   AMMO ' + s.ammo + '/' + s.ammo_cap +
                 '   CR ' + s.credits);
      lines.push('REPUTATION ' + s.reputation + (s.rank ? ' (' + s.rank + ')' : ''));
    } else if (s.survival) {
      lines.push('LIVES ' + s.lives + '   SHIELD ' + s.shield.toFixed(0));
    } else {
      lines.push('METEORS ' + s.score);
    }
    lines.push('FPS ' + s.fps +
               (s.paused ? '   [PAUSED]' : '') +
               (s.game_over ? '   [GAME OVER]' : '') +
               (s.docked ? '   [DOCKED]' : ''));
    hud.textContent = lines.join('\n');
  } catch (e) { /* jeu éteint ou page pas encore servie : on réessaie */ }
}, 250);
</script>
</body>
</html>
"##;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn cmd_updates_remote_inputs() {
        let mut s = RemoteState::new();
        assert!(apply_cmd_to(
            &mut s,
            r#"{"up":true,"down":false,"left":true,"right":false,"fire":true}"#
        ));
        assert!(s.up && s.left && s.fire);
        assert!(!s.down && !s.right);
    }

    #[test]
    fn malformed_cmd_is_refused() {
        let mut s = RemoteState::new();
        assert!(!apply_cmd_to(&mut s, "pas du json"));
        assert!(!s.up && !s.down && !s.left && !s.right && !s.fire);
    }

    #[test]
    fn cmd_with_pin_is_checked() {
        let mut s = RemoteState::new();
        s.pin = "1234".to_string();
        // sans PIN dans le corps : refusé
        assert!(!apply_cmd_to(
            &mut s,
            r#"{"up":true,"down":false,"left":false,"right":false,"fire":false}"#
        ));
        assert!(!s.up && !s.down);
        // mauvais PIN : refusé
        assert!(!apply_cmd_to(
            &mut s,
            r#"{"up":true,"down":false,"left":false,"right":false,"fire":false,"pin":"9999"}"#
        ));
        assert!(!s.up && !s.down);
        // bon PIN : accepté
        assert!(apply_cmd_to(
            &mut s,
            r#"{"up":true,"down":false,"left":false,"right":false,"fire":false,"pin":"1234"}"#
        ));
        assert!(s.up);
        assert!(!s.down);
    }

    #[test]
    fn snapshot_exposes_the_hud_values() {
        let state = GameState::new(); // jeu libre : ni économie ni survie
        let s = snapshot(&state);
        assert!(!s.economy && !s.survival);
        assert_eq!(s.score, state.meteors_destroyed);
        assert_eq!(s.fps, state.fps);
        assert_eq!(s.docked, state.dock_links);
    }

    #[test]
    fn state_json_serializes() {
        let parsed: serde_json::Value = serde_json::from_str(&state_json_from(&RemoteState::new())).unwrap();
        assert!(parsed.get("fps").is_some());
        assert!(parsed.get("fuel").is_some());
        assert!(parsed.get("rank").is_some());
        assert_eq!(parsed.get("pin_required"), Some(&serde_json::json!(false)));
    }

    /// Bout en bout (sans fenêtre, juste un socket local) : le serveur sert
    /// la page de contrôle, accepte une commande (`POST /cmd`) et expose
    /// l'état (`GET /state`). La commande envoyée (`down`) n'est touchée par
    /// aucun autre test : le partage du `STATE` global entre tests parallèles
    /// n'introduit pas de course.
    #[test]
    fn server_serves_page_accepts_commands_and_exposes_state() {
        use std::io::{Read, Write};
        use std::time::{Duration, Instant};

        let url = start().expect("le serveur doit démarrer");
        let host_port = url.trim_end_matches('/').trim_start_matches("http://").to_string();
        // (en-têtes HTTP/1.1 + corps optionnel - `Connection: close` pour que
        // le test n'ait pas à gérer le keep-alive)
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

        // la page de contrôle est servie
        let page = conn("GET / HTTP/1.1", "");
        assert!(page.contains("200 OK"), "{page}");
        assert!(page.contains("Meteors Mining"));
        assert!(page.contains("FIRE"));

        // une commande est acceptée et lue par la boucle de jeu
        let body = r##"{"up":false,"down":true,"left":false,"right":false,"fire":false}"##;
        let ok = conn(
            &format!(
                "POST /cmd HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}",
                body.len()
            ),
            body,
        );
        assert!(ok.contains("200 OK"), "{ok}");
        // la commande arrive dans le thread serveur : attendre qu'elle soit
        // appliquée (timeout 2 s)
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if STATE.lock().unwrap().down {
                break;
            }
            assert!(Instant::now() < deadline, "commande non appliquée à temps");
            std::thread::sleep(Duration::from_millis(10));
        }

        // l'état est exposé en JSON
        let state = conn("GET /state HTTP/1.1", "");
        assert!(state.contains("200 OK"), "{state}");
        assert!(state.contains("\"fps\""), "{state}");
    }

    #[test]
    fn publish_state_preserves_commands() {
        // simuler une commande reçue du téléphone
        {
            let mut s = STATE.lock().unwrap();
            s.up = true;
            s.fire = true;
        }
        // publish_state doit conserver up et fire
        crate::remote::publish_state(&crate::state::GameState::new());
        let s = STATE.lock().unwrap();
        assert!(s.up, "up doit être préservé après publish_state");
        assert!(s.fire, "fire doit être préservé après publish_state");
        assert!(!s.down, "down doit rester à false");
    }
}
