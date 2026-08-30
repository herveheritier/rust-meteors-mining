//! Contrôles du joueur : touches clavier, joystick tactile et
//! télécommande, selon le mode de déplacement (portage des blocs
//! `select case` de `mainLoop`) - portage de `src/game.rs`.

use macroquad::prelude::*;
use crate::audio::Sounds;
use crate::config::*;
use crate::generate::fire_bullet;
use crate::geom::Triangle;
use crate::scenario;
use crate::shape::Shape;
use crate::state::GameState;

/// Front montant de la touche F (mode d'affichage) : vrai une seule fois par
/// pression physique. Plus robuste que `is_key_pressed` seul : quand le
/// serveur X marque un KeyDown comme **répétition** (relâchement perdu
/// pendant la bascule plein écran - `XUnmapWindow/XMapWindow` de miniquad,
/// voir `render::enter_fullscreen`), macroquad n'ajoute pas la touche à
/// `keys_pressed` (il avale la pression) mais `is_key_down` passe quand même
/// à vrai - le front montant rattrape la pression. `state.f_was_down` porte
/// l'état de la frame précédente.
pub fn f_pressed(state: &mut GameState) -> bool {
    let down = is_key_down(KeyCode::F);
    let pressed = is_key_pressed(KeyCode::F) || (down && !state.f_was_down);
    state.f_was_down = down;
    pressed
}

/// Index de la forme **contrôlée** par le joueur : le vaisseau normalement,
/// le cosmonaute EVA quand le vaisseau est détruit (`cosmonaut_active`) - la
/// caméra, la mire et le HUD d'accostage suivent ce pilote (voir `main.rs`).
pub fn pilot_index(state: &GameState) -> usize {
    if state.cosmonaut_active {
        state.eva_cosmonaut as usize
    } else {
        PLAYER_INDEX
    }
}

/// Le joueur donne-t-il une commande de déplacement (flèches ↑/↓/←/→, tous
/// les modes de déplacement) ? Utilisé pour déclencher la rétraction des
/// liens quand le vaisseau démarre de la base (voir `update`).
pub fn player_moving_input() -> bool {
    up_pressed() || down_pressed() || left_pressed() || right_pressed()
}

/// Commandes de déplacement : touche clavier, joystick tactile (`touch.rs`,
/// bas-gauche), télécommande (`remote.rs`, téléphone sur le réseau local) OU
/// manette de jeu (`gamepad.rs`, stick gauche / croix directionnelle) - les
/// quatre pilotent comme les flèches.
pub fn up_pressed() -> bool {
    is_key_down(KeyCode::Up) || crate::touch::up() || crate::remote::up() || crate::gamepad::up()
}

pub fn down_pressed() -> bool {
    is_key_down(KeyCode::Down) || crate::touch::down() || crate::remote::down() || crate::gamepad::down()
}

pub fn left_pressed() -> bool {
    is_key_down(KeyCode::Left) || crate::touch::left() || crate::remote::left() || crate::gamepad::left()
}

pub fn right_pressed() -> bool {
    is_key_down(KeyCode::Right) || crate::touch::right() || crate::remote::right() || crate::gamepad::right()
}

/// Tir : clavier (Shift), bouton de tir tactile (`touch.rs`, bas-droite),
/// télécommande (`remote.rs`) OU manette (bouton A / gâchette droite,
/// `gamepad.rs`).
pub fn fire_pressed() -> bool {
    is_key_down(KeyCode::LeftShift)
        || is_key_down(KeyCode::RightShift)
        || crate::touch::fire()
        || crate::remote::fire()
        || crate::gamepad::fire()
}

