//! Écran titre (ex `titleLoop` de `meteorsMining.bas`).
//!
//! Fond d'étoiles (caméra qui dérive), bannière « METEORS MINING » en
//! couleurs arc-en-ciel rotatives, et cinq invites (la 2e détaille les règles
//! du scénario courant — voir `scenario::scenario_rules`). Se termine sur une
//! touche (sauf F, qui bascule le plein écran ; O, qui ouvre l'écran de
//! paramétrage ; et N/B/1-3, qui changent de scénario — la ligne des règles
//! clignote alors brièvement dans la couleur du scénario pour attirer l'œil).

use macroquad::prelude::*;

use crate::audio::Sounds;
use crate::config::{ATTEMPT_FPS, VIEWPORT_WIDTH};
use crate::game;
use crate::geom::Point;
use crate::render::{
    argb_to_color, cycle_view_mode, draw_settings_box, draw_stars, draw_zoomed, native_camera,
    virtual_camera, window_scaled,
};
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

/// Durée (s) du flash de la ligne des règles après un changement de scénario
/// (N/B/1-3) : toute la ligne passe dans la couleur du scénario et clignote
/// pour attirer l'œil.
const RULES_FLASH_DURATION: f64 = 1.2;

/// Fréquence (Hz) du clignotement pendant le flash de la ligne des règles.
const RULES_FLASH_HZ: f64 = 6.0;

/// Dessine une ligne centrée (invite de l'écran titre), en blanc.
fn draw_centered_line(line: &str, y: f32) {
    let w = measure_text(line, None, 16, 1.0).width;
    draw_text(line, (VIEWPORT_WIDTH as f32 - w) / 2.0, y, 16.0, WHITE);
}

/// Dessine une ligne de segments centrée (règles ou sauvegarde de l'écran
/// titre, voir `scenario::scenario_rules` / `scenario::save_summary_segments`)
/// : chaque segment dans sa couleur — `color: None` = blanc, `Some(argb)` =
/// couleur du scénario. `flash_color` (pendant le flash après un changement
/// de scénario) remplace toutes les couleurs par celle du scénario.
fn draw_segments_line(
    segments: &[crate::scenario::RuleSegment],
    y: f32,
    flash_color: Option<u32>,
) {
    let total_w: f32 = segments
        .iter()
        .map(|s| measure_text(&s.text, None, 16, 1.0).width)
        .sum();
    let mut x = (VIEWPORT_WIDTH as f32 - total_w) / 2.0;
    for seg in segments {
        let color = flash_color.or(seg.color).map(argb_to_color).unwrap_or(WHITE);
        draw_text(&seg.text, x, y, 16.0, color);
        x += measure_text(&seg.text, None, 16, 1.0).width;
    }
}

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

