//! Tests du module `scenario` (définitions, règles affichées, magasin, rangs,
//! atelier, survie, persistance) - déplacés tels quels de `scenario.rs`.
//! `bool_assert_comparison` : un test compare le résultat d'une API (`is_ok`)
//! à `true` - `#[allow]` ciblé sur ce module de tests, le `assert!`
//! équivalent n'apportant pas de lisibilité ici.
#![allow(clippy::bool_assert_comparison)]

use super::*;
use crate::config::{
    MOVING_MODE_4_WAYS, MOVING_MODE_DIRECTIONAL, MOVING_MODE_INERTIAL, MOVING_MODE_REALISTIC,
};
use crate::persist::{get_i32_from, set_i32_to};
use crate::state::default_elements;

/// État prêt pour le scénario Progression (départ appliqué).
fn progression_state() -> GameState {
    let mut s = GameState::new();
    s.scenario = ScenarioId::Progression;
    apply_start(&mut s);
    s
}

/// État prêt pour le scénario Survival (départ appliqué).
fn survival_state() -> GameState {
    let mut s = GameState::new();
    s.scenario = ScenarioId::Survival;
    apply_start(&mut s);
    s
}

/// Chemin temporaire unique par test (répertoire temporaire du système).
fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "meteors_mining_scenario_test_{}_{}",
        std::process::id(),
        name
    ))
}

#[test]
fn scenario_rules_are_derived_from_data() {
    // les règles affichées à l'écran titre décrivent chaque scénario à
    // partir de ses données : coûts des modes, vies, bouclier,
    // invulnérabilité, rangs
    let free = scenario_rules_text(ScenarioId::FreePlay);
    assert!(free.contains("aucun coût"));
    assert!(free.contains("illimités"));

    let prog = scenario_rules_text(ScenarioId::Progression);
    assert!(prog.contains("INERTIAL 15")); // coûts des modes depuis les données
    assert!(prog.contains("4 WAYS 30"));
    assert!(prog.contains("DIRECTIONAL 45"));
    assert!(prog.contains("CADET"));
    assert!(prog.contains("ACE"));
    assert!(prog.contains("crédits"));

    let surv = scenario_rules_text(ScenarioId::Survival);
    assert!(surv.contains("3 vies"));
    assert!(surv.contains("bouclier 3"));
    assert!(surv.contains("×1"));
    assert!(surv.contains("2 s d'invulnérabilité"));
}

#[test]
fn ship_starts_docked_only_when_at_rest_at_station_center() {
    // le vaisseau démarre à quai (liens attachés, « DOCKED ») seulement
    // s'il est **immobile au centre de la station** : position (0,0) et
    // vitesse nulle - voir `start_docked`, appliqué au lancement
    let mut s = GameState::new();
    assert!(start_docked(&s)); // valeurs par défaut : centre, immobile
    // position initiale différente de 0 : le vaisseau démarre en vol
    s.initial_ship_x = 300.0;
    assert!(!start_docked(&s));
    s.initial_ship_x = 0.0;
    s.initial_ship_y = -200.0;
    assert!(!start_docked(&s));
    s.initial_ship_y = 0.0;
    // vitesse initiale non nulle au centre : pas à quai non plus
    s.initial_ship_velocity = 2.0;
    assert!(!start_docked(&s));
    // l'orientation seule n'empêche pas le démarrage à quai
    s.initial_ship_velocity = 0.0;
    s.initial_ship_orientation = 90.0;
    assert!(start_docked(&s));
}

#[test]
fn scenario_rules_mark_values_with_scenario_color() {
    // les valeurs chiffrées (coûts, vies, bouclier, dégâts, rangs) portent
    // `color = Some(...)` - la couleur propre du scénario - et les libellés
    // `None` : c'est ce qui fait ressortir le changement de stat au
    // basculement de scénario (et chaque scénario a sa couleur)
    let prog = scenario_rules(ScenarioId::Progression);
    let highlighted: Vec<&str> = prog
        .iter()
        .filter(|s| s.color.is_some())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(
        highlighted,
        vec!["INERTIAL 15", "4 WAYS 30", "DIRECTIONAL 45", "CADET", "ACE"]
    );
    assert!(prog.iter().any(|s| s.color.is_none() && s.text.contains("rangs")));
    // les valeurs de Progression sont en jaune, celles de Survival en cyan
    assert!(prog.iter().filter(|s| s.color.is_some()).all(|s| s.color == Some(RULES_COLOR_YELLOW)));

    let surv = scenario_rules(ScenarioId::Survival);
    let highlighted: Vec<&str> = surv
        .iter()
        .filter(|s| s.color.is_some())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(highlighted, vec!["3", "3", "1", "2"]);
    assert!(surv.iter().filter(|s| s.color.is_some()).all(|s| s.color == Some(RULES_COLOR_CYAN)));
    assert!(surv.iter().any(|s| s.color.is_none() && s.text.contains("bouclier")));

    // jeu libre : aucune valeur à mettre en évidence
    assert!(scenario_rules(ScenarioId::FreePlay)
        .iter()
        .all(|s| s.color.is_none()));
}

#[test]
fn save_summary_shows_restored_progression() {
    // le résumé d.écran titre décrit la sauvegarde restaurée : crédits,
    // modes débloqués et réputation (+ rang) en Progression ; vies et
    // bouclier en Survival ; aucune en jeu libre
    let free = GameState::new();
    assert!(save_summary(&free).contains("aucune sauvegarde"));

    let mut prog = progression_state();
    prog.resources.credits = 42;
    prog.resources.reputation = 60.0; // ACE
    prog.unlocked_modes = [true, true, false, true];
    let summary = save_summary(&prog);
    assert!(summary.contains("crédits 42"));
    assert!(summary.contains("modes 3/4"));
    assert!(summary.contains("réputation 60"));
    assert!(summary.contains("(ACE)"));

    let mut surv = survival_state();
    surv.resources.lives = 2;
    surv.resources.shield = 1.5;
    let summary = save_summary(&surv);
    assert!(summary.contains("2 vies"));
    assert!(summary.contains("bouclier 1.5"));
}

#[test]
fn save_summary_segments_highlight_values() {
    // les valeurs du résumé (crédits, modes, réputation, rang, vies,
    // bouclier) portent `color = Some(couleur du scénario)`, les libellés
    // `None` - mêmes segments que `save_summary`, pour la coloration à
    // l'écran titre
    let mut prog = progression_state();
    prog.resources.credits = 42;
    prog.resources.reputation = 60.0; // ACE
    prog.unlocked_modes = [true, true, false, true];
    let segs = save_summary_segments(&prog);
    let highlighted: Vec<&str> = segs
        .iter()
        .filter(|s| s.color.is_some())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(highlighted, vec!["42", "3/4", "60", " (ACE)", "0"]);
    assert!(segs.iter().filter(|s| s.color.is_some()).all(|s| s.color == Some(RULES_COLOR_YELLOW)));
    assert_eq!(
        save_summary(&prog),
        segs.iter().map(|s| s.text.as_str()).collect::<String>()
    );

    let mut surv = survival_state();
    surv.resources.lives = 2;
    surv.resources.shield = 1.5;
    let segs = save_summary_segments(&surv);
    let highlighted: Vec<&str> = segs
        .iter()
        .filter(|s| s.color.is_some())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(highlighted, vec!["2", "1.5", "0"]);
    assert!(segs.iter().filter(|s| s.color.is_some()).all(|s| s.color == Some(RULES_COLOR_CYAN)));

    // jeu libre : la seule valeur mise en évidence est le record (toujours
    // affiché, même sans sauvegarde - voir save_summary_segments)
    let free_segs = save_summary_segments(&GameState::new());
    let highlighted: Vec<&str> = free_segs
        .iter()
        .filter(|s| s.color.is_some())
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(highlighted, vec!["0"]); // record 0 du jeu libre
}

#[test]
fn new_record_announced_once_per_session() {
    // « NEW RECORD » : annoncé **une seule fois** par session, au premier
    // dépassement d'un record enregistré non nul ; le tout premier record
    // d'un scénario (record 0) reste silencieux ; une nouvelle partie
    // (apply_start) ou un changement de scénario (load_progression) réarme
    // l'annonce
    let mut s = progression_state();
    // record 0 : premier dépassement silencieux
    s.meteors_destroyed = 5;
    assert!(maybe_update_high_score(&mut s));
    assert_eq!(s.high_score, 5);
    assert!(!s.message_queue.contains("NEW RECORD"), "record 0 silencieux : {}", s.message_queue);

    // points suivants : plus d'annonce (le record vient d'être relevé)
    s.meteors_destroyed = 6;
    assert!(maybe_update_high_score(&mut s));
    assert!(!s.message_queue.contains("NEW RECORD"));

    // nouvelle partie : l'annonce est réarmée (le record 6 survit en état)
    apply_start(&mut s);
    assert!(!s.score_record_announced);
    s.meteors_destroyed = 7; // 7 > 6 : dépassement d'un record non nul
    assert!(maybe_update_high_score(&mut s));
    assert!(s.message_queue.contains("NEW RECORD: 7"), "annonce : {}", s.message_queue);

    // une seule fois : le point suivant ne ré-annonce pas
    s.meteors_destroyed = 8;
    assert!(maybe_update_high_score(&mut s));
    // l'annonce de 7 est restée la seule dans la file
    assert_eq!(s.message_queue.matches("NEW RECORD").count(), 1);
}