/// Contrôles du vaisseau selon `state.moving_mode` (port fidèle des blocs
/// `select case` de `mainLoop`) + tir (Shift gauche/droit, ex
/// `case 42, 54` des modes). REALISTIC reprend INERTIAL mais laisse la
/// vitesse angulaire du vaisseau vivre après le relâchement des propulseurs
/// latéraux.
///
/// NB : les formules QB64 `60*valeur/fps` deviennent `valeur*60*dt`
/// (équivalent à 60 FPS). La convention d'écran (y vers le bas, `-sin` dans
/// `moving_shape`) est reproduite telle quelle, signes compris.
pub fn player_controls(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    sounds: Option<&mut Sounds>,
    dt: f64,
) {
    // vaisseau détruit : le joueur contrôle le cosmonaute EVA éjecté (seul
    // objectif : rejoindre la base) - pas de tir ni de carburant
    if state.cosmonaut_active {
        cosmonaut_controls(state, shapes, dt);
        return;
    }
    state.player.thrust = 0.0;

    // carburant (scénarios à économie) : les poussées avant/arrière sont
    // bloquées quand le réservoir est vide - les rotations restent libres
    let fuel_ok = scenario::fuel_available(state);
    // boost de vitesse (consommable) : la poussée est amplifiée pendant
    // `BOOST_DURATION` (voir `scenario::boost_factor`)
    let boost = scenario::boost_factor(state);

    // (portée dédiée : l'emprunt mutable de `shapes[PLAYER_INDEX]` doit se
    // terminer avant le tir, qui réemprunte tout `shapes`)
    {
    let player = &mut shapes[PLAYER_INDEX];
    match state.moving_mode {
        MOVING_MODE_DIRECTIONAL => {
            // les modes sans inertie angulaire ne laissent pas une ancienne
            // rotation de REALISTIC continuer après un changement de mode
            player.rotation = 0.0;
            if fuel_ok && up_pressed() {
                player.velocity += PLAYER_ACCELERATION * 60.0 * dt * boost;
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
            }
            if right_pressed() {
                player.direction -= PLAYER_ROTATION_SPEED * 60.0 * dt;
                player.orientation = -player.direction;
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                if player.velocity > 0.0 {
                    // peut devenir négatif une frame (comme l'original), puis
                    // sera ramené à 0
                    player.velocity -= PLAYER_ACCELERATION * 60.0 * dt;
                    state.player.revert_thrusted = -5;
                } else {
                    player.velocity = 0.0;
                }
            }
            if left_pressed() {
                player.direction += PLAYER_ROTATION_SPEED * 60.0 * dt;
                player.orientation = -player.direction;
                state.player.rotate_left_thrusted = -5; // jet latéral gauche
            }
        }
        MOVING_MODE_INERTIAL => {
            // INERTIAL tourne à vitesse imposée tant que la touche est tenue
            player.rotation = 0.0;
            if fuel_ok && up_pressed() {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt * boost, player.orientation, 1.0, -1.0);
            }
            if right_pressed() {
                player.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt * boost, player.orientation, -1.0, 1.0);
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if left_pressed() {
                player.orientation -= PLAYER_ROTATION_SPEED * 60.0 * dt;
                state.player.rotate_left_thrusted = -5; // jet latéral gauche
            }
        }
        MOVING_MODE_REALISTIC => {
            // Même poussée vectorielle qu'INERTIAL. Les propulseurs latéraux
            // accélèrent progressivement la rotation ; la vitesse angulaire
            // reste ensuite dans `player.rotation` quand les touches sont
            // relâchées, et la poussée opposée permet de la compenser.
            if fuel_ok && up_pressed() {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt * boost, player.orientation, 1.0, -1.0);
            }
            let rotate_right = right_pressed();
            let rotate_left = left_pressed();
            // Le relâchement ne modifie pas la vitesse angulaire. Une poussée
            // opposée agit comme un frein : elle peut ramener la rotation à
            // zéro, puis la faire repartir dans l'autre sens si elle reste
            // maintenue.
            player.rotation = realistic_rotation_after_input(
                player.rotation,
                rotate_right,
                rotate_left,
                dt,
            );
            if rotate_right {
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt * boost, player.orientation, -1.0, 1.0);
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if rotate_left {
                state.player.rotate_left_thrusted = -5; // jet latéral gauche
            }
        }
        MOVING_MODE_4_WAYS => {
            player.rotation = 0.0;
            if fuel_ok && up_pressed() {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt * boost;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if fuel_ok && right_pressed() {
                let dx = player.direction.cos() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt * boost;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt * boost;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if fuel_ok && left_pressed() {
                let dx = player.direction.cos() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt * boost;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                state.player.rotate_left_thrusted = -5; // jet latéral gauche
            }
        }
        _ => {}
    }
    }

    // tir : Shift gauche/droit (ex `case 42, 54` des quatre modes) - le
    // cooldown `fire` (1/3 s) bloque les tirs suivants ; le scénario
    // consomme les munitions par arme (`try_fire` renvoie le masque des
    // armes qui ont tiré) et bloque le tir quand plus aucune arme n'a de
    // munitions (cooldown non réinitialisé - le tir part dès qu'une arme
    // est armée)
    if fire_pressed() && state.player.fire <= 0.0 {
        let fired = scenario::try_fire(state);
        if fired.iter().any(|&f| f) {
            fire_bullet(shapes, triangles, &fired);
            state.player.fire = PLAYER_FIRE_COOLDOWN;
            state.bullets_fired += 1;
            if let Some(sounds) = sounds {
                sounds.play_bullet();
            }
        }
    }
}

