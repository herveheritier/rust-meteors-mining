//! Écran titre (ex `titleLoop` de `meteorsMining.bas`).
//!
//! Fond d'étoiles (caméra qui dérive), bannière « METEORS MINING » en
//! couleurs arc-en-ciel rotatives, et trois invites. Se termine sur une
//! touche (sauf F, qui bascule le plein écran).

use macroquad::prelude::*;

use crate::config::{ATTEMPT_FPS, VIEWPORT_WIDTH};
use crate::geom::Point;
use crate::render::{argb_to_color, cycle_view_mode, draw_stars, draw_zoomed, native_camera, virtual_camera};
use crate::state::ViewMode;
use crate::state::GameState;
use std::time::Duration;

/// Bannière « METEORS MINING » en ASCII art (8 lignes × 125 colonnes,
/// extraite telle quelle de l'original — les caractères `[]`/`[I]` dessinent
/// les lettres en blocs).
pub const BANNER: [&str; 8] = [
    "     []    []                                                    []    [] []            []                       ",
    "     [I]  [I]           []                   []                  [I]  [I]       []            []          []     ",
    "     [][][][]  [III]  [IIII]  [III]   [III]   [III]   [III]      [][][][] []     [III]  []     [III]   [II]      ",
    "     [] [] [] []   []   []   []   I] []   [] []   [] []          [] [] [] []     []  [] []     []  [] []  []     ",
    "     []    [] [IIII]    []   [IIII]  []   [] []       [III]      []    [] []     []  [] []     []  []  [III]     ",
    "     []    [] []        []   []      []   [] []           []     []    []  []    []  []  []    []  []     []     ",
    "     []    []  [III]     [I]  [III]   [III]  []       [III]      []    []   [II] []  []   [II] []  [] []  []     ",
    "                                                                                                       [II]      ",
];

/// Couleurs arc-en-ciel (ex `nextRainbowColor` de l'original) : HSV → RGB
/// avec s=1, v=1, hue en degrés.
fn rainbow(hue: f64) -> u32 {
    let hue = hue.rem_euclid(360.0);
    let sector = hue / 60.0;
    let i = sector.floor() as i32;
    let f = sector - i as f64;
    let q = 1.0 - f;
    let t = f;
    let (r, g, b) = match i {
        0 => (255, (255.0 * t) as i32, 0),
        1 => ((255.0 * q) as i32, 255, 0),
        2 => (0, 255, (255.0 * t) as i32),
        3 => (0, (255.0 * q) as i32, 255),
        4 => ((255.0 * t) as i32, 0, 255),
        _ => (255, 0, (255.0 * q) as i32),
    };
    (0xFF << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// Écran titre : boucle jusqu'à une touche (autre que F), ex `titleLoop`.
pub async fn title_loop(state: &mut GameState, assets: &crate::render::Assets, rt: &RenderTarget) {
    const COLOR_STEPS: f64 = 48.0;
    const COLOR_SPEED: f64 = 0.3;
    let mut color_step = 0.0;
    let mut camera = Point::new(0.0, 0.0);

    let banner_rows = BANNER.len();
    let banner_cols = BANNER[0].len();
    let mut banner_colors = vec![0u32; banner_cols];

    // pacing (ex `_limit ATTEMPT_FPS`), filet anti-fuite comme l'original.
    const LIMIT_FPS: bool = true;
    let target_frame = 1.0 / ATTEMPT_FPS as f64;
    let mut last_frame = get_time();

    loop {
        if LIMIT_FPS {
            let elapsed = get_time() - last_frame;
            if elapsed < target_frame {
                std::thread::sleep(Duration::from_secs_f64(target_frame - elapsed));
            }
            last_frame = get_time();
        }

        // touche F : plein écran ; toute autre touche : lancement
        let mut key: Option<KeyCode> = None;
        for k in [
            KeyCode::F,
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::Space,
            KeyCode::A,
            KeyCode::B,
            KeyCode::C,
            KeyCode::D,
            KeyCode::E,
            KeyCode::G,
            KeyCode::H,
            KeyCode::I,
            KeyCode::K,
            KeyCode::L,
            KeyCode::M,
            KeyCode::N,
            KeyCode::O,
            KeyCode::P,
            KeyCode::Q,
            KeyCode::R,
            KeyCode::S,
            KeyCode::T,
            KeyCode::U,
            KeyCode::V,
            KeyCode::W,
            KeyCode::X,
            KeyCode::Y,
            KeyCode::Z,
        ] {
            if is_key_pressed(k) {
                key = Some(k);
                break;
            }
        }
        if let Some(k) = key {
            if k == KeyCode::F {
                // NB : on ne fait PAS `continue` ici — `keys_pressed` n'est vidé
                // qu'à `end_frame`, atteint seulement quand la coroutine rend la
                // main à `next_frame` : un `continue` re-testerait F à l'infini
                // (gel avec cadre figé, boucle sans rendu). On cède une frame
                // (le keypress est consommé), comme l'original qui relit
                // `inkey$` (consommant) à chaque itération.
                cycle_view_mode(state);
                next_frame().await;
                continue;
            }
            break;
        }

        // rendu selon le mode d'affichage : fenêtré → direct ; plein écran
        // zoomé → vue virtuelle 960×540 puis étirée ; plein écran natif →
        // rendu direct à la définition réelle de l'écran (sans buffer)
        match state.view_mode {
            ViewMode::Windowed => set_default_camera(),
            ViewMode::Zoomed => set_camera(&virtual_camera(rt)),
            ViewMode::Native => set_camera(&native_camera()),
        }

        // fond noir + étoiles (caméra qui dérive vers le bas, ex titleLoop)
        clear_background(BLACK);
        draw_stars(assets, camera);

        // caméra qui descend
        camera.y += 1.0;
        camera.normalize_world(&state.world);

        // rotation des couleurs (ex titleLoop : nouvelle couleur à droite,
        // décalage de tout le tableau)
        let h = color_step * 360.0 / COLOR_STEPS;
        color_step += COLOR_SPEED;
        if color_step >= COLOR_STEPS {
            color_step -= COLOR_STEPS;
        }
        banner_colors[banner_cols - 1] = rainbow(h);
        for i in 0..banner_cols - 1 {
            banner_colors.swap(i, i + 1);
        }

        // bannière : un caractère par colonne, chaque colonne colorée
        // (ex titleLoop : `_printstring` de chaque caractère)
        for j in 0..banner_rows {
            for i in 0..banner_cols {
                let ch = BANNER[j].as_bytes()[i] as char;
                let x = (VIEWPORT_WIDTH as f64 / banner_cols as f64) * i as f64;
                let y = 10.0 * (8.0 + j as f64);
                let color = banner_colors[banner_cols - 1 - i];
                draw_text(&ch.to_string(), x as f32, y as f32, 8.0, argb_to_color(color));
            }
        }

        // invites (ex titleLoop)
        let infos = [
            "[ F : window / zoomed / native fullscreen ]",
            "[ ESC to quit ]",
            "[ Hit other key to launch ]",
        ];
        let mut y = 10.0 * (8.0 + banner_rows as f64) + 20.0;
        for line in infos {
            let w = measure_text(line, None, 16, 1.0).width;
            draw_text(line, (VIEWPORT_WIDTH as f32 - w) / 2.0, y as f32, 16.0, WHITE);
            y += 20.0;
        }

        // étirement de la vue virtuelle uniquement en plein écran zoomé
        if state.view_mode == ViewMode::Zoomed {
            draw_zoomed(rt);
        }

        next_frame().await
    }
}