/// Écran titre : boucle jusqu'à une touche (autre que F, O ou N/B/1-3), ex
/// `titleLoop`. `sounds` sert à l'écran de paramétrage (touche O), accessible
/// depuis le titre — musique et volume y sont réglables ; N/B ou 1-3 changent
/// de scénario (jeu libre ↔ Progression ↔ Survival). Renvoie `true` si le
/// bouton RESTART de l'écran de paramétrage a été cliqué (le jeu doit se
/// relancer).
pub async fn title_loop(
    state: &mut GameState,
    assets: &crate::render::Assets,
    rt: &RenderTarget,
    sounds: &mut Sounds,
) -> bool {
    const COLOR_STEPS: f64 = 48.0;
    const COLOR_SPEED: f64 = 0.3;
    let mut color_step = 0.0;
    let mut camera = Point::new(0.0, 0.0);
    // fin (temps absolu) du flash de la ligne des règles ; 0 = aucun flash
    let mut flash_until: f64 = 0.0;

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

        // touche F : plein écran ; O : écran de paramétrage ; N/B/1-3 :
        // scénario ; toute autre touche : lancement
        let mut key: Option<KeyCode> = None;
        for k in [
            KeyCode::F,
            KeyCode::Escape,
            KeyCode::Enter,
            KeyCode::Space,
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
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
            if k == KeyCode::O {
                // ouvre l'écran de paramétrage (mêmes initialisations que la
                // touche O du jeu) ; sous-boucle d'input + rendu jusqu'à la
                // fermeture (CLOSE ou ESC — consommé ici, ne quitte pas le
                // jeu), puis retour à l'écran titre. Un clic sur RESTART
                // (relance demandée) sort immédiatement du titre.
                state.settings_previous_mode = state.moving_mode;
                state.settings_focus = state.moving_mode;
                state.settings_box = true;
                while state.settings_box {
                    if LIMIT_FPS {
                        let elapsed = get_time() - last_frame;
                        if elapsed < target_frame {
                            std::thread::sleep(Duration::from_secs_f64(target_frame - elapsed));
                        }
                        last_frame = get_time();
                    }
                    if game::handle_settings_input(state, Some(sounds)) {
                        // ferme l'écran : si la relance échoue (retour de
                        // `main`), la partie démarre sans l'écran ouvert
                        state.settings_box = false;
                        return true;
                    }
                    draw_frame(
                        state,
                        assets,
                        rt,
                        camera,
                        &banner_colors,
                        sounds,
                        get_time() < flash_until,
                    );
                    next_frame().await;
                }
                continue;
            }
            // après toute sélection de scénario : règles de départ appliquées
            // (cycle/select), progression enregistrée restaurée puis nouveau
            // scénario persisté, et flash de la ligne des règles (1,2 s —
            // voir `draw_frame`) ; comme F, on cède une frame (keypress
            // consommé)
            let mut restore = |state: &mut GameState| {
                crate::scenario::load_progression(state);
                let _ = crate::scenario::save_progression(state);
                flash_until = get_time() + RULES_FLASH_DURATION;
            };
            if k == KeyCode::N {
                // bascule de scénario : jeu libre → Progression → Survival
                crate::scenario::cycle_scenario(state);
                restore(state);
                next_frame().await;
                continue;
            }
            if k == KeyCode::B {
                // bascule au scénario précédent (inverse de N)
                crate::scenario::cycle_scenario_back(state);
                restore(state);
                next_frame().await;
                continue;
            }
            if k == KeyCode::Key1 || k == KeyCode::Key2 || k == KeyCode::Key3 {
                // sélection directe : 1 = jeu libre, 2 = Progression,
                // 3 = Survival
                let id = match k {
                    KeyCode::Key1 => crate::scenario::ScenarioId::FreePlay,
                    KeyCode::Key2 => crate::scenario::ScenarioId::Progression,
                    _ => crate::scenario::ScenarioId::Survival,
                };
                crate::scenario::select_scenario(state, id);
                restore(state);
                next_frame().await;
                continue;
            }
            break;
        }

        // caméra qui descend + rotation des couleurs (ex titleLoop : nouvelle
        // couleur à droite, décalage de tout le tableau) — avant le rendu,
        // comme l'original qui dérive la caméra après les étoiles
        camera.y += 1.0;
        camera.normalize_world(&state.world);
        let h = color_step * 360.0 / COLOR_STEPS;
        color_step += COLOR_SPEED;
        if color_step >= COLOR_STEPS {
            color_step -= COLOR_STEPS;
        }
        banner_colors[banner_cols - 1] = rainbow(h);
        for i in 0..banner_cols - 1 {
            banner_colors.swap(i, i + 1);
        }

        draw_frame(
            state,
            assets,
            rt,
            camera,
            &banner_colors,
            sounds,
            get_time() < flash_until,
        );
        next_frame().await
    }

    // le `break` ci-dessus quitte la boucle (lancement de la partie)
    false
}

/// Dessine une frame de l'écran titre : caméra selon le mode d'affichage,
/// fond d'étoiles, bannière arc-en-ciel, invites, l'écran de paramétrage s'il
/// est ouvert (touche O) et l'étirement de la vue virtuelle le cas échéant.
/// `flash_rules` (vrai juste après un changement de scénario N/B/1-3, pendant
/// `RULES_FLASH_DURATION`) fait clignoter toute la ligne des règles dans la
/// couleur du scénario pour attirer l'œil sur les valeurs qui viennent de
/// changer.
fn draw_frame(
    state: &GameState,
    assets: &crate::render::Assets,
    rt: &RenderTarget,
    camera: Point,
    banner_colors: &[u32],
    sounds: &Sounds,
    flash_rules: bool,
) {
    // rendu selon le mode d'affichage : fenêtré → direct (ou étiré si la
    // fenêtre est plus grande que 960×540) ; plein écran zoomé → vue virtuelle
    // 960×540 puis étirée ; plein écran natif → rendu direct à la définition
    // réelle de l'écran (sans buffer)
    match state.view_mode {
        ViewMode::Windowed => {
            if window_scaled() {
                set_camera(&virtual_camera(rt));
            } else {
                set_default_camera();
            }
        }
        ViewMode::Zoomed => set_camera(&virtual_camera(rt)),
        ViewMode::Native => set_camera(&native_camera()),
    }

    // fond noir + étoiles (caméra qui dérive vers le bas, ex titleLoop)
    clear_background(BLACK);
    draw_stars(assets, camera);

    // bannière : un caractère par colonne, chaque colonne colorée
    // (ex titleLoop : `_printstring` de chaque caractère)
    let banner_rows = BANNER.len();
    let banner_cols = BANNER[0].len();
    for j in 0..banner_rows {
        for i in 0..banner_cols {
            let ch = BANNER[j].as_bytes()[i] as char;
            let x = (VIEWPORT_WIDTH as f64 / banner_cols as f64) * i as f64;
            let y = 10.0 * (8.0 + j as f64);
            let color = banner_colors[banner_cols - 1 - i];
            draw_text(&ch.to_string(), x as f32, y as f32, 8.0, argb_to_color(color));
        }
    }

    // invites (ex titleLoop) — la 1re ligne affiche le scénario courant, la
    // 2e ses règles (valeurs en surbrillance — voir
    // `scenario::scenario_rules`) et la 3e la progression enregistrée de ce
    // scénario, valeurs en surbrillance aussi (minerais, modes, réputation,
    // rang, vies, bouclier — voir `scenario::save_summary_segments`)
    let scenario = crate::scenario::scenario(state.scenario);
    let mut y = 10.0 * (8.0 + banner_rows as f64) + 20.0;
    draw_centered_line(
        &format!("[ SCENARIO : {} — {} ]", scenario.name, scenario.description),
        y as f32,
    );
    y += 20.0;

    // ligne des règles : segments alignés, valeurs colorées (couleur du
    // scénario — voir `scenario::scenario_rules`) — les coûts/vies/bouclier/
    // dégâts/rangs sautent aux yeux quand on change de scénario (N/B/1-3).
    // Juste après un changement, tout le texte clignote dans la couleur du
    // scénario (flash) pour attirer l'œil sur ce qui vient de changer.
    let mut segments = vec![crate::scenario::RuleSegment {
        text: "[ RULES : ".to_string(),
        color: None,
    }];
    segments.extend(crate::scenario::scenario_rules(state.scenario));
    segments.push(crate::scenario::RuleSegment {
        text: " ]".to_string(),
        color: None,
    });
    // phase du clignotement (parité de l'horloge) : alternance ≈ 3 cycles/s —
    // pendant le flash, toute la ligne prend la couleur du scénario
    let blink_on = flash_rules && ((get_time() * RULES_FLASH_HZ) as i64 % 2 == 0);
    let flash_color = if blink_on {
        Some(scenario.rules_color)
    } else {
        None
    };
    draw_segments_line(&segments, y as f32, flash_color);
    y += 20.0;

    // ligne de la progression enregistrée : mêmes segments, valeurs (minerais,
    // modes, réputation, rang, vies, bouclier) dans la couleur du scénario —
    // voir `scenario::save_summary_segments`
    let mut save_segments = vec![crate::scenario::RuleSegment {
        text: "[ SAVE : ".to_string(),
        color: None,
    }];
    save_segments.extend(crate::scenario::save_summary_segments(state));
    save_segments.push(crate::scenario::RuleSegment {
        text: " ]".to_string(),
        color: None,
    });
    draw_segments_line(&save_segments, y as f32, None);
    y += 20.0;
    for line in [
        "[ N/B : scenario (1-3 : pick)  |  F : window / zoomed / native  |  O : settings ]",
        "[ ESC to quit ]",
        "[ Hit other key to launch ]",
    ] {
        draw_centered_line(line, y as f32);
        y += 20.0;
    }

    // écran de paramétrage par-dessus (touche O)
    if state.settings_box {
        draw_settings_box(state, sounds);
    }

    // étirement de la vue virtuelle en plein écran zoomé et en fenêtré
    // agrandi (définition choisie)
    if state.view_mode == ViewMode::Zoomed
        || (state.view_mode == ViewMode::Windowed && window_scaled())
    {
        draw_zoomed(rt);
    }
}
