//! Écran de paramétrage (touche O, aussi depuis l'écran titre) :
//! cases audio, volume, panneau GRAPHICS, PIN de la
//! télécommande, RESET / RESET PROGRESSION / RESTART /
//! CLOSE - portage de `src/game.rs`.

use macroquad::prelude::*;
use crate::audio::Sounds;
use crate::config::*;
use crate::persist;
use crate::render::{cycle_view_mode, enter_fullscreen, mouse_to_game, settings_box_layout};
use crate::scenario;
use crate::state::{GameState, RenderStyle, ViewMode};

/// Clic sur l'écran de paramétrage (touche O) : les cases MUSIC / AUTO
/// GENERATE / ANTIALIAS basculent, un clic sur la barre du volume donne la
/// fraction demandée (0..1), les lignes RENDER / WINDOW / SIZE font cycler
/// leur valeur, RESET remet les réglages par défaut et CLOSE ferme l'écran.
pub enum SettingsClick {
    None,
    Music,
    AutoGenerate,
    /// Clic sur la barre du volume maître (fraction 0..1 demandée).
    Volume(f32),
    /// Clic sur la barre du sous-volume MUSIQUE (fraction 0..1 demandée).
    MusicVolume(f32),
    /// Clic sur la barre du sous-volume EFFETS (fraction 0..1 demandée).
    EffectsVolume(f32),
    /// Clic sur la barre du sous-volume AMBIANCE (fraction 0..1 demandée).
    AmbientVolume(f32),
    RenderStyle,
    WindowMode,
    WindowSize,
    Antialias,
    /// Affiche/coupe l'interface tactile (joystick + bouton de tir, `touch.rs`).
    TouchUi,
    /// Bascule la sauvegarde de position du vaisseau à la sortie (le
    /// prochain lancement repart de la dernière position).
    SavePosition,
    /// Ligne REMOTE PIN : arme la saisie du code de la télécommande (ou, si
    /// la saisie est déjà armée, valide le code tapé).
    PinEdit,
    /// Relance le jeu (affiché quand un réglage modifié exige un redémarrage).
    Restart,
    Reset,
    /// Remet à zéro la progression du scénario (minerais, modes payés,
    /// réputation, extensions, vies/bouclier).
    ResetProgress,
    Close,
}

/// Détecte un clic sur l'écran de paramétrage (touche O) : contrôle cliqué
/// (case, volume, ligne graphique, RESTART, RESET ou CLOSE). Le bouton
/// RESTART n'est actif que si un réglage modifié (l'anticrénelage) diffère de
/// la valeur appliquée par la fenêtre.
pub fn settings_box_click(state: &GameState) -> SettingsClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return SettingsClick::None;
    }
    let l = settings_box_layout();
    let m = mouse_to_game();
    if l.music.contains(m) {
        return SettingsClick::Music;
    }
    if l.auto_generate.contains(m) {
        return SettingsClick::AutoGenerate;
    }
    if l.volume_track.contains(m) {
        return SettingsClick::Volume(((m.x - l.volume_track.x) / l.volume_track.w).clamp(0.0, 1.0));
    }
    if l.music_volume_track.contains(m) {
        return SettingsClick::MusicVolume(((m.x - l.music_volume_track.x) / l.music_volume_track.w).clamp(0.0, 1.0));
    }
    if l.effects_volume_track.contains(m) {
        return SettingsClick::EffectsVolume(
            ((m.x - l.effects_volume_track.x) / l.effects_volume_track.w).clamp(0.0, 1.0),
        );
    }
    if l.ambient_volume_track.contains(m) {
        return SettingsClick::AmbientVolume(
            ((m.x - l.ambient_volume_track.x) / l.ambient_volume_track.w).clamp(0.0, 1.0),
        );
    }
    if l.render.contains(m) {
        return SettingsClick::RenderStyle;
    }
    if l.window_mode.contains(m) {
        return SettingsClick::WindowMode;
    }
    if l.window_size.contains(m) {
        return SettingsClick::WindowSize;
    }
    if l.antialias.contains(m) {
        return SettingsClick::Antialias;
    }
    if l.touch_ui.contains(m) {
        return SettingsClick::TouchUi;
    }
    if l.pin_edit.contains(m) {
        return SettingsClick::PinEdit;
    }
    if l.save_position.contains(m) {
        return SettingsClick::SavePosition;
    }
    if state.antialias != state.antialias_applied && l.restart.contains(m) {
        return SettingsClick::Restart;
    }
    if (scenario::has_economy(state) || scenario::has_survival(state)) && l.reset_progress.contains(m) {
        return SettingsClick::ResetProgress;
    }
    if l.reset.contains(m) {
        return SettingsClick::Reset;
    }
    if l.close.contains(m) {
        return SettingsClick::Close;
    }
    SettingsClick::None
}

