//! Persistance de la progression d'une partie (extrait de `scenario.rs`) :
//! clés `prog_*` du fichier de config (`persist.rs`) - crédits, modes payés,
//! réputation, extensions d'atelier, armes et radar possédés (Progression) ;
//! vies et bouclier (Survival, bornés aux capacités du scénario) ;
//! avancement des objectifs DAG (scénarios custom). Chaque scénario n'écrit
//! que ses propres clés, sans écraser la sauvegarde de l'autre.

use std::io;
use std::path::Path;

use super::*;
// ─── Persistance de la progression ──────────────────────────────────────────

/// Clés du fichier de config (voir `persist.rs`) portant la progression d'un
/// scénario - le scénario choisi et sa sauvegarde :
/// - `scenario`        - scénario choisi (0 = jeu libre, 1 = Progression,
///   2 = Survival)
/// - `prog_minerals`   - minerais en banque (Progression)
/// - `prog_modes`      - modes de déplacement débloqués (masque binaire : bit
///   i = mode i débloqué, Progression)
/// - `prog_reputation` - réputation × 10 (entier, au dixième près,
///   Progression)
/// - `prog_lives`      - vies restantes (Survival)
/// - `prog_shield`     - bouclier restant × 10 (entier, au dixième près,
///   Survival)
/// - `prog_up_fuel`    - extensions de réservoir achetées (Progression)
/// - `prog_up_ammo`    - extensions de chargeur achetées (Progression)
/// - `prog_up_cargo`   - extensions de soute achetées (Progression)
/// - `prog_weapons`    - armes du catalogue possédées (masque binaire : bit
///   i = arme i achetée, Progression ; les munitions par arme repartent
///   pleines à chaque lancement, non persistées)
/// - `prog_radar`      - radar de bord possédé (0/1, Progression) - la
///   minimap globale reste éteinte tant que le radar n'est pas acheté
/// - `prog_objectives` - objectifs DAG complétés (IDs séparés par virgules,
///   scénarios custom)
/// - `prog_meteors` / `prog_docks` / `prog_bullets_fired` /
///   `prog_bullets_lost` / `prog_survive` - compteurs d'avancement des
///   conditions d'objectifs (scénarios custom) : restaurés au lancement pour
///   que l'avancement de la phase en cours soit identique à la sortie
const SCENARIO_KEY: &str = "scenario";
const PROG_CREDITS_KEY: &str = "prog_credits";
/// Ancienne clé (sauvegardes créées avant le renommage minerais → crédits) :
/// relue en secours à la restauration, jamais réécrite.
const PROG_MINERALS_LEGACY_KEY: &str = "prog_minerals";
const PROG_MODES_KEY: &str = "prog_modes";
const PROG_REPUTATION_KEY: &str = "prog_reputation";
const PROG_LIVES_KEY: &str = "prog_lives";
const PROG_SHIELD_KEY: &str = "prog_shield";
const PROG_UP_FUEL_KEY: &str = "prog_up_fuel";
const PROG_UP_AMMO_KEY: &str = "prog_up_ammo";
const PROG_UP_CARGO_KEY: &str = "prog_up_cargo";
const PROG_WEAPONS_KEY: &str = "prog_weapons";
const PROG_RADAR_KEY: &str = "prog_radar";
/// Radar de contrôleur aérien possédé (0/1, Progression) - le scope à
/// balayage reste éteint tant qu'il n'est pas acheté. La version **active**
/// (minimap ou ATC) est persistée séparément (clé globale `radar_kind`).
const PROG_ATC_RADAR_KEY: &str = "prog_atc_radar";
const PROG_OBJECTIVES_KEY: &str = "prog_objectives";
// compteurs d'avancement des conditions d'objectifs (scénarios custom)
const PROG_METEORS_KEY: &str = "prog_meteors";
const PROG_DOCKS_KEY: &str = "prog_docks";
const PROG_BULLETS_FIRED_KEY: &str = "prog_bullets_fired";
const PROG_BULLETS_LOST_KEY: &str = "prog_bullets_lost";
const PROG_SURVIVE_KEY: &str = "prog_survive";

