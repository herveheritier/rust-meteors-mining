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

use std::collections::{HashSet, VecDeque};

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
    /// Temps de survie accumulé pendant la partie (secondes, réinitialisé à la mort).
    pub active_time: f64,
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
    /// Titres des complétions en attente d'affichage (une bannière à la
    /// fois : quand la courante expire, la suivante est affichée).
    notification_queue: VecDeque<String>,
}

impl Default for ObjectiveTracker {
    fn default() -> Self {
        Self {
            objectives: Vec::new(),
            completed_ids: HashSet::new(),
            scenario_index: None,
            last_completed_title: None,
            notification_timer: 0.0,
            notification_queue: VecDeque::new(),
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
        self.last_completed_title = None;
        self.notification_timer = 0.0;
        self.notification_queue.clear();

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
                    active_time: 0.0,
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
            obj.active_time = 0.0;
        }
        self.last_completed_title = None;
        self.notification_timer = 0.0;
        self.notification_queue.clear();
    }

    /// Décrémente le timer de notification (appelé chaque frame) et enchaîne
    /// la notification suivante de la file quand la bannière courante expire.
    pub fn tick(&mut self, dt: f64) {
        if self.notification_timer > 0.0 {
            self.notification_timer -= dt;
            if self.notification_timer <= 0.0 {
                self.notification_timer = 0.0;
                self.last_completed_title = None;
                if let Some(next) = self.notification_queue.pop_front() {
                    self.last_completed_title = Some(next);
                    self.notification_timer = 4.0;
                }
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
    ///
    /// Tous les objectifs débloqués sont évalués dans la **même passe**,
    /// sans attendre la fin d'une notification : un objectif qui vient
    /// d'être assigné (prérequis satisfaits) et dont la condition est déjà
    /// remplie à cet instant est marqué comme réalisé sur-le-champ, puis on
    /// passe à l'objectif suivant. Sans cela, l'état pouvait changer entre
    /// l'assignation et l'évaluation (ex. minerais dépensés au magasin à
    /// quai) et l'objectif n'était jamais validé. Les notifications de
    /// complétion s'affichent une à la fois (file d'attente).
    pub fn update(&mut self, state: &GameState, dt: f64) -> Vec<ObjectiveResult> {
        if self.objectives.is_empty() {
            return Vec::new();
        }

        let mut newly_completed = Vec::new();
        let is_playing = !state.paused && !state.game_over && !state.cosmonaut_active && !state.dock_box && !state.shop_box;

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

            // Mettre à jour le temps de survie pour les objectifs SurviveTime
            if obj.condition.condition_type == "SurviveTime" {
                if is_playing {
                    obj.active_time += dt;
                } else if state.cosmonaut_active || state.game_over {
                    obj.active_time = 0.0;
                }
            }

            // Évaluer la condition
            if evaluate_condition(obj, state) {
                obj.completed = true;
                self.completed_ids.insert(obj.id.clone());
                newly_completed.push(ObjectiveResult {
                    id: obj.id.clone(),
                    reward: obj.reward.clone(),
                });
                // Notifier la complétion : la bannière s'affiche si aucune
                // n'est en cours, sinon le titre attend dans la file (une
                // bannière à la fois, enchaînée par `tick`)
                let title = obj.title.clone();
                if self.notification_timer <= 0.0 {
                    self.last_completed_title = Some(title);
                    self.notification_timer = 4.0;
                } else {
                    self.notification_queue.push_back(title);
                }
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

/// Résout l'index du mode de vol cible (compatibilité 0-based et 1-based historique).
pub fn resolve_target_mode(cond: &crate::scenario_loader::JsonCondition, title: &str, description: &str) -> i32 {
    let mode = cond.mode;
    let t_lower = title.to_lowercase();
    let d_lower = description.to_lowercase();

    if t_lower.contains("inert") || d_lower.contains("inert") {
        return crate::config::MOVING_MODE_INERTIAL;
    }
    if t_lower.contains("4 voie") || t_lower.contains("4 way") || d_lower.contains("4 voie") || d_lower.contains("4 way") {
        return crate::config::MOVING_MODE_4_WAYS;
    }
    if t_lower.contains("direction") || d_lower.contains("direction") {
        return crate::config::MOVING_MODE_DIRECTIONAL;
    }

    match mode {
        0 => crate::config::MOVING_MODE_INERTIAL,
        1 => crate::config::MOVING_MODE_4_WAYS,
        2 => crate::config::MOVING_MODE_DIRECTIONAL,
        3 => crate::config::MOVING_MODE_REALISTIC,
        _ => mode,
    }
}

/// Évalue une condition de validation contre l'état du jeu.
fn evaluate_condition(obj: &TrackedObjective, state: &GameState) -> bool {
    match obj.condition.condition_type.as_str() {
        "DestroyAsteroids" => state.meteors_destroyed >= obj.condition.required as i32,
        "CollectMinerals" => state.resources.minerals >= obj.condition.required as i32,
        "ReachReputation" => state.resources.reputation >= obj.condition.required as f64,
        "DockAtStation" => state.docking_count >= obj.condition.required as i32,
        "UnlockMovementMode" => {
            let target_mode = resolve_target_mode(&obj.condition, &obj.title, &obj.description);
            if target_mode >= 0 && (target_mode as usize) < state.unlocked_modes.len() {
                state.unlocked_modes[target_mode as usize] || state.moving_mode == target_mode
            } else {
                false
            }
        }
        "SurviveTime" => {
            let required_sec = if obj.condition.seconds > 0.0 {
                obj.condition.seconds
            } else if obj.condition.required > 0 {
                obj.condition.required as f64
            } else {
                30.0
            };
            obj.active_time >= required_sec
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
            hits >= obj.condition.hits as i32 && precision >= obj.condition.min_precision
        }
        "BuyUpgrade" => {
            // Vérifier si une ligne d'amélioration a atteint le niveau requis
            match obj.condition.track.as_str() {
                "Fuel" => state.resources.fuel_level >= obj.condition.level,
                "Ammo" => state.resources.ammo_level >= obj.condition.level,
                "Cargo" => state.resources.cargo_level >= obj.condition.level,
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

    fn make_test_obj(cond: JsonCondition) -> TrackedObjective {
        TrackedObjective {
            id: "test".to_string(),
            title: "Test".to_string(),
            description: "desc".to_string(),
            prerequisites: vec![],
            condition: cond,
            reward: JsonReward::default(),
            completed: false,
            active_time: 0.0,
        }
    }

    #[test]
    fn evaluate_destroy_asteroids() {
        let mut state = GameState::new();
        let cond = JsonCondition {
            condition_type: "DestroyAsteroids".to_string(),
            required: 5,
            ..Default::default()
        };
        assert!(!evaluate_condition(&make_test_obj(cond.clone()), &state));
        state.meteors_destroyed = 4;
        assert!(!evaluate_condition(&make_test_obj(cond.clone()), &state));
        state.meteors_destroyed = 5;
        assert!(evaluate_condition(&make_test_obj(cond), &state));
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
        assert!(!evaluate_condition(&make_test_obj(cond.clone()), &state));
        state.resources.minerals = 10;
        assert!(evaluate_condition(&make_test_obj(cond), &state));
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
        assert!(!evaluate_condition(&make_test_obj(cond.clone()), &state));
        state.resources.reputation = 25.0;
        assert!(evaluate_condition(&make_test_obj(cond), &state));
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
        assert!(!evaluate_condition(&make_test_obj(cond.clone()), &state)); // pas débloqué
        state.unlocked_modes[1] = true;
        assert!(evaluate_condition(&make_test_obj(cond), &state));
    }

    #[test]
    fn already_met_objective_completes_at_assignment() {
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
                active_time: 0.0,
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
                active_time: 0.0,
            },
        ];

        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);

        // "a" n'est pas encore complété (0 minerais < 10 requis)
        let results = tracker.update(&state, 0.0);
        assert_eq!(results.len(), 0);

        // "a" est débloqué (pas de prérequis), "b" ne l'est pas ("a" non complété)
        let unlocked = tracker.unlocked_objectives();
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].id, "a");

        // On obtient 10 minerais → "a" est complété, et "b" (required: 0,
        // toujours vrai) dont le prérequis vient d'être satisfait est
        // **déjà rempli au moment où il est désigné** : il est marqué comme
        // réalisé dans la même passe, puis on passe à l'objectif suivant.
        // Les deux bannières sont mises en file (une à la fois).
        state.resources.minerals = 10;
        let results = tracker.update(&state, 0.0);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a");
        assert_eq!(results[1].id, "b");
        assert!(tracker.notification_timer > 0.0);
        assert_eq!(tracker.last_completed_title.as_deref(), Some("A"));

        // les deux objectifs sont marqués comme réalisés : plus rien à faire
        let results = tracker.update(&state, 0.0);
        assert_eq!(results.len(), 0);

        // quand la bannière de "a" expire, celle de "b" s'affiche à son tour
        tracker.tick(tracker.notification_timer + 0.01);
        assert!(tracker.notification_timer > 0.0);
        assert_eq!(tracker.last_completed_title.as_deref(), Some("B"));
    }

    #[test]
    fn state_change_after_assignment_does_not_uncomplete() {
        // Cas réel : à quai, le joueur dépense ses minerais au magasin après
        // qu'un objectif (déjà satisfait à l'assignation) a été complété.
        // L'évaluation se fait à l'assignation, pas plus tard : la dépense
        // ultérieure ne doit pas empêcher la complétion.
        let mut tracker = ObjectiveTracker::default();
        tracker.objectives = vec![TrackedObjective {
            id: "mine".to_string(),
            title: "Mine".to_string(),
            description: "desc".to_string(),
            prerequisites: vec![],
            condition: JsonCondition {
                condition_type: "CollectMinerals".to_string(),
                required: 10,
                ..Default::default()
            },
            reward: JsonReward::default(),
            completed: false,
            active_time: 0.0,
        }];

        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);

        // le joueur a déjà 12 minerais quand l'objectif est désigné
        state.resources.minerals = 12;
        let results = tracker.update(&state, 0.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mine");
        assert!(tracker.completed_ids.contains("mine"));

        // puis il dépense 10 minerais au magasin : l'objectif reste réalisé
        state.resources.minerals = 2;
        let results = tracker.update(&state, 0.0);
        assert_eq!(results.len(), 0);
        assert!(tracker.completed_ids.contains("mine"));
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
            active_time: 0.0,
        }];

        let state = GameState::new();
        let r1 = tracker.update(&state, 0.0);
        assert_eq!(r1.len(), 1);
        let r2 = tracker.update(&state, 0.0);
        assert_eq!(r2.len(), 0); // déjà complété
    }

    #[test]
    fn evaluate_survive_time_tracks_duration_and_resets_on_death() {
        let mut tracker = ObjectiveTracker::default();
        tracker.objectives = vec![TrackedObjective {
            id: "surv".to_string(),
            title: "Survive Test".to_string(),
            description: "desc".to_string(),
            prerequisites: vec![],
            condition: JsonCondition {
                condition_type: "SurviveTime".to_string(),
                seconds: 5.0,
                ..Default::default()
            },
            reward: JsonReward::default(),
            completed: false,
            active_time: 0.0,
        }];

        let mut state = GameState::new();

        // 1. Tick 3 secondes -> non complété (active_time = 3.0 < 5.0)
        let res = tracker.update(&state, 3.0);
        assert_eq!(res.len(), 0);
        assert_eq!(tracker.objectives[0].active_time, 3.0);

        // 2. Éjection cosmonaute (mort/respawn) -> active_time réinitialisé à 0.0
        state.cosmonaut_active = true;
        let res = tracker.update(&state, 1.0);
        assert_eq!(res.len(), 0);
        assert_eq!(tracker.objectives[0].active_time, 0.0);

        // 3. Joueur secouru et en vol -> tick 6 secondes -> complété (active_time = 6.0 >= 5.0)
        state.cosmonaut_active = false;
        let res = tracker.update(&state, 6.0);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "surv");
        assert!(tracker.objectives[0].completed);
    }

    #[test]
    fn evaluate_unlock_movement_mode_validates_when_unlocked_or_active() {
        let mut tracker = ObjectiveTracker::default();
        tracker.objectives = vec![TrackedObjective {
            id: "mode_inertial".to_string(),
            title: "Nouveau Mode de Vol".to_string(),
            description: "Achetez le mode de déplacement Inerte au magasin.".to_string(),
            prerequisites: vec![],
            condition: JsonCondition {
                condition_type: "UnlockMovementMode".to_string(),
                mode: 0,
                ..Default::default()
            },
            reward: JsonReward::default(),
            completed: false,
            active_time: 0.0,
        }];

        let mut state = GameState::new();
        state.scenario = crate::scenario::ScenarioId::Progression;
        crate::scenario::apply_start(&mut state);

        // Au départ en Progression, seul REALISTIC (3) est débloqué ([false, false, false, true])
        // INERTIAL (0) n'est pas encore débloqué
        let res = tracker.update(&state, 0.0);
        assert_eq!(res.len(), 0);

        // Le joueur achète le mode INERTIAL (0) au magasin -> unlocked_modes[0] = true
        state.unlocked_modes[crate::config::MOVING_MODE_INERTIAL as usize] = true;
        let res = tracker.update(&state, 0.0);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "mode_inertial");
        assert!(tracker.objectives[0].completed);
    }
}
