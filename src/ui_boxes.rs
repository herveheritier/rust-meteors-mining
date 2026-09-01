//! Fenêtres plein écran : aide (S) et paramétrage (O) -
//! mise en page et rendu (issu de `src/render.rs`).

use macroquad::prelude::*;
use crate::config::*;
use crate::render::*;
use crate::font::{draw_text, measure_text};
use crate::audio::Sounds;
use crate::scenario;
use crate::state::GameState;

/// Géométrie du bouton CLOSE de la fenêtre d'aide (ex `windowUtils_createButton`
/// avec `left=20, bottom=20`) : fenêtre 320×240 centrée, bouton en bas à
/// gauche. Renvoie le rectangle écran du bouton (pour la détection de clic
/// côté logique).
pub fn help_box_layout() -> Rect {
    let w = 320.0;
    let h = 240.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    // buttonWidth = max(len("CLOSE")*8 + 2*padding, 60) ; buttonHeight = 16+10
    let btn_w = (5.0 * 8.0 + 2.0 * BOX_PADDING).max(60.0);
    let btn_h = 26.0;
    let btn_left = left + 20.0;
    let btn_top = top + h - 20.0 - 20.0;
    Rect::new(btn_left, btn_top, btn_w, btn_h)
}

/// Dessine la fenêtre d'aide (touche S, ex `help` de windowUtils) : fond,
/// bordure, libellés des touches et bouton CLOSE (hover blanc).
pub fn draw_help_box() {
    let w = 320.0;
    let h = 240.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // libellés des touches (ex windowUtils_createLabel, à 10 px de gauche,
    // 16 px d'écart) - la touche T est listée mais non implémentée dans
    // l'original (bloc commenté), on la conserve telle quelle
    let labels = [
        "P : pause",
        "S : show keys (this screen)",
        "T : dump triangles to console",
        "A : switch automatic shape generation",
        "D : display data",
        "F : cycle window / zoomed / native fullscreen",
        "G : generate a shape",
        "O : settings (audio, graphics)",
        "K : kill all shapes",
        "L : show events log",
        "Enter : dock box (when docked)",
    ];
    for (i, label) in labels.iter().enumerate() {
        draw_text(
            label,
            left + 10.0,
            top + 10.0 + 16.0 * i as f32 + 12.0,
            16.0,
            argb_to_color(BOX_FG),
        );
    }

    // position de la souris dans la fenêtre (ex lbl2 mis à jour par la boucle)
    let m = mouse_to_game();
    let coords = format!("{},{}", (m.x - left) as i32, (m.y - top) as i32);
    draw_text(&coords, left + 240.0, top + 5.0 + 12.0, 16.0, argb_to_color(BOX_FG));

    // bouton CLOSE
    draw_box_button("CLOSE", help_box_layout());
}

/// Géométrie des contrôles de l'écran de paramétrage : fenêtre 560×280
/// centrée en deux colonnes - à gauche les cases MUSIC, AUTO GENERATE et
/// TOUCH UI, la barre horizontale du volume (ascenseur) et le bouton RESET
/// PROGRESSION (pleine largeur de la colonne) ; à droite le panneau
/// « GRAPHICS » (style de rendu, mode d'affichage fenêtré/plein écran,
/// définition de fenêtre, anticrénelage) ; les boutons RESET et CLOSE côte
/// à côte en bas. (Le mode de déplacement se choisit désormais au magasin de
/// la station - bouton SHOP de la boîte DOCK STATION.)
pub struct SettingsLayout {
    /// Ligne cliquable de la case MUSIC.
    pub music: Rect,
    /// Ligne cliquable de la case AUTO GENERATE.
    pub auto_generate: Rect,
    /// Barre horizontale du volume maître (ascenseur) : zone
    /// cliquable/glissable de 22 px de haut, avec la piste de 6 px centrée à
    /// l'intérieur.
    pub volume_track: Rect,
    /// Barre du sous-volume MUSIQUE (ascenseur, même géométrie que
    /// `volume_track`).
    pub music_volume_track: Rect,
    /// Barre du sous-volume EFFETS (ascenseur - tirs, explosions, minerais,
    /// moteurs).
    pub effects_volume_track: Rect,
    /// Barre du sous-volume AMBIANCE (ascenseur - boucle de fond).
    pub ambient_volume_track: Rect,
    /// Panneau des options graphiques (fond + bordure + libellé « GRAPHICS »).
    pub graphics_panel: Rect,
    /// Ligne RENDER : style de rendu des triangles (clic = cycle TEXTURED →
    /// COLORED → MESH).
    pub render: Rect,
    /// Ligne WINDOW : mode d'affichage (clic = cycle WINDOWED → ZOOMED →
    /// NATIVE).
    pub window_mode: Rect,
    /// Ligne SIZE : définition de la fenêtre (clic = cycle 960×540 → …).
    pub window_size: Rect,
    /// Ligne cliquable de la case ANTIALIAS (MSAA, appliquée au lancement).
    pub antialias: Rect,
    /// Bouton RESET PROGRESSION (remet à zéro la progression du scénario -
    /// crédits, modes payés, réputation, extensions, vies/bouclier ; visible
    /// seulement en scénario à économie ou à survie).
    pub reset_progress: Rect,
    /// Ligne cliquable de la case TOUCH UI (interface tactile bas-gauche /
    /// bas-droite, `touch.rs`).
    pub touch_ui: Rect,
    /// Ligne REMOTE PIN (télécommande HTTP) : affiche le code courant (ou
    /// `NONE`) ; un clic arme la saisie clavier (4 chiffres max, ENTRÉE
    /// valide, ÉCHAP annule - vide + ENTRÉE = aucune protection).
    pub pin_edit: Rect,
    /// Ligne cliquable de la case SAVE POSITION (le vaisseau repart de sa
    /// dernière position à la sortie - colonne droite, sous le panneau
    /// GRAPHICS).
    pub save_position: Rect,
    /// Ligne cliquable de la case STARS 3x3 (étoiles du fond dessinées en
    /// 3×3 px au lieu de 1×1 - colonne droite, sous SAVE POSITION).
    pub stars_big: Rect,
    /// Bouton RESET (réglages par défaut).
    pub reset: Rect,
    /// Bouton RESTART (relance le jeu - affiché uniquement quand un réglage
    /// modifié exige un redémarrage, ex l'anticrénelage).
    pub restart: Rect,
    /// Bouton CLOSE (ferme l'écran).
    pub close: Rect,
}