#[test]
fn mode_costs_text_skips_free_modes() {
    // seuls les modes payants apparaissent (coût 0 = déjà débloqué, omis)
    assert_eq!(
        mode_costs_text(&PROGRESSION_SCENARIO),
        "INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45 crédits"
    );
    assert_eq!(mode_costs_text(&FREE_PLAY_SCENARIO), "aucun");
}

#[test]
fn free_play_has_no_economy_and_unlimited_resources() {
    // jeu libre (défaut) : aucun coût, tous les modes disponibles,
    // carburant et munitions illimités
    let mut s = GameState::new();
    assert!(!has_economy(&s));
    assert!(fuel_available(&s));
    assert!(try_fire(&mut s).iter().any(|&f| f)); // pas de consommation
    assert_eq!(total_ammo(&s), 0);
    for m in 0..MOVING_MODE_COUNT {
        assert_eq!(locked_cost(&s, m), None);
        assert!(try_select_mode(&mut s, m));
        assert_eq!(s.moving_mode, m);
    }
}

#[test]
fn progression_starts_realistic_with_start_resources() {
    // départ : REALISTIC (mode de départ, gratuit par configuration), les
    // modes payants (INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45) sont
    // verrouillés ; réservoir et chargeur pleins, pas de crédits,
    // réputation nulle
    let s = progression_state();
    assert!(has_economy(&s));
    assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
    assert_eq!(s.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
    assert_eq!(total_ammo(&s), PROGRESSION_SCENARIO.start_ammo); // arme de base
    assert_eq!(s.resources.credits, 0);
    assert_eq!(s.resources.reputation, 0.0);
    assert_eq!(locked_cost(&s, MOVING_MODE_REALISTIC), None);
    assert_eq!(locked_cost(&s, MOVING_MODE_INERTIAL), Some(15));
    assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), Some(30));
    assert_eq!(locked_cost(&s, MOVING_MODE_DIRECTIONAL), Some(45));
}

#[test]
fn cycle_scenario_toggles_and_reapplies_start() {
    // jeu libre → Progression → Survival → (customs) → jeu libre (touche N)
    // ; chaque bascule réapplique les règles de départ du scénario
    let mut s = GameState::new();
    cycle_scenario(&mut s);
    assert_eq!(s.scenario, ScenarioId::Progression);
    assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
    cycle_scenario(&mut s);
    assert_eq!(s.scenario, ScenarioId::Survival);
    assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
    assert!(s.unlocked_modes.iter().all(|&u| u));
    // on cycle jusqu'à revenir à FreePlay (les customs éventuels sont traversés)
    let total = total_scenario_count();
    for _ in 2..total {
        cycle_scenario(&mut s);
    }
    assert_eq!(s.scenario, ScenarioId::FreePlay);
    assert_eq!(s.resources, Resources::default());
    assert!(s.unlocked_modes.iter().all(|&u| u));
}

#[test]
fn cycle_scenario_back_goes_to_previous() {
    // touche B : inverse de N - on recule jusqu'à revenir à FreePlay
    let mut s = GameState::new();
    let total = total_scenario_count();
    for _ in 0..total {
        cycle_scenario_back(&mut s);
    }
    assert_eq!(s.scenario, ScenarioId::FreePlay);
    assert_eq!(s.resources, Resources::default());
    assert!(s.unlocked_modes.iter().all(|&u| u));
}

#[test]
fn select_scenario_picks_directly_and_applies_start() {
    // touches 1/2/3 : sélection directe (pas de bascule) avec les règles
    // de départ du scénario choisi
    let mut s = GameState::new(); // jeu libre
    select_scenario(&mut s, ScenarioId::Progression);
    assert_eq!(s.scenario, ScenarioId::Progression);
    assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
    select_scenario(&mut s, ScenarioId::Survival);
    assert_eq!(s.scenario, ScenarioId::Survival);
    assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
    select_scenario(&mut s, ScenarioId::FreePlay);
    assert_eq!(s.scenario, ScenarioId::FreePlay);
    assert!(s.unlocked_modes.iter().all(|&u| u));
}

#[test]
fn progression_pays_minerals_to_unlock_modes() {
    // 4 WAYS coûte 30 crédits : payé, débloqué définitivement (la
    // re-sélection est ensuite gratuite) ; sans assez de crédits, refus
    let mut s = progression_state();
    s.resources.credits = 30;
    assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
    assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
    assert_eq!(s.resources.credits, 0);
    assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), None);
    // un mode gratuit (REALISTIC) reste re-sélectionnable sans frais
    assert!(try_select_mode(&mut s, MOVING_MODE_REALISTIC));
    assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
    assert_eq!(s.resources.credits, 0);
    // INERTIAL coûte 15 : pas assez (0) → refus, mode inchangé
    assert!(!try_select_mode(&mut s, MOVING_MODE_INERTIAL));
    assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
    // DIRECTIONAL coûte 45 : pas assez (0) → refus, mode inchangé
    assert!(!try_select_mode(&mut s, MOVING_MODE_DIRECTIONAL));
    assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
    assert!(s.message_queue.contains("NOT ENOUGH CREDITS"));
}

#[test]
fn fuel_is_consumed_while_thrusting_and_blocks_when_empty() {
    let mut s = progression_state();
    let fuel0 = s.resources.fuel;
    s.player.thrusted = -5;
    consume_fuel(&mut s, 1.0);
    assert!(s.resources.fuel < fuel0);
    // réservoir vidé pendant la poussée : message + poussée bloquée
    s.resources.fuel = 0.5;
    consume_fuel(&mut s, 1.0);
    assert_eq!(s.resources.fuel, 0.0);
    assert!(s.message_queue.contains("OUT OF FUEL"));
    assert!(!fuel_available(&s));
}

#[test]
fn ammo_is_consumed_per_shot_and_blocks_when_empty() {
    // seules les armes possédées tirent : au départ seule l'arme de base
    // (coût 0) est équipée - un tir consomme 1 munition de son stock, les
    // autres slots restent à 0 (armes non possédées)
    let mut s = progression_state();
    let mut only_base = [false; WEAPON_SLOTS];
    only_base[0] = true;
    assert_eq!(try_fire(&mut s), only_base);
    assert_eq!(total_ammo(&s), PROGRESSION_SCENARIO.start_ammo - 1);
    s.resources.weapon_ammo[0] = 1;
    assert_eq!(try_fire(&mut s), only_base);
    assert!(s.message_queue.contains("OUT OF AMMO"));
    // chargeur vide : plus aucune arme ne peut tirer (tir bloqué)
    assert_eq!(try_fire(&mut s), [false; WEAPON_SLOTS]);
    assert_eq!(total_ammo(&s), 0);
}

// ─── armes du catalogue (achat, munitions par arme) ────────────────────

#[test]
fn base_weapons_are_owned_and_paid_weapons_are_locked() {
    // au départ en Progression : les armes à coût nul (arme de base) sont
    // équipées et chargées ; les armes payantes sont à acheter (non
    // équipées, munitions à 0) ; jeu libre : seule l'arme 1 équipe le
    // vaisseau
    let s = progression_state();
    for i in 0..weapon_slot_count() {
        let spec = weapon_spec(i);
        assert_eq!(weapon_owned(&s, i), spec.cost == 0, "{}", spec.name);
        assert_eq!(
            s.resources.weapon_ammo[i],
            if spec.cost == 0 {
                PROGRESSION_SCENARIO.start_ammo
            } else {
                0
            },
            "{}",
            spec.name
        );
        assert_eq!(weapon_cost(&s, i), (spec.cost > 0).then_some(spec.cost));
    }
    // jeu libre : le vaisseau n'est équipé que de l'arme 1 (index 0),
    // les autres armes du catalogue ne sont pas possédées
    let f = GameState::new();
    assert!(
        (0..weapon_slot_count()).all(|i| weapon_owned(&f, i) == (i == 0)),
        "seule l'arme 1 équipe le vaisseau en jeu libre"
    );
    // et le tir ne fait partir que l'arme 1 (masque du tir)
    let mut only_first = [false; WEAPON_SLOTS];
    only_first[0] = true;
    assert_eq!(try_fire(&mut f.clone()), only_first);
}

#[test]
fn paid_weapon_is_bought_with_minerals_and_loaded() {
    // une arme payante (ex ARME 2, 30 crédits) : refusée sans assez de
    // crédits, achetée ensuite - équipée, livrée chargée à la capacité
    // courante, puis non rachetable
    let mut s = progression_state();
    let i = (0..weapon_slot_count()).find(|&i| weapon_spec(i).cost > 0).unwrap();
    assert_eq!(
        buy_weapon(&mut s, i),
        WeaponOutcome::Insufficient(weapon_spec(i).cost)
    );
    assert!(s.message_queue.contains("NOT ENOUGH CREDITS"));
    s.resources.credits = 100;
    let cost = weapon_cost(&s, i).unwrap();
    assert_eq!(buy_weapon(&mut s, i), WeaponOutcome::Purchased(cost));
    assert!(weapon_owned(&s, i));
    assert_eq!(s.resources.weapon_ammo[i], ammo_capacity(&s)); // livrée chargée
    assert_eq!(s.resources.credits, 100 - cost);
    assert!(s.message_queue.contains("WEAPON"));
    assert_eq!(buy_weapon(&mut s, i), WeaponOutcome::Owned); // déjà possédée
}

