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
    /// Banc d'essai au démarrage : nombre d'épisodes exécutés en continu dans
    /// le processus à pleine vitesse (0 = aucun - le serveur attend les
    /// `POST /bench` de l'entraîneur).
    pub bench: u64,
    /// Scénario du banc d'essai (économie pour la boucle de minage du
    /// vaisseau, jeu libre sinon).
    pub bench_economy: bool,
    /// Scénario **à objectifs** du banc d'essai (Phase 2) : index du scénario
    /// chargé depuis `scenarios/*.scenario.json` (résolu par `--scenario <id>`,
    /// ex. `campaign_prospector`) - `None` = pas de scénario custom.
    pub bench_custom: Option<usize>,
    /// Garde-fou du banc d'essai (pas par épisode - défaut `DEFAULT_BENCH_MAX_STEPS`).
    pub bench_max_steps: u64,
    /// Enregistrer les trajectoires du banc d'essai (JSONL obs+action, pour
    /// l'entraînement RL) ?
    pub bench_trajectories: bool,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        HeadlessOptions {
            port: DEFAULT_DRIVER_PORT,
            fps: 60.0,
            seed: 0,
            target_eva: false,
            auto_generate: false,
            bench: 0,
            bench_economy: false,
            bench_custom: None,
            bench_max_steps: 60 * 120, // cf. driver::DEFAULT_BENCH_MAX_STEPS
            bench_trajectories: false,
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
            "--bench" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    opts.bench = v;
                }
                i += 1;
            }
            "--scenario" => {
                if let Some(v) = args.get(i + 1) {
                    opts.bench_economy = v == "economy" || v == "progression";
                    // scénario à objectifs (Phase 2) : l'id d'un scénario
                    // chargé depuis scenarios/*.scenario.json (ex.
                    // campaign_prospector) prime sur economy/free
                    if let Some(idx) = crate::scenario_loader::loaded_scenarios()
                        .iter()
                        .position(|ls| ls.data.json.id == v.as_str())
                    {
                        opts.bench_custom = Some(idx);
                        opts.bench_economy = false;
                    }
                }
                i += 1;
            }
            "--max-steps" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u64>().ok()) {
                    opts.bench_max_steps = v.max(1);
                }
                i += 1;
            }
            "--trajectories" => opts.bench_trajectories = true,
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Libellé stable d'un scénario d'épisode pour les fichiers de trajectoires
/// et les événements : `"free"` / `"economy"` / l'**id du scénario à
/// objectifs** (Phase 2 - ex. `campaign_prospector`).
#[cfg(not(target_arch = "wasm32"))]
fn scenario_label(scenario: crate::driver::EpisodeScenario) -> String {
    match scenario {
        crate::driver::EpisodeScenario::FreePlay => "free".to_string(),
        crate::driver::EpisodeScenario::Economy => "economy".to_string(),
        crate::driver::EpisodeScenario::Custom(idx) => {
            crate::scenario_loader::loaded_data(idx)
                .map(|d| d.json.id.clone())
                .unwrap_or_else(|| format!("custom-{idx}"))
        }
    }
}

