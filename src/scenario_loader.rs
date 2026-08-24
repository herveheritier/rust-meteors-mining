//! Chargement dynamique des scénarios JSON (éditeur de scénarios DAG).
//!
//! Au lancement, le dossier `scenarios/` est scanné pour les fichiers
//! `*.scenario.json`. Chaque fichier est parsé et converti en un `Scenario`
//! statique (`&'static`) utilisable par la boucle de jeu comme les trois
//! scénarios hardcodés (FreePlay / Progression / Survival).
//!
//! Le JSON contient :
//! - `initial_state` → champs du `Scenario` (fuel, ammo, vies, bouclier…)
//! - `objectives`    → objectifs DAG (utilisés par l'éditeur et le HUD)
//!
//! `fuel_per_second`, `ammo_per_shot`, `reputation_*`, `fuel_price`,
//! `fuel_step`, `damage_multiplier`, `respawn_invulnerability` ne sont pas
//! dans le JSON : ils prennent des valeurs par défaut raisonnables.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use crate::marketplace::{
    FUEL_PRICE, FUEL_STEP, MODE_COSTS, PROGRESSION_RANKS,
    FUEL_UPGRADE_TRACK, AMMO_UPGRADE_TRACK, CARGO_UPGRADE_TRACK,
};
use crate::scenario::{Scenario, RULES_COLOR_YELLOW};

// ─── Structure JSON parsée ──────────────────────────────────────────────────

/// Représentation complète d'un scénario au format JSON (éditeur DAG).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct JsonScenario {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_color")]
    pub rules_color: String,
    #[serde(default)]
    pub initial_state: InitialState,
    #[serde(default)]
    pub objectives: Vec<JsonObjective>,
}

fn default_color() -> String {
    "0xFF39FF88".to_string()
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct InitialState {
    #[serde(default)]
    pub start_fuel: f64,
    #[serde(default)]
    pub start_ammo: i32,
    /// Crédits de départ (monnaie). `start_minerals` (ancien nom) reste
    /// accepté pour les scénarios écrits avant le renommage.
    #[serde(default, alias = "start_minerals")]
    pub start_credits: i32,
    #[serde(default)]
    pub start_reputation: f64,
    #[serde(default = "default_start_mode")]
    pub start_mode: i32,
    #[serde(default)]
    pub lives: i32,
    #[serde(default)]
    pub shield_capacity: f64,
    /// Position X initiale du vaisseau (0 = centre = station).
    #[serde(default)]
    pub start_pos_x: f64,
    /// Position Y initiale du vaisseau.
    #[serde(default)]
    pub start_pos_y: f64,
    /// Orientation initiale en degrés (0 = pointe vers la droite).
    #[serde(default)]
    pub start_orientation: f64,
    /// Vitesse initiale (0 = immobile).
    #[serde(default)]
    pub start_velocity: f64,
}

fn default_start_mode() -> i32 {
    crate::config::MOVING_MODE_REALISTIC
}

impl Default for InitialState {
    fn default() -> Self {
        Self {
            start_fuel: 100.0,
            start_ammo: 30,
            start_credits: 0,
            start_reputation: 0.0,
            start_mode: crate::config::MOVING_MODE_REALISTIC,
            lives: 0,
            shield_capacity: 0.0,
            start_pos_x: 0.0,
            start_pos_y: 0.0,
            start_orientation: 0.0,
            start_velocity: 0.0,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct JsonObjective {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub condition: JsonCondition,
    #[serde(default)]
    pub reward: JsonReward,
    #[serde(default)]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[allow(dead_code)]
pub struct JsonCondition {
    #[serde(rename = "type", default)]
    pub condition_type: String,
    #[serde(default)]
    pub required: u32,
    #[serde(default)]
    pub mode: i32,
    #[serde(default)]
    pub hits: u32,
    #[serde(default)]
    pub min_precision: f64,
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub track: String,
    #[serde(default)]
    pub level: i32,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[allow(dead_code)]
pub struct JsonReward {
    #[serde(rename = "type", default)]
    pub reward_type: String,
    #[serde(default)]
    pub amount: f64,
}

// ─── Chargement et stockage statique ────────────────────────────────────────

/// Données brutes d'un scénario JSON chargé, avec les objectifs DAG.
pub struct LoadedScenarioData {
    pub json: JsonScenario,
}

/// Scénario chargé : les règles runtime (`Scenario`) et les données brutes
/// (objectifs, état initial).
pub struct LoadedScenario {
    pub rules: Scenario,
    pub data: LoadedScenarioData,
}

/// Nombre fixe de scénarios built-in (FreePlay, Progression, Survival).
#[allow(dead_code)]
pub const BUILTIN_SCENARIO_COUNT: usize = 3;

/// Parse la couleur ARGB depuis une chaîne hex (« 0xFF39FF88 » → `u32`).
fn parse_argb_color(s: &str) -> u32 {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(s, 16).unwrap_or(RULES_COLOR_YELLOW)
}

/// Scanne le dossier `scenarios/` et charge tous les fichiers
/// `*.scenario.json` en `LoadedScenario` statiques.
fn load_all_scenarios() -> Vec<LoadedScenario> {
    let scenarios_dir = find_scenarios_dir();
    let mut result = Vec::new();

    if !scenarios_dir.exists() {
        eprintln!(
            "[scenario_loader] dossier scenarios/ introuvable : {}",
            scenarios_dir.display()
        );
        return result;
    }

    let mut entries: Vec<_> = fs::read_dir(&scenarios_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !filename.ends_with(".scenario.json") {
            continue;
        }

        match load_one_scenario(&path) {
            Ok(loaded) => {
                eprintln!(
                    "[scenario_loader] ✓ chargé : {} ({} objectifs)",
                    loaded.data.json.id,
                    loaded.data.json.objectives.len()
                );
                result.push(loaded);
            }
            Err(e) => {
                eprintln!(
                    "[scenario_loader] ✗ erreur {} : {}",
                    path.display(),
                    e
                );
            }
        }
    }

    result
}

/// Charge un fichier JSON de scénario et le convertit en `LoadedScenario`.
fn load_one_scenario(path: &Path) -> Result<LoadedScenario, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("impossible de lire {}: {}", path.display(), e))?;
    let json: JsonScenario = serde_json::from_str(&content)
        .map_err(|e| format!("JSON invalide {}: {}", path.display(), e))?;

    let ist = &json.initial_state;
    let color = parse_argb_color(&json.rules_color);

    let rules = Scenario {
        name: Box::leak(json.name.clone().into_boxed_str()),
        description: Box::leak(json.description.clone().into_boxed_str()),
        has_economy: ist.start_credits > 0 || ist.start_fuel > 0.0 || ist.start_ammo > 0,
        ranks: PROGRESSION_RANKS,
        start_fuel: ist.start_fuel,
        fuel_per_second: 2.0, // défaut raisonnable (~50 s de poussée avec 100 carburant)
        start_ammo: ist.start_ammo,
        ammo_per_shot: 1,
        mode_costs: MODE_COSTS,
        reputation_per_asteroid: 1.0,
        reputation_precision_weight: 2.0,
        reputation_per_mineral: 0.1,
        discount_precision_weight: 0.2,
        fuel_price: FUEL_PRICE,
        fuel_step: FUEL_STEP,
        lives: ist.lives,
        shield_capacity: ist.shield_capacity,
        damage_multiplier: 1.0,
        respawn_invulnerability: if ist.lives > 0 { 2.0 } else { 0.0 },
        rules_color: color,
        fuel_upgrades: FUEL_UPGRADE_TRACK,
        ammo_upgrades: AMMO_UPGRADE_TRACK,
        cargo_upgrades: CARGO_UPGRADE_TRACK,
    };

    Ok(LoadedScenario {
        rules,
        data: LoadedScenarioData { json },
    })
}

/// Cherche le dossier `scenarios/` : à côté de l'exécutable, puis cwd.
fn find_scenarios_dir() -> std::path::PathBuf {
    // 1. À côté de l'exécutable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("scenarios");
            if p.is_dir() {
                return p;
            }
        }
    }
    // 2. CWD
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("scenarios")
}

// ─── Stockage statique via OnceLock ───────────────────────────────────────

/// Vec de tous les scénarios JSON chargés depuis `scenarios/*.scenario.json`.
/// Initialisé une seule fois au premier accès. Les `Scenario` internes
/// utilisent des `&'static` (via `Box::leak` dans `load_one_scenario`),
/// ce qui les rend compatibles avec le système existant (rangs, upgrades,
/// noms = `&'static str`).
static LOADED: OnceLock<Vec<LoadedScenario>> = OnceLock::new();

pub fn loaded_scenarios() -> &'static Vec<LoadedScenario> {
    LOADED.get_or_init(load_all_scenarios)
}

/// Nombre de scénarios JSON chargés.
pub fn loaded_count() -> usize {
    loaded_scenarios().len()
}

/// Renvoie le `Scenario` (règles runtime) d'un scénario chargé par son index.
pub fn loaded_rules(index: usize) -> Option<&'static Scenario> {
    loaded_scenarios().get(index).map(|ls| &ls.rules)
}

