//! Boucle de jeu — portage de `mainLoop.bas`.
//!
//! Jalon M2 : input (déplacement du vaisseau, 3 modes), physique, monde
//! torique (via `moving_shape`), pause, plein écran.
//! Jalon M3 : météores — génération en jeu (touche G + automatique), détection
//! de collisions (SAT) avec choc élastique, résolution (destruction de
//! triangles, débris, messages, centres). Les balles (M4), l'accostage (M5)
//! et les sons (M4+) viendront ensuite.

use macroquad::prelude::*;
use ::rand::Rng;

use crate::audio::Sounds;
use crate::config::*;
use crate::garbage::{generate_garbages, moving_garbage, Garbage};
use crate::generate::{create_alien, create_gem, create_shape, fire_bullet};
use crate::geom::{Point, Triangle};
use crate::render::{camera_for, choice_box_layout, cycle_view_mode, help_box_layout, mouse_to_game};
use crate::shape::{compute_shape_center, detect_collision, moving_shape, resolve_elastic_collision, Shape};
use crate::state::{Element, GameState, ViewMode};

/// Action demandée par la boucle de jeu pour la frame courante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Continue,
}

/// Traite l'input, les contrôles joueur, la physique et les collisions pour
/// une frame. Renvoie l'action demandée et la caméra (centrée joueur) à
/// utiliser pour le rendu.
///
/// Ordre fidèle à `mainLoop` : input → contrôles → compteurs de poussée →
/// remise à zéro des indicateurs de collision → déplacements (formes gelées
/// en pause, débris toujours actifs — comportement de l'original) →
/// collisions → caméra → génération automatique.
pub fn update(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    garbages: &mut Vec<Garbage>,
    elements: &mut [Element],
    rng: &mut impl Rng,
    // Sons du jeu — `None` (tests) pour un `update` silencieux.
    mut sounds: Option<&mut Sounds>,
    dt: f64,
) -> (Action, Point) {
    // FPS mesurés (affichés au HUD, utilisés par les messages en Phase 4)
    state.fps = get_fps();

    // Caméra de la frame précédente — utilisée par la touche G comme
    // l'original (qui lit `camera` calculée à l'itération précédente).
    let mut camera = camera_for(state, &shapes[PLAYER_INDEX]);

    // ESC : quitter
    if is_key_pressed(KeyCode::Escape) {
        return (Action::Quit, camera);
    }

    // dernier keycode pressé (affiché par le mode I, ex `keycode = inp(96)`
    // de l'original : codes ASCII pour les lettres, 72/75/77/80 pour les
    // flèches, 42/54 pour les shifts)
    if let Some(k) = get_keys_pressed().iter().next() {
        state.last_keycode = qb_keycode(*k);
    }

    // Fenêtre d'aide ouverte (touche S) : le monde est gelé, seul le bouton
    // CLOSE est traité (ex boucle bloquante de `windowUtils_help`).
    if state.help_box {
        if help_box_click() {
            state.help_box = false;
        }
        return (Action::Continue, camera);
    }

    // Boîte de choix DOCK STATION ouverte : le monde est gelé — seuls les
    // clics sur UNLOAD/CLOSE sont traités (ex boucle bloquante de
    // `windowUtils_choiceBox`).
    if state.dock_box {
        if choice_box_click() {
            // NB : l'original ignore le choix (`r%` non utilisé) — le cargo
            // est vidé de toute façon à l'accostage (branche « else » de
            // `docking`, frame suivante).
            state.dock_box = false;
        }
        return (Action::Continue, camera);
    }

    // F : cycle des modes d'affichage — fenêtré → plein écran zoomé (render
    // target étirée) → plein écran natif (définition réelle, sans buffer) →
    // fenêtré.
    if is_key_pressed(KeyCode::F) {
        cycle_view_mode(state);
        state.send_message(match state.view_mode {
            ViewMode::Windowed => "WINDOWED",
            ViewMode::Zoomed => "FULLSCREEN (ZOOMED)",
            ViewMode::Native => "FULLSCREEN (NATIVE)",
        });
    }

    // M : bascule la musique (ex `M : mute music` de mainLoop)
    if is_key_pressed(KeyCode::M) {
        if let Some(sounds) = sounds.as_deref_mut() {
            sounds.toggle_music();
            state.send_message(if sounds.music_on { "MUSIC ON" } else { "MUSIC OFF" });
        }
    }

    // P : pause
    if is_key_pressed(KeyCode::P) {
        state.paused = !state.paused;
    }

    // A : génération automatique des météores (ex `autoGenerateShape%`)
    if is_key_pressed(KeyCode::A) {
        state.auto_generate = !state.auto_generate;
    }

    // G : génère un météore près du vaisseau (ex `mainLoop`) : à
    // `VIEWPORT_WIDTH \ 4` à droite du joueur, immobile.
    if is_key_pressed(KeyCode::G) {
        let idx = create_shape(state, shapes, triangles, camera, elements, rng);
        let player = &shapes[PLAYER_INDEX];
        shapes[idx].position = Point::new(player.position.x + VIEWPORT_WIDTH / 4.0, player.position.y);
        shapes[idx].velocity = 0.0;
    }

    // C : crée un alien (ex `mainLoop` → `createAlien`)
    if is_key_pressed(KeyCode::C) {
        create_alien(shapes, triangles);
    }

    // S : fenêtre d'aide (ex `showKeys%` → `help`)
    if is_key_pressed(KeyCode::S) {
        state.help_box = true;
    }

    // D : affichage des données des formes (ex `showData%`)
    if is_key_pressed(KeyCode::D) {
        state.show_data = !state.show_data;
    }

    // I : affichage des informations de debug (ex `showInfo%`)
    if is_key_pressed(KeyCode::I) {
        state.show_info = !state.show_info;
    }

    // cooldown de tir (décrémenté à chaque frame, comme l'original)
    if state.player.fire > 0.0 {
        state.player.fire -= dt;
    }

    // contrôles joueur selon le mode de déplacement (inclut le tir + son)
    player_controls(state, shapes, triangles, sounds.as_deref_mut(), dt);

    // compteurs de poussée : -5 à la pression, +1 par frame jusqu'à 0 —
    // la flamme (et le son, Phase 4) persiste ~5 frames après relâchement.
    if state.player.thrusted != 0 {
        state.player.thrusted += 1;
    }
    if state.player.revert_thrusted != 0 {
        state.player.revert_thrusted += 1;
    }

    // physique + collisions (détection, résolution, sons d'impact)
    collisions(state, shapes, triangles, garbages, elements, rng, sounds, dt);

    // accostage à la station (ex « detect return to the base ») — peut ouvrir
    // la boîte UNLOAD/CLOSE, auquel cas le reste de la frame est gelé
    docking(state, shapes, elements);
    if state.dock_box {
        return (Action::Continue, camera);
    }

    // caméra fraîche (après déplacements et résolution, comme l'original)
    camera = camera_for(state, &shapes[PLAYER_INDEX]);

    // supprime les balles sorties de la zone de dessin (ex mainLoop)
    delete_out_of_range_bullets(state, shapes, triangles, camera);

    // formes vivantes (avec nettoyage des formes « oubliées » par la logique)
    let alive_shapes = count_alive_shapes(shapes, triangles);

    // génération automatique : 5 % de chance par frame tant que la limite
    // n'est pas atteinte (ex `mainLoop`) — non gelée par la pause, comme
    // l'original.
    if state.auto_generate && alive_shapes < state.max_meteor_shapes && rng.gen::<f64>() > 0.95 {
        create_shape(state, shapes, triangles, camera, elements, rng);
    }

    (Action::Continue, camera)
}

