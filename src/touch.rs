//! Interface tactile (jeu sur écran tactile / mobile) : un **joystick
//! virtuel** en bas à gauche pilote le vaisseau (↑/↓ = poussée avant/arrière,
//! ←/→ = rotation - les mêmes commandes que les flèches) et un **bouton de
//! tir** en bas à droite déclenche les canons (comme Shift). Les deux
//! fonctionnent au doigt (touches macroquad) et aussi à la **souris** (clic
//! maintenu - poste de travail et version web) : la souris est un point de
//! contrôle permanent tant que le bouton gauche est enfoncé (dédupliqué d'un
//! éventuel événement tactile du même geste), et une pression dans la zone du
//! joystick « saisit » le manche - on peut sortir de la zone sans couper la
//! commande, jusqu'au relâchement.
//!
//! Le jeu interroge l'état via `up()/down()/left()/right()/fire()` - combiné
//! au clavier dans `game.rs` - et la boucle principale affiche les contrôles
//! avec `draw()` (`main.rs`), masqués quand une boîte recouvre l'écran.

use macroquad::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::{VIEWPORT_HEIGHT, VIEWPORT_WIDTH};
use crate::font::{draw_text, measure_text};

/// Interface tactile active ? (case TOUCH UI de l'écran de paramétrage,
/// persistée - clé `touch_ui`, voir `main.rs` et `handle_settings_input`).
/// Éteinte : les contrôles ne sont ni dessinés ni pris en compte (le jeu se
/// pilote au clavier seul - sinon des zones invisibles resteraient cliquables
/// à la souris). Synchronisée à partir de `GameState.touch_ui`.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// La souris a « saisi » le joystick : une pression commencée dans la zone de
/// saisie verrouille le manche à la souris (le vecteur reste borné au manche)
/// même si le curseur sort ensuite de la zone - jusqu'au relâchement. Sans
/// cela, un glissement au-delà du rayon de saisie couperait la commande en
/// plein virage. Spécifique à la souris (les doigts ont leurs points propres).
static JOY_GRABBED: AtomicBool = AtomicBool::new(false);

