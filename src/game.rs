//! Boucle de jeu - portage de `mainLoop.bas`.
//!
//! Jalon M2 : input (déplacement du vaisseau, 4 modes), physique, monde
//! torique (via `moving_shape`), pause, plein écran.
//! Jalon M3 : météores - génération en jeu (touche G + automatique), détection
//! de collisions (SAT) avec choc élastique, résolution (destruction de
//! triangles, débris, messages, centres). Les balles (M4), l'accostage (M5)
//! et les sons (M4+) viendront ensuite.

use macroquad::prelude::*;
use ::rand::Rng;

use crate::audio::Sounds;
use crate::config::*;
use crate::cosmonaut::{animate_eva_cosmonaut, COSMONAUTE_EVA_PARK};
// gameplay « météores & collisions » (force de réaction à la base, débris,
// plafond et génération des météores) : constantes de la carte éponyme de
// l'outil de gestion (src/marketplace.rs, généré)
use crate::marketplace::*;
use crate::garbage::{generate_garbages, moving_garbage, Garbage};
use crate::generate::{
    create_alien, create_mineral, create_shape, eject_cargo_minerals, fire_bullet, release_meteor_minerals,
};
use crate::persist;
use crate::scenario;
use crate::geom::{Point, Triangle};
use crate::render::{
    camera_for, choice_box_layout, cycle_view_mode, enter_fullscreen, help_box_layout, mouse_to_game,
    settings_box_layout, shop_box_layout,
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

/// Traite l'input, les contrôles joueur, la physique et les collisions pour
/// une frame. Renvoie l'action demandée et la caméra (centrée joueur) à
/// utiliser pour le rendu.
///
/// Ordre fidèle à `mainLoop` : input → contrôles → compteurs de poussée →
/// remise à zéro des indicateurs de collision → déplacements (formes gelées
/// en pause, débris toujours actifs - comportement de l'original) →
/// collisions → caméra → génération automatique.
pub fn update(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    garbages: &mut Vec<Garbage>,
    elements: &mut [Element],
    rng: &mut impl Rng,
    // Sons du jeu - `None` (tests) pour un `update` silencieux.
    mut sounds: Option<&mut Sounds>,
    dt: f64,
) -> (Action, Point) {
    // FPS mesurés (affichés au HUD, utilisés par les messages en Phase 4)
    state.fps = get_fps();

    // Caméra de la frame précédente - utilisée par la touche G comme
    // l'original (qui lit `camera` calculée à l'itération précédente). Elle
    // suit le pilote : le vaisseau, ou le cosmonaute EVA quand le vaisseau
    // est détruit.
    let mut camera = camera_for(state, &shapes[pilot_index(state)]);

    // Écran de paramétrage ouvert (touche O) : seul l'input de l'écran est
    // traité (voir `handle_settings_input`) - le monde, lui, **continue de
    // tourner** : les météores et les débris dérivent derrière l'écran (le
    // vaisseau reste vulnérable, seule la touche P gèle le monde). Un clic
    // sur RESTART demande la relance du jeu ; un clic sur RESET PROGRESSION
    // reconstruit le vaisseau (les plans liés aux extensions achetées
    // disparaissent avec les niveaux remis à zéro).
    if state.settings_box {
        let result = handle_settings_input(state, sounds.as_deref_mut());
        if result.progression_reset {
            crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
        }
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        let action = if result.restart { Action::Restart } else { Action::Continue };
        return (action, camera);
    }

    // ESC : quitter
    if is_key_pressed(KeyCode::Escape) {
        return (Action::Quit, camera);
    }

    // Game over (scénario Survival, dernière vie perdue) : le monde est gelé
    // - seules les touches de quitter (ESC, ci-dessus) restent actives ; le
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

    // Fenêtre d'aide ouverte (touche S) : seul le bouton CLOSE est traité
    // (ex boucle bloquante de `windowUtils_help`) - le monde, lui,
    // **continue de tourner** : les météores et les débris dérivent derrière
    // la fenêtre (le vaisseau reste vulnérable, seule la touche P gèle le
    // monde).
    if state.help_box {
        if help_box_click() {
            state.help_box = false;
        }
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

    // Animation d'accostage (3 s, avant la boîte DOCK STATION) : le vaisseau
    // pivote vers la droite (orientation 0) tout en se recentrant au centre
    // de la station (voir `advance_dock_animation` et
    // `render::draw_docking_line`). Le monde, lui, **continue de tourner** :
    // les météores et les débris dérivent autour de la base pendant
    // l'animation (voir `collisions` - le vaisseau qui s'aligne est protégé,
    // comme à quai). Seule la touche P (pause) gèle le monde.
    if state.dock_anim > 0.0 {
        advance_dock_animation(state, shapes, triangles, dt);
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

    // Récupération du cosmonaute EVA (vaisseau détruit, il a rejoint la base) :
    // un cordon jaillit de l'anneau jusqu'à lui puis le ramène sur l'anneau
    // (`advance_eva_recovery`, cordon dessiné par
    // `render::draw_eva_recovery_cable`), puis le **fondu enchaîné** fait
    // apparaître le vaisseau reconstruit au centre, liens attachés
    // (`advance_eva_crossfade` - la caméra glisse de l'anneau vers le centre).
    // Le monde, lui, **continue de tourner** : les météores et les débris
    // dérivent pendant toute la séquence (le cosmonaute est un non-collider,
    // il ne peut pas être percuté pendant que le cordon le tire).
    if state.eva_recovery > 0.0 {
        advance_eva_recovery(state, shapes, triangles, dt);
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }
    if state.eva_crossfade > 0.0 {
        let camera = advance_eva_crossfade(state, shapes, triangles, dt);
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

    // Le vaisseau démarre de la base (lancement ou respawn : liens attachés à
    // quai, mire cachée - voir `state.dock_links`) : dès que le joueur donne
    // une commande de déplacement (flèches, tous modes), les liens se
    // rétractent (même animation qu'au départ après CLOSE), puis le vaisseau
    // est libre. Entrée ouvre la boîte DOCK STATION (UNLOAD / SHOP / CLOSE)
    // pour décharger ou faire ses achats sans quitter l'accostage - sinon la
    // boîte ne s'ouvre qu'au bout de l'animation d'accostage automatique.
    if state.dock_links {
        if is_key_pressed(KeyCode::Enter) {
            state.dock_box = true;
        } else if player_moving_input() {
            release_links(state);
        }
    }

    // Rétraction des liens d'accostage au départ (CLOSE de la boîte ou
    // démarrage de la base) : le vaisseau reste au centre, les 4 traits néon
    // se rétractent vers le bord intérieur de l'anneau (voir
    // `advance_dock_retract` et `render::draw_docking_line`), puis le
    // vaisseau est libre. Le monde, lui, **continue de tourner** : les
    // météores et les débris dérivent pendant la rétraction (le vaisseau
    // tenu au centre est protégé, comme à quai).
    if state.dock_retract > 0.0 {
        advance_dock_retract(state, shapes, triangles, dt);
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

    // Boîte de choix DOCK STATION ouverte : seuls les clics sur UNLOAD /
    // SHOP / CLOSE sont traités (ex boucle bloquante de
    // `windowUtils_choiceBox`). UNLOAD décharge la soute (crédits
    // disponibles pour le ravitaillement juste après) et SHOP ouvre le
    // magasin de la station - le carburant et les munitions s'y achètent
    // indépendamment (section RAVITAILLEMENT, plus de bouton REFUEL/REARM) ;
    // la boîte ne se ferme qu'avec CLOSE, pour décharger puis se ravitailler
    // dans le même accostage. Le vaisseau est gelé, mais le **monde, lui,
    // continue** : les météores et les débris dérivent autour de la base
    // (voir `collisions` - le vaisseau à quai est protégé). Après CLOSE, la
    // rétraction des liens garde elle aussi le monde vivant (vaisseau
    // protégé au centre).
    if state.dock_box {
        match choice_box_click() {
            ChoiceClick::None => {}
            ChoiceClick::Unload => {
                // déchargement immédiat - NB : l'original ignore le choix
                // (`r%` non utilisé) et vide la soute de toute façon à
                // l'accostage (branche « else » de `docking`, frame
                // suivante) ; ici il est anticipé pour financer le
                // ravitaillement du même accostage
                scenario::unload_cargo(state, elements);
                for e in elements.iter_mut() {
                    e.count = 0;
                }
                state.player.cargo_qty = 0;
                // la progression (crédits) est persistée au déchargement
                let _ = scenario::save_progression(state);
            }
            ChoiceClick::Shop => {
                // ouvre le magasin de la station (la boîte réapparaît en
                // fermant le magasin - on reste accosté) ; les curseurs du
                // ravitaillement partent d'office sur le **maximum achetable**
                // avec les minerais courants (on peut toujours s'en sortir
                // même avec peu de minerais - la quantité se règle ensuite à
                // la souris / la molette)
                state.dock_box = false;
                state.shop_box = true;
                state.shop_drag = None;
                state.shop_tab = crate::config::SHOP_TAB_SUPPLIES; // onglet ravitaillement par défaut
                state.shop_feedback.clear();
                state.shop_fuel_qty = scenario::affordable_fuel_qty(state);
                for i in 0..scenario::weapon_slot_count() {
                    if scenario::weapon_owned(state, i) {
                        state.shop_ammo_qty[i] = scenario::affordable_ammo_qty(state, i) as f64;
                    }
                }
            }
            ChoiceClick::Close => {
                // quitte l'accostage : les liens néon se rétractent
                // (animation de `DOCK_RETRACT_DURATION`, monde vivant)
                undock(state);
            }
        }
        // toujours à quai (CLOSE lance la rétraction, traitée en tête de
        // frame) : le monde continue de vivre - météores et débris dérivent,
        // se heurtent à la base (indestructible) et entre eux, tandis que le
        // vaisseau accosté reste intact (voir `collisions`)
        if state.dock_box {
            collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        }
        return (Action::Continue, camera);
    }

    // Magasin de la station ouvert (bouton SHOP de la boîte DOCK STATION) :
    // les curseurs du ravitaillement sont mis à jour à chaque frame
    // (`shop_update` : glisser, molette, bornage aux crédits), puis les
    // clics sur les lignes de mode de déplacement (sélection gratuite ou
    // déblocage contre crédits, `scenario::try_select_mode`), les lignes
    // d'extension (achat contre crédits, `scenario::buy_upgrade`), les
    // lignes de ravitaillement (achat de la **quantité du curseur**) et sur
    // CLOSE (retour à la boîte DOCK STATION, toujours accosté) sont traités.
    // Le vaisseau est gelé, mais le **monde, lui, continue** : les météores
    // et les débris dérivent autour de la base (voir `collisions` - le
    // vaisseau à quai est protégé).
    if state.shop_box {
        shop_update(state);
        match shop_box_click(state) {
            ShopClick::None => {}
            ShopClick::Mode(mode) => select_mode_and_save(state, mode),
            ShopClick::Weapon(i) => buy_weapon_and_save(state, shapes, triangles, i),
            ShopClick::BuyRadar => buy_radar_and_save(state),
            // ravitaillement : carburant et munitions achetés indépendamment,
            // à la quantité choisie sur le curseur de la ligne (crédits
            // persistés) - le résultat (achat / refus / plein) s'affiche dans
            // le pied de la fenêtre (`shop_feedback`)
            ShopClick::Refuel => {
                match scenario::buy_fuel_qty(state, state.shop_fuel_qty) {
                    scenario::SupplyOutcome::Purchased(cost) => {
                        state.shop_feedback = format!("Carburant acheté (-{} CR)", cost);
                        state.shop_feedback_ok = true;
                    }
                    scenario::SupplyOutcome::Insufficient(_) => {
                        state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
                        state.shop_feedback_ok = false;
                    }
                    scenario::SupplyOutcome::Full => state.shop_feedback.clear(),
                }
                let _ = scenario::save_progression(state);
            }
            ShopClick::Rearm(i) => {
                match scenario::buy_ammo_qty(state, i, state.shop_ammo_qty[i] as i32) {
                    scenario::SupplyOutcome::Purchased(cost) => {
                        state.shop_feedback = format!("Munitions achetées (-{} CR)", cost);
                        state.shop_feedback_ok = true;
                    }
                    scenario::SupplyOutcome::Insufficient(_) => {
                        state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
                        state.shop_feedback_ok = false;
                    }
                    scenario::SupplyOutcome::Full => state.shop_feedback.clear(),
                }
                let _ = scenario::save_progression(state);
            }
            ShopClick::RefillAll => {
                // plein de carburant + munitions (toutes armes possédées) au
                // maximum achetable - un seul clic pour tout ravitailler
                let had_missing =
                    (scenario::fuel_capacity(state) - state.resources.fuel).max(0.0) > 0.0
                        || (0..scenario::weapon_slot_count())
                            .filter(|&i| scenario::weapon_owned(state, i))
                            .any(|i| scenario::ammo_capacity(state) - state.resources.weapon_ammo[i] > 0);
                let mut spent = 0;
                let fuel_missing = (scenario::fuel_capacity(state) - state.resources.fuel).max(0.0);
                if let scenario::SupplyOutcome::Purchased(c) =
                    scenario::buy_fuel_qty(state, fuel_missing)
                {
                    spent += c;
                }
                for i in 0..scenario::weapon_slot_count() {
                    if scenario::weapon_owned(state, i) {
                        let missing =
                            (scenario::ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0);
                        if let scenario::SupplyOutcome::Purchased(c) =
                            scenario::buy_ammo_qty(state, i, missing)
                        {
                            spent += c;
                        }
                    }
                }
                if spent > 0 {
                    state.shop_feedback = format!("Ravitaillement complet (-{} CR)", spent);
                    state.shop_feedback_ok = true;
                } else if had_missing {
                    state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
                    state.shop_feedback_ok = false;
                } else {
                    state.shop_feedback = "Tout est déjà plein".to_string();
                    state.shop_feedback_ok = true;
                }
                let _ = scenario::save_progression(state);
            }
            ShopClick::BuyFuelUpgrade => {
                buy_upgrade_and_save(state, shapes, triangles, scenario::UpgradeTrackId::Fuel)
            }
            ShopClick::BuyAmmoUpgrade => {
                buy_upgrade_and_save(state, shapes, triangles, scenario::UpgradeTrackId::Ammo)
            }
            ShopClick::BuyCargoUpgrade => {
                buy_upgrade_and_save(state, shapes, triangles, scenario::UpgradeTrackId::Cargo)
            }
            ShopClick::Close => {
                state.shop_box = false;
                state.dock_box = true;
                state.shop_feedback.clear();
            }
        }
        // toujours à quai (CLOSE ramène à la boîte DOCK STATION) : le monde
        // continue de vivre - météores et débris dérivent (vaisseau protégé)
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

    // F : cycle des modes d'affichage - fenêtré → plein écran zoomé (render
    // target étirée) → plein écran natif (définition réelle, sans buffer) →
    // fenêtré (le HUD annonce le mode activé à chaque pression). Détection
    // robuste (`f_pressed`) : une pression avalée par le filtre de répétition
    // de macroquad après une bascule plein écran reste comptée - sans quoi il
    // faut presser F deux fois pour changer de mode.
    if f_pressed(state) {
        cycle_view_mode(state);
        state.send_message(crate::config::view_mode_message(state.view_mode as i32));
    }

    // M : bascule la musique (ex `M : mute music` de mainLoop) - persistée
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

    // A : génération automatique des météores (ex `autoGenerateShape%`) -
    // pour la session en cours uniquement (repart active au lancement, voir
    // `main.rs` - non persistée)
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

    // O : écran de paramétrage (options audio et graphiques - le mode de
    // déplacement se choisit au magasin de la station, bouton SHOP)
    if is_key_pressed(KeyCode::O) {
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
    // frame (comme le cooldown de tir) - à 0, le vaisseau redevient vulnérable
    if state.invulnerable > 0.0 {
        state.invulnerable = (state.invulnerable - dt).max(0.0);
    }

    // contrôles joueur selon le mode de déplacement (inclut le tir + son)
    player_controls(state, shapes, triangles, sounds.as_deref_mut(), dt);

    // compteurs de poussée : -5 à la pression, +1 par frame jusqu'à 0 -
    // la flamme (et le son, Phase 4) persiste ~5 frames après relâchement.
    if state.player.thrusted != 0 {
        state.player.thrusted += 1;
    }
    if state.player.revert_thrusted != 0 {
        state.player.revert_thrusted += 1;
    }
    // idem pour les jets latéraux de rotation (touches ← et →)
    if state.player.rotate_left_thrusted != 0 {
        state.player.rotate_left_thrusted += 1;
    }
    if state.player.rotate_right_thrusted != 0 {
        state.player.rotate_right_thrusted += 1;
    }

    // animation des membres du cosmonaute EVA : bras et jambes qui **s'agitent
    // pendant la poussée** puis retombent au repos (`cosmonaut::animate_eva_cosmonaut`)
    // - avant la physique : `moving_shape` recalcule les positions réelles des
    // triangles animés dans la foulée. Garé (vaisseau intact), il revient au repos.
    if state.eva_cosmonaut >= 0 {
        let eva = state.eva_cosmonaut as usize;
        let thrusting = state.cosmonaut_active && state.player.thrusted != 0;
        animate_eva_cosmonaut(&mut shapes[eva], triangles, thrusting, get_time(), dt);
    }

    // scénario à économie : le carburant est consommé tant que le moteur
    // est allumé (flamme avant/arrière) - annonce OUT OF FUEL à la rupture.
    // Pas en mode cosmonaute EVA (le vaisseau est détruit, le carburant ne
    // sert plus - la combinaison ne brûle pas le réservoir)
    if !state.cosmonaut_active {
        scenario::consume_fuel(state, dt);
    }

    // physique + collisions (détection, résolution, sons d'impact)
    collisions(state, shapes, triangles, garbages, elements, rng, sounds, dt);

    // guide d'accostage : la mire ne s'affiche que lors du RETOUR à la base
    // (voir `update_docking_guide`) - avant `docking`, qui peut déclencher
    // l'animation d'accostage (et couper le guide). Le pilote suit le
    // cosmonaute EVA quand le vaisseau est détruit.
    let pilot = pilot_index(state);
    update_docking_guide(
        state,
        shapes[pilot].position,
        shapes[STATION_INDEX].position,
        shapes[STATION_INDEX].radius,
    );

    // accostage à la station (ex « detect return to the base ») - peut ouvrir
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
    // n'est pas atteinte (ex `mainLoop`) - non gelée par la pause, comme
    // l'original.
    if state.auto_generate && alive_shapes < state.max_meteor_shapes && rng.gen::<f64>() > 0.95 {
        create_shape(state, shapes, triangles, camera, elements, rng);
    }

    // Suivi des objectifs DAG (scénarios custom) : vérifie les conditions,
    // attribue les récompenses et affiche les messages.
    // On extrait le tracker temporairement pour éviter l'emprunt croisé
    // (le tracker lit `state` pendant que `update` le modifie).
    let mut tracker = std::mem::take(&mut state.objective_tracker);
    if tracker.has_objectives() {
        let results = tracker.update(state, dt);
        for result in results {
            // Appliquer la récompense
            match result.reward.reward_type.as_str() {
                "Credits" | "Minerals" => {
                    // `Minerals` (ancien nom) reste accepté pour les
                    // scénarios écrits avant le renommage minerais → crédits
                    state.resources.credits += result.reward.amount as i32;
                }
                "Reputation" => {
                    state.resources.reputation += result.reward.amount;
                }
                "Fuel" => {
                    let cap = crate::scenario::fuel_capacity(state);
                    state.resources.fuel = (state.resources.fuel + result.reward.amount).min(cap);
                }
                "Ammo" => {
                    let cap = crate::scenario::ammo_capacity(state);
                    for i in 0..crate::scenario::weapon_slot_count() {
                        if crate::scenario::weapon_owned(state, i) {
                            state.resources.weapon_ammo[i] =
                                (state.resources.weapon_ammo[i] + result.reward.amount as i32).min(cap);
                        }
                    }
                }
                "Victory" => {
                    state.send_message("OBJECTIVE: VICTORY!");
                }
                _ => {}
            }
            // Message HUD
            let title = tracker
                .objective_title(&result.id)
                .unwrap_or(&result.id);
            state.send_message(&format!(">> {} <<", title));
        }
    }
    tracker.tick(dt);
    state.objective_tracker = tracker;

    // télécommande (`remote.rs`) : publie l'état du jeu (HUD, ressources) que
    // la page du téléphone affiche en direct via `GET /state`
    crate::remote::publish_state(state);

    (Action::Continue, camera)
}

/// Physique et collisions pour une frame (ex sections « moves shapes »,
/// « moves garbages », « detects collisions », « resolves collisions » de
/// `mainLoop`).
///
/// Seuls les déplacements des formes sont gelés en pause ; les débris, les
/// collisions et la génération automatique continuent (comportement exact de
/// l'original). À quai (boîte DOCK STATION ou magasin ouverts, voir
/// `update`) et pendant les cinématiques d'accostage (animation d'accostage,
/// rétraction des liens, fondu enchaîné du secours EVA), les météores
/// continuent de dériver mais le vaisseau est **protégé** : aucune collision
/// avec lui n'est détectée. Le monde ne se fige que sur la touche P (pause).
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
    // vaisseau « tenu » (boîte DOCK STATION ou magasin ouverts, animation
    // d'accostage, rétraction des liens, fondu enchaîné du secours EVA) : il
    // est **protégé** - il reste intact pendant que les météores dérivent
    // autour de la base (aucun impact qui l'endommagerait, aucun choc
    // élastique qui le pousserait hors de l'anneau, aucun vol de minerai de
    // soute)
    let player_docked = state.dock_box
        || state.shop_box
        || state.dock_anim > 0.0
        || state.dock_retract > 0.0
        || state.eva_crossfade > 0.0;
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
            // vaisseau à quai : aucune collision avec lui (protégé)
            if player_docked && (i == PLAYER_INDEX || j == PLAYER_INDEX) {
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
                if detect_collision(&shapes[i], &shapes[j], i, j, triangles) {
                    // pas de choc élastique entre un minerai et (vaisseau ou
                    // météore), ni avec la station
                    let no_elastic = (shapes[i].who_i_am == WHOIAM_MINERAL
                        && (shapes[j].who_i_am == WHOIAM_PLAYER || shapes[j].who_i_am == WHOIAM_METEOR))
                        || (shapes[j].who_i_am == WHOIAM_MINERAL
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
        let collid_by_who = shapes[collid_by as usize].who_i_am;

        if collid_by_who == WHOIAM_PLAYER
            && who == WHOIAM_MINERAL
            && state.player.cargo_qty < state.player.cargo_size
        {
            // ramassage d.un minerai (M4 - nécessite les balles pour créer des
            // minerais) : détruit, son élément est compté dans la soute
            shapes[shape_index].life = 0;
            triangles[i].life = 0;
            let element = triangles[i].element as usize;
            if element < elements.len() {
                elements[element].count += 1;
            }
            state.player.cargo_qty += 1;
            if let Some(sounds) = sounds.as_mut() {
                sounds.play_mineral();
            }
            if state.player.cargo_qty >= state.player.cargo_size {
                state.send_message("YOUR LOADING BAY IS FULL, YOU MUST UNLOAD IT AT THE STATION");
            }
        } else if collid_by_who == WHOIAM_MINERAL && who == WHOIAM_PLAYER {
            // déjà résolu côté minerai (cargaison pleine)
        } else if collid_by_who == WHOIAM_STATION && who == WHOIAM_PLAYER {
            // accostage (M5)
        } else if who == WHOIAM_STATION {
            // la station est indestructible
        } else if who == WHOIAM_PLAYER && scenario::has_survival(state) {
            // scénario Survival : le bouclier encaisse les impacts (le
            // triangle du vaisseau n'est pas tué) ; s'il est percé, le
            // vaisseau est détruit - une vie est perdue et il respawne à la
            // station (bouclier rechargé par le scénario), ou la partie est
            // terminée en dernière vie (le monde se gèle, HUD GAME OVER)
            let shield_before = state.resources.shield;
            let lives_before = state.resources.lives;
            match scenario::player_hit(state, 1.0) {
                scenario::PlayerHit::Absorbed => {}
                scenario::PlayerHit::Destroyed(_) => {
                    // les minerais collectés sont rejetés autour du crash
                    // avant le respawn (la position du vaisseau est encore
                    // celle du crash) - à récupérer en revenant sur place
                    eject_cargo_minerals(state, shapes, triangles, elements, rng);
                    respawn_player(state, shapes, triangles);
                }
                scenario::PlayerHit::GameOver => {
                    // dernière vie perdue : le vaisseau reste détruit - son
                    // chargement est rejeté autour du crash comme ailleurs
                    triangles[i].life = 0;
                    shapes[PLAYER_INDEX].life = 0;
                    eject_cargo_minerals(state, shapes, triangles, elements, rng);
                }
            }
            // la progression Survival (vies, bouclier) est persistée quand un
            // impact l'a modifiée (pas à chaque impact absorbé par
            // l'invulnérabilité post-respawn, qui ne change rien)
            if state.resources.shield != shield_before || state.resources.lives != lives_before {
                let _ = scenario::save_progression(state);
            }
        } else if collid_by_who == WHOIAM_METEOR && who == WHOIAM_MINERAL {
            // un météore percute un minerai : il l.absorbe - le minerai
            // disparaît entièrement et la quantité de minerai du météore
            // augmente (`minerals`, libérée si le météore est lui-même
            // détruit par un autre météore). Le météore le plus proche de la
            // minerai est celui qui l.a percuté (`collid_by` ne porte que le
            // type, pas l.index). Une seule fois par minerai (tout le minerai
            // est tué au premier triangle). NB : un minerai **rejeté de la
            // soute** du vaisseau détruit (`ejected_cargo`) n'est PAS absorbée
            // - elle doit rester ramassable par le cosmonaute EVA (ou le
            // vaisseau ressuscité en Survival), le minerai n'est pas perdu
            // avec le crash ; sans choc élastique (météore/minerai), il
            // traverse simplement le météore.
            if shapes[shape_index].life > 0 && !shapes[shape_index].ejected_cargo {
                let mineral_pos = shapes[shape_index].position;
                if let Some(meteor) = nearest_meteor(shapes, mineral_pos) {
                    shapes[meteor].minerals += 1;
                }
                shapes[shape_index].life = 0;
                for j in shapes[shape_index].first_triangle..=shapes[shape_index].last_triangle {
                    triangles[j].life = 0;
                }
            }
        } else if collid_by_who == WHOIAM_MINERAL && who == WHOIAM_METEOR {
            // déjà résolu côté minerai (absorption) : le météore n.est pas
            // endommagé en avalant le minerai (un minerai de soute, lui,
            // traverse sans rien faire)
        } else if who == WHOIAM_PLAYER {
            // vaisseau joueur : mesh multi-triangles (35 faces) mais toujours
            // « 1 impact = détruit » (l'ancien triangle unique valait 1 vie)
            // - tous les triangles meurent en même temps, le vaisseau ne
            // s'effrite pas impact après impact (une seule fois : `life`
            // passe à 0, les autres triangles en collision de la même frame
            // ne refont rien)
            if shapes[shape_index].life > 0 {
                shapes[shape_index].life = 0;
                for j in shapes[shape_index].first_triangle..=shapes[shape_index].last_triangle {
                    triangles[j].life = 0;
                }
                state.send_message("YOUR SPACESHIP IS DAMAGED, THE STATION CAN CARRY OUT REPAIRS");
                state.send_message("REPAIRS ARE NOT FREE OF CHARGE");
                // vaisseau détruit (jeu libre/Progression - le Survival a son
                // propre respawn) : le cosmonaute est éjecté à la position du
                // crash - le joueur le contrôle pour rejoindre la base (une
                // seule fois : `cosmonaut_active`)
                if !state.cosmonaut_active {
                    activate_cosmonaut(state, shapes, triangles);
                }
                // les minerais collectés sont **rejetés autour** du crash :
                // la soute est vidée en minerais éparpillés à proximité, que
                // le cosmonaute pourra ramasser pour les ramener à la station
                eject_cargo_minerals(state, shapes, triangles, elements, rng);
                // débris du crash + son d'impact (comme pour toute forme
                // détruite - voir la branche générique ci-dessous)
                if let Some(sounds) = sounds.as_mut() {
                    let dx = shapes[shape_index].position.x - shapes[PLAYER_INDEX].position.x;
                    let dy = shapes[shape_index].position.y - shapes[PLAYER_INDEX].position.y;
                    let dist = dx.hypot(dy);
                    let v = (1.0 - dist / WORLD_WIDTH.hypot(WORLD_HEIGHT)).powi(3) as f32;
                    sounds.play_explosion(rng, v);
                }
                generate_garbages(garbages, &triangles[i], shapes, rng);
            }
        } else {
            triangles[i].life = 0;
            // ne descend jamais sous 0 : une forme déjà morte (ex une munition
            // dont les triangles ont été tués côté météore) reste morte
            if shapes[shape_index].life > 0 {
                shapes[shape_index].life -= 1;
            }
            // un météore qui percute la **station** (base indestructible)
            // subit une force de réaction : le triangle explose (débris +
            // son ci-dessous) et sa composante de vitesse vers la base est
            // réfléchie - l'explosion repousse le météore, la composante
            // tangentielle (glissement le long de l'anneau) est conservée.
            // Une seule fois par météore par frame : le premier triangle en
            // collision renverse la composante radiale, les suivants voient
            // déjà `vn >= 0` et ne refont rien.
            if who == WHOIAM_METEOR
                && collid_by_who == WHOIAM_STATION
                && shapes[shape_index].life > 0
            {
                // normale du choc : du centre de la base vers le point
                // d'impact - le centre réel du triangle qui explose
                let dx = triangles[i].real_center.x - shapes[STATION_INDEX].position.x;
                let dy = triangles[i].real_center.y - shapes[STATION_INDEX].position.y;
                let norm = dx.hypot(dy);
                if norm > 0.0 {
                    let (nx, ny) = (dx / norm, dy / norm);
                    // vitesse monde du météore (y inversé : `moving_shape`
                    // soustrait `sin(direction)·v`)
                    let speed = shapes[shape_index].velocity;
                    let vx = speed * shapes[shape_index].direction.cos();
                    let vy = -speed * shapes[shape_index].direction.sin();
                    let vn = vx * nx + vy * ny; // radiale (négative = vers la base)
                    if vn < 0.0 {
                        // seule la TRAJECTOIRE est réfléchie : la direction
                        // du rebond (selon la restitution) est recalculée,
                        // mais la VITESSE du météore est conservée - le choc
                        // avec la base repousse le météore sans le ralentir
                        let rx = vx - (1.0 + METEOR_STATION_RESTITUTION) * vn * nx;
                        let ry = vy - (1.0 + METEOR_STATION_RESTITUTION) * vn * ny;
                        let r = rx.hypot(ry);
                        if r > 0.0 {
                            let d = (-ry).atan2(rx);
                            shapes[shape_index].direction = if d < 0.0 { d + TAU } else { d };
                            shapes[shape_index].velocity = speed;
                        }
                    }
                }
            }
            // collision vaisseau/minerai non résolue parce que soute pleine
            if collid_by_who == WHOIAM_PLAYER && who == WHOIAM_MINERAL {
                state.send_message("YOU CANNOT TAKE ANY ADDITIONAL RESOURCES, UNLOAD AT THE STATION");
            }
            // si le joueur détruit un météore, la limite de météores augmente
            // (ex mainLoop : compteur + « R+1 » affiché - le bonus flottant
            // et les sons arrivent en M4) et la réputation du scénario
            // augmente (d'autant plus que la précision de tir est bonne)
            if collid_by_who == WHOIAM_BULLET
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
            // toute munition qui touche un triangle de météore est détruite en
            // même temps que ce triangle : elle ne traverse pas le météore -
            // un tir consomme au plus un triangle (le météore survit s'il en
            // reste). Idempotent : plusieurs triangles du même météore
            // chevauchés la même frame ne la détruisent qu'une fois.
            if collid_by_who == WHOIAM_BULLET && who == WHOIAM_METEOR {
                let bullet_idx = collid_by as usize;
                if bullet_idx < shapes.len() && shapes[bullet_idx].who_i_am == WHOIAM_BULLET {
                    shapes[bullet_idx].life = 0;
                    for t in shapes[bullet_idx].first_triangle..=shapes[bullet_idx].last_triangle {
                        triangles[t].life = 0;
                    }
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
            // libère son minerai : un minerai apparaît (le minerai n.est pas
            // détruit avec le météore). Un missile qui touche directement
            // un minerai, lui, le détruit - pas de nouveau minerai : c.est le
            // seul cas de destruction de minerai (`who == WHOIAM_MINERAL` n'entre
            // pas ici).
            if triangles[i].element > 0 && who == WHOIAM_METEOR {
                if collid_by_who == WHOIAM_BULLET && triangles[i].element > 0 {
                    let source = triangles[i];
                    create_mineral(shapes, triangles, elements, &source, rng);
                    if shapes[shape_index].minerals > 0 {
                        shapes[shape_index].minerals -= 1;
                    }
                }
            }
            // le météore est détruit (par un autre météore ou par un missile
            // du vaisseau) : ses minerais restants - absorbés de minerais
            // mangés - sont libérés en minerais à sa position, jamais détruits
            // avec lui. Une seule fois : `minerals` passe à 0 dans
            // `release_meteor_minerals`, les triangles suivants du même
            // météore ne relibèrent rien.
            if who == WHOIAM_METEOR
                && shapes[shape_index].life <= 0
                && (collid_by_who == WHOIAM_METEOR || collid_by_who == WHOIAM_BULLET)
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

    // ramassage des minerais par le **cosmonaute EVA** (vaisseau détruit) : il
    // les ramasse par proximité (non-collider - les minerais le traversent) et
    // les **rapporte à la station** : la soute est déchargée à l'accostage
    // après le secours (`docking`/`rescue_cosmonaut`), comme pour le vaisseau
    eva_collect_minerals(state, shapes, triangles, elements, sounds);
}

/// Ramassage des minerais par le **cosmonaute EVA** : chaque minerai dont le
/// centre entre dans le rayon `EVA_PICKUP_RADIUS` du cosmonaute est ramassée
/// - détruite, son élément est compté dans la **même soute que le vaisseau**
/// (déchargée en crédits à la station après le secours). Soute pleine, plus
/// de ramassage. Sans effet quand le vaisseau est intact (`cosmonaut_active`
/// faux) : le cosmonaute garé ne ramasse rien.
fn eva_collect_minerals(
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
        if g == eva || shapes[g].who_i_am != WHOIAM_MINERAL || shapes[g].life <= 0 {
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
        // ramassage : le minerai est détruit, son élément compté dans la soute
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
            sounds.play_mineral();
        }
        if state.player.cargo_qty >= state.player.cargo_size {
            state.send_message("YOUR LOADING BAY IS FULL, YOU MUST UNLOAD IT AT THE STATION");
        }
    }
}

/// Météore vivant le plus proche d'une position donnée - utilisé par
/// l.absorption d.un minerai : `collid_by` ne porte que le type
/// (`WHOIAM_METEOR`), pas l'index de la forme qui a percuté le minerai - on
/// attribue donc l.absorption au météore le plus proche du minerai (celui
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
/// Survival - `scenario::PlayerHit::Destroyed`) : position, rotation et
/// vitesse remises à zéro (comme au départ), coque et triangles réparés - le
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
    // le vaisseau est un mesh multi-triangles reconstruit avec la composition
    // courante (les plans liés aux upgrades apparaissent selon les niveaux
    // d'atelier - `vaisseau::rebuild_player_vaisseau`, qui recale aussi les
    // positions réelles sur les cinématiques posées ci-dessus)
    crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
    // flamme et cooldown de tir coupés (le moteur ne brûle plus au respawn)
    state.player.thrusted = 0;
    state.player.revert_thrusted = 0;
    state.player.rotate_left_thrusted = 0;
    state.player.rotate_right_thrusted = 0;
    state.player.fire = 0.0;
    state.player_at_station = -1; // docké à la station (comme au lancement)
    state.player_enter_station = 0;
    // à quai : les liens d'accostage se rattachent au vaisseau (mire cachée)
    // jusqu'à ce que le joueur reparte (rétraction, voir `release_links`)
    state.dock_links = true;
}

/// Le vaisseau est détruit (jeu libre/Progression) : le joueur devient le
/// **cosmonaute éjecté** - il apparaît à la position du crash (le vaisseau
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
    state.send_message("SHIP DESTROYED - RETURN TO THE STATION");
}

/// Le cosmonaute EVA a rejoint la base : il est **secouru** - le vaisseau est
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
    state.send_message("RESCUED - THE STATION REBUILT YOUR SHIP");
}

/// Le cosmonaute EVA a atteint la zone d'accostage (vaisseau détruit) : la
/// station le **récupère** - un cordon va jaillir de l'anneau jusqu'à lui et
/// le ramener sur l'anneau (voir `advance_eva_recovery` et
/// `render::draw_eva_recovery_cable`). Le monde continue de tourner :
/// `docking` est appelée dans la frame, la suite est traitée en tête de
/// `update` (les frames suivantes font avancer `collisions` juste après
/// l'animation). Après la récupération, le fondu enchaîné fait apparaître le
/// vaisseau reconstruit (`advance_eva_crossfade`, terminé par
/// `rescue_cosmonaut`).
fn start_eva_recovery(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return; // cosmonaute EVA absent : rien à récupérer
    }
    let c = &shapes[idx];
    state.eva_recovery_from_pos = c.position;
    // point de l'anneau dans la direction du cosmonaute (le cordon le ramène
    // radialement sur le bord intérieur de l'anneau, comme les liens)
    let r = c.position.x.hypot(c.position.y);
    state.eva_recovery_to_pos = if r < 1.0 {
        Point::new(STATION_INNER_RADIUS, 0.0) // au centre : vers la droite
    } else {
        Point::new(
            c.position.x / r * STATION_INNER_RADIUS,
            c.position.y / r * STATION_INNER_RADIUS,
        )
    };
    state.eva_recovery = EVA_RECOVERY_DURATION;
    // le cosmonaute est immobilisé pendant que le cordon le tire, mais il
    // **garde son orientation** (il reste tourné comme il l'était en arrivant
    // - pas de repositionnement brutal à la récupération)
    let c = &mut shapes[idx];
    c.velocity = 0.0;
    c.direction = 0.0;
    c.rotation = 0.0;
    for j in c.first_triangle..=c.last_triangle {
        compute_real_positions(&mut triangles[j], c.position, c.center, c.orientation);
    }
    state.player.thrusted = 0; // flamme coupée : plus de poussée
    state.player.revert_thrusted = 0;
    state.player.rotate_left_thrusted = 0; // ni de jets latéraux
    state.player.rotate_right_thrusted = 0;
    state.send_message("STATION RECOVERY - HOLD ON");
}

/// Fait avancer la **récupération** du cosmonaute EVA d'une frame, en deux
/// phases (vitesse nulle - le monde continue de tourner, voir `update`) :
/// pendant la fraction
/// `EVA_CABLE_DEPLOY_FRACTION` de `EVA_RECOVERY_DURATION`, le cordon jaillit
/// de l'anneau vers le cosmonaute qui reste **sur place** ; une fois
/// complètement déployé (tendu), il le **ramène sur l'anneau** - position
/// interpolée (smoothstep) de `eva_recovery_from_pos` vers
/// `eva_recovery_to_pos` sur la phase restante. À la fin, le **fondu
/// enchaîné** démarre : le vaisseau est reconstruit au centre de la station
/// (`respawn_player`, liens attachés) et le cosmonaute s'efface pendant que
/// le vaisseau apparaît (`advance_eva_crossfade`).
fn advance_eva_recovery(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.eva_recovery = (state.eva_recovery - dt).max(0.0);
    // avancement global 0..1 sur toute la durée de la récupération
    let t = (1.0 - state.eva_recovery / EVA_RECOVERY_DURATION).clamp(0.0, 1.0);
    let idx = state.eva_cosmonaut as usize;
    let c = &mut shapes[idx];
    if t < EVA_CABLE_DEPLOY_FRACTION {
        // Phase 1 : le cordon se déploie de l'anneau vers le cosmonaute,
        // qui reste **sur place** tant qu'il n'est pas complètement tendu
        c.position = state.eva_recovery_from_pos;
    } else {
        // Phase 2 : cordon complètement déployé, il ramène le cosmonaute
        // sur l'anneau - interpolation lissée (smoothstep) sur la phase
        let u = ((t - EVA_CABLE_DEPLOY_FRACTION) / (1.0 - EVA_CABLE_DEPLOY_FRACTION)).clamp(0.0, 1.0);
        let e = u * u * (3.0 - 2.0 * u);
        c.position.x = state.eva_recovery_from_pos.x
            + (state.eva_recovery_to_pos.x - state.eva_recovery_from_pos.x) * e;
        c.position.y = state.eva_recovery_from_pos.y
            + (state.eva_recovery_to_pos.y - state.eva_recovery_from_pos.y) * e;
    }
    c.velocity = 0.0;
    c.rotation = 0.0;
    for j in c.first_triangle..=c.last_triangle {
        compute_real_positions(&mut triangles[j], c.position, c.center, c.orientation);
    }
    if state.eva_recovery <= 0.0 {
        state.eva_recovery = 0.0;
        // le cosmonaute est sur l'anneau : le vaisseau est reconstruit au
        // centre de la station (liens attachés) - le fondu enchaîné le fait
        // apparaître pendant que le cosmonaute s'efface
        respawn_player(state, shapes, triangles);
        state.eva_crossfade = EVA_CROSSFADE_DURATION;
    }
}

/// Fait avancer le **fondu enchaîné** de la récupération d'une frame : le
/// cosmonaute ramené sur l'anneau s'efface (alpha décroissant, rendu par
/// `main.rs`) pendant que le vaisseau reconstruit apparaît au centre de la
/// station, liens attachés (alpha croissant) - la caméra glisse de l'anneau
/// vers le centre (renvoyée à `update`). À la fin, le secours est terminé :
/// le cosmonaute retourne à son poste et le contrôle revient au vaisseau
/// (`rescue_cosmonaut`).
fn advance_eva_crossfade(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) -> Point {
    state.eva_crossfade = (state.eva_crossfade - dt).max(0.0);
    // caméra : glisse du cosmonaute (sur l'anneau) vers le centre de la
    // station où le vaisseau apparaît - interpolée (smoothstep) sur la durée
    let idx = state.eva_cosmonaut as usize;
    let pos = shapes[idx].position;
    let t = (1.0 - state.eva_crossfade / EVA_CROSSFADE_DURATION).clamp(0.0, 1.0);
    let e = t * t * (3.0 - 2.0 * t);
    let mut camera = Point::new(
        VIEWPORT_WIDTH / 2.0 - pos.x * (1.0 - e),
        VIEWPORT_HEIGHT / 2.0 - pos.y * (1.0 - e),
    );
    camera.normalize_world(&state.world);
    if state.eva_crossfade <= 0.0 {
        state.eva_crossfade = 0.0;
        rescue_cosmonaut(state, shapes, triangles);
    }
    camera
}

/// Détecte le retour à la base (ex « detect return to the base » de
/// `mainLoop`) : le vaisseau est docké quand son centre entre dans la zone
/// d'accostage - le cercle de rayon `STATION_DOCK_DISTANCE` autour du centre
/// de la station (vérification circulaire, comme la mire affichée à l'écran)
/// **et** qu'il est presque immobile (`STATION_DOCK_SPEED`) : il faut ralentir
/// pour terminer l'accostage (la mire passe du rouge au vert avec la qualité
/// de l'approche).
///
/// NB : comme l'original, le choix UNLOAD/CLOSE de la boîte était ignoré
/// (`r%` non utilisé) - le cargo reste vidé de toute façon à l'accostage
/// (au plus tard à la frame suivant la fermeture de la boîte ; le bouton
/// UNLOAD de la boîte le vide immédiatement). Le ravitaillement (carburant +
/// munitions), lui, n'est plus automatique : il s'achète indépendamment au
/// magasin (section RAVITAILLEMENT).
fn docking(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    elements: &mut [Element],
) {
    // vaisseau détruit : le cosmonaute EVA rejoint la base - dès qu'il atteint
    // la zone d'accostage (cercle de rayon `STATION_DOCK_DISTANCE` au centre,
    // la station est en (0,0)), la **récupération** démarre : un cordon
    // jaillit de l'anneau et le ramène sur l'anneau, puis le fondu enchaîné
    // fait apparaître le vaisseau reconstruit (le monde continue de tourner -
    // la suite est traitée en tête de `update` : `advance_eva_recovery` puis
    // `advance_eva_crossfade`, qui termine par `rescue_cosmonaut`)
    if state.cosmonaut_active {
        let c = &shapes[state.eva_cosmonaut as usize];
        if state.eva_recovery <= 0.0
            && crate::geom::wrapped_distance(c.position, shapes[STATION_INDEX].position, &state.world)
                < STATION_DOCK_DISTANCE
        {
            start_eva_recovery(state, shapes, triangles);
        }
        return;
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let delta = crate::geom::wrapped_delta(
        shapes[PLAYER_INDEX].position,
        shapes[STATION_INDEX].position,
        &state.world,
    );
    let in_zone = delta.x * delta.x + delta.y * delta.y < STATION_DOCK_DISTANCE * STATION_DOCK_DISTANCE;
    // l'accostage se termine seulement si le vaisseau est presque immobile
    if in_zone && shapes[PLAYER_INDEX].velocity.abs() < STATION_DOCK_SPEED {
        if state.player_at_station == 0 {
            state.player_at_station = -1;
            state.player_enter_station = -1;
            shapes[PLAYER_INDEX].velocity = 0.0;
            // animation d'accostage (3 s) avant la boîte DOCK STATION : le
            // vaisseau pivote vers la droite et se recentre au centre - la
            // boîte s'ouvre à la fin (`advance_dock_animation`)
            state.dock_anim = DOCK_ANIMATION_DURATION;
            state.dock_anim_from_pos = shapes[PLAYER_INDEX].position;
            state.dock_anim_from_orient = shapes[PLAYER_INDEX].orientation;
            // l'accostage démarre : le guide est coupé - il ne réapparaîtra
            // qu'à un prochain retour (et pas pendant qu'on quitte l'accostage)
            state.docking_guide = false;
            // compteur d'accostages (objectifs DAG)
            state.docking_count += 1;
        } else {
            // déchargement : la soute est convertie en crédits (scénario à
            // économie) puis vidée - le ravitaillement s'achète au magasin
            // (section RAVITAILLEMENT)
            let had_cargo = state.player.cargo_qty > 0;
            scenario::unload_cargo(state, elements);
            for e in elements.iter_mut() {
                e.count = 0;
            }
            state.player.cargo_qty = 0;
            state.player_enter_station = 0;
            state.player_at_station = -1;
            // la progression (crédits) n'est persistée que s'il y avait du
            // cargo (cette branche tourne à chaque frame à quai - pas
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
/// Le monde, lui, continue de tourner pendant l'animation (appelé par
/// `update`, qui fait avancer `collisions` juste après - le vaisseau qui
/// s'aligne est protégé) ; le trait d'accostage est dessiné par
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
        state.dock_box = true; // ouvre la boîte DOCK STATION (monde vivant)
    }
}

/// Le vaisseau quitte l'accostage (bouton CLOSE de la boîte DOCK STATION) :
/// ferme la boîte puis libère le vaisseau (rétraction des liens).
fn undock(state: &mut GameState) {
    state.dock_box = false;
    release_links(state);
}

/// Libère le vaisseau : détache les liens (s'ils étaient attachés à quai,
/// lancement/respawn) et démarre la **rétraction des liens** - le vaisseau
/// reste au centre de la station, les 4 traits néon se rétractent vers le
/// bord intérieur de l'anneau pendant `DOCK_RETRACT_DURATION` (le monde
/// continue de tourner, voir `advance_dock_retract`), puis il est libre. En
/// quittant la base, le **guide d'accostage est coupé** : la mire ne
/// réapparaîtra qu'au retour (franchissement de la limite extérieure en
/// entrant).
fn release_links(state: &mut GameState) {
    state.dock_links = false;
    state.docking_guide = false;
    state.dock_retract = DOCK_RETRACT_DURATION;
}

/// Met à jour le **guide d'accostage** (la mire au centre de la station) :
/// il ne s'affiche **que lors du retour à la base** - le vaisseau doit avoir
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
    // vaisseau détruit : le cosmonaute éjecté doit TOUJOURS voir la mire -
    // elle le guide vers la base (le « retour » classique ne s'applique pas)
    if state.cosmonaut_active {
        state.docking_guide = true;
        state.dock_was_outside = true;
        return;
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let dist = crate::geom::wrapped_distance(player_position, station_position, &state.world);
    let outside = dist >= station_radius;
    if outside {
        state.docking_guide = false;
    } else if state.dock_was_outside {
        // vient de franchir la limite extérieure de la base en entrant :
        // c'est le retour - le guide s'affiche
        state.docking_guide = true;
    }
    state.dock_was_outside = outside;
}

/// Le joueur donne-t-il une commande de déplacement (flèches ↑/↓/←/→, tous
/// les modes de déplacement) ? Utilisé pour déclencher la rétraction des
/// liens quand le vaisseau démarre de la base (voir `update`).
fn player_moving_input() -> bool {
    up_pressed() || down_pressed() || left_pressed() || right_pressed()
}

/// Commandes de déplacement : touche clavier, joystick tactile (`touch.rs`,
/// bas-gauche) OU télécommande (`remote.rs`, téléphone sur le réseau local) -
/// les trois pilotent comme les flèches.
fn up_pressed() -> bool {
    is_key_down(KeyCode::Up) || crate::touch::up() || crate::remote::up()
}

fn down_pressed() -> bool {
    is_key_down(KeyCode::Down) || crate::touch::down() || crate::remote::down()
}

fn left_pressed() -> bool {
    is_key_down(KeyCode::Left) || crate::touch::left() || crate::remote::left()
}

fn right_pressed() -> bool {
    is_key_down(KeyCode::Right) || crate::touch::right() || crate::remote::right()
}

/// Tir : Shift (clavier), bouton de tir tactile (`touch.rs`, bas-droite) OU
/// télécommande (`remote.rs`).
fn fire_pressed() -> bool {
    is_key_down(KeyCode::LeftShift)
        || is_key_down(KeyCode::RightShift)
        || crate::touch::fire()
        || crate::remote::fire()
}

/// Fait avancer la rétraction des liens d'accostage d'une frame : le vaisseau
/// reste immobilisé exactement au centre de la station (position 0,0,
/// orientation 0) pendant `DOCK_RETRACT_DURATION` - les liens se rétractent
/// visuellement (voir `render::draw_docking_line`). À la fin, le vaisseau est
/// libre (le monde se dégèle, `docking` peut le faire repartir).
///
/// Le monde, lui, continue de tourner pendant la rétraction (appelé par
/// `update`, qui fait avancer `collisions` juste après - le vaisseau tenu au
/// centre est protégé).
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
    /// Décharge la soute (crédits disponibles pour le ravitaillement).
    Unload,
    /// Ouvre le magasin de la station (carburant, munitions, armes,
    /// extensions et modes de déplacement en scénario à économie).
    Shop,
    /// Ferme la boîte.
    Close,
}

/// Détecte un clic sur la boîte de choix DOCK STATION (ex
/// `windowUtils_choiceBox`) et renvoie le bouton cliqué (contrairement à
/// l'original, le choix n'est plus ignoré : UNLOAD et SHOP agissent). SHOP
/// ouvre le magasin de la station (le carburant et les munitions s'y
/// achètent indépendamment).
fn choice_box_click() -> ChoiceClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return ChoiceClick::None;
    }
    let l = choice_box_layout();
    let m = mouse_to_game();
    if l.unload.contains(m) {
        ChoiceClick::Unload
    } else if l.shop.contains(m) {
        ChoiceClick::Shop
    } else if l.close.contains(m) {
        ChoiceClick::Close
    } else {
        ChoiceClick::None
    }
}

/// Bouton cliqué sur le magasin de la station (bouton SHOP de la boîte DOCK
/// STATION).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShopClick {
    None,
    /// Sélectionne / débloque un mode de déplacement (index `MOVING_MODE_*`).
    Mode(i32),
    /// Achète une arme du catalogue (index dans `VAISSEAU_WEAPONS`).
    Weapon(usize),
    /// Achète le **radar de bord** (minimap globale - onglet ÉQUIPEMENT).
    BuyRadar,
    /// Achète la quantité du curseur de carburant (ligne FUEL du
    /// ravitaillement).
    Refuel,
    /// Achète la quantité du curseur de munitions de l'arme `i` (ligne AMMO
    /// de l'arme - une par arme possédée).
    Rearm(usize),
    /// Remplit le carburant et les munitions de toutes les armes possédées
    /// au maximum achetable (bouton TOUT REMPLIR).
    RefillAll,
    /// Achète l'extension de réservoir de carburant (atelier).
    BuyFuelUpgrade,
    /// Achète l'extension de chargeur de munitions (atelier).
    BuyAmmoUpgrade,
    /// Achète l'extension de soute (atelier).
    BuyCargoUpgrade,
    /// Revient à la boîte DOCK STATION (toujours accosté).
    Close,
}

/// Détecte un clic sur le magasin de la station : un **bouton « pilule »**
/// de l'onglet actif (achat d'arme, sélection/déblocage de mode, extension,
/// ravitaillement à la quantité du curseur, TOUT REMPLIR) ou le bouton
/// CLOSE. Les onglets et les pistes des curseurs ne sont PAS traités ici :
/// ils le sont par `shop_update` (état mutable : bascule d'onglet, début de
/// glisser).
fn shop_box_click(state: &GameState) -> ShopClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return ShopClick::None;
    }
    let l = shop_box_layout(state);
    let m = mouse_to_game();
    // onglets : la bascule d'onglet est traitée par `shop_update`
    if l.tabs.iter().any(|t| t.contains(m)) {
        return ShopClick::None;
    }
    // pistes des curseurs : le clic glisse la quantité (`shop_update`)
    if (l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m))
        || l.slider_ammo.iter().any(|t| t.w > 0.0 && t.contains(m))
    {
        return ShopClick::None;
    }
    // boutons d'action de l'onglet actif (un seul onglet affiché à la fois :
    // les rectangles des autres onglets sont vides)
    match state.shop_tab {
        crate::config::SHOP_TAB_WEAPONS => {
            for (i, r) in l.buy_weapon.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Weapon(i);
                }
            }
            if l.buy_radar.w > 0.0 && l.buy_radar.contains(m) {
                return ShopClick::BuyRadar;
            }
        }
        crate::config::SHOP_TAB_WORKSHOP => {
            if l.buy_fuel_upgrade.contains(m) {
                return ShopClick::BuyFuelUpgrade;
            }
            if l.buy_ammo_upgrade.contains(m) {
                return ShopClick::BuyAmmoUpgrade;
            }
            if l.buy_cargo_upgrade.contains(m) {
                return ShopClick::BuyCargoUpgrade;
            }
        }
        crate::config::SHOP_TAB_MODES => {
            for (i, r) in l.buy_mode.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Mode(MOVING_MODE_ORDER[i]);
                }
            }
        }
        _ => {
            if l.buy_fuel.w > 0.0 && l.buy_fuel.contains(m) {
                return ShopClick::Refuel;
            }
            for (i, r) in l.buy_ammo.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Rearm(i);
                }
            }
            if l.refill_all.w > 0.0 && l.refill_all.contains(m) {
                return ShopClick::RefillAll;
            }
        }
    }
    if l.close.contains(m) {
        ShopClick::Close
    } else {
        ShopClick::None
    }
}