/// Physique et collisions pour une frame (ex sections « moves shapes »,
/// « moves garbages », « detects collisions », « resolves collisions » de
/// `mainLoop`).
///
/// Seuls les déplacements des formes sont gelés en pause ; les débris, les
/// collisions et la génération automatique continuent (comportement exact de
/// l'original).
fn collisions(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    garbages: &mut Vec<Garbage>,
    elements: &mut [Element],
    rng: &mut impl Rng,
    mut sounds: Option<&mut Sounds>,
    dt: f64,
) {
    // remet à zéro les indicateurs de collision de tous les triangles
    for t in triangles.iter_mut() {
        t.collid = false;
    }

    // déplace les formes (gelées en pause) puis les débris (toujours actifs)
    if !state.paused {
        for i in 0..shapes.len() {
            moving_shape(&mut shapes[i], triangles, &state.world, dt);
        }
    }
    for g in garbages.iter_mut() {
        moving_garbage(g, dt);
    }

    // ─── détection de collisions (paires de formes proches) ────────────────
    let mut elastic_pairs: Vec<(usize, usize)> = Vec::new();
    for i in 0..shapes.len() {
        // pas de détection si la forme n'est pas un collider
        if !shapes[i].is_collider {
            continue;
        }
        for j in (i + 1)..shapes.len() {
            if !shapes[j].is_collider {
                continue;
            }
            // pas de collision entre le vaisseau et ses propres balles
            if shapes[i].who_i_am == WHOIAM_PLAYER && shapes[j].who_i_am == WHOIAM_BULLET {
                continue;
            }
            // détection seulement si les formes ne sont pas trop éloignées
            let x_dist = (shapes[i].position.x + shapes[i].center.x
                - shapes[j].position.x
                - shapes[j].center.x)
                .abs();
            let y_dist = (shapes[i].position.y + shapes[i].center.y
                - shapes[j].position.y
                - shapes[j].center.y)
                .abs();
            let sum_radius = shapes[i].radius + shapes[j].radius;
            if x_dist <= sum_radius && y_dist <= sum_radius {
                if detect_collision(&shapes[i], &shapes[j], triangles) {
                    // pas de choc élastique entre une gemme et (vaisseau ou
                    // météore), ni avec la station
                    let no_elastic = (shapes[i].who_i_am == WHOIAM_GEM
                        && (shapes[j].who_i_am == WHOIAM_PLAYER || shapes[j].who_i_am == WHOIAM_METEOR))
                        || (shapes[j].who_i_am == WHOIAM_GEM
                            && (shapes[i].who_i_am == WHOIAM_PLAYER || shapes[i].who_i_am == WHOIAM_METEOR))
                        || shapes[i].who_i_am == WHOIAM_STATION
                        || shapes[j].who_i_am == WHOIAM_STATION;
                    if !no_elastic {
                        elastic_pairs.push((i, j));
                    }
                }
            }
        }
    }

    // choc élastique entre les paires en collision (après détection, pour
    // éviter les emprunts simultanés de `shapes`)
    for (i, j) in elastic_pairs {
        let (left, right) = shapes.split_at_mut(j);
        resolve_elastic_collision(&mut left[i], &mut right[0]);
    }

    // ─── résolution des collisions ─────────────────────────────────────────
    let mut previous_shape_index = -1i32;
    for i in 0..triangles.len() {
        if !triangles[i].collid {
            continue;
        }
        let shape_index = triangles[i].shape_index as usize;
        let who = shapes[shape_index].who_i_am;
        let collid_by = triangles[i].collid_by;

        if collid_by == WHOIAM_PLAYER
            && who == WHOIAM_GEM
            && state.player.cargo_qty < state.player.cargo_size
        {
            // ramassage d'une gemme (M4 — nécessite les balles pour créer des
            // gemmes) : détruite, son élément est compté dans la soute
            shapes[shape_index].life = 0;
            triangles[i].life = 0;
            let element = triangles[i].element as usize;
            if element < elements.len() {
                elements[element].count += 1;
            }
            state.player.cargo_qty += 1;
            if let Some(sounds) = sounds.as_mut() {
                sounds.play_gem();
            }
            if state.player.cargo_qty >= state.player.cargo_size {
                state.send_message("YOUR LOADING BAY IS FULL, YOU MUST UNLOAD IT AT THE STATION");
            }
        } else if collid_by == WHOIAM_GEM && who == WHOIAM_PLAYER {
            // déjà résolu côté gemme (cargaison pleine)
        } else if collid_by == WHOIAM_STATION && who == WHOIAM_PLAYER {
            // accostage (M5)
        } else if who == WHOIAM_STATION {
            // la station est indestructible
        } else {
            triangles[i].life = 0;
            shapes[shape_index].life -= 1;
            if who == WHOIAM_PLAYER {
                state.send_message("YOUR SPACESHIP IS DAMAGED, THE STATION CAN CARRY OUT REPAIRS");
                state.send_message("REPAIRS ARE NOT FREE OF CHARGE");
            }
            // collision vaisseau/gemme non résolue parce que soute pleine
            if collid_by == WHOIAM_PLAYER && who == WHOIAM_GEM {
                state.send_message("YOU CANNOT TAKE ANY ADDITIONAL RESOURCES, UNLOAD AT THE STATION");
            }
            // si le joueur détruit un météore, la limite de météores augmente
            // (ex mainLoop : compteur + « R+1 » affiché — le bonus flottant
            // et les sons arrivent en M4)
            if collid_by == WHOIAM_BULLET
                && who == WHOIAM_METEOR
                && shapes[shape_index].life <= 0
            {
                state.meteors_destroyed += 1;
                if state.max_meteor_shapes < SHAPES_COUNT {
                    state.max_meteor_shapes += 1;
                }
            }
            // débris + son d'impact : volume selon la distance au vaisseau,
            // un des 10 sons d'explosion au hasard (ex mainLoop :
            // `v! = (1 - dist/diag)^3`, `shexp(s%)`)
            if let Some(sounds) = sounds.as_mut() {
                let dx = shapes[shape_index].position.x - shapes[PLAYER_INDEX].position.x;
                let dy = shapes[shape_index].position.y - shapes[PLAYER_INDEX].position.y;
                let dist = dx.hypot(dy);
                let v = (1.0 - dist / WORLD_WIDTH.hypot(WORLD_HEIGHT)).powi(3) as f32;
                sounds.play_explosion(rng, v);
            }
            generate_garbages(garbages, &triangles[i], shapes, rng);
            if triangles[i].element > 0 {
                if collid_by == WHOIAM_BULLET && triangles[i].element > 0 {
                    // une gemme apparaît (M4) — copie du triangle source
                    let source = triangles[i];
                    create_gem(shapes, triangles, elements, &source, rng);
                }
            }
        }

        // recalcule le centre de la forme (une fois par forme, pas pour la
        // station)
        if shape_index as i32 != previous_shape_index {
            if who != WHOIAM_STATION {
                compute_shape_center(&mut shapes[shape_index], triangles);
            }
            previous_shape_index = shape_index as i32;
        }
    }
}