/// Masque binaire des modes de déplacement débloqués (bit i = mode i).
fn unlocked_mask(state: &GameState) -> i32 {
    state.unlocked_modes.iter().enumerate().fold(0, |mask, (i, &unlocked)| {
        if unlocked {
            mask | (1 << i)
        } else {
            mask
        }
    })
}

/// Masque binaire des armes possédées (bit i = arme i du catalogue - seules
/// les armes du catalogue sont persistées, le canon classique de repli est
/// toujours possédé).
/// Masque binaire des armes possédées (clé `prog_weapons`). `pub(crate)` :
/// les tests vérifient la clé écrite avec ce même masque.
pub(crate) fn weapons_owned_mask(state: &GameState) -> i32 {
    (0..weapon_slot_count()).fold(0, |mask, i| {
        if state.resources.weapon_owned[i] {
            mask | (1 << i)
        } else {
            mask
        }
    })
}

/// Enregistre la progression courante dans un fichier de config donné :
/// toujours le scénario choisi, et les ressources du scénario - crédits,
/// modes débloqués, réputation, extensions d'atelier et **armes possédées**
/// en Progression, vies et bouclier en Survival (les munitions par arme
/// repartent pleines au lancement : non persistées). Chaque scénario n'écrit
/// que ses propres clés : les clés `prog_*` de l'autre scénario ne sont pas
/// réécrites (une partie Progression ne vide pas la sauvegarde Survival, et
/// inversement). Version chemin explicite (tests).
pub fn save_progression_to(path: &Path, state: &GameState) -> io::Result<()> {
    crate::persist::set_i32_to(path, SCENARIO_KEY, scenario_index(state.scenario) as i32)?;
    if has_economy(state) {
        crate::persist::set_i32_to(path, PROG_CREDITS_KEY, state.resources.credits)?;
        crate::persist::set_i32_to(path, PROG_MODES_KEY, unlocked_mask(state))?;
        crate::persist::set_i32_to(
            path,
            PROG_REPUTATION_KEY,
            (state.resources.reputation * 10.0).round() as i32,
        )?;
        // extensions d'atelier (réservoir, chargeur, soute)
        crate::persist::set_i32_to(path, PROG_UP_FUEL_KEY, state.resources.fuel_level)?;
        crate::persist::set_i32_to(path, PROG_UP_AMMO_KEY, state.resources.ammo_level)?;
        crate::persist::set_i32_to(path, PROG_UP_CARGO_KEY, state.resources.cargo_level)?;
        // armes possédées (les munitions par arme repartent pleines au
        // lancement : non persistées)
        crate::persist::set_i32_to(path, PROG_WEAPONS_KEY, weapons_owned_mask(state))?;
        // radar de bord possédé (minimap globale)
        crate::persist::set_i32_to(path, PROG_RADAR_KEY, state.resources.radar_owned as i32)?;
        // radar de contrôleur aérien possédé (scope à balayage)
        crate::persist::set_i32_to(path, PROG_ATC_RADAR_KEY, state.resources.atc_radar_owned as i32)?;
        // version de radar active (0 = minimap, 1 = contrôleur aérien) -
        // écrite dans le fichier de config comme le mode de déplacement
        crate::persist::save_radar_kind_to(path, radar_kind_index(state.radar_kind))?;
    }
    if has_survival(state) {
        crate::persist::set_i32_to(path, PROG_LIVES_KEY, state.resources.lives)?;
        crate::persist::set_i32_to(
            path,
            PROG_SHIELD_KEY,
            (state.resources.shield * 10.0).round() as i32,
        )?;
    }
    // Objectifs DAG complétés (scénarios custom) : IDs séparés par virgules,
    // et compteurs d'avancement des conditions (météores détruits, accostages,
    // tirs, temps de survie) - restaurés au lancement pour que l'avancement
    // de la phase en cours soit identique à la sortie
    if crate::scenario::is_custom(state.scenario) && state.objective_tracker.has_objectives() {
        let completed: Vec<&str> = state.objective_tracker.completed_ids.iter().map(|s| s.as_str()).collect();
        crate::persist::set_str_to(path, PROG_OBJECTIVES_KEY, &completed.join(","))?;
        crate::persist::set_i32_to(path, PROG_METEORS_KEY, state.meteors_destroyed)?;
        crate::persist::set_i32_to(path, PROG_DOCKS_KEY, state.docking_count)?;
        crate::persist::set_i32_to(path, PROG_BULLETS_FIRED_KEY, state.bullets_fired)?;
        crate::persist::set_i32_to(path, PROG_BULLETS_LOST_KEY, state.bullets_lost)?;
        // temps de survie cumulé par objectif SurviveTime (« id=secondes », …)
        let survive: Vec<String> = state
            .objective_tracker
            .objectives
            .iter()
            .filter(|o| o.condition.condition_type == "SurviveTime" && o.active_time > 0.0)
            .map(|o| format!("{}={:.1}", o.id, o.active_time))
            .collect();
        if survive.is_empty() {
            let _ = crate::persist::delete_key_from(path, PROG_SURVIVE_KEY);
        } else {
            crate::persist::set_str_to(path, PROG_SURVIVE_KEY, &survive.join(","))?;
        }
    }
    Ok(())
}

