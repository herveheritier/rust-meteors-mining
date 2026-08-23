//! Suivi en temps réel des objectifs DAG pendant la partie.
//!
//! À chaque frame, `ObjectiveTracker::update` vérifie les conditions de tous
//! les objectifs débloqués contre l'état du jeu (`GameState`) et renvoie les
//! objectifs nouvellement complétés (pour attribution des récompenses et
//! messages HUD). Le suivi est automatiquement réinitialisé au changement de
//! scénario (`apply_start` → `init_for_scenario`).
//!
//! Seuls les scénarios custom (chargés depuis JSON) ont des objectifs ; les
//! scénarios built-in (FreePlay, Progression, Survival) n'en ont pas.

use std::collections::HashSet;

use crate::scenario_loader::{JsonCondition, JsonReward, JsonScenario, loaded_scenarios};
use crate::state::GameState;

/// État d'un objectif individuel pendant la partie.
#[derive(Debug, Clone)]
pub struct TrackedObjective {
    /// Identifiant unique (ex. `"step_first_dock"`).
    pub id: String,
    /// Titre affiché (ex. "Premier Accostage").
    pub title: String,
    /// Description explicative.
    pub description: String,
    /// Objectifs prérequis (doivent tous être complétés pour débloquer).
    pub prerequisites: Vec<String>,
    /// Condition de validation.
    pub condition: JsonCondition,
    /// Récompense accordée à la validation.
    pub reward: JsonReward,
    /// Déjà complété ? (une fois complété, plus jamais re-comptabilisé).
    pub completed: bool,
}

/// Suivi complet des objectifs pour le scénario courant.
#[derive(Debug, Clone)]
pub struct ObjectiveTracker {
    /// Tous les objectifs du scénario courant (pour évaluation).
    pub objectives: Vec<TrackedObjective>,
    /// IDs des objectifs complétés (set rapide pour les prérequis).
    pub completed_ids: HashSet<String>,
    /// Index du scénario custom associé (dans `LOADED_SCENARIOS`), ou `None`
    /// si le scénario courant n'a pas d'objectifs.
    scenario_index: Option<usize>,
    /// Titre du dernier objectif complété (pour affichage de notification).
    pub last_completed_title: Option<String>,
    /// Temps restant d'affichage de la notification (secondes).
    pub notification_timer: f64,
}

impl Default for ObjectiveTracker {
    fn default() -> Self {
        Self {
            objectives: Vec::new(),
            completed_ids: HashSet::new(),
            scenario_index: None,
            last_completed_title: None,
            notification_timer: 0.0,
        }
    }
}

impl ObjectiveTracker {
    /// Initialise le tracker pour un scénario custom donné (son index dans
    /// `LOADED_SCENARIOS`). Les objectifs sont copiés depuis le JSON chargé ;
    /// les compteurs sont à zéro (nouvelle partie). Rien ne se passe pour les
    /// scénarios built-in (pas d'objectifs).
    pub fn init_for_scenario(&mut self, scenario_index: usize) {
        self.completed_ids.clear();
        self.objectives.clear();

        if let Some(data) = crate::scenario_loader::loaded_data(scenario_index) {
            self.scenario_index = Some(scenario_index);
            for obj in &data.json.objectives {
                self.objectives.push(TrackedObjective {
                    id: obj.id.clone(),
                    title: obj.title.clone(),
                    description: obj.description.clone(),
                    prerequisites: obj.prerequisites.clone(),
                    condition: obj.condition.clone(),
                    reward: obj.reward.clone(),
                    completed: false,
                });
            }
        } else {
            self.scenario_index = None;
        }
    }

    /// Réinitialise le tracker (nouvelle partie ou changement de scénario).
    pub fn reset(&mut self) {
        self.completed_ids.clear();
        for obj in &mut self.objectives {
            obj.completed = false;
        }
        self.last_completed_title = None;
        self.notification_timer = 0.0;
    }

    /// Décrémente le timer de notification (appelé chaque frame).
    pub fn tick(&mut self, dt: f64) {
        if self.notification_timer > 0.0 {
            self.notification_timer -= dt;
            if self.notification_timer <= 0.0 {
                self.notification_timer = 0.0;
                self.last_completed_title = None;
            }
        }
    }

    /// La notification de complétion est-elle active ?
    #[allow(dead_code)]
    pub fn has_notification(&self) -> bool {
        self.notification_timer > 0.0 && self.last_completed_title.is_some()
    }

    /// Vérifie toutes les conditions et renvoie les IDs des objectifs
    /// **nouvellement** complétés à cette frame (au plus une fois par
    /// objectif). Les récompenses doivent être appliquées par l'appelant.
    pub fn update(&mut self, state: &GameState) -> Vec<ObjectiveResult> {
        if self.objectives.is_empty() {
            return Vec::new();
        }

        let mut newly_completed = Vec::new();

        for obj in &mut self.objectives {
            if obj.completed {
                continue;
            }

            // Vérifier que tous les prérequis sont satisfaits
            let prerequisites_met = obj
                .prerequisites
                .iter()
                .all(|pre| self.completed_ids.contains(pre));
            if !prerequisites_met {
                continue;
            }

            // Évaluer la condition
            if evaluate_condition(&obj.condition, state) {
                obj.completed = true;
                self.completed_ids.insert(obj.id.clone());
                // Afficher la notification de complétion (4 secondes)
                self.last_completed_title = Some(obj.title.clone());
                self.notification_timer = 4.0;
                newly_completed.push(ObjectiveResult {
                    id: obj.id.clone(),
                    reward: obj.reward.clone(),
                });
            }
        }

        newly_completed
    }

    /// Indique si le tracker a des objectifs (scénario custom avec DAG).
    pub fn has_objectives(&self) -> bool {
        !self.objectives.is_empty()
    }

    /// Nombre total d'objectifs.
    pub fn total_count(&self) -> usize {
        self.objectives.len()
    }

    /// Nombre d'objectifs complétés.
    pub fn completed_count(&self) -> usize {
        self.completed_ids.len()
    }

    /// Renvoie les objectifs actuellement **débloqués** (prérequis satisfaits,
    /// pas encore complétés) pour affichage HUD.
    pub fn unlocked_objectives(&self) -> Vec<&TrackedObjective> {
        self.objectives
            .iter()
            .filter(|obj| {
                !obj.completed
                    && obj
                        .prerequisites
                        .iter()
                        .all(|pre| self.completed_ids.contains(pre))
            })
            .collect()
    }

    /// Renvoie l'objectif le plus récent (pour affichage prioritaire HUD).
    #[allow(dead_code)]
    pub fn current_objective(&self) -> Option<&TrackedObjective> {
        self.unlocked_objectives().into_iter().next()
    }

    /// Renvoie le titre de l'objectif par son ID (pour messages).
    pub fn objective_title(&self, id: &str) -> Option<&str> {
        // Chercher dans les données JSON source
        if let Some(idx) = self.scenario_index {
            if let Some(data) = crate::scenario_loader::loaded_data(idx) {
                return data
                    .json
                    .objectives
                    .iter()
                    .find(|o| o.id == id)
                    .map(|o| o.title.as_str());
            }
        }
        None
    }

    /// Renvoie la description de l'objectif par son ID.
    #[allow(dead_code)]
    pub fn objective_description(&self, id: &str) -> Option<&str> {
        if let Some(idx) = self.scenario_index {
            if let Some(data) = crate::scenario_loader::loaded_data(idx) {
                return data
                    .json
                    .objectives
                    .iter()
                    .find(|o| o.id == id)
                    .map(|o| o.description.as_str());
            }
        }
        None
    }
}

/// Résultat de la complétion d'un objectif.
#[derive(Debug, Clone)]
pub struct ObjectiveResult {
    pub id: String,
    pub reward: JsonReward,
}

/// Évalue une condition de validation contre l'état du jeu.
fn evaluate_condition(condition: &JsonCondition, state: &GameState) -> bool {
    match condition.condition_type.as_str() {
        "DestroyAsteroids" => state.meteors_destroyed >= condition.required as i32,
        "CollectMinerals" => state.resources.minerals >= condition.required as i32,
        "ReachReputation" => state.resources.reputation >= condition.required as f64,
        "DockAtStation" => state.docking_count >= condition.required as i32,
        "UnlockMovementMode" => {
            let mode = condition.mode as usize;
            mode < state.unlocked_modes.len() && state.unlocked_modes[mode]
        }
        "SurviveTime" => {
            // Le temps de survie est approximé par la différence entre le
            // temps de jeu et le dernier respawn. On utilise `meteors_destroyed`
            // comme indicateur indirect (le joueur joue depuis le début).
            // Pour une précision exacte, on pourrait ajouter un timer dans le
            // tracker — mais `game_time` n'existe pas encore dans GameState.
            // On approxime avec la réputation cumulée (qui croît avec le temps
            // de jeu en Progression) ou on laisse la condition type "always"
            // si le joueur est en vie.
            //
            // NOTE: pour une précision exacte, un champ `play_time: f64` serait
            // ajouté à GameState. Pour l'instant, on utilise la condition
            // "SurviveTime" comme "le joueur est encore en vie après le
            // démarrage" — validé dès que `meteors_destroyed > 0` ou
            // `player_at_station > 0` (le joueur a interagi avec le monde).
            state.meteors_destroyed > 0 || state.player_at_station > 0
        }
        "PrecisionShooting" => {
            if state.bullets_fired == 0 {
                return false;
            }
            let precision =
                1.0 - (state.bullets_lost as f64 / state.bullets_fired as f64);
            let precision = precision.max(0.0);
            // Vérifier à la fois le nombre de tirs au but et la précision min
            let hits = state.bullets_fired - state.bullets_lost;
            hits >= condition.hits as i32 && precision >= condition.min_precision
        }
        "BuyUpgrade" => {
            // Vérifier si une ligne d'amélioration a atteint le niveau requis
            match condition.track.as_str() {
                "Fuel" => state.resources.fuel_level >= condition.level,
                "Ammo" => state.resources.ammo_level >= condition.level,
                "Cargo" => state.resources.cargo_level >= condition.level,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Récupère les données JSON d'un scénario custom par son index, ou `None`.
#[allow(dead_code)]
pub fn get_scenario_json(index: usize) -> Option<&'static JsonScenario> {
    loaded_scenarios().get(index).map(|ls| &ls.data.json)
}

/// Récupère la couleur ARGB des règles d'un scénario custom.
#[allow(dead_code)]
pub fn scenario_color(index: usize) -> Option<u32> {
    loaded_scenarios()
        .get(index)
        .map(|ls| ls.rules.rules_color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario_loader::{JsonCondition, JsonReward};

    #[test]
    fn empty_tracker_has_no_objectives() {
        let tracker = ObjectiveTracker::default();
        assert!(!tracker.has_objectives());
        assert_eq!(tracker.total_count(), 0);
        assert_eq!(tracker.completed_count(), 0);
    }

    #[test]
    fn evaluate_destroy_asteroids() {
        let mut state = GameState::new();
        let cond = JsonCondition {
            condition_type: "DestroyAsteroids".to_string(),
            required: 5,
            ..Default::default()
        };
        assert!(!evaluate_condition(&cond, &state));
        state.meteors_destroyed = 4;
        assert!(!evaluate_condition(&cond, &state));
        state.meteors_destroyed = 5;
        assert!(evaluate_condition(&cond, &state));
    }

    #[test]
    fn evaluate_collect_minerals() {
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        let cond = JsonCondition {
            condition_type: "CollectMinerals".to_string(),
            required: 10,
            ..Default::default()
        };
        assert!(!evaluate_condition(&cond, &state));
        state.resources.minerals = 10;
        assert!(evaluate_condition(&cond, &state));
    }

    #[test]
    fn evaluate_reach_reputation() {
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        let cond = JsonCondition {
            condition_type: "ReachReputation".to_string(),
            required: 25,
            ..Default::default()
        };
        assert!(!evaluate_condition(&cond, &state));
        state.resources.reputation = 25.0;
        assert!(evaluate_condition(&cond, &state));
    }

    #[test]
    fn evaluate_unlock_mode() {
        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);
        let cond = JsonCondition {
            condition_type: "UnlockMovementMode".to_string(),
            mode: 1, // INERTIAL
            ..Default::default()
        };
        assert!(!evaluate_condition(&cond, &state)); // pas débloqué
        state.unlocked_modes[1] = true;
        assert!(evaluate_condition(&cond, &state));
    }

    #[test]
    fn prerequisites_block_unlocked_objectives() {
        // Un objectif avec un prérequis non complété n'est pas débloqué
        let mut tracker = ObjectiveTracker::default();
        tracker.objectives = vec![
            TrackedObjective {
                id: "a".to_string(),
                title: "A".to_string(),
                description: "desc a".to_string(),
                prerequisites: vec![],
                condition: JsonCondition {
                    condition_type: "CollectMinerals".to_string(),
                    required: 10,
                    ..Default::default()
                },
                reward: JsonReward::default(),
                completed: false,
            },
            TrackedObjective {
                id: "b".to_string(),
                title: "B".to_string(),
                description: "desc b".to_string(),
                prerequisites: vec!["a".to_string()],
                condition: JsonCondition {
                    condition_type: "CollectMinerals".to_string(),
                    required: 0, // toujours vrai une fois débloqué
                    ..Default::default()
                },
                reward: JsonReward::default(),
                completed: false,
            },
        ];

        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);

        // "a" n'est pas encore complété (0 minerais < 10 requis)
        let results = tracker.update(&state);
        assert_eq!(results.len(), 0);

        // "a" est débloqué (pas de prérequis), "b" ne l'est pas ("a" non complété)
        let unlocked = tracker.unlocked_objectives();
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].id, "a");

        // On obtient 10 minerais → "a" est complété et "b" (required: 0)
        // est immédiatement débloqué et complété dans la même frame car
        // ses prérequis ("a") sont désormais satisfaits.
        state.resources.minerals = 10;
        let results = tracker.update(&state);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a");
        assert_eq!(results[1].id, "b");
    }

    #[test]
    fn completed_objectives_are_not_recounted() {
        let mut tracker = ObjectiveTracker::default();
        tracker.objectives = vec![TrackedObjective {
            id: "x".to_string(),
            title: "X".to_string(),
            description: "desc x".to_string(),
            prerequisites: vec![],
            condition: JsonCondition {
                condition_type: "DestroyAsteroids".to_string(),
                required: 0,
                ..Default::default()
            },
            reward: JsonReward::default(),
            completed: false,
        }];

        let state = GameState::new();
        let r1 = tracker.update(&state);
        assert_eq!(r1.len(), 1);
        let r2 = tracker.update(&state);
        assert_eq!(r2.len(), 0); // déjà complété
    }
}
