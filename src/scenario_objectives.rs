//! Scénarios orientés objectifs et dépendances.
//!
//! Ce module définit la structure des chaînes d'objectifs (DAG - Graph Orienté
//! Acyclique), où chaque objectif peut avoir un ou plusieurs prérequis.
//! Ce fichier est mis à jour et généré par l'outil dédié
//! `tools/scenario-editor/index.html`.

use std::collections::HashSet;

/// Condition de réussite d'un objectif.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ObjectiveCondition {
    /// Détruire un nombre cumulé d'astéroïdes.
    DestroyAsteroids { required: u32 },
    /// Collecter un nombre cumulé de minerais/gemmes.
    CollectMinerals { required: u32 },
    /// Atteindre un niveau de réputation.
    ReachReputation { required: f64 },
    /// Effectuer un nombre d'accostages réussis à la station.
    DockAtStation { required: u32 },
    /// Débloquer un niveau d'amélioration d'atelier ("Fuel", "Ammo", "Cargo").
    BuyUpgrade { track: &'static str, level: i32 },
    /// Débloquer un mode de déplacement spécifique (index 0..3).
    UnlockMovementMode { mode: i32 },
    /// Survivre pendant une durée donnée en secondes.
    SurviveTime { seconds: f64 },
    /// Atteindre un nombre de tirs réussis avec une précision minimale (0.0..1.0).
    PrecisionShooting { hits: u32, min_precision: f64 },
}

/// Récompense octroyée lors de la validation d'un objectif.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ObjectiveReward {
    /// Minerais bonus.
    Minerals(i32),
    /// Points de réputation bonus.
    Reputation(f64),
    /// Carburant offert.
    Fuel(f64),
    /// Munitions offertes.
    Ammo(i32),
    /// Déblocage offert d'un mode de déplacement.
    UnlockMode(i32),
    /// Victoire finale du scénario.
    Victory,
    /// Aucune récompense directe (jalon d'étape).
    None,
}

/// Spécification d'un objectif individuel dans le graphe de dépendances.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ObjectiveSpec {
    /// Identifiant unique (ex. "obj_mine_10_gold").
    pub id: &'static str,
    /// Nom court (ex. "Premier Minage").
    pub title: &'static str,
    /// Description explicative (ex. "Récoltez 10 minerais d'or dans la ceinture").
    pub description: &'static str,
    /// Identifiants des objectifs prérequis qui doivent être complétés avant d'activer cet objectif.
    pub prerequisites: &'static [&'static str],
    /// Condition de validation.
    pub condition: ObjectiveCondition,
    /// Récompense.
    pub reward: ObjectiveReward,
}

/// Définition complète d'une chaîne de scénario avec objectifs interdépendants.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ScenarioChain {
    /// Identifiant unique de la chaîne (ex. "campaign_mining").
    pub id: &'static str,
    /// Nom affiché du scénario.
    pub name: &'static str,
    /// Description générale du scénario.
    pub description: &'static str,
    /// Couleur ARGB d'accent du scénario.
    pub rules_color: u32,
    /// Liste de tous les objectifs composants le graphe.
    pub objectives: &'static [ObjectiveSpec],
}

#[allow(dead_code)]
impl ScenarioChain {
    /// Vérifie la validité du DAG d'objectifs (absence de cycle, références valides).
    pub fn validate_dag(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for obj in self.objectives {
            if !ids.insert(obj.id) {
                return Err(format!("Identifiant d'objectif en double : '{}'", obj.id));
            }
        }

        // Vérifier que tous les prérequis existent
        for obj in self.objectives {
            for &pre in obj.prerequisites {
                if !ids.contains(pre) {
                    return Err(format!(
                        "Objectif '{}' référence un prérequis inexistant : '{}'",
                        obj.id, pre
                    ));
                }
                if pre == obj.id {
                    return Err(format!(
                        "L'objectif '{}' ne peut pas dépendre de lui-même",
                        obj.id
                    ));
                }
            }
        }

        // Détection de cycles par tri topologique / DFS
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for obj in self.objectives {
            if !visited.contains(obj.id) {
                if self.has_cycle_dfs(obj.id, &mut visited, &mut rec_stack) {
                    return Err(format!(
                        "Dépendance cyclique détectée impliquant l'objectif '{}'",
                        obj.id
                    ));
                }
            }
        }

        Ok(())
    }

    fn has_cycle_dfs(
        &self,
        node_id: &'static str,
        visited: &mut HashSet<&'static str>,
        rec_stack: &mut HashSet<&'static str>,
    ) -> bool {
        visited.insert(node_id);
        rec_stack.insert(node_id);

        if let Some(obj) = self.objectives.iter().find(|o| o.id == node_id) {
            for &pre in obj.prerequisites {
                if !visited.contains(pre) {
                    if self.has_cycle_dfs(pre, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(pre) {
                    return true;
                }
            }
        }

        rec_stack.remove(node_id);
        false
    }

    /// Retourne la liste des objectifs actuellement débloqués (dont tous les prérequis sont satisfaits).
    pub fn unlocked_objectives<'a>(
        &'a self,
        completed_ids: &HashSet<&str>,
    ) -> Vec<&'a ObjectiveSpec> {
        self.objectives
            .iter()
            .filter(|obj| {
                !completed_ids.contains(obj.id)
                    && obj
                        .prerequisites
                        .iter()
                        .all(|pre| completed_ids.contains(pre))
            })
            .collect()
    }
}

// ============================================================================
// Données générées pour la campagne d'apprentissage "Campagne de Prospection"
// ============================================================================

#[allow(dead_code)]
pub const PROLOGUE_OBJECTIVES: &[ObjectiveSpec] = &[
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
pub const CAMPAIGN_SCENARIO_CHAIN: ScenarioChain = ScenarioChain {
    id: "campaign_prospector",
    name: "CAMPAGNE DE PROSPECTION",
    description: "Séquence d'objectifs guidée pour devenir un prospecteur chevronné.",
    rules_color: 0xFF39FF88,
    objectives: PROLOGUE_OBJECTIVES,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_dag() {
        assert!(CAMPAIGN_SCENARIO_CHAIN.validate_dag().is_ok());
    }

    #[test]
    fn test_unlocked_objectives() {
        let mut completed = HashSet::new();
        let unlocked = CAMPAIGN_SCENARIO_CHAIN.unlocked_objectives(&completed);
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].id, "step_first_dock");

        completed.insert("step_first_dock");
        let unlocked = CAMPAIGN_SCENARIO_CHAIN.unlocked_objectives(&completed);
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].id, "step_mine_gems");
    }

    #[test]
    fn test_cycle_detection() {
        static CYCLIC_OBJECTIVES: &[ObjectiveSpec] = &[
            ObjectiveSpec {
                id: "A",
                title: "A",
                description: "",
                prerequisites: &["B"],
                condition: ObjectiveCondition::DockAtStation { required: 1 },
                reward: ObjectiveReward::None,
            },
            ObjectiveSpec {
                id: "B",
                title: "B",
                description: "",
                prerequisites: &["A"],
                condition: ObjectiveCondition::DockAtStation { required: 1 },
                reward: ObjectiveReward::None,
            },
        ];

        let chain = ScenarioChain {
            id: "cyclic",
            name: "Cyclic",
            description: "",
            rules_color: 0,
            objectives: CYCLIC_OBJECTIVES,
        };

        assert!(chain.validate_dag().is_err());
    }
}