/// Calcule la géométrie de l'écran de paramétrage (voir `SettingsLayout`).
pub fn settings_box_layout() -> SettingsLayout {
    let w = 560.0;
    let h = 380.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let col_w = 250.0;
    let col_left = left + 20.0;
    let col_right = left + w - 20.0 - col_w;

    // colonne gauche : cases audio + 4 barres de volume (maître + musique /
    // effets / ambiance) + RESET PROGRESSION + TOUCH UI + REMOTE PIN
    let music = Rect::new(col_left, top + 44.0, col_w, 26.0);
    let auto_generate = Rect::new(col_left, top + 76.0, col_w, 26.0);
    // volume : barre horizontale (ascenseur) sur la majeure partie de la
    // ligne, après le libellé ; zone de clic de 22 px de haut
    let track = |y: f32| Rect::new(col_left + 100.0, y, col_w - 104.0, 22.0);
    // espacement 32 px : la valeur en % (dessinée sous la barre, 26 px sous
    // le haut) ne doit pas chevaucher la piste suivante
    let volume_track = track(top + 108.0);
    let music_volume_track = track(top + 140.0);
    let effects_volume_track = track(top + 172.0);
    let ambient_volume_track = track(top + 204.0);
    // RESET PROGRESSION : bouton pleine largeur de la colonne gauche, sous les
    // barres (remet à zéro la progression du scénario courant)
    let reset_progress = Rect::new(col_left, top + 240.0, col_w, 26.0);
    // TOUCH UI : case à cocher sous RESET PROGRESSION (interface tactile
    // joystick + bouton de tir, `touch.rs`)
    let touch_ui = Rect::new(col_left, top + 270.0, col_w, 26.0);
    // REMOTE PIN : ligne à cliquable sous TOUCH UI (télécommande HTTP - le
    // code est saisi au clavier après le clic, voir `game.rs`)
    let pin_edit = Rect::new(col_left, top + 300.0, col_w, 26.0);

    // colonne droite : panneau des options graphiques
    let graphics_panel = Rect::new(col_right, top + 44.0, col_w, 176.0);
    let row_w = col_w - 20.0;
    let render = Rect::new(col_right + 10.0, top + 66.0, row_w, 26.0);
    let window_mode = Rect::new(col_right + 10.0, top + 96.0, row_w, 26.0);
    let window_size = Rect::new(col_right + 10.0, top + 126.0, row_w, 26.0);
    let antialias = Rect::new(col_right + 10.0, top + 156.0, row_w, 26.0);
    // SAVE POSITION : case sous le panneau GRAPHICS (colonne droite) - le
    // vaisseau repart de sa dernière position (persistée à la sortie)
    let save_position = Rect::new(col_right + 6.0, top + 232.0, row_w + 8.0, 26.0);
    // STARS 3x3 : case sous SAVE POSITION (colonne droite) - étoiles du fond
    // en 3×3 px (visibilité du champ d'étoiles selon l'écran)
    let stars_big = Rect::new(col_right + 6.0, top + 264.0, row_w + 8.0, 26.0);

    // boutons en bas : RESET à gauche, CLOSE à droite (ex
    // `windowUtils_choiceBox` : 1er sur la moitié gauche, 2e sur la moitié
    // droite) et RESTART au centre - affiché seulement si un redémarrage est
    // nécessaire
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let btn_h = 26.0;
    let w1 = btn_w("RESET");
    let w2 = btn_w("CLOSE");
    let w3 = btn_w("RESTART");
    let left1 = left + (w / 2.0 - w1) / 2.0 - BOX_PADDING;
    let left2 = left + (3.0 * w / 2.0 - w2) / 2.0 - BOX_PADDING;
    let top_btn = top + h - 20.0 - btn_h;
    let reset = Rect::new(left1, top_btn, w1, btn_h);
    let close = Rect::new(left2, top_btn, w2, btn_h);
    let restart = Rect::new(left + (w - w3) / 2.0 - BOX_PADDING, top_btn, w3, btn_h);

    SettingsLayout {
        music,
        auto_generate,
        volume_track,
        music_volume_track,
        effects_volume_track,
        ambient_volume_track,
        graphics_panel,
        render,
        window_mode,
        window_size,
        antialias,
        reset_progress,
        touch_ui,
        pin_edit,
        save_position,
        stars_big,
        reset,
        restart,
        close,
    }
}

/// Dessine l'écran de paramétrage (touche O) : fond, bordure, titre, les
/// deux colonnes (audio + RESET PROGRESSION à gauche, panneau « GRAPHICS » à
/// droite) et les boutons RESET / CLOSE (ex `windowUtils`). `sounds` fournit
/// l'état musique et le volume courant.
/// Dessine une barre de volume (maître ou sous-volume) : libellé à gauche,
/// piste + remplissage + curseur à droite, valeur en % centrée sous la
/// barre. `value` est la fraction 0..1 affichée ; `label` est posé au bord
/// gauche de la colonne.
pub fn draw_volume_bar(track: Rect, label: &str, value: f32, m: Vec2) {
    let color = argb_to_color(if track.contains(m) { BOX_HOVER } else { BOX_FG });
    draw_text(label, track.x - 96.0, track.y + 15.0, 16.0, color);
    let bar_y = track.y + (track.h - 6.0) / 2.0;
    let fill = track.w * value.clamp(0.0, 1.0);
    draw_rectangle(track.x, bar_y, track.w, 6.0, argb_to_color(0x601AB2FF));
    draw_rectangle(track.x, bar_y, fill, 6.0, color);
    // curseur (ascenseur) : barre verticale de 14 px, centrée sur la piste
    let thumb_x = (track.x + fill - 2.0).clamp(track.x, track.x + track.w - 4.0);
    draw_rectangle(thumb_x, bar_y - 4.0, 4.0, 14.0, color);
    let value = format!("{}%", (value * 100.0).round() as i32);
    let value_w = measure_text(&value, None, 16, 1.0).width;
    draw_text(
        &value,
        track.x + (track.w - value_w) / 2.0,
        track.y + track.h + 4.0,
        16.0,
        argb_to_color(BOX_FG_DIM),
    );
}