/// Met à jour le magasin de la station à chaque frame : bascule d'onglet
/// (un clic sur un onglet change l'onglet actif et efface le retour
/// d'action), curseurs du ravitaillement (pression sur une piste = début de
/// glisser - la quantité saute au pointeur ; glisser bouton maintenu ;
/// molette = ± un paquet de la ressource) et bornage des quantités au
/// manque des réservoirs et aux crédits disponibles
/// (`scenario::clamp_shop_quantities`). Appelé avant `shop_box_click`.
fn shop_update(state: &mut GameState) {
    let l = shop_box_layout(state);
    let m = mouse_to_game();
    // bascule d'onglet : une pression sur un onglet change l'onglet actif
    // (et efface le retour d'action de l'onglet précédent)
    if is_mouse_button_pressed(MouseButton::Left) {
        for (i, tab) in l.tabs.iter().enumerate() {
            if tab.contains(m) && state.shop_tab as usize != i {
                state.shop_tab = i as u8;
                state.shop_feedback.clear();
                break;
            }
        }
    }
    // début de glisser : une pression sur une piste saisit le curseur
    if is_mouse_button_pressed(MouseButton::Left) {
        if l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m) {
            state.shop_drag = Some(0);
        } else {
            for (i, track) in l.slider_ammo.iter().enumerate() {
                if track.w > 0.0 && track.contains(m) && scenario::weapon_owned(state, i) {
                    state.shop_drag = Some(1 + i);
                    break;
                }
            }
        }
    }
    // glisser : la valeur suit le pointeur tant que le bouton est maintenu
    if let Some(target) = state.shop_drag {
        if is_mouse_button_down(MouseButton::Left) {
            if target == 0 {
                let track = l.slider_fuel;
                if track.w > 0.0 {
                    let missing = (scenario::fuel_capacity(state) - state.resources.fuel).max(0.0);
                    let frac = ((m.x - track.x) / track.w).clamp(0.0, 1.0) as f64;
                    state.shop_fuel_qty = frac * missing;
                }
            } else if let Some(&track) = l.slider_ammo.get(target - 1) {
                if track.w > 0.0 {
                    let missing =
                        (scenario::ammo_capacity(state) - state.resources.weapon_ammo[target - 1])
                            .max(0) as f64;
                    let frac = ((m.x - track.x) / track.w).clamp(0.0, 1.0) as f64;
                    state.shop_ammo_qty[target - 1] = frac * missing;
                }
            }
        } else {
            state.shop_drag = None; // bouton relâché
        }
    }
    // molette sur une piste : ± un paquet de la ressource (10 carburant,
    // le paquet de l'arme pour les munitions)
    let wheel = mouse_wheel().1;
    if wheel != 0.0 {
        if l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m) {
            let step = crate::scenario::scenario(state.scenario).fuel_step;
            state.shop_fuel_qty += wheel as f64 * step;
        } else {
            for (i, track) in l.slider_ammo.iter().enumerate() {
                if track.w > 0.0 && track.contains(m) && scenario::weapon_owned(state, i) {
                    let step = scenario::weapon_spec(i).ammo_pack as f64;
                    state.shop_ammo_qty[i] += wheel as f64 * step;
                    break;
                }
            }
        }
    }
    scenario::clamp_shop_quantities(state);
}