#[test]
fn each_weapon_fires_its_own_ammo() {
    // deux armes possédées : un tir consomme 1 munition de chacune ; une
    // arme à court de munitions ne tire pas, l'autre continue
    let mut s = progression_state();
    let paid = (0..weapon_slot_count()).find(|&i| weapon_spec(i).cost > 0).unwrap();
    s.resources.credits = 1000;
    let cost = weapon_cost(&s, paid).unwrap();
    assert_eq!(buy_weapon(&mut s, paid), WeaponOutcome::Purchased(cost));
    let mut both = [false; WEAPON_SLOTS];
    both[0] = true;
    both[paid] = true;
    assert_eq!(try_fire(&mut s), both);
    assert_eq!(s.resources.weapon_ammo[0], PROGRESSION_SCENARIO.start_ammo - 1);
    assert_eq!(s.resources.weapon_ammo[paid], ammo_capacity(&s) - 1);
    // arme de base à court : seul l'arme payante tire (masque partiel)
    s.resources.weapon_ammo[0] = 0;
    let mut only_paid = [false; WEAPON_SLOTS];
    only_paid[paid] = true;
    assert_eq!(try_fire(&mut s), only_paid);
    assert_eq!(s.resources.weapon_ammo[paid], ammo_capacity(&s) - 2);
    // les deux à court : tir bloqué (aucun slot armé)
    s.resources.weapon_ammo[paid] = 0;
    assert_eq!(try_fire(&mut s), [false; WEAPON_SLOTS]);
}

#[test]
fn supplies_charge_ammo_packs_per_weapon() {
    // la recharge des munitions (magasin, ligne AMMO - indépendante du
    // carburant) facture **par arme possédée**, au paquet de l'arme :
    // 6 paquets × prix ARME 1 + 6 paquets × prix ARME 2 (30 munitions à
    // la capacité de base, paquets de 5) ; le carburant reste intact
    let mut s = progression_state();
    let paid = (0..weapon_slot_count()).find(|&i| weapon_spec(i).cost > 0).unwrap();
    s.resources.credits = 1000;
    let cost = weapon_cost(&s, paid).unwrap();
    assert_eq!(buy_weapon(&mut s, paid), WeaponOutcome::Purchased(cost));
    s.resources.weapon_ammo[0] = 0;
    s.resources.weapon_ammo[paid] = 0;
    let expected = [0, paid]
        .iter()
        .map(|&i| {
            let spec = weapon_spec(i);
            (ammo_capacity(&s) / spec.ammo_pack) * spec.ammo_price
        })
        .sum::<i32>();
    assert_eq!(ammo_refill_cost(&s), expected);
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Purchased(expected));
    assert_eq!(total_ammo(&s), 2 * ammo_capacity(&s));
    assert_eq!(s.resources.fuel, PROGRESSION_SCENARIO.start_fuel); // intact
}

#[test]
fn owned_weapons_persist_and_restore_loaded() {
    // les armes achetées sont enregistrées avec la progression
    // (`prog_weapons`) et restaurées au lancement suivant - **chargées**
    // (les munitions par arme repartent pleines, non persistées) ; les
    // armes de base (coût 0) restent équipées même sans la clé (ancienne
    // sauvegarde)
    let p = temp_path("weapons.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    let paid = (0..weapon_slot_count()).find(|&i| weapon_spec(i).cost > 0).unwrap();
    s.resources.credits = 1000;
    let cost = weapon_cost(&s, paid).unwrap();
    assert_eq!(buy_weapon(&mut s, paid), WeaponOutcome::Purchased(cost));
    s.resources.weapon_ammo[paid] = 3; // non persisté
    save_progression_to(&p, &s).unwrap();
    assert_eq!(get_i32_from(&p, "prog_weapons"), Some(weapons_owned_mask(&s)));

    let mut t = progression_state();
    assert!(!weapon_owned(&t, paid)); // départ neuf
    load_progression_from(&p, &mut t);
    assert!(weapon_owned(&t, paid));
    assert_eq!(t.resources.weapon_ammo[paid], ammo_capacity(&t)); // repart chargée
    assert_eq!(t.resources.credits, 1000 - cost);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn radar_on_by_default_outside_economy_and_buyable_with_credits() {
    // jeu libre : le radar est allumé par défaut (comportement
    // historique), rien à acheter (pas de prix)
    let free = GameState::new();
    assert!(has_radar(&free));
    assert_eq!(radar_price(&free), None);
    assert_eq!(buy_radar(&mut GameState::new()), RadarOutcome::Owned);

    // Progression : radar éteint par défaut, prix affiché (remise
    // CADET = 0), achat contre crédits
    let mut s = progression_state();
    assert!(!has_radar(&s));
    assert_eq!(radar_price(&s), Some((20, 20)));
    assert_eq!(radar_cost(&s), Some(20));
    // pas assez de crédits : refusé, radar toujours éteint
    s.resources.credits = 5;
    assert_eq!(buy_radar(&mut s), RadarOutcome::Insufficient(20));
    assert!(!has_radar(&s));
    // assez de crédits : acheté, crédits déduits, radar allumé
    s.resources.credits = 20;
    assert_eq!(buy_radar(&mut s), RadarOutcome::Purchased(20));
    assert!(has_radar(&s));
    assert_eq!(s.resources.credits, 0);
    // déjà possédé : plus de prix, plus d'achat possible
    assert_eq!(radar_price(&s), None);
    assert_eq!(buy_radar(&mut s), RadarOutcome::Owned);
}

#[test]
fn radar_persists_and_resets_with_progression() {
    // le radar acheté est enregistré (`prog_radar`) et restauré au
    // lancement suivant ; RESET PROGRESSION l'éteint à nouveau
    let p = temp_path("radar.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.resources.credits = 100;
    assert_eq!(buy_radar(&mut s), RadarOutcome::Purchased(20));
    save_progression_to(&p, &s).unwrap();
    assert_eq!(get_i32_from(&p, "prog_radar"), Some(1));

    // un départ neuf a le radar éteint ; rechargé, il est allumé
    let mut fresh = progression_state();
    assert!(!has_radar(&fresh));
    load_progression_from(&p, &mut fresh);
    assert!(has_radar(&fresh));
    assert_eq!(fresh.resources.credits, 80);

    // RESET : la clé est supprimée et le radar repart éteint
    reset_progression_from(&p, &mut fresh);
    assert_eq!(get_i32_from(&p, "prog_radar"), None);
    assert!(!has_radar(&fresh));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn reputation_rewards_destructions_and_precision() {
    // précision 100 % : gain = 1 × (1 + 2×1,0) = 3 ; précision 50 % :
    // gain = 1 × (1 + 2×0,5) = 2 ; chaque destruction ajoute
    let mut precise = progression_state();
    precise.bullets_fired = 10;
    precise.bullets_lost = 0;
    on_meteor_destroyed(&mut precise);
    assert_eq!(precise.resources.reputation, 3.0);
    on_meteor_destroyed(&mut precise);
    assert!(precise.resources.reputation > 3.0);

    let mut imprecise = progression_state();
    imprecise.bullets_fired = 10;
    imprecise.bullets_lost = 5;
    on_meteor_destroyed(&mut imprecise);
    assert!((imprecise.resources.reputation - 2.0).abs() < 1e-9);
    assert!(precise.resources.reputation > imprecise.resources.reputation);
}

#[test]
fn reputation_ranks_unlock_at_thresholds() {
    // les paliers de réputation sont débloqués aux seuils de la table :
    // CADET 0, PILOT 10, VETERAN 25, ACE 50 - le rang courant est le plus
    // haut palier franchi
    assert_eq!(rank_at(PROGRESSION_RANKS, 0.0).map(|r| r.name), Some("CADET"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 9.9).map(|r| r.name), Some("CADET"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 10.0).map(|r| r.name), Some("PILOT"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 24.9).map(|r| r.name), Some("PILOT"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 25.0).map(|r| r.name), Some("VETERAN"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 50.0).map(|r| r.name), Some("ACE"));
    assert_eq!(rank_at(PROGRESSION_RANKS, 999.0).map(|r| r.name), Some("ACE"));
    // jeu libre : pas de table de rangs → aucun rang affiché
    assert_eq!(current_rank(&GameState::new()), None);
    assert_eq!(current_rank(&progression_state()), Some("CADET"));
}

#[test]
fn rank_up_is_announced_when_a_tier_is_crossed() {
    // une destruction qui franchit le seuil de 10 annonce le palier
    // suivant (« RANK UP: PILOT ») ; rester dans le même palier ne dit
    // rien
    let mut s = progression_state();
    s.bullets_fired = 10;
    s.bullets_lost = 0; // précision 100 % : gain de 3 par destruction
    s.resources.reputation = 8.0;
    on_meteor_destroyed(&mut s); // 8 → 11 : franchit 10
    assert_eq!(s.resources.reputation, 11.0);
    assert!(s.message_queue.contains("RANK UP: PILOT"));

    let mut same = progression_state();
    same.bullets_fired = 10;
    same.bullets_lost = 0;
    same.resources.reputation = 5.0;
    on_meteor_destroyed(&mut same); // 5 → 8 : reste CADET
    assert!(!same.message_queue.contains("RANK UP"));
}

// ─── survie (vies, bouclier, dégâts) ───────────────────────────────────

#[test]
fn survival_starts_with_lives_shield_and_no_economy() {
    // départ Survival : vies et bouclier pleins, pas d'économie (fuel et
    // munitions illimités), tous les modes débloqués, pas de rangs
    let s = survival_state();
    assert!(has_survival(&s));
    assert!(!has_economy(&s));
    assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
    assert!(s.unlocked_modes.iter().all(|&u| u));
    assert!(fuel_available(&s));
    assert!(try_fire(&mut s.clone()).iter().any(|&f| f)); // pas de consommation
    assert_eq!(current_rank(&s), None);
}

#[test]
fn shield_absorbs_impacts_then_ship_is_destroyed() {
    // chaque impact vide le bouclier d'un point ; le suivant (bouclier à
    // 0) détruit le vaisseau : une vie perdue, bouclier rechargé, message
    let mut s = survival_state();
    for _ in 0..3 {
        assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Absorbed);
    }
    assert_eq!(s.resources.lives, 3);
    assert_eq!(s.resources.shield, 0.0);
    assert!(!s.game_over);

    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Destroyed(2));
    assert_eq!(s.resources.lives, 2);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
    assert!(s.message_queue.contains("SHIP DESTROYED - 2 LIVES LEFT"));
    // le respawn accorde une invulnérabilité temporaire
    assert_eq!(s.invulnerable, SURVIVAL_SCENARIO.respawn_invulnerability);
}

#[test]
fn respawn_invulnerability_absorbs_impacts_without_shield_loss() {
    // pendant la fenêtre d'invulnérabilité, les impacts sont absorbés
    // sans toucher au bouclier ni aux vies ; à l'échéance, ils entament
    // le bouclier normalement
    let mut s = survival_state();
    s.invulnerable = 2.0;
    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Absorbed);
    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Absorbed);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity); // intact
    assert_eq!(s.resources.lives, 3);

    s.invulnerable = 0.0; // fin de la fenêtre (décomptée par `game.rs`)
    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Absorbed);
    assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity - 1.0);
}

