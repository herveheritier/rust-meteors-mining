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
    CollectMinerals { required: u32 },
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
    Minerals(i32),
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
        id: "step_first_dock",
        title: "Premier Accostage",
        description: "Accostez à la station pour initialiser les systèmes de bord.",
        prerequisites: &[],
        condition: ObjectiveCondition::DockAtStation { required: 1 },
        reward: ObjectiveReward::Minerals(5),
    },
    ObjectiveSpec {
        id: "step_mine_gems",
        title: "Récolte Initiale",
        description: "Détruisez des météores et collectez 10 minerais.",
        prerequisites: &["step_first_dock"],
        condition: ObjectiveCondition::CollectMinerals { required: 10 },
        reward: ObjectiveReward::Minerals(10),
    },
    ObjectiveSpec {
        id: "step_reputation",
        title: "Notoriété du Mineur",
        description: "Atteignez 25 points de réputation auprès de la station.",
        prerequisites: &["step_mine_gems"],
        condition: ObjectiveCondition::ReachReputation { required: 25.0 },
        reward: ObjectiveReward::Reputation(10.0),
    },
    ObjectiveSpec {
        id: "step_unlock_inertial",
        title: "Nouveau Mode de Vol",
        description: "Achetez le mode de déplacement Inerte au magasin.",
        prerequisites: &["step_reputation"],
        condition: ObjectiveCondition::UnlockMovementMode { mode: 1 },
        reward: ObjectiveReward::Fuel(50.0),
    },
    ObjectiveSpec {
        id: "step_master_pilot",
        title: "Maître Pilote",
        description: "Détruisez 50 météores avec le nouveau système de navigation.",
        prerequisites: &["step_unlock_inertial"],
        condition: ObjectiveCondition::DestroyAsteroids { required: 50 },
        reward: ObjectiveReward::Victory,
    },
];

#[allow(dead_code)]
pub const GENERATED_SCENARIO_CHAIN: ScenarioChain = ScenarioChain {
    id: "campaign_prospector",
    name: "CAMPAGNE DE PROSPECTION",
    description: "Séquence d'objectifs guidée pour devenir un prospecteur chevronné.",
    rules_color: 0xFF39FF88,
    objectives: GENERATED_OBJECTIVES,
};
