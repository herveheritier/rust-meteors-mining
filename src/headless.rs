//! Mode d'exécution **sans rendu** (Phase 2 de l'auto-entraînement) :
//! l'interface de contrôle de `driver.rs` est servie par une boucle de jeu
//! à **pas fixe**, sans fenêtre macroquad, sans rendu ni audio - la **même
//! physique** que la partie réelle (`game::update`). Depuis l'entraîneur
//! (indépendant), le protocole est **inchangé** (`GET /obs`, `POST /cmd`,
//! `POST /reset`) : seule la cadence change - plus de temps réel. Le pilote
//! externe avance d'un pas par commande, l'autopilote de référence file à
//! cadence bornée.
//!
//! macroquad initialise son contexte **dans sa fenêtre** : hors fenêtre, ses
//! fonctions d'entrée (`is_key_pressed`…), `get_time` et `get_fps`
//! **paniquent**. Ce module fournit donc les lectures qu'`update` fait
//! encore en cours de partie sous une forme **neutralisée** quand le mode
//! headless est actif (`active`) : clavier/souris éteints (le pilote externe
//! agit via `driver.rs`), horloge et FPS remplacés. Quand le mode est
//! inactif (jeu normal), les mêmes fonctions relaient exactement macroquad :
//! le comportement ne change pas.
//!
//! Le mode est activé au tout début du processus headless (`activate`),
//! avant la première frame - `main.rs` aiguille l'entrée native vers
//! `run` quand `--headless` est passé. Sur wasm (et dans le jeu normal),
//! le mode est toujours inactif.
#![allow(clippy::module_name_repetitions)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Mode headless actif ? (positionné par `activate`, au démarrage d'un
/// processus `--headless` - jamais dans le jeu normal ni sur wasm.)
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Origine de l'horloge du mode headless (premier appel à `now`) : remplace
/// `macroquad::get_time`, indisponible sans fenêtre.
static T0: OnceLock<Instant> = OnceLock::new();

/// Active le mode headless (au début de `run`). Sans effet si déjà actif.
pub fn activate() {
    ACTIVE.store(true, Ordering::SeqCst);
}

/// Mode headless actif ?
pub fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// Horloge de la simulation (secondes) : temps écoulé depuis le premier appel
/// en mode headless, sinon `macroquad::get_time` (jeu normal).
pub fn now() -> f64 {
    if active() {
        let t0 = *T0.get_or_init(Instant::now);
        t0.elapsed().as_secs_f64()
    } else {
        macroquad::time::get_time()
    }
}

/// FPS mesurés : `0` en mode headless (aucune fenêtre), sinon
/// `macroquad::get_fps`.
pub fn fps() -> i32 {
    if active() {
        0
    } else {
        macroquad::time::get_fps()
    }
}

/// Touche pressée (front) : jamais en mode headless (le pilote externe agit
/// via `driver.rs`), sinon `macroquad::is_key_pressed`.
pub fn key_pressed(key: macroquad::prelude::KeyCode) -> bool {
    if active() {
        false
    } else {
        macroquad::prelude::is_key_pressed(key)
    }
}

/// Première touche pressée de la frame (pour l'affichage du keycode) :
/// `None` en mode headless.
pub fn first_key_pressed() -> Option<macroquad::prelude::KeyCode> {
    if active() {
        None
    } else {
        macroquad::prelude::get_keys_pressed().iter().next().copied()
    }
}

// ─── boucle headless accélérée (natif uniquement - pas de fenêtre) ─────────

/// Port d'écoute par défaut de l'interface headless (le même que celui du jeu
/// normal - `driver.rs` le déclare sous `cfg(not(wasm))`, une constante locale
/// garde la valeur visible sur toutes les cibles).
const DEFAULT_DRIVER_PORT: u16 = 8643;

/// Options de la ligne de commande headless (`--headless`).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct HeadlessOptions {
    /// Port d'écoute de l'interface de contrôle (défaut `DRIVER_PORT`).
    pub port: u16,
    /// Cadence du pas fixe en images/seconde (défaut 60 - la cadence du jeu).
    pub fps: f64,
    /// Graine du premier épisode (monde initial avant le premier `POST /reset`).
    pub seed: u64,
    /// Cible du premier épisode (`"ship"` par défaut, `"eva"` possible).
    pub target_eva: bool,
    /// Monde qui se peuple dès le premier épisode (météores générés) ?
    pub auto_generate: bool,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        HeadlessOptions {
            port: DEFAULT_DRIVER_PORT,
            fps: 60.0,
            seed: 0,
            target_eva: false,
            auto_generate: false,
        }
    }
}

