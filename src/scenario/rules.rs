//! Règles affichées à l'écran titre (extrait de `scenario.rs`) : la ligne
//! `[ RULES : … ]` est dérivée des **données** du scénario (`scenario_rules`,
//! valeurs chiffrées en surbrillance dans la couleur propre du scénario -
//! jaune Progression, cyan Survival), et la ligne `[ SAVE : … ]` montre la
//! progression enregistrée du scénario (`save_summary_segments`,
//! `save_summary`). Utilisées par `title.rs` ; les fonctions sont pures et
//! testables sans macroquad.
use super::*;

// ─── Règles affichées (écran titre) ─────────────────────────────────────────

/// Segment de la ligne des règles (écran titre) : un libellé discret ou une
/// valeur chiffrée mise en évidence (coût, vies, bouclier, dégâts, durée,
/// rang) - colorée à l'affichage de la couleur du scénario (`color`) pour
/// faire ressortir ce qui change quand on bascule de scénario (N/B/1-3).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleSegment {
    /// Texte du segment.
    pub text: String,
    /// Couleur ARGB du segment : `Some` = valeur mise en évidence, dans la
    /// couleur du scénario (voir `Scenario::rules_color`) ; `None` = texte
    /// par défaut (blanc).
    pub color: Option<u32>,
}

/// Règles du scénario `id`, découpées en segments (voir `RuleSegment`) pour
/// l'écran titre : dérivées des données du scénario - coûts des modes,
/// carburant/munitions, vies, bouclier, dégâts, invulnérabilité, rangs. Les
/// valeurs portent `color = Some(couleur du scénario)`, les libellés `None`.
/// Fonction pure (tests).
pub fn scenario_rules(id: ScenarioId) -> Vec<RuleSegment> {
    let s = scenario(id);
    let mut out = Vec::new();
    let label = |out: &mut Vec<RuleSegment>, text: &str| {
        if !text.is_empty() {
            out.push(RuleSegment {
                text: text.to_string(),
                color: None,
            });
        }
    };
    let value = |out: &mut Vec<RuleSegment>, text: String| {
        out.push(RuleSegment {
            text,
            color: Some(s.rules_color),
        });
    };
    match id {
        ScenarioId::FreePlay => {
            label(
                &mut out,
                "aucun coût - carburant/munitions illimités, tous les modes débloqués",
            );
        }
        ScenarioId::Custom(_) => {
            if s.lives > 0 {
                value(&mut out, s.lives.to_string());
                label(
                    &mut out,
                    &format!(" vie{}, bouclier ", if s.lives > 1 { "s" } else { "" }),
                );
                value(&mut out, format!("{}", s.shield_capacity));
            } else if s.has_economy {
                label(&mut out, "économie personnalisée");
            } else {
                label(&mut out, "mode personnalisé");
            }
        }
        ScenarioId::Progression => {
            label(&mut out, "modes payants : ");
            let costs = mode_costs_pairs(&s);
            for (i, (name, cost)) in costs.iter().enumerate() {
                if i > 0 {
                    label(&mut out, ", ");
                }
                value(&mut out, format!("{} {}", name, cost));
            }
            label(
                &mut out,
                " crédits ; carburant/munitions payants ; rangs : ",
            );
            if let Some(first) = PROGRESSION_RANKS.first() {
                value(&mut out, first.name.to_string());
            }
            // « → » : la police embarquée (DejaVu Sans Mono) possède le glyphe
            label(&mut out, " → ");
            if let Some(last) = PROGRESSION_RANKS.last() {
                value(&mut out, last.name.to_string());
            }
        }
        ScenarioId::Survival => {
            value(&mut out, s.lives.to_string());
            label(
                &mut out,
                &format!(" vie{}, bouclier ", if s.lives > 1 { "s" } else { "" }),
            );
            value(&mut out, format!("{}", s.shield_capacity));
            label(&mut out, ", dégâts ×");
            value(&mut out, format!("{}", s.damage_multiplier));
            label(&mut out, ", ");
            value(&mut out, format!("{}", s.respawn_invulnerability));
            label(&mut out, " s d'invulnérabilité après respawn");
        }
    }
    out
}

/// Texte complet des règles (segments concaténés, sans coloration) - réservé
/// aux tests (l'écran titre affiche les segments colorés).
#[cfg(test)]
pub fn scenario_rules_text(id: ScenarioId) -> String {
    scenario_rules(id).iter().map(|s| s.text.as_str()).collect()
}

