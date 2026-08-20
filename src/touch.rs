//! Interface tactile (jeu sur écran tactile / mobile) : un **joystick
//! virtuel** en bas à gauche pilote le vaisseau (↑/↓ = poussée avant/arrière,
//! ←/→ = rotation — les mêmes commandes que les flèches) et un **bouton de
//! tir** en bas à droite déclenche les canons (comme Shift). Les deux
//! fonctionnent au doigt (touches macroquad) et, à défaut de doigt actif, à
//! la souris (clic maintenu) pour tester sur un poste de travail.
//!
//! Le jeu interroge l'état via `up()/down()/left()/right()/fire()` — combiné
//! au clavier dans `game.rs` — et la boucle principale affiche les contrôles
//! avec `draw()` (`main.rs`), masqués quand une boîte recouvre l'écran.

use macroquad::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{VIEWPORT_HEIGHT, VIEWPORT_WIDTH};

/// Interface tactile active ? (case TOUCH UI de l'écran de paramétrage,
/// persistée — clé `touch_ui`, voir `main.rs` et `handle_settings_input`).
/// Éteinte : les contrôles ne sont ni dessinés ni pris en compte (le jeu se
/// pilote au clavier seul — sinon des zones invisibles resteraient cliquables
/// à la souris). Synchronisée à partir de `GameState.touch_ui`.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Active/coupe l'interface tactile (réglage TOUCH UI de l'écran O).
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

// ─── Disposition (coordonnées du jeu, vue 960×540) ──────────────────────────

/// Centre du joystick (coin bas-gauche).
const JOY_CENTER: Vec2 = vec2(118.0, VIEWPORT_HEIGHT as f32 - 112.0);
/// Rayon de la zone de saisie : un doigt posé dans ce rayon pilote le
/// joystick (le socle dessiné est plus petit, la zone est généreuse).
const JOY_TOUCH_RADIUS: f32 = 140.0;
/// Rayon du socle dessiné.
const JOY_BASE_RADIUS: f32 = 56.0;
/// Déplacement maximal du manche (le vecteur du joystick y est borné).
const JOY_KNOB_RADIUS: f32 = 30.0;
/// Zone morte centrale : un doigt posé près du centre ne pilote rien.
const DEADZONE: f32 = 14.0;
/// Centre du bouton de tir (coin bas-droit).
const FIRE_CENTER: Vec2 = vec2(VIEWPORT_WIDTH as f32 - 96.0, VIEWPORT_HEIGHT as f32 - 96.0);
/// Rayon de la zone de saisie du bouton de tir.
const FIRE_TOUCH_RADIUS: f32 = 86.0;
/// Rayon du bouton dessiné.
const FIRE_RADIUS: f32 = 52.0;

/// L'interface tactile est-elle active (`set_enabled`) ?
fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Points actifs (en coordonnées du jeu) : les doigts posés, ou la souris
/// maintenue quand aucun doigt n'est actif (repli desktop — les touches
/// simulant la souris, on n'ajoute la souris que sans doigt pour ne pas
/// compter deux fois le même contact). Vide quand l'interface tactile est
/// désactivée (`set_enabled(false)`) : rien ne pilote ni ne tire.
fn active_points() -> Vec<Vec2> {
    if !enabled() {
        return Vec::new();
    }
    let mut pts: Vec<Vec2> = touches()
        .iter()
        .map(|t| crate::render::screen_to_game(t.position))
        .collect();
    if pts.is_empty() && is_mouse_button_down(MouseButton::Left) {
        let (x, y) = mouse_position();
        pts.push(crate::render::screen_to_game(vec2(x, y)));
    }
    pts
}

/// Vecteur courant du joystick (borné à `JOY_KNOB_RADIUS`), `None` si aucun
/// doigt/souris n'est dans la zone de saisie. Plusieurs doigts dans la zone :
/// le plus proche du centre pilote.
fn joystick_vector() -> Option<Vec2> {
    let mut best: Option<(f32, Vec2)> = None;
    for p in active_points() {
        let d = p - JOY_CENTER;
        let dist = d.length();
        if dist > JOY_TOUCH_RADIUS {
            continue;
        }
        if best.map_or(true, |(bd, _)| dist < bd) {
            best = Some((dist, d));
        }
    }
    best.map(|(_, d)| d.clamp_length_max(JOY_KNOB_RADIUS))
}

/// Vecteur du joystick hors zone morte : `Some(v)` si le manche est
/// suffisamment écarté du centre pour piloter.
fn joystick_axis() -> Option<Vec2> {
    let v = joystick_vector()?;
    if v.length() < DEADZONE {
        return None;
    }
    Some(v)
}