#[test]
fn classic_scenarios_have_no_invulnerability() {
    // hors Survival, aucun répit : pas d'invulnérabilité au départ
    // (apply_start) ni après un impact
    let mut s = GameState::new(); // jeu libre
    apply_start(&mut s);
    assert_eq!(s.invulnerable, 0.0);
    let p = progression_state();
    assert_eq!(p.invulnerable, 0.0);
}

#[test]
fn damage_multiplier_scales_impacts() {
    // le multiplicateur de dégâts aggrave les impacts : à ×2, le bouclier
    // (2 points) est vidé par un seul impact au lieu de deux
    let s = Scenario {
        damage_multiplier: 2.0,
        ..SURVIVAL_SCENARIO
    };
    assert_eq!(scaled_impact(s, 1.0), 2.0);
    assert_eq!(scaled_impact(SURVIVAL_SCENARIO, 1.0), 1.0);
}

#[test]
fn last_life_lost_is_game_over() {
    // dernière vie perdue : partie terminée (game_over), message unique,
    // les impacts suivants ne changent plus rien
    let mut s = survival_state();
    s.resources.lives = 1;
    s.resources.shield = 0.0;
    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::GameOver);
    assert!(s.game_over);
    assert_eq!(s.resources.lives, 0);
    assert_eq!(s.resources.shield, 0.0);
    assert!(s.message_queue.contains("GAME OVER"));
    let queue = s.message_queue.clone();
    assert_eq!(player_hit(&mut s, 1.0), PlayerHit::GameOver);
    assert_eq!(s.message_queue, queue); // pas de nouveau message
}

#[test]
fn classic_scenarios_ignore_survival_impacts() {
    // jeu libre et Progression n'ont ni vies ni bouclier : un impact ne
    // change rien (la coque est gérée par la collision classique)
    for scenario_id in [ScenarioId::FreePlay, ScenarioId::Progression] {
        let mut s = GameState::new();
        s.scenario = scenario_id;
        apply_start(&mut s);
        let before = s.resources;
        assert_eq!(player_hit(&mut s, 1.0), PlayerHit::Absorbed);
        assert_eq!(s.resources, before); // aucun changement
    }
}

#[test]
fn game_over_is_reset_by_apply_start() {
    // une nouvelle partie (apply_start) efface le game over
    let mut s = survival_state();
    s.game_over = true;
    apply_start(&mut s);
    assert!(!s.game_over);
    assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
}

#[test]
fn cargo_unload_converts_gems_to_minerals() {
    // GOLD vaut 5, IRON 3, WATER 2 : la soute déchargée est convertie ;
    // en jeu libre, aucun minerai n'est gagné
    let mut s = progression_state();
    let mut elements = default_elements();
    elements[1].count = 2; // GOLD ×2 = 10
    elements[2].count = 1; // IRON ×1 = 3
    elements[3].count = 1; // WATER ×1 = 2
    unload_cargo(&mut s, &elements);
    assert_eq!(s.resources.credits, 15);
    assert!(s.message_queue.contains("+15 CREDITS"));

    let mut f = GameState::new();
    unload_cargo(&mut f, &elements);
    assert_eq!(f.resources.credits, 0);
    assert!(f.message_queue.is_empty());
}

#[test]
fn cargo_unload_grants_reputation_and_rank_up() {
    // le commerce est récompensé : chaque minerai déchargé rapporte de la
    // réputation (0,1 en Progression) - 100 crédits → +10 → le seuil
    // PILOT (10) est franchi, « RANK UP: PILOT » est annoncé
    let mut s = progression_state();
    let mut elements = default_elements();
    elements[1].count = 20; // GOLD ×20 = 100 crédits
    unload_cargo(&mut s, &elements);
    assert_eq!(s.resources.credits, 100);
    assert!(
        (s.resources.reputation - 10.0).abs() < 1e-9,
        "réputation {}",
        s.resources.reputation
    );
    assert!(s.message_queue.contains("RANK UP: PILOT"));

    // en jeu libre, pas d.économie : ni crédits ni réputation
    let mut f = GameState::new();
    unload_cargo(&mut f, &elements);
    assert_eq!(f.resources.reputation, 0.0);
}

#[test]
fn fuel_and_ammo_are_purchased_independently() {
    // carburant et munitions s'achètent **indépendamment** au magasin :
    // le plein de carburant (5 pas × 1 = 5) ne touche pas aux munitions,
    // et le rechargement des munitions (4 paquets × 1 = 4) pas au
    // carburant - chacun est facturé à part
    let mut s = progression_state();
    s.resources.credits = 100;
    s.resources.fuel = 50.0;
    s.resources.weapon_ammo[0] = 10;
    assert_eq!(fuel_refill_cost(&s), 5); // 50 manquants / 10 par pas
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Purchased(5));
    assert_eq!(s.resources.fuel, fuel_capacity(&s)); // 100 (base)
    assert_eq!(s.resources.credits, 95);
    assert_eq!(s.resources.weapon_ammo[0], 10); // munitions intactes
    assert!(s.message_queue.contains("FUEL PURCHASED"));

    assert_eq!(ammo_refill_cost(&s), 4); // 20 manquants / 5 par paquet
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Purchased(4));
    assert_eq!(s.resources.credits, 91);
    assert_eq!(total_ammo(&s), ammo_capacity(&s)); // 30 (base)
    assert_eq!(s.resources.fuel, 100.0); // carburant intact
    assert!(s.message_queue.contains("AMMO PURCHASED"));
}

#[test]
fn supplies_are_refused_without_enough_credits() {
    // carburant et munitions refusés séparément avec seulement 2 crédits
    // (plein de carburant 10, recharge de munitions 6) : réservoirs
    // inchangés, message envoyé une seule fois par coût (pas de
    // répétition à chaque clic)
    let mut s = progression_state();
    s.resources.credits = 2;
    s.resources.fuel = 0.0;
    s.resources.weapon_ammo[0] = 0;
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Insufficient(10));
    assert_eq!(s.resources.fuel, 0.0);
    assert!(s.message_queue.contains("NOT ENOUGH CREDITS FOR FUEL"));
    let queue = s.message_queue.clone();
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Insufficient(10));
    assert_eq!(s.message_queue, queue); // pas de nouveau message
    // munitions : coût différent (6) → message distinct
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Insufficient(6));
    assert!(s.message_queue.contains("NOT ENOUGH CREDITS FOR AMMO"));
    // crédits obtenus : chaque achat est accepté et le manque effacé
    // (assez pour les deux : plein de carburant 10 + recharge 6)
    s.resources.credits = 16;
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Purchased(10));
    assert_eq!(s.supplies_shortage_cost, 0);
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Purchased(6));
}

#[test]
fn full_tank_costs_nothing() {
    // réservoir et chargeurs pleins : rien à payer, aucun message
    let mut s = progression_state();
    assert_eq!(fuel_refill_cost(&s), 0);
    assert_eq!(ammo_refill_cost(&s), 0);
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Full);
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Full);
    assert!(s.message_queue.is_empty());
}

