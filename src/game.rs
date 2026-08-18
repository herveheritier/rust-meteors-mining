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
use crate::cosmonaut::{animate_eva_cosmonaut, COSMONAUTE_EVA_PARK};
use crate::garbage::{generate_garbages, moving_garbage, Garbage};
use crate::generate::{create_alien, create_gem, create_shape, fire_bullet, release_meteor_minerals};
use crate::persist;
use crate::scenario;
use crate::geom::{Point, Triangle};
use crate::render::{
    camera_for, choice_box_layout, cycle_view_mode, help_box_layout, mouse_to_game, settings_box_layout,
    workshop_box_layout,
};
use crate::shape::{
    compute_real_positions, compute_shape_center, detect_collision, moving_shape, resolve_elastic_collision,
    Shape,
};
use crate::state::{Element, GameState, RenderStyle, ViewMode};

/// Action demandée par la boucle de jeu pour la frame courante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Relance le jeu (bouton RESTART de l'écran de paramétrage, ex après
    /// un changement d'anticrénelage) : `main.rs` relance l'exécutable puis
    /// quitte.
    Restart,
    Continue,
}

/// Index de la forme **contrôlée** par le joueur : le vaisseau normalement,
/// le cosmonaute EVA quand le vaisseau est détruit (`cosmonaut_active`) — la
/// caméra, la mire et le HUD d'accostage suivent ce pilote (voir `main.rs`).
pub fn pilot_index(state: &GameState) -> usize {
    if state.cosmonaut_active {
        state.eva_cosmonaut as usize
    } else {
        PLAYER_INDEX
    }
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
    // l'original (qui lit `camera` calculée à l'itération précédente). Elle
    // suit le pilote : le vaisseau, ou le cosmonaute EVA quand le vaisseau
    // est détruit.
    let mut camera = camera_for(state, &shapes[pilot_index(state)]);

    // Écran de paramétrage ouvert (touche O) : le monde est gelé et seul
    // l'input de l'écran est traité (voir `handle_settings_input`). Un clic
    // sur RESTART demande la relance du jeu.
    if state.settings_box {
        let restart = handle_settings_input(state, sounds.as_deref_mut());
        let action = if restart { Action::Restart } else { Action::Continue };
        return (action, camera);
    }

    // ESC : quitter
    if is_key_pressed(KeyCode::Escape) {
        return (Action::Quit, camera);
    }

    // Game over (scénario Survival, dernière vie perdue) : le monde est gelé
    // — seules les touches de quitter (ESC, ci-dessus) restent actives ; le
    // HUD affiche GAME OVER.
    if state.game_over {
        return (Action::Continue, camera);
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

    // Animation d'accostage (3 s, avant la boîte DOCK STATION) : le monde est
    // gelé — le vaisseau pivote vers la droite (orientation 0) tout en se
    // recentrant au centre de la station (voir `advance_dock_animation` et
    // `render::draw_docking_line`).
    if state.dock_anim > 0.0 {
        advance_dock_animation(state, shapes, triangles, dt);
        return (Action::Continue, camera);
    }

    // Le vaisseau démarre de la base (lancement ou respawn : liens attachés à
    // quai, mire cachée — voir `state.dock_links`) : dès que le joueur donne
    // une commande de déplacement (flèches, tous modes), les liens se
    // rétractent (même animation qu'au départ après CLOSE), puis le vaisseau
    // est libre.
    if state.dock_links && player_moving_input() {
        release_links(state);
    }

    // Rétraction des liens d'accostage au départ (CLOSE de la boîte ou
    // démarrage de la base) : le monde est gelé — le vaisseau reste au centre,
    // les 4 traits néon se rétractent vers le bord intérieur de l'anneau
    // (voir `advance_dock_retract` et `render::draw_docking_line`), puis le
    // vaisseau est libre.
    if state.dock_retract > 0.0 {
        advance_dock_retract(state, shapes, triangles, dt);
        return (Action::Continue, camera);
    }

    // Boîte de choix DOCK STATION ouverte : le monde est gelé — seuls les
    // clics sur UNLOAD / REFUEL/REARM / UPGRADES / CLOSE sont traités (ex
    // boucle bloquante de `windowUtils_choiceBox`). UNLOAD décharge la soute
    // (minerais disponibles pour REFUEL/REARM juste après), REFUEL/REARM
    // achète carburant + munitions (`scenario::purchase_supplies`) et
    // UPGRADES ouvre l'atelier d'amélioration du vaisseau (scénario à
    // économie) ; la boîte ne se ferme qu'avec CLOSE, pour décharger puis se
    // ravitailler dans le même accostage.
    if state.dock_box {
        match choice_box_click(state) {
            ChoiceClick::None => {}
            ChoiceClick::Unload => {
                // déchargement immédiat — NB : l'original ignore le choix
                // (`r%` non utilisé) et vide la soute de toute façon à
                // l'accostage (branche « else » de `docking`, frame
                // suivante) ; ici il est anticipé pour financer le
                // ravitaillement du même accostage
                scenario::unload_cargo(state, elements);
                for e in elements.iter_mut() {
                    e.count = 0;
                }
                state.player.cargo_qty = 0;
                // la progression (minerais) est persistée au déchargement
                let _ = scenario::save_progression(state);
            }
            ChoiceClick::Refuel => {
                // ravitaillement manuel (plus d'achat automatique au
                // déchargement) : pleins si les minerais suffisent, sinon
                // message « NOT ENOUGH MINERALS » ; les minerais dépensés
                // sont persistés (sans quoi un ravitaillement suivi d'une
                // sortie serait gratuit au lancement suivant)
                scenario::purchase_supplies(state);
                let _ = scenario::save_progression(state);
            }
            ChoiceClick::Upgrades => {
                // ouvre l'atelier d'amélioration (la boîte réapparaît en
                // fermant l'atelier — on reste accosté)
                state.dock_box = false;
                state.workshop_box = true;
            }
            ChoiceClick::Close => {
                // quitte l'accostage : les liens néon se rétractent
                // (animation de `DOCK_RETRACT_DURATION`, monde gelé)
                undock(state);
            }
        }
        return (Action::Continue, camera);
    }

    // Atelier d'amélioration du vaisseau ouvert (bouton UPGRADES de la boîte
    // DOCK STATION, scénario à économie) : le monde est gelé — les clics sur
    // les lignes d'extension (achat contre minerais, `scenario::buy_upgrade`)
    // et sur CLOSE (retour à la boîte DOCK STATION, toujours accosté) sont
    // traités.
    if state.workshop_box {
        match workshop_box_click() {
            WorkshopClick::None => {}
            WorkshopClick::BuyFuel => buy_upgrade_and_save(state, scenario::UpgradeTrackId::Fuel),
            WorkshopClick::BuyAmmo => buy_upgrade_and_save(state, scenario::UpgradeTrackId::Ammo),
            WorkshopClick::BuyCargo => buy_upgrade_and_save(state, scenario::UpgradeTrackId::Cargo),
            WorkshopClick::Close => {
                state.workshop_box = false;
                state.dock_box = true;
            }
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

    // M : bascule la musique (ex `M : mute music` de mainLoop) — persistée
    if is_key_pressed(KeyCode::M) {
        if let Some(sounds) = sounds.as_deref_mut() {
            sounds.toggle_music();
            state.send_message(if sounds.music_on { "MUSIC ON" } else { "MUSIC OFF" });
            let _ = persist::set_bool("music", sounds.music_on);
        }
    }

    // P : pause
    if is_key_pressed(KeyCode::P) {
        state.paused = !state.paused;
    }

    // A : génération automatique des météores (ex `autoGenerateShape%`) —
    // pour la session en cours uniquement (repart active au lancement, voir
    // `main.rs` — non persistée)
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

    // O : écran de paramétrage (choix du mode de déplacement du vaisseau) —
    // on mémorise le mode courant pour n'annoncer le changement au HUD qu'à
    // la fermeture, s'il a été modifié ; le focus clavier part sur le mode
    // courant.
    if is_key_pressed(KeyCode::O) {
        state.settings_previous_mode = state.moving_mode;
        state.settings_focus = state.moving_mode;
        state.settings_box = true;
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

    // invulnérabilité post-respawn (scénario Survival) : décompte à chaque
    // frame (comme le cooldown de tir) — à 0, le vaisseau redevient vulnérable
    if state.invulnerable > 0.0 {
        state.invulnerable = (state.invulnerable - dt).max(0.0);
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

    // animation des membres du cosmonaute EVA : bras et jambes qui **s'agitent
    // pendant la poussée** puis retombent au repos (`cosmonaut::animate_eva_cosmonaut`)
    // — avant la physique : `moving_shape` recalcule les positions réelles des
    // triangles animés dans la foulée. Garé (vaisseau intact), il revient au repos.
    if state.eva_cosmonaut >= 0 {
        let eva = state.eva_cosmonaut as usize;
        let thrusting = state.cosmonaut_active && state.player.thrusted != 0;
        animate_eva_cosmonaut(&mut shapes[eva], triangles, thrusting, get_time(), dt);
    }

    // scénario à économie : le carburant est consommé tant que le moteur
    // est allumé (flamme avant/arrière) — annonce OUT OF FUEL à la rupture.
    // Pas en mode cosmonaute EVA (le vaisseau est détruit, le carburant ne
    // sert plus — la combinaison ne brûle pas le réservoir)
    if !state.cosmonaut_active {
        scenario::consume_fuel(state, dt);
    }

    // physique + collisions (détection, résolution, sons d'impact)
    collisions(state, shapes, triangles, garbages, elements, rng, sounds, dt);

    // guide d'accostage : la mire ne s'affiche que lors du RETOUR à la base
    // (voir `update_docking_guide`) — avant `docking`, qui peut déclencher
    // l'animation d'accostage (et couper le guide). Le pilote suit le
    // cosmonaute EVA quand le vaisseau est détruit.
    let pilot = pilot_index(state);
    update_docking_guide(
        state,
        shapes[pilot].position,
        shapes[STATION_INDEX].position,
        shapes[STATION_INDEX].radius,
    );

    // accostage à la station (ex « detect return to the base ») — peut ouvrir
    // la boîte UNLOAD/CLOSE, auquel cas le reste de la frame est gelé. Quand
    // le vaisseau est détruit, c'est le retour du cosmonaute EVA qui est
    // détecté (secours : vaisseau reconstruit, voir `rescue_cosmonaut`)
    docking(state, shapes, triangles, elements);
    if state.dock_box {
        return (Action::Continue, camera);
    }

    // caméra fraîche (après déplacements et résolution, comme l'original)
    camera = camera_for(state, &shapes[pilot_index(state)]);

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
        } else if who == WHOIAM_PLAYER && scenario::has_survival(state) {
            // scénario Survival : le bouclier encaisse les impacts (le
            // triangle du vaisseau n'est pas tué) ; s'il est percé, le
            // vaisseau est détruit — une vie est perdue et il respawne à la
            // station (bouclier rechargé par le scénario), ou la partie est
            // terminée en dernière vie (le monde se gèle, HUD GAME OVER)
            let shield_before = state.resources.shield;
            let lives_before = state.resources.lives;
            match scenario::player_hit(state, 1.0) {
                scenario::PlayerHit::Absorbed => {}
                scenario::PlayerHit::Destroyed(_) => respawn_player(state, shapes, triangles),
                scenario::PlayerHit::GameOver => {
                    // dernière vie perdue : le vaisseau reste détruit
                    triangles[i].life = 0;
                    shapes[PLAYER_INDEX].life = 0;
                }
            }
            // la progression Survival (vies, bouclier) est persistée quand un
            // impact l'a modifiée (pas à chaque impact absorbé par
            // l'invulnérabilité post-respawn, qui ne change rien)
            if state.resources.shield != shield_before || state.resources.lives != lives_before {
                let _ = scenario::save_progression(state);
            }
        } else if collid_by == WHOIAM_METEOR && who == WHOIAM_GEM {
            // un météore percute une gemme : il l'absorbe — la gemme
            // disparaît entièrement et la quantité de minerai du météore
            // augmente (`minerals`, libérée si le météore est lui-même
            // détruit par un autre météore). Le météore le plus proche de la
            // gemme est celui qui l'a percutée (`collid_by` ne porte que le
            // type, pas l'index). Une seule fois par gemme (toute la gemme
            // est tuée au premier triangle).
            if shapes[shape_index].life > 0 {
                let gem_pos = shapes[shape_index].position;
                if let Some(meteor) = nearest_meteor(shapes, gem_pos) {
                    shapes[meteor].minerals += 1;
                }
                shapes[shape_index].life = 0;
                for j in shapes[shape_index].first_triangle..=shapes[shape_index].last_triangle {
                    triangles[j].life = 0;
                }
            }
        } else if collid_by == WHOIAM_GEM && who == WHOIAM_METEOR {
            // déjà résolu côté gemme (absorption) : le météore n'est pas
            // endommagé en avalant la gemme
        } else {
            triangles[i].life = 0;
            shapes[shape_index].life -= 1;
            if who == WHOIAM_PLAYER {
                state.send_message("YOUR SPACESHIP IS DAMAGED, THE STATION CAN CARRY OUT REPAIRS");
                state.send_message("REPAIRS ARE NOT FREE OF CHARGE");
                // vaisseau détruit (jeu libre/Progression — le Survival a son
                // propre respawn) : le cosmonaute est éjecté à la position du
                // crash — le joueur le contrôle pour rejoindre la base (une
                // seule fois : `cosmonaut_active`)
                if shapes[shape_index].life <= 0 && !state.cosmonaut_active {
                    activate_cosmonaut(state, shapes, triangles);
                }
            }
            // collision vaisseau/gemme non résolue parce que soute pleine
            if collid_by == WHOIAM_PLAYER && who == WHOIAM_GEM {
                state.send_message("YOU CANNOT TAKE ANY ADDITIONAL RESOURCES, UNLOAD AT THE STATION");
            }
            // si le joueur détruit un météore, la limite de météores augmente
            // (ex mainLoop : compteur + « R+1 » affiché — le bonus flottant
            // et les sons arrivent en M4) et la réputation du scénario
            // augmente (d'autant plus que la précision de tir est bonne)
            if collid_by == WHOIAM_BULLET
                && who == WHOIAM_METEOR
                && shapes[shape_index].life <= 0
            {
                state.meteors_destroyed += 1;
                scenario::on_meteor_destroyed(state);
                // la réputation est persistée à chaque astéroïde détruit
                let _ = scenario::save_progression(state);
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
            // un triangle minéralisé d'un MÉTÉORE détruit par un missile
            // libère son minerai : une gemme apparaît (le minerai n'est pas
            // détruit avec le météore). Un missile qui touche directement
            // une gemme, elle, la détruit — pas de nouvelle gemme : c'est le
            // seul cas de destruction de minerai (`who == WHOIAM_GEM` n'entre
            // pas ici).
            if triangles[i].element > 0 && who == WHOIAM_METEOR {
                if collid_by == WHOIAM_BULLET && triangles[i].element > 0 {
                    let source = triangles[i];
                    create_gem(shapes, triangles, elements, &source, rng);
                    if shapes[shape_index].minerals > 0 {
                        shapes[shape_index].minerals -= 1;
                    }
                }
            }
            // le météore est détruit (par un autre météore ou par un missile
            // du vaisseau) : ses minerais restants — absorbés de gemmes
            // mangées — sont libérés en gemmes à sa position, jamais détruits
            // avec lui. Une seule fois : `minerals` passe à 0 dans
            // `release_meteor_minerals`, les triangles suivants du même
            // météore ne relibèrent rien.
            if who == WHOIAM_METEOR
                && shapes[shape_index].life <= 0
                && (collid_by == WHOIAM_METEOR || collid_by == WHOIAM_BULLET)
            {
                release_meteor_minerals(shapes, triangles, elements, shape_index, rng);
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

    // ramassage des gemmes par le **cosmonaute EVA** (vaisseau détruit) : il
    // les ramasse par proximité (non-collider — les gemmes le traversent) et
    // les **rapporte à la station** : la soute est déchargée à l'accostage
    // après le secours (`docking`/`rescue_cosmonaut`), comme pour le vaisseau
    eva_collect_gems(state, shapes, triangles, elements, sounds);
}

/// Ramassage des gemmes par le **cosmonaute EVA** : chaque gemme dont le
/// centre entre dans le rayon `EVA_PICKUP_RADIUS` du cosmonaute est ramassée
/// — détruite, son élément est compté dans la **même soute que le vaisseau**
/// (déchargée en minerais à la station après le secours). Soute pleine, plus
/// de ramassage. Sans effet quand le vaisseau est intact (`cosmonaut_active`
/// faux) : le cosmonaute garé ne ramasse rien.
fn eva_collect_gems(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    elements: &mut [Element],
    mut sounds: Option<&mut Sounds>,
) {
    if !state.cosmonaut_active {
        return;
    }
    let eva = state.eva_cosmonaut as usize;
    if eva >= shapes.len() {
        return;
    }
    let pos = shapes[eva].position;
    for g in 0..shapes.len() {
        if g == eva || shapes[g].who_i_am != WHOIAM_GEM || shapes[g].life <= 0 {
            continue;
        }
        // soute pleine : plus de ramassage
        if state.player.cargo_qty >= state.player.cargo_size {
            return;
        }
        let d = (shapes[g].position.x - pos.x).hypot(shapes[g].position.y - pos.y);
        if d > EVA_PICKUP_RADIUS {
            continue;
        }
        // ramassage : la gemme est détruite, son élément compté dans la soute
        let first = shapes[g].first_triangle;
        let element = triangles[first].element as usize;
        if element < elements.len() {
            elements[element].count += 1;
        }
        state.player.cargo_qty += 1;
        shapes[g].life = 0;
        for j in shapes[g].first_triangle..=shapes[g].last_triangle {
            triangles[j].life = 0;
        }
        if let Some(sounds) = sounds.as_mut() {
            sounds.play_gem();
        }
        if state.player.cargo_qty >= state.player.cargo_size {
            state.send_message("YOUR LOADING BAY IS FULL, YOU MUST UNLOAD IT AT THE STATION");
        }
    }
}

/// Météore vivant le plus proche d'une position donnée — utilisé par
/// l'absorption d'une gemme : `collid_by` ne porte que le type
/// (`WHOIAM_METEOR`), pas l'index de la forme qui a percuté la gemme — on
/// attribue donc l'absorption au météore le plus proche de la gemme (celui
/// qui vient de la percuter).
fn nearest_meteor(shapes: &[Shape], pos: Point) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, s) in shapes.iter().enumerate() {
        if s.who_i_am != WHOIAM_METEOR || s.life <= 0 {
            continue;
        }
        let d = (s.position.x - pos.x).hypot(s.position.y - pos.y);
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
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

/// Restaure le vaisseau à la station après une destruction (scénario
/// Survival — `scenario::PlayerHit::Destroyed`) : position, rotation et
/// vitesse remises à zéro (comme au départ), coque et triangles réparés — le
/// bouclier est déjà rechargé par `scenario::player_hit`. Le vaisseau se
/// retrouve à quai, dans l'état « déjà docké » du lancement (pas de boîte
/// DOCK STATION).
fn respawn_player(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    let p = &mut shapes[PLAYER_INDEX];
    p.position = Point::new(0.0, 0.0); // la station est au centre du monde
    p.direction = 0.0;
    p.velocity = 0.0;
    p.orientation = 0.0;
    p.rotation = 0.0;
    p.life = 1;
    for j in p.first_triangle..=p.last_triangle {
        triangles[j].life = 1;
    }
    // flamme et cooldown de tir coupés (le moteur ne brûle plus au respawn)
    state.player.thrusted = 0;
    state.player.revert_thrusted = 0;
    state.player.fire = 0.0;
    state.player_at_station = -1; // docké à la station (comme au lancement)
    state.player_enter_station = 0;
    // à quai : les liens d'accostage se rattachent au vaisseau (mire cachée)
    // jusqu'à ce que le joueur reparte (rétraction, voir `release_links`)
    state.dock_links = true;
}

/// Le vaisseau est détruit (jeu libre/Progression) : le joueur devient le
/// **cosmonaute éjecté** — il apparaît à la position du crash (le vaisseau
/// détruit reste invisible sur place, ses triangles sont morts) et doit
/// rejoindre la base (voir `rescue_cosmonaut`). La caméra, la mire et le HUD
/// suivent le cosmonaute (`pilot_index`).
fn activate_cosmonaut(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return; // cosmonaute EVA absent (jamais créé) : rien à éjecter
    }
    let crash = shapes[PLAYER_INDEX].position;
    let c = &mut shapes[idx];
    c.position = crash;
    c.direction = 0.0;
    c.velocity = 0.0;
    c.orientation = 0.0;
    c.rotation = 0.0;
    for j in c.first_triangle..=c.last_triangle {
        compute_real_positions(&mut triangles[j], c.position, c.center, c.orientation);
    }
    state.cosmonaut_active = true;
    state.docking_guide = true; // la mire guide le retour
    state.send_message("SHIP DESTROYED — RETURN TO THE STATION");
}

/// Le cosmonaute EVA a rejoint la base : il est **secouru** — le vaisseau est
/// reconstruit à la station (même état qu'au lancement, `respawn_player`), le
/// cosmonaute retourne à son poste (garé hors écran en bord de monde) et le
/// contrôle revient au vaisseau (qui démarre à quai, liens attachés).
fn rescue_cosmonaut(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    respawn_player(state, shapes, triangles);
    let idx = state.eva_cosmonaut as usize;
    let c = &mut shapes[idx];
    c.position = COSMONAUTE_EVA_PARK;
    c.direction = 0.0;
    c.velocity = 0.0;
    c.orientation = 0.0;
    c.rotation = 0.0;
    for j in c.first_triangle..=c.last_triangle {
        compute_real_positions(&mut triangles[j], c.position, c.center, c.orientation);
    }
    state.cosmonaut_active = false;
    state.send_message("RESCUED — THE STATION REBUILT YOUR SHIP");
}

/// Détecte le retour à la base (ex « detect return to the base » de
/// `mainLoop`) : le vaisseau est docké quand son centre entre dans la zone
/// d'accostage — le cercle de rayon `STATION_DOCK_DISTANCE` autour du centre
/// de la station (vérification circulaire, comme la mire affichée à l'écran)
/// **et** qu'il est presque immobile (`STATION_DOCK_SPEED`) : il faut ralentir
/// pour terminer l'accostage (la mire passe du rouge au vert avec la qualité
/// de l'approche).
///
/// NB : comme l'original, le choix UNLOAD/CLOSE de la boîte était ignoré
/// (`r%` non utilisé) — le cargo reste vidé de toute façon à l'accostage
/// (au plus tard à la frame suivant la fermeture de la boîte ; le bouton
/// UNLOAD de la boîte le vide immédiatement). Le ravitaillement (carburant +
/// munitions), lui, n'est plus automatique : il s'achète via le bouton
/// REFUEL/REARM de la boîte DOCK STATION.
fn docking(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    elements: &mut [Element],
) {
    // vaisseau détruit : le cosmonaute EVA rejoint la base — dès qu'il atteint
    // la zone d'accostage (cercle de rayon `STATION_DOCK_DISTANCE` au centre,
    // la station est en (0,0)), il est secouru : le vaisseau est reconstruit
    // à la station et le contrôle y revient (voir `rescue_cosmonaut`)
    if state.cosmonaut_active {
        let c = &shapes[state.eva_cosmonaut as usize];
        if c.position.x.hypot(c.position.y) < STATION_DOCK_DISTANCE {
            rescue_cosmonaut(state, shapes, triangles);
        }
        return;
    }
    let dx = shapes[PLAYER_INDEX].position.x - shapes[STATION_INDEX].position.x;
    let dy = shapes[PLAYER_INDEX].position.y - shapes[STATION_INDEX].position.y;
    let in_zone = dx * dx + dy * dy < STATION_DOCK_DISTANCE * STATION_DOCK_DISTANCE;
    // l'accostage se termine seulement si le vaisseau est presque immobile
    if in_zone && shapes[PLAYER_INDEX].velocity.abs() < STATION_DOCK_SPEED {
        if state.player_at_station == 0 {
            state.player_at_station = -1;
            state.player_enter_station = -1;
            shapes[PLAYER_INDEX].velocity = 0.0;
            // animation d'accostage (3 s) avant la boîte DOCK STATION : le
            // vaisseau pivote vers la droite et se recentre au centre — la
            // boîte s'ouvre à la fin (`advance_dock_animation`)
            state.dock_anim = DOCK_ANIMATION_DURATION;
            state.dock_anim_from_pos = shapes[PLAYER_INDEX].position;
            state.dock_anim_from_orient = shapes[PLAYER_INDEX].orientation;
            // l'accostage démarre : le guide est coupé — il ne réapparaîtra
            // qu'à un prochain retour (et pas pendant qu'on quitte l'accostage)
            state.docking_guide = false;
        } else {
            // déchargement : la soute est convertie en minerais (scénario à
            // économie) puis vidée — le ravitaillement s'achète via le
            // bouton REFUEL/REARM de la boîte DOCK STATION
            let had_cargo = state.player.cargo_qty > 0;
            scenario::unload_cargo(state, elements);
            for e in elements.iter_mut() {
                e.count = 0;
            }
            state.player.cargo_qty = 0;
            state.player_enter_station = 0;
            state.player_at_station = -1;
            // la progression (minerais) n'est persistée que s'il y avait du
            // cargo (cette branche tourne à chaque frame à quai — pas
            // d'écriture du fichier de config à chaque frame)
            if had_cargo {
                let _ = scenario::save_progression(state);
            }
        }
    } else {
        if state.player_at_station == -1 {
            // NB : typo de l'original (« LIVING » pour « LEAVING ») conservée
            state.send_message("YOU ARE LIVING THE STATION");
        }
        state.player_at_station = 0;
    }
}

/// Fait avancer l'animation d'accostage d'une frame : le vaisseau pivote
/// vers la droite (orientation 0) tout en se recentrant **exactement** au
/// centre de la station (position 0,0), avec une interpolation lissée
/// (smoothstep) sur `DOCK_ANIMATION_DURATION`. À la fin de l'animation, la
/// boîte DOCK STATION s'ouvre et le message d'accostage est envoyé (comme
/// avant, mais repoussé après l'animation).
///
/// Le monde est gelé pendant l'animation (appelé par `update` avant la
/// physique) ; le trait d'accostage est dessiné par
/// `render::draw_docking_line`.
fn advance_dock_animation(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.dock_anim = (state.dock_anim - dt).max(0.0);
    // avancement 0..1 avec lissage (smoothstep) pour un mouvement fluide
    let t = (1.0 - state.dock_anim / DOCK_ANIMATION_DURATION).clamp(0.0, 1.0);
    let e = t * t * (3.0 - 2.0 * t);
    let p = &mut shapes[PLAYER_INDEX];
    // recentrage exact sur le centre de la station (0,0)
    p.position.x = state.dock_anim_from_pos.x + (0.0 - state.dock_anim_from_pos.x) * e;
    p.position.y = state.dock_anim_from_pos.y + (0.0 - state.dock_anim_from_pos.y) * e;
    // pivot vers la droite : orientation 0, par le chemin le plus court
    let delta = shortest_angle_delta(state.dock_anim_from_orient, 0.0);
    p.orientation = state.dock_anim_from_orient + delta * e;
    // vitesse nulle (le vaisseau est immobilisé pendant l'accostage)
    p.velocity = 0.0;
    p.rotation = 0.0;
    // recalcule les positions réelles des triangles du vaisseau
    for i in p.first_triangle..=p.last_triangle {
        compute_real_positions(&mut triangles[i], p.position, p.center, p.orientation);
    }
    if state.dock_anim <= 0.0 {
        state.dock_anim = 0.0;
        state.send_message("YOU ARE DOCKED AT THE STATION");
        state.dock_box = true; // ouvre la boîte UNLOAD/REFUEL/CLOSE (jeu gelé)
    }
}

/// Le vaisseau quitte l'accostage (bouton CLOSE de la boîte DOCK STATION) :
/// ferme la boîte puis libère le vaisseau (rétraction des liens).
fn undock(state: &mut GameState) {
    state.dock_box = false;
    release_links(state);
}

/// Libère le vaisseau : détache les liens (s'ils étaient attachés à quai,
/// lancement/respawn) et démarre la **rétraction des liens** — le vaisseau
/// reste au centre de la station, les 4 traits néon se rétractent vers le
/// bord intérieur de l'anneau pendant `DOCK_RETRACT_DURATION` (monde gelé,
/// voir `advance_dock_retract`), puis il est libre. En quittant la base, le
/// **guide d'accostage est coupé** : la mire ne réapparaîtra qu'au retour
/// (franchissement de la limite extérieure en entrant).
fn release_links(state: &mut GameState) {
    state.dock_links = false;
    state.docking_guide = false;
    state.dock_retract = DOCK_RETRACT_DURATION;
}

/// Met à jour le **guide d'accostage** (la mire au centre de la station) :
/// il ne s'affiche **que lors du retour à la base** — le vaisseau doit avoir
/// quitté la base (franchi la limite extérieure en sortant, `dock_was_outside`
/// repassé à vrai) puis la **recroiser en entrant** (front montant : la
/// distance passe de ≥ au rayon à < au rayon). Il ne s'affiche donc jamais
/// pendant que le vaisseau quitte l'accostage ni à quai (les états « tenu »
/// masquent de toute façon la mire dans `render::docking_marker_visible`).
fn update_docking_guide(
    state: &mut GameState,
    player_position: Point,
    station_position: Point,
    station_radius: f64,
) {
    // vaisseau détruit : le cosmonaute éjecté doit TOUJOURS voir la mire —
    // elle le guide vers la base (le « retour » classique ne s'applique pas)
    if state.cosmonaut_active {
        state.docking_guide = true;
        state.dock_was_outside = true;
        return;
    }
    let dist = (player_position.x - station_position.x).hypot(player_position.y - station_position.y);
    let outside = dist >= station_radius;
    if outside {
        state.docking_guide = false;
    } else if state.dock_was_outside {
        // vient de franchir la limite extérieure de la base en entrant :
        // c'est le retour — le guide s'affiche
        state.docking_guide = true;
    }
    state.dock_was_outside = outside;
}

/// Le joueur donne-t-il une commande de déplacement (flèches ↑/↓/←/→, tous
/// les modes de déplacement) ? Utilisé pour déclencher la rétraction des
/// liens quand le vaisseau démarre de la base (voir `update`).
fn player_moving_input() -> bool {
    is_key_down(KeyCode::Up)
        || is_key_down(KeyCode::Down)
        || is_key_down(KeyCode::Left)
        || is_key_down(KeyCode::Right)
}

/// Fait avancer la rétraction des liens d'accostage d'une frame : le vaisseau
/// reste immobilisé exactement au centre de la station (position 0,0,
/// orientation 0) pendant `DOCK_RETRACT_DURATION` — les liens se rétractent
/// visuellement (voir `render::draw_docking_line`). À la fin, le vaisseau est
/// libre (le monde se dégèle, `docking` peut le faire repartir).
///
/// Le monde est gelé pendant la rétraction (appelé par `update` avant la
/// physique).
fn advance_dock_retract(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.dock_retract = (state.dock_retract - dt).max(0.0);
    let p = &mut shapes[PLAYER_INDEX];
    // le vaisseau reste exactement au centre, pointant vers la droite
    p.position.x = 0.0;
    p.position.y = 0.0;
    p.orientation = 0.0;
    p.velocity = 0.0;
    p.rotation = 0.0;
    // recalcule les positions réelles des triangles du vaisseau
    for i in p.first_triangle..=p.last_triangle {
        compute_real_positions(&mut triangles[i], p.position, p.center, p.orientation);
    }
    if state.dock_retract <= 0.0 {
        state.dock_retract = 0.0;
    }
}

/// Écart angulaire le plus court (radians, dans ]-π, π]) entre deux angles,
/// pour pivoter vers la droite (orientation 0) sans faire un tour complet.
fn shortest_angle_delta(from: f64, to: f64) -> f64 {
    let mut d = (to - from) % TAU;
    if d > std::f64::consts::PI {
        d -= TAU;
    }
    if d < -std::f64::consts::PI {
        d += TAU;
    }
    d
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

/// Bouton cliqué sur la boîte de choix DOCK STATION (accostage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChoiceClick {
    None,
    /// Décharge la soute (minerais disponibles pour REFUEL/REARM).
    Unload,
    /// Achète carburant + munitions contre minerais.
    Refuel,
    /// Ouvre l'atelier d'amélioration du vaisseau (scénario à économie).
    Upgrades,
    /// Ferme la boîte.
    Close,
}

/// Détecte un clic sur la boîte de choix DOCK STATION (ex
/// `windowUtils_choiceBox`) et renvoie le bouton cliqué (contrairement à
/// l'original, le choix n'est plus ignoré : UNLOAD, REFUEL/REARM et UPGRADES
/// agissent). Le bouton UPGRADES n'existe qu'en scénario à économie (la
/// géométrie est vide sinon).
fn choice_box_click(state: &GameState) -> ChoiceClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return ChoiceClick::None;
    }
    let l = choice_box_layout(scenario::has_economy(state));
    let m = mouse_to_game();
    if l.unload.contains(m) {
        ChoiceClick::Unload
    } else if l.refuel.contains(m) {
        ChoiceClick::Refuel
    } else if l.upgrades.contains(m) {
        ChoiceClick::Upgrades
    } else if l.close.contains(m) {
        ChoiceClick::Close
    } else {
        ChoiceClick::None
    }
}

/// Bouton cliqué sur l'atelier d'amélioration du vaisseau (bouton UPGRADES
/// de la boîte DOCK STATION, scénario à économie).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkshopClick {
    None,
    /// Achète l'extension de réservoir de carburant.
    BuyFuel,
    /// Achète l'extension de chargeur de munitions.
    BuyAmmo,
    /// Achète l'extension de soute.
    BuyCargo,
    /// Revient à la boîte DOCK STATION (toujours accosté).
    Close,
}

/// Détecte un clic sur l'atelier d'amélioration : une ligne d'extension
/// (achat) ou le bouton CLOSE (retour à la boîte DOCK STATION).
fn workshop_box_click() -> WorkshopClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return WorkshopClick::None;
    }
    let l = workshop_box_layout();
    let m = mouse_to_game();
    if l.fuel.contains(m) {
        WorkshopClick::BuyFuel
    } else if l.ammo.contains(m) {
        WorkshopClick::BuyAmmo
    } else if l.cargo.contains(m) {
        WorkshopClick::BuyCargo
    } else if l.close.contains(m) {
        WorkshopClick::Close
    } else {
        WorkshopClick::None
    }
}

/// Achète une extension d'atelier (réservoir, chargeur ou soute) puis persiste
/// la progression (minerais, niveaux d'extension) — les réservoirs montent à
/// la nouvelle capacité et la soute s'agrandit dans `buy_upgrade`.
fn buy_upgrade_and_save(state: &mut GameState, track: scenario::UpgradeTrackId) {
    scenario::buy_upgrade(state, track);
    let _ = scenario::save_progression(state);
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

/// Clic sur l'écran de paramétrage (touche O) : un radio-bouton de mode
/// sélectionne le mode (appliqué immédiatement, l'écran reste ouvert pour
/// comparer), les cases MUSIC / AUTO GENERATE / ANTIALIAS basculent, un clic
/// sur la barre du volume donne la fraction demandée (0..1), les lignes
/// RENDER / WINDOW / SIZE font cycler leur valeur, RESET remet les réglages
/// par défaut et CLOSE ferme l'écran.
enum SettingsClick {
    None,
    Mode(i32),
    Music,
    AutoGenerate,
    Volume(f32),
    RenderStyle,
    WindowMode,
    WindowSize,
    Antialias,
    /// Relance le jeu (affiché quand un réglage modifié exige un redémarrage).
    Restart,
    Reset,
    Close,
}

/// Détecte un clic sur l'écran de paramétrage (touche O) : contrôle cliqué
/// (mode, case, volume, ligne graphique, RESTART, RESET ou CLOSE). Le bouton
/// RESTART n'est actif que si un réglage modifié (l'anticrénelage) diffère de
/// la valeur appliquée par la fenêtre.
fn settings_box_click(state: &GameState) -> SettingsClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return SettingsClick::None;
    }
    let l = settings_box_layout();
    let m = mouse_to_game();
    for (i, rect) in l.modes.iter().enumerate() {
        if rect.contains(m) {
            return SettingsClick::Mode(i as i32);
        }
    }
    if l.music.contains(m) {
        return SettingsClick::Music;
    }
    if l.auto_generate.contains(m) {
        return SettingsClick::AutoGenerate;
    }
    if l.volume_track.contains(m) {
        return SettingsClick::Volume(((m.x - l.volume_track.x) / l.volume_track.w).clamp(0.0, 1.0));
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
    if state.antialias != state.antialias_applied && l.restart.contains(m) {
        return SettingsClick::Restart;
    }
    if l.reset.contains(m) {
        return SettingsClick::Reset;
    }
    if l.close.contains(m) {
        return SettingsClick::Close;
    }
    SettingsClick::None
}

/// Traite l'input de l'écran de paramétrage (touche O) : clavier (flèches
/// ↑/↓ = focus radio, Entrée = applique le mode, ESC = ferme) et clic souris
/// (radio, cases MUSIC / AUTO GENERATE / ANTIALIAS, barre de volume, lignes
/// RENDER / WINDOW / SIZE, RESTART, RESET, CLOSE). Les réglages modifiés sont
/// persistés immédiatement (le mode l'est à la fermeture). Utilisé par la
/// boucle de jeu et par l'écran titre (`title.rs`). `sounds` est optionnel :
/// absent, musique et volume ne sont pas modifiables. Renvoie `true` si le
/// bouton RESTART a été cliqué (le jeu doit se relancer).
pub fn handle_settings_input(state: &mut GameState, mut sounds: Option<&mut Sounds>) -> bool {
    let mut restart = false;
    if is_key_pressed(KeyCode::Up) {
        state.settings_focus = settings_focus_move(state.settings_focus, -1);
    }
    if is_key_pressed(KeyCode::Down) {
        state.settings_focus = settings_focus_move(state.settings_focus, 1);
    }
    if is_key_pressed(KeyCode::Enter) {
        // la sélection passe par le scénario : un mode verrouillé (scénario
        // à économie) est payé en minerais, refusé si insuffisant ; un mode
        // acheté est persisté (minerais déduits + mode débloqué)
        if scenario::try_select_mode(state, state.settings_focus) {
            let _ = scenario::save_progression(state);
        }
    }
    match settings_box_click(state) {
        SettingsClick::Mode(m) => {
            // un clic recentre le focus clavier sur le mode choisi ; la
            // sélection passe par le scénario (voir Entrée ci-dessus)
            state.settings_focus = m;
            if scenario::try_select_mode(state, m) {
                let _ = scenario::save_progression(state);
            }
        }
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
        SettingsClick::RenderStyle => {
            state.render_style = next_render_style(state.render_style);
            let _ = persist::save_render_style(state.render_style as i32);
        }
        SettingsClick::WindowMode => {
            // même cycle que la touche F (fenêtré → zoomé → natif) ; le
            // mode est persisté à chaque clic
            cycle_view_mode(state);
            let _ = persist::save_window_mode(state.view_mode as i32);
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
        SettingsClick::Restart => restart = true,
        SettingsClick::Reset => reset_settings(state, sounds.as_deref_mut()),
        SettingsClick::Close => close_and_persist(state),
        SettingsClick::None => {}
    }
    // glisser sur la barre de volume (bouton maintenu) : réglage continu
    // tant que le pointeur reste sur la piste
    if is_mouse_button_down(MouseButton::Left) {
        let l = settings_box_layout();
        let m = mouse_to_game();
        if l.volume_track.contains(m) {
            set_volume_fraction(
                sounds.as_deref_mut(),
                ((m.x - l.volume_track.x) / l.volume_track.w).clamp(0.0, 1.0),
            );
        }
    }
    if is_key_pressed(KeyCode::Escape) {
        close_and_persist(state);
    }
    restart
}

/// Ferme l'écran de paramétrage ; renvoie `true` si le mode de déplacement a
/// été modifié pendant l'écran (un message HUD annonce alors le mode activé).
fn close_settings(state: &mut GameState) -> bool {
    state.settings_box = false;
    let changed = state.moving_mode != state.settings_previous_mode;
    if changed {
        state.send_message(&format!("MOVING MODE: {}", moving_mode_label(state.moving_mode)));
    }
    changed
}

/// Ferme l'écran de paramétrage et persiste le mode de déplacement dans le
/// fichier de config s'il a changé (le message HUD est envoyé par
/// `close_settings`).
fn close_and_persist(state: &mut GameState) {
    if close_settings(state) {
        let _ = persist::save_moving_mode(state.moving_mode);
    }
}

/// Déplace le focus des radio-boutons de l'écran de paramétrage (flèches
/// haut/bas), borné à `[0, MOVING_MODE_COUNT-1]`.
fn settings_focus_move(focus: i32, delta: i32) -> i32 {
    (focus + delta).clamp(0, MOVING_MODE_COUNT - 1)
}

/// Remet les réglages par défaut (bouton RESET) : mode DIRECTIONAL, musique
/// en marche, génération automatique active, volume 100 %, rendu texturé,
/// fenêtré à 960×540, anticrénelage éteint — les valeurs par défaut ne sont
/// réenregistrées à la fermeture que si elles ont été modifiées pendant
/// l'écran. Partie « réglages » pure du RESET (testable sans contexte
/// macroquad) : remet les valeurs par défaut (mode DIRECTIONAL, génération
/// automatique, rendu texturé, fenêtre 960×540, anticrénelage éteint, focus
/// recentré).
fn reset_settings_fields(state: &mut GameState) {
    // mode de déplacement : défaut du scénario (DIRECTIONAL en jeu libre,
    // INERTIAL en Progression — le RESET ne débloque jamais un mode payant)
    state.moving_mode = scenario::start_mode(state.scenario);
    state.settings_focus = state.moving_mode;
    state.auto_generate = true;
    state.render_style = RenderStyle::Textured;
    state.window_size = 0;
    state.antialias = false;
}

/// Remet les réglages par défaut (bouton RESET) : champs par défaut
/// (`reset_settings_fields`), retour fenêtré à 960×540, musique en marche,
/// volume 100 %, et clés de réglage du fichier de config supprimées — les
/// valeurs par défaut ne sont réenregistrées à la fermeture que si elles ont
/// été modifiées pendant l'écran. NB : la progression d'un scénario à
/// économie (scénario choisi, minerais, modes payés, réputation — clés
/// `scenario`/`prog_*`) n'est pas supprimée : seuls les réglages repartent
/// aux défauts.
fn reset_settings(state: &mut GameState, sounds: Option<&mut Sounds>) {
    reset_settings_fields(state);
    apply_view_mode(state, ViewMode::Windowed);
    if state.view_mode == ViewMode::Windowed {
        request_new_screen_size(VIEWPORT_WIDTH as f32, VIEWPORT_HEIGHT as f32);
    }
    if let Some(sounds) = sounds {
        sounds.set_volume(1.0);
        if !sounds.music_on {
            sounds.toggle_music();
        }
    }
    // seules les clés de réglage sont supprimées — le scénario et sa
    // progression (`scenario`, `prog_*`) survivent au RESET
    for key in [
        "moving_mode",
        "music",
        "auto_generate",
        "volume",
        "render_style",
        "window_mode",
        "window_size",
        "antialias",
    ] {
        let _ = persist::delete_key(key);
    }
}

/// Style de rendu suivant dans le cycle (TEXTURED → COLORED → MESH → …).
fn next_render_style(style: RenderStyle) -> RenderStyle {
    match style {
        RenderStyle::Textured => RenderStyle::Colored,
        RenderStyle::Colored => RenderStyle::Mesh,
        RenderStyle::Mesh => RenderStyle::Textured,
    }
}

/// Index de définition de fenêtre suivant dans le cycle (960×540 → 1280×720
/// → … → retour), borné à `WINDOW_SIZES`.
fn next_window_size(index: i32) -> i32 {
    (index + 1) % WINDOW_SIZES.len() as i32
}

/// Dimensions `(largeur, hauteur)` de la définition de fenêtre `index`.
fn window_size_dims(index: i32) -> (f32, f32) {
    let (w, h) = WINDOW_SIZES[index.clamp(0, WINDOW_SIZES.len() as i32 - 1) as usize];
    (w as f32, h as f32)
}

/// Bascule vers un mode d'affichage donné (bouton RESET) : entre dans le
/// plein écran EWMH si la cible est zoomé/natif, en sort (ClientMessage
/// REMOVE via libX11) sinon — voir `cycle_view_mode`.
fn apply_view_mode(state: &mut GameState, target: ViewMode) {
    if state.view_mode == target {
        return;
    }
    match (state.view_mode, target) {
        // fenêtré → plein écran : le chemin de rendu (zoomé ou natif) ne
        // change que la caméra, la bascule EWMH est la même
        (ViewMode::Windowed, _) => set_fullscreen(true),
        // déjà en plein écran : seul le chemin de rendu change
        (ViewMode::Zoomed, ViewMode::Native) | (ViewMode::Native, ViewMode::Zoomed) => {}
        // plein écran → fenêtré : REMOVE EWMH (repli : redimensionnement à
        // la définition choisie)
        (_, ViewMode::Windowed) => {
            if !crate::x11::set_fullscreen(false) {
                let (w, h) = window_size_dims(state.window_size);
                request_new_screen_size(w, h);
            }
        }
        _ => {}
    }
    state.view_mode = target;
}

/// Applique le volume maître depuis une fraction (0..1) de la barre et le
/// persiste. N'écrit le fichier que si la valeur change réellement (glisser
/// sur la barre ne réécrit pas le config à chaque frame).
fn set_volume_fraction(sounds: Option<&mut Sounds>, fraction: f32) {
    if let Some(sounds) = sounds {
        let pct = (fraction.clamp(0.0, 1.0) * 100.0).round() as i32;
        let current = (sounds.volume * 100.0).round() as i32;
        if pct != current {
            sounds.set_volume(pct as f32 / 100.0);
            let _ = persist::set_i32("volume", pct);
        }
    }
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
    // vaisseau détruit : le joueur contrôle le cosmonaute EVA éjecté (seul
    // objectif : rejoindre la base) — pas de tir ni de carburant
    if state.cosmonaut_active {
        cosmonaut_controls(state, shapes, dt);
        return;
    }
    state.player.thrust = 0.0;

    // carburant (scénarios à économie) : les poussées avant/arrière sont
    // bloquées quand le réservoir est vide — les rotations restent libres
    let fuel_ok = scenario::fuel_available(state);

    // (portée dédiée : l'emprunt mutable de `shapes[PLAYER_INDEX]` doit se
    // terminer avant le tir, qui réemprunte tout `shapes`)
    {
    let player = &mut shapes[PLAYER_INDEX];
    match state.moving_mode {
        MOVING_MODE_DIRECTIONAL => {
            if fuel_ok && is_key_down(KeyCode::Up) {
                player.velocity += PLAYER_ACCELERATION * 60.0 * dt;
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
            }
            if is_key_down(KeyCode::Right) {
                player.direction -= PLAYER_ROTATION_SPEED * 60.0 * dt;
                player.orientation = -player.direction;
            }
            if fuel_ok && is_key_down(KeyCode::Down) {
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
            if fuel_ok && is_key_down(KeyCode::Up) {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, 1.0, -1.0);
            }
            if is_key_down(KeyCode::Right) {
                player.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
            }
            if fuel_ok && is_key_down(KeyCode::Down) {
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
            if fuel_ok && is_key_down(KeyCode::Up) {
                state.player.thrust = 0.1;
                state.player.thrusted = -5;
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if fuel_ok && is_key_down(KeyCode::Right) {
                let dx = player.direction.cos() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if fuel_ok && is_key_down(KeyCode::Down) {
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if fuel_ok && is_key_down(KeyCode::Left) {
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
    // cooldown `fire` (1/3 s) bloque les tirs suivants ; le scénario
    // consomme des munitions et bloque le tir quand le chargeur est vide
    if (is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift))
        && state.player.fire <= 0.0
        && scenario::try_fire(state)
    {
        fire_bullet(shapes, triangles);
        state.player.fire = PLAYER_FIRE_COOLDOWN;
        state.bullets_fired += 1;
        if let Some(sounds) = sounds {
            sounds.play_bullet();
        }
    }
}

/// Contrôles du **cosmonaute EVA** (vaisseau détruit — voir
/// `activate_cosmonaut`) : la poussée est **vectorielle** (comme le mode
/// INERTIAL du vaisseau) — ↑ exerce une poussée dans l'orientation qui
/// **s'ajoute au vecteur de déplacement** (`thrust_vector`) ; pour changer de
/// direction il faut d'abord **s'orienter** (←/→ font tourner la figure, sans
/// modifier la trajectoire en cours) **puis pousser** : le mouvement dévie
/// progressivement. Un seul propulseur : pas de frein ni de marche arrière. Il
/// faut doser la poussée pour rejoindre la base (`docking`/`rescue_cosmonaut`).
/// Sans tir ni carburant.
fn cosmonaut_controls(state: &mut GameState, shapes: &mut [Shape], dt: f64) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return;
    }
    let c = &mut shapes[idx];
    state.player.thrust = 0.0;
    // poussée vectorielle : la poussée (selon l'orientation) s'ajoute au
    // vecteur de déplacement actuel, direction et vitesse recalculées
    if is_key_down(KeyCode::Up) {
        state.player.thrust = 0.1;
        state.player.thrusted = -5;
        thrust_vector(c, PLAYER_ACCELERATION * 60.0 * dt, c.orientation, 1.0, -1.0);
    }
    // orientation seule : la figure tourne, la trajectoire ne change pas
    // (elle ne sera déviée que par une poussée ultérieure)
    if is_key_down(KeyCode::Right) {
        c.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
    }
    if is_key_down(KeyCode::Left) {
        c.orientation -= PLAYER_ROTATION_SPEED * 60.0 * dt;
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
    fn meteor_meteor_collision_releases_minerals() {
        // deux météores se percutent et sont détruits : leurs minerais sont
        // libérés en gemmes à leur position (une gemme par unité de minerai)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_METEOR, 0, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0),
        ];
        shapes[0].minerals = 2; // le premier météore contient 2 minerais
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

        // les deux météores sont détruits, les minerais du premier libérés
        assert_eq!(shapes[0].life, 0);
        assert_eq!(shapes[1].life, 0);
        assert_eq!(shapes[0].minerals, 0);
        let gems = shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM).count();
        assert_eq!(gems, 2);
    }

    #[test]
    fn meteor_absorbs_gem_increasing_its_minerals() {
        // un météore percute une gemme : il l'absorbe — la gemme disparaît
        // et la quantité de minerai du météore augmente (sans endommager le
        // météore)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_GEM, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 2.0, 2.0),
        ];
        triangles[0].element = 1; // GOLD
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // la gemme a été absorbée (détruite), le météore a gagné un minerai
        assert_eq!(shapes[0].life, 0);
        assert_eq!(shapes[1].minerals, 1);
        // le météore n'est pas endommagé par l'absorption
        assert_eq!(shapes[1].life, 1);
        assert_eq!(triangles[1].life, 1);
    }

    #[test]
    fn meteor_breaks_on_station_ring() {
        // Dérive volontaire : le rayon de la station couvre l'anneau visible
        // (r ≈ 110-162, plus de `radius = 36` forcé comme l'original). Un
        // météore posé sur l'anneau (loin du centre) doit être détruit par la
        // détection de collision par triangles (SAT), pas traverser la base.
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        let mut stars = Vec::new();
        let mut elements = Vec::new();
        let mut rng = seed();
        crate::generate::prepare(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut stars,
            &mut elements,
            &mut rng,
        );

        // le rayon de collision de la station couvre l'anneau (dérive volontaire)
        assert!(
            shapes[STATION_INDEX].radius >= 160.0,
            "rayon station {}",
            shapes[STATION_INDEX].radius
        );

        // météore posé sur la bande de l'anneau, à droite du centre (x = 140)
        let idx = crate::generate::generate_shape(
            &mut shapes,
            &mut triangles,
            8,
            TRIANGLE_BASE_MIN,
            TRIANGLE_BASE_MAX,
            TRIANGLE_HEIGHT_MIN,
            TRIANGLE_HEIGHT_MAX,
            &elements,
            &mut rng,
        );
        {
            let m = &mut shapes[idx];
            m.who_i_am = WHOIAM_METEOR;
            m.is_collider = true;
            m.velocity = 0.0;
            m.position = Point::new(140.0, 0.0);
        }
        // sans élément minéral : pas de gemme créée, test ciblé sur la collision
        for i in shapes[idx].first_triangle..=shapes[idx].last_triangle {
            triangles[i].element = 0;
        }
        compute_shape_center(&mut shapes[idx], &triangles);
        let initial_life = shapes[idx].life;

        let mut garbages = Vec::new();
        collisions(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut rng,
            None,
            1.0 / 60.0,
        );

        // le météore a perdu au moins un triangle (SAT contre l'anneau),
        // la station est intacte
        assert!(
            shapes[idx].life < initial_life,
            "météore intact sur l'anneau : life {} (initial {})",
            shapes[idx].life,
            initial_life
        );
        assert_eq!(shapes[STATION_INDEX].life, 66);
        assert_eq!(triangles[shapes[STATION_INDEX].first_triangle].life, 1);
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
    fn missile_destroying_meteor_releases_absorbed_minerals() {
        // un missile détruit un météore qui contient des minerais absorbés
        // (gemmes mangées, sans triangle minéralisé restant) : les minerais
        // sont libérés en gemmes — pas détruits avec le météore
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_BULLET, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        shapes[1].minerals = 3; // 3 minerais absorbés, plus de triangle minéralisé
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le météore est détruit et ses minerais libérés (pas détruits)
        assert_eq!(shapes[1].life, 0);
        assert_eq!(shapes[1].minerals, 0);
        let gems = shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM).count();
        assert_eq!(gems, 3, "les 3 minerais absorbés doivent être libérés");
    }

    #[test]
    fn missile_hitting_gem_directly_destroys_it() {
        // un missile qui touche directement une gemme la DÉTRUIT : c'est le
        // seul cas de destruction de minerai — aucune nouvelle gemme n'est
        // créée (pas de « libération »)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_BULLET, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_GEM, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[1].element = 1; // GOLD
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // la gemme est détruite et aucune nouvelle gemme n'est apparue
        assert_eq!(shapes[1].life, 0);
        assert_eq!(triangles[1].life, 0);
        let gems = shapes.iter().filter(|s| s.who_i_am == WHOIAM_GEM).count();
        assert_eq!(gems, 1, "la gemme détruite ne doit pas être dupliquée");
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
    fn survival_shield_absorbs_player_impact() {
        // scénario Survival : un impact météore sur le vaisseau est absorbé
        // par le bouclier — le vaisseau et son triangle restent intacts
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Survival;
        crate::scenario::apply_start(&mut state);
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(state.resources.shield, 2.0); // 3 - 1 impact absorbé
        assert_eq!(state.resources.lives, 3);
        assert_eq!(shapes[0].life, 1); // vaisseau intact
        assert_eq!(triangles[0].life, 1);
    }

    #[test]
    fn survival_destroyed_ship_respawns_at_station() {
        // bouclier déjà vide : l'impact détruit le vaisseau — une vie perdue,
        // respawn à la station (position réinitialisée, bouclier rechargé)
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Survival;
        crate::scenario::apply_start(&mut state);
        state.resources.shield = 0.0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 300.0, 200.0),
            test_shape(WHOIAM_METEOR, 1, 1, 302.0, 202.0), // sur le vaisseau
        ];
        let mut triangles = vec![test_triangle(0, 0, 300.0, 200.0), test_triangle(1, 1, 302.0, 202.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(state.resources.lives, 2);
        assert_eq!(
            state.resources.shield,
            crate::scenario::SURVIVAL_SCENARIO.shield_capacity
        );
        assert_eq!(shapes[0].position, Point::new(0.0, 0.0)); // respawn station
        assert_eq!(shapes[0].velocity, 0.0);
        assert_eq!(shapes[0].life, 1);
        assert_eq!(triangles[0].life, 1);
        assert_eq!(state.player_at_station, -1);
        assert_eq!(
            state.invulnerable,
            crate::scenario::SURVIVAL_SCENARIO.respawn_invulnerability
        );
        assert!(state.message_queue.contains("SHIP DESTROYED"));
    }

    #[test]
    fn respawn_player_restores_ship_at_station() {
        // restauration directe (appelée par la collision Survival) : position,
        // vitesse, orientation, coque et flammes remises à zéro, état « à
        // quai » comme au lancement
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 300.0, 200.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0)];
        shapes[0].velocity = 4.0;
        shapes[0].direction = 1.5;
        state.player_at_station = 0;
        state.player.thrusted = 3;

        respawn_player(&mut state, &mut shapes, &mut triangles);

        assert_eq!(shapes[0].position, Point::new(0.0, 0.0));
        assert_eq!(shapes[0].velocity, 0.0);
        assert_eq!(shapes[0].direction, 0.0);
        assert_eq!(shapes[0].life, 1);
        assert_eq!(triangles[0].life, 1);
        assert_eq!(state.player.thrusted, 0);
        assert_eq!(state.player_at_station, -1);
        // à quai : les liens d'accostage se rattachent au vaisseau (mire
        // cachée) jusqu'au départ (rétraction via `release_links`)
        assert!(state.dock_links);
    }

    #[test]
    fn docking_starts_animation_instead_of_opening_box_directly() {
        // le joueur revient à la station après être parti (playerAtStation = 0,
        // ex mainLoop : la boîte n'apparaît qu'au retour), presque immobile →
        // l'ANIMATION d'accostage démarre (3 s) au lieu d'ouvrir la boîte
        // immédiatement : vitesse mise à 0, message pas encore envoyé
        let mut state = GameState::new();
        state.player_at_station = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        shapes[0].velocity = 0.2; // < STATION_DOCK_SPEED : accostage possible
        let mut elements = default_elements();
        let mut triangles = Vec::new();
        elements[1].count = 4;

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        assert_eq!(state.dock_anim, DOCK_ANIMATION_DURATION);
        assert!(!state.dock_box); // pas encore : l'animation d'abord
        assert_eq!(shapes[0].velocity, 0.0);
        assert_eq!(state.player_at_station, -1);
        assert!(!state.message_queue.contains("YOU ARE DOCKED AT THE STATION"));
        // le cargo n'est pas vidé tant que la boîte n'est pas ouverte
        assert_eq!(elements[1].count, 4);
    }

    #[test]
    fn docking_animation_opens_box_after_duration_and_centers_ship() {
        // l'animation d'accostage dure DOCK_ANIMATION_DURATION : le vaisseau
        // pivote vers la droite (orientation 0) tout en se recentrant
        // exactement au centre (0,0) ; à la fin, la boîte s'ouvre et le
        // message est envoyé
        let mut state = GameState::new();
        state.player_at_station = 0;
        // le vaisseau est dans la zone d'accostage (distance ≈ 14 < 15)
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 10.0, 10.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        shapes[0].velocity = 0.2;
        shapes[0].orientation = 2.5; // loin de 0 : il doit pivoter vers la droite
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0)];
        let mut elements = default_elements();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);
        assert_eq!(state.dock_anim, DOCK_ANIMATION_DURATION);
        assert!(!state.dock_box);

        // à mi-animation : le vaisseau a avancé vers le centre et pivoté
        // (mais pas encore arrivé)
        advance_dock_animation(&mut state, &mut shapes, &mut triangles, DOCK_ANIMATION_DURATION / 2.0);
        assert!(shapes[0].position.x.abs() < 10.0);
        assert!(shapes[0].orientation.abs() < 2.5);
        assert!(!state.dock_box);

        // animation terminée : centré, pointant vers la droite, boîte ouverte
        advance_dock_animation(&mut state, &mut shapes, &mut triangles, DOCK_ANIMATION_DURATION);
        assert_eq!(state.dock_anim, 0.0);
        assert!(state.dock_box);
        assert_eq!(shapes[0].position.x, 0.0);
        assert_eq!(shapes[0].position.y, 0.0);
        assert_eq!(shapes[0].orientation % TAU, 0.0);
        assert!(state.message_queue.contains("YOU ARE DOCKED AT THE STATION"));
    }

    #[test]
    fn closing_dock_box_starts_link_retraction() {
        // CLOSE quitte l'accostage : la boîte se ferme et la **rétraction des
        // liens** démarre (le vaisseau reste au centre, monde gelé) ; à la fin
        // de `DOCK_RETRACT_DURATION`, le vaisseau est libre
        let mut state = GameState::new();
        state.dock_box = true;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0)];

        undock(&mut state);
        assert!(!state.dock_box);
        assert_eq!(state.dock_retract, DOCK_RETRACT_DURATION);

        // à mi-rétraction : le vaisseau reste exactement au centre (0,0),
        // orientation 0, immobilisé — les liens se rétractent encore
        advance_dock_retract(&mut state, &mut shapes, &mut triangles, DOCK_RETRACT_DURATION / 2.0);
        assert!(state.dock_retract > 0.0);
        assert_eq!(shapes[0].position.x, 0.0);
        assert_eq!(shapes[0].position.y, 0.0);
        assert_eq!(shapes[0].orientation, 0.0);
        assert_eq!(shapes[0].velocity, 0.0);

        // rétraction terminée : le vaisseau est libre (délesté de l'état)
        advance_dock_retract(&mut state, &mut shapes, &mut triangles, DOCK_RETRACT_DURATION);
        assert_eq!(state.dock_retract, 0.0);
    }

    #[test]
    fn release_links_detaches_and_starts_retraction() {
        // au lancement, le vaisseau est à quai (liens attachés, mire cachée —
        // voir `state.dock_links`) ; dès qu'il démarre (commande de mouvement
        // ou CLOSE après un accostage), `release_links` détache les liens et
        // lance la rétraction (monde gelé pendant `DOCK_RETRACT_DURATION`)
        let mut state = GameState::new();
        assert!(state.dock_links); // à quai au lancement

        release_links(&mut state);
        assert!(!state.dock_links);
        assert_eq!(state.dock_retract, DOCK_RETRACT_DURATION);
    }

    #[test]
    fn docking_guide_activates_only_on_return() {
        // la mire (guide d'accostage) ne s'affiche QUE lors du RETOUR à la
        // base : pas à quai, pas pendant qu'on quitte l'accostage, pas en
        // vol — elle s'active quand le vaisseau franchit la limite extérieure
        // de la base EN ENTRANT (après l'avoir franchie en sortant)
        let mut state = GameState::new();
        state.dock_links = false;
        let station = Point::new(0.0, 0.0);
        let radius = 160.0;
        // à quai au centre (ex juste après le départ) : pas de guide
        update_docking_guide(&mut state, Point::new(0.0, 0.0), station, radius);
        assert!(!state.docking_guide);
        // le vaisseau sort : franchit la limite extérieure en sortant
        update_docking_guide(&mut state, Point::new(radius + 1.0, 0.0), station, radius);
        assert!(!state.docking_guide);
        assert!(state.dock_was_outside);
        // le vaisseau revient : franchit la limite en entrant → guide actif
        update_docking_guide(&mut state, Point::new(radius - 1.0, 0.0), station, radius);
        assert!(state.docking_guide);
        assert!(!state.dock_was_outside);
        // en approche (dans le rayon) : le guide reste actif
        update_docking_guide(&mut state, Point::new(10.0, 0.0), station, radius);
        assert!(state.docking_guide);
        // l'accostage démarre : le guide est coupé (et le restera après le
        // départ — il ne se réactive qu'à un nouveau franchissement en entrant)
        state.docking_guide = false;
        update_docking_guide(&mut state, Point::new(5.0, 0.0), station, radius);
        assert!(!state.docking_guide); // toujours dans le rayon : pas de réactivation
        update_docking_guide(&mut state, Point::new(radius + 1.0, 0.0), station, radius);
        update_docking_guide(&mut state, Point::new(radius - 1.0, 0.0), station, radius);
        assert!(state.docking_guide); // nouveau retour → guide réactivé
    }

    #[test]
    fn shortest_angle_delta_takes_the_short_path() {
        // de π vers 0 : le chemin le plus court est -π (pas +π)
        assert_eq!(shortest_angle_delta(std::f64::consts::PI, 0.0), -std::f64::consts::PI);
        // de 2.5 vers 0 : -2.5 (pas 3.78)
        assert!((shortest_angle_delta(2.5, 0.0) + 2.5).abs() < 1e-12);
        // déjà à 0 : pas de rotation
        assert_eq!(shortest_angle_delta(0.0, 0.0), 0.0);
    }

    #[test]
    fn docking_requires_nearly_zero_velocity() {
        // dans la zone d'accostage mais trop rapide : pas de boîte, l'état
        // « en approche » reste ; presque immobile : l'accostage se termine
        let mut state = GameState::new();
        state.player_at_station = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        shapes[0].velocity = 3.0; // > STATION_DOCK_SPEED
        let mut elements = default_elements();
        let mut triangles = Vec::new();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);
        assert!(!state.dock_box);
        assert_eq!(state.player_at_station, 0); // pas encore docké

        // le vaisseau ralentit : l'accostage se termine (l'ANIMATION démarre,
        // la boîte n'ouvrira qu'à la fin de ses 3 s)
        shapes[0].velocity = 0.2;
        docking(&mut state, &mut shapes, &mut triangles, &mut elements);
        assert_eq!(state.dock_anim, DOCK_ANIMATION_DURATION);
        assert!(!state.dock_box);
        assert_eq!(shapes[0].velocity, 0.0);
    }

    #[test]
    fn docking_uses_circular_zone_matching_the_marker() {
        // la zone d'accostage est le cercle de rayon `STATION_DOCK_DISTANCE`
        // (comme la mire affichée au centre de la station) : un coin du carré
        // circonscrit (12,12 — distance ≈ 17 > 15) n'accoste pas, une
        // diagonale à distance < rayon (10,10 — ≈ 14,1 < 15) accoste
        let mut state = GameState::new();
        state.player_at_station = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 12.0, 12.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut elements = default_elements();
        let mut triangles = Vec::new();
        docking(&mut state, &mut shapes, &mut triangles, &mut elements);
        assert!(!state.dock_box);
        assert_eq!(state.player_at_station, 0); // pas docké (coin hors cercle)

        let mut state = GameState::new();
        state.player_at_station = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 10.0, 10.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut elements = default_elements();
        docking(&mut state, &mut shapes, &mut triangles, &mut elements);
        assert_eq!(state.dock_anim, DOCK_ANIMATION_DURATION); // l'animation démarre
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
        let mut triangles = Vec::new();
        elements[1].count = 4;
        state.player.cargo_qty = 4;

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

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
        let mut triangles = Vec::new();
        elements[1].count = 4;
        state.player.cargo_qty = 4;

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        assert!(!state.dock_box);
        assert_eq!(elements[1].count, 0);
        assert_eq!(state.player.cargo_qty, 0);
        assert_eq!(state.player_enter_station, 0);
    }

    #[test]
    fn docking_converts_cargo_but_does_not_buy_supplies() {
        // scénario Progression : le déchargement à la station convertit la
        // soute en minerais (GOLD ×4 = 20) mais n'achète plus le
        // ravitaillement — il se paie via le bouton REFUEL/REARM de la boîte
        // DOCK STATION : réservoirs et minerais intacts, pas de message
        // d'achat
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        state.player_at_station = -1; // déjà docké (boîte refermée)
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 1.0, 1.0),
            test_shape(WHOIAM_STATION, 1, 1, 0.0, 0.0),
        ];
        let mut elements = default_elements();
        let mut triangles = Vec::new();
        elements[1].count = 4;
        state.player.cargo_qty = 4;
        state.resources.fuel = 10.0;
        state.resources.ammo = 5;

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        assert_eq!(elements[1].count, 0);
        assert_eq!(state.player.cargo_qty, 0);
        assert_eq!(state.resources.minerals, 20);
        assert_eq!(state.resources.fuel, 10.0);
        assert_eq!(state.resources.ammo, 5);
        assert!(state.message_queue.contains("CARGO UNLOADED: +20 MINERALS"));
        assert!(!state.message_queue.contains("SUPPLIES PURCHASED"));
    }

    #[test]
    fn refuel_button_purchases_supplies() {
        // le bouton REFUEL/REARM de la boîte DOCK STATION appelle
        // `purchase_supplies` : réservoirs pleins contre minerais (le clic
        // lui-même est testé via le scénario — ici le paiement, pur)
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        state.resources.minerals = 100;
        state.resources.fuel = 10.0;
        state.resources.ammo = 5;

        assert_eq!(
            crate::scenario::purchase_supplies(&mut state),
            crate::scenario::SupplyOutcome::Purchased(14)
        );
        assert_eq!(state.resources.minerals, 86);
        assert_eq!(state.resources.fuel, crate::scenario::fuel_capacity(&state)); // 100
        assert_eq!(state.resources.ammo, crate::scenario::ammo_capacity(&state)); // 30
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
        let mut triangles = Vec::new();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

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

    #[test]
    fn closing_settings_informs_when_mode_changed() {
        // le mode a été modifié pendant l'écran : la fermeture annonce le
        // mode activé au HUD
        let mut state = GameState::new();
        state.settings_box = true;
        state.settings_previous_mode = MOVING_MODE_DIRECTIONAL;
        state.moving_mode = MOVING_MODE_INERTIAL;

        close_settings(&mut state);

        assert!(!state.settings_box);
        assert!(state.message_queue.contains("MOVING MODE: INERTIAL"));
    }

    #[test]
    fn closing_settings_is_silent_when_mode_unchanged() {
        // aucun changement pendant l'écran : pas de message à la fermeture
        let mut state = GameState::new();
        state.settings_box = true;
        state.settings_previous_mode = MOVING_MODE_DIRECTIONAL;
        state.moving_mode = MOVING_MODE_DIRECTIONAL;

        close_settings(&mut state);

        assert!(!state.settings_box);
        assert!(state.message_queue.is_empty());
    }

    #[test]
    fn moving_mode_labels_match_constants() {
        // libellés de l'écran de paramétrage et du message HUD : ordre des
        // constantes MOVING_MODE_*
        assert_eq!(moving_mode_label(MOVING_MODE_INERTIAL), "INERTIAL");
        assert_eq!(moving_mode_label(MOVING_MODE_4_WAYS), "4 WAYS");
        assert_eq!(moving_mode_label(MOVING_MODE_DIRECTIONAL), "DIRECTIONAL");
    }

    #[test]
    fn settings_focus_is_clamped_between_modes() {
        // flèches ↑/↓ : le focus des radio-boutons reste dans
        // [0, MOVING_MODE_COUNT-1]
        assert_eq!(
            settings_focus_move(MOVING_MODE_INERTIAL, -1),
            MOVING_MODE_INERTIAL
        );
        assert_eq!(
            settings_focus_move(MOVING_MODE_DIRECTIONAL, 1),
            MOVING_MODE_DIRECTIONAL
        );
        assert_eq!(settings_focus_move(MOVING_MODE_INERTIAL, 1), MOVING_MODE_4_WAYS);
        assert_eq!(settings_focus_move(MOVING_MODE_4_WAYS, 1), MOVING_MODE_DIRECTIONAL);
        assert_eq!(settings_focus_move(MOVING_MODE_DIRECTIONAL, -1), MOVING_MODE_4_WAYS);
    }

    #[test]
    fn reset_settings_restores_defaults() {
        // bouton RESET : mode DIRECTIONAL (défaut), focus recentré,
        // génération automatique active, rendu texturé, fenêtre 960×540 et
        // anticrénelage éteint (sons non testables hors jeu)
        let mut state = GameState::new();
        state.moving_mode = MOVING_MODE_4_WAYS;
        state.settings_focus = MOVING_MODE_INERTIAL;
        state.auto_generate = false;
        state.render_style = RenderStyle::Mesh;
        state.window_size = 2;
        state.antialias = true;

        reset_settings_fields(&mut state);

        assert_eq!(state.moving_mode, MOVING_MODE_DIRECTIONAL);
        assert_eq!(state.settings_focus, MOVING_MODE_DIRECTIONAL);
        assert!(state.auto_generate);
        assert_eq!(state.render_style, RenderStyle::Textured);
        assert_eq!(state.window_size, 0);
        assert!(!state.antialias);
    }

    #[test]
    fn render_style_and_window_size_cycle_within_bounds() {
        // cycle RENDER : TEXTURED → COLORED → MESH → TEXTURED ; cycle SIZE :
        // borné à WINDOW_SIZES (retour à 0 après la dernière définition)
        let mut style = RenderStyle::Textured;
        for expected in [RenderStyle::Colored, RenderStyle::Mesh, RenderStyle::Textured] {
            style = next_render_style(style);
            assert_eq!(style, expected);
        }
        let mut size = 0;
        for _ in 0..WINDOW_SIZES.len() {
            size = next_window_size(size);
        }
        assert_eq!(size, 0);
        assert_eq!(window_size_dims(0), (960.0, 540.0));
        assert_eq!(window_size_dims(3), (1920.0, 1080.0));
    }

    /// Vaisseau joueur (1 triangle) + cosmonaute EVA (non-collider) + météore
    /// qui chevauche le vaisseau : le décor du test d'éjection.
    fn ejection_scene() -> (GameState, Vec<Shape>, Vec<Triangle>) {
        let mut state = GameState::new();
        // joueur : 1 triangle à (0,0)
        let player = test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0);
        // cosmonaute EVA : garé en bord de monde, non-collider (les météores
        // le traversent : son seul objectif est de rejoindre la base)
        let mut eva = test_shape(WHOIAM_COSMONAUT, 1, 1, -1400.0, -1400.0);
        eva.is_collider = false;
        // météore : 1 triangle, chevauche le vaisseau (distance 2 < rayons 20)
        let meteor = test_shape(WHOIAM_METEOR, 2, 2, 2.0, 2.0);
        let shapes = vec![player, eva, meteor];
        let triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, -1400.0, -1400.0),
            test_triangle(2, 2, 2.0, 2.0),
        ];
        state.eva_cosmonaut = 1;
        (state, shapes, triangles)
    }

    #[test]
    fn destroyed_ship_ejects_the_cosmonaut() {
        // le vaisseau détruit → le cosmonaute EVA apparaît à la position du
        // crash et devient le pilote (`cosmonaut_active`) ; le vaisseau est
        // mort, invisible
        let (mut state, mut shapes, mut triangles) = ejection_scene();
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le vaisseau est détruit, le cosmonaute éjecté au point du crash
        assert_eq!(shapes[PLAYER_INDEX].life, 0);
        assert!(state.cosmonaut_active);
        assert_eq!(shapes[state.eva_cosmonaut as usize].position, shapes[PLAYER_INDEX].position);
        // le pilote suivi par la caméra/mire/HUD est le cosmonaute
        assert_eq!(pilot_index(&state), state.eva_cosmonaut as usize);
        // le cosmonaute n'a pas été endommagé par la collision
        assert_eq!(shapes[state.eva_cosmonaut as usize].life, 1);
    }

    #[test]
    fn cosmonaut_reaching_the_station_is_rescued() {
        // le cosmonaute EVA atteint la zone d'accostage (centre de la station)
        // → il est secouru : le vaisseau est reconstruit à la station (état de
        // lancement) et le cosmonaute retourne à son poste
        let (mut state, mut shapes, mut triangles) = ejection_scene();
        // le vaisseau est détruit et le cosmonaute éjecté au centre (la zone)
        shapes[PLAYER_INDEX].life = 0;
        triangles[0].life = 0;
        state.cosmonaut_active = true;
        let eva = state.eva_cosmonaut as usize;
        shapes[eva].position = Point::new(0.0, 0.0);
        let mut elements = default_elements();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        // secouru : le contrôle revient au vaisseau, reconstruit à quai
        assert!(!state.cosmonaut_active);
        assert_eq!(shapes[PLAYER_INDEX].life, 1);
        assert_eq!(triangles[0].life, 1);
        assert_eq!(shapes[PLAYER_INDEX].position, Point::new(0.0, 0.0));
        assert!(state.dock_links); // démarre à quai, comme au lancement
        assert_eq!(state.player_at_station, -1);
        // le cosmonaute est garé hors écran, à son poste
        assert_eq!(shapes[eva].position, COSMONAUTE_EVA_PARK);
        assert_eq!(pilot_index(&state), PLAYER_INDEX);
    }

    #[test]
    fn cosmonaut_far_from_the_station_is_not_rescued() {
        // le cosmonaute n'est pas encore arrivé : pas de secours, le vaisseau
        // reste détruit
        let (mut state, mut shapes, mut triangles) = ejection_scene();
        shapes[PLAYER_INDEX].life = 0;
        triangles[0].life = 0;
        state.cosmonaut_active = true;
        let eva = state.eva_cosmonaut as usize;
        shapes[eva].position = Point::new(300.0, 200.0); // loin de la base
        let mut elements = default_elements();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        assert!(state.cosmonaut_active);
        assert_eq!(shapes[PLAYER_INDEX].life, 0);
        assert_eq!(shapes[eva].position, Point::new(300.0, 200.0));
    }

    #[test]
    fn eva_cosmonaut_picks_up_nearby_gems() {
        // le cosmonaute EVA ramasse une gemme proche : gemme détruite, son
        // élément compté dans la soute (rapportée à la station au secours)
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_GEM, 2, 2, 105.0, 100.0), // à 5 unités
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 105.0, 100.0),
        ];
        triangles[2].element = 1; // GOLD
        let mut elements = default_elements();

        eva_collect_gems(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 0, "la gemme est ramassée");
        assert_eq!(triangles[2].life, 0);
        assert_eq!(elements[1].count, 1);
        assert_eq!(state.player.cargo_qty, 1);
        // le cosmonaute n'est pas touché par le ramassage
        assert_eq!(shapes[1].life, 1);
    }

    #[test]
    fn eva_cosmonaut_ignores_distant_or_inactive_gems() {
        // gemme trop loin du cosmonaute : pas de ramassage
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_GEM, 2, 2, 200.0, 100.0), // à 100 unités
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 200.0, 100.0),
        ];
        triangles[2].element = 1;
        let mut elements = default_elements();

        eva_collect_gems(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 1, "gemme trop loin");
        assert_eq!(state.player.cargo_qty, 0);

        // vaisseau intact (pas d'EVA) : le cosmonaute garé ne ramasse rien
        let mut state = GameState::new();
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_GEM, 2, 2, 105.0, 100.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 105.0, 100.0),
        ];
        triangles[2].element = 1;
        let mut elements = default_elements();

        eva_collect_gems(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 1, "vaisseau intact : pas d'EVA");
        assert_eq!(state.player.cargo_qty, 0);
    }
}
