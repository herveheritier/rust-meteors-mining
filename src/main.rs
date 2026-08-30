//! Portage Rust de « Meteors Mining ».
//!
//! - **Phase 0** (faite) : fenêtre 960×540, boucle macroquad.
//! - **Phase 1** (faite) : modèle de données - `config.rs` (constantes),
//!   `geom.rs` (Point/World/Segment/Triangle), `shape.rs` (Shape + meshes),
//!   `garbage.rs` (débris), `state.rs` (état du jeu), `generate.rs`
//!   (génération procédurale + `prepare`).
//! - **Phase 2** (faite) : rendu - `render.rs` (assets, étoiles
//!   précalculées, triangles texturés, formes, caméra centrée joueur, HUD,
//!   messages). Plein écran = **zoom** : vue 960×540 rendue dans une texture
//!   puis étirée (F → fenêtre 1920×1080, même contenu juste plus grand -
//!   voir `docs/PORTAGE.md` §4.1).
//! - **Phases 3-4** (jalons M2 à M5 faits) : boucle de jeu - `game.rs`
//!   (input, 4 modes de déplacement, pause, plein écran, météores : G + auto,
//!   collisions SAT + élastique, débris, messages, tirs, minerais, accostage,
//!   aide S, debug D/I), `title.rs` (écran titre).

mod audio;
mod build_info;
mod config;
mod cosmonaut;
mod difficulty;
mod modding;
mod dock_render;
mod docking;
mod eva;
mod font;
mod game;
mod gamepad;
mod garbage;
mod generate;
mod geom;
mod hud;
mod input;
mod marketplace;
mod objective_tracker;
mod persist;
mod remote;
mod render;
mod scenario;
mod scenario_loader;
mod scenario_objectives;
mod settings;
mod shape;
mod shop;
mod shop_render;
mod state;
mod title;
mod touch;
mod ui_boxes;
mod vaisseau;
mod wasm_audio_shims; // no-op audio sur wasm (silencieux) - vide sur le natif
mod x11;

use macroquad::prelude::*;

use crate::config::{
    view_mode_message, ATTEMPT_FPS, EVA_CROSSFADE_DURATION, PLAYER_INDEX, STATION_INDEX,
    VIEWPORT_HEIGHT, VIEWPORT_WIDTH, WINDOW_SIZES, WINDOW_TITLE,
};
use crate::geom::Point;
use crate::shape::Shape;
use crate::state::{GameState, RenderStyle, ViewMode};
use std::f64::consts::TAU;