/// Paires (nom, coût) des modes de déplacement payants (coût > 0).
fn mode_costs_pairs(s: &Scenario) -> Vec<(&'static str, i32)> {
    s.mode_costs
        .iter()
        .enumerate()
        .filter(|(_, cost)| **cost > 0)
        .map(|(i, cost)| (mode_label(i as i32), *cost))
        .collect()
}

/// « 4 WAYS 30, DIRECTIONAL 45 crédits » - coûts des modes de déplacement
/// payants (coût 0 = mode déjà débloqué, omis). Réservé aux tests (les règles
/// de l'écran titre sont découpées en segments par `scenario_rules`).
#[cfg(test)]
pub(crate) fn mode_costs_text(s: &Scenario) -> String {
    let costs = mode_costs_pairs(s);
    if costs.is_empty() {
        "aucun".to_string()
    } else {
        costs
            .iter()
            .map(|(name, cost)| format!("{} {}", name, cost))
            .collect::<Vec<_>>()
            .join(", ")
            + " crédits"
    }
}

/// Résumé segmenté de la progression **enregistrée** du scénario courant,
/// affiché à l'écran titre sous les règles : `state.resources` contient déjà
/// la sauvegarde restaurée (voir `load_progression`) - crédits, modes
/// débloqués et réputation (+ rang) en Progression, vies et bouclier en
/// Survival ; jeu libre : aucune sauvegarde. Découpé en segments comme
/// `scenario_rules` : les valeurs (crédits, modes, réputation, rang, vies,
/// bouclier) portent `color = Some(couleur du scénario)`, les libellés `None`.
/// Fonction pure (tests).
pub fn save_summary_segments(state: &GameState) -> Vec<RuleSegment> {
    let color = scenario(state.scenario).rules_color;
    let value = |text: String| RuleSegment {
        text,
        color: Some(color),
    };
    let label = |text: &str| RuleSegment {
        text: text.to_string(),
        color: None,
    };
    // record (high-score) du scénario : affiché pour TOUS les scénarios (y
    // compris le jeu libre, qui n'a pas d'autre sauvegarde) - le score
    // composite (crédits gagnés + astéroïdes + objectifs) et son record
    // sont la trace de progression universelle
    let mut high_score = vec![
        label(" - record "),
        value(state.high_score.to_string()),
    ];
    let mut out = match state.scenario {
        ScenarioId::FreePlay => vec![label("aucune sauvegarde (jeu libre)")],
        ScenarioId::Custom(_) => {
            let mut out = vec![];
            if has_economy(state) {
                out.push(label("crédits "));
                out.push(value(state.resources.credits.to_string()));
            }
            if has_survival(state) {
                if !out.is_empty() {
                    out.push(label(" - "));
                }
                out.push(value(state.resources.lives.to_string()));
                out.push(label(if state.resources.lives > 1 {
                    " vies - bouclier "
                } else {
                    " vie - bouclier "
                }));
                out.push(value(format!("{:.1}", state.resources.shield)));
            }
            if out.is_empty() {
                out.push(label("(pas de progression)").to_owned());
            }
            out
        }
        ScenarioId::Progression => {
            let unlocked = state.unlocked_modes.iter().filter(|&&u| u).count();
            let mut out = vec![
                label("crédits "),
                value(state.resources.credits.to_string()),
                label(" - modes "),
                value(format!("{}/{}", unlocked, MOVING_MODE_COUNT)),
                label(" - réputation "),
                value((state.resources.reputation as i32).to_string()),
            ];
            if let Some(rank) = current_rank(state) {
                out.push(value(format!(" ({})", rank)));
            }
            out
        }
        ScenarioId::Survival => vec![
            value(state.resources.lives.to_string()),
            label(if state.resources.lives > 1 {
                " vies - bouclier "
            } else {
                " vie - bouclier "
            }),
            value(format!("{:.1}", state.resources.shield)),
        ],
    };
    out.append(&mut high_score);
    out
}

/// Texte complet du résumé de sauvegarde (segments concaténés, sans
/// coloration) - réservé aux tests (l'écran titre affiche les segments
/// colorés, voir `save_summary_segments`).
#[cfg(test)]
pub fn save_summary(state: &GameState) -> String {
    save_summary_segments(state)
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}