/// Exécute un **banc d'essai en continu** : enchaîne `req.episodes` épisodes
/// de bout en bout **dans le processus**, à pleine vitesse - chaque épisode
/// est une remise à zéro (`reset_episode`, graine `req.seed + i`), joué par
/// l'**autopilote du jeu** (`state.autopilot`) jusqu'à sa terminaison
/// explicite (`advance_episode`) ou le garde-fou `req.max_steps`. Aucune
/// publication d'observation ni d'aller-retour HTTP par pas : c'est
/// l'accélération au-delà du pas-à-pas (centaines d'épisodes/s, mesurées par
/// le rapport). Déterministe à la graine - même demande → même déroulé.
/// (Même signature monde que `reset_episode` - les mêmes vecteurs.)
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn run_bench(
    state: &mut crate::state::GameState,
    shapes: &mut Vec<crate::shape::Shape>,
    triangles: &mut Vec<crate::geom::Triangle>,
    garbages: &mut Vec<crate::garbage::Garbage>,
    elements: &mut Vec<crate::state::Element>,
    stars: &mut Vec<crate::geom::Point>,
    rng: &mut rand_chacha::ChaCha12Rng,
    req: &crate::driver::BenchRequest,
) -> crate::driver::BenchReport {
    use crate::driver::{
        episode_reward, reset_episode, BenchEpisodeResult, BenchReport, EpisodeOutcome,
        EpisodeReset, EpisodeTrack, ResetTarget,
    };
    use std::io::Write;
    // le banc d'essai est la mesure de la **référence** (l'autopilote du jeu) :
    // il ne doit pas hériter d'un pilote externe resté engagé ni d'actions
    // enfoncées d'un épisode précédent (sans ceci, un entraîneur qui termine
    // un épisode pilote-engagé avec des boutons encore enfoncés fausserait
    // tous les épisodes du banc suivant)
    crate::driver::clear_driver();
    let t_wall = std::time::Instant::now();
    let dt = 1.0 / 60.0;
    // trajectoires (RL) : fichier JSONL dans le dossier temporaire headless -
    // une ligne par pas (observation + action de l'autopilote) + bornes
    // d'épisode, pour un entraînement hors-ligne sur les décisions de la ligne
    // de base
    let traj_path = if req.trajectories {
        let dir = std::env::temp_dir().join(format!("meteors_mining_headless_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let target = match req.target {
            ResetTarget::Eva => "eva",
            ResetTarget::Ship => "ship",
        };
        let scenario = scenario_label(req.scenario);
        Some(
            dir.join(format!(
                "trajectories_{}_{}_{}_{}.jsonl",
                req.seed, req.episodes, target, scenario
            )),
        )
    } else {
        None
    };
    let mut traj = traj_path
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok())
        .map(std::io::BufWriter::new);
    let mut results = Vec::with_capacity(req.episodes as usize);
    for i in 0..req.episodes {
        let seed = req.seed.wrapping_add(i);
        // monde neuf à la graine + situation de départ (vaisseau / EVA),
        // comme un `POST /reset` consommé par la boucle
        reset_episode(
            state,
            shapes,
            triangles,
            garbages,
            elements,
            stars,
            rng,
            EpisodeReset {
                seed,
                target: req.target,
                x: req.x,
                y: req.y,
                auto_generate: req.auto_generate,
                scenario: req.scenario,
            },
        );
        // l'autopilote du jeu joue l'épisode (la référence de la ligne de
        // base - même pilote que `POST /cmd {"autopilot":true}`)
        state.autopilot = true;
        let t_episode = state.session_time;
        let mut track = EpisodeTrack::begin(i + 1, req.target, t_episode);
        if let Some(w) = traj.as_mut() {
            let _ = writeln!(
                w,
                "{}",
                serde_json::json!({
                    "event": "episode",
                    "seed": seed,
                    "target": match req.target {
                        ResetTarget::Eva => "eva",
                        ResetTarget::Ship => "ship",
                    },
                    "scenario": scenario_label(req.scenario),
                    "x": req.x,
                    "y": req.y,
                    "auto_generate": req.auto_generate,
                })
            );
        }
        // boucle de l'épisode : pas fixe à pleine vitesse (aucune attente),
        // jusqu'à la terminaison explicite ou le garde-fou
        while !track.done && track.steps < req.max_steps {
            // trajectoire : observation (état avant le pas) + action que
            // l'autopilote va appliquer ce pas - calculée ici avec les mêmes
            // fonctions pures que `player_controls`
            if let Some(w) = traj.as_mut() {
                let obs = crate::driver::observe(state, shapes);
                let pilot = if state.cosmonaut_active {
                    let p = crate::autopilot::autopilot_eva_inputs(state, shapes, dt);
                    crate::autopilot::PilotInputs {
                        up: p.up,
                        down: false,
                        left: p.left,
                        right: p.right,
                        fire: false,
                    }
                } else {
                    crate::autopilot::autopilot_inputs(state, shapes)
                };
                let _ = writeln!(
                    w,
                    "{}",
                    serde_json::json!({
                        "event": "step",
                        "seed": seed,
                        "step": track.steps + 1,
                        "t": (state.session_time - t_episode).max(0.0),
                        "action": {
                            "up": pilot.up,
                            "down": pilot.down,
                            "left": pilot.left,
                            "right": pilot.right,
                            "fire": pilot.fire,
                        },
                        "obs": obs,
                    })
                );
            }
            crate::game::update(
                state,
                shapes,
                triangles,
                garbages,
                elements,
                rng,
                None,
                dt,
            );
            crate::driver::advance_episode(&mut track, state, shapes);
        }
        // dénouement : récompense avec **les mêmes règles que l'entraîneur**
        // (vitesse d'entrée du pilote au moment du dénouement, distance
        // finale à la station)
        let seconds = (state.session_time - t_episode).max(0.0);
        let obs_end = crate::driver::observe(state, shapes);
        let entry_speed = match track.outcome {
            Some(EpisodeOutcome::EvaRecovered) => obs_end.eva.speed,
            Some(EpisodeOutcome::Delivered) => obs_end.ship.speed,
            _ => 0.0,
        };
        let reward = episode_reward(
            track.outcome,
            seconds,
            entry_speed,
            obs_end.station_dist,
            track.objective_bonus,
        );
        if let Some(w) = traj.as_mut() {
            let _ = writeln!(
                w,
                "{}",
                serde_json::json!({
                    "event": "episode_end",
                    "seed": seed,
                    "outcome": track.outcome.map(|o| o.label()),
                    "steps": track.steps,
                    "seconds": seconds,
                    "reward": reward,
                })
            );
        }
        results.push(BenchEpisodeResult {
            seed,
            outcome: track.outcome.map(|o| o.label().to_string()),
            steps: track.steps,
            seconds,
            deliveries: track.deliveries,
            collected: track.collected,
            objectives_completed: track.objectives_completed,
            objectives_total: state.objective_tracker.total_count() as u32,
            objective_bonus: track.objective_bonus,
            entry_speed,
            reward,
        });
    }
    if let Some(w) = traj.as_mut() {
        let _ = w.flush();
    }
    let wall_seconds = t_wall.elapsed().as_secs_f64();
    let count = |o: &str| {
        results
            .iter()
            .filter(|r| r.outcome.as_deref() == Some(o))
            .count() as u64
    };
    let mean = |f: fn(&BenchEpisodeResult) -> f64| -> f64 {
        if results.is_empty() {
            0.0
        } else {
            results.iter().map(f).sum::<f64>() / results.len() as f64
        }
    };
    BenchReport {
        episodes: req.episodes,
        wall_seconds,
        episodes_per_second: if wall_seconds > 0.0 {
            req.episodes as f64 / wall_seconds
        } else {
            0.0
        },
        delivered: count("delivered"),
        eva_recovered: count("eva_recovered"),
        destroyed: count("destroyed"),
        objectives_complete: count("objectives_complete"),
        timed_out: req.episodes
            - count("delivered")
            - count("eva_recovered")
            - count("destroyed")
            - count("objectives_complete"),
        mean_seconds: mean(|r| r.seconds),
        mean_reward: mean(|r| r.reward),
        trajectory_file: traj_path.map(|p| p.display().to_string()),
        results,
    }
}