fn window_conf() -> Conf {
    // options graphiques persistées (écran de paramétrage) : définition de
    // la fenêtre (taille initiale en fenêtré) et anticrénelage MSAA - ce
    // dernier est fixé à la **création** de la fenêtre (macroquad ne permet
    // pas de le changer à chaud) : il prend effet au lancement suivant
    // taille de fenêtre persistée : la taille **réelle** (redimensionnement à
    // la main, clés `win_w`/`win_h`) prime sur l'index du réglage SIZE
    let (win_w, win_h) = persist::load_window_px_size()
        .or_else(persist::load_window_size)
        .unwrap_or((VIEWPORT_WIDTH as i32, VIEWPORT_HEIGHT as i32));
    let antialias = persist::get_bool("antialias").unwrap_or(false);
    Conf {
        window_title: WINDOW_TITLE.to_owned(),
        window_width: win_w,
        window_height: win_h,
        sample_count: if antialias { 4 } else { 1 },
        high_dpi: true,
        // pas de vsync : l'original (QB64) tourne sans vsync (110 FPS) ;
        // miniquad force `swap_interval = 1` par défaut, ce qui plafonne le
        // rendu au rafraîchissement de l'écran.
        platform: miniquad::conf::Platform {
            swap_interval: Some(0),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Cadence de boucle (filet anti-fuite, ex `_limit ATTEMPT_FPS` de l'original) :
/// bloque jusqu'à ce que `target_frame` (s) se soient écoulées depuis la
/// dernière frame, puis met `last_frame` à jour.
///
/// `std::thread::sleep` **panique sur wasm32-unknown-unknown** (« can't
/// sleep », voir `std::sys::thread::unsupported`) : sur le web on ne dort pas
/// (le navigateur cadence déjà la boucle via `requestAnimationFrame`, ~60 FPS,
/// bien sous le plafond - le sleep ne servirait à rien).
fn frame_pace(target_frame: f64, last_frame: &mut f64) {
    let elapsed = get_time() - *last_frame;
    if elapsed < target_frame {
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_secs_f64(target_frame - elapsed));
    }
    *last_frame = get_time();
}

/// Relance l'exécutable courant (bouton RESTART de l'écran de paramétrage,
/// ex après un changement d'anticrénelage) : le fichier de config contient
/// déjà les réglages modifiés. Renvoie `false` si la relance a échoué (le jeu
/// continue alors normalement - le réglage s'appliquera au lancement manuel).
fn restart_process() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(exe).args(&args).spawn().is_ok()
}

/// Préparation du lancement d'une partie, après l'écran titre : position et
/// orientation initiales du vaisseau (scénarios custom, valeurs de l'éditeur
/// de scénarios ; la dernière position sauvegardée prime avec l'option SAVE
/// POSITION) puis état d'accostage de départ - à quai si le départ est
/// immobile au centre de la station. Termine en vidant la file d'input (le
/// keypress qui a lancé la partie ne doit pas être relu par la première
/// frame de jeu, ex F → mode d'affichage re-basculé).
fn launch_setup(state: &mut GameState, shapes: &mut [Shape]) {
    let mut start_x = state.initial_ship_x;
    let mut start_y = state.initial_ship_y;
    let start_orientation = state.initial_ship_orientation.to_radians();
    let start_velocity = state.initial_ship_velocity;
    // option SAVE POSITION (case de l'écran de paramétrage) : la dernière
    // position du vaisseau (sauvegardée à la sortie) écrase la position de
    // départ - le joueur reprend exactement où il était (position nulle =
    // à quai, comportement normal)
    if state.save_position {
        if let (Some(x), Some(y)) = (persist::get_i32("ship_x"), persist::get_i32("ship_y")) {
            start_x = x as f64;
            start_y = y as f64;
        }
    }
    if start_x != 0.0 || start_y != 0.0 || start_orientation != 0.0 || start_velocity != 0.0 {
        let s = &mut shapes[PLAYER_INDEX];
        s.position.x = start_x;
        s.position.y = start_y;
        s.orientation = start_orientation;
        s.velocity = start_velocity;
    }
    // Le vaisseau démarre à quai (liens d'accostage attachés, statut
    // « DOCKED ») seulement s'il est **immobile au centre de la station** :
    // position initiale (0,0) ET vitesse nulle - une position ou une vitesse
    // initiale non nulle (scénario custom de l'éditeur) signifie que le
    // vaisseau démarre en vol, hors de la base (pas de liens, pas
    // d'accostage ; la mire réapparaîtra au retour, voir
    // `docking::update_docking_guide`)
    let start_docked = crate::scenario::start_docked(state);
    state.dock_links = start_docked;
    state.player_at_station = if start_docked { -1 } else { 0 };

    clear_input_queue();
}

/// Annonce l'URL de la télécommande (journal + message HUD). NB : la file de
/// messages du HUD découpe sur '/' (séparateur) - le message affiche
/// l'adresse sans le schéma `http://` (l'URL complète est visible dans
/// l'écran de paramétrage, touche O).
fn announce_remote(state: &mut GameState, url: &str) {
    // NB : la macro info! de macroquad ne capture pas les identifiants
    // inline (contrairement à format!) - arguments explicites obligatoires
    info!("Remote control ready: {}", url);
    let host_port = url.trim_start_matches("http://").trim_end_matches('/');
    state.send_message(&format!("REMOTE CONTROL: {host_port}"));
}

#[macroquad::main(window_conf)]
async fn main() {
    // ─── Phase 1 : modèle de données ────────────────────────────────────────
    // L'état initial (monde torique, joueur, étoiles, station) est construit
    // par `prepare`, exactement comme le `prepare` du jeu QB64.
    let mut state = GameState::new();
    // police embarquée (DejaVu Sans Mono, `include_bytes!`) : définie comme
    // police par défaut dès le démarrage - tous les textes utilisent son jeu
    // de caractères étendu (Latin-1, flèches, coches) à l'échelle 8 px
    crate::font::init();
    // réglages persistés (fichier de config utilisateur, norme XDG - ex
    // `~/.config/meteors-mining/meteors_mining.cfg`) : le mode de
    // déplacement choisi au magasin de la station (bouton SHOP de la
    // boîte DOCK STATION) remplace le défaut au lancement (comme s'il venait
    // d'être sélectionné, sans message intempestif) ; volume et musique
    // s'appliquent aux sons ci-dessous. NB : la génération automatique des
    // météores
    // (touche A) n'est **pas** persistée - elle repart toujours active au
    // lancement (défaut de `GameState`), pour que le monde ne soit jamais
    // vide sans que le joueur l'ait demandé pour la session en cours.
    if let Some(mode) = persist::load_moving_mode() {
        state.moving_mode = mode;
    }
    // options graphiques persistées (écran de paramétrage O) : style de
    // rendu et anticrénelage (reflété par la case, l'effet étant appliqué
    // par `window_conf`).
    if let Some(style) = persist::load_render_style() {
        state.render_style = match style {
            1 => RenderStyle::Colored,
            2 => RenderStyle::Mesh,
            _ => RenderStyle::Textured,
        };
    }
    // position de la fenêtre fenêtrée persistée (déplacement à la main,
    // clés `win_x`/`win_y`) : restaurée au lancement (X11 ; sans effet hors
    // Linux), avant l'entrée éventuelle en plein écran ci-dessous
    if let Some((x, y)) = persist::load_window_pos() {
        crate::x11::move_window(x, y);
    }
    // mode d'affichage persisté (touche F ou clic WINDOW de l'écran O) : le
    // jeu démarre dans le dernier mode utilisé - le plein écran (zoomé ou
    // natif) est entré dès l'écran titre, comme le ferait la touche F ; le
    // cycle F reste prévisible puisqu'il part de l'état réellement appliqué
    if let Some(mode) = persist::load_view_mode() {
        state.view_mode = match mode {
            1 => ViewMode::Zoomed,
            2 => ViewMode::Native,
            _ => ViewMode::Windowed,
        };
        if state.view_mode != ViewMode::Windowed {
            render::enter_fullscreen();
        }
    }
    if let Some(size) = persist::get_i32("window_size") {
        if (0..WINDOW_SIZES.len() as i32).contains(&size) {
            state.window_size = size;
        }
    }
    if let Some(aa) = persist::get_bool("antialias") {
        state.antialias = aa;
    }
    // interface tactile (joystick + bouton de tir, case TOUCH UI de l'écran
    // de paramétrage) : affichée par défaut - le réglage est persisté (clé
    // `touch_ui`) et synchronise l'interrupteur de `touch.rs` (les contrôles
    // ne sont pris en compte que s'ils sont affichés)
    if let Some(on) = persist::get_bool("touch_ui") {
        state.touch_ui = on;
    }
    crate::touch::set_enabled(state.touch_ui);
    // option SAVE POSITION (case de l'écran de paramétrage, clé
    // `save_position`) : le vaisseau repart de sa dernière position à la
    // sortie (voir plus bas, au lancement de la partie)
    if let Some(on) = persist::get_bool("save_position") {
        state.save_position = on;
    }
    // PIN de la télécommande HTTP (ligne REMOTE PIN de l'écran de
    // paramétrage, clé `remote_pin`) : chargé au lancement - vide = aucune
    // protection (comportement historique)
    if let Some(pin) = persist::load_remote_pin() {
        state.remote_pin = pin;
    }
    // la valeur effectivement appliquée par la fenêtre (`window_conf` lit la
    // même clé) : si `antialias` en diffère ensuite, un redémarrage est
    // nécessaire (bouton RESTART de l'écran de paramétrage)
    state.antialias_applied = state.antialias;
    // scénario (choisi à l'écran titre, touche N - défaut : jeu libre) : le
    // dernier scénario joué est persisté (`scenario`) - on le restaure avant
    // d'appliquer ses règles de départ (ressources initiales, modes débloqués
    // et, en PROGRESSION, le mode de déplacement imposé REALISTIC), puis la
    // progression enregistrée (minerais, modes payés, réputation - clés
    // `prog_*`) est surimposée sur ces valeurs de départ
    if let Some(id) = crate::scenario::load_scenario() {
        state.scenario = id;
    }
    crate::scenario::apply_start(&mut state);
    crate::scenario::load_progression(&mut state);
    let mut shapes = Vec::new();
    let mut triangles = Vec::new();
    let mut stars: Vec<Point> = Vec::new();
    let mut elements = Vec::new();
    let mut rng = generate::seeded_rng();
    generate::prepare(&mut state, &mut shapes, &mut triangles, &mut stars, &mut elements, &mut rng);
    // cosmonaute EVA - le pilote contrôlé quand le vaisseau est détruit
    // (`game.rs` : `activate_cosmonaut`/`rescue_cosmonaut`) : chargé depuis
    // `assets/cosmonaute.json` (couleurs par face), petit, garé hors écran en
    // bord de monde, téléporté au crash le moment venu (aucun cosmonaute
    // décoratif près de la base)
    state.eva_cosmonaut = cosmonaut::create_eva_cosmonaut(&mut shapes, &mut triangles) as i32;

    info!(
        "Phase 1 OK : {} formes (dont cosmonaute), {} triangles, {} étoiles, {} éléments",
        shapes.len(),
        triangles.len(),
        stars.len(),
        elements.len(),
    );

    // ─── Phase 2 : assets ───────────────────────────────────────────────────
    let assets = render::Assets::load().await;
    info!(
        "Phase 2 OK : 4 textures + {} couches d'étoiles précalculées",
        assets.star_layers.len(),
    );

    // Débris (vides au départ, remplis par les explosions en M3/M4)
    let mut garbages = Vec::new();

    // ─── Audio (Phase 4, ex les `_sndopen` de `meteorsMining.bas`) ─────────
    // Sons chargés une fois ; l'ambiance démarre avec la partie (après
    // l'écran titre), la musique dès le lancement si activée - comme
    // l'original (`mainLoop`) mais rendu cohérent avec l'écran de paramétrage
    // accessible depuis le titre.
    let mut sounds = audio::Sounds::load().await;
    // réglages audio persistés (clés `volume` et `music` du fichier de
    // config) : volume maître avant le démarrage des boucles ; la musique
    // démarre dès l'écran titre si activée - l'écran de paramétrage (touche
    // O) y est accessible et sa case MUSIC doit refléter l'état réel
    if let Some(pct) = persist::get_i32("volume") {
        sounds.set_volume(pct as f32 / 100.0);
    }
    // sous-volumes persistés (musique / effets / ambiance - clés
    // `music_volume`, `effects_volume`, `ambient_volume`) : appliqués avant
    // le démarrage des boucles
    if let Some(pct) = persist::get_i32("music_volume") {
        sounds.music_volume = (pct as f32 / 100.0).clamp(0.0, 1.0);
    }
    if let Some(pct) = persist::get_i32("effects_volume") {
        sounds.effects_volume = (pct as f32 / 100.0).clamp(0.0, 1.0);
    }
    if let Some(pct) = persist::get_i32("ambient_volume") {
        sounds.ambient_volume = (pct as f32 / 100.0).clamp(0.0, 1.0);
    }
    if persist::get_bool("music").unwrap_or(true) {
        sounds.start_music();
    }

    // ─── Zoom plein écran (touche F) ────────────────────────────────────────
    // La vue 960×540 est rendue dans une texture puis affichée étirée dans la
    // fenêtre (ex `letterbox.rs` de macroquad) : en fenêtré elle est affichée
    // 1:1, en « plein écran » la fenêtre est agrandie et le contenu zoomé -
    // même contenu, juste plus grand (le vrai plein écran EWMH n'est pas
    // fiable sur tous les affichages, voir `docs/PORTAGE.md` §7).
    let render_target = render_target(VIEWPORT_WIDTH as u32, VIEWPORT_HEIGHT as u32);
    render_target.texture.set_filter(FilterMode::Linear);



    // ─── Écran titre (jalon M5, ex `titleLoop`) ─────────────────────────────
    // `sounds` est transmis pour l'écran de paramétrage (touche O) accessible
    // depuis le titre (musique et volume y sont réglables). Triplet en retour :
    // ESC → quitter l'application ; `true` en 2e position → le bouton RESTART
    // a été cliqué → on relance le jeu immédiatement.
    let (title_quit, title_restart, _title_progression_reset) =
        title::title_loop(&mut state, &assets, &render_target, &mut sounds).await;
    if title_quit {
        // ESC pressé sur l'écran titre : quitter l'application
        return;
    }
    if title_restart && restart_process() {
        return;
    }

    // Le vaisseau démarre **à quai** au centre de la station : position,
    // vitesse et coque remises à zéro (au premier lancement il est déjà
    // neuf - `prepare` ; au retour de l'écran titre, touche T sur GAME OVER,
    // il était peut-être détruit - la reconstruction suit les niveaux
    // d'atelier courants, progression éventuellement remise à zéro par le
    // bouton RESET PROGRESSION du titre - `_title_progression_reset`).
    eva::respawn_player(&mut state, &mut shapes, &mut triangles);
    launch_setup(&mut state, &mut shapes);
    // Briefing pré-partie : les scénarios custom avec objectifs affichent
    // leur résumé (objectifs DAG, contraintes, conseil) avant de jouer
    state.briefing_box = state.objective_tracker.has_objectives();
    state.briefing_scroll = 0.0; // nouveau briefing : défilement en haut

    // ─── Télécommande HTTP (piloter le jeu depuis un téléphone) ─────────────
    // Le serveur local démarre au lancement (`remote.rs`) : la page de
    // contrôle (joystick + FIRE + état en direct) est servie sur le réseau
    // local - l'URL à ouvrir sur le téléphone est annoncée (message HUD +)
    // et journalisée. En cas d'échec (port occupé…), le jeu continue sans
    // télécommande. Une seule fois par processus : au retour de l'écran titre
    // (T sur GAME OVER), le serveur tourne déjà.
    match crate::remote::start() {
        Ok(url) => announce_remote(&mut state, &url),
        Err(e) => info!("Remote control disabled: {e}"),
    }

    // ambiance + musique de la partie (ex `_sndloop sh6&/sh7&` de mainLoop).
    // La musique est relue du fichier : un changement fait pendant l'écran de
    // paramétrage du titre est pris en compte (`start_music` est sans effet
    // si elle est déjà en lecture).
    sounds.start_ambient();
    if persist::get_bool("music").unwrap_or(true) {
        sounds.start_music();
    }

    // ─── Boucle principale (Phase 3 / jalons M2-M5) ─────────────────────────
    // Limitation de boucle (ex `_limit ATTEMPT_FPS` = 600 de l'original) :
    // la physique est en `dt` (indépendante du FPS), mais un pas de temps
    // stable améliore la régularité (interpolation du centre, compteurs de
    // poussée, comptage des frames) et évite de chauffer le GPU inutilement.
    // macroquad 0.4 n'offre ni `set_target_fps` ni vsync → pacing manuel.
    // NB : le cap à 600 ne se déclenche jamais ici (FPS réel ~230 en fenêtré,
    // ~65 en plein écran sur le GPU virtio) - c'est un filet anti-fuite comme
    // le `_limit 600` de l'original.
    const LIMIT_FPS: bool = true;
    let target_frame = 1.0 / ATTEMPT_FPS as f64;
    let mut last_frame = get_time();
    // le mode d'affichage actif (persisté, touche F) est annoncé dans le HUD
    // au lancement de la partie - même message que la touche F
    state.send_message(view_mode_message(state.view_mode as i32));
    let mut pending_fullscreen = state.view_mode != ViewMode::Windowed;
    loop {
        if LIMIT_FPS {
            frame_pace(target_frame, &mut last_frame);
        }

        // filet de sécurité : ré-applique le plein écran si le titre a
        // basculé (touche F) mais que la bascule n'a pas abouti avant le
        // lancement (entrée propre - voir `render::enter_fullscreen`)
        if pending_fullscreen {
            pending_fullscreen = false;
            render::enter_fullscreen();
        }

        // fenêtre fenêtrée : persiste position/taille réelles quand elles
        // changent (déplacement ou redimensionnement par le WM) - au plus une
        // vérification par seconde (`persist_window_geometry`)
        render::persist_window_geometry(&state);

        // Input + physique + collisions (mouvement, météores, pause, modes
        // d'affichage, musique) - M2/M3. La caméra est calculée par update
        // (comme l'original, après la résolution des collisions).
        // Manette : gilrs met à jour l'état interne à la lecture de ses
        // événements - poll au début de la frame (no-op sur wasm)
        crate::gamepad::poll();
        let dt = get_frame_time() as f64;
        let (action, camera) = game::update(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut rng,
            Some(&mut sounds),
            dt,
        );
        match action {
            game::Action::Quit => {
                // filet de sécurité : la progression (minerais, modes,
                // réputation, vies/bouclier) est écrite à la sortie, au cas
                // où un changement n'aurait pas été persisté au moment où il
                // s'est produit
                let _ = crate::scenario::save_progression(&state);
                // option SAVE POSITION : la position du vaisseau (centres)
                // est sauvegardée pour le prochain lancement (clés `ship_x` /
                // `ship_y`, arrondies à l'unité monde)
                if state.save_position {
                    let p = shapes[PLAYER_INDEX].position;
                    let _ = persist::set_i32("ship_x", p.x.round() as i32);
                    let _ = persist::set_i32("ship_y", p.y.round() as i32);
                }
                break;
            }
            // RESTART (écran de paramétrage, ex changement d'anticrénelage) :
            // relance l'exécutable - les réglages sont déjà écrits dans le
            // fichier de config - puis quitte (sans effet si la relance échoue)
            game::Action::Restart => {
                if restart_process() {
                    break;
                }
            }
            // R / bouton NEW GAME (écran GAME OVER) : repartir du début - la
            // progression est remise à zéro (clés `prog_*` supprimées, règles
            // de départ réappliquées : vies et bouclier pleins en Survival,
            // compteurs à zéro, extensions d'atelier perdues) et le vaisseau
            // renaît à quai au centre de la station ; le monde (météores,
            // débris, minerais) continue de tourner
            game::Action::NewGame => {
                game::reset_for_new_game(&mut state, &mut shapes, &mut triangles);
            }
            // T / bouton TITLE (écran GAME OVER) : retour à l'écran titre -
            // progression et position sauvegardées comme à la sortie (ESC),
            // puis l'écran titre rejoue son choix (poursuivre, repartir du
            // début, changer de scénario) avant de relancer la partie
            game::Action::BackToTitle => {
                let _ = crate::scenario::save_progression(&state);
                if state.save_position {
                    let p = shapes[PLAYER_INDEX].position;
                    let _ = persist::set_i32("ship_x", p.x.round() as i32);
                    let _ = persist::set_i32("ship_y", p.y.round() as i32);
                }
                // ré-applique le scénario (peut être changé au titre) + les
                // règles de départ + la progression enregistrée : l'état est
                // propre pour l'écran titre (résumé de sauvegarde) comme pour
                // la partie qui suit
                if let Some(id) = crate::scenario::load_scenario() {
                    state.scenario = id;
                }
                crate::scenario::apply_start(&mut state);
                crate::scenario::load_progression(&mut state);
                // la touche T (ou le clic) qui a demandé le retour ne doit pas
                // être relue par l'écran titre (une touche quelconque y lance
                // la partie)
                clear_input_queue();
                let (title_quit, title_restart, _progression_reset) =
                    title::title_loop(&mut state, &assets, &render_target, &mut sounds).await;
                if title_quit {
                    // ESC sur l'écran titre : quitter l'application
                    break;
                }
                if title_restart && restart_process() {
                    break;
                }
                // vaisseau reconstruit à quai (progression éventuellement
                // remise à zéro au titre, ou partie terminée) puis position
                // et état d'accostage de départ (mêmes règles qu'au lancement)
                eva::respawn_player(&mut state, &mut shapes, &mut triangles);
                launch_setup(&mut state, &mut shapes);
                // briefing pré-partie ré-affiché (défilement remis en haut)
                state.briefing_box = state.objective_tracker.has_objectives();
                state.briefing_scroll = 0.0;
                // ambiance/musique relancées si le titre les a coupées, et
                // mode d'affichage ré-annoncé (même message qu'au lancement)
                sounds.start_ambient();
                if persist::get_bool("music").unwrap_or(true) {
                    sounds.start_music();
                }
                // l'URL de la télécommande est ré-annoncée dans le HUD (le
                // serveur tourne déjà - démarré au lancement du processus)
                if let Some(url) = crate::remote::url() {
                    announce_remote(&mut state, &url);
                }
                state.send_message(view_mode_message(state.view_mode as i32));
                pending_fullscreen = state.view_mode != ViewMode::Windowed;
                // la caméra de la frame (calculée avant le titre) est obsolète
                // : on saute le rendu de cette frame, la suivante repart propre
                continue;
            }
            game::Action::Continue => {}
        }

        // boucle moteur avant/recul (ex `_sndloop/_sndpause sh8&/sh9&`) :
        // coupée pendant les boîtes (l'original la coupe à l'accostage) et
        // pendant l'animation d'accostage / la rétraction des liens
        let engine_on = state.player.thrusted != 0
            && !state.dock_box
            && !state.shop_box
            && !state.help_box
            && !state.settings_box
            && state.dock_anim <= 0.0
            && state.dock_retract <= 0.0;
        sounds.engine(engine_on);
        sounds.reverse_engine(
            state.player.revert_thrusted != 0
                && !state.dock_box
                && !state.shop_box
                && !state.help_box
                && !state.settings_box
                && state.dock_anim <= 0.0
                && state.dock_retract <= 0.0,
        );

        // --- Rendu (toujours actif, même en pause) ---
        // Fenêtré : dessin direct 1:1 (fenêtre = viewport 960×540). Plein
        // écran zoomé : rendu dans la vue virtuelle 960×540 puis étirée (F,
        // 2e mode). Plein écran natif : rendu direct à la définition réelle
        // de l'écran via une caméra zoomée - SANS render target (F, 3e mode).
        match state.view_mode {
            // fenêtré : dessin direct 1:1 à la définition native (960×540),
            // sinon la vue est rendue dans la texture puis étirée (letterbox)
            // - voir `render::window_scaled`
            ViewMode::Windowed => {
                if render::window_scaled() {
                    set_camera(&render::virtual_camera(&render_target));
                } else {
                    set_default_camera();
                }
            }
            ViewMode::Zoomed => set_camera(&render::virtual_camera(&render_target)),
            ViewMode::Native => set_camera(&render::native_camera()),
        }
        clear_background(BLACK);
        // fenêtre modale ouverte (magasin, paramètres, aide, DOCK) : le monde
        // continue de tourner derrière mais l'œil est sur la fenêtre - la
        // densité d'étoiles est réduite (gain GPU, imperceptible)
        let modal_overlay = state.dock_box || state.shop_box || state.help_box || state.settings_box;
        render::draw_stars(&assets, camera, modal_overlay);

        // formes (météores, station…) puis le vaisseau joueur par-dessus - le
        // cosmonaute EVA est retiré de la boucle : il est dessiné **au premier
        // plan**, après le vaisseau (c'est le pilote quand le vaisseau est
        // détruit ; garé, il est cullé)
        let eva = state.eva_cosmonaut as usize;
        for (i, shape) in shapes.iter().enumerate().skip(1) {
            if i == eva {
                continue;
            }
            render::draw_shape(
                &state,
                &assets,
                shape,
                &mut triangles,
                camera,
                &elements,
                state.show_data,
                1.0,
            );
        }
        // le pilote (vaisseau, ou cosmonaute EVA quand il est détruit) guide
        // la mire, le HUD d'accostage et la flamme de poussée
        let pilot = crate::input::pilot_index(&state);
        // fondu enchaîné de la récupération EVA : pendant `eva_crossfade`, le
        // cosmonaute ramené sur l'anneau s'efface (`cosmonaut_fade` 1→0)
        // pendant que le vaisseau reconstruit apparaît au centre avec ses
        // liens (`ship_fade` 0→1). En dehors, tout est opaque (1.0)
        let cross = state.eva_crossfade / EVA_CROSSFADE_DURATION; // 1 → 0
        let ship_fade = (1.0 - cross).clamp(0.0, 1.0) as f32;
        let cosmonaut_fade = if state.eva_crossfade > 0.0 {
            cross.clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        // mire d'accostage au centre de la station : dessinée **sous le
        // vaisseau** (par-dessus l'anneau de la station, avant le joueur) -
        // semi-transparente, rouge → vert selon la vitesse d'approche. Elle
        // **disparaît quand le vaisseau est tenu par les liens** (à quai,
        // animation d'accostage, accosté, rétraction) et **réapparaît quand
        // le vaisseau franchit la limite extérieure de la base** au retour
        // (voir `render::docking_marker_visible`) - en mode cosmonaute EVA
        // elle est toujours visible (il doit rejoindre la base)
        if render::docking_marker_visible(
            &state,
            shapes[pilot].position,
            shapes[STATION_INDEX].position,
            shapes[STATION_INDEX].radius,
        ) {
            render::draw_docking_marker(
                camera,
                &state.world,
                shapes[STATION_INDEX].position,
                shapes[STATION_INDEX].radius,
                shapes[pilot].position,
                shapes[pilot].velocity,
            );
        }
        // traits d'accostage : pendant l'animation (3 s, avant la boîte) ils
        // relient le bord intérieur de la station aux côtés du vaisseau - néon
        // vert, sous le vaisseau (après la mire) ; au départ (CLOSE), ils se
        // rétractent vers le bord (voir `render::draw_docking_line`). Pendant
        // le fondu enchaîné de la récupération EVA, ils apparaissent avec le
        // vaisseau reconstruit (`ship_fade`)
        render::draw_docking_line(
            &state,
            camera,
            &state.world,
            &shapes[STATION_INDEX],
            &shapes[PLAYER_INDEX],
            ship_fade,
        );
        render::draw_shape(
            &state,
            &assets,
            &shapes[PLAYER_INDEX],
            &mut triangles,
            camera,
            &elements,
            state.show_data,
            ship_fade,
        );
        // cosmonaute EVA au **premier plan** (par-dessus le vaisseau et tout
        // le reste du monde) : dessiné **uniquement quand il est éjecté**
        // (`cosmonaut_active`) - jamais de cosmonaute supplémentaire dans le
        // monde quand le vaisseau est intact (garé, il n'est pas affiché).
        // Pendant sa récupération, le cordon orange est dessiné **sous lui**
        // (`draw_eva_recovery_cable`) ; pendant le fondu enchaîné, il s'efface
        // (`cosmonaut_fade`)
        if state.cosmonaut_active && state.eva_cosmonaut >= 0 {
            render::draw_eva_recovery_cable(&state, camera, &state.world, &shapes[eva]);
            render::draw_shape(
                &state,
                &assets,
                &shapes[eva],
                &mut triangles,
                camera,
                &elements,
                state.show_data,
                cosmonaut_fade,
            );
        }

        // effet de poussée : les gaz sortent des propulseurs configurés
        // (`VAISSEAU_THRUSTERS` - ↑ orange à l'arrière, ↓ bleu
        // à l'avant, ← et → jets latéraux pendant les rotations) ; le
        // cosmonaute EVA, lui, n'a qu'un **petit propulseur sur le dos** -
        // une flamme animée, et pas de marche arrière ni de jets latéraux
        // (voir `render::draw_cosmonaut_thruster`)
        if state.player.thrusted != 0 || state.player.revert_thrusted != 0
            || state.player.rotate_left_thrusted != 0 || state.player.rotate_right_thrusted != 0
        {
            if state.cosmonaut_active {
                if state.player.thrusted != 0 {
                    render::draw_cosmonaut_thruster(&shapes[pilot], camera, &state.world);
                }
            } else {
                // gaz d'éjection : le mesh configuré de chaque propulseur
                // (`VAISSEAU_THRUSTERS` - ↑, ↓, ←, →) est affiché **scintillant**
                // seulement quand il tire, sinon rien n'est affiché. Les jets
                // latéraux sont **croisés** : touche ← = propulseur DROITE,
                // touche → = propulseur GAUCHE (la flamme sort du propulseur
                // qui pousse). Repli (liste vide) : gaz classique au centre de
                // rotation (cercles), angles et couleurs par défaut par touche.
                let jets = crate::vaisseau::vaisseau_thrusters();
                const DEFAULT_ANGLE: [f64; 4] = [TAU / 2.0, 0.0, -TAU / 4.0, TAU / 4.0];
                const DEFAULT_COLOR: [u32; 4] =
                    [0xFFFFA000, 0xFF00A0FF, 0xFFFF5AC8, 0xFF39FF88];
                let at = |i: usize| match jets.get(i) {
                    Some((t, p)) => {
                        // le propulseur tire : son mesh (la flamme) scintille -
                        // teinté de la couleur configurée et allongé le long de
                        // la direction d'éjection (repère éditeur → jeu)
                        let tris = crate::vaisseau::thruster_mesh_triangles(t, *p);
                        render::draw_thruster_gas(
                            &shapes[pilot],
                            &tris,
                            *p,
                            -t.ejection_angle_degrees.to_radians(),
                            t.color,
                            camera,
                            &state.world,
                        );
                    }
                    None => render::ejection_flow(
                        &shapes[pilot],
                        shapes[pilot].center,
                        DEFAULT_ANGLE[i],
                        DEFAULT_COLOR[i],
                        camera,
                        &state.world,
                    ),
                };
                if state.player.thrusted != 0 {
                    at(0); // propulseur arrière
                }
                if state.player.revert_thrusted != 0 {
                    at(1); // propulseur avant
                }
                if state.player.rotate_left_thrusted != 0 {
                    at(3); // propulseur DROITE (croisé : rotation gauche)
                }
                if state.player.rotate_right_thrusted != 0 {
                    at(2); // propulseur GAUCHE (croisé : rotation droite)
                }
            }
        }

        // débris
        for g in &garbages {
            render::draw_garbage(g, camera, &state.world);
        }

        render::draw_cargo(&state, &elements);
        // HUD en haut de l'écran : stats + ressources sur une ligne, puis le
        // statut d'accostage à la suite (même ligne - `draw_hud` renvoie la
        // fin de ligne). La distance affichée est celle du pilote (le
        // cosmonaute EVA quand le vaisseau est détruit)
        let hud_end_x = render::draw_hud(&state);
        render::draw_docking_hud(
            &state,
            shapes[pilot].position,
            shapes[STATION_INDEX].position,
            shapes[pilot].velocity,
            hud_end_x,
        );
        // Objectifs DAG (scénarios custom) : panneau dans le coin supérieur droit
        render::draw_objectives_hud(&state);
        // consommables actifs (bouclier temporaire, boost, mines - touches
        // 1/2/3) sous le HUD, et score composite + record en bas à droite
        // (le statut d'accostage garde toute la ligne principale)
        render::draw_consumables_hud(&state);
        render::draw_score_hud(&state);
        // journal de bord (touche L) et briefing pré-partie (scénarios
        // custom - lancé au démarrage de la partie, fermé par ENTRÉE/ÉCHAP)
        render::draw_log_box(&state);
        if state.briefing_box {
            render::draw_briefing_box(&state);
        }

        // Écran PAUSE (touche P) : le monde est gelé - l'overlay rend l'état
        // visible (assombrissement + bandeau) tant qu'aucune fenêtre ne
        // recouvre l'écran (les boîtes, dessinées plus bas, passent par-dessus)
        if state.paused && !state.dock_box && !state.shop_box && !state.help_box && !state.settings_box {
            render::draw_pause_overlay();
        }

        // affichages de debug (touches D et I)
        if state.show_info {
            render::draw_info(&state, &shapes, &triangles, &garbages, &elements);
        }
        render::draw_message(&mut state);

        // interface tactile (joystick virtuel bas-gauche + bouton de tir
        // bas-droite, `touch.rs`) : affichée pendant le jeu quand le réglage
        // TOUCH UI est coché - masquée quand une boîte (accostage, magasin,
        // aide, paramétrage) recouvre l'écran ou en fin de partie (le monde
        // est gelé, les contrôles ne servent plus)
        if state.touch_ui
            && !state.dock_box
            && !state.shop_box
            && !state.help_box
            && !state.settings_box
            && !state.game_over
        {
            crate::touch::draw();
        }

        // boîte de choix DOCK STATION (accostage), magasin de la station
        // (bouton SHOP), fenêtre d'aide (touche S) et écran de
        // paramétrage (touche O) par-dessus le jeu
        if state.dock_box {
            render::draw_choice_box(&state);
        }
        if state.shop_box {
            // aperçu du vaisseau équipé dans l'onglet ÉQUIPEMENT : le mesh
            // réel du vaisseau (et des armes survolées) est redessiné à
            // l'échelle dans la fenêtre ; `elements` porte la soute
            // (ingrédients de l'onglet FABRICATION)
            render::draw_shop_box(&state, &shapes, &triangles, &elements);
        }
        if state.help_box {
            render::draw_help_box();
        }
        if state.settings_box {
            render::draw_settings_box(&state, &sounds);
        }

        // la vue virtuelle est étirée dans la fenêtre en plein écran zoomé
        // et en fenêtré agrandi (définition choisie) - fenêtré natif : dessin
        // direct ; natif : rendu direct, rien à blitter
        if state.view_mode == ViewMode::Zoomed
            || (state.view_mode == ViewMode::Windowed && render::window_scaled())
        {
            render::draw_zoomed(&render_target);
        }

        next_frame().await
    }
}