/// Active/coupe l'interface tactile (réglage TOUCH UI de l'écran O).
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// L'interface tactile est-elle active (`set_enabled`) ? Lue par
/// `hud::commands_button_click` : le bouton COMMANDES du HUD n'ouvre son
/// panneau que quand l'interface tactile est affichée.
pub fn is_enabled() -> bool {
    enabled()
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

/// Point de contrôle de la souris (repli desktop/web) : le bouton gauche
/// maintenu, quelle que soit la position du curseur (le joystick et le bouton
/// de tir filtrent ensuite selon leurs zones). Une pression commencée dans la
/// zone du joystick « saisit » le manche (`JOY_GRABBED`) : la souris garde la
/// commande même si elle sort de la zone, jusqu'au relâchement. Renvoie la
/// position souris en coordonnées du jeu, ou `None` si le bouton n'est pas
/// enfoncé.
fn mouse_point() -> Option<Vec2> {
    if !is_mouse_button_down(MouseButton::Left) {
        JOY_GRABBED.store(false, Ordering::Relaxed);
        return None;
    }
    let m = crate::render::mouse_to_game();
    if m.distance(JOY_CENTER) <= JOY_TOUCH_RADIUS {
        JOY_GRABBED.store(true, Ordering::Relaxed);
    }
    Some(m)
}

/// Points actifs (en coordonnées du jeu) : les doigts posés, plus la souris
/// maintenue (repli desktop/web - voir `mouse_point`). La souris est TOUJOURS
/// ajoutée quand le bouton gauche est enfoncé, sans dépendre de `touches()`
/// vide : sur certains navigateurs une souris émet aussi des événements
/// tactiles (le repli conditionné à l'absence de doigt était alors bloqué et
/// les contrôles restaient inertes). Un même contact souris/tactile (double
/// événement du même geste) est dédupliqué. Vide quand l'interface tactile est
/// désactivée (`set_enabled(false)`) : rien ne pilote ni ne tire.
fn active_points() -> Vec<Vec2> {
    if !enabled() {
        return Vec::new();
    }
    let mut pts: Vec<Vec2> = touches()
        .iter()
        .map(|t| crate::render::screen_to_game(t.position))
        .collect();
    if let Some(m) = mouse_point() {
        // même contact souris + tactile (navigateur qui émet les deux pour un
        // seul geste) : ne pas compter deux fois le même point
        pts.retain(|p| p.distance(m) > 2.0);
        pts.push(m);
    }
    pts
}

/// Vecteur courant du joystick (borné à `JOY_KNOB_RADIUS`), `None` si aucun
/// doigt/souris n'est dans la zone de saisie. Plusieurs doigts dans la zone :
/// le plus proche du centre pilote. Manche déjà saisi par la souris
/// (`JOY_GRABBED`) : la commande reste active hors de la zone (vecteur borné).
fn joystick_vector() -> Option<Vec2> {
    let grabbed = JOY_GRABBED.load(Ordering::Relaxed);
    let mut best: Option<(f32, Vec2)> = None;
    for p in active_points() {
        let d = p - JOY_CENTER;
        let dist = d.length();
        // zone de saisie, ou manche saisi par la souris (sortie de zone sans
        // couper la commande)
        if dist > JOY_TOUCH_RADIUS && !grabbed {
            continue;
        }
        if best.is_none_or(|(bd, _)| dist < bd) {
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

/// Petit triangle directionnel (▲/▼/◀/▶ - dessinés plutôt qu'affichés en
/// texte pour une taille/forme contrôlée) : pointe à `len` px de `center` le
/// long de `dir` (vecteur unitaire), base 16 px en retrait, demi-largeur 6 px.
fn draw_arrow(center: Vec2, dir: Vec2, len: f32, color: Color) {
    let tip = center + dir * len;
    let back = center + dir * (len - 16.0);
    let side = vec2(-dir.y, dir.x) * 6.0;
    draw_triangle(tip, back + side, back - side, color);
}

/// Dessine le joystick (bas-gauche) et le bouton de tir (bas-droite) -
/// semi-transparents, par-dessus le jeu. À appeler pendant le jeu seulement
/// (boîtes fermées, voir `main.rs`). Le joystick et le bouton FIRE se
/// pilotent aussi à la souris (clic maintenu, `mouse_point`) : le survol de
/// leurs zones de saisie éclaircit les contrôles pour le montrer.
pub fn draw() {
    if !enabled() {
        return;
    }
    // survol souris (coordonnées du jeu) : éclaircit les contrôles quand le
    // curseur entre dans une zone de saisie - la souris pilote le jeu
    let m = crate::render::mouse_to_game();
    let joy_hover = m.distance(JOY_CENTER) <= JOY_TOUCH_RADIUS;
    let fire_hover = m.distance(FIRE_CENTER) <= FIRE_TOUCH_RADIUS;
    // ── joystick ────────────────────────────────────────────────────────────
    let knob = joystick_vector().unwrap_or_default();
    let active = knob.length() >= DEADZONE;
    // socle + anneau (plus clairs au survol souris : la zone répond)
    draw_circle(
        JOY_CENTER.x,
        JOY_CENTER.y,
        JOY_BASE_RADIUS,
        rgba(1.0, 1.0, 1.0, if joy_hover { 0.20 } else { 0.12 }),
    );
    draw_circle_lines(
        JOY_CENTER.x,
        JOY_CENTER.y,
        JOY_BASE_RADIUS,
        2.0,
        rgba(1.0, 1.0, 1.0, if joy_hover { 0.55 } else { 0.30 }),
    );
    // flèches directionnelles sur le socle
    let arrow_len = JOY_BASE_RADIUS * 0.62;
    let arrow_color = rgba(1.0, 1.0, 1.0, if joy_hover { 0.75 } else { 0.45 });
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
    } else if fire_hover {
        // survol souris : le bouton répond au clic maintenu
        rgba(1.0, 0.75, 0.75, 0.75)
    } else {
        rgba(1.0, 0.55, 0.55, 0.55)
    };
    let label_alpha = if pressed {
        1.0
    } else if fire_hover {
        0.90
    } else {
        0.75
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
        rgba(1.0, 1.0, 1.0, label_alpha),
    );
}