/// Achète une arme du catalogue au magasin (bouton MARCHÉ de la boîte DOCK
/// STATION) puis persiste la progression (crédits, armes possédées). Le
/// mesh de l'arme achetée apparaît sur le vaisseau : reconstruction avec la
/// nouvelle composition (`vaisseau::rebuild_player_vaisseau`). Le résultat
/// (achat / refus) s'affiche dans le pied de la fenêtre (`shop_feedback`).
fn buy_weapon_and_save(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    i: usize,
) {
    match scenario::buy_weapon(state, i) {
        scenario::WeaponOutcome::Purchased(cost) => {
            crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
            state.shop_feedback = format!("Arme achetée (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::WeaponOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::WeaponOutcome::Owned => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Achète le **radar de bord** au magasin (bouton MARCHÉ de la boîte DOCK
/// STATION, onglet ÉQUIPEMENT) puis persiste la progression (crédits, radar
/// possédé) : la minimap globale (positions des météores) s'affiche dès
/// l'achat (`scenario::has_radar`). Le résultat (achat / refus) s'affiche
/// dans le pied de la fenêtre (`shop_feedback`).
fn buy_radar_and_save(state: &mut GameState) {
    match scenario::buy_radar(state) {
        scenario::RadarOutcome::Purchased(cost) => {
            state.shop_feedback = format!("Radar installé (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::RadarOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::RadarOutcome::Owned => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Achète une extension du magasin (réservoir, chargeur ou soute) puis persiste
/// la progression (crédits, niveaux d'extension) - les réservoirs montent à
/// la nouvelle capacité et la soute s'agrandit dans `buy_upgrade`. Un plan du
/// vaisseau lié à la ligne achetée peut apparaître : le mesh est reconstruit
/// avec la nouvelle composition (`vaisseau::rebuild_player_vaisseau`). Le
/// résultat (achat / refus) s'affiche dans le pied de la fenêtre.
fn buy_upgrade_and_save(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    track: scenario::UpgradeTrackId,
) {
    match scenario::buy_upgrade(state, track) {
        scenario::UpgradeOutcome::Purchased(cost) => {
            crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
            state.shop_feedback = format!("Extension achetée (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::UpgradeOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::UpgradeOutcome::Maxed => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Sélectionne un mode de déplacement dans le magasin (bouton MARCHÉ de la
/// boîte DOCK STATION) : la sélection passe par le scénario (un mode
/// verrouillé est payé en crédits, refusé si insuffisant - messages HUD) ;
/// le mode devenu courant est annoncé au HUD, et le mode + la progression
/// (crédits, modes débloqués) sont persistés immédiatement. Le résultat
/// s'affiche dans le pied de la fenêtre (`shop_feedback`).
fn select_mode_and_save(state: &mut GameState, mode: i32) {
    if scenario::try_select_mode(state, mode) {
        state.send_message(&format!("MOVING MODE: {}", crate::marketplace::mode_label(mode)));
        let _ = persist::save_moving_mode(state.moving_mode);
        let _ = scenario::save_progression(state);
        state.shop_feedback = format!("Mode de vol : {}", crate::marketplace::mode_label(mode));
        state.shop_feedback_ok = true;
    } else {
        state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
        state.shop_feedback_ok = false;
    }
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

/// Clic sur l'écran de paramétrage (touche O) : les cases MUSIC / AUTO
/// GENERATE / ANTIALIAS basculent, un clic sur la barre du volume donne la
/// fraction demandée (0..1), les lignes RENDER / WINDOW / SIZE font cycler
/// leur valeur, RESET remet les réglages par défaut et CLOSE ferme l'écran.
enum SettingsClick {
    None,
    Music,
    AutoGenerate,
    Volume(f32),
    RenderStyle,
    WindowMode,
    WindowSize,
    Antialias,
    /// Affiche/coupe l'interface tactile (joystick + bouton de tir, `touch.rs`).
    TouchUi,
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
fn settings_box_click(state: &GameState) -> SettingsClick {
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
fn push_pin_digit(state: &mut GameState, digit: char) {
    if state.settings_pin_buffer.len() < 4 {
        state.settings_pin_buffer.push(digit);
    }
}

/// Valide la saisie du PIN de la télécommande : le code (vide = aucune
/// protection) est appliqué à l'état et persisté.
fn confirm_remote_pin(state: &mut GameState) {
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
fn close_settings(state: &mut GameState) {
    state.settings_box = false;
}

/// Ferme l'écran de paramétrage (voir `close_settings`).
fn close_and_persist(state: &mut GameState) {
    close_settings(state);
}

/// Remet les réglages par défaut (bouton RESET) : musique en marche,
/// génération automatique active, volume 100 %, rendu texturé, fenêtré à
/// 960×540, anticrénelage éteint - les valeurs par défaut ne sont
/// réenregistrées à la fermeture que si elles ont été modifiées pendant
/// l'écran. NB : le mode de déplacement n'est plus un réglage (il se choisit
/// au magasin de la station) - le RESET ne le touche pas.
fn reset_settings_fields(state: &mut GameState) {
    state.auto_generate = true;
    state.render_style = RenderStyle::Textured;
    state.window_size = 0;
    state.antialias = false;
    state.touch_ui = true; // interface tactile affichée par défaut
}

/// Remet les réglages par défaut (bouton RESET) : champs par défaut
/// (`reset_settings_fields`), retour fenêtré à 960×540, musique en marche,
/// volume 100 %, et clés de réglage du fichier de config supprimées - les
/// valeurs par défaut ne sont réenregistrées à la fermeture que si elles ont
/// été modifiées pendant l'écran. NB : la progression d'un scénario à
/// économie (scénario choisi, minerais, modes payés, réputation - clés
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
    // seules les clés de réglage sont supprimées - le scénario et sa
    // progression (`scenario`, `prog_*` : minerais, modes payés, réputation,
    // mode de déplacement choisi) survivent au RESET
    for key in [
        "music",
        "auto_generate",
        "volume",
        "render_style",
        "window_size",
        "antialias",
        "touch_ui",
    ] {
        let _ = persist::delete_key(key);
    }
    crate::touch::set_enabled(state.touch_ui);
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
/// REMOVE via libX11) sinon - voir `cycle_view_mode`.
fn apply_view_mode(state: &mut GameState, target: ViewMode) {
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
        (_, ViewMode::Windowed) => {
            if !crate::x11::set_fullscreen(false) {
                let (w, h) = window_size_dims(state.window_size);
                request_new_screen_size(w, h);
            }
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

/// Contrôles du vaisseau selon `state.moving_mode` (port fidèle des blocs
/// `select case` de `mainLoop`) + tir (Shift gauche/droit, ex
/// `case 42, 54` des modes). REALISTIC reprend INERTIAL mais laisse la
/// vitesse angulaire du vaisseau vivre après le relâchement des propulseurs
/// latéraux.
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
    // objectif : rejoindre la base) - pas de tir ni de carburant
    if state.cosmonaut_active {
        cosmonaut_controls(state, shapes, dt);
        return;
    }
    state.player.thrust = 0.0;

    // carburant (scénarios à économie) : les poussées avant/arrière sont
    // bloquées quand le réservoir est vide - les rotations restent libres
    let fuel_ok = scenario::fuel_available(state);

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
                player.velocity += PLAYER_ACCELERATION * 60.0 * dt;
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
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, 1.0, -1.0);
            }
            if right_pressed() {
                player.orientation += PLAYER_ROTATION_SPEED * 60.0 * dt;
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, -1.0, 1.0);
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
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, 1.0, -1.0);
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
                thrust_vector(player, PLAYER_ACCELERATION * 60.0 * dt, player.orientation, -1.0, 1.0);
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
                let dy = player.direction.sin() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
            }
            if fuel_ok && right_pressed() {
                let dx = player.direction.cos() * player.velocity + PLAYER_ACCELERATION * 60.0 * dt;
                let dy = player.direction.sin() * player.velocity;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                state.player.rotate_right_thrusted = -5; // jet latéral droit
            }
            if fuel_ok && down_pressed() {
                let dx = player.direction.cos() * player.velocity;
                let dy = player.direction.sin() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt;
                player.direction = dy.atan2(dx);
                player.velocity = dx.hypot(dy);
                player.orientation = -player.direction;
                if player.velocity > 0.0 {
                    state.player.revert_thrusted = -5;
                }
            }
            if fuel_ok && left_pressed() {
                let dx = player.direction.cos() * player.velocity - PLAYER_ACCELERATION * 60.0 * dt;
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
fn cosmonaut_controls(state: &mut GameState, shapes: &mut [Shape], dt: f64) {
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

/// Met à jour la vitesse angulaire du mode REALISTIC : une commande latérale
/// l'accélère progressivement jusqu'à `±PLAYER_ROTATION_SPEED`, le relâchement
/// la conserve, et la commande opposée la freine jusqu'à l'arrêt.
fn realistic_rotation_after_input(
    current: f64,
    right: bool,
    left: bool,
    dt: f64,
) -> f64 {
    let direction = match (right, left) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    (current + direction * PLAYER_ROTATION_ACCELERATION * dt)
        .clamp(-PLAYER_ROTATION_SPEED, PLAYER_ROTATION_SPEED)
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
    fn realistic_rotation_is_preserved_when_released() {
        let dt = 1.0 / 60.0;
        let mut speed = 0.0;
        for _ in 0..30 {
            speed = realistic_rotation_after_input(speed, true, false, dt);
        }
        assert!((speed - PLAYER_ROTATION_SPEED).abs() < 1e-12);
        assert_eq!(realistic_rotation_after_input(speed, false, false, dt), speed);
        for _ in 0..30 {
            speed = realistic_rotation_after_input(speed, false, true, dt);
        }
        assert!(speed.abs() < 1e-12);
        assert_eq!(realistic_rotation_after_input(speed, true, true, dt), speed);
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
        // libérés en minerais à leur position (un minerai par unité de minerai)
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
        let minerals = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).count();
        assert_eq!(minerals, 2);
    }

    #[test]
    fn meteor_absorbs_mineral_increasing_its_minerals() {
        // un météore percute un minerai : il l.absorbe - le minerai disparaît
        // et la quantité de minerai du météore augmente (sans endommager le
        // météore)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_MINERAL, 0, 0, 0.0, 0.0),
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

        // le minerai a été absorbé (détruit), le météore a gagné un minerai
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
        // sans élément minéral : pas de minerai créé, test ciblé sur la collision
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
    fn meteor_bounces_off_station_ring() {
        // un météore qui percute la base subit une **force de réaction** : le
        // triangle qui collisionne explose (débris) et la composante de sa
        // vitesse vers la station est réfléchie avec la restitution réglée
        // dans l'outil - le météore rebondit le long de la normale du point
        // d'impact, la station reste intacte
        let mut state = GameState::new();
        // station (index 0) : triangle (0,0)-(10,0)-(0,10) à l'origine
        let mut shapes = vec![
            test_shape(WHOIAM_STATION, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 2, 2.0, 2.0),
        ];
        shapes[0].velocity = 0.0;
        // météore : 2 triangles - le premier (à (2,2), 10×10) chevauche la
        // station, le second (à (50,50)) est hors de portée
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 2.0, 2.0),
            test_triangle(2, 1, 50.0, 50.0),
        ];
        // le météore fonce vers le centre de la base (direction π = -x)
        shapes[1].velocity = 1.0;
        shapes[1].direction = TAU / 2.0;
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(
            &mut state,
            &mut shapes,
            &mut triangles,
            &mut garbages,
            &mut elements,
            &mut rng,
            None,
            0.0,
        );

        // le triangle qui chevauchait la station a explosé : le météore
        // survit (life 2 → 1), la station est intacte
        assert_eq!(shapes[1].life, 1);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(shapes[0].life, 1);
        assert_eq!(triangles[0].life, 1);
        // l'explosion a généré ses débris
        assert!(
            garbages.iter().any(|g| g.life > 0),
            "aucun débris vivant : {}",
            garbages.len()
        );
        // force de réaction : seule la TRAJECTOIRE est réfléchie - la
        // direction du rebond (selon la restitution réglée dans l'outil) est
        // recalculée le long de la normale du point d'impact (diagonale
        // (1,1)), mais la VITESSE du météore est conservée (le choc avec la
        // base repousse le météore sans le ralentir). On recalcule la
        // direction attendue depuis la constante (`METEOR_STATION_RESTITUTION`
        // est un paramètre de mise au point : le test ne doit pas dépendre de
        // sa valeur exacte). Vitesse initiale 1.0 vers -x : vx = -1, vy = 0 ;
        // normale du choc (1/√2, 1/√2).
        let e = crate::marketplace::METEOR_STATION_RESTITUTION;
        let inv_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
        let (vx, vy) = (-1.0, 0.0);
        let vn = vx * inv_sqrt2 + vy * inv_sqrt2; // radiale (négative = vers la base)
        let rx = vx - (1.0 + e) * vn * inv_sqrt2;
        let ry = vy - (1.0 + e) * vn * inv_sqrt2;
        let mut expected_dir = (-ry).atan2(rx);
        if expected_dir < 0.0 {
            expected_dir += TAU;
        }
        let m = &shapes[1];
        assert!(
            (m.velocity - 1.0).abs() < 1e-9,
            "la vitesse doit être conservée : {} (attendu 1.0)",
            m.velocity
        );
        assert!(
            (m.direction - expected_dir).abs() < 1e-9,
            "direction après réaction : {} (attendu {})",
            m.direction,
            expected_dir
        );
        // la composante radiale est bien inversée : le météore s'éloigne du
        // centre de la base (vitesse radiale positive après la réaction)
        assert!(
            m.velocity * m.direction.cos() * inv_sqrt2 - m.velocity * m.direction.sin() * inv_sqrt2 > 0.0,
            "le météore doit s'éloigner de la base"
        );
    }

    #[test]
    fn meteor_recoils_away_from_station_over_frames() {
        // la force de réaction ne se limite pas à une frame : après le
        // rebond, le météore survivant **s'éloigne** de la base frame après
        // frame (au lieu de continuer à labourer l'anneau, triangle par
        // triangle, jusqu'à sa destruction) - la station reste intacte
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_STATION, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 2, 2.0, 2.0),
        ];
        shapes[0].velocity = 0.0;
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 2.0, 2.0),
            test_triangle(2, 1, 50.0, 50.0),
        ];
        // le météore fonce vers le centre de la base (direction π = -x)
        shapes[1].velocity = 1.0;
        shapes[1].direction = TAU / 2.0;
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        // frame 1 : impact - le triangle qui chevauchait la base explose et
        // le météore rebondit (composante radiale réfléchie), il survit
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
        assert_eq!(shapes[1].life, 1);
        let pos = shapes[1].position;
        let dist0 = pos.x.hypot(pos.y);

        // frame 2 : plus aucun triangle en collision - le météore continue de
        // s'éloigner (distance au centre croissante), ne perd plus de vie, la
        // station est intacte
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
        assert_eq!(shapes[1].life, 1, "le météore ne doit plus s'effriter");
        let pos2 = shapes[1].position;
        let dist1 = pos2.x.hypot(pos2.y);
        assert!(
            dist1 > dist0,
            "le météore doit s'éloigner : distance {} → {}",
            dist0,
            dist1
        );
        assert_eq!(shapes[0].life, 1);
        assert_eq!(triangles[0].life, 1);
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
    fn bullet_destroying_mineral_triangle_creates_mineral() {
        // une balle détruit un triangle avec élément : un minerai apparaît
        // (ex mainLoop : `if element > 0 and collidBy = BULLET → createMineral`).
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

        // un minerai a été créé (forme supplémentaire WHOIAM_MINERAL)
        let mineral = shapes.iter().find(|s| s.who_i_am == WHOIAM_MINERAL);
        assert!(mineral.is_some(), "un minerai doit apparaître");
        assert_eq!(mineral.unwrap().element, 1);
        assert_eq!(triangles[1].life, 0);
        assert_eq!(shapes[1].life, 0);
    }

    #[test]
    fn bullet_fully_destroyed_when_hitting_meteor_triangle() {
        // toute munition qui touche un triangle de météore est détruite en
        // même temps que ce triangle, même si le météore survit : un tir
        // consomme au plus un triangle - la munition (mesh multi-triangles,
        // ex missile) ne traverse pas le météore pour en grignoter d'autres.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_BULLET, 0, 1, 0.0, 0.0), // munition 2 triangles
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0), // météore 2 triangles
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),   // munition : touche le météore
            test_triangle(1, 0, 50.0, 50.0), // munition : loin (ne touche rien)
            test_triangle(2, 1, 0.0, 0.0),   // météore : touché par la munition
            test_triangle(3, 1, 60.0, 60.0), // météore : intact
        ];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // la munition entière est détruite (y compris le triangle qui n'a pas
        // touché) en même temps que le triangle du météore touché
        assert_eq!(shapes[0].life, 0, "la munition doit être détruite");
        assert_eq!(triangles[0].life, 0);
        assert_eq!(triangles[1].life, 0, "tous les triangles de la munition meurent");
        // le météore survit mais perd exactement le triangle touché
        assert_eq!(shapes[1].life, 1, "le météore survit");
        assert_eq!(triangles[2].life, 0, "triangle du météore touché détruit");
        assert_eq!(triangles[3].life, 1, "triangle du météore intact");
        // le météore n'est pas compté comme détruit par le joueur
        assert_eq!(state.meteors_destroyed, 0);
    }

    #[test]
    fn docked_ship_is_protected_while_meteors_keep_drifting() {
        // à quai (boîte DOCK STATION ouverte) : le monde continue de vivre -
        // les météores dérivent autour de la base, mais le vaisseau accosté
        // est **protégé** : aucun impact ne l'endommage ni ne le pousse. Une
        // fois la boîte fermée, le vaisseau libre est de nouveau vulnérable.
        let mut state = GameState::new();
        state.dock_box = true; // vaisseau à quai, boîte DOCK STATION ouverte
        // vaisseau (index 0) au centre, météore (index 1) dont un triangle
        // chevauche le premier triangle du vaisseau (même géométrie que les
        // tests voisins)
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 3, 2.0, 2.0),
        ];
        // le météore dérive vers +x (comme dans le monde vivant à quai)
        shapes[1].velocity = 1.0;
        shapes[1].direction = 0.0;
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),   // vaisseau : chevauché par le météore
            test_triangle(1, 0, 50.0, 50.0), // vaisseau : loin (ne touche rien)
            test_triangle(2, 1, 2.0, 2.0),   // météore : chevauche le vaisseau
            test_triangle(3, 1, 60.0, 60.0), // météore : intact
        ];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 1.0 / 60.0);

        // le météore a dérivé : le monde ne s'est pas arrêté à quai
        assert!(
            shapes[1].position.x > 2.0,
            "le météore doit continuer de se déplacer à quai : {}",
            shapes[1].position.x
        );
        // le vaisseau accosté est intact : aucun impact, aucun dégât
        assert_eq!(shapes[0].life, 2, "le vaisseau à quai ne doit pas être endommagé");
        assert_eq!(triangles[0].life, 1);
        assert_eq!(shapes[1].life, 2, "le météore non plus (aucune collision détectée)");
        assert!(garbages.is_empty(), "aucune explosion à quai");

        // boîte fermée (vaisseau libre) : le même météore, toujours en
        // chevauchement après sa dérive, l'endommage maintenant - un impact
        // suffit à détruire le vaisseau (1 impact = détruit, comme l'original)
        state.dock_box = false;
        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 1.0 / 60.0);
        assert_eq!(shapes[0].life, 0, "vaisseau libre : détruit par l'impact");
        assert_eq!(triangles[0].life, 0);
        assert_eq!(triangles[1].life, 0, "tous les triangles du vaisseau meurent");
        assert_eq!(shapes[1].life, 1, "le météore survit mais perd son triangle");
        assert_eq!(triangles[2].life, 0);
        assert_eq!(triangles[3].life, 1, "triangle du météore intact");
    }

    #[test]
    fn missile_destroying_meteor_releases_absorbed_minerals() {
        // un missile détruit un météore qui contient des minerais absorbés
        // (minerais mangés, sans triangle minéralisé restant) : les minerais
        // sont libérés en minerais - pas détruits avec le météore
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
        let minerals = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).count();
        assert_eq!(minerals, 3, "les 3 minerais absorbés doivent être libérés");
    }

    #[test]
    fn missile_hitting_mineral_directly_destroys_it() {
        // un missile qui touche directement un minerai le DÉTRUIT : c.est le
        // seul cas de destruction de minerai - aucun nouveau minerai n.est
        // créée (pas de « libération »)
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_BULLET, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_MINERAL, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[1].element = 1; // GOLD
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le minerai est détruit et aucun nouveau minerai n.est apparu
        assert_eq!(shapes[1].life, 0);
        assert_eq!(triangles[1].life, 0);
        let minerals = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).count();
        assert_eq!(minerals, 1, "le minerai détruit ne doit pas être dupliqué");
    }

    #[test]
    fn player_collects_mineral_into_cargo() {
        // le vaisseau ramasse un minerai : élément compté, soute remplie
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_MINERAL, 1, 1, 2.0, 2.0),
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
        // par le bouclier - le vaisseau et son triangle restent intacts
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
        // bouclier déjà vide : l'impact détruit le vaisseau - une vie perdue,
        // respawn à la station (position réinitialisée, bouclier rechargé)
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Survival;
        crate::scenario::apply_start(&mut state);
        state.resources.shield = 0.0;
        // vaisseau mesh réel (plage allouée - le respawn le reconstruit)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        shapes[0].position = Point::new(300.0, 200.0);
        push_test_shape(&mut shapes, &mut triangles, WHOIAM_METEOR, 302.0, 202.0); // sur le vaisseau
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
        // faces visibles aux niveaux courants (les plans liés aux upgrades
        // n'apparaissent qu'à partir de leur niveau)
        assert_eq!(shapes[0].life, crate::vaisseau::vaisseau_visible_face_count(&state) as i32);
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
        // vaisseau mesh réel (plage allouée - le respawn le reconstruit)
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        shapes[0].position = Point::new(300.0, 200.0);
        push_test_shape(&mut shapes, &mut triangles, WHOIAM_STATION, 0.0, 0.0);
        shapes[0].velocity = 4.0;
        shapes[0].direction = 1.5;
        state.player_at_station = 0;
        state.player.thrusted = 3;

        respawn_player(&mut state, &mut shapes, &mut triangles);

        assert_eq!(shapes[0].position, Point::new(0.0, 0.0));
        assert_eq!(shapes[0].velocity, 0.0);
        assert_eq!(shapes[0].direction, 0.0);
        // faces visibles aux niveaux courants (les plans liés aux upgrades
        // n'apparaissent qu'à partir de leur niveau)
        assert_eq!(shapes[0].life, crate::vaisseau::vaisseau_visible_face_count(&state) as i32);
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
        // liens** démarre (le vaisseau reste au centre, monde vivant) ; à la
        // fin de `DOCK_RETRACT_DURATION`, le vaisseau est libre
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
        // orientation 0, immobilisé - les liens se rétractent encore
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
        // au lancement, le vaisseau est à quai (liens attachés, mire cachée -
        // voir `state.dock_links`) ; dès qu'il démarre (commande de mouvement
        // ou CLOSE après un accostage), `release_links` détache les liens et
        // lance la rétraction (monde vivant pendant `DOCK_RETRACT_DURATION`)
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
        // vol - elle s'active quand le vaisseau franchit la limite extérieure
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
        // départ - il ne se réactive qu'à un nouveau franchissement en entrant)
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
        // circonscrit (12,12 - distance ≈ 17 > 15) n'accoste pas, une
        // diagonale à distance < rayon (10,10 - ≈ 14,1 < 15) accoste
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
        // ravitaillement - il se paie au magasin (section RAVITAILLEMENT) :
        // réservoirs et crédits intacts, pas de message d.achat
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
        state.resources.weapon_ammo[0] = 5;

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        assert_eq!(elements[1].count, 0);
        assert_eq!(state.player.cargo_qty, 0);
        assert_eq!(state.resources.credits, 20);
        assert_eq!(state.resources.fuel, 10.0);
        assert_eq!(state.resources.weapon_ammo[0], 5);
        assert!(state.message_queue.contains("CARGO UNLOADED: +20 CREDITS"));
        assert!(!state.message_queue.contains("SUPPLIES PURCHASED"));
    }

    #[test]
    fn shop_fuel_and_ammo_are_purchased_independently() {
        // les lignes FUEL / AMMO du magasin (plus de bouton REFUEL/REARM
        // dans la boîte DOCK STATION) remplissent chaque réservoir contre
        // minerais, indépendamment : 9 pour le carburant (90/10), 5 pour les
        // munitions (25/5) - le paiement est testé via le scénario, ici le
        // couplage avec l'état de jeu
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        state.resources.credits = 100;
        state.resources.fuel = 10.0;
        state.resources.weapon_ammo[0] = 5;

        // carburant seul : les munitions restent intactes
        assert_eq!(
            crate::scenario::purchase_fuel(&mut state),
            crate::scenario::SupplyOutcome::Purchased(9)
        );
        assert_eq!(state.resources.credits, 91);
        assert_eq!(state.resources.fuel, crate::scenario::fuel_capacity(&state)); // 100
        assert_eq!(state.resources.weapon_ammo[0], 5);

        // munitions seules : le carburant reste plein
        assert_eq!(
            crate::scenario::purchase_ammo(&mut state),
            crate::scenario::SupplyOutcome::Purchased(5)
        );
        assert_eq!(state.resources.credits, 86);
        assert_eq!(
            crate::scenario::total_ammo(&state),
            crate::scenario::ammo_capacity(&state)
        ); // 30
        assert_eq!(state.resources.fuel, 100.0);
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
    fn closing_settings_just_closes() {
        // l'écran de paramétrage ne touche plus au mode de déplacement (il se
        // choisit au magasin de la station) : la fermeture ne fait que fermer
        let mut state = GameState::new();
        state.settings_box = true;
        state.moving_mode = MOVING_MODE_INERTIAL;

        close_settings(&mut state);

        assert!(!state.settings_box);
        assert!(state.message_queue.is_empty());
        assert_eq!(state.moving_mode, MOVING_MODE_INERTIAL);
    }

    #[test]
    fn moving_mode_labels_match_catalog() {
        // noms affichés (magasin, messages HUD) = catalogue `MOVING_MODES`
        // généré par l'outil de gestion
        assert_eq!(crate::marketplace::mode_label(MOVING_MODE_REALISTIC), "REALISTIC");
        assert_eq!(crate::marketplace::mode_label(MOVING_MODE_INERTIAL), "INERTIAL");
        assert_eq!(crate::marketplace::mode_label(MOVING_MODE_4_WAYS), "4 WAYS");
        assert_eq!(crate::marketplace::mode_label(MOVING_MODE_DIRECTIONAL), "DIRECTIONAL");
        // un mode hors catalogue n'a pas de nom
        assert_eq!(crate::marketplace::mode_label(99), "?");
    }

    #[test]
    fn reset_settings_restores_defaults() {
        // bouton RESET : génération automatique active, rendu texturé,
        // fenêtre 960×540 et anticrénelage éteint (sons non testables hors
        // jeu) - le mode de déplacement n'est pas un réglage : il n'est pas
        // touché
        let mut state = GameState::new();
        state.moving_mode = MOVING_MODE_4_WAYS;
        state.auto_generate = false;
        state.render_style = RenderStyle::Mesh;
        state.window_size = 2;
        state.antialias = true;

        reset_settings_fields(&mut state);

        assert_eq!(state.moving_mode, MOVING_MODE_4_WAYS);
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

    /// Ajoute une forme de test (un triangle, positionné à `(x, y)`) à la
    /// suite des formes existantes - pour les scènes construites autour du
    /// vaisseau mesh réel (il occupe l'index 0 avec sa plage de triangles
    /// allouée). Le triangle porte `t.position = (0,0)` : `moving_shape`
    /// calcule les positions réelles depuis la position de la forme (le
    /// double-application des `test_triangle` à coordonnées non nulles
    /// décalerait le triangle hors de la scène).
    fn push_test_shape(
        shapes: &mut Vec<Shape>,
        triangles: &mut Vec<Triangle>,
        who: i32,
        x: f64,
        y: f64,
    ) -> usize {
        let idx = shapes.len();
        let first = triangles.len();
        shapes.push(test_shape(who, first, first, x, y));
        triangles.push(test_triangle(first as i32, idx as i32, 0.0, 0.0));
        idx
    }

    /// Vaisseau joueur (mesh réel, plage allouée à la composition maximale -
    /// le respawn reconstruit le vaisseau) + cosmonaute EVA (non-collider) +
    /// météore qui chevauche le vaisseau : le décor du test d'éjection.
    fn ejection_scene() -> (GameState, Vec<Shape>, Vec<Triangle>) {
        let mut state = GameState::new();
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        // joueur : le mesh réel du vaisseau (index 0)
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        // cosmonaute EVA : garé en bord de monde, non-collider (les météores
        // le traversent : son seul objectif est de rejoindre la base)
        let eva = push_test_shape(&mut shapes, &mut triangles, WHOIAM_COSMONAUT, -1400.0, -1400.0);
        shapes[eva].is_collider = false;
        // météore : 1 triangle, chevauche le vaisseau (distance 2 < rayons 20)
        push_test_shape(&mut shapes, &mut triangles, WHOIAM_METEOR, 2.0, 2.0);
        state.eva_cosmonaut = eva as i32;
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
    fn cosmonaut_reaching_the_station_starts_recovery_then_is_rescued() {
        // le cosmonaute EVA atteint la zone d'accostage (centre de la station)
        // → la RÉCUPÉRATION démarre : un cordon le ramène sur l'anneau
        // (`eva_recovery`), puis le fondu enchaîné fait apparaître le vaisseau
        // reconstruit au centre (`eva_crossfade`) - à la fin, il est secouru :
        // contrôle revenu au vaisseau, cosmonaute garé à son poste
        let (mut state, mut shapes, mut triangles) = ejection_scene();
        // le vaisseau est détruit et le cosmonaute éjecté au centre (la zone)
        shapes[PLAYER_INDEX].life = 0;
        triangles[0].life = 0;
        state.cosmonaut_active = true;
        let eva = state.eva_cosmonaut as usize;
        shapes[eva].position = Point::new(0.0, 0.0);
        let mut elements = default_elements();

        docking(&mut state, &mut shapes, &mut triangles, &mut elements);

        // la récupération démarre (pas de secours immédiat) : le monde est
        // gelé, le cosmonaute reste le pilote
        assert!(state.eva_recovery > 0.0);
        assert!(state.cosmonaut_active);
        assert_eq!(pilot_index(&state), eva);
        assert_eq!(shapes[PLAYER_INDEX].life, 0); // vaisseau pas encore reconstruit

        // récupération terminée : le cosmonaute est ramené sur l'anneau (le
        // rayon cible), le vaisseau reconstruit au centre (liens attachés),
        // le fondu enchaîné démarre
        advance_eva_recovery(&mut state, &mut shapes, &mut triangles, EVA_RECOVERY_DURATION);
        let to = state.eva_recovery_to_pos;
        assert!(state.eva_recovery <= 0.0);
        assert!(state.eva_crossfade > 0.0);
        // vaisseau reconstruit : toutes ses faces **visibles** revivent
        // (`life` = nombre de triangles de la composition aux niveaux
        // courants - les plans liés aux upgrades n'apparaissent qu'à partir
        // de leur niveau)
        assert_eq!(shapes[PLAYER_INDEX].life, crate::vaisseau::vaisseau_visible_face_count(&state) as i32);
        assert!(state.dock_links); // démarre à quai, comme au lancement
        assert_eq!(shapes[eva].position, to); // sur l'anneau (cosmonaute éjecté au centre : vers la droite)

        // fondu terminé : le secours est complet - contrôle revenu au
        // vaisseau, cosmonaute garé à son poste
        advance_eva_crossfade(&mut state, &mut shapes, &mut triangles, EVA_CROSSFADE_DURATION);
        assert!(!state.cosmonaut_active);
        assert_eq!(shapes[PLAYER_INDEX].position, Point::new(0.0, 0.0));
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
    fn eva_cosmonaut_picks_up_nearby_minerals() {
        // le cosmonaute EVA ramasse un minerai proche : minerai détruit, son
        // élément compté dans la soute (rapportée à la station au secours)
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_MINERAL, 2, 2, 105.0, 100.0), // à 5 unités
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 105.0, 100.0),
        ];
        triangles[2].element = 1; // GOLD
        let mut elements = default_elements();

        eva_collect_minerals(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 0, "le minerai est ramassé");
        assert_eq!(triangles[2].life, 0);
        assert_eq!(elements[1].count, 1);
        assert_eq!(state.player.cargo_qty, 1);
        // le cosmonaute n'est pas touché par le ramassage
        assert_eq!(shapes[1].life, 1);
    }

    #[test]
    fn eva_cosmonaut_ignores_distant_or_inactive_minerals() {
        // minerai trop loin du cosmonaute : pas de ramassage
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_MINERAL, 2, 2, 200.0, 100.0), // à 100 unités
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 200.0, 100.0),
        ];
        triangles[2].element = 1;
        let mut elements = default_elements();

        eva_collect_minerals(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 1, "minerai trop loin");
        assert_eq!(state.player.cargo_qty, 0);

        // vaisseau intact (pas d'EVA) : le cosmonaute garé ne ramasse rien
        let mut state = GameState::new();
        state.eva_cosmonaut = 1;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_MINERAL, 2, 2, 105.0, 100.0),
        ];
        let mut triangles = vec![
            test_triangle(0, 0, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 105.0, 100.0),
        ];
        triangles[2].element = 1;
        let mut elements = default_elements();

        eva_collect_minerals(&mut state, &mut shapes, &mut triangles, &mut elements, None);

        assert_eq!(shapes[2].life, 1, "vaisseau intact : pas d'EVA");
        assert_eq!(state.player.cargo_qty, 0);
    }

    #[test]
    fn ejected_cargo_minerals_are_not_absorbed_by_meteors() {
        // REGRESSION : les minerais de la soute rejetés au crash étaient
        // absorbés par le météore du crash (encore vivant, posé sur le
        // vaisseau détruit) AVANT que le cosmonaute ne puisse les ramasser -
        // le minerai était perdu. Un minerai de soute (`ejected_cargo`)
        // chevauchant un météore doit **survivre** à la collision, quand une
        // minerai normal (minerai libéré d.un météore détruit) est absorbé.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_MINERAL, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[0].element = 1; // GOLD
        shapes[0].ejected_cargo = true; // minerai de soute (rejeté au crash)
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le minerai de soute survit (pas absorbé), le météore n.a rien gagné
        assert_eq!(shapes[0].life, 1, "le minerai de soute ne doit pas être absorbé");
        assert_eq!(triangles[0].life, 1);
        assert_eq!(shapes[1].minerals, 0);
        assert_eq!(shapes[1].life, 1);

        // un minerai NORMAL au même endroit, lui, est absorbé par le météore
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_MINERAL, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[0].element = 1; // GOLD
        // ejected_cargo reste false (défaut) : le minerai est absorbé
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(shapes[0].life, 0, "un minerai normal est absorbé");
        assert_eq!(shapes[1].minerals, 1);
    }
}