/// Contrôles du **cosmonaute EVA** (vaisseau détruit - voir
/// `activate_cosmonaut`) : la poussée est **vectorielle** (comme le mode
/// INERTIAL du vaisseau) - ↑ exerce une poussée dans l'orientation qui
/// **s'ajoute au vecteur de déplacement** (`thrust_vector`) ; pour changer de
/// direction il faut d'abord **s'orienter** (←/→ font tourner la figure, sans
/// modifier la trajectoire en cours) **puis pousser** : le mouvement dévie
/// progressivement. Un seul propulseur : pas de frein ni de marche arrière. Il
/// faut doser la poussée pour rejoindre la base (`docking`/`rescue_cosmonaut`).
/// Sans tir ni carburant.
pub fn cosmonaut_controls(state: &mut GameState, shapes: &mut [Shape], dt: f64) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return;
    }
    let c = &mut shapes[idx];
    state.player.thrust = 0.0;
    // poussée vectorielle : la poussée (selon l'orientation) s'ajoute au
    // vecteur de déplacement actuel, direction et vitesse recalculées
    if up_pressed() {
        state.player.thrust = 0.1;
        state.player.thrusted = -5;
        thrust_vector(c, PLAYER_ACCELERATION * 60.0 * dt, c.orientation, 1.0, -1.0);
    }
    // orientation seule : la figure tourne, la trajectoire ne change pas
    // (elle ne sera déviée que par une poussée ultérieure)
    if right_pressed() {
        c.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
    }
    if left_pressed() {
        c.orientation -= PLAYER_ROTATION_SPEED * 60.0 * dt;
    }
}

/// Convertit un `KeyCode` macroquad en keycode QB64 (ex `inp(96)`) : codes
/// ASCII pour les lettres, 72/75/77/80 pour les flèches, 42/54 pour les
/// shifts - utilisé par l'affichage I.
pub fn qb_keycode(k: KeyCode) -> i32 {
    match k {
        KeyCode::A => 65,
        KeyCode::B => 66,
        KeyCode::C => 67,
        KeyCode::D => 68,
        KeyCode::E => 69,
        KeyCode::F => 70,
        KeyCode::G => 71,
        KeyCode::H => 72,
        KeyCode::I => 73,
        KeyCode::J => 74,
        KeyCode::K => 75,
        KeyCode::L => 76,
        KeyCode::M => 77,
        KeyCode::N => 78,
        KeyCode::O => 79,
        KeyCode::P => 80,
        KeyCode::Q => 81,
        KeyCode::R => 82,
        KeyCode::S => 83,
        KeyCode::T => 84,
        KeyCode::U => 85,
        KeyCode::V => 86,
        KeyCode::W => 87,
        KeyCode::X => 88,
        KeyCode::Y => 89,
        KeyCode::Z => 90,
        KeyCode::Up => 72,
        KeyCode::Left => 75,
        KeyCode::Right => 77,
        KeyCode::Down => 80,
        KeyCode::LeftShift => 42,
        KeyCode::RightShift => 54,
        KeyCode::Escape => 1,
        KeyCode::Space => 32,
        _ => 0,
    }
}

/// Met à jour la vitesse angulaire du mode REALISTIC (rad/s) : une commande
/// latérale l'accélère progressivement jusqu'à `±PLAYER_ROTATION_SPEED * 60`
/// (la constante d'origine est par frame, comme les autres modes), le
/// relâchement la conserve, et la commande opposée la freine jusqu'à l'arrêt.
pub fn realistic_rotation_after_input(
    current: f64,
    right: bool,
    left: bool,
    dt: f64,
) -> f64 {
    let max_speed = PLAYER_ROTATION_SPEED * 60.0;
    let accel = PLAYER_ROTATION_ACCELERATION * 60.0;
    let direction = match (right, left) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    (current + direction * accel * dt)
        .clamp(-max_speed, max_speed)
}

/// Ajoute une poussée le long de `orientation` (ex blocs INERTIAL de
/// `mainLoop`) : combine la vitesse actuelle avec la poussée, puis recalcule
/// direction/vitesse en polaires.
pub fn thrust_vector(player: &mut Shape, acc: f64, orientation: f64, sx: f64, sy: f64) {
    let dx1 = player.direction.cos() * player.velocity;
    let dy1 = player.direction.sin() * player.velocity;
    let dx2 = orientation.cos() * acc * sx;
    let dy2 = orientation.sin() * acc * sy;
    let dx = dx1 + dx2;
    let dy = dy1 + dy2;
    player.direction = dy.atan2(dx);
    player.velocity = dx.hypot(dy);
}