#[test]
fn supplies_can_be_bought_by_unit() {
    // le ravitaillement s'achète **à la quantité** (ligne FUEL / AMMO du
    // magasin, curseur) : tout achat paie au moins un paquet de la
    // ressource (10 carburant, le paquet de l'arme pour les munitions) -
    // même avec peu de crédits, on peut toujours s.en sortir (ex 3
    // crédits → 30 carburant) sans devoir financer un plein complet
    let mut s = progression_state();
    s.resources.credits = 3;
    s.resources.fuel = 0.0;
    s.resources.weapon_ammo[0] = 0;
    // carburant : 1 minerai par paquet de 10 - 30 unités = 3 paquets,
    // 5 unités = 1 paquet (minimum), 0 = rien ; le nombre de paquets
    // affiché suit la facturation
    assert_eq!(fuel_pack_count(&s, 30.0), 3);
    assert_eq!(fuel_pack_count(&s, 5.0), 1);
    assert_eq!(fuel_qty_cost(&s, 30.0), 3);
    assert_eq!(fuel_qty_cost(&s, 5.0), 1);
    assert_eq!(fuel_qty_cost(&s, 0.0), 0);
    assert_eq!(buy_fuel_qty(&mut s, 30.0), SupplyOutcome::Purchased(3));
    assert_eq!(s.resources.fuel, 30.0);
    assert_eq!(s.resources.credits, 0);
    assert!(s.message_queue.contains("FUEL PURCHASED"));
    // munitions : paquets de 5 à 1 minerai (ARME 1) - 7 unités = 2
    // paquets, et le carburant acheté reste intact
    s.resources.credits = 5;
    assert_eq!(ammo_pack_count(&s, 0, 7), 2);
    assert_eq!(ammo_qty_cost(&s, 0, 7), 2);
    assert_eq!(buy_ammo_qty(&mut s, 0, 7), SupplyOutcome::Purchased(2));
    assert_eq!(s.resources.weapon_ammo[0], 7);
    assert_eq!(s.resources.credits, 3);
    assert_eq!(s.resources.fuel, 30.0);
    assert!(s.message_queue.contains("AMMO PURCHASED"));
    // quantité nulle : rien à payer ; quantité au-delà du manque : bornée
    assert_eq!(buy_fuel_qty(&mut s, 0.0), SupplyOutcome::Full);
    assert_eq!(buy_ammo_qty(&mut s, 0, 0), SupplyOutcome::Full);
    s.resources.credits = 10;
    assert_eq!(buy_fuel_qty(&mut s, 999.0), SupplyOutcome::Purchased(7)); // 70 manquants
    assert_eq!(s.resources.fuel, fuel_capacity(&s));
    assert_eq!(s.resources.credits, 3);
}

#[test]
fn shop_sliders_clamp_to_affordable() {
    // les curseurs du magasin sont bornés au **manque** des réservoirs et
    // à ce que les crédits permettent : jamais une quantité dont le coût
    // dépasserait les crédits - 4 crédits → 40 carburant (4 paquets de
    // 10), 3 crédits → 15 munitions (3 paquets de 5)
    let mut s = progression_state();
    s.resources.credits = 4;
    s.resources.fuel = 0.0;
    assert_eq!(affordable_fuel_qty(&s), 40.0);
    s.shop_fuel_qty = 100.0; // au-delà de l'achetable
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 40.0);
    // le curseur ne dépasse jamais le manque du réservoir
    s.resources.credits = 100;
    s.resources.fuel = 90.0;
    assert_eq!(affordable_fuel_qty(&s), 10.0);
    s.shop_fuel_qty = 50.0;
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 10.0);
    // munitions : paquets de 5 → 3 crédits = 15 unités
    s.resources.credits = 3;
    s.resources.fuel = 0.0;
    s.resources.weapon_ammo[0] = 0;
    assert_eq!(affordable_ammo_qty(&s, 0), 15);
    s.shop_ammo_qty[0] = 30.0;
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_ammo_qty[0], 15.0);
    // sans crédits : plus rien d.achetable, curseurs à zéro
    s.resources.credits = 0;
    assert_eq!(affordable_fuel_qty(&s), 0.0);
    assert_eq!(affordable_ammo_qty(&s, 0), 0);
    s.shop_fuel_qty = 20.0;
    s.shop_ammo_qty[0] = 10.0;
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 0.0);
    assert_eq!(s.shop_ammo_qty[0], 0.0);
}

#[test]
fn shop_sliders_snap_to_packs() {
    // l'aimantation aux paquets : une quantité glissée entre deux paquets
    // retombe sur le multiple le plus proche (jamais un paquet payé sans
    // ses unités) - sauf le plein du réservoir, qui reste atteignable en
    // bout de piste même si le manque n'est pas multiple du paquet (le
    // dernier paquet est alors pris en entier, aucune unité perdue)
    assert_eq!(snap_to_pack(24.0, 10.0, 100.0), 20.0);
    assert_eq!(snap_to_pack(26.0, 10.0, 100.0), 30.0);
    assert_eq!(snap_to_pack(7.0, 5.0, 30.0), 5.0);
    assert_eq!(snap_to_pack(0.0, 10.0, 100.0), 0.0);
    assert_eq!(snap_to_pack(84.0, 10.0, 85.0), 80.0); // pas encore le bout
    assert_eq!(snap_to_pack(85.0, 10.0, 85.0), 85.0); // le plein reste valide

    // curseur FUEL : 27 (entre 2 et 3 paquets) → aimanté à 30
    let mut s = progression_state();
    s.resources.credits = 5;
    s.resources.fuel = 50.0; // manque 50 → paquets de 10
    s.shop_fuel_qty = 27.0;
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 30.0);
    // manque non multiple (85) : les paquets s'aimantent, le plein (85,
    // 9 paquets - le dernier partiel pris en entier) reste atteignable
    s.resources.credits = 1000;
    s.resources.fuel = 15.0;
    s.shop_fuel_qty = 84.0;
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 80.0);
    s.shop_fuel_qty = 85.0; // bout de la piste
    clamp_shop_quantities(&mut s);
    assert_eq!(s.shop_fuel_qty, 85.0);
    assert_eq!(fuel_qty_cost(&s, 85.0), 9);
}

// ─── persistance de la progression ─────────────────────────────────────