/// Résultat du traitement de l'input de l'écran de paramétrage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsResult {
    /// Le bouton RESTART a été cliqué (le jeu doit se relancer).
    pub restart: bool,
    /// Le bouton RESET PROGRESSION a été cliqué (la progression du scénario a
    /// été remise à zéro - la boucle de jeu doit reconstruire le vaisseau
    /// pour retirer les plans liés aux extensions désormais perdues).
    pub progression_reset: bool,
}

/// Traite l'input de l'écran de paramétrage (touche O) : clavier (ESC =
/// ferme) et clic souris (cases MUSIC / AUTO GENERATE / ANTIALIAS, barre de
/// volume, lignes RENDER / WINDOW / SIZE, RESTART, RESET, RESET PROGRESSION,
/// CLOSE). Les réglages modifiés sont persistés immédiatement. Utilisé par la
/// boucle de jeu et par l'écran titre (`title.rs`). `sounds` est optionnel :
/// absent, musique et volume ne sont pas modifiables.
pub fn handle_settings_input(state: &mut GameState, mut sounds: Option<&mut Sounds>) -> SettingsResult {
    let mut result = SettingsResult::default();
    match settings_box_click(state) {
        SettingsClick::Music => {
            if let Some(snd) = sounds.as_deref_mut() {
                snd.toggle_music();
                let _ = persist::set_bool("music", snd.music_on);
            }
        }
        SettingsClick::AutoGenerate => {
            // pour la session en cours uniquement (non persistée : la
            // génération automatique repart active au lancement)
            state.auto_generate = !state.auto_generate;
        }
        SettingsClick::Volume(fraction) => set_volume_fraction(sounds.as_deref_mut(), fraction),
        SettingsClick::MusicVolume(fraction) => {
            if let Some(snd) = sounds.as_deref_mut() {
                snd.music_volume = fraction.clamp(0.0, 1.0);
                let _ = persist::set_i32("music_volume", (snd.music_volume * 100.0).round() as i32);
                snd.apply_gains();
            }
        }
        SettingsClick::EffectsVolume(fraction) => {
            if let Some(snd) = sounds.as_deref_mut() {
                snd.effects_volume = fraction.clamp(0.0, 1.0);
                let _ = persist::set_i32("effects_volume", (snd.effects_volume * 100.0).round() as i32);
                snd.apply_gains();
            }
        }
        SettingsClick::AmbientVolume(fraction) => {
            if let Some(snd) = sounds.as_deref_mut() {
                snd.ambient_volume = fraction.clamp(0.0, 1.0);
                let _ = persist::set_i32("ambient_volume", (snd.ambient_volume * 100.0).round() as i32);
                snd.apply_gains();
            }
        }
        SettingsClick::RenderStyle => {
            state.render_style = next_render_style(state.render_style);
            let _ = persist::save_render_style(state.render_style as i32);
        }
        SettingsClick::WindowMode => {
            // même cycle que la touche F (fenêtré → zoomé → natif) ; le
            // mode est persisté dans `cycle_view_mode` : le jeu redémarre
            // dans le dernier mode utilisé
            cycle_view_mode(state);
        }
        SettingsClick::WindowSize => {
            state.window_size = next_window_size(state.window_size);
            let _ = persist::save_window_size(state.window_size);
            // en fenêtré, la nouvelle définition s'applique aussitôt ; en
            // plein écran elle prendra effet au retour en fenêtré
            if state.view_mode == ViewMode::Windowed {
                let (w, h) = window_size_dims(state.window_size);
                request_new_screen_size(w, h);
            }
        }
        SettingsClick::Antialias => {
            state.antialias = !state.antialias;
            let _ = persist::set_bool("antialias", state.antialias);
            state.send_message(if state.antialias {
                "ANTIALIAS ON (NEXT LAUNCH)"
            } else {
                "ANTIALIAS OFF"
            });
        }
        SettingsClick::TouchUi => {
            state.touch_ui = !state.touch_ui;
            let _ = persist::set_bool("touch_ui", state.touch_ui);
            crate::touch::set_enabled(state.touch_ui);
        }
        SettingsClick::SavePosition => {
            state.save_position = !state.save_position;
            let _ = persist::set_bool("save_position", state.save_position);
            state.send_message(if state.save_position {
                "SAVE POSITION ON"
            } else {
                "SAVE POSITION OFF"
            });
        }
        SettingsClick::PinEdit => {
            if state.settings_pin_edit {
                // second clic (ou ENTRÉE) : valide la saisie en cours
                confirm_remote_pin(state);
            } else {
                // arme la saisie : le tampon part du code actuel (modifiable)
                state.settings_pin_buffer = state.remote_pin.clone();
                state.settings_pin_edit = true;
            }
        }
        SettingsClick::Restart => result.restart = true,
        SettingsClick::Reset => reset_settings(state, sounds.as_deref_mut()),
        SettingsClick::ResetProgress => {
            scenario::reset_progression(state);
            result.progression_reset = true;
            state.send_message("PROGRESSION RESET");
        }
        SettingsClick::Close => close_and_persist(state),
        SettingsClick::None => {}
    }
    // glisser sur une barre de volume (bouton maintenu) : réglage continu
    // tant que le pointeur reste sur la piste (maître, musique, effets,
    // ambiance)
    if is_mouse_button_down(MouseButton::Left) {
        let l = settings_box_layout();
        let m = mouse_to_game();
        let frac = |track: Rect| ((m.x - track.x) / track.w).clamp(0.0, 1.0);
        if l.volume_track.contains(m) {
            set_volume_fraction(sounds, frac(l.volume_track));
        } else if l.music_volume_track.contains(m) {
            let f = frac(l.music_volume_track);
            if let Some(snd) = sounds {
                snd.music_volume = f;
                let _ = persist::set_i32("music_volume", (f * 100.0).round() as i32);
                snd.apply_gains();
            }
        } else if l.effects_volume_track.contains(m) {
            let f = frac(l.effects_volume_track);
            if let Some(snd) = sounds {
                snd.effects_volume = f;
                let _ = persist::set_i32("effects_volume", (f * 100.0).round() as i32);
                snd.apply_gains();
            }
        } else if l.ambient_volume_track.contains(m) {
            let f = frac(l.ambient_volume_track);
            if let Some(snd) = sounds {
                snd.ambient_volume = f;
                let _ = persist::set_i32("ambient_volume", (f * 100.0).round() as i32);
                snd.apply_gains();
            }
        }
    }
    // Saisie du PIN de la télécommande : les chiffres remplissent le tampon
    // (4 max), RETOUR ARRIÈRE efface, ENTRÉE valide, ÉCHAP annule la saisie
    // (sans fermer l'écran). Les autres clés de l'écran (ESC = fermer) sont
    // neutralisées pendant la saisie.
    if state.settings_pin_edit {
        for key in get_keys_pressed() {
            match key {
                KeyCode::Key0 | KeyCode::Kp0 => push_pin_digit(state, '0'),
                KeyCode::Key1 | KeyCode::Kp1 => push_pin_digit(state, '1'),
                KeyCode::Key2 | KeyCode::Kp2 => push_pin_digit(state, '2'),
                KeyCode::Key3 | KeyCode::Kp3 => push_pin_digit(state, '3'),
                KeyCode::Key4 | KeyCode::Kp4 => push_pin_digit(state, '4'),
                KeyCode::Key5 | KeyCode::Kp5 => push_pin_digit(state, '5'),
                KeyCode::Key6 | KeyCode::Kp6 => push_pin_digit(state, '6'),
                KeyCode::Key7 | KeyCode::Kp7 => push_pin_digit(state, '7'),
                KeyCode::Key8 | KeyCode::Kp8 => push_pin_digit(state, '8'),
                KeyCode::Key9 | KeyCode::Kp9 => push_pin_digit(state, '9'),
                KeyCode::Backspace => {
                    state.settings_pin_buffer.pop();
                }
                KeyCode::Enter | KeyCode::KpEnter => confirm_remote_pin(state),
                KeyCode::Escape => {
                    state.settings_pin_edit = false;
                }
                _ => {}
            }
        }
        return result;
    }
    if is_key_pressed(KeyCode::Escape) {
        close_and_persist(state);
    }
    result
}

