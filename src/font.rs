//! Police embarquée dans le binaire (`include_bytes!`, aucune dépendance au
//! système d'exploitation → portable sur toutes les plateformes).
//!
//! **DejaVu Sans Mono** (licence Bitstream Vera, voir
//! `assets/fonts/LICENSE-DejaVuSansMono.txt`) est chargée au démarrage et
//! définie comme police par défaut de macroquad (`set_default_font`) : tous
//! les `draw_text` / `measure_text` du jeu utilisent la vraie largeur des
//! glyphes (avance 0.602 em) et un **jeu de caractères étendu** (Latin-1
//! accentué, flèches `→`, coches `✓`… que la police par défaut de macroquad
//! - ProggyClean, purement ASCII - ne possède pas).
//!
//! Pour que les **grilles fixes** du jeu (HUD 8 px par caractère, écran
//! debug, lignes espacées de 16 px) restent valides avec une police plus
//! large, tout le texte est dessiné à l'échelle `scale()` mesurée au
//! chargement :
//!
//! - avance d'un caractère à 16 px = 0.602 × 16 ≈ 9.63 px ; on dessine à
//!   8.0 / 9.63 ≈ 0.831 pour retomber sur la grille **8 px** ;
//! - hauteur de ligne à 16 px ≈ 18.6 × 0.831 ≈ **15.5 px** (< 16 px, les
//!   lignes ne se chevauchent pas).
//!
//! Les deux wrappers `draw_text` / `measure_text` ont les mêmes signatures
//! que celles de macroquad : les appels existants sont inchangés (la
//! résolution de nom dans `render.rs`, `title.rs` et `touch.rs` préfère la
//! version locale via un import explicite).

use macroquad::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

/// Échelle du texte : mesurée au chargement pour que l'avance d'un
/// caractère à 16 px soit de 8 px (grille fixe du jeu, ex `hud_col_x`).
/// Stockée en bits f32 dans un `AtomicU32` (défaut 1.0 avant `init`).
static FONT_SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

fn scale() -> f32 {
    f32::from_bits(FONT_SCALE.load(Ordering::Relaxed))
}

/// Charge la police embarquée, la définit comme police par défaut de
/// macroquad et mesure l'échelle 8 px. À appeler une fois au démarrage,
/// après l'ouverture de la fenêtre (le chargement utilise l'atlas de
/// macroquad).
pub fn init() {
    let font = load_ttf_font_from_bytes(include_bytes!("../assets/fonts/DejaVuSansMono.ttf"))
        .expect("police DejaVu Sans Mono embarquée illisible");
    set_default_font(font);
    // avance réelle d'un caractère à 16 px (police monospace) → échelle pour
    // retomber sur la grille 8 px du jeu
    let w = macroquad::text::measure_text("M", None, 16, 1.0).width;
    if w > 0.0 {
        FONT_SCALE.store((8.0 / w).to_bits(), Ordering::Relaxed);
    }
}

/// Dessine du texte à l'échelle 8 px (même signature que
/// `macroquad::text::draw_text`).
pub fn draw_text(text: &str, x: f32, y: f32, font_size: f32, color: Color) -> TextDimensions {
    macroquad::text::draw_text_ex(
        text,
        x,
        y,
        TextParams {
            font_size: font_size as u16,
            font_scale: scale(),
            color,
            ..Default::default()
        },
    )
}

/// Mesure du texte à l'échelle 8 px (même signature que
/// `macroquad::text::measure_text`).
pub fn measure_text(
    text: &str,
    font: Option<&Font>,
    font_size: u16,
    font_scale: f32,
) -> TextDimensions {
    macroquad::text::measure_text(text, font, font_size, font_scale * scale())
}