#[test]
fn progression_save_and_restore_round_trips() {
    // une partie Progression (crédits, modes payés, réputation) est
    // enregistrée puis restaurée sur un départ neuf du scénario - les
    // réservoirs, eux, repartent pleins (non persistés)
    let p = temp_path("roundtrip.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.resources.credits = 42;
    s.resources.reputation = 3.5;
    s.unlocked_modes = [true, true, false, true];
    save_progression_to(&p, &s).unwrap();

    let mut fresh = progression_state();
    assert_eq!(fresh.resources.credits, 0); // départ neuf
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.credits, 42);
    assert_eq!(fresh.resources.reputation, 3.5);
    assert_eq!(fresh.unlocked_modes, [true, true, false, true]);
    assert_eq!(fresh.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
    assert_eq!(total_ammo(&fresh), PROGRESSION_SCENARIO.start_ammo);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn reset_progression_clears_saved_progression() {
    // RESET PROGRESSION : une progression (crédits, modes payés,
    // réputation, extensions, mode choisi) est remise à zéro - les clés
    // `prog_*` et `moving_mode` du fichier sont supprimées et l'état
    // repart sur les règles de départ du scénario (REALISTIC seul
    // débloqué, réservoirs pleins)
    let p = temp_path("resetprog.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.resources.credits = 77;
    s.resources.reputation = 50.0;
    s.resources.fuel_level = 2;
    s.unlocked_modes = [true, true, true, true];
    s.moving_mode = MOVING_MODE_4_WAYS;
    save_progression_to(&p, &s).unwrap();
    set_i32_to(&p, "moving_mode", MOVING_MODE_4_WAYS).unwrap();

    reset_progression_from(&p, &mut s);
    // état remis au départ : crédits 0, réputation nulle, extensions 0,
    // seul REALISTIC (gratuit) débloqué, mode de départ
    assert_eq!(s.resources.credits, 0);
    assert_eq!(s.resources.reputation, 0.0);
    assert_eq!(s.resources.fuel_level, 0);
    assert_eq!(s.unlocked_modes, [false, false, false, true]);
    assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
    assert_eq!(s.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
    assert_eq!(total_ammo(&s), PROGRESSION_SCENARIO.start_ammo); // arme de base

    // clés de progression supprimées du fichier (mode compris) : un
    // rechargement sur un départ neuf ne retrouve aucune progression
    assert_eq!(get_i32_from(&p, "prog_minerals"), None);
    assert_eq!(get_i32_from(&p, "prog_modes"), None);
    assert_eq!(get_i32_from(&p, "prog_reputation"), None);
    assert_eq!(get_i32_from(&p, "prog_up_fuel"), None);
    assert_eq!(get_i32_from(&p, "prog_weapons"), None);
    assert_eq!(get_i32_from(&p, "moving_mode"), None);
    let mut fresh = progression_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.credits, 0);
    assert_eq!(fresh.unlocked_modes, [false, false, false, true]);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn objective_counters_are_persisted_and_restored() {
    // l'avancement des conditions d'objectifs (météores détruits,
    // accostages, tirs, temps de survie) est sauvegardé avec la
    // progression et restauré : après une sortie en cours de partie, la
    // phase du scénario reprend là où elle s'est arrêtée (pas de remise à
    // zéro des compteurs). Test sans objet si aucun scénario custom avec
    // objectifs n'est chargé (dossier scenarios/ absent).
    let Some(idx) = crate::scenario_loader::loaded_scenarios()
        .iter()
        .position(|ls| !ls.data.json.objectives.is_empty())
    else {
        return;
    };
    let p = temp_path("objcounters.cfg");
    let _ = std::fs::remove_file(&p);

    let mut s = GameState::new();
    s.scenario = ScenarioId::Custom(idx);
    apply_start(&mut s); // initialise le tracker d'objectifs
    assert!(s.objective_tracker.has_objectives());
    s.meteors_destroyed = 37;
    s.docking_count = 2;
    s.bullets_fired = 60;
    s.bullets_lost = 8;
    // objectif SurviveTime présent ? y accumuler du temps de survie
    let survive_id = s
        .objective_tracker
        .objectives
        .iter()
        .find(|o| o.condition.condition_type == "SurviveTime")
        .map(|o| o.id.clone());
    if let Some(id) = &survive_id {
        if let Some(obj) = s.objective_tracker.objectives.iter_mut().find(|o| o.id == *id) {
            obj.active_time = 12.5;
        }
    }
    save_progression_to(&p, &s).unwrap();

    // nouveau lancement : départ frais (compteurs à zéro) puis
    // restauration de la progression
    let mut fresh = GameState::new();
    fresh.scenario = ScenarioId::Custom(idx);
    apply_start(&mut fresh);
    assert_eq!(fresh.meteors_destroyed, 0);
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.meteors_destroyed, 37);
    assert_eq!(fresh.docking_count, 2);
    assert_eq!(fresh.bullets_fired, 60);
    assert_eq!(fresh.bullets_lost, 8);
    if let Some(id) = &survive_id {
        let obj = fresh
            .objective_tracker
            .objectives
            .iter()
            .find(|o| o.id == *id)
            .expect("objectif SurviveTime restauré");
        assert!((obj.active_time - 12.5).abs() < 0.05);
    }
    // l'avancement seul (sans objectif complété) constitue une
    // sauvegarde réelle : le lancement propose de poursuivre
    assert!(has_saved_progression_from(&p, &fresh));

    // RESET PROGRESSION : compteurs supprimés et remis à zéro
    reset_progression_from(&p, &mut fresh);
    assert_eq!(fresh.meteors_destroyed, 0);
    assert_eq!(fresh.docking_count, 0);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn refuel_spent_minerals_are_persisted() {
    // un ravitaillement (carburant + munitions achetés au magasin)
    // déduit des crédits : la sauvegarde doit conserver la valeur
    // déduite - pas de ravitaillement gratuit au lancement suivant (le
    // jeu écrit la progression après chaque achat)
    let p = temp_path("refuel.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.resources.credits = 100;
    s.resources.fuel = 50.0;
    s.resources.weapon_ammo[0] = 10;
    let cost_fuel = match purchase_fuel(&mut s) {
        SupplyOutcome::Purchased(c) => c,
        _ => panic!("carburant attendu"),
    };
    let cost_ammo = match purchase_ammo(&mut s) {
        SupplyOutcome::Purchased(c) => c,
        _ => panic!("munitions attendues"),
    };
    let cost = cost_fuel + cost_ammo;
    assert_eq!(s.resources.credits, 100 - cost);
    save_progression_to(&p, &s).unwrap();

    let mut fresh = progression_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.credits, 100 - cost); // dépense conservée
    let _ = std::fs::remove_file(&p);
}

#[test]
fn free_play_save_does_not_clobber_progression() {
    // enregistrer une partie libre ne réécrit pas les clés `prog_*` : la
    // sauvegarde d'un scénario à économie survit (seul `scenario` change)
    let p = temp_path("freeplay.cfg");
    let _ = std::fs::remove_file(&p);
    let mut prog = progression_state();
    prog.resources.credits = 77;
    save_progression_to(&p, &prog).unwrap();

    let free = GameState::new(); // jeu libre
    save_progression_to(&p, &free).unwrap();
    assert_eq!(get_i32_from(&p, "scenario"), Some(0));
    assert_eq!(get_i32_from(&p, "prog_credits"), Some(77)); // conservés
    // seul REALISTIC est débloqué au départ (INERTIAL est payant)
    assert_eq!(get_i32_from(&p, "prog_modes"), Some(0b1000));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn load_legacy_prog_minerals_key_as_credits() {
    // compatibilité ascendante : une sauvegarde créée avant le renommage
    // (clé `prog_minerals`) doit charger ses crédits malgré tout
    let p = temp_path("legacycredits.cfg");
    let _ = std::fs::remove_file(&p);
    set_i32_to(&p, "scenario", 1).unwrap();
    set_i32_to(&p, "prog_minerals", 55).unwrap(); // ancienne clé seule

    let mut fresh = progression_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.credits, 55);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn has_saved_progression_detects_only_real_saves() {
    // une sauvegarde **réelle** (progression différente du départ) est
    // détectée ; un scénario seulement sélectionné à l'écran titre (les
    // clés `prog_*` sont écrites aux valeurs du départ) ne l'est pas -
    // pas de choix « poursuivre / repartir » inutile au lancement
    let p = temp_path("hassave.cfg");
    let _ = std::fs::remove_file(&p);
    // Progression jamais jouée : pas de sauvegarde
    let fresh = progression_state();
    assert!(!has_saved_progression_from(&p, &fresh));
    // même après un cycle écran titre (select + save_progression) : les
    // clés existent mais aux valeurs du départ - toujours pas de sauvegarde
    save_progression_to(&p, &fresh).unwrap();
    assert!(!has_saved_progression_from(&p, &fresh));
    // joué (crédits gagnés) : la sauvegarde est réelle
    let mut played = fresh.clone();
    played.resources.credits = 42;
    save_progression_to(&p, &played).unwrap();
    assert!(has_saved_progression_from(&p, &played));
    // détectée aussi sur l'état restauré (ex écran titre après relance)
    let mut restored = progression_state();
    load_progression_from(&p, &mut restored);
    assert!(has_saved_progression_from(&p, &restored));
    // jeu libre : jamais de sauvegarde
    assert!(!has_saved_progression_from(&p, &GameState::new()));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn has_saved_progression_detects_survival_damage() {
    // Survival : une vie perdue ou un bouclier entamé = sauvegarde réelle ;
    // départ complet (vies et bouclier pleins) = pas de sauvegarde
    let p = temp_path("hassavesurv.cfg");
    let _ = std::fs::remove_file(&p);
    let fresh = survival_state();
    assert!(!has_saved_progression_from(&p, &fresh));
    let mut played = fresh.clone();
    played.resources.shield = 2.5; // impact subi
    save_progression_to(&p, &played).unwrap();
    assert!(has_saved_progression_from(&p, &played));
    let mut restored = survival_state();
    load_progression_from(&p, &mut restored);
    assert!(has_saved_progression_from(&p, &restored));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn load_restores_paid_moving_mode_only() {
    // un mode payé puis sélectionné (clé `moving_mode`) est restauré à la
    // reprise ; un mode jamais débloqué n'est pas imposé (départ du
    // scénario : REALISTIC)
    let p = temp_path("mode.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.unlocked_modes = [true, true, false, true];
    save_progression_to(&p, &s).unwrap();
    set_i32_to(&p, "moving_mode", MOVING_MODE_4_WAYS).unwrap();

    let mut fresh = progression_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.moving_mode, MOVING_MODE_4_WAYS);

    // mode non débloqué par la sauvegarde : pas restauré
    set_i32_to(&p, "moving_mode", MOVING_MODE_DIRECTIONAL).unwrap();
    let mut fresh2 = progression_state();
    fresh2.moving_mode = MOVING_MODE_INERTIAL;
    load_progression_from(&p, &mut fresh2);
    assert_eq!(fresh2.moving_mode, MOVING_MODE_INERTIAL);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn load_does_nothing_in_free_play() {
    // la progression n'est restaurée que pour un scénario à économie : en
    // jeu libre, l'état de départ est conservé (tous les modes)
    let p = temp_path("freeload.cfg");
    let _ = std::fs::remove_file(&p);
    let mut prog = progression_state();
    prog.resources.credits = 55;
    save_progression_to(&p, &prog).unwrap();

    let mut free = GameState::new();
    load_progression_from(&p, &mut free);
    assert_eq!(free.resources.credits, 0);
    assert!(free.unlocked_modes.iter().all(|&u| u));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn load_scenario_key_is_bounded() {
    // la clé `scenario` valide est lue (0 = jeu libre, 1 = Progression,
    // 2 = Survival) ; absente ou hors bornes → None (jeu libre par défaut)
    let p = temp_path("scenario.cfg");
    let _ = std::fs::remove_file(&p);
    std::fs::write(&p, "scenario=1\n").unwrap();
    assert_eq!(load_scenario_from(&p), Some(ScenarioId::Progression));
    std::fs::write(&p, "scenario=2\n").unwrap();
    assert_eq!(load_scenario_from(&p), Some(ScenarioId::Survival));
    std::fs::write(&p, "scenario=0\n").unwrap();
    assert_eq!(load_scenario_from(&p), Some(ScenarioId::FreePlay));
    std::fs::write(&p, "scenario=9\n").unwrap();
    assert_eq!(load_scenario_from(&p), None);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn survival_save_does_not_clobber_progression() {
    // enregistrer une partie Survival écrit ses clés (vies, bouclier)
    // sans réécrire celles de Progression (crédits, modes, réputation)
    let p = temp_path("survival.cfg");
    let _ = std::fs::remove_file(&p);
    let mut prog = progression_state();
    prog.resources.credits = 33;
    save_progression_to(&p, &prog).unwrap();

    let mut surv = survival_state();
    surv.resources.lives = 2;
    surv.resources.shield = 1.5;
    save_progression_to(&p, &surv).unwrap();
    assert_eq!(get_i32_from(&p, "scenario"), Some(2));
    assert_eq!(get_i32_from(&p, "prog_lives"), Some(2));
    assert_eq!(get_i32_from(&p, "prog_shield"), Some(15)); // 1,5 × 10
    assert_eq!(get_i32_from(&p, "prog_credits"), Some(33)); // conservés
    let _ = std::fs::remove_file(&p);
}

#[test]
fn progression_save_does_not_clobber_survival_save() {
    // l'inverse : une sauvegarde Progression laisse les clés Survival
    // (vies, bouclier) intactes
    let p = temp_path("progsurv.cfg");
    let _ = std::fs::remove_file(&p);
    let mut surv = survival_state();
    surv.resources.lives = 1;
    surv.resources.shield = 0.5;
    save_progression_to(&p, &surv).unwrap();

    let prog = progression_state();
    save_progression_to(&p, &prog).unwrap();
    assert_eq!(get_i32_from(&p, "scenario"), Some(1));
    assert_eq!(get_i32_from(&p, "prog_lives"), Some(1)); // conservées
    assert_eq!(get_i32_from(&p, "prog_shield"), Some(5));
    let _ = std::fs::remove_file(&p);
}

#[test]
fn survival_progression_save_and_restore_round_trips() {
    // une partie Survival (vies, bouclier) enregistrée est restaurée sur
    // un départ neuf du scénario
    let p = temp_path("surv_roundtrip.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = survival_state();
    s.resources.lives = 2;
    s.resources.shield = 1.5;
    save_progression_to(&p, &s).unwrap();

    let mut fresh = survival_state();
    assert_eq!(fresh.resources.lives, SURVIVAL_SCENARIO.lives); // départ neuf
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.lives, 2);
    assert_eq!(fresh.resources.shield, 1.5);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn load_clamps_survival_save_to_scenario_capacities() {
    // une sauvegarde hors bornes (fichier édité à la main) est bornée par
    // les capacités du scénario : jamais plus de 3 vies ni de 3.0 de
    // bouclier
    let p = temp_path("surv_clamp.cfg");
    let _ = std::fs::remove_file(&p);
    std::fs::write(&p, "prog_lives=9\nprog_shield=99\n").unwrap();
    let mut fresh = survival_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.lives, SURVIVAL_SCENARIO.lives);
    assert_eq!(fresh.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn game_over_save_restores_fresh_lives() {
    // sauvegarde à 0 vie (partie terminée puis quittée) : la partie
    // suivante repart avec les vies de départ, pas dans l'état game over
    let p = temp_path("surv_gameover.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = survival_state();
    s.resources.lives = 0;
    s.resources.shield = 0.0;
    save_progression_to(&p, &s).unwrap();

    let mut fresh = survival_state();
    load_progression_from(&p, &mut fresh);
    assert_eq!(fresh.resources.lives, SURVIVAL_SCENARIO.lives); // départ
    assert!(!fresh.game_over);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn upgrade_capacity_sums_base_and_bonuses() {
    // capacité d'une ligne = base + bonus des extensions achetées,
    // bornée au nombre d'extensions ; la prochaine extension est celle du
    // niveau courant, `None` au max
    assert_eq!(track_capacity(&FUEL_UPGRADE_TRACK, 0), 100);
    assert_eq!(track_capacity(&FUEL_UPGRADE_TRACK, 1), 130);
    assert_eq!(track_capacity(&FUEL_UPGRADE_TRACK, 3), 220);
    assert_eq!(track_capacity(&FUEL_UPGRADE_TRACK, 99), 220); // borné
    assert_eq!(track_capacity(&AMMO_UPGRADE_TRACK, 2), 55);
    assert_eq!(track_capacity(&CARGO_UPGRADE_TRACK, 2), 10); // 5 + 2 + 3
    assert_eq!(track_capacity(&EMPTY_UPGRADE_TRACK, 3), 0);
    assert_eq!(next_upgrade(&FUEL_UPGRADE_TRACK, 0).unwrap().cost, 10);
    assert_eq!(next_upgrade(&FUEL_UPGRADE_TRACK, 3), None); // max
}

#[test]
fn buy_upgrade_purchases_and_extends_capacity() {
    // l'extension de réservoir paie en crédits, monte le niveau et le
    // réservoir repart plein à la nouvelle capacité ; la soute s'agrandit
    // immédiatement
    let mut s = progression_state();
    s.resources.credits = 50;
    assert_eq!(fuel_capacity(&s), 100.0);
    assert_eq!(
        buy_upgrade(&mut s, UpgradeTrackId::Fuel),
        UpgradeOutcome::Purchased(10)
    );
    assert_eq!(s.resources.fuel_level, 1);
    assert_eq!(s.resources.credits, 40);
    assert_eq!(fuel_capacity(&s), 130.0);
    assert_eq!(s.resources.fuel, 130.0); // plein à la nouvelle capacité
    assert!(s.message_queue.contains("PURCHASED"));

    assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Cargo), UpgradeOutcome::Purchased(20));
    assert_eq!(s.resources.cargo_level, 1);
    assert_eq!(cargo_capacity(&s), 7);
    assert_eq!(s.player.cargo_size, 7); // soute agrandie
}

#[test]
fn buy_upgrade_refuses_without_minerals_and_maxes_out() {
    // pas assez de crédits : refus, niveau et crédits inchangés ; au
    // niveau max : plus d'achat ; hors scénario à économie : pas d'atelier
    let mut s = progression_state();
    s.resources.credits = 5;
    assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Insufficient(10));
    assert_eq!(s.resources.fuel_level, 0);
    assert!(s.message_queue.contains("NOT ENOUGH CREDITS"));

    s.resources.credits = 1000;
    s.resources.fuel_level = 3;
    assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Maxed);
    assert_eq!(s.resources.credits, 1000);

    let mut f = GameState::new(); // jeu libre : pas d'atelier
    assert_eq!(buy_upgrade(&mut f, UpgradeTrackId::Fuel), UpgradeOutcome::Maxed);
    assert_eq!(f.resources.fuel_level, 0);
}

#[test]
fn reputation_discount_comes_from_the_highest_rank_reached() {
    // la remise est celle du plus haut rang dont le seuil est atteint ;
    // sans rang (table vide) ou réputation nulle : 0
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 0.0), 0);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 9.9), 0);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 10.0), 5);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 24.9), 5);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 25.0), 10);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 49.9), 10);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 50.0), 15);
    assert_eq!(reputation_discount(PROGRESSION_RANKS, 999.0), 15);
    assert_eq!(reputation_discount(&[], 999.0), 0);
}