/// Poussée avant (↑ du clavier) : manche tiré vers le haut.
pub fn up() -> bool {
    match joystick_axis() {
        Some(v) => v.y < 0.0 && v.y.abs() >= v.x.abs() * 0.5,
        None => false,
    }
}

/// Poussée arrière / frein (↓) : manche tiré vers le bas.
pub fn down() -> bool {
    match joystick_axis() {
        Some(v) => v.y > 0.0 && v.y.abs() >= v.x.abs() * 0.5,
        None => false,
    }
}

/// Rotation gauche (←) : manche tiré à gauche.
pub fn left() -> bool {
    match joystick_axis() {
        Some(v) => v.x < 0.0 && v.x.abs() >= v.y.abs() * 0.5,
        None => false,
    }
}

/// Rotation droite (→) : manche tiré à droite.
pub fn right() -> bool {
    match joystick_axis() {
        Some(v) => v.x > 0.0 && v.x.abs() >= v.y.abs() * 0.5,
        None => false,
    }
}

/// Tir (Shift) : un doigt/souris posé sur le bouton de tir.
pub fn fire() -> bool {
    active_points()
        .iter()
        .any(|p| p.distance(FIRE_CENTER) <= FIRE_TOUCH_RADIUS)
}

/// Couleur RGBA courte (composantes 0..1).
fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::new(r, g, b, a)
}

/// Petit triangle directionnel (▲/▼/◀/▶ — la police par défaut n'a pas ces
/// glyphes, ils sont dessinés) : pointe à `len` px de `center` le long de
/// `dir` (vecteur unitaire), base 16 px en retrait, demi-largeur 6 px.
fn draw_arrow(center: Vec2, dir: Vec2, len: f32, color: Color) {
    let tip = center + dir * len;
    let back = center + dir * (len - 16.0);
    let side = vec2(-dir.y, dir.x) * 6.0;
    draw_triangle(tip, back + side, back - side, color);
}

/// Dessine le joystick (bas-gauche) et le bouton de tir (bas-droite) —
/// semi-transparents, par-dessus le jeu. À appeler pendant le jeu seulement
/// (boîtes fermées, voir `main.rs`).
pub fn draw() {
    if !enabled() {
        return;
    }
    // ── joystick ────────────────────────────────────────────────────────────
    let knob = joystick_vector().unwrap_or_default();
    let active = knob.length() >= DEADZONE;
    // socle + anneau
    draw_circle(JOY_CENTER.x, JOY_CENTER.y, JOY_BASE_RADIUS, rgba(1.0, 1.0, 1.0, 0.12));
    draw_circle_lines(
        JOY_CENTER.x,
        JOY_CENTER.y,
        JOY_BASE_RADIUS,
        2.0,
        rgba(1.0, 1.0, 1.0, 0.30),
    );
    // flèches directionnelles sur le socle
    let arrow_len = JOY_BASE_RADIUS * 0.62;
    let arrow_color = rgba(1.0, 1.0, 1.0, 0.45);
    draw_arrow(JOY_CENTER, vec2(0.0, -1.0), arrow_len, arrow_color);
    draw_arrow(JOY_CENTER, vec2(0.0, 1.0), arrow_len, arrow_color);
    draw_arrow(JOY_CENTER, vec2(-1.0, 0.0), arrow_len, arrow_color);
    draw_arrow(JOY_CENTER, vec2(1.0, 0.0), arrow_len, arrow_color);
    // manche (suit le doigt)
    let k = JOY_CENTER + knob;
    draw_circle(
        k.x,
        k.y,
        JOY_KNOB_RADIUS,
        rgba(1.0, 1.0, 1.0, if active { 0.55 } else { 0.30 }),
    );
    draw_circle_lines(k.x, k.y, JOY_KNOB_RADIUS, 2.0, rgba(1.0, 1.0, 1.0, 0.60));

    // ── bouton de tir ───────────────────────────────────────────────────────
    let pressed = fire();
    let base = if pressed {
        rgba(1.0, 0.25, 0.25, 0.55)
    } else {
        rgba(1.0, 0.30, 0.30, 0.22)
    };
    let ring = if pressed {
        rgba(1.0, 1.0, 1.0, 0.90)
    } else {
        rgba(1.0, 0.55, 0.55, 0.55)
    };
    draw_circle(FIRE_CENTER.x, FIRE_CENTER.y, FIRE_RADIUS, base);
    draw_circle_lines(FIRE_CENTER.x, FIRE_CENTER.y, FIRE_RADIUS, 3.0, ring);
    let label = "FIRE";
    let w = measure_text(label, None, 14, 1.0).width;
    draw_text(
        label,
        FIRE_CENTER.x - w / 2.0,
        FIRE_CENTER.y + 5.0,
        14.0,
        rgba(1.0, 1.0, 1.0, if pressed { 1.0 } else { 0.75 }),
    );
}