/// Ajoute un chiffre au tampon de saisie du PIN (4 chiffres maximum).
pub fn push_pin_digit(state: &mut GameState, digit: char) {
    if state.settings_pin_buffer.len() < 4 {
        state.settings_pin_buffer.push(digit);
    }
}

/// Valide la saisie du PIN de la télécommande : le code (vide = aucune
/// protection) est appliqué à l'état et persisté.
pub fn confirm_remote_pin(state: &mut GameState) {
    let pin = state.settings_pin_buffer.clone();
    state.remote_pin = pin.clone();
    let _ = persist::save_remote_pin(&pin);
    state.settings_pin_edit = false;
    let msg = if pin.is_empty() {
        "REMOTE PIN OFF".to_string()
    } else {
        format!("REMOTE PIN: {pin}")
    };
    state.send_message(&msg);
}

/// Ferme l'écran de paramétrage. (Le mode de déplacement se choisit au
/// magasin de la station et y est persisté à la sélection - rien à
/// réenregistrer ici.)
pub fn close_settings(state: &mut GameState) {
    state.settings_box = false;
}

/// Ferme l'écran de paramétrage (voir `close_settings`).
pub fn close_and_persist(state: &mut GameState) {
    close_settings(state);
}

/// Remet les réglages par défaut (bouton RESET) : musique en marche,
/// génération automatique active, volume 100 %, rendu texturé, fenêtré à
/// 960×540, anticrénelage éteint - les valeurs par défaut ne sont
/// réenregistrées à la fermeture que si elles ont été modifiées pendant
/// l'écran. NB : le mode de déplacement n'est plus un réglage (il se choisit
/// au magasin de la station) - le RESET ne le touche pas.
pub fn reset_settings_fields(state: &mut GameState) {
    state.auto_generate = true;
    state.render_style = RenderStyle::Textured;
    state.window_size = 0;
    state.antialias = false;
    state.touch_ui = true; // interface tactile affichée par défaut
    state.save_position = false; // position du vaisseau non sauvegardée
}