/// Enregistre la progression courante dans le fichier de config utilisateur
/// (voir `save_progression_to`). Appelé à chaque modification de la
/// progression (déchargement, ravitaillement carburant/munitions au magasin,
/// achat de mode,
/// astéroïde détruit, achat d'extension à l'atelier, impact subi), après un
/// changement de scénario (écran titre, touche N) et à la sortie du jeu
/// (filet de sécurité dans `main.rs`).
pub fn save_progression(state: &GameState) -> io::Result<()> {
    save_progression_to(&crate::persist::config_path(), state)
}

/// Scénario enregistré dans un fichier de config donné (dernier scénario
/// joué), si la clé est présente et valide ; sinon `None` (jeu libre).
pub fn load_scenario_from(path: &Path) -> Option<ScenarioId> {
    match crate::persist::get_i32_from(path, SCENARIO_KEY) {
        Some(0) => Some(ScenarioId::FreePlay),
        Some(1) => Some(ScenarioId::Progression),
        Some(2) => Some(ScenarioId::Survival),
        Some(i @ 3..) => {
            let custom_idx = (i as usize) - 3;
            if custom_idx < crate::scenario_loader::loaded_count() {
                Some(ScenarioId::Custom(custom_idx))
            } else {
                None // scénario custom supprimé depuis la dernière session
            }
        }
        _ => None,
    }
}

/// Scénario enregistré dans le fichier de config utilisateur (voir
/// `load_scenario_from`).
pub fn load_scenario() -> Option<ScenarioId> {
    load_scenario_from(&crate::persist::config_path())
}

