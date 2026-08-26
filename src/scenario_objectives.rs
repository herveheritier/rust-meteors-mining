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
        id: "def_alert",
        title: "Alerte Météores",
        description: "L'essaim approche de la base : abattez 10 météores avant qu'ils n'atteignent le périmètre.",
        prerequisites: &[],
        condition: ObjectiveCondition::DestroyAsteroids { required: 10 },
        reward: ObjectiveReward::Credits(10),
    },
    ObjectiveSpec {
        id: "def_precision",
        title: "Tir de Défense",
        description: "Les munitions sont limitées : réussissez 15 tirs au but avec au moins 60 % de précision.",
        prerequisites: &["def_alert"],
        condition: ObjectiveCondition::DestroyAsteroids { required: 10 },
        reward: ObjectiveReward::Fuel(50.0),
    },
    ObjectiveSpec {
        id: "def_rearm",
        title: "Rotation à la Base",
        description: "Revenez à la station pour décharger le minerai et réarmer entre deux vagues.",
        prerequisites: &["def_alert"],
        condition: ObjectiveCondition::DockAtStation { required: 1 },
        reward: ObjectiveReward::Ammo(30),
    },
    ObjectiveSpec {
        id: "def_wave2",
        title: "Deuxième Vague",
        description: "Une nouvelle salve se présente : intensifiez la défense et détruisez 35 météores au total.",
        prerequisites: &["def_alert"],
        condition: ObjectiveCondition::DestroyAsteroids { required: 35 },
        reward: ObjectiveReward::Credits(25),
    },
    ObjectiveSpec {
        id: "def_hold",
        title: "Tenir la Ligne",
        description: "Restez en vol 120 secondes sans vous faire détruire : la défense tient.",
        prerequisites: &["def_alert"],
        condition: ObjectiveCondition::SurviveTime { seconds: 120.0 },
        reward: ObjectiveReward::Reputation(20.0),
    },
    ObjectiveSpec {
        id: "def_perimeter",
        title: "Périmètre Sécurisé",
        description: "Les vagues s'enchaînent : gardez le périmètre de la base dégagé en détruisant 70 météores au total.",
        prerequisites: &["def_wave2", "def_precision"],
        condition: ObjectiveCondition::DestroyAsteroids { required: 70 },
        reward: ObjectiveReward::Reputation(40.0),
    },
    ObjectiveSpec {
        id: "def_victory",
        title: "Base en Sécurité",
        description: "La menace est éliminée : détruisez 100 météores au total pour sécuriser définitivement la base.",
        prerequisites: &["def_perimeter", "def_rearm"],
        condition: ObjectiveCondition::DestroyAsteroids { required: 100 },
        reward: ObjectiveReward::Victory,
    },
];

#[allow(dead_code)]
pub const GENERATED_SCENARIO_CHAIN: ScenarioChain = ScenarioChain {
    id: "base_defense",
    name: "DÉFENSE DE LA BASE",
    description: "Des vagues de météores menacent la base : abattez-les avant qu'elles n'atteignent le périmètre, réarmez à la station et tenez jusqu'à la sécurisation complète.",
    rules_color: 0xFFFFA028,
    objectives: GENERATED_OBJECTIVES,
};