/// Compte les formes vivantes et nettoie les formes « oubliées » par la
/// logique (tous leurs triangles morts → forme morte), ex la boucle de
/// dessin de `mainLoop`. Le vaisseau (index 0) n'est ni compté ni nettoyé.
fn count_alive_shapes(shapes: &mut [Shape], triangles: &[Triangle]) -> i32 {
    let mut alive = 0;
    for i in 1..shapes.len() {
        if shapes[i].life <= 0 {
            continue;
        }
        let mut t = 0;
        for j in shapes[i].first_triangle..=shapes[i].last_triangle {
            if triangles[j].life > 0 {
                t += 1;
            }
        }
        if t == 0 {
            shapes[i].life = 0;
            continue;
        }
        alive += 1;
    }
    alive
}

/// Détecte le retour à la base (ex « detect return to the base » de
/// `mainLoop`) : à moins de 5 px de la station, le vaisseau est docké.
///
/// NB : comme l'original, le choix UNLOAD/CLOSE de la boîte est ignoré
/// (`r%` non utilisé) — le cargo est vidé de toute façon à l'accostage.
fn docking(state: &mut GameState, shapes: &mut [Shape], elements: &mut [Element]) {
    if (shapes[PLAYER_INDEX].position.x - shapes[STATION_INDEX].position.x).abs() < STATION_DOCK_DISTANCE
        && (shapes[PLAYER_INDEX].position.y - shapes[STATION_INDEX].position.y).abs() < STATION_DOCK_DISTANCE
    {
        if state.player_at_station == 0 {
            state.player_at_station = -1;
            state.player_enter_station = -1;
            shapes[PLAYER_INDEX].velocity = 0.0;
            state.send_message("YOU ARE DOCKED AT THE STATION");
            state.dock_box = true; // ouvre la boîte UNLOAD/CLOSE (jeu gelé)
        } else {
            // déchargement : éléments vidés, soute vidée
            for e in elements.iter_mut() {
                e.count = 0;
            }
            state.player.cargo_qty = 0;
            state.player_enter_station = 0;
            state.player_at_station = -1;
        }
    } else {
        if state.player_at_station == -1 {
            // NB : typo de l'original (« LIVING » pour « LEAVING ») conservée
            state.send_message("YOU ARE LIVING THE STATION");
        }
        state.player_at_station = 0;
    }
}