#[test]
fn precision_boosts_the_reputation_discount() {
    // la remise du rang est multipliée par (1 + poids × précision) :
    // ACE (−15 %) avec poids 1.0 → 0 % de précision : 15 %, 50 % : ~22 %,
    // 100 % : 30 % - sans tir, la précision vaut 0
    let mut s = progression_state();
    s.resources.reputation = 50.0; // ACE
    assert_eq!(current_discount(&s), 15);

    s.bullets_fired = 10;
    s.bullets_lost = 5; // précision 0,5 → 15 × 1,5 = 22,5 → 23 (arrondi)
    assert_eq!(current_discount(&s), 23);

    s.bullets_lost = 0; // précision 1,0 → 15 × 2 = 30
    assert_eq!(current_discount(&s), 30);

    // sans rang (CADET, remise 0) ou poids nul, la précision ne change
    // rien ; jeu libre : pas de remise du tout
    s.resources.reputation = 0.0;
    s.bullets_lost = 0;
    assert_eq!(current_discount(&s), 0);
    s.resources.reputation = 50.0;
    let mut f = GameState::new(); // jeu libre
    f.bullets_fired = 10;
    assert_eq!(current_discount(&f), 0);

    // la remise amplifiée est appliquée aux coûts payés (atelier)
    s.resources.reputation = 50.0;
    s.bullets_fired = 10;
    s.bullets_lost = 0; // 30 % de remise
    s.resources.credits = 1000;
    let outcome = buy_upgrade(&mut s, UpgradeTrackId::Ammo);
    assert_eq!(outcome, UpgradeOutcome::Purchased(7)); // 10 × 70 % = 7
}

#[test]
fn discounted_cost_is_rounded_down_and_never_negative() {
    // coût × (100 − remise) / 100, arrondi à l'entier inférieur - la
    // remise est bornée 0..100
    assert_eq!(discounted_cost(10, 0), 10);
    assert_eq!(discounted_cost(10, 5), 9); // 9,5 → 9
    assert_eq!(discounted_cost(20, 15), 17); // 17,0
    assert_eq!(discounted_cost(100, 100), 0);
    assert_eq!(discounted_cost(50, -5), 50); // remise bornée à 0
    assert_eq!(discounted_cost(50, 120), 0); // remise bornée à 100
}