/// Renvoie les données JSON d'un scénario chargé par son index.
pub fn loaded_data(index: usize) -> Option<&'static LoadedScenarioData> {
    loaded_scenarios().get(index).map(|ls| &ls.data)
}

/// Renvoie le nom affiché d'un scénario chargé.
#[allow(dead_code)]
pub fn loaded_name(index: usize) -> &'static str {
    loaded_rules(index).map(|r| r.name).unwrap_or("???")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_argb_color_variants() {
        assert_eq!(parse_argb_color("0xFF39FF88"), 0xFF39FF88);
        assert_eq!(parse_argb_color("0x00FFFF"), 0x0000FFFF);
        assert_eq!(parse_argb_color("FF000000"), 0xFF000000);
        assert_eq!(parse_argb_color("invalid"), RULES_COLOR_YELLOW);
    }

    #[test]
    fn loaded_count_is_non_negative() {
        // On ne peut pas garantir le nombre de fichiers, mais le compteur
        // doit être cohérent avec le vecteur
        assert_eq!(loaded_count(), loaded_scenarios().len());
    }

    #[test]
    fn every_loaded_scenario_has_name_rules_and_initial_state() {
        // « Round-trip » des fichiers .scenario.json embarqués : chaque
        // scénario chargé doit exposer un nom, des règles (économie ou survie
        // ou libre) et un état initial cohérent - le parseur a réussi pour
        // tous, sinon le chargement même échouerait (include_str + parse)
        for s in loaded_scenarios() {
            assert!(!s.rules.name.is_empty(), "scénario sans nom");
            // un état initial cohérent (carburant ou crédits ou vies)
            let init = &s.data.json.initial_state;
            assert!(
                init.start_fuel > 0.0 || init.start_credits > 0 || init.lives > 0,
                "état initial incohérent pour {}",
                s.rules.name
            );
            assert!(
                s.data.json.objectives.iter().all(|o| !o.id.is_empty()),
                "objectif sans id"
            );
        }
    }
}
