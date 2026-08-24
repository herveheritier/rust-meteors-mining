//! Code généré automatiquement par l'Éditeur de Scénarios DAG.
//! Fichier : src/scenario_objectives.rs
//!
//! NB : ce module est historique - les objectifs sont désormais chargés
//! dynamiquement depuis les fichiers JSON (scenario_loader.rs). Les
//! éléments ci-dessous sont conservés pour la compatibilité de l'outil
//! d'édition qui génère ce fichier.

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ObjectiveCondition {
    DestroyAsteroids { required: u32 },
    CollectCredits { required: u32 },
    ReachReputation { required: f64 },
    DockAtStation { required: u32 },
    BuyUpgrade { track: &'static str, level: i32 },
    UnlockMovementMode { mode: i32 },
    SurviveTime { seconds: f64 },
    PrecisionShooting { hits: u32, min_precision: f64 },
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ObjectiveReward {
    Credits(i32),
    Reputation(f64),
    Fuel(f64),
    Ammo(i32),
    UnlockMode(i32),
    Victory,
    None,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ObjectiveSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub prerequisites: &'static [&'static str],
    pub condition: ObjectiveCondition,
    pub reward: ObjectiveReward,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ScenarioChain {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub rules_color: u32,
    pub objectives: &'static [ObjectiveSpec],
}

#[allow(dead_code)]
pub const GENERATED_OBJECTIVES: &[ObjectiveSpec] = &[
    ObjectiveSpec {
        id: "step_start",
        title: "Premier Objectif",
        description: "Survivre ",
        prerequisites: &[],
        condition: ObjectiveCondition::SurviveTime { seconds: 30.0 },
        reward: ObjectiveReward::Credits(10),
    },
    ObjectiveSpec {
        id: "obj_mt66lao7",
        title: "Mode Inertiel",
        description: "Acheter le mode de  contrôle  inertial",
        prerequisites: &["step_start"],
        condition: ObjectiveCondition::UnlockMovementMode { mode: 1 },
        reward: ObjectiveReward::Credits(10),
    },
];

#[allow(dead_code)]
pub const GENERATED_SCENARIO_CHAIN: ScenarioChain = ScenarioChain {
    id: "test",
    name: "TEST",
    description: "Description de votre nouveau scénario.",
    rules_color: 0xFF39FF88,
    objectives: GENERATED_OBJECTIVES,
};