/// Supprime les balles sorties de la zone de dessin (ex « deletes bullets
/// outer of draw area » de `mainLoop`) : forme et triangles tués, compteur
/// `bullets_lost` incrémenté par triangle.
fn delete_out_of_range_bullets(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    camera: Point,
) {
    for i in 0..shapes.len() {
        if shapes[i].life <= 0 {
            continue;
        }
        if shapes[i].who_i_am == WHOIAM_BULLET {
            let mut pt = Point::new(shapes[i].position.x + camera.x, shapes[i].position.y + camera.y);
            pt.normalize_world(&state.world);
            if pt.x < DRAW_MINX || pt.x > DRAW_MAXX || pt.y < DRAW_MINY || pt.y > DRAW_MAXY {
                shapes[i].life = 0;
                for j in shapes[i].first_triangle..=shapes[i].last_triangle {
                    triangles[j].life = 0;
                    state.bullets_lost += 1;
                }
            }
        }
    }
}

/// Détecte un clic sur la boîte de choix UNLOAD/CLOSE (ex
/// `windowUtils_choiceBox`). Renvoie `true` si un bouton a été cliqué (la
/// boîte se ferme alors — le choix lui-même est ignoré, comme l'original).
fn choice_box_click() -> bool {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return false;
    }
    let (unload_rect, close_rect) = choice_box_layout();
    let m = mouse_to_game();
    unload_rect.contains(m) || close_rect.contains(m)
}