/// Imprime le rapport d'un banc d'essai (cadence réelle en épisodes/s, temps
/// mur, répartition des dénouements, récompense moyenne, trajectoires) -
/// format lisible du `--bench` CLI.
#[cfg(not(target_arch = "wasm32"))]
fn print_bench_report(report: &crate::driver::BenchReport) {
    println!(
        "banc d'essai terminé : {} épisodes en {:.2} s mur → {:.0} épisodes/s",
        report.episodes, report.wall_seconds, report.episodes_per_second
    );
    println!(
        "  livrés : {} · secourus EVA : {} · détruits : {} · objectifs : {} · délais (garde-fou) : {}",
        report.delivered, report.eva_recovered, report.destroyed,
        report.objectives_complete, report.timed_out
    );
    println!(
        "  temps de simulation moyen : {:.1} s · récompense moyenne : {:.1}",
        report.mean_seconds, report.mean_reward
    );
    if let Some(path) = &report.trajectory_file {
        println!("  trajectoires (RL) : {path}");
    }
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

    // banc d'essai au démarrage (`--bench N`) : le lot s'exécute **en
    // continu dans le processus** à pleine vitesse (reset → autopilote →
    // terminaison explicite, aucun aller-retour HTTP par pas), le rapport est
    // imprimé puis l'interface continue de servir (l'entraîneur peut le
    // relire via `GET /bench`)
    if opts.bench > 0 {
        // mode EVA : le crash est posé à 300 unités à l'est par défaut (le
        // même départ que `evaluate.py`) - un crash en (0, 0), centre de la
        // station, serait récupéré au premier pas (épisodes triviaux)
        let (x, y) = if opts.target_eva { (300.0, 0.0) } else { (0.0, 0.0) };
        // scénario de l'épisode : scénario à objectifs demandé (`--scenario
        // <id>`, Phase 2), sinon économie ou jeu libre
        let scenario = if let Some(idx) = opts.bench_custom {
            crate::driver::EpisodeScenario::Custom(idx)
        } else if opts.bench_economy {
            crate::driver::EpisodeScenario::Economy
        } else {
            crate::driver::EpisodeScenario::FreePlay
        };
        let req = crate::driver::BenchRequest {
            episodes: opts.bench,
            seed: opts.seed,
            target: if opts.target_eva {
                crate::driver::ResetTarget::Eva
            } else {
                crate::driver::ResetTarget::Ship
            },
            x,
            y,
            auto_generate: opts.auto_generate,
            scenario,
            max_steps: opts.bench_max_steps,
            trajectories: opts.bench_trajectories,
        };
        let report = run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        crate::driver::publish_bench_report(report.clone());
        crate::driver::publish_state(&mut state, &shapes);
        print_bench_report(&report);
        world_ready = true;
    }

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
        // banc d'essai demandé par `POST /bench` : le lot s'exécute **en
        // continu dans le processus** à pleine vitesse (aucun aller-retour
        // HTTP par pas - l'autopilote du jeu joue chaque épisode jusqu'à sa
        // terminaison explicite), puis le rapport est publié pour `GET /bench`
        if let Some(req) = crate::driver::take_bench() {
            let report = run_bench(
                &mut state,
                &mut shapes,
                &mut triangles,
                &mut garbages,
                &mut elements,
                &mut stars,
                &mut rng,
                &req,
            );
            crate::driver::publish_bench_report(report);
            // l'observation publiée reste celle de l'état final du dernier
            // épisode du lot (le rapport, lui, est servi par `/bench`)
            crate::driver::publish_state(&mut state, &shapes);
            continue;
        }
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
        crate::driver::publish_state(&mut state, &shapes);
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

    /// Environnement de banc d'essai : monde + vecteurs, comme la boucle
    /// headless - renvoie aussi le RNG (réinitialisé à chaque appel par la
    /// fonction elle-même via `reset_episode`). Le tuple de 7 éléments est
    /// volontaire : chaque test du banc déstructure directement ses champs.
    #[allow(clippy::type_complexity)]
    fn bench_env() -> (
        GameState,
        Vec<crate::shape::Shape>,
        Vec<crate::geom::Triangle>,
        Vec<crate::garbage::Garbage>,
        Vec<crate::state::Element>,
        Vec<crate::geom::Point>,
        rand_chacha::ChaCha12Rng,
    ) {
        crate::headless::activate();
        (
            GameState::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            rand_chacha::ChaCha12Rng::seed_from_u64(0),
        )
    }

    /// Le banc d'essai **en continu dans le processus** enchaîne les épisodes
    /// à pleine vitesse : chaque épisode EVA (graines 1..3) joué par
    /// l'autopilote du jeu se termine en **secours** (`eva_recovered`), le
    /// rapport expose le déroulé complet et la cadence mur.
    #[test]
    fn bench_runs_eva_episodes_to_completion() {
        let (mut state, mut shapes, mut triangles, mut garbages, mut elements, mut stars, mut rng) =
            bench_env();
        let req = crate::driver::BenchRequest {
            episodes: 3,
            seed: 1,
            target: crate::driver::ResetTarget::Eva,
            x: 300.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::FreePlay,
            max_steps: 60 * 60, // garde-fou généreux : 60 s de simulation
            trajectories: false,
        };
        let report = super::run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        assert_eq!(report.episodes, 3);
        assert_eq!(report.results.len(), 3);
        assert_eq!(report.eva_recovered, 3, "l'autopilote ramène le cosmonaute");
        assert_eq!(report.delivered, 0);
        assert_eq!(report.timed_out, 0);
        assert!(report.wall_seconds > 0.0, "temps mur mesuré");
        assert!(report.episodes_per_second > 0.0, "cadence mesurée");
        for r in &report.results {
            assert_eq!(r.outcome.as_deref(), Some("eva_recovered"));
            assert!(r.steps > 0);
            assert!(r.seconds > 0.0);
        }
    }

    /// Le banc d'essai **dégage un pilote externe resté engagé** : un
    /// entraîneur qui termine un épisode rejoué pas à pas (pilote externe
    /// engagé, boutons encore enfoncés - `driver: true, up: true, left: true`)
    /// ne doit pas fausser les épisodes du banc suivant - c'est l'autopilote
    /// de référence qui pilote (`clear_driver` appelé en tête de `run_bench`,
    /// sinon le vaisseau pousserait en permanence et mourrait avant la
    /// première mission).
    #[test]
    fn bench_disengages_a_stuck_external_driver() {
        // l'entraîneur laisse le pilote externe engagé avec des boutons enfoncés
        assert!(crate::driver::apply_cmd(r#"{"driver":true,"up":true,"left":true}"#));
        assert!(crate::driver::engaged());
        assert!(crate::driver::up() && crate::driver::left());
        let req = crate::driver::BenchRequest {
            episodes: 3,
            seed: 1,
            target: crate::driver::ResetTarget::Eva,
            x: 300.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::FreePlay,
            max_steps: 60 * 60, // garde-fou généreux : 60 s de simulation
            trajectories: false,
        };
        let (mut state, mut shapes, mut triangles, mut garbages, mut elements, mut stars, mut rng) =
            bench_env();
        let report = super::run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        // les épisodes sont joués par l'autopilote (pas par le pilote externe
        // resté engagé) : le cosmonaute EVA est secouru, comme sans pilote
        assert_eq!(report.eva_recovered, 3, "l'autopilote pilote le banc");
        // et le pilote externe a été dégagé / relâché par le banc
        assert!(!crate::driver::engaged());
        assert!(!crate::driver::up() && !crate::driver::left());
    }

    /// Le banc d'essai est **déterministe à la graine** : même demande → même
    /// déroulé (dénouements, pas, temps de simulation de chaque épisode) -
    /// seule la cadence mur varie.
    #[test]
    fn bench_is_deterministic_per_seed() {
        let req = crate::driver::BenchRequest {
            episodes: 4,
            seed: 100,
            target: crate::driver::ResetTarget::Eva,
            x: 300.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::FreePlay,
            max_steps: 60 * 60,
            trajectories: false,
        };
        let (mut s1, mut sh1, mut tr1, mut g1, mut e1, mut st1, mut r1) = bench_env();
        let a = super::run_bench(
            &mut s1, &mut sh1, &mut tr1, &mut g1, &mut e1, &mut st1, &mut r1, &req,
        );
        let (mut s2, mut sh2, mut tr2, mut g2, mut e2, mut st2, mut r2) = bench_env();
        let b = super::run_bench(
            &mut s2, &mut sh2, &mut tr2, &mut g2, &mut e2, &mut st2, &mut r2, &req,
        );
        let strip = |rep: &crate::driver::BenchReport| {
            rep.results
                .iter()
                .map(|r| (r.outcome.clone(), r.steps, (r.seconds * 100.0) as u64))
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(&a), strip(&b), "même graine → même déroulé");
    }

    /// Le rapport du banc d'essai expose la **récompense** de chaque épisode -
    /// les mêmes règles que l'entraîneur : secours EVA réussi → +1000 − 2·s −
    /// pénalité d'arrivée trop rapide (vitesse d'entrée > 30 u/s). La formule
    /// exacte est vérifiée avec la vitesse d'entrée exposée (elle peut
    /// dépasser 30 u/s selon la graine - la pénalité s'applique alors).
    #[test]
    fn bench_reports_trainer_rewards() {
        let (mut state, mut shapes, mut triangles, mut garbages, mut elements, mut stars, mut rng) =
            bench_env();
        let req = crate::driver::BenchRequest {
            episodes: 2,
            seed: 1,
            target: crate::driver::ResetTarget::Eva,
            x: 300.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::FreePlay,
            max_steps: 60 * 60,
            trajectories: false,
        };
        let report = super::run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        for r in &report.results {
            assert_eq!(r.outcome.as_deref(), Some("eva_recovered"));
            // formule de l'entraîneur : +1000 − 2·s − max(0, v_entrée − 30)·5
            let overshoot = (r.entry_speed - 30.0).max(0.0) * 5.0;
            let expected = 1000.0 - 2.0 * r.seconds - overshoot;
            assert!(
                (r.reward - expected).abs() < 1e-6,
                "récompense = formule de l'entraîneur : {}",
                r.reward
            );
            assert!(r.entry_speed > 0.0, "vitesse d'entrée mesurée");
        }
        let mean = report.results.iter().map(|r| r.reward).sum::<f64>() / 2.0;
        assert!((report.mean_reward - mean).abs() < 1e-6);
    }

    /// Un banc d'essai avec `trajectories` écrit un fichier JSONL : une ligne
    /// d'épisode, une ligne par pas (observation + action de l'autopilote) et
    /// une ligne de dénouement (avec la récompense) - lisible par un
    /// entraînement RL hors-ligne. Chemin exposé dans le rapport.
    #[test]
    fn bench_writes_trajectory_files() {
        let (mut state, mut shapes, mut triangles, mut garbages, mut elements, mut stars, mut rng) =
            bench_env();
        let req = crate::driver::BenchRequest {
            episodes: 2,
            seed: 10,
            target: crate::driver::ResetTarget::Eva,
            x: 300.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::FreePlay,
            max_steps: 60 * 60,
            trajectories: true,
        };
        let report = super::run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        let path = report.trajectory_file.expect("le chemin est exposé");
        let content = std::fs::read_to_string(&path).expect("fichier lisible");
        // bornes d'épisode + pas + dénouements : 2 épisodes → ≥ 6 lignes
        let lines: Vec<&str> = content.lines().collect();
        assert!(lines.len() >= 6, "{} lignes", lines.len());
        // chaque ligne est du JSON ; les pas portent observation + action
        let steps: Vec<serde_json::Value> = lines
            .iter()
            .filter(|l| l.contains("\"event\":\"step\""))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(!steps.is_empty());
        let first = &steps[0];
        assert!(first.get("obs").is_some(), "observation par pas");
        let action = first.get("action").unwrap();
        assert!(action.get("up").is_some() && action.get("fire").is_some());
        // dénouements avec récompense
        let ends: Vec<serde_json::Value> = lines
            .iter()
            .filter(|l| l.contains("\"event\":\"episode_end\""))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(ends.len(), 2);
        assert!(ends[0].get("reward").and_then(|r| r.as_f64()).unwrap_or(0.0) > 900.0);
        // le fichier est dans le dossier temporaire headless
        assert!(path.contains("meteors_mining_headless_"), "{path}");
    }

    /// Le banc d'essai vaisseau (économie) joue la **boucle de minage** : les
    /// épisodes de la plage de graines se terminent (livraison ou destruction)
    /// ou atteignent le garde-fou - le rapport compte chaque dénouement et les
    /// livraisons effectuées.
    #[test]
    fn bench_ship_economy_episodes_reach_outcomes() {
        let (mut state, mut shapes, mut triangles, mut garbages, mut elements, mut stars, mut rng) =
            bench_env();
        let req = crate::driver::BenchRequest {
            episodes: 3,
            seed: 500,
            target: crate::driver::ResetTarget::Ship,
            x: 0.0,
            y: 0.0,
            auto_generate: false,
            scenario: crate::driver::EpisodeScenario::Economy,
            max_steps: 60 * 90, // garde-fou : 90 s de simulation par épisode
            trajectories: false,
        };
        let report = super::run_bench(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut stars,
            &mut rng,
            &req,
        );
        assert_eq!(report.results.len(), 3);
        let sum = report.delivered + report.eva_recovered + report.destroyed + report.timed_out;
        assert_eq!(sum, 3, "chaque épisode a un dénouement (ou le garde-fou)");
        assert!(report.delivered > 0, "la boucle de minage livre au moins un épisode");
        for r in &report.results {
            assert!(r.steps > 0);
        }
    }

}