/// Surimpose la progression enregistrée sur l'état courant (après
/// `apply_start`) : crédits, modes débloqués, réputation et niveaux
/// d'atelier en Progression, vies et bouclier en Survival, **record du
/// scénario** (clé `highscore_<index>`) partout. Les valeurs sont
/// bornées par les règles du scénario (jamais plus de vies ni de bouclier
/// que la capacité, jamais plus d'extensions que le nombre défini). En
/// Survival, une sauvegarde à 0 vie (partie terminée) repart sur le départ
/// complet. Le mode de déplacement enregistré (`moving_mode`) est restauré
/// s'il est débloqué par la sauvegarde (sinon le mode de départ du scénario
/// reste - jamais un mode non payé). Ne touche pas au scénario courant ; les
/// réservoirs repartent pleins à la **capacité courante** (extensions
/// comprises) et la soute est agrandie selon le niveau restauré. Sans effet
/// en jeu libre. Version chemin explicite (tests).
pub fn load_progression_from(path: &Path, state: &mut GameState) {
    let s = scenario(state.scenario);
    // record (high-score) du scénario : restauré quel que soit le scénario
    // (jeu libre compris) - il survit au RESET PROGRESSION et à une partie
    // repartie du début, et n'est jamais réduit par une sauvegarde plus faible.
    // L'annonce « NEW RECORD » est réarmée : dépasser le record restauré
    // pendant la session doit l'annoncer (une fois).
    state.high_score = load_high_score_from(path, state.scenario);
    state.score_record_announced = false;
    if s.has_economy {
        // crédits : clé courante, puis l'ancienne clé `prog_minerals` en
        // secours pour les sauvegardes créées avant le renommage
        let credits = crate::persist::get_i32_from(path, PROG_CREDITS_KEY)
            .or_else(|| crate::persist::get_i32_from(path, PROG_MINERALS_LEGACY_KEY));
        if let Some(credits) = credits {
            state.resources.credits = credits.max(0);
        }
        if let Some(mask) = crate::persist::get_i32_from(path, PROG_MODES_KEY) {
            for (i, unlocked) in state.unlocked_modes.iter_mut().enumerate() {
                // Les modes dont l'outil a fixé le coût à 0 (REALISTIC, et
                // INERTIAL si paramétré gratuit) restent débloqués même pour
                // une ancienne sauvegarde dont le masque ne connaissait que
                // trois modes.
                *unlocked = mask & (1 << i) != 0 || MODE_COSTS[i] == 0;
            }
            // le mode enregistré n'est restauré que s'il est débloqué par la
            // sauvegarde (un mode payé puis sélectionné retrouve sa place ; un
            // mode jamais payé retombe sur le mode de départ)
            if let Some(mode) = crate::persist::load_moving_mode_from(path) {
                if (0..MOVING_MODE_COUNT).contains(&mode) && state.unlocked_modes[mode as usize] {
                    state.moving_mode = mode;
                }
            }
        }
        if let Some(reputation) = crate::persist::get_i32_from(path, PROG_REPUTATION_KEY) {
            state.resources.reputation = (reputation as f64 / 10.0).max(0.0);
        }
        // extensions d'atelier : restaurées (bornées au nombre d'extensions)
        if let Some(level) = crate::persist::get_i32_from(path, PROG_UP_FUEL_KEY) {
            state.resources.fuel_level = level.clamp(0, s.fuel_upgrades.tiers.len() as i32);
        }
        if let Some(level) = crate::persist::get_i32_from(path, PROG_UP_AMMO_KEY) {
            state.resources.ammo_level = level.clamp(0, s.ammo_upgrades.tiers.len() as i32);
        }
        if let Some(level) = crate::persist::get_i32_from(path, PROG_UP_CARGO_KEY) {
            state.resources.cargo_level = level.clamp(0, s.cargo_upgrades.tiers.len() as i32);
        }
        // armes possédées restaurées (masque binaire) : les armes de base
        // (coût 0) restent équipées même pour une ancienne sauvegarde sans
        // la clé ; chaque arme possédée repart **chargée** à la capacité
        // courante (les munitions ne sont pas persistées)
        if let Some(mask) = crate::persist::get_i32_from(path, PROG_WEAPONS_KEY) {
            for i in 0..weapon_slot_count() {
                state.resources.weapon_owned[i] = mask & (1 << i) != 0;
            }
        }
        for i in 0..weapon_slot_count() {
            state.resources.weapon_owned[i] =
                state.resources.weapon_owned[i] || weapon_spec(i).cost == 0;
            state.resources.weapon_ammo[i] = if state.resources.weapon_owned[i] {
                ammo_capacity(state)
            } else {
                0
            };
        }
        // radar de bord : possédé si la sauvegarde l'a acheté (sinon éteint)
        if let Some(radar) = crate::persist::get_i32_from(path, PROG_RADAR_KEY) {
            state.resources.radar_owned = radar != 0;
        }
        // radar de contrôleur aérien : possédé si la sauvegarde l'a acheté
        if let Some(atc) = crate::persist::get_i32_from(path, PROG_ATC_RADAR_KEY) {
            state.resources.atc_radar_owned = atc != 0;
        }
        // version de radar active : restaurée si elle a été achetée - jamais
        // une version non possédée (repli sur la minimap)
        if let Some(kind) = crate::persist::load_radar_kind_from(path) {
            state.radar_kind = radar_kind_from_index(kind);
            if !radar_kind_available(state, state.radar_kind) {
                state.radar_kind = RadarKind::Minimap;
            }
        }
        // réservoirs pleins à la capacité courante (extensions comprises) et
        // soute à la taille du niveau restauré
        state.resources.fuel = fuel_capacity(state);
        state.player.cargo_size = cargo_capacity(state);
    }
    if s.lives > 0 {
        // vies et bouclier (Survival) : bornés aux capacités du scénario ;
        // une sauvegarde à 0 vie (partie terminée) repart au départ complet
        if let Some(lives) = crate::persist::get_i32_from(path, PROG_LIVES_KEY) {
            if lives > 0 {
                state.resources.lives = lives.min(s.lives);
            }
        }
        if let Some(shield) = crate::persist::get_i32_from(path, PROG_SHIELD_KEY) {
            state.resources.shield = (shield as f64 / 10.0).clamp(0.0, s.shield_capacity);
        }
    }
    // Objectifs DAG complétés (scénarios custom) : restaurer depuis la
    // sauvegarde (IDs séparés par virgules)
    if is_custom(state.scenario) && state.objective_tracker.has_objectives() {
        if let Some(ids_str) = crate::persist::get_str_from(path, PROG_OBJECTIVES_KEY) {
            for id in ids_str.split(',') {
                let id = id.trim().to_string();
                if !id.is_empty() {
                    state.objective_tracker.completed_ids.insert(id.clone());
                    // Marquer l'objectif comme complété dans le tracker
                    if let Some(obj) = state.objective_tracker.objectives.iter_mut().find(|o| o.id == *id) {
                        obj.completed = true;
                    }
                }
            }
        }
        // compteurs d'avancement des conditions (météores détruits, accostages,
        // tirs, temps de survie) : restaurés pour que l'avancement de la phase
        // en cours soit identique à la sortie
        if let Some(v) = crate::persist::get_i32_from(path, PROG_METEORS_KEY) {
            state.meteors_destroyed = v.max(0);
        }
        if let Some(v) = crate::persist::get_i32_from(path, PROG_DOCKS_KEY) {
            state.docking_count = v.max(0);
        }
        if let Some(v) = crate::persist::get_i32_from(path, PROG_BULLETS_FIRED_KEY) {
            state.bullets_fired = v.max(0);
        }
        if let Some(v) = crate::persist::get_i32_from(path, PROG_BULLETS_LOST_KEY) {
            state.bullets_lost = v.max(0);
        }
        // temps de survie cumulé par objectif SurviveTime (« id=secondes », …)
        if let Some(survive) = crate::persist::get_str_from(path, PROG_SURVIVE_KEY) {
            for pair in survive.split(',') {
                let Some((id, secs)) = pair.split_once('=') else { continue; };
                let Ok(secs) = secs.parse::<f64>() else { continue; };
                if let Some(obj) = state.objective_tracker.objectives.iter_mut().find(|o| o.id == id) {
                    obj.active_time = secs.max(0.0);
                }
            }
        }
    }
}