/// Détecte un clic sur le bouton CLOSE de la fenêtre d'aide (ex
/// `windowUtils_help`).
fn help_box_click() -> bool {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return false;
    }
    let close_rect = help_box_layout();
    let m = mouse_to_game();
    close_rect.contains(m)
}

/// Contrôles du vaisseau selon `state.moving_mode` (port fidèle des trois
/// blocs `select case` de `mainLoop`) + tir (Shift gauche/droit, ex
/// `case 42, 54` des trois modes).
///
/// NB : les formules QB64 `60*valeur/fps` deviennent `valeur*60*dt`
/// (équivalent à 60 FPS). La convention d'écran (y vers le bas, `-sin` dans
/// `moving_shape`) est reproduite telle quelle, signes compris.
fn player_controls(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    sounds: Option<&mut Sounds>,
    dt: f64,
) {
    state.player.thrust = 0.0;

    // (portée dédiée : l'emprunt mutable de `shapes[PLAYER_INDEX]` doit se
    // terminer avant le tir, qui réemprunte tout `shapes`)
    {
    let player = &mut shapes[PLAYER_INDEX];
    match state.moving_mode {
        MOVING_MODE_DIRECTIONAL => {
            if is_key_down(KeyCode::Up) {
                player.velocity += PLAYER_ACCELERATION * 60.0 * dt;
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
            }
            if is_key_down(KeyCode::Right) {
                player.direction -= PLAYER_ROTATION_SPEED * 60.0 * dt;
                player.orientation = -player.direction;
            }
            if is_key_down(KeyCode::Down) {
                if player.velocity > 0.0 {
                    // peut devenir négatif une frame (comme l'original), puis
                    // sera ramené à 0
                    player.velocity -= PLAYER_ACCELERATION * 60.0 * dt;
                    state.player.revert_thrusted = -5;
                } else {
                    player.velocity = 0.0;
                }
            }
            if is_key_down(KeyCode::Left) {
                player.direction += PLAYER_ROTATION_SPEED * 60.0 * dt;
                player.orientation = -player.direction;
            }
        }
        MOVING_MODE_INERTIAL => {
            if is_key_down(KeyCode::Up) {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, 1.0, -1.0);
            }
            if is_key_down(KeyCode::Right) {
                player.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
            }
            if is_key_down(KeyCode::Down) {
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, -1.0, 1.0);
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if is_key_down(KeyCode::Left) {
                player.orientation -= PLAYER_ROTATION_SPEED * 60.0 * dt;
            }
        }
        MOVING_MODE_4_WAYS => {
            if is_key_down(KeyCode::Up) {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if is_key_down(KeyCode::Right) {
                let dx = player.direction.cos() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if is_key_down(KeyCode::Down) {
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if is_key_down(KeyCode::Left) {
                let dx = player.direction.cos() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
        }
        _ => {}
    }
    }

    // tir : Shift gauche/droit (ex `case 42, 54` des trois modes) — le
    // cooldown `fire` (1/3 s) bloque les tirs suivants
    if (is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift))
        && state.player.fire <= 0.0
    {
        fire_bullet(shapes, triangles);
        state.player.fire = PLAYER_FIRE_COOLDOWN;
        state.bullets_fired += 1;
        if let Some(sounds) = sounds {
            sounds.play_bullet();
        }
    }
}

/// Convertit un `KeyCode` macroquad en keycode QB64 (ex `inp(96)`) : codes
/// ASCII pour les lettres, 72/75/77/80 pour les flèches, 42/54 pour les
/// shifts — utilisé par l'affichage I.
fn qb_keycode(k: KeyCode) -> i32 {
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

/// Ajoute une poussée le long de `orientation` (ex blocs INERTIAL de
/// `mainLoop`) : combine la vitesse actuelle avec la poussée, puis recalcule
/// direction/vitesse en polaires.
fn thrust_vector(player: &mut Shape, acc: f64, orientation: f64, sx: f64, sy: f64) {
    let dx1 = player.direction.cos() * player.velocity;
    let dy1 = player.direction.sin() * player.velocity;
    let dx2 = orientation.cos() * acc * sx;
    let dy2 = orientation.sin() * acc * sy;
    let dx = dx1 + dx2;
    let dy = dy1 + dy2;
    player.direction = dy.atan2(dx);
    player.velocity = dx.hypot(dy);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::default_elements;
    use ::rand::SeedableRng;
    use ::rand_chacha::ChaCha12Rng;

    fn seed() -> ChaCha12Rng {
        ChaCha12Rng::seed_from_u64(42)
    }

    /// Construit une forme simple à `n` triangles (index `first..=last`).
    fn test_shape(who: i32, first: usize, last: usize, x: f64, y: f64) -> Shape {
        let mut s = Shape::default();
        s.who_i_am = who;
        s.is_collider = true;
        s.first_triangle = first;
        s.last_triangle = last;
        s.life = (last - first + 1) as i32;
        s.radius = 10.0;
        s.position = Point::new(x, y);
        s
    }

    /// Construit un triangle simple (sommets locaux fixes) rattaché à une
    /// forme, avec ses positions réelles calculées.
    fn test_triangle(id: i32, shape_index: i32, x: f64, y: f64) -> Triangle {
        let mut t = Triangle::default();
        t.id = id;
        t.shape_index = shape_index;
        t.create(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0));
        t.position = Point::new(x, y);
        // positions réelles : sommet local + position de la forme (forme à
        // l'origine, sans rotation)
        t.real_a = Point::new(t.a.x + x, t.a.y + y);
        t.real_b = Point::new(t.b.x + x, t.b.y + y);
        t.real_c = Point::new(t.c.x + x, t.c.y + y);
        t.real_center = Point::new(t.center.x + x, t.center.y + y);
        t.real_min = Point::new(t.real_a.x.min(t.real_b.x).min(t.real_c.x), t.real_a.y.min(t.real_b.y).min(t.real_c.y));
        t.real_max = Point::new(t.real_a.x.max(t.real_b.x).max(t.real_c.x), t.real_a.y.max(t.real_b.y).max(t.real_c.y));
        t
    }

    #[test]
    fn thrust_vector_recomputes_polar() {
        let mut player = Shape::default();
        player.direction = 0.0;
        player.velocity = 1.0;
        // poussée le long de l'orientation (0 = +x), sx=1, sy=-1
        thrust_vector(&mut player, 0.05, 0.0, 1.0, -1.0);
        assert!((player.velocity - 1.05).abs() < 1e-12);
        assert_eq!(player.direction, 0.0);

        // orientation perpendiculaire : la direction dévie (signe -sin)
        let mut player = Shape::default();
        player.direction = 0.0;
        player.velocity = 1.0;
        thrust_vector(&mut player, 0.05, std::f64::consts::FRAC_PI_2, 1.0, -1.0);
        assert!((player.velocity - 1.0f64.hypot(0.05)).abs() < 1e-12);
        assert!(player.direction < 0.0); // atan2(-0.05, 1)
    }

    #[test]
    fn directional_deceleration_clamps_at_zero() {
        // vérifie la sémantique du bloc Down du mode directionnel : à vitesse
        // nulle, la décélération ne passe pas en négatif.
        let mut player = Shape::default();
        player.velocity = 0.0;
        if player.velocity > 0.0 {
            player.velocity -= PLAYER_ACCELERATION * 60.0 * (1.0 / 60.0);
        } else {
            player.velocity = 0.0;
        }
        assert_eq!(player.velocity, 0.0);
    }

    #[test]
    fn collision_destroys_triangles_and_spawns_debris() {
        // deux météores de deux triangles qui se chevauchent → les triangles
        // meurent, les formes perdent de la vie, des débris apparaissent
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_METEOR, 0, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 0, 0.0, 0.0),
            test_triangle(2, 1, 2.0, 2.0),
            test_triangle(3, 1, 2.0, 2.0),
        ];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        // chevauchement : distance (2,2) < rayon cumulé (20)
        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(triangles[0].life, 0);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(triangles[2].life, 0);
        assert_eq!(triangles[3].life, 0);
        assert_eq!(shapes[0].life, 0);
        assert_eq!(shapes[1].life, 0);
        // des débris apparaissent (le comptage exact dépend de la réutilisation
        // des slots morts, comme l'original : vie 0 → slot réutilisé)
        assert!(garbages.len() >= 2 * GARBAGE_PER_TRIANGLE);
        assert!(garbages.len() <= 4 * GARBAGE_PER_TRIANGLE);
        // le centre est recalculé (vie <= 0 → inchangé, pas de panique)
        compute_shape_center(&mut shapes[0], &triangles);
    }

    #[test]
    fn station_is_indestructible() {
        // la station est un collider mais ses triangles ne meurent jamais
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_STATION, 0, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 0, 0.0, 0.0),
            test_triangle(2, 1, 2.0, 2.0),
            test_triangle(3, 1, 2.0, 2.0),
        ];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // triangles de la station intacts, le météore est détruit
        assert_eq!(triangles[0].life, 1);
        assert_eq!(triangles[1].life, 1);
        assert_eq!(triangles[2].life, 0);
        assert_eq!(triangles[3].life, 0);
        assert_eq!(shapes[0].life, 2);
        assert_eq!(shapes[1].life, 0);
    }

    #[test]
    fn player_meteor_collision_damages_player() {
        // le joueur perd un triangle et reçoit le message de dégâts
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 0, 0.0, 0.0),
            test_triangle(2, 1, 2.0, 2.0),
            test_triangle(3, 1, 2.0, 2.0),
        ];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(shapes[0].life, 0);
        assert!(state.message_queue.contains("YOUR SPACESHIP IS DAMAGED"));
    }

    #[test]
    fn elastic_collision_swaps_velocity_between_equal_masses() {
        // choc élastique entre deux météores de même taille (2 triangles,
        // masse = 2-1 = 1 sans le +1 de l'original) : la vitesse est
        // transférée d'un météore à l'autre
        let mut a = test_shape(WHOIAM_METEOR, 0, 1, 0.0, 0.0);
        let mut b = test_shape(WHOIAM_METEOR, 2, 3, 1.0, 0.0);
        a.velocity = 1.0;
        a.direction = 0.0;
        b.velocity = 0.0;

        resolve_elastic_collision(&mut a, &mut b);

        // masses identiques : v1 → v2 et v2 → v1 (direction +x)
        assert!(a.velocity < 0.01);
        assert!(b.velocity > 0.99);
    }

    #[test]
    fn bullet_destroying_mineral_triangle_creates_gem() {
        // une balle détruit un triangle avec élément : une gemme apparaît
        // (ex mainLoop : `if element > 0 and collidBy = BULLET → createGem`).
        // La détection pose les indicateurs `collid`/`collid_by` (resetés en
        // début de frame) : on utilise une vraie balle qui chevauche le
        // météore.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_BULLET, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[1].element = 1; // GOLD
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // une gemme a été créée (forme supplémentaire WHOIAM_GEM)
        let gem = shapes.iter().find(|s| s.who_i_am == WHOIAM_GEM);
        assert!(gem.is_some(), "une gemme doit apparaître");
        assert_eq!(gem.unwrap().element, 1);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(shapes[1].life, 0);
    }

    #[test]
    fn player_collects_gem_into_cargo() {
        // le vaisseau ramasse une gemme : élément compté, soute remplie
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_GEM, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[1].element = 2; // IRON
        triangles[1].collid = true;
        triangles[1].collid_by = WHOIAM_PLAYER;
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(elements[2].count, 1);
        assert_eq!(state.player.cargo_qty, 1);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(shapes[1].life, 0);
    }

    #[test]
    fn docking_opens_choice_box_and_velocity_zeroed() {
        // le joueur revient à la station après être parti (playerAtStation = 0,
        // ex mainLoop : la boîte n'apparaît qu'au retour) → boîte ouverte,
        // vitesse mise à 0, message envoyé
        let mut state = GameState::new();
        state.player_at_station = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        shapes[0].velocity = 3.0;
        let mut elements = default_elements();
        elements[1].count = 4;

        docking(&mut state, &mut shapes, &mut elements);

        assert!(state.dock_box);
        assert_eq!(shapes[0].velocity, 0.0);
        assert_eq!(state.player_at_station, -1);
        assert!(state.message_queue.contains("YOU ARE DOCKED AT THE STATION"));
        // le cargo n'est pas vidé tant que la boîte est ouverte (le
        // déchargement arrive à la frame suivante, comme l'original)
        assert_eq!(elements[1].count, 4);
    }

    #[test]
    fn initial_docking_at_game_start_unloads() {
        // au démarrage, le joueur EST à la station avec playerAtStation = -1 :
        // la boîte ne s'ouvre pas, le cargo est directement vidé (original)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut elements = default_elements();
        elements[1].count = 4;
        state.player.cargo_qty = 4;

        docking(&mut state, &mut shapes, &mut elements);

        assert!(!state.dock_box);
        assert_eq!(elements[1].count, 0);
        assert_eq!(state.player.cargo_qty, 0);
    }

    #[test]
    fn docking_unloads_cargo_next_frame() {
        // boîte refermée : à la frame suivante, le cargo est vidé
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        state.player_at_station = -1; // déjà docké (boîte refermée)
        let mut elements = default_elements();
        elements[1].count = 4;
        state.player.cargo_qty = 4;

        docking(&mut state, &mut shapes, &mut elements);

        assert!(!state.dock_box);
        assert_eq!(elements[1].count, 0);
        assert_eq!(state.player.cargo_qty, 0);
        assert_eq!(state.player_enter_station, 0);
    }

    #[test]
    fn leaving_station_sends_message() {
        // joueur loin de la station : message « LIVING » (typo de l'original)
        let mut state = GameState::new();
        state.player_at_station = -1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 100.0, 100.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut elements = default_elements();

        docking(&mut state, &mut shapes, &mut elements);

        assert_eq!(state.player_at_station, 0);
        assert!(state.message_queue.contains("YOU ARE LIVING THE STATION"));
    }

    #[test]
    fn out_of_range_bullets_are_deleted() {
        // une balle sortie de la zone de dessin est tuée, triangles et
        // compteur `bullets_lost` mis à jour
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_BULLET, 1, 1, 2000.0, 0.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2000.0, 0.0)];
        let camera = Point::new(0.0, 0.0);

        delete_out_of_range_bullets(&mut state, &mut shapes, &mut triangles, camera);

        assert_eq!(shapes[1].life, 0);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(state.bullets_lost, 1);
        // le joueur n'est pas touché
        assert_eq!(shapes[0].life, 1);
        assert_eq!(state.bullets_lost, 1);
    }

    #[test]
    fn count_alive_shapes_cleans_forgotten_shapes() {
        // index 0 = vaisseau (jamais compté ni nettoyé) ; une forme dont tous
        // les triangles sont morts est « oubliée » → vie mise à 0 et plus
        // comptée ; l'autre reste vivante
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 2, 0.0, 0.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 0.0, 0.0),
            test_triangle(2, 2, 0.0, 0.0),
        ];
        triangles[1].life = 0;
        let alive = count_alive_shapes(&mut shapes, &triangles);
        assert_eq!(alive, 1); // seul le météore d'index 2 est vivant
        assert_eq!(shapes[1].life, 0); // nettoyé
        assert_eq!(shapes[2].life, 1); // toujours vivant
    }
}