/// Parse la ligne de commande du processus : extrait les options `--headless`
/// (les autres arguments sont ignorés - le jeu normal ne lit pas ses options).
/// Les valeurs invalides gardent leur défaut.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn parse_args() -> HeadlessOptions {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = HeadlessOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--headless" => {}
            "--port" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                    opts.port = v;
                }
                i += 1;
            }
            "--fps" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
                    opts.fps = v.max(1.0);
                }
                i += 1;
            }
            "--seed" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    opts.seed = v;
                }
                i += 1;
            }
            "--target" => {
                if let Some(v) = args.get(i + 1) {
                    opts.target_eva = v == "eva";
                }
                i += 1;
            }
            "--auto-generate" => opts.auto_generate = true,
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Boucle headless : sert l'interface de contrôle (`driver.rs`) sur
/// `opts.port` et fait avancer la **vraie physique du jeu** (`game::update`)
/// à pas fixe (`1 / opts.fps`), sans fenêtre.
///
/// Cadence :
/// - une **remise à zéro** (`POST /reset`) fait toujours avancer d'un pas
///   (publication de l'observation de départ de l'épisode) ;
/// - quand le **pilote externe est engagé** (`POST /cmd {"driver":true}`),
///   chaque `POST /cmd` fait avancer d'un pas : la boucle est pilotée pas à
///   pas par l'entraîneur, qui peut donc aller bien plus vite que le temps
///   réel (une étape d'épisode par aller-retour HTTP) ;
/// - quand **l'autopilote du jeu pilote** (`POST /cmd {"autopilot":true}`,
///   sans pilote externe), la boucle **file** à cadence bornée
///   (`FREE_RUN_FPS`) - assez rapide pour accélérer la ligne de base, assez
///   lente pour que l'entraîneur échantillonne les fenêtres transitoires de
///   l'observation (récupération EVA, accostage) ;
/// - sinon elle attend (aucune décision en attente - pas de combustion de
///   CPU).
///
/// La configuration persistée du joueur n'est jamais lue, et la progression
/// n'est jamais écrite : les lectures/écritures `persist` sont isolées dans
/// un dossier jetable (`XDG_CONFIG_HOME` pointé sur un répertoire temporaire)
/// pour que l'entraînement ne touche pas à la sauvegarde réelle.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(opts: &HeadlessOptions) -> ! {
    use ::rand::SeedableRng;
    // configuration utilisateur isolée : l'entraînement ne lit ni n'écrit
    // jamais la sauvegarde réelle du joueur
    let tmp = std::env::temp_dir().join(format!("meteors_mining_headless_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    // edition 2024 : `std::env::set_var` est unsafe (processus mono-thread à
    // ce stade - le serveur HTTP n'est pas encore démarré)
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
    }
    activate();

    // interface de contrôle sur le port demandé
    match crate::driver::start_on(opts.port) {
        Ok(url) => println!("auto-entraînement headless : interface sur {url} (pas fixe {:.0} Hz)", opts.fps),
        Err(e) => {
            eprintln!("✗ impossible de démarrer l'interface sur le port {} : {e}", opts.port);
            eprintln!("  (le jeu normal ou une autre instance headless l'occupe ?)");
            std::process::exit(1);
        }
    }

    // monde + état du jeu, exactement comme la boucle réelle (`main.rs`) :
    // la remise à zéro d'épisode reconstruit tout (monde, vaisseau,
    // cosmonaute EVA, étoiles) à la graine
    let mut state = crate::state::GameState::new();
    let mut shapes = Vec::new();
    let mut triangles = Vec::new();
    let mut garbages = Vec::new();
    let mut elements = Vec::new();
    let mut stars = Vec::new();
    let mut rng = rand_chacha::ChaCha12Rng::seed_from_u64(opts.seed);
    let mut world_ready = false;

    let dt = 1.0 / opts.fps;
    // cadence du file libre (autopilote) en pas/seconde
    const FREE_RUN_FPS: f64 = 480.0;
    // dernière séquence de commande consommée (`driver.rs` l'incrémente à
    // chaque `POST /cmd` : un nouveau pas est demandé)
    let mut last_cmd_seq = 0u64;
    // cadence du **file libre** (autopilote sans pilote externe) : bornée
    // pour que l'observation reste lisible par l'entraîneur (une fenêtre
    // d'épisode transitoire - récupération EVA, accostage - dure assez en
    // temps réel pour être échantillonnée par un `GET /obs`). Le pilotage
    // **pas-à-pas** (`/cmd`, pilote externe) n'est, lui, pas borné : il va
    // à la vitesse des commandes de l'entraîneur.
    let free_dt = 1.0 / FREE_RUN_FPS;
    let mut last_free = std::time::Instant::now();
    loop {
        // bascules de l'autopilote demandées par `POST /cmd`, puis remise à
        // zéro d'épisode (`POST /reset`) - comme la boucle réelle
        crate::driver::sync_autopilot(&mut state);
        let mut step = false;
        if let Some(req) = crate::driver::take_reset() {
            crate::driver::reset_episode(
                &mut state,
                &mut shapes,
                &mut triangles,
                &mut garbages,
                &mut elements,
                &mut stars,
                &mut rng,
                req,
            );
            world_ready = true;
            step = true;
        }
        let seq = crate::driver::cmd_seq();
        if seq != last_cmd_seq {
            last_cmd_seq = seq;
            step = true;
        }
        // premier épisode avant tout `POST /reset` : un monde initial est
        // construit (graine/options de la ligne de commande) pour que
        // `GET /obs` serve une observation dès le démarrage
        if !world_ready {
            crate::driver::reset_episode(
                &mut state,
                &mut shapes,
                &mut triangles,
                &mut garbages,
                &mut elements,
                &mut stars,
                &mut rng,
                crate::driver::EpisodeReset {
                    seed: opts.seed,
                    target: if opts.target_eva {
                        crate::driver::ResetTarget::Eva
                    } else {
                        crate::driver::ResetTarget::Ship
                    },
                    x: 0.0,
                    y: 0.0,
                    auto_generate: opts.auto_generate,
                    scenario: crate::driver::EpisodeScenario::FreePlay,
                },
            );
            world_ready = true;
            step = true;
        }
        // autopilote sans pilote externe : la ligne de base **file** (aucune
        // commande attendue - elle pilote toute seule), à cadence bornée
        let free_run = state.autopilot && !crate::driver::engaged();
        if free_run {
            step = true;
        }
        if !step {
            // aucune décision en attente : on attend (le serveur HTTP tourne
            // dans son propre thread)
            std::thread::sleep(std::time::Duration::from_micros(200));
            continue;
        }
        // file libre : respecte la cadence bornée (`free_dt`) pour garder les
        // observations échantillonnables par l'entraîneur
        if free_run {
            let elapsed = last_free.elapsed().as_secs_f64();
            if elapsed < free_dt {
                std::thread::sleep(std::time::Duration::from_secs_f64(free_dt - elapsed));
            }
            last_free = std::time::Instant::now();
        }
        // pas fixe : la même physique que la partie réelle, silencieuse
        // (`None` = aucun son - aucune fenêtre), à la cadence demandée
        let (_, _camera) = crate::game::update(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut rng,
            None,
            dt,
        );
        // observation du pas publiée pour `GET /obs` (après `update` :
        // l'état vu est celui d'après les actions du pas - comme la boucle
        // réelle)
        crate::driver::publish_state(&state, &shapes);
    }
}