/// Remet les réglages par défaut (bouton RESET) : champs par défaut
/// (`reset_settings_fields`), retour fenêtré à 960×540, musique en marche,
/// volume 100 %, et clés de réglage du fichier de config supprimées - les
/// valeurs par défaut ne sont réenregistrées à la fermeture que si elles ont
/// été modifiées pendant l'écran. NB : la progression d'un scénario à
/// économie (scénario choisi, minerais, modes payés, réputation - clés
/// `scenario`/`prog_*`) n'est pas supprimée : seuls les réglages repartent
/// aux défauts.
pub fn reset_settings(state: &mut GameState, sounds: Option<&mut Sounds>) {
    reset_settings_fields(state);
    apply_view_mode(state, ViewMode::Windowed);
    if state.view_mode == ViewMode::Windowed {
        request_new_screen_size(VIEWPORT_WIDTH as f32, VIEWPORT_HEIGHT as f32);
    }
    if let Some(sounds) = sounds {
        sounds.set_volume(1.0);
        // sous-volumes : retour à 100 % (les clés sont supprimées ci-dessous)
        sounds.music_volume = 1.0;
        sounds.effects_volume = 1.0;
        sounds.ambient_volume = 1.0;
        sounds.apply_gains();
        if !sounds.music_on {
            sounds.toggle_music();
        }
    }
    // seules les clés de réglage sont supprimées - le scénario et sa
    // progression (`scenario`, `prog_*` : minerais, modes payés, réputation,
    // mode de déplacement choisi) survivent au RESET
    for key in [
        "music",
        "auto_generate",
        "volume",
        "music_volume",
        "effects_volume",
        "ambient_volume",
        "render_style",
        "window_size",
        "antialias",
        "touch_ui",
        "save_position",
    ] {
        let _ = persist::delete_key(key);
    }
    crate::touch::set_enabled(state.touch_ui);
}