/// Surimpose la progression enregistrée dans le fichier de config utilisateur
/// (voir `load_progression_from`). Appelé au lancement (après `apply_start`)
/// et après un changement de scénario (écran titre, touche N).
pub fn load_progression(state: &mut GameState) {
    load_progression_from(&crate::persist::config_path(), state);
}

/// Remet la progression du scénario courant à zéro (bouton RESET PROGRESSION
/// de l'écran de paramétrage) : les clés `prog_*` du fichier de config
/// (crédits, modes payés, réputation, extensions d'atelier, armes
/// possédées, vies/bouclier) et le mode de déplacement choisi (`moving_mode`)
/// sont supprimées, puis les règles de départ du scénario sont réappliquées
/// (`apply_start`) : crédits 0, seuls les modes gratuits (coût 0) débloqués
/// et les armes de base (coût 0) équipées, réputation nulle, réservoirs
/// pleins, mode de départ (REALISTIC en Progression). Les réglages (musique,
/// volume, rendu, fenêtre) et le scénario choisi sont conservés.
pub fn reset_progression(state: &mut GameState) {
    reset_progression_from(&crate::persist::config_path(), state);
}

/// Version chemin explicite de `reset_progression` (tests) : supprime les
/// clés de progression du fichier donné puis réapplique `apply_start`.
pub fn reset_progression_from(path: &Path, state: &mut GameState) {
    for key in [
        PROG_CREDITS_KEY,
        PROG_MODES_KEY,
        PROG_REPUTATION_KEY,
        PROG_UP_FUEL_KEY,
        PROG_UP_AMMO_KEY,
        PROG_UP_CARGO_KEY,
        PROG_WEAPONS_KEY,
        PROG_RADAR_KEY,
        PROG_ATC_RADAR_KEY,
        PROG_LIVES_KEY,
        PROG_SHIELD_KEY,
        PROG_OBJECTIVES_KEY,
        PROG_METEORS_KEY,
        PROG_DOCKS_KEY,
        PROG_BULLETS_FIRED_KEY,
        PROG_BULLETS_LOST_KEY,
        PROG_SURVIVE_KEY,
        "moving_mode",
        "radar_kind",
    ] {
        let _ = crate::persist::delete_key_from(path, key);
    }
    // NB : la clé `highscore_<index>` n'est PAS supprimée - le record d'un
    // scénario survit à la remise à zéro de sa progression (le RESET
    // réinitialise la partie, pas les records).
    apply_start(state);
}

