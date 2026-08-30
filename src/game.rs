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
use crate::cosmonaut::{animate_eva_cosmonaut};
// gameplay « météores & collisions » (force de réaction à la base, débris,
// plafond et génération des météores) : constantes de la carte éponyme de
// l'outil de gestion (src/marketplace.rs, généré)
use crate::marketplace::*;
use crate::garbage::{generate_garbages, moving_garbage, Garbage};
use crate::generate::{
    create_alien, create_boss_meteor, create_mineral, create_shape, create_warp_gate,
    eject_cargo_minerals, release_meteor_minerals,
};
use crate::persist;
use crate::scenario;
use crate::geom::{Point, Triangle};
use crate::render::{camera_for, cycle_view_mode, help_box_layout, mouse_to_game};
use crate::shape::{
    compute_shape_center, detect_collision, moving_shape, resolve_elastic_collision, Shape,
};
use crate::state::{Element, GameState};
// sous-modules issus du découpage de ce fichier : accostage (`docking`),
// cosmonaute EVA (`eva`), contrôles (`input`), écran de paramétrage
// (`settings`) et magasin de la station (`shop`) - voir `main.rs`
use crate::docking::*;
use crate::eva::*;
use crate::input::*;
use crate::settings::*;
use crate::shop::*;

/// Action demandée par la boucle de jeu pour la frame courante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    /// Relance le jeu (bouton RESTART de l'écran de paramétrage, ex après
    /// un changement d'anticrénelage) : `main.rs` relance l'exécutable puis
    /// quitte.
    Restart,
    /// GAME OVER (touche R ou bouton NEW GAME du HUD) : repartir du début -
    /// progression remise à zéro et vaisseau renaît à quai (voir
    /// `reset_for_new_game`, appelée par `main.rs`). Le monde continue.
    NewGame,
    /// GAME OVER (touche T ou bouton TITLE du HUD) : retour à l'écran titre -
    /// progression sauvegardée, puis l'écran titre repropose poursuivre ou
    /// repartir au lancement suivant (géré par `main.rs`).
    BackToTitle,
    Continue,
}