#[test]
fn reputation_discounts_upgrade_supplies_and_mode_costs() {
    // rang ACE (−15 %) : la remise s'applique à tous les coûts de la
    // station - atelier, ravitaillement et déblocage des modes
    let mut s = progression_state();
    s.resources.credits = 1000;
    s.resources.reputation = 50.0;

    // atelier : extension de réservoir 10 → 8 (10 × 85 % = 8,5 → 8), et
    // l'atelier affiche le prix remisé
    assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Purchased(8));
    assert_eq!(s.resources.credits, 992);
    let line = upgrade_line(&s, UpgradeTrackId::Ammo);
    assert_eq!(line.next.map(|u| u.cost), Some(8), "prix affiché remisé (10 → 8)");

    // ravitaillement : 13 pas de carburant (130/10) + 6 pas de munitions
    // (30/5) = 19 → 16 remisés, achetés indépendamment (13 → 11,
    // 6 → 5) : les coûts affichés au magasin sont les prix remisés
    s.resources.fuel = 0.0;
    s.resources.weapon_ammo[0] = 0;
    assert_eq!(fuel_refill_cost(&s), 11); // 13 × 85 % = 11,05 → 11
    assert_eq!(ammo_refill_cost(&s), 5); // 6 × 85 % = 5,1 → 5
    assert_eq!(purchase_fuel(&mut s), SupplyOutcome::Purchased(11));
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Purchased(5));
    assert_eq!(s.resources.credits, 976);

    // modes payants : 4 WAYS 30 → 25 (tarif de base et prix remisé
    // exposés pour l'affichage du magasin)
    assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), Some(25));
    assert_eq!(mode_unlock_prices(&s, MOVING_MODE_4_WAYS), Some((30, 25)));
    s.resources.credits = 1000;
    assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
    assert_eq!(s.resources.credits, 975);
}

#[test]
fn supplies_fill_to_upgraded_capacity() {
    // après une extension de chargeur, la recharge (magasin, ligne AMMO)
    // remplit la nouvelle capacité (et le prix est recalculé sur ce
    // qu'il manque)
    let mut s = progression_state();
    s.resources.credits = 200;
    assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Ammo), UpgradeOutcome::Purchased(10));
    assert_eq!(ammo_capacity(&s), 40);
    s.resources.weapon_ammo[0] = 20;
    // 20 munitions manquantes = 4 pas de 5 × 1 minerai (carburant plein)
    assert_eq!(ammo_refill_cost(&s), 4);
    assert_eq!(purchase_ammo(&mut s), SupplyOutcome::Purchased(4));
    assert_eq!(total_ammo(&s), 40);
}

#[test]
fn upgrade_levels_persist_and_restore_clamped() {
    // niveaux d'atelier enregistrés avec la progression Progression, puis
    // restaurés sur un départ neuf : réservoirs pleins à la capacité
    // augmentée et soute agrandie ; un niveau farfelu du fichier retombe
    // au max d'extensions
    let p = temp_path("upgrades.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.resources.credits = 77;
    s.resources.fuel_level = 2;
    s.resources.ammo_level = 1;
    s.resources.cargo_level = 1;
    save_progression_to(&p, &s).unwrap();
    assert_eq!(get_i32_from(&p, "prog_up_fuel"), Some(2));
    assert_eq!(get_i32_from(&p, "prog_up_cargo"), Some(1));

    let mut t = progression_state();
    assert_eq!(t.resources.fuel_level, 0); // départ neuf
    load_progression_from(&p, &mut t);
    assert_eq!(t.resources.fuel_level, 2);
    assert_eq!(t.resources.ammo_level, 1);
    assert_eq!(t.resources.cargo_level, 1);
    assert_eq!(t.resources.fuel, 170.0); // 100 + 30 + 40
    assert_eq!(total_ammo(&t), 40); // 30 + 10
    assert_eq!(t.player.cargo_size, 7); // 5 + 2
    assert_eq!(t.resources.credits, 77);

    std::fs::write(&p, "prog_up_fuel=99\n").unwrap();
    let mut u = progression_state();
    load_progression_from(&p, &mut u);
    assert_eq!(u.resources.fuel_level, 3); // borné au max d'extensions
    assert_eq!(u.resources.fuel, 220.0);
    let _ = std::fs::remove_file(&p);
}

// ─── Score composite et record (high-score) ───────────────────────────

#[test]
fn composite_score_sums_credits_meteors_and_objectives() {
    // score composite : crédits **gagnés** (pas le solde, que les achats
    // diminuent) + astéroïdes détruits + 50 points par objectif DAG complété
    let mut s = progression_state();
    assert_eq!(composite_score(&s), 0);
    s.credits_earned = 120;
    s.resources.credits = 20; // 100 dépensés : le solde ne compte pas
    s.meteors_destroyed = 7;
    assert_eq!(composite_score(&s), 127);
    s.objective_tracker.completed_ids.insert("obj_a".into());
    s.objective_tracker.completed_ids.insert("obj_b".into());
    assert_eq!(composite_score(&s), 127 + 2 * SCORE_PER_OBJECTIVE);
}

#[test]
fn credits_earned_tracks_unload_and_objective_rewards() {
    // déchargement : les crédits gagnés suivent le solde ; récompense
    // d'objectif DAG : le cumul suit aussi (score = valeur produite, pas
    // ce qui reste en banque)
    let mut s = progression_state();
    let mut elements = default_elements();
    elements[1].count = 3; // 3 or × 5 = 15 crédits (ELEMENT_VALUES)
    unload_cargo(&mut s, &elements);
    assert_eq!(s.resources.credits, 15);
    assert_eq!(s.credits_earned, 15);
    // achat : le solde baisse, le cumul non
    s.resources.credits -= 10;
    assert_eq!(s.credits_earned, 15);
}

#[test]
fn high_score_updates_only_when_beaten_and_persists_per_scenario() {
    // le record n'est relevé que si le score courant dépasse l'ancien, et
    // chaque scénario écrit sa propre clé `highscore_<index>` (index global,
    // scénarios custom compris)
    let p = temp_path("highscore.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = progression_state();
    s.meteors_destroyed = 10;
    assert!(maybe_update_high_score(&mut s)); // 10 > 0 : record battu
    assert_eq!(s.high_score, 10);
    assert_eq!(
        get_i32_from(&p, &high_score_key(ScenarioId::Progression)),
        None
    ); // la version chemin explicite n'écrit pas ici
    assert_eq!(
        crate::persist::set_i32_to(&p, &high_score_key(ScenarioId::Progression), 10).is_ok(),
        true
    );

    // restauration : le record du scénario est surimposé sur l'état
    let mut t = progression_state();
    load_progression_from(&p, &mut t);
    assert_eq!(t.high_score, 10);

    // score inférieur : pas de nouveau record, la clé n'est pas réduite
    let mut u = progression_state();
    load_progression_from(&p, &mut u); // record restauré : 10
    assert_eq!(u.high_score, 10);
    u.meteors_destroyed = 4; // score courant 4 < 10
    assert!(!maybe_update_high_score(&mut u));
    assert_eq!(u.high_score, 10);

    // score supérieur : relevé
    let mut v = progression_state();
    load_progression_from(&p, &mut v); // record restauré : 10
    v.meteors_destroyed = 12;
    assert!(maybe_update_high_score(&mut v));
    assert_eq!(v.high_score, 12);

    // clé propre au scénario : le record Survival vit ailleurs
    assert_eq!(
        high_score_key(ScenarioId::Survival),
        "highscore_2".to_string()
    );
    assert_eq!(high_score_key(ScenarioId::FreePlay), "highscore_0");
    let _ = std::fs::remove_file(&p);
}

#[test]
fn high_score_survives_progression_reset_and_load() {
    // RESET PROGRESSION / « repartir du début » : la progression (crédits,
    // modes, vies) est effacée, le record du scénario est conservé ; le jeu
    // libre aussi a un record (affiché même sans sauvegarde)
    let p = temp_path("highscore_reset.cfg");
    let _ = std::fs::remove_file(&p);
    let mut s = survival_state();
    s.meteors_destroyed = 33;
    assert!(maybe_update_high_score(&mut s));
    // écrit le record dans le fichier (comme la persistance réelle - le
    // chemin utilisateur est utilisé par `maybe_update_high_score`)
    crate::persist::set_i32_to(&p, &high_score_key(ScenarioId::Survival), 33).unwrap();

    let mut t = survival_state();
    t.resources.lives = 1; // progression entamée
    load_progression_from(&p, &mut t);
    assert_eq!(t.high_score, 33);
    reset_progression_from(&p, &mut t);
    assert_eq!(t.resources.lives, 3); // règles de départ réappliquées
    assert_eq!(load_high_score_from(&p, ScenarioId::Survival), 33); // record intact

    // jeu libre : record restauré aussi (aucune autre sauvegarde)
    let mut f = GameState::new();
    f.scenario = ScenarioId::FreePlay;
    apply_start(&mut f);
    load_progression_from(&p, &mut f);
    assert_eq!(f.high_score, 0); // pas de record Survival pour le jeu libre
    let _ = std::fs::remove_file(&p);
}

#[test]
fn save_summary_shows_high_score_for_all_scenarios() {
    // la ligne SAVE de l'écran titre affiche le record pour tous les
    // scénarios, jeu libre compris (dernier segment, valeur en surbrillance)
    let mut s = GameState::new();
    s.scenario = ScenarioId::FreePlay;
    apply_start(&mut s);
    s.high_score = 42;
    let text: String = save_summary_segments(&s).iter().map(|g| g.text.as_str()).collect();
    assert!(text.contains("record 42"), "ligne SAVE : {text}");
    // la valeur du record est mise en évidence (couleur du scénario)
    let segments = save_summary_segments(&s);
    let seg = segments
        .iter()
        .find(|g| g.text == "42")
        .expect("segment valeur du record");
    assert_eq!(seg.color, Some(RULES_COLOR_YELLOW));

    // jeu libre sans record : « aucune sauvegarde » + record 0
    let mut z = GameState::new();
    z.scenario = ScenarioId::FreePlay;
    apply_start(&mut z);
    let text: String = save_summary_segments(&z).iter().map(|g| g.text.as_str()).collect();
    assert!(text.contains("aucune sauvegarde"));
    assert!(text.contains("record 0"));
}