/// Y a-t-il une progression **enregistrée** pour le scénario courant ?
///
/// Détecte une sauvegarde **réelle** (le joueur a joué et progressé), pas une
/// simple sélection du scénario à l'écran titre (qui écrit déjà les clés
/// `prog_*` aux valeurs du départ) : `state` contient la progression
/// restaurée (`load_progression`) - ses valeurs sont comparées à celles d'un
/// départ frais (`apply_start` sur un état vierge) ; une sauvegarde nulle
/// (valeurs identiques au départ) est ignorée, pour ne pas proposer un choix
/// inutile au lancement. En jeu libre, jamais de sauvegarde. Utilisé à
/// l'écran titre pour proposer « poursuivre le scénario » ou « repartir du
/// début » au lancement (`title.rs`).
pub fn has_saved_progression(state: &GameState) -> bool {
    has_saved_progression_from(&crate::persist::config_path(), state)
}

/// Version chemin explicite de `has_saved_progression` (tests) : mêmes règles,
/// et en plus les **objectifs DAG complétés** (scénarios custom) restaurés
/// depuis la sauvegarde (`prog_objectives`) - des étapes validées constituent
/// une progression même si les ressources sont revenues à leur départ (ex
/// récompenses dépensées).
pub fn has_saved_progression_from(path: &Path, state: &GameState) -> bool {
    let s = scenario(state.scenario);
    // jeu libre : jamais de sauvegarde
    if !s.has_economy && s.lives == 0 && !is_custom(state.scenario) {
        return false;
    }
    // objectifs DAG complétés (scénarios custom) : une sauvegarde avec des
    // étapes validées est réelle même si les ressources sont revenues au
    // départ
    if is_custom(state.scenario) {
        if let Some(ids) = crate::persist::get_str_from(path, PROG_OBJECTIVES_KEY) {
            if !ids.trim().is_empty() {
                return true;
            }
        }
        // compteurs d'avancement : une session qui a progressé sans compléter
        // d'objectif (ex 30 météores détruits sur 50) reste une sauvegarde
        // réelle - le lancement doit proposer de poursuivre
        for key in [
            PROG_METEORS_KEY,
            PROG_DOCKS_KEY,
            PROG_BULLETS_FIRED_KEY,
            PROG_BULLETS_LOST_KEY,
        ] {
            if crate::persist::get_i32_from(path, key).unwrap_or(0) > 0 {
                return true;
            }
        }
        if let Some(survive) = crate::persist::get_str_from(path, PROG_SURVIVE_KEY) {
            if !survive.trim().is_empty() {
                return true;
            }
        }
    }
    // départ frais : les valeurs de référence pour comparer
    let mut fresh = GameState::new();
    fresh.scenario = state.scenario;
    apply_start(&mut fresh);
    if s.has_economy
        && (state.resources.credits != fresh.resources.credits
            || (state.resources.reputation - fresh.resources.reputation).abs() > 1e-9
            || state.resources.fuel_level != fresh.resources.fuel_level
            || state.resources.ammo_level != fresh.resources.ammo_level
            || state.resources.cargo_level != fresh.resources.cargo_level
            || state.unlocked_modes != fresh.unlocked_modes
            || state.resources.weapon_owned != fresh.resources.weapon_owned)
        {
            return true;
        }
    if s.lives > 0 {
        // une vie perdue ou un bouclier entamé : le joueur a joué
        if state.resources.lives < s.lives || (state.resources.shield - s.shield_capacity).abs() > 1e-9 {
            return true;
        }
    }
    false
}
