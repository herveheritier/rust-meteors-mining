//! Écran titre (ex `titleLoop` de `meteorsMining.bas`).
//!
//! Fond d'étoiles (caméra qui dérive), bannière « METEORS MINING » en
//! couleurs arc-en-ciel rotatives, et cinq invites (la 2e détaille les règles
//! du scénario courant - voir `scenario::scenario_rules`). Se termine sur une
//! touche (sauf F, qui bascule le plein écran ; O, qui ouvre l'écran de
//! paramétrage ; et N/B/1-3, qui changent de scénario - la ligne des règles
//! clignote alors brièvement dans la couleur du scénario pour attirer l'œil).
//! Une rangée de boutons souris en bas de l'écran reproduit ces mêmes actions
//! (`title_mouse_click` / `draw_title_buttons`) : chaque bouton est
//! l'équivalent d'une touche, et la boîte de choix au lancement d'un scénario
//! sauvegardé a aussi ses boutons (poursuivre / repartir / annuler).

use macroquad::prelude::*;

use crate::audio::Sounds;
use crate::config::{ATTEMPT_FPS, VIEWPORT_HEIGHT, VIEWPORT_WIDTH};
use crate::font::{draw_text, measure_text};
use crate::geom::Point;
use crate::render::{
    argb_to_color, cycle_view_mode, draw_settings_box, draw_stars, draw_zoomed, mouse_to_game,
    native_camera, persist_window_geometry, virtual_camera, window_scaled,
};
use crate::state::ViewMode;
use crate::state::GameState;

/// Bannière « METEORS MINING » en ASCII art (8 lignes × 125 colonnes,
/// extraite telle quelle de l'original - les caractères `[]`/`[I]` dessinent
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

/// Pas vertical (px) d'une ligne de texte de l'écran titre.
const TITLE_LINE_H: f64 = 20.0;
/// Taille de police (px) des lignes de texte de l'écran titre.
const TITLE_FONT: u16 = 16;
/// Largeur maximale (px) d'une ligne de texte avant repli : le viewport moins
/// une petite marge de chaque côté (au-delà, on insère un saut de ligne).
const TITLE_TEXT_W: f32 = (VIEWPORT_WIDTH - 8.0) as f32;

