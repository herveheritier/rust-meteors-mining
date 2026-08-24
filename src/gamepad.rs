//! Manette de jeu (gamepad) en complément du clavier / joystick tactile /
//! télécommande (`touch.rs`, `remote.rs`).
//!
//! macroquad 0.4 n'offre **aucune** API manette (miniquad 0.4.11 :
//! « gamepads soon » dans `input.rs`) : on passe par la crate `gilrs`, qui
//! lit les manettes directement (evdev sur Linux, XInput sur Windows, IOKit
//! sur macOS) via un thread d'événements interne. Sur le **web** (WASM),
//! gilrs ne compile pas : ce module fournit des stubs vides (`up()`/`down()`/
//! `left()`/`right()`/`fire()` → faux) - le clavier et le joystick tactile
//! restent utilisables.
//!
//! API alignée sur `touch.rs`/`remote.rs` : les commandes de déplacement et
//! le tir sont exposés en booléens, combinés par `input.rs` avec les autres
//! sources. Le **stick gauche** pilote le vaisseau (directionnel, mort-zone
//! `DEAD_ZONE`) et les boutons A/RT (ou B, gâchette droite) tirent.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use gilrs::{Axis, Button, Gilrs};
    use std::sync::{Mutex, MutexGuard, Once};

    /// Mort-zone du stick gauche (en dessous, la direction est neutre).
    pub const DEAD_ZONE: f64 = 0.4;

    static INIT: Once = Once::new();
    static GILRS: Mutex<Option<Gilrs>> = Mutex::new(None);

    fn gilrs() -> MutexGuard<'static, Option<Gilrs>> {
        INIT.call_once(|| {
            // pas de manette branchée : `Gilrs::new` échoue seulement pour une
            // erreur interne - aucun panique, on reste sans gamepad
            *GILRS.lock().unwrap() = Gilrs::new().ok();
        });
        GILRS.lock().unwrap()
    }

    /// À appeler à chaque frame (tête de la boucle de jeu) : gilrs met à jour
    /// l'état interne des manettes à la lecture de ses événements - sans ce
    /// poll, `is_pressed`/`axis_data` resteraient figés sur l'état initial.
    pub fn poll() {
        if let Some(g) = gilrs().as_mut() {
            while g.next_event().is_some() {}
        }
    }

    fn axis_value(a: Axis) -> f64 {
        gilrs()
            .as_ref()
            .and_then(|g| g.gamepads().find_map(|(_, gp)| gp.axis_data(a).map(|d| d.value() as f64)))
            .unwrap_or(0.0)
    }

    fn button_down(b: Button) -> bool {
        gilrs()
            .as_ref()
            .is_some_and(|g| g.gamepads().any(|(_, gp)| gp.is_pressed(b)))
    }

    pub fn up() -> bool {
        axis_value(Axis::LeftStickY) < -DEAD_ZONE || button_down(Button::DPadUp)
    }

    pub fn down() -> bool {
        axis_value(Axis::LeftStickY) > DEAD_ZONE || button_down(Button::DPadDown)
    }

    pub fn left() -> bool {
        axis_value(Axis::LeftStickX) < -DEAD_ZONE || button_down(Button::DPadLeft)
    }

    pub fn right() -> bool {
        axis_value(Axis::LeftStickX) > DEAD_ZONE || button_down(Button::DPadRight)
    }

    /// Tir : bouton A (South) ou gâchette droite (comme les jeux de tir).
    pub fn fire() -> bool {
        button_down(Button::South) || button_down(Button::RightTrigger)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{down, fire, left, poll, right, up};

#[cfg(target_arch = "wasm32")]
mod wasm_stub {
    /// Stub : pas de manette sur le web (clavier + tactile seulement).
    pub fn poll() {}
    pub fn up() -> bool { false }
    pub fn down() -> bool { false }
    pub fn left() -> bool { false }
    pub fn right() -> bool { false }
    pub fn fire() -> bool { false }
}

#[cfg(target_arch = "wasm32")]
pub use wasm_stub::{down, fire, left, poll, right, up};