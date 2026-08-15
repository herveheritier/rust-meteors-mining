//! Portage Rust de « Meteors Mining ».
//!
//! - **Phase 0** (faite) : fenêtre 960×540, boucle macroquad.
//! - **Phase 1** (faite) : modèle de données — `config.rs` (constantes),
//!   `geom.rs` (Point/World/Segment/Triangle), `shape.rs` (Shape + meshes),
//!   `garbage.rs` (débris), `state.rs` (état du jeu), `generate.rs`
//!   (génération procédurale + `prepare`).
//! - **Phase 2** (faite) : rendu — `render.rs` (assets, étoiles
//!   précalculées, triangles texturés, formes, caméra centrée joueur, HUD,
//!   messages). Plein écran = **zoom** : vue 960×540 rendue dans une texture
//!   puis étirée (F → fenêtre 1920×1080, même contenu juste plus grand —
//!   voir `docs/PORTAGE.md` §4.1).
//! - **Phases 3-4** (jalons M2 à M5 faits) : boucle de jeu — `game.rs`
//!   (input, 3 modes de déplacement, pause, plein écran, météores : G + auto,
//!   collisions SAT + élastique, débris, messages, tirs, gemmes, accostage,
//!   aide S, debug D/I), `title.rs` (écran titre).

mod config;
mod game;
mod garbage;
mod generate;
mod geom;
mod render;
mod shape;
mod state;
mod title;

use macroquad::prelude::*;
use ::rand::SeedableRng;
use ::rand_chacha::ChaCha12Rng;

use crate::config::{
    ATTEMPT_FPS, FULLSCREEN_HEIGHT, FULLSCREEN_WIDTH, PLAYER_INDEX, VIEWPORT_HEIGHT, VIEWPORT_WIDTH,
};
use crate::geom::Point;
use crate::state::GameState;
use std::f64::consts::TAU;
use std::time::Duration;

fn window_conf() -> Conf {
    Conf {
        window_title: "Meteors Mining (Rust port)".to_owned(),
        window_width: VIEWPORT_WIDTH as i32,
        window_height: VIEWPORT_HEIGHT as i32,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    // ─── Phase 1 : modèle de données ────────────────────────────────────────
    // L'état initial (monde torique, joueur, étoiles, station) est construit
    // par `prepare`, exactement comme le `prepare` du jeu QB64.
    let mut state = GameState::new();
    let mut shapes = Vec::new();
    let mut triangles = Vec::new();
    let mut stars: Vec<Point> = Vec::new();
    let mut elements = Vec::new();
    let mut rng = ChaCha12Rng::from_entropy();
    generate::prepare(&mut state, &mut shapes, &mut triangles, &mut stars, &mut elements, &mut rng);

    info!(
        "Phase 1 OK : {} formes, {} triangles, {} étoiles, {} éléments",
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

    // ─── Zoom plein écran (touche F) ────────────────────────────────────────
    // La vue 960×540 est rendue dans une texture puis affichée étirée dans la
    // fenêtre (ex `letterbox.rs` de macroquad) : en fenêtré elle est affichée
    // 1:1, en « plein écran » la fenêtre est agrandie et le contenu zoomé —
    // même contenu, juste plus grand (le vrai plein écran EWMH n'est pas
    // fiable sur tous les affichages, voir `docs/PORTAGE.md` §7).
    let render_target = render_target(VIEWPORT_WIDTH as u32, VIEWPORT_HEIGHT as u32);
    render_target.texture.set_filter(FilterMode::Linear);

    // ─── Écran titre (jalon M5, ex `titleLoop`) ─────────────────────────────
    title::title_loop(&mut state, &assets, &render_target).await;

    // le keypress qui a lancé la partie (ex F du titre) est encore dans la
    // file d'input : sans ça, la première frame de jeu le verrait (ex F →
    // `state.fullscreen` re-basculé) et annulerait le redimensionnement.
    clear_input_queue();

    // ─── Boucle principale (Phase 3 / jalons M2-M5) ─────────────────────────
    // Limitation de boucle (ex `_limit ATTEMPT_FPS` = 600 de l'original) :
    // la physique est en `dt` (indépendante du FPS), mais un pas de temps
    // stable améliore la régularité (interpolation du centre, compteurs de
    // poussée, comptage des frames) et évite de chauffer le GPU inutilement.
    // macroquad 0.4 n'offre ni `set_target_fps` ni vsync → pacing manuel.
    let target_frame = 1.0 / ATTEMPT_FPS as f64;
    let mut last_frame = get_time();
    let mut pending_fullscreen = state.fullscreen;
    loop {
        let elapsed = get_time() - last_frame;
        if elapsed < target_frame {
            std::thread::sleep(Duration::from_secs_f64(target_frame - elapsed));
        }
        last_frame = get_time();

        // filet de sécurité : ré-applique la taille plein écran si le titre a
        // basculé (touche F) mais que le redimensionnement n'a pas abouti
        // avant le lancement (voir `docs/PORTAGE.md` §7)
        if pending_fullscreen {
            pending_fullscreen = false;
            request_new_screen_size(FULLSCREEN_WIDTH as f32, FULLSCREEN_HEIGHT as f32);
        }

        // Input + physique + collisions (mouvement, météores, pause, plein
        // écran) — M2/M3. La caméra est calculée par update (comme l'original,
        // après la résolution des collisions).
        let dt = get_frame_time() as f64;
        let (action, camera) = game::update(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut rng,
            dt,
        );
        if action == game::Action::Quit {
            break;
        }

        // --- Rendu (toujours actif, même en pause) ---
        // dans la vue virtuelle 960×540, puis zoom plein écran (F)
        set_camera(&render::virtual_camera(&render_target));
        clear_background(BLACK);
        render::draw_stars(&assets, camera);

        // formes (météores, station…) puis le vaisseau joueur par-dessus
        for i in 1..shapes.len() {
            render::draw_shape(
                &state,
                &assets,
                &shapes[i],
                &mut triangles,
                camera,
                &elements,
                state.show_data,
            );
        }
        render::draw_shape(
            &state,
            &assets,
            &shapes[PLAYER_INDEX],
            &mut triangles,
            camera,
            &elements,
            state.show_data,
        );

        // effet de poussée (3 cercles orange en avant, bleu en recul)
        if state.player.thrusted != 0 {
            render::ejection_flow(&shapes[PLAYER_INDEX], TAU / 2.0, 0xFFFFA000, camera, &state.world);
        }
        if state.player.revert_thrusted != 0 {
            render::ejection_flow(&shapes[PLAYER_INDEX], 0.0, 0xFF00A0FF, camera, &state.world);
        }

        // débris
        for g in &garbages {
            render::draw_garbage(g, camera, &state.world);
        }

        render::draw_cargo(&state, &elements);
        render::draw_hud(&state);

        // affichages de debug (touches D et I)
        if state.show_info {
            render::draw_info(&state, &shapes, &triangles, &garbages, &elements);
        }
        render::draw_message(&mut state);

        // boîte de choix DOCK STATION (accostage) et fenêtre d'aide (touche S)
        // par-dessus le jeu
        if state.dock_box {
            render::draw_choice_box();
        }
        if state.help_box {
            render::draw_help_box();
        }

        // la vue virtuelle est affichée dans la fenêtre (1:1 en fenêtré,
        // zoomée en plein écran)
        render::draw_zoomed(&render_target);

        next_frame().await
    }
}