/// Découpe un mot en morceaux dont chacun tient dans `max_w` (px) à la taille
/// `size` - repli d'un **mot seul** trop large (ex une chaîne sans espace).
fn chunk_word(word: &str, size: u16, max_w: f32) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for c in word.chars() {
        let mut candidate = cur.clone();
        candidate.push(c);
        if !cur.is_empty() && measure_text(&candidate, None, size, 1.0).width > max_w {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// Un mot de l'écran titre : son texte, la couleur du segment (`RuleSegment`)
/// dont il provient (`None` = blanc) et s'il doit être précédé d'un espace
/// (`false` pour un morceau d'un mot trop long découpé - collé au précédent).
type TitleWord = (String, Option<u32>, bool);

/// Empile des mots en lignes de largeur mesurée ≤ `max_w` (px) à la taille
/// `size` (`space_w` = largeur d'un espace). Une ligne est coupée quand le
/// mot suivant ne tient plus. Renvoie les lignes (leurs mots).
fn pack_words(
    words: Vec<TitleWord>,
    size: u16,
    space_w: f32,
    max_w: f32,
) -> Vec<Vec<TitleWord>> {
    let mut lines: Vec<Vec<TitleWord>> = Vec::new();
    let mut line: Vec<TitleWord> = Vec::new();
    let mut line_w = 0.0f32;
    for (w, col, space_before) in words {
        let w_w = measure_text(&w, None, size, 1.0).width;
        let gap = if !line.is_empty() && space_before {
            space_w
        } else {
            0.0
        };
        if !line.is_empty() && line_w + gap + w_w > max_w {
            lines.push(std::mem::take(&mut line));
            line_w = 0.0;
        }
        line_w += if line.is_empty() {
            w_w
        } else if space_before {
            space_w + w_w
        } else {
            w_w
        };
        line.push((w, col, space_before));
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Repli d'un texte en lignes de largeur ≤ `max_w` : coupe aux espaces (et
/// par caractère pour un mot trop large). Les lignes renvoyées s'affichent
/// centrées (invite, scénario, raccourcis…).
fn wrap_text(text: &str, size: u16, max_w: f32) -> Vec<String> {
    let space_w = measure_text(" ", None, size, 1.0).width;
    pack_words(
        words_from(text, size, max_w),
        size,
        space_w,
        max_w,
    )
    .into_iter()
    .map(|line| {
        let mut s = String::new();
        for (w, _col, space_before) in line {
            if !s.is_empty() && space_before {
                s.push(' ');
            }
            s.push_str(&w);
        }
        s
    })
    .collect()
}

/// Mots (`TitleWord`) d'un texte : découpe aux espaces et, pour un mot trop
/// large, par caractère - couleur `None` (blanc).
fn words_from(text: &str, size: u16, max_w: f32) -> Vec<TitleWord> {
    let mut tokens = Vec::new();
    for w in text.split_whitespace() {
        let mut first = true;
        for chunk in chunk_word(w, size, max_w) {
            tokens.push((chunk, None, first));
            first = false;
        }
    }
    tokens
}

/// Repli d'une ligne de segments (règles / sauvegarde) en lignes de largeur
/// ≤ `max_w`, en conservant la **couleur** de chaque mot (`RuleSegment::color`).
fn wrap_segments(
    segments: &[crate::scenario::RuleSegment],
    size: u16,
    max_w: f32,
) -> Vec<Vec<TitleWord>> {
    let space_w = measure_text(" ", None, size, 1.0).width;
    let mut tokens = Vec::new();
    for seg in segments {
        let mut first = true;
        for w in seg.text.split_whitespace() {
            for chunk in chunk_word(w, size, max_w) {
                tokens.push((chunk, seg.color, first));
                first = false;
            }
        }
    }
    pack_words(tokens, size, space_w, max_w)
}

/// Dessine un texte **replié** s'il dépasse la largeur de l'écran (saut de
/// ligne aux espaces), chaque ligne centrée, en blanc. Renvoie le nombre de
/// lignes dessinées (pour avancer `y`).
fn draw_centered_line(line: &str, y: f32) -> usize {
    let size = TITLE_FONT;
    let lines = wrap_text(line, size, TITLE_TEXT_W);
    for (i, l) in lines.iter().enumerate() {
        let w = measure_text(l, None, size, 1.0).width;
        draw_text(
            l,
            (VIEWPORT_WIDTH as f32 - w) / 2.0,
            y + i as f32 * TITLE_LINE_H as f32,
            size as f32,
            WHITE,
        );
    }
    lines.len()
}

/// Dessine une ligne de segments centrée (règles ou sauvegarde de l'écran
/// titre, voir `scenario::scenario_rules` / `scenario::save_summary_segments`)
/// : chaque segment dans sa couleur - `color: None` = blanc, `Some(argb)` =
/// couleur du scénario. `flash_color` (pendant le flash après un changement
/// de scénario) remplace toutes les couleurs par celle du scénario. Le texte
/// est **replié** s'il dépasse la largeur de l'écran (saut de ligne aux
/// espaces, couleurs conservées). Renvoie le nombre de lignes dessinées.
fn draw_segments_line(
    segments: &[crate::scenario::RuleSegment],
    y: f32,
    flash_color: Option<u32>,
) -> usize {
    let size = TITLE_FONT;
    let space_w = measure_text(" ", None, size, 1.0).width;
    let lines = wrap_segments(segments, size, TITLE_TEXT_W);
    for (li, words) in lines.iter().enumerate() {
        // largeur totale (espaces entre mots comptés, sauf morceau collé)
        let total: f32 = words.iter().enumerate().fold(0.0, |acc, (i, (w, _, sb))| {
            acc + measure_text(w, None, size, 1.0).width
                + if i > 0 && *sb { space_w } else { 0.0 }
        });
        let mut x = (VIEWPORT_WIDTH as f32 - total) / 2.0;
        let ly = y + li as f32 * TITLE_LINE_H as f32;
        for (i, (w, col, space_before)) in words.iter().enumerate() {
            if i > 0 && *space_before {
                x += space_w;
            }
            let color = flash_color.or(*col).map(argb_to_color).unwrap_or(WHITE);
            draw_text(w, x, ly, size as f32, color);
            x += measure_text(w, None, size, 1.0).width;
        }
    }
    lines.len()
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

/// Action déclenchable à l'écran titre : soit par une touche du clavier
/// (O, N/B/1-3, toute autre = lancement - ESC et F sont traitées à part en
/// haut de la boucle), soit par un clic sur un bouton souris équivalent (voir
/// `title_mouse_click` / `draw_title_buttons`). `key_action` convertit une
/// touche, `title_mouse_click` fournit la même action via les boutons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleAction {
    /// Toute touche (autre que F/ESC/O/N/B/1-3) ou bouton « Lancer ».
    Launch,
    /// Touche O ou bouton « Réglages » (écran de paramétrage).
    Settings,
    /// Touche N ou bouton « < Scénario » (scénario précédent).
    ScenarioPrev,
    /// Touche B ou bouton « Scénario > » (scénario suivant).
    ScenarioNext,
    /// Touche 1-3 : sélection directe d'un scénario.
    Pick(crate::scenario::ScenarioId),
    /// Touche F (traitée à part) ou bouton « Mode » (plein écran).
    Fullscreen,
    /// Touche ESC (traitée à part) ou bouton « Quitter » (quitter la partie).
    Quit,
}

/// Convertit une touche détectée de l'écran titre en action (voir
/// `TitleAction`). Les touches ESC et F sont déjà traitées avant l'appel.
fn key_action(k: KeyCode) -> TitleAction {
    match k {
        KeyCode::O => TitleAction::Settings,
        KeyCode::N => TitleAction::ScenarioNext,
        KeyCode::B => TitleAction::ScenarioPrev,
        KeyCode::Key1 => TitleAction::Pick(crate::scenario::ScenarioId::FreePlay),
        KeyCode::Key2 => TitleAction::Pick(crate::scenario::ScenarioId::Progression),
        KeyCode::Key3 => TitleAction::Pick(crate::scenario::ScenarioId::Survival),
        KeyCode::Key4 => TitleAction::Pick(crate::scenario::scenario_id_from_index(3)),
        KeyCode::Key5 => TitleAction::Pick(crate::scenario::scenario_id_from_index(4)),
        KeyCode::Key6 => TitleAction::Pick(crate::scenario::scenario_id_from_index(5)),
        KeyCode::Key7 => TitleAction::Pick(crate::scenario::scenario_id_from_index(6)),
        KeyCode::Key8 => TitleAction::Pick(crate::scenario::scenario_id_from_index(7)),
        KeyCode::Key9 => TitleAction::Pick(crate::scenario::scenario_id_from_index(8)),
        _ => TitleAction::Launch,
    }
}

/// Libellés des boutons souris de l'écran titre (ordre : scénario ‹/›, mode,
/// réglages, quitter, lancer) - chaque bouton déclenche l'action équivalente
/// d'une touche clavier.
const TITLE_BUTTON_LABELS: [&str; 6] = [
    "< Scénario",
    "Scénario >",
    "Mode (F)",
    "Réglages (O)",
    "Quitter",
    "Lancer",
];

/// Géométrie des boutons souris de l'écran titre.
struct TitleButtons {
    prev_scenario: Rect,
    next_scenario: Rect,
    mode: Rect,
    settings: Rect,
    quit: Rect,
    launch: Rect,
}

/// Calcule la géométrie des boutons souris de l'écran titre : une rangée
/// horizontale centrée en bas de l'écran (au-dessus de l'affichage version),
/// chacune équivalente à une action clavier.
fn title_buttons_layout() -> TitleButtons {
    let btn_h = 26.0;
    let gap = 8.0;
    let widths: Vec<f32> = TITLE_BUTTON_LABELS
        .iter()
        .map(|l| measure_text(l, None, 16, 1.0).width + 2.0 * crate::render::BOX_PADDING)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + gap * (TITLE_BUTTON_LABELS.len() - 1) as f32;
    let mut x = (VIEWPORT_WIDTH as f32 - total) / 2.0;
    let y = VIEWPORT_HEIGHT as f32 - 40.0;
    let mut make = |w: f32| {
        let r = Rect::new(x, y, w, btn_h);
        x += w + gap;
        r
    };
    let prev_scenario = make(widths[0]);
    let next_scenario = make(widths[1]);
    let mode = make(widths[2]);
    let settings = make(widths[3]);
    let quit = make(widths[4]);
    let launch = make(widths[5]);
    TitleButtons {
        prev_scenario,
        next_scenario,
        mode,
        settings,
        quit,
        launch,
    }
}

/// Détermine l'action demandée par un clic gauche sur un bouton souris de
/// l'écran titre (`None` = clic hors bouton). Les boutons couvrent les mêmes
/// actions que les touches clavier (voir `TitleAction`).
fn title_mouse_click() -> Option<TitleAction> {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return None;
    }
    let l = title_buttons_layout();
    let m = mouse_to_game();
    if l.prev_scenario.contains(m) {
        Some(TitleAction::ScenarioPrev)
    } else if l.next_scenario.contains(m) {
        Some(TitleAction::ScenarioNext)
    } else if l.mode.contains(m) {
        Some(TitleAction::Fullscreen)
    } else if l.settings.contains(m) {
        Some(TitleAction::Settings)
    } else if l.quit.contains(m) {
        Some(TitleAction::Quit)
    } else if l.launch.contains(m) {
        Some(TitleAction::Launch)
    } else {
        None
    }
}

/// Dessine les boutons souris de l'écran titre (hover blanc, comme les
/// boutons des boîtes - voir `draw_box_button`).
fn draw_title_buttons() {
    let l = title_buttons_layout();
    let rects = [
        l.prev_scenario,
        l.next_scenario,
        l.mode,
        l.settings,
        l.quit,
        l.launch,
    ];
    for (label, rect) in TITLE_BUTTON_LABELS.iter().zip(rects) {
        crate::shop_render::draw_box_button(label, rect);
    }
}

/// Choix du lancement (boîte « SAUVEGARDE TROUVEE ») cliqué à la souris : les
/// boutons reproduisent les touches du choix (1/ENTER/C = poursuivre, 2/R =
/// repartir, ESC = annuler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchChoiceClick {
    None,
    Continue,
    Restart,
    Cancel,
}

/// Géométrie des boutons souris de la boîte de choix au lancement (une rangée
/// centrée en bas de la boîte).
struct LaunchChoiceLayout {
    continue_btn: Rect,
    restart_btn: Rect,
    cancel_btn: Rect,
}

fn launch_choice_layout() -> LaunchChoiceLayout {
    const W: f32 = 560.0;
    const H: f32 = 200.0;
    let left = ((VIEWPORT_WIDTH as f32 - W) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - H) / 2.0).round();
    const LABELS: [&str; 3] = ["POURSUIVRE", "REPARTIR", "ANNULER"];
    let btn_h = 26.0;
    let gap = 12.0;
    let pad = 2.0 * crate::render::BOX_PADDING;
    let widths: [f32; 3] = [
        measure_text(LABELS[0], None, 16, 1.0).width + pad,
        measure_text(LABELS[1], None, 16, 1.0).width + pad,
        measure_text(LABELS[2], None, 16, 1.0).width + pad,
    ];
    let total = widths.iter().sum::<f32>() + gap * 2.0;
    let mut x = left + (W - total) / 2.0;
    let y = top + H - 20.0 - btn_h;
    let continue_btn = Rect::new(x, y, widths[0], btn_h);
    x += widths[0] + gap;
    let restart_btn = Rect::new(x, y, widths[1], btn_h);
    x += widths[1] + gap;
    let cancel_btn = Rect::new(x, y, widths[2], btn_h);
    LaunchChoiceLayout {
        continue_btn,
        restart_btn,
        cancel_btn,
    }
}

/// Détecte un clic gauche sur un bouton de la boîte de choix au lancement
/// (`None` = aucun).
fn launch_choice_click() -> LaunchChoiceClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return LaunchChoiceClick::None;
    }
    let l = launch_choice_layout();
    let m = mouse_to_game();
    if l.continue_btn.contains(m) {
        LaunchChoiceClick::Continue
    } else if l.restart_btn.contains(m) {
        LaunchChoiceClick::Restart
    } else if l.cancel_btn.contains(m) {
        LaunchChoiceClick::Cancel
    } else {
        LaunchChoiceClick::None
    }
}