/// Traite l'input, les contrôles joueur, la physique et les collisions pour
/// une frame. Renvoie l'action demandée et la caméra (centrée joueur) à
/// utiliser pour le rendu.
///
/// Ordre fidèle à `mainLoop` : input → contrôles → compteurs de poussée →
/// remise à zéro des indicateurs de collision → déplacements (formes gelées
/// en pause, débris toujours actifs - comportement de l'original) →
/// collisions → caméra → génération automatique.
// Signature volontairement plate (miroir de l'original QB64) : tous les
// vecteurs du monde sont passés par la boucle principale - `#[allow]`
// ciblé plutôt qu'un refactor risqué en struct.
#[allow(clippy::too_many_arguments)]
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

    // Écran de **briefing pré-partie** (scénarios custom avec objectifs,
    // affiché au lancement de la partie) : seul l'input de l'écran est traité
    // - ENTRÉE / ÉCHAP / clic sur CLOSE ferment le briefing et libèrent la
    // partie. Le monde, lui, continue de tourner derrière (le vaisseau à
    // quai est protégé - voir `collisions`).
    if state.briefing_box {
        // défilement du contenu : souris (saisie/déplacement du curseur de
        // l'ascenseur ou clic sur la piste - offset absolu) ou clavier
        // (molette, flèches, PgPréc/PgSuiv - offset relatif). L'offset est
        // borné par le contenu réel - un long briefing se fait avec un
        // ascenseur sans rien déborder du panneau (`draw_briefing_box`)
        if let Some(scroll) = crate::hud::briefing_mouse_scroll(state) {
            state.briefing_scroll = scroll;
        } else {
            state.briefing_scroll = (state.briefing_scroll + crate::hud::briefing_scroll_delta())
                .clamp(0.0, crate::hud::briefing_scroll_max(state));
        }
        if is_key_pressed(KeyCode::Enter)
            || is_key_pressed(KeyCode::Escape)
            || crate::hud::briefing_close_clicked()
        {
            state.briefing_box = false;
        }
        collisions(state, shapes, triangles, garbages, elements, rng, sounds.as_deref_mut(), dt);
        return (Action::Continue, camera);
    }

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
    // - seules les fins de partie restent actives : R = nouvelle partie
    // (progression remise à zéro), T = retour à l'écran titre, ESC = quitter
    // (ci-dessus) ; les deux boutons cliquables du HUD renvoient aux mêmes
    // actions (tactile inclus - le toucher génère un clic). Le HUD affiche
    // GAME OVER + le rappel des touches.
    if state.game_over {
        if is_key_pressed(KeyCode::R) {
            return (Action::NewGame, camera);
        }
        if is_key_pressed(KeyCode::T) {
            return (Action::BackToTitle, camera);
        }
        if let Some(action) = game_over_button_click() {
            return (action, camera);
        }
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
            // FABRICATION : un consommable est fabriqué à partir des minerais
            // de la soute (prélevés - `scenario::craft`), ajouté à
            // l'inventaire (touches 1/2/3 pour utiliser en vol)
            ShopClick::Craft(i) => match scenario::craft_consumable(state, elements, i) {
                scenario::CraftOutcome::Crafted(_) => {
                    state.shop_feedback = "Consommable fabriqué".to_string();
                    state.shop_feedback_ok = true;
                }
                scenario::CraftOutcome::NotEnough => {
                    state.shop_feedback = "PAS ASSEZ DE MINERAIS EN SOUTE".to_string();
                    state.shop_feedback_ok = false;
                }
            },
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

    // L : journal de bord - les EVENT_LOG_LEN derniers événements (tirs,
    // minerais, accostages, achats…) dans un panneau consultable (la touche
    // n'est plus utilisée dans le jeu)
    if is_key_pressed(KeyCode::L) {
        state.log_box = !state.log_box;
    }

    // 1 / 2 / 3 : utiliser un consommable fabriqué (onglet FABRICATION du
    // magasin) - 1 = bouclier temporaire, 2 = boost de vitesse, 3 = mine
    if is_key_pressed(KeyCode::Key1) {
        scenario::use_consumable(state, shapes, triangles, CRAFT_SHIELD);
    }
    if is_key_pressed(KeyCode::Key2) {
        scenario::use_consumable(state, shapes, triangles, CRAFT_BOOST);
    }
    if is_key_pressed(KeyCode::Key3) {
        scenario::use_consumable(state, shapes, triangles, CRAFT_MINE);
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

    // ─── session : temps de vol, distance parcourue, difficulté, vagues ─────
    // (hors pause et hors fin de partie - le monde gelé ne compte pas)
    if !state.paused && !state.game_over {
        // temps de partie (moteur de la difficulté adaptative) + distance
        // parcourue (vitesse du vaisseau × 60 × dt, comme `moving_shape`)
        state.session_time += dt;
        state.session_stats.flight_time += dt;
        let speed = shapes[PLAYER_INDEX].velocity;
        state.session_stats.distance += speed * 60.0 * dt;
        // décompte du boost de vitesse (consommable)
        if state.boost_timer > 0.0 {
            state.boost_timer = (state.boost_timer - dt).max(0.0);
        }
        // décomptes des vagues : météore spécial (boss) et portails
        if state.boss_timer > 0.0 {
            state.boss_timer = (state.boss_timer - dt).max(0.0);
        }
        if state.warp_timer > 0.0 {
            state.warp_timer = (state.warp_timer - dt).max(0.0);
        }
        // apparition du boss : à échéance, s'il n'y en a pas déjà un vivant
        if state.boss_timer <= 0.0 && !alive_boss(shapes) {
            create_boss_meteor(state, shapes, triangles, camera, elements, rng);
            state.send_message("SPECIAL METEOR INBOUND!");
            state.log_event("MÉTÉORE SPÉCIAL EN APPROCHE");
            state.boss_timer = BOSS_SPAWN_INTERVAL;
        }
        // apparition d'un portail : à échéance, si la limite n'est pas atteinte
        if state.warp_timer <= 0.0 && alive_warp_gates(shapes) < WARP_GATE_MAX {
            create_warp_gate(state, shapes, triangles, camera, rng);
            state.log_event("PORTAL SPAWNED");
            state.warp_timer = WARP_GATE_SPAWN_INTERVAL;
        }
    }

    // formes vivantes (avec nettoyage des formes « oubliées » par la logique)
    let alive_shapes = count_alive_shapes(shapes, triangles);

    // génération automatique (ex `mainLoop`) - non gelée par la pause, comme
    // l'original. **Difficulté adaptative** (`difficulty.rs`) : la chance par
    // frame croît avec les paliers (0,05 → 0,15 max) et la population maximale
    // reçoit un bonus par palier (en plus du +1 par météore détruit).
    let max_shapes = state.max_meteor_shapes + crate::difficulty::max_meteors_bonus(state);
    let spawn_chance = crate::difficulty::spawn_chance(state);
    if state.auto_generate
        && alive_shapes < max_shapes
        && rng.r#gen::<f64>() > 1.0 - spawn_chance
    {
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
                    // crédits gagnés cumulés (score composite)
                    state.credits_earned += result.reward.amount as i32;
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
#[allow(clippy::too_many_arguments)]
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
        for s in shapes.iter_mut() {
            moving_shape(s, triangles, &state.world, dt);
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
            if x_dist <= sum_radius && y_dist <= sum_radius
                && detect_collision(&shapes[i], &shapes[j], i, j, triangles) {
                    // pas de choc élastique entre un minerai et (vaisseau ou
                    // météore), ni avec la station - ni avec les portails
                    // (statiques, ils ne bougent pas) et les mines (posées,
                    // elles explosent - voir la résolution)
                    let no_elastic = (shapes[i].who_i_am == WHOIAM_MINERAL
                        && (shapes[j].who_i_am == WHOIAM_PLAYER || shapes[j].who_i_am == WHOIAM_METEOR))
                        || (shapes[j].who_i_am == WHOIAM_MINERAL
                            && (shapes[i].who_i_am == WHOIAM_PLAYER || shapes[i].who_i_am == WHOIAM_METEOR))
                        || shapes[i].who_i_am == WHOIAM_STATION
                        || shapes[j].who_i_am == WHOIAM_STATION
                        || shapes[i].who_i_am == WHOIAM_WARP_GATE
                        || shapes[j].who_i_am == WHOIAM_WARP_GATE
                        || shapes[i].who_i_am == WHOIAM_MINE
                        || shapes[j].who_i_am == WHOIAM_MINE;
                    if !no_elastic {
                        elastic_pairs.push((i, j));
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
    // triangles de la base déjà endommagés cette frame (un impact par
    // météore par frame - pas de cumul multiple pour un même chevauchement)
    let mut damaged_station_tris: Vec<usize> = Vec::new();
    // le bouclier temporaire (consommable) absorbe au plus un impact par frame
    let mut temp_absorbed = false;
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
            state.log_event(&format!(
                "MINERAL RÉCUPÉRÉ ({})",
                if element < elements.len() {
                    elements[element].name.clone()
                } else {
                    "?".to_string()
                }
            ));
        } else if collid_by_who == WHOIAM_MINERAL && who == WHOIAM_PLAYER {
            // déjà résolu côté minerai (cargaison pleine)
        } else if collid_by_who == WHOIAM_STATION && who == WHOIAM_PLAYER {
            // accostage (M5)
        } else if who == WHOIAM_STATION {
            // la station est indestructible - mais les impacts de **météores**
            // l'**endommagent** : chaque triangle percuté gagne 1 point de
            // dégât ; à `STATION_TRIANGLE_DAMAGE_MAX`, le triangle meurt (un
            // trou s'ouvre dans l'anneau - les météores suivants peuvent
            // passer à travers). Les balles et le vaisseau (accostage) ne
            // l'endommagent pas. Une seule fois par triangle par frame.
            if collid_by_who == WHOIAM_METEOR && !damaged_station_tris.contains(&i) {
                damaged_station_tris.push(i);
                let tri = &mut triangles[i];
                tri.damage += 1;
                if tri.damage >= STATION_TRIANGLE_DAMAGE_MAX {
                    tri.life = 0;
                }
            }
        } else if who == WHOIAM_WARP_GATE {
            // portail : indestructible (il ne se consume que lorsque le
            // vaisseau le traverse - voir la branche joueur ci-dessous)
        } else if who == WHOIAM_MINE {
            // mine : indestructible aux chocs (elle explose au contact d'un
            // météore - voir la branche météore ci-dessous)
        } else if collid_by_who == WHOIAM_WARP_GATE && who == WHOIAM_PLAYER {
            // le vaisseau traverse un **portail** : téléporté d'une fraction
            // du monde torique (`WARP_JUMP_FRACTION` × largeur) dans la
            // direction qui l'éloigne du portail - raccourci stratégique ou
            // fuite. Le portail est consommé (une seule fois : sa vie passe
            // à 0 au premier passage, les triangles suivants en collision de
            // la même frame ne refont rien).
            let gate_idx = collid_by as usize;
            if shapes[gate_idx].life <= 0 {
                continue;
            }
            let gate_pos = shapes[gate_idx].position;
            let ship_pos = shapes[PLAYER_INDEX].position;
            // direction de la fuite : du centre du portail vers le vaisseau
            let mut dx = ship_pos.x - gate_pos.x;
            let mut dy = ship_pos.y - gate_pos.y;
            let norm = dx.hypot(dy);
            if norm < 1.0 {
                // vaisseau pile sur le portail : on suit son orientation
                dx = shapes[PLAYER_INDEX].orientation.cos();
                dy = -shapes[PLAYER_INDEX].orientation.sin();
            } else {
                dx /= norm;
                dy /= norm;
            }
            let jump = WARP_JUMP_FRACTION * WORLD_WIDTH;
            let mut p = Point::new(ship_pos.x + dx * jump, ship_pos.y + dy * jump);
            p.normalize_world(&state.world);
            shapes[PLAYER_INDEX].position = p;
            state.send_message("WARP JUMP!");
            state.log_event("PORTAL: WARP JUMP");
            // le portail est consommé (il disparaît)
            shapes[gate_idx].life = 0;
            for t in &mut triangles[shapes[gate_idx].first_triangle..=shapes[gate_idx].last_triangle] {
                t.life = 0;
            }
        } else if who == WHOIAM_PLAYER && !temp_absorbed && state.temp_shield > 0.0 {
            // bouclier temporaire (consommable SHIELD) : absorbe l'impact
            // sans toucher à la coque ni au bouclier du scénario, dans tous
            // les scénarios - jusqu'à épuisement
            temp_absorbed = true;
            state.temp_shield = (state.temp_shield - 1.0).max(0.0);
            if state.temp_shield <= 0.0 {
                state.send_message("TEMPORARY SHIELD DEPLETED");
            }
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
            // est tué au premier triangle). Un minerai **rejeté de la soute**
            // du vaisseau détruit est absorbé comme n.importe quel minerai
            // (il suit les règles du monde - récupérable en détruisant le
            // météore qui l.a avalé) ; sans choc élastique (météore/minerai),
            // le minerai traverse simplement le météore.
            if shapes[shape_index].life > 0 {
                let mineral_pos = shapes[shape_index].position;
                if let Some(meteor) = nearest_meteor(shapes, mineral_pos) {
                    shapes[meteor].minerals += 1;
                }
                shapes[shape_index].life = 0;
                for t in &mut triangles[shapes[shape_index].first_triangle..=shapes[shape_index].last_triangle] {
                    t.life = 0;
                }
            }
        } else if collid_by_who == WHOIAM_MINERAL && who == WHOIAM_METEOR {
            // déjà résolu côté minerai (absorption) : le météore n.est pas
            // endommagé en avalant le minerai (il le traverse simplement)
        } else if collid_by_who == WHOIAM_STATION
            && who == WHOIAM_MINERAL
            && shapes[shape_index].ejected_cargo
        {
            // minerai de **soute relâché** au crash (crash près de la base) :
            // il traverse la station sans être détruit - comme il n'est pas
            // absorbé par les météores, il doit rester ramassable par le
            // cosmonaute EVA / le vaisseau ressuscité (le minerai n.est pas
            // perdu avec le vaisseau)
        } else if collid_by_who == WHOIAM_MINE && who == WHOIAM_METEOR {
            // une **mine** explose au contact d'un météore : tous les
            // triangles de météore dans son rayon sont détruits (débris,
            // minerais libérés, réputation/score comptés) - la mine est
            // consommée. Une seule fois par mine (sa vie passe à 0 au
            // premier contact).
            explode_mine(
                state,
                shapes,
                triangles,
                garbages,
                elements,
                rng,
                sounds.as_deref_mut(),
                collid_by as usize,
            );
        } else if collid_by_who == WHOIAM_MINE && who == WHOIAM_BULLET {
            // une munition qui touche une mine est détruite (la mine, elle,
            // n'explose qu'au contact d'un météore)
            let bullet_idx = collid_by as usize;
            if bullet_idx < shapes.len() && shapes[bullet_idx].who_i_am == WHOIAM_BULLET {
                shapes[bullet_idx].life = 0;
                for t in &mut triangles[shapes[bullet_idx].first_triangle..=shapes[bullet_idx].last_triangle] {
                    t.life = 0;
                }
            }
        } else if collid_by_who == WHOIAM_MINE && who == WHOIAM_PLAYER {
            // le vaisseau chevauche une mine (déployée **sous lui**, à sa
            // position)) : la mine ne réagit qu'au contact d'un **météore** -
            // elle n'explose pas et n'endommage pas le vaisseau, qui reste
            // intact et s'en éloigne simplement. Sans ce cas dédié (placé
            // AVANT la branche générique `who == WHOIAM_PLAYER`), déployer
            // une mine détruisait le vaisseau sans aucune collision visible.
        } else if who == WHOIAM_PLAYER {
            // vaisseau joueur : mesh multi-triangles (35 faces) mais toujours
            // « 1 impact = détruit » (l'ancien triangle unique valait 1 vie)
            // - tous les triangles meurent en même temps, le vaisseau ne
            // s'effrite pas impact après impact (une seule fois : `life`
            // passe à 0, les autres triangles en collision de la même frame
            // ne refont rien)
            if shapes[shape_index].life > 0 {
                shapes[shape_index].life = 0;
                for t in &mut triangles[shapes[shape_index].first_triangle..=shapes[shape_index].last_triangle] {
                    t.life = 0;
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
                // le météore spécial (boss) libère son PLATINUM et rapporte
                // un bonus de réputation à sa destruction (le minerai rare
                // renforce la réputation - voir le README)
                if shapes[shape_index].is_boss {
                    state.resources.reputation +=
                        scenario::scenario(state.scenario).reputation_per_asteroid * 5.0;
                    state.send_message("SPECIAL METEOR DESTROYED: PLATINUM +");
                    state.log_event("MÉTÉORE SPÉCIAL DÉTRUIT (PLATINUM)");
                } else {
                    state.log_event("MÉTÉORE DÉTRUIT");
                }
                scenario::on_meteor_destroyed(state);
                // le score composite vient de monter : record relevé et
                // persisté si battu (clé `highscore_<index>`)
                scenario::maybe_update_high_score(state);
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
                    for t in &mut triangles[shapes[bullet_idx].first_triangle..=shapes[bullet_idx].last_triangle] {
                        t.life = 0;
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
            if triangles[i].element > 0 && who == WHOIAM_METEOR
                && collid_by_who == WHOIAM_BULLET && triangles[i].element > 0 {
                    let source = triangles[i];
                    create_mineral(shapes, triangles, elements, &source, rng);
                    // statistiques de session : triangles minéralisés détruits
                    state.session_stats.minerals_destroyed += 1;
                    if shapes[shape_index].minerals > 0 {
                        shapes[shape_index].minerals -= 1;
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

    // NB : le **cosmonaute EVA ne ramasse pas les minerais** - ceux relâchés
    // au crash restent dans l'espace (`eject_cargo_minerals`) et ne seront
    // récupérés que par le vaisseau reconstruit (ou ressuscité en Survival)
    // à son retour, par collision.
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
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Un **météore spécial** (boss) est-il vivant ? - un seul boss à la fois :
/// pas d'apparition tant que le précédent n'est pas détruit.
fn alive_boss(shapes: &[Shape]) -> bool {
    shapes
        .iter()
        .any(|s| s.who_i_am == WHOIAM_METEOR && s.is_boss && s.life > 0)
}

/// Nombre de **portails** vivants (plafonné à `WARP_GATE_MAX`).
fn alive_warp_gates(shapes: &[Shape]) -> i32 {
    shapes
        .iter()
        .filter(|s| s.who_i_am == WHOIAM_WARP_GATE && s.life > 0)
        .count() as i32
}

/// Explosion d'une **mine** (consommable fabriqué) au contact d'un météore :
/// tous les météores dont le centre est dans `MINE_RADIUS` sont détruits
/// (triangles tués, débris, minerais libérés, réputation/score comptés), la
/// mine est consommée. Une seule fois par mine (`life` passé à 0 au premier
/// contact - les contacts suivants de la même frame ne refont rien).
#[allow(clippy::too_many_arguments)]
fn explode_mine(
    state: &mut GameState,
    shapes: &mut Vec<Shape>,
    triangles: &mut Vec<Triangle>,
    garbages: &mut Vec<Garbage>,
    elements: &mut [Element],
    rng: &mut impl Rng,
    mut sounds: Option<&mut Sounds>,
    mine_idx: usize,
) {
    if mine_idx >= shapes.len()
        || shapes[mine_idx].who_i_am != WHOIAM_MINE
        || shapes[mine_idx].life <= 0
    {
        return;
    }
    let mine_pos = shapes[mine_idx].position;
    // la mine est consommée par l'explosion
    shapes[mine_idx].life = 0;
    for t in &mut triangles[shapes[mine_idx].first_triangle..=shapes[mine_idx].last_triangle] {
        t.life = 0;
    }
    state.send_message("MINE EXPLODED");
    state.log_event("MINE EXPLOSION");
    if let Some(sounds) = sounds.as_mut() {
        let dx = mine_pos.x - shapes[PLAYER_INDEX].position.x;
        let dy = mine_pos.y - shapes[PLAYER_INDEX].position.y;
        let dist = dx.hypot(dy);
        let v = (1.0 - dist / WORLD_WIDTH.hypot(WORLD_HEIGHT)).powi(3) as f32;
        sounds.play_explosion(rng, v);
    }
    // tous les météores dans le rayon de l'explosion
    let in_radius: Vec<usize> = shapes
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.who_i_am == WHOIAM_METEOR
                && m.life > 0
                && (m.position.x - mine_pos.x).hypot(m.position.y - mine_pos.y) <= MINE_RADIUS
        })
        .map(|(i, _)| i)
        .collect();
    for mi in in_radius {
        let mfirst = shapes[mi].first_triangle;
        let mlast = shapes[mi].last_triangle;
        // minerais des triangles minéralisés libérés (comme un tir - le
        // minerai n'est pas détruit avec le météore)
        for ti in mfirst..=mlast {
            if triangles[ti].life > 0 && triangles[ti].element > 0 {
                let source = triangles[ti];
                create_mineral(shapes, triangles, elements, &source, rng);
                state.session_stats.minerals_destroyed += 1;
                if shapes[mi].minerals > 0 {
                    shapes[mi].minerals -= 1;
                }
            }
        }
        // débris depuis le premier triangle encore vivant
        if let Some(t) = triangles[mfirst..=mlast].iter().find(|t| t.life > 0) {
            generate_garbages(garbages, t, shapes, rng);
        }
        for t in &mut triangles[mfirst..=mlast] {
            t.life = 0;
        }
        // le météore est détruit : réputation, score, record, progression
        if shapes[mi].life > 0 {
            shapes[mi].life = 0;
            state.meteors_destroyed += 1;
            if shapes[mi].is_boss {
                state.resources.reputation +=
                    scenario::scenario(state.scenario).reputation_per_asteroid * 5.0;
                state.log_event("MÉTÉORE SPÉCIAL DÉTRUIT (PLATINUM)");
            }
            scenario::on_meteor_destroyed(state);
            scenario::maybe_update_high_score(state);
            let _ = scenario::save_progression(state);
        }
        // minerais absorbés restants libérés (jamais détruits avec le météore)
        release_meteor_minerals(shapes, triangles, elements, mi, rng);
    }
}

/// Compte les formes vivantes et nettoie les formes « oubliées » par la
/// logique (tous leurs triangles morts → forme morte), ex la boucle de
/// dessin de `mainLoop`. Le vaisseau (index 0) n'est ni compté ni nettoyé.
fn count_alive_shapes(shapes: &mut [Shape], triangles: &[Triangle]) -> i32 {
    let mut alive = 0;
    for s in shapes.iter_mut().skip(1) {
        if s.life <= 0 {
            continue;
        }
        let mut t = 0;
        for tri in &triangles[s.first_triangle..=s.last_triangle] {
            if tri.life > 0 {
                t += 1;
            }
        }
        if t == 0 {
            s.life = 0;
            continue;
        }
        alive += 1;
    }
    alive
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

/// Clic sur les boutons de l'écran GAME OVER (NEW GAME / TITLE, dessinés par
/// le HUD - voir `hud::game_over_buttons_layout`) : renvoie l'action du
/// bouton visé, ou `None`. Fonctionne aussi au tactile (le toucher génère un
/// clic souris).
fn game_over_button_click() -> Option<Action> {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return None;
    }
    let m = mouse_to_game();
    let [restart, title] = crate::hud::game_over_buttons_layout();
    if restart.contains(m) {
        Some(Action::NewGame)
    } else if title.contains(m) {
        Some(Action::BackToTitle)
    } else {
        None
    }
}

/// Nouvelle partie après un GAME OVER (touche R, bouton NEW GAME) : la
/// progression du scénario est remise à zéro (clés `prog_*` supprimées,
/// règles de départ réappliquées - vies et bouclier pleins en Survival,
/// compteurs d'objectifs à zéro, extensions d'atelier perdues) et le vaisseau
/// renaît **à quai** au centre de la station, coque reconstruite selon les
/// niveaux remis à zéro (voir `eva::respawn_player`). Le monde (météores,
/// débris, minerais) continue de tourner. Appelée par `main.rs` sur
/// `Action::NewGame`.
pub fn reset_for_new_game(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    // supprime les clés `prog_*` du scénario puis réapplique les règles de
    // départ (ne renvoie rien - pas d'erreur possible en cas de clé absente)
    scenario::reset_progression(state);
    crate::eva::respawn_player(state, shapes, triangles);
    state.paused = false;
    state.send_message("NEW GAME");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cosmonaut::COSMONAUTE_EVA_PARK;
    use crate::state::{default_elements, RenderStyle};
    // fonctions et types issus du découpage de `game.rs` (apportés par
    // `super::*`, les globs du parent)
    use ::rand::SeedableRng;
    use ::rand_chacha::ChaCha12Rng;

    fn seed() -> ChaCha12Rng {
        ChaCha12Rng::seed_from_u64(42)
    }

    /// Construit une forme simple à `n` triangles (index `first..=last`).
    fn test_shape(who: i32, first: usize, last: usize, x: f64, y: f64) -> Shape {
        Shape {
            who_i_am: who,
            is_collider: true,
            first_triangle: first,
            last_triangle: last,
            life: (last - first + 1) as i32,
            radius: 10.0,
            position: Point::new(x, y),
            ..Shape::default()
        }
    }

    /// Construit un triangle simple (sommets locaux fixes) rattaché à une
    /// forme, avec ses positions réelles calculées.
    fn test_triangle(id: i32, shape_index: i32, x: f64, y: f64) -> Triangle {
        let mut t = Triangle {
            id,
            shape_index,
            ..Triangle::default()
        };
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
        // vitesse angulaire en rad/s : plafond = PLAYER_ROTATION_SPEED * 60
        // (la constante d'origine est par frame - voir realistic_rotation_after_input)
        let max_speed = PLAYER_ROTATION_SPEED * 60.0;
        let dt = 1.0 / 60.0;
        let mut speed = 0.0;
        for _ in 0..30 {
            speed = realistic_rotation_after_input(speed, true, false, dt);
        }
        assert!((speed - max_speed).abs() < 1e-9);
        assert_eq!(realistic_rotation_after_input(speed, false, false, dt), speed);
        for _ in 0..30 {
            speed = realistic_rotation_after_input(speed, false, true, dt);
        }
        assert!(speed.abs() < 1e-9);
        assert_eq!(realistic_rotation_after_input(speed, true, true, dt), speed);
    }

    #[test]
    fn thrust_vector_recomputes_polar() {
        let mut player = Shape {
            direction: 0.0,
            velocity: 1.0,
            ..Shape::default()
        };
        // poussée le long de l'orientation (0 = +x), sx=1, sy=-1
        thrust_vector(&mut player, 0.05, 0.0, 1.0, -1.0);
        assert!((player.velocity - 1.05).abs() < 1e-12);
        assert_eq!(player.direction, 0.0);

        // orientation perpendiculaire : la direction dévie (signe -sin)
        let mut player = Shape {
            direction: 0.0,
            velocity: 1.0,
            ..Shape::default()
        };
        thrust_vector(&mut player, 0.05, std::f64::consts::FRAC_PI_2, 1.0, -1.0);
        assert!((player.velocity - 1.0f64.hypot(0.05)).abs() < 1e-12);
        assert!(player.direction < 0.0); // atan2(-0.05, 1)
    }

    #[test]
    fn directional_deceleration_clamps_at_zero() {
        // vérifie la sémantique du bloc Down du mode directionnel : à vitesse
        // nulle, la décélération ne passe pas en négatif.
        let step = PLAYER_ACCELERATION * 60.0 * (1.0 / 60.0);
        // décélération depuis une vitesse positive : converge vers 0 sans
        // jamais passer en négatif (le bloc `else` de l'original force 0)
        let mut v = 1.0;
        for _ in 0..1000 {
            v = if v > 0.0 { (v - step).max(0.0) } else { 0.0 };
        }
        assert_eq!(v, 0.0);
        // vitesse nulle : reste à 0 (pas de décélération négative)
        let mut v = 0.0;
        v = if v > 0.0 { (v - step).max(0.0) } else { 0.0 };
        assert_eq!(v, 0.0);
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
        for t in &mut triangles[shapes[idx].first_triangle..=shapes[idx].last_triangle] {
            t.element = 0;
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
    fn new_game_after_game_over_resets_run_and_ship() {
        // R sur l'écran GAME OVER : progression remise à zéro (ressources et
        // niveaux d'atelier du départ, partie rejouable) et vaisseau renaît à
        // quai au centre de la station, coque reconstruite sans les plans
        // liés aux extensions perdues
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        // une partie avancée : extensions d'atelier achetées, credits
        // accumulés, partie terminée, monde en pause
        state.resources.cargo_level = 2; // le plan lié (index 14) apparaît
        state.resources.credits = 99;
        state.game_over = true;
        state.paused = true;
        let mut shapes = Vec::new();
        let mut triangles = Vec::new();
        crate::vaisseau::create_player_vaisseau(&state, &mut shapes, &mut triangles);
        let upgraded_faces = shapes[0].life;
        shapes[0].position = Point::new(300.0, 200.0);

        reset_for_new_game(&mut state, &mut shapes, &mut triangles);

        // progression remise à zéro : partie rejouable, ressources du départ
        assert!(!state.game_over);
        assert!(!state.paused);
        assert_eq!(state.resources.credits, 0);
        assert_eq!(state.resources.cargo_level, 0);
        // vaisseau renaît à quai (position station, liens attachés, coque
        // reconstruite sans le plan lié à l'extension de soute perdue)
        assert_eq!(shapes[0].position, Point::new(0.0, 0.0));
        assert!(state.dock_links);
        assert_eq!(state.player_at_station, -1);
        assert_eq!(
            shapes[0].life,
            crate::vaisseau::vaisseau_visible_face_count(&state) as i32
        );
        assert!(shapes[0].life < upgraded_faces);
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
    fn meteors_absorb_minerals_released_from_ship_cargo() {
        // SPEC : un météore qui percute un minerai l.absorbe - y compris un
        // minerai **relâché de la soute** au crash : il suit les règles du
        // monde, la quantité absorbée est récupérable en détruisant le
        // météore qui l.a avalé.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_MINERAL, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        triangles[0].element = 1; // GOLD
        shapes[0].ejected_cargo = true; // minerai relâché de la soute au crash
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le minerai relâché de la soute est absorbé par le météore (comme
        // n.importe quel minerai), sans endommager le météore
        assert_eq!(shapes[0].life, 0, "le minerai relâché est absorbé par le météore");
        assert_eq!(triangles[0].life, 0);
        assert_eq!(shapes[1].minerals, 1, "le météore a absorbé le minerai (récupérable en le détruisant)");
        assert_eq!(shapes[1].life, 1, "le météore n'est pas endommagé en avalant le minerai");
    }

    #[test]
    fn destroyed_ship_releases_all_cargo_minerals_without_destruction() {
        // SPEC : quand le vaisseau est détruit, TOUS les minerais de la soute
        // sont relâchés dans l'espace (un minerai par unité), sans destruction
        // au crash - la soute est vidée et les minerais relâchés restent
        // vivants (`ejected_cargo`), **dans l'espace**, jusqu'au retour du
        // vaisseau reconstruit (le cosmonaute EVA ne les ramasse pas). Ils
        // suivent ensuite les règles du monde : absorbés par le météore qui
        // les percute (récupérables en le détruisant), jamais détruits par
        // l'épave (non-collider) ni par la station (crash au centre) - rien
        // n'est perdu.
        let (mut state, mut shapes, mut triangles) = ejection_scene();
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        elements[1].count = 3; // GOLD ×3
        elements[2].count = 2; // IRON ×2
        state.player.cargo_qty = 5;
        let mut rng = seed();

        let alive_minerals = |shapes: &[Shape]| {
            shapes
                .iter()
                .filter(|s| s.who_i_am == WHOIAM_MINERAL && s.life > 0)
                .count()
        };

        // frame du crash : le vaisseau meurt, les 5 minerais sont relâchés
        // dans l'espace (soute vidée) - aucun n'est perdu ni détruit
        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);
        assert_eq!(shapes[PLAYER_INDEX].life, 0, "le vaisseau est détruit");
        assert!(state.cosmonaut_active);
        assert_eq!(state.player.cargo_qty, 0, "soute vidée au crash");
        assert_eq!(alive_minerals(&shapes), 5, "les 5 minerais relâchés restent dans l'espace");
        assert!(
            shapes
                .iter()
                .filter(|s| s.who_i_am == WHOIAM_MINERAL && s.life > 0)
                .all(|s| s.ejected_cargo),
            "les minerais relâchés sont marqués ejected_cargo (protégés de la station)"
        );

        // frames suivantes : le météore du crash chevauche les minerais, la
        // station est au centre du crash - les minerais percutés par le
        // météore sont **absorbés** (récupérables en le détruisant), les
        // autres restent dans l'espace ; rien n'est perdu ni détruit (épave
        // non-collider, station protectrice, cosmonaute qui ne ramasse pas)
        for _ in 0..10 {
            collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);
        }
        assert_eq!(
            state.player.cargo_qty as usize + alive_minerals(&shapes) + shapes[2].minerals as usize,
            5,
            "minerais relâchés : soit dans l'espace, soit absorbés par le météore du crash (rien de perdu)"
        );
    }

    #[test]
    fn meteor_impacts_damage_the_station_triangle() {
        // un météore qui percute la base endommage le triangle percuté
        // (1 point par impact) ; à STATION_TRIANGLE_DAMAGE_MAX, le triangle
        // meurt (un trou s'ouvre dans l'anneau). La détection repose sur la
        // géométrie (les indicateurs `collid` sont resetés en début de
        // frame) : le triangle du météore chevauche celui de la base, la
        // collision est re-détectée à chaque frame. Chaque impact détruit
        // aussi le météore (son triangle explose contre la base) : on le
        // ravive entre les frames pour simuler une série d'impacts.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_STATION, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        for _ in 0..STATION_TRIANGLE_DAMAGE_MAX - 1 {
            shapes[1].life = 1;
            triangles[1].life = 1;
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
        }
        // dégâts cumulés, triangle encore vivant sous le seuil
        assert_eq!(triangles[0].damage, STATION_TRIANGLE_DAMAGE_MAX - 1);
        assert_eq!(triangles[0].life, 1);

        // dernier impact : le triangle de la base meurt
        shapes[1].life = 1;
        triangles[1].life = 1;
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
        assert_eq!(triangles[0].life, 0, "le triangle de la base doit mourir au seuil");
    }

    #[test]
    fn station_damage_ignores_bullets_and_player() {
        // seuls les **météores** endommagent la base : une balle ou le
        // vaisseau (accostage) ne font pas gagner de dégât au triangle
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_STATION, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_BULLET, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        triangles[0].collid = true;
        triangles[0].collid_by = 1; // percuté par la balle
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
        assert_eq!(triangles[0].damage, 0, "une balle n'endommage pas la base");
        assert_eq!(triangles[0].life, 1);
    }

    #[test]
    fn warp_gate_teleports_the_ship_and_is_consumed() {
        // le vaisseau traverse un portail : téléporté d'environ 25 % de la
        // largeur du monde dans la direction qui l'éloigne du portail - le
        // portail est consommé. Le triangle du vaisseau (à 5,0) chevauche
        // celui du portail (à 0,0) : la collision est détectée par la
        // géométrie.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 5.0, 0.0),
            test_shape(WHOIAM_WARP_GATE, 1, 1, 0.0, 0.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 5.0, 0.0), test_triangle(1, 1, 0.0, 0.0)];
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

        // saut ≈ 25 % de WORLD_WIDTH (990) dans la direction qui éloigne le
        // vaisseau du portail : le vaisseau est à droite du portail → +x
        let jump = WARP_JUMP_FRACTION * WORLD_WIDTH;
        assert!(
            (shapes[0].position.x - (5.0 + jump)).abs() < 1.0,
            "position après warp : {}",
            shapes[0].position.x
        );
        assert_eq!(shapes[0].position.y, 0.0);
        assert_eq!(shapes[1].life, 0, "le portail est consommé");
        assert!(state.message_queue.contains("WARP JUMP"));
    }

    #[test]
    fn temp_shield_absorbs_impacts_in_any_scenario() {
        // le bouclier temporaire (consommable) absorbe les impacts dans tous
        // les scénarios : le vaisseau reste intact, le bouclier décroît
        let mut state = GameState::new();
        state.temp_shield = 2.0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 1, 1, 2.0, 2.0),
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        triangles[0].collid = true;
        triangles[0].collid_by = 1; // percuté par le météore
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

        assert_eq!(state.temp_shield, 1.0);
        assert_eq!(shapes[0].life, 1, "le vaisseau est intact");
        assert_eq!(triangles[0].life, 1);
        assert_eq!(state.resources.lives, 0); // aucun impact subi (classique)
    }

    #[test]
    fn mine_explodes_destroying_meteors_in_radius() {
        // une mine explose au contact d'un météore : tous les météores dont
        // le centre est dans MINE_RADIUS sont détruits (minerais libérés),
        // la mine est consommée, les météores hors rayon restent intacts.
        // Le vaisseau est garé loin du carnage.
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, -5000.0, -5000.0),
            test_shape(WHOIAM_MINE, 1, 1, 0.0, 0.0),
            test_shape(WHOIAM_METEOR, 2, 2, 2.0, 2.0), // chevauche la mine : déclenche l'explosion
            test_shape(WHOIAM_METEOR, 3, 3, 60.0, 0.0), // dans le rayon 130
            test_shape(WHOIAM_METEOR, 4, 4, 400.0, 0.0), // hors rayon
        ];
        shapes[2].minerals = 2; // 2 minerais absorbés à libérer
        let mut triangles = vec![
            test_triangle(0, 0, -5000.0, -5000.0),
            test_triangle(1, 1, 0.0, 0.0),
            test_triangle(2, 2, 2.0, 2.0),
            test_triangle(3, 3, 60.0, 0.0),
            test_triangle(4, 4, 400.0, 0.0),
        ];
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

        assert_eq!(shapes[1].life, 0, "la mine est consommée");
        assert_eq!(shapes[2].life, 0, "le météore déclencheur est détruit");
        assert_eq!(shapes[3].life, 0, "le météore dans le rayon est détruit");
        assert_eq!(shapes[4].life, 1, "le météore lointain est intact");
        assert_eq!(state.meteors_destroyed, 2);
        // les 2 minerais absorbés du météore déclencheur sont libérés (jamais perdus)
        let minerals = shapes.iter().filter(|s| s.who_i_am == WHOIAM_MINERAL).count();
        assert_eq!(minerals, 2);
        assert!(state.message_queue.contains("MINE EXPLODED"));
    }

    #[test]
    fn ship_overlapping_a_mine_is_not_destroyed() {
        // REGRESSION : déployer une mine (posée **sous** le vaisseau, à sa
        // position) faisait détruire le vaisseau sans aucune collision
        // visible - la collision vaisseau↔mine retombait dans la branche
        // générique qui détruit le vaisseau. La mine ne réagissant qu'au
        // contact d'un météore, le vaisseau qui la chevauche doit rester
        // intact (et la mine reste posée).
        let mut state = GameState::new();
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0),
            test_shape(WHOIAM_MINE, 1, 1, 2.0, 2.0), // mine chevauchant le vaisseau
        ];
        let mut triangles = vec![test_triangle(0, 0, 0.0, 0.0), test_triangle(1, 1, 2.0, 2.0)];
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        assert_eq!(shapes[0].life, 1, "le vaisseau ne doit pas être détruit par une mine chevauchante");
        assert_eq!(triangles[0].life, 1);
        assert_eq!(shapes[1].life, 1, "la mine reste posée (elle n'explose pas au contact du vaisseau)");
    }

    #[test]
    fn destroyed_ship_does_not_reabsorb_scattered_minerals() {
        // REGRESSION : le vaisseau détruit restait un **collider** - il
        // « re-ramassait » dans sa soute (et finissait par détruire) les
        // minerais éparpillés autour du crash. Le vaisseau mort doit cesser
        // d'être un collider : les minerais relâchés restent dans l'espace
        // (pour le vaisseau reconstruit à son retour), la soute reste vide.
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 1; // cosmonaute présent (garé au loin)
        state.player.cargo_qty = 0;
        let mut shapes = vec![
            test_shape(WHOIAM_PLAYER, 0, 0, 0.0, 0.0), // vaisseau détruit (mort, non-collider)
            test_shape(WHOIAM_COSMONAUT, 1, 1, 100.0, 100.0),
            test_shape(WHOIAM_MINERAL, 2, 2, 3.0, 3.0), // minerai éparpillé, chevauche le crash
        ];
        shapes[PLAYER_INDEX].life = 0; // vaisseau mort (triangles tués)
        shapes[PLAYER_INDEX].is_collider = false; // posé par `activate_cosmonaut`
        shapes[1].is_collider = false; // le cosmonaute est un non-collider (comme en jeu)
        let mut triangles = vec![
            test_triangle(0, PLAYER_INDEX as i32, 0.0, 0.0),
            test_triangle(1, 1, 100.0, 100.0),
            test_triangle(2, 2, 3.0, 3.0),
        ];
        triangles[2].element = 1; // GOLD
        let mut garbages = Vec::new();
        let mut elements = default_elements();
        let mut rng = seed();

        collisions(&mut state, &mut shapes, &mut triangles, &mut garbages, &mut elements, &mut rng, None, 0.0);

        // le minerai éparpillé survit (ni aspiré ni détruit par le vaisseau
        // mort) et reste **dans l'espace** - le cosmonaute ne le ramasse pas,
        // la soute reste vide jusqu'au retour du vaisseau
        assert_eq!(shapes[2].life, 1, "le minerai éparpillé reste dans l'espace");
        assert_eq!(triangles[2].life, 1);
        assert_eq!(state.player.cargo_qty, 0, "soute vide : pas de re-ramassage par le vaisseau mort, ni par le cosmonaute");
    }
}