pub fn draw_settings_box(state: &GameState, sounds: &Sounds) {
    let w = 560.0;
    let h = 380.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let msg = "*** SETTINGS ***";
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text_shadow(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    let layout = settings_box_layout();
    let m = mouse_to_game();

    // cases à cocher MUSIC (état depuis les sons) et AUTO GENERATE
    draw_checkbox(layout.music, sounds.music_on, "MUSIC", m);
    draw_checkbox(layout.auto_generate, state.auto_generate, "AUTO GENERATE", m);

    // volumes : barre maître puis trois sous-volumes (musique, effets,
    // ambiance) - piste, remplissage selon la valeur et curseur vertical,
    // valeur en % centrée sous la barre (hover blanc sur toute la zone)
    draw_volume_bar(layout.volume_track, "VOLUME", sounds.volume, m);
    draw_volume_bar(layout.music_volume_track, "MUSIC", sounds.music_volume, m);
    draw_volume_bar(layout.effects_volume_track, "EFFECTS", sounds.effects_volume, m);
    draw_volume_bar(layout.ambient_volume_track, "AMBIENT", sounds.ambient_volume, m);

    // panneau GRAPHICS : fond + bordure + libellé en tête, puis les lignes
    // RENDER / WINDOW / SIZE (valeurs cyclables dans un cadre) et la case
    // ANTIALIAS ; note si l'anticrénelage n'est effectif qu'au lancement
    let g = layout.graphics_panel;
    draw_rectangle(g.x, g.y, g.w, g.h, argb_to_color(BOX_PANEL_BG));
    draw_rectangle_lines(g.x, g.y, g.w, g.h, 1.0, argb_to_color(BOX_PANEL_BORDER));
    draw_text("GRAPHICS", g.x + 10.0, g.y + 14.0, 16.0, argb_to_color(BOX_FG));
    draw_cycle_row(layout.render, "RENDER", render_style_label(state.render_style as i32), m);
    draw_cycle_row(layout.window_mode, "WINDOW", window_mode_label(state.view_mode as i32), m);
    draw_cycle_row(layout.window_size, "SIZE", &window_size_label(state.window_size), m);
    draw_checkbox(layout.antialias, state.antialias, "ANTIALIAS", m);

    // un réglage modifié qui n'est effectif qu'au lancement (l'anticrénelage)
    // et diffère de la valeur appliquée par la fenêtre : note + bouton
    // RESTART (relance le jeu, les réglages étant déjà enregistrés)
    if state.antialias != state.antialias_applied {
        draw_text(
            "RESTART REQUIRED",
            g.x + 30.0,
            layout.antialias.y + 40.0,
            16.0,
            argb_to_color(BOX_FG_DIM),
        );
        draw_box_button("RESTART", layout.restart);
    }

    // RESET PROGRESSION : remet à zéro la progression du scénario courant
    // (crédits, modes payés, réputation, extensions, vies/bouclier) - affiché
    // seulement quand il y a une progression à remettre (scénario à économie
    // ou à survie) ; en jeu libre, rien à réinitialiser
    if scenario::has_economy(state) || scenario::has_survival(state) {
        draw_box_button("RESET PROGRESSION", layout.reset_progress);
    }
    draw_checkbox(layout.touch_ui, state.touch_ui, "TOUCH UI", m);
    draw_checkbox(layout.save_position, state.save_position, "SAVE POSITION", m);
    draw_checkbox(layout.stars_big, state.stars_big, "STARS 3x3", m);

    // télécommande : ligne REMOTE PIN (code à saisir au clavier après un
    // clic - ENTRÉE valide, ÉCHAP annule, vide + ENTRÉE = aucune protection)
    // et rappel de l'URL de la page de contrôle (le téléphone pilote le
    // vaisseau sur le réseau local - voir `remote.rs`), en bas de la colonne
    // gauche, au-dessus des boutons RESET / CLOSE
    let pin_row = layout.pin_edit;
    let pin_color = argb_to_color(if pin_row.contains(m) { BOX_HOVER } else { BOX_FG });
    draw_text("REMOTE PIN", pin_row.x + 4.0, pin_row.y + 18.0, 16.0, pin_color);
    let pin_display = if state.settings_pin_edit {
        let mut shown = state.settings_pin_buffer.clone();
        shown.push('_');
        shown
    } else if state.remote_pin.is_empty() {
        "NONE".to_string()
    } else {
        "\u{2022}".repeat(state.remote_pin.len())
    };
    let pin_w = measure_text(&pin_display, None, 16, 1.0).width;
    draw_text(
        &pin_display,
        pin_row.x + pin_row.w - 4.0 - pin_w,
        pin_row.y + 18.0,
        16.0,
        pin_color,
    );
    if let Some(url) = crate::remote::url() {
        draw_text(
            &format!("REMOTE: {url}"),
            layout.music.x + 4.0,
            top + 326.0,
            13.0,
            argb_to_color(BOX_FG_DIM),
        );
    }
    draw_box_button("RESET", layout.reset);
    draw_box_button("CLOSE", layout.close);
}

/// Dessine une ligne de réglage cyclable (RENDER / WINDOW / SIZE) : libellé
/// à gauche, valeur dans un petit cadre à droite (clic = cycle, hover blanc).
pub fn draw_cycle_row(rect: Rect, label: &str, value: &str, m: Vec2) {
    let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
    draw_text(label, rect.x + 4.0, rect.y + 18.0, 16.0, color);
    let value_w = measure_text(value, None, 16, 1.0).width;
    let value_x = rect.x + rect.w - 4.0 - value_w;
    draw_rectangle_lines(value_x - 6.0, rect.y + 3.0, value_w + 12.0, 18.0, 1.0, color);
    draw_text(value, value_x, rect.y + 17.0, 16.0, color);
}

/// Dessine une case à cocher (carré 14×14 + libellé à droite, hover blanc) ;
/// cochée = croix de validation.
pub fn draw_checkbox(rect: Rect, checked: bool, label: &str, m: Vec2) {
    let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
    let x = rect.x + 4.0;
    let y = rect.y + 6.0;
    draw_rectangle_lines(x, y, 14.0, 14.0, 1.5, color);
    if checked {
        draw_line(x + 2.0, y + 7.0, x + 6.0, y + 11.0, 2.0, color);
        draw_line(x + 6.0, y + 11.0, x + 12.0, y + 3.0, 2.0, color);
    }
    draw_text(label, rect.x + 26.0, rect.y + 18.0, 16.0, color);
}