#[cfg(test)]
mod tests {
    use crate::state::GameState;
    use ::rand::SeedableRng;

    /// Déroule `steps` pas à pas fixe (60 Hz) sur le monde de la graine, en
    /// autopilote (l'ordinateur joue : la boucle doit tourner sans fenêtre) -
    /// renvoie des repères pour comparer deux exécutions (dont un point du
    /// champ d'étoiles, généré à la graine : discriminant sûr entre graines).
    fn roll(seed: u64, steps: u32) -> (f64, f64, f64, bool, i32, f64, f64) {
        crate::headless::activate();
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut garbages = Vec::new();
        let mut elements = Vec::new();
        let mut stars = Vec::new();
        let mut rng = rand_chacha::ChaCha12Rng::seed_from_u64(seed);
        crate::driver::reset_episode(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            crate::driver::EpisodeReset {
                seed,
                target: crate::driver::ResetTarget::Ship,
                x: 0.0,
                y: 0.0,
                auto_generate: true,
                scenario: crate::driver::EpisodeScenario::FreePlay,
            },
        );
        state.autopilot = true;
        let dt = 1.0 / 60.0;
        for _ in 0..steps {
            crate::game::update(
                &mut state,
                &mut shapes,
                &mut triangles,
                &mut garbages,
                &mut elements,
                &mut rng,
                None,
                dt,
            );
        }
        let ship = &shapes[crate::config::PLAYER_INDEX];
        let star = stars.first().map(|s| (s.x, s.y)).unwrap_or((0.0, 0.0));
        (
            state.session_time,
            ship.position.x,
            ship.position.y,
            state.dock_links,
            state.meteors_destroyed,
            star.0,
            star.1,
        )
    }

    /// La boucle de jeu tourne **sans fenêtre** (mode headless actif) : pas de
    /// panique, la partie avance (le vaisseau quitte la base, le monde se
    /// peuple) et le résultat est **déterministe** (même graine → mêmes
    /// positions, même temps de session).
    #[test]
    fn headless_loop_steps_deterministically_without_window() {
        let a = roll(7, 600);
        let b = roll(7, 600);
        assert_eq!(a, b, "même graine → même déroulé (positions, temps, météores)");
        assert!(a.0 > 4.0, "le temps de session avance : {}", a.0);
    }

    /// Deux graines différentes ne donnent pas le même monde (la graine pilote
    /// bien la génération).
    #[test]
    fn different_seeds_differ() {
        let a = roll(7, 600);
        let b = roll(8, 600);
        // le monde (étoiles) dépend de la graine - même déroulé sinon
        assert_ne!((a.5, a.6), (b.5, b.6), "la graine change la partie");
        assert_eq!(a.0, b.0, "le temps de session avance au même rythme");
    }


}