/// Style de rendu suivant dans le cycle (TEXTURED → COLORED → MESH → …).
pub fn next_render_style(style: RenderStyle) -> RenderStyle {
    match style {
        RenderStyle::Textured => RenderStyle::Colored,
        RenderStyle::Colored => RenderStyle::Mesh,
        RenderStyle::Mesh => RenderStyle::Textured,
    }
}

/// Index de définition de fenêtre suivant dans le cycle (960×540 → 1280×720
/// → … → retour), borné à `WINDOW_SIZES`.
pub fn next_window_size(index: i32) -> i32 {
    (index + 1) % WINDOW_SIZES.len() as i32
}

/// Dimensions `(largeur, hauteur)` de la définition de fenêtre `index`.
pub fn window_size_dims(index: i32) -> (f32, f32) {
    let (w, h) = WINDOW_SIZES[index.clamp(0, WINDOW_SIZES.len() as i32 - 1) as usize];
    (w as f32, h as f32)
}

/// Bascule vers un mode d'affichage donné (bouton RESET) : entre dans le
/// plein écran EWMH si la cible est zoomé/natif, en sort (ClientMessage
/// REMOVE via libX11) sinon - voir `cycle_view_mode`.
pub fn apply_view_mode(state: &mut GameState, target: ViewMode) {
    if state.view_mode == target {
        return;
    }
    match (state.view_mode, target) {
        // fenêtré → plein écran : le chemin de rendu (zoomé ou natif) ne
        // change que la caméra, la bascule EWMH est la même (entrée propre,
        // sans l'unmap/remap de miniquad - voir `render::enter_fullscreen`)
        (ViewMode::Windowed, _) => enter_fullscreen(),
        // déjà en plein écran : seul le chemin de rendu change
        (ViewMode::Zoomed, ViewMode::Native) | (ViewMode::Native, ViewMode::Zoomed) => {}
        // plein écran → fenêtré : REMOVE EWMH (repli : redimensionnement à
        // la définition choisie)
        (_, ViewMode::Windowed)
            if !crate::x11::set_fullscreen(false) => {
                let (w, h) = window_size_dims(state.window_size);
                request_new_screen_size(w, h);
            }
        _ => {}
    }
    state.view_mode = target;
    // le dernier mode utilisé est persisté : le jeu redémarre dedans
    let _ = crate::persist::save_view_mode(target as i32);
}

/// Applique le volume maître depuis une fraction (0..1) de la barre et le
/// persiste. N'écrit le fichier que si la valeur change réellement (glisser
/// sur la barre ne réécrit pas le config à chaque frame).
pub fn set_volume_fraction(sounds: Option<&mut Sounds>, fraction: f32) {
    if let Some(sounds) = sounds {
        let pct = (fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
        let current = (sounds.volume * 100.0).round() as i32;
        if pct != current {
            sounds.set_volume(pct as f32 / 100.0);
            let _ = persist::set_i32("volume", pct);
        }
    }
}