/// Écran titre : boucle jusqu'à une touche (autre que F, O ou N/B/1-3), ex
/// `titleLoop`. `sounds` sert à l'écran de paramétrage (touche O), accessible
/// depuis le titre - musique et volume y sont réglables ; N/B ou 1-3 changent
/// de scénario (jeu libre ↔ Progression ↔ Survival). Renvoie `(quit, restart,
/// progression_reset)` : `quit` si ESC a été pressé (l'application doit se
/// fermer), `restart` si le bouton RESTART de l'écran de paramétrage a été
/// cliqué (le jeu doit se relancer), `progression_reset` si le bouton RESET
/// PROGRESSION a été cliqué depuis le titre (le vaisseau doit être reconstruit
/// au lancement de la partie - les plans liés aux extensions remises à zéro ne
/// sont pas visibles à l'écran titre, qui ne dessine pas le monde).
pub async fn title_loop(
    state: &mut GameState,
    assets: &crate::render::Assets,
    rt: &RenderTarget,
    sounds: &mut Sounds,
) -> (bool, bool, bool) {
    const COLOR_STEPS: f64 = 48.0;
    const COLOR_SPEED: f64 = 0.3;
    let mut color_step = 0.0;
    let mut camera = Point::new(0.0, 0.0);
    // fin (temps absolu) du flash de la ligne des règles ; 0 = aucun flash
    let mut flash_until: f64 = 0.0;

    let banner_cols = BANNER[0].len();
    let mut banner_colors = vec![0u32; banner_cols];
    // RESET PROGRESSION cliqué depuis l'écran de paramétrage du titre : la
    // progression est remise à zéro mais le vaisseau n'est pas reconstruit ici
    // (l'écran titre ne dessine pas le monde) - le drapeau est rendu à `main`
    // pour que le mesh soit reconstruit au lancement de la partie.
    let mut progression_reset = false;

    // pacing (ex `_limit ATTEMPT_FPS`), filet anti-fuite comme l'original.
    const LIMIT_FPS: bool = true;
    let target_frame = 1.0 / ATTEMPT_FPS as f64;
    let mut last_frame = get_time();

    loop {
        if LIMIT_FPS {
            crate::frame_pace(target_frame, &mut last_frame);
        }

        // fenêtre fenêtrée : persiste position/taille réelles quand elles
        // changent (déplacement ou redimensionnement par le WM) - au plus une
        // vérification par seconde (`persist_window_geometry`)
        persist_window_geometry(state);

        // ESC : quitter l'application (invite « [ ESC to quit ] » de l'écran
        // titre) - les autres touches lancent la partie
        if is_key_pressed(KeyCode::Escape) {
            return (true, false, progression_reset);
        }

        // touche F : plein écran ; O : écran de paramétrage ; N/B/1-3 :
        // scénario ; toute autre touche : lancement
        let mut key: Option<KeyCode> = None;
        for k in [
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
        // F : détection robuste (front montant de `is_key_down` - une pression
        // avalée par le filtre de répétition de macroquad après une bascule
        // plein écran reste comptée, voir `input::f_pressed`)
        //
        // NB : on ne fait PAS `continue` ici - `keys_pressed` n'est vidé qu'à
        // `end_frame`, atteint seulement quand la coroutine rend la main à
        // `next_frame` : un `continue` re-testerait F à l'infini (gel avec
        // cadre figé, boucle sans rendu). On cède une frame (le keypress est
        // consommé), comme l'original qui relit `inkey$` (consommant) à chaque
        // itération.
        if crate::input::f_pressed(state) {
            cycle_view_mode(state);
            next_frame().await;
            continue;
        }
        // action de l'écran titre : une touche du clavier (ESC et F étant déjà
        // traitées en haut de la boucle) ou, à défaut (aucune touche), un clic
        // sur un bouton souris de l'écran titre - l'équivalent exact de la
        // touche (voir `title_mouse_click` / `draw_title_buttons`). Les deux
        // pilotent les mêmes branches ci-dessous.
        let mut action = key.map(key_action);
        if action.is_none() {
            action = title_mouse_click();
        }
        if let Some(action) = &action {
            match *action {
                // quitter : l'invite « [ ESC to quit ] » vaut aussi pour le
                // bouton souris « Quitter »
                TitleAction::Quit => return (true, false, progression_reset),
                // plein écran (bouton « Mode (F) ») : même cycle que la touche
                // F ; comme pour F, on cède une frame (clic consommé)
                TitleAction::Fullscreen => {
                    cycle_view_mode(state);
                    next_frame().await;
                    continue;
                }
                TitleAction::Settings => {
                    // ouvre l'écran de paramétrage (mêmes initialisations que
                    // la touche O du jeu) ; sous-boucle d'input + rendu jusqu'à
                    // la fermeture (CLOSE ou ESC - consommé ici, ne quitte pas
                    // le jeu), puis retour à l'écran titre. Un clic sur RESTART
                    // (relance demandée) sort immédiatement du titre.
                    state.settings_box = true;
                    while state.settings_box {
                        if LIMIT_FPS {
                            crate::frame_pace(target_frame, &mut last_frame);
                        }
                        let result = crate::settings::handle_settings_input(state, Some(sounds));
                        if result.progression_reset {
                            progression_reset = true;
                        }
                        if result.restart {
                            // ferme l'écran : si la relance échoue (retour de
                            // `main`), la partie démarre sans l'écran ouvert
                            state.settings_box = false;
                            return (false, true, progression_reset);
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
                // toute sélection de scénario (boutons «< Scénario >», touches
                // N/B ou sélection directe 1-3) : règles de départ appliquées
                // (cycle/select), progression enregistrée restaurée puis
                // nouveau scénario persisté, et flash de la ligne des règles
                // (1,2 s - voir `draw_frame`) ; comme F, on cède une frame
                // (clic/touche consommé)
                TitleAction::ScenarioNext | TitleAction::ScenarioPrev | TitleAction::Pick(_) => {
                    let mut restore = |state: &mut GameState| {
                        crate::scenario::load_progression(state);
                        let _ = crate::scenario::save_progression(state);
                        flash_until = get_time() + RULES_FLASH_DURATION;
                    };
                    match *action {
                        TitleAction::ScenarioNext => crate::scenario::cycle_scenario(state),
                        TitleAction::ScenarioPrev => crate::scenario::cycle_scenario_back(state),
                        TitleAction::Pick(id) => crate::scenario::select_scenario(state, id),
                        _ => {}
                    }
                    restore(state);
                    next_frame().await;
                    continue;
                }
                // lancement de la partie : s'il existe une progression
                // enregistrée pour le scénario courant, proposer de
                // **poursuivre** ou de **repartir du début** (sous-boucle
                // d'input + rendu, comme l'écran de paramétrage ci-dessus -
                // l'état courant porte déjà la sauvegarde restaurée par
                // `load_progression`).
                TitleAction::Launch => {
                    if crate::scenario::has_saved_progression(state) {
                        // la touche/souris qui a lancé la partie est encore
                        // dans la file d'input : on cède une frame avant de
                        // lire les touches du choix, sinon elle serait relue
                        // immédiatement (ex R = lancement → « repartir »)
                        next_frame().await;
                        let mut launch = false;
                        'launch_choice: loop {
                            if LIMIT_FPS {
                                crate::frame_pace(target_frame, &mut last_frame);
                            }
                            for k in [
                                KeyCode::Escape,
                                KeyCode::Enter,
                                KeyCode::Space,
                                KeyCode::Key1,
                                KeyCode::Key2,
                                KeyCode::C,
                                KeyCode::R,
                            ] {
                                if is_key_pressed(k) {
                                    match k {
                                        // ESC : annule le lancement, retour à
                                        // l'écran titre (le scénario reste
                                        // sélectionné)
                                        KeyCode::Escape => break 'launch_choice,
                                        // poursuivre le scénario : l'état porte
                                        // déjà la progression restaurée
                                        KeyCode::Enter
                                        | KeyCode::Space
                                        | KeyCode::Key1
                                        | KeyCode::C => {
                                            launch = true;
                                            break 'launch_choice;
                                        }
                                        // repartir du début : progression remise
                                        // à zéro (clés `prog_*` supprimées,
                                        // règles de départ réappliquées - le
                                        // vaisseau sera reconstruit au lancement,
                                        // voir `main.rs`)
                                        KeyCode::Key2 | KeyCode::R => {
                                            crate::scenario::reset_progression(state);
                                            progression_reset = true;
                                            launch = true;
                                            break 'launch_choice;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            // clic souris sur un bouton de la boîte de choix :
                            // mêmes actions que les touches (poursuivre /
                            // repartir / annuler)
                            match launch_choice_click() {
                                LaunchChoiceClick::Continue => {
                                    launch = true;
                                    break 'launch_choice;
                                }
                                LaunchChoiceClick::Restart => {
                                    crate::scenario::reset_progression(state);
                                    progression_reset = true;
                                    launch = true;
                                    break 'launch_choice;
                                }
                                LaunchChoiceClick::Cancel => break 'launch_choice,
                                LaunchChoiceClick::None => {}
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
                            draw_launch_choice(state);
                            next_frame().await;
                        }
                        if !launch {
                            // ESC / bouton ANNULER : retour à l'écran titre.
                            // La touche est encore dans la file d'input (le
                            // `break` ne la consomme pas, la file n'est vidée
                            // qu'à `next_frame`) : on cède une frame avant de
                            // continuer, sinon la même pression serait relue
                            // par la boucle du titre et rouvrirait immédiatement
                            // cet écran de choix (ESC semblait ne pas fermer la
                            // boîte).
                            next_frame().await;
                            continue;
                        }
                    }
                    break;
                }
            }
        }

        // caméra qui descend + rotation des couleurs (ex titleLoop : nouvelle
        // couleur à droite, décalage de tout le tableau) - avant le rendu,
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
    (false, false, progression_reset)
}

/// Affiche la liste des objectifs DAG d'un scénario custom sur l'écran titre
/// avec leur statut : complété (✓ vert), débloqué (→ jaune), verrouillé
/// (🔒 gris). Renvoie la position Y après la dernière ligne affichée.
fn draw_objectives_list(state: &crate::state::GameState, start_y: f64) -> f64 {
    let tracker = &state.objective_tracker;
    if !tracker.has_objectives() {
        return start_y;
    }

    let mut y = start_y;

    // En-tête
    let header = format!(
        "[ OBJECTIFS : {}/{} ]",
        tracker.completed_count(),
        tracker.total_count()
    );
    y += draw_centered_line(&header, y as f32) as f64 * TITLE_LINE_H;

    // Lister tous les objectifs avec leur statut
    let all_objectives = &tracker.objectives;
    let completed_ids = &tracker.completed_ids;

    // Déterminer les objectifs débloqués (prérequis satisfaits)
    let mut unlocked_set = std::collections::HashSet::new();
    for obj in all_objectives.iter() {
        if !completed_ids.contains(&obj.id)
            && obj
                .prerequisites
                .iter()
                .all(|pre| completed_ids.contains(pre))
        {
            unlocked_set.insert(obj.id.clone());
        }
    }

    for obj in all_objectives.iter() {
        // « ✓ » (complété) et « → » (débloqué) : la police embarquée (DejaVu
        // Sans Mono) possède ces glyphes - « · » (U+00B7) est en Latin-1
        let (symbol, color) = if completed_ids.contains(&obj.id) {
            ("✓", 0xFF39FF88u32) // vert néon = complété
        } else if unlocked_set.contains(&obj.id) {
            ("→", 0xFFFFFF00u32) // jaune = débloqué (en cours)
        } else {
            ("·", 0xFF666688u32) // gris = verrouillé
        };

        let line = format!("  {} {} - {}", symbol, obj.title, obj.description);
        // pas de troncature : le texte est **replié** au besoin (saut de
        // ligne aux espaces) s'il dépasse la largeur de l'écran
        y += draw_centered_line_color(&line, y as f32, argb_to_color(color)) as f64 * TITLE_LINE_H;
    }

    y += 4.0; // espace avant les touches
    y
}

/// Dessine un texte **replié** s'il dépasse la largeur de l'écran (saut de
/// ligne aux espaces), chaque ligne centrée, avec la couleur donnée. Renvoie
/// le nombre de lignes dessinées.
fn draw_centered_line_color(line: &str, y: f32, color: Color) -> usize {
    let size = TITLE_FONT;
    let lines = wrap_text(line, size, TITLE_TEXT_W);
    for (i, l) in lines.iter().enumerate() {
        let w = measure_text(l, None, size, 1.0).width;
        draw_text(
            l,
            (VIEWPORT_WIDTH as f32 - w) / 2.0,
            y + i as f32 * TITLE_LINE_H as f32,
            size as f32,
            color,
        );
    }
    lines.len()
}

/// Boîte de choix affichée au lancement d'un scénario qui a une progression
/// enregistrée (`has_saved_progression`) : propose de **poursuivre le
/// scénario** ou de **repartir du début** - avec le résumé de la sauvegarde
/// (`SAVE`, mêmes segments que l'écran titre) et les touches du choix. Dessinée
/// par-dessus l'écran titre (fond sombre + bordure, comme l'écran de
/// paramétrage) ; la sous-boucle d'input correspondante est dans `title_loop`.
fn draw_launch_choice(state: &GameState) {
    // fond opaque et sombre (bleu nuit) : le BOX_BG partagé (0xD0, ~82 %)
    // laisse transparaître la bannière et les étoiles derrière la boîte, ce
    // qui gêne la lecture du résumé de sauvegarde et des deux options - un
    // fond opaque foncé maximise le contraste avec le texte clair
    const BG: u32 = 0xFF0A1E44;
    const BORDER: u32 = 0xFF1AB2FF;
    const FG: u32 = 0xFFD6EEFF;
    const FG_DIM: u32 = 0xFFC2E4FF;
    // poursuivre = vert néon (comme les objectifs complétés), repartir du
    // début = rouge (remise à zéro de la progression)
    const CONTINUE: u32 = 0xFF39FF88;
    const RESTART: u32 = 0xFFFF5A5A;

    let w = 560.0;
    let h = 200.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BORDER));

    // titre centré
    let title = "*** SAUVEGARDE TROUVEE ***";
    let title_w = measure_text(title, None, 16, 1.0).width;
    draw_text(
        title,
        left + (w - title_w) / 2.0,
        top + 30.0,
        16.0,
        argb_to_color(FG),
    );

    // résumé de la progression enregistrée (mêmes segments que la ligne
    // `[ SAVE : … ]` de l'écran titre) - tronqué s'il est trop long
    let scenario = crate::scenario::scenario(state.scenario);
    let summary: String = crate::scenario::save_summary_segments(state)
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let summary_line = format!("{} - {}", scenario.name, summary);
    let summary_line = if summary_line.len() > 76 {
        format!("{}...", &summary_line[..73])
    } else {
        summary_line
    };
    let summary_w = measure_text(&summary_line, None, 12, 1.0).width;
    draw_text(
        &summary_line,
        left + (w - summary_w) / 2.0,
        top + 58.0,
        12.0,
        argb_to_color(FG_DIM),
    );

    // les deux options (avec leurs équivalents clavier)
    draw_centered_line_color("1 / ENTER : POURSUIVRE LE SCENARIO", top + 90.0, argb_to_color(CONTINUE));
    draw_centered_line_color("2 / R : REPARTIR DU DEBUT", top + 118.0, argb_to_color(RESTART));
    // et trois boutons souris équivalents en bas de la boîte (le clic pilote
    // le choix, comme les touches ci-dessus)
    let l = launch_choice_layout();
    crate::shop_render::draw_box_button("POURSUIVRE", l.continue_btn);
    crate::shop_render::draw_box_button("REPARTIR", l.restart_btn);
    crate::shop_render::draw_box_button("ANNULER", l.cancel_btn);
}

/// Dessine une frame de l'écran titre : caméra selon le mode d'affichage,
/// fond d'étoiles, bannière arc-en-ciel, invites, les boutons souris
/// (équivalents des actions clavier, masqués pendant l'écran de paramétrage),
/// l'écran de paramétrage s'il est ouvert (touche O) et l'étirement de la vue
/// virtuelle le cas échéant.
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
    draw_stars(assets, camera, false);

    // bannière : un caractère par colonne, chaque colonne colorée
    // (ex titleLoop : `_printstring` de chaque caractère)
    let banner_rows = BANNER.len();
    let banner_cols = BANNER[0].len();
    for (j, row) in BANNER.iter().enumerate() {
        for (i, ch) in row.bytes().enumerate() {
            let ch = ch as char;
            let x = (VIEWPORT_WIDTH / banner_cols as f64) * i as f64;
            let y = 10.0 * (8.0 + j as f64);
            let color = banner_colors[banner_cols - 1 - i];
            draw_text(&ch.to_string(), x as f32, y as f32, 8.0, argb_to_color(color));
        }
    }

    // invites (ex titleLoop) - la 1re ligne affiche le scénario courant, la
    // 2e ses règles (valeurs en surbrillance - voir
    // `scenario::scenario_rules`) et la 3e la progression enregistrée de ce
    // scénario, valeurs en surbrillance aussi (minerais, modes, réputation,
    // rang, vies, bouclier - voir `scenario::save_summary_segments`)
    let scenario = crate::scenario::scenario(state.scenario);
    let mut y = 10.0 * (8.0 + banner_rows as f64) + 20.0;
    y += draw_centered_line(
        &format!("[ SCENARIO : {} - {} ]", scenario.name, scenario.description),
        y as f32,
    ) as f64
        * TITLE_LINE_H;

    // ligne des règles : segments alignés, valeurs colorées (couleur du
    // scénario - voir `scenario::scenario_rules`) - les coûts/vies/bouclier/
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
    // phase du clignotement (parité de l'horloge) : alternance ≈ 3 cycles/s -
    // pendant le flash, toute la ligne prend la couleur du scénario
    let blink_on = flash_rules && ((get_time() * RULES_FLASH_HZ) as i64 % 2 == 0);
    let flash_color = if blink_on {
        Some(scenario.rules_color)
    } else {
        None
    };
    let rules_lines = draw_segments_line(&segments, y as f32, flash_color);
    y += rules_lines as f64 * TITLE_LINE_H;

    // ligne de la progression enregistrée : mêmes segments, valeurs (minerais,
    // modes, réputation, rang, vies, bouclier) dans la couleur du scénario -
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
    let save_lines = draw_segments_line(&save_segments, y as f32, None);
    y += save_lines as f64 * TITLE_LINE_H;

    // Objectifs DAG (scénarios custom) : liste des étapes avec statut
    if crate::scenario::is_custom(state.scenario) {
        y = draw_objectives_list(state, y);
    }

    let custom_count = crate::scenario_loader::loaded_count();
    let key_hint = if custom_count > 0 {
        format!(
            "[ N/B : scenario (1-{} : pick)  |  F : window / zoomed / native  |  O : settings ]",
            3 + custom_count.min(6)
        )
    } else {
        "[ N/B : scenario (1-3 : pick)  |  F : window / zoomed / native  |  O : settings ]"
            .to_string()
    };
    for line in [
        key_hint.as_str(),
        "[ ESC to quit ]",
        "[ Hit a key or click a button below to launch ]",
    ] {
        let n = draw_centered_line(line, y as f32);
        y += n as f64 * TITLE_LINE_H;
    }

    // version + numéro de build (petit, coin bas-droit - voir
    // `build_info::display`) : discrète, sans perturber les invites centrées
    let version_text = crate::build_info::display();
    let v_w = measure_text(&version_text, None, 8, 1.0).width;
    draw_text(
        &version_text,
        VIEWPORT_WIDTH as f32 - v_w - 4.0,
        VIEWPORT_HEIGHT as f32 - 6.0,
        8.0,
        argb_to_color(0xFF6B6B7E),
    );

    // boutons souris de l'écran titre (équivalents des actions clavier) -
    // masqués pendant l'écran de paramétrage, dessiné par-dessus
    if !state.settings_box {
        draw_title_buttons();
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
