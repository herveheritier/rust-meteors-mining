//! Scénarios de jeu.
//!
//! Un scénario est un ensemble de règles (ressources économiques, verrous de
//! gameplay, récompenses) appliquées par des points d'accroche purs dans la
//! boucle de jeu - voir les appels à `crate::scenario` dans `game.rs`. Les
//! règles sont des **données** (`Scenario`) et les effets des **fonctions
//! pures** testables sans macroquad.
//!
//! Trois scénarios :
//! - `FreePlay` (défaut) : comportement historique du port - pas d'économie,
//!   tous les modes de déplacement disponibles, carburant et munitions
//!   illimités.
//! - `Progression` (l'exemple décrit) : le vaisseau démarre en mode REALISTIC
//!   (gratuit, coût 0 paramétré dans l'outil) ; il doit accumuler des crédits
//!   (minerais collectés sur les astéroïdes, déchargés à la station) pour
//!   débloquer les modes payants (INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45) ;
//!   chaque poussée consomme du carburant et chaque tir des munitions
//!   (remplis à la station, contre crédits) ; détruire des astéroïdes
//!   augmente la réputation, d'autant plus que la précision de tir est bonne,
//!   et décharger de la cargaison en rapporte aussi (commerce récompensé).
//!   À la station, le **magasin** (bouton SHOP de la boîte DOCK STATION)
//!   permet d'acheter contre crédits des **extensions de vaisseau** :
//!   réservoir de carburant, chargeur de munitions et soute (capacités
//!   augmentées, persistées comme la progression).
//! - `Survival` (preuve que le système s'étend à d'autres mécaniques) : ni
//!   économie ni verrous - le vaisseau a des **vies** et un **bouclier** qui
//!   absorbe les impacts ; quand il est percé, l'impact suivant détruit le
//!   vaisseau : une vie est perdue et il respawne à la station (dernière vie
//!   perdue = fin de partie). Le **multiplicateur de dégâts** aggrave les
//!   impacts (bouclier vidé plus vite).
//!
//! La progression d'un scénario est persistée dans le fichier de config
//! (`persist.rs`, clés `scenario`, `prog_*`) et restaurée au lancement
//! suivant : crédits/modes/réputation/niveaux d.atelier en Progression,
//! vies/bouclier en Survival - chaque scénario n'écrit que ses propres clés.
//! Le carburant et les munitions, eux, repartent pleins à chaque lancement
//! (à la capacité courante, extensions comprises).

use std::io;
use std::path::Path;

use crate::config::{
    MOVING_MODE_COUNT, MOVING_MODE_DIRECTIONAL, MOVING_MODE_REALISTIC, WEAPON_SLOTS,
};
use crate::state::{Element, GameState};

/// Identifiant d'un scénario (choisi à l'écran titre, touche N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioId {
    /// Jeu libre : comportement historique, sans économie.
    FreePlay,
    /// Économie : crédits, carburant/munitions payants, réputation.
    Progression,
    /// Survie : vies, bouclier et multiplicateur de dégâts - sans économie.
    Survival,
    /// Scénario chargé depuis un fichier JSON (scenarios/*.scenario.json,
    /// éditeur de scénarios DAG). L'index désigne le scénario dans la
    /// liste `LOADED_SCENARIOS` du module `scenario_loader`.
    Custom(usize),
}

/// Ressources économiques du joueur (scénarios à économie) et de survie
/// (vies, bouclier).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resources {
    /// Carburant en unités (0 = réservoir vide, plus de poussée).
    pub fuel: f64,
    /// Crédits - la monnaie : obtenus en déchargeant les minerais à la
    /// station, dépensés pour les modes de déplacement, les armes et le
    /// ravitaillement.
    pub credits: i32,
    /// Réputation - augmente avec les astéroïdes détruits et la précision.
    pub reputation: f64,
    /// Vies restantes (scénario Survival) - 0 = partie terminée.
    pub lives: i32,
    /// Bouclier restant (scénario Survival) : absorbe les impacts avant la
    /// coque ; rechargé au respawn.
    pub shield: f64,
    /// Niveau du réservoir de carburant (atelier) : nombre d'extensions
    /// achetées (0 = capacité de base) - Progression.
    pub fuel_level: i32,
    /// Niveau du chargeur de munitions (atelier) - Progression.
    pub ammo_level: i32,
    /// Niveau de la soute (atelier) - Progression.
    pub cargo_level: i32,
    /// Munitions par arme du catalogue (`VAISSEAU_WEAPONS`, index de l'arme ;
    /// catalogue vide = un seul emplacement pour le canon classique de
    /// repli) : chaque arme tire tant que son propre stock n'est pas vide -
    /// 0 = plus de tirs pour cette arme (scénarios à économie ; ignorées en
    /// jeu libre, les munitions y sont illimitées).
    pub weapon_ammo: [i32; WEAPON_SLOTS],
    /// Armes du catalogue **possédées** (achetées au magasin) : `false` = à
    /// acheter (son mesh n'est pas construit sur le vaisseau et elle ne
    /// tire pas) ; toujours `true` en Survival (hors économie) et pour les
    /// armes de base (coût 0). En jeu libre, le vaisseau n'est équipé que
    /// de l'arme 1 (index 0) - les autres armes du catalogue ne sont pas
    /// possédées (voir `weapon_owned`). Le canon classique (catalogue vide)
    /// est toujours possédé.
    pub weapon_owned: [bool; WEAPON_SLOTS],
    /// Radar de bord **possédé** (acheté au magasin) : affiche la minimap
    /// globale (points des météores et des autres formes) - `false` = radar
    /// éteint par défaut en scénario à économie. Hors économie (jeu libre /
    /// Survival / custom sans économie), le radar est **toujours allumé**
    /// (comportement historique, sans achat possible).
    pub radar_owned: bool,
}

/// Types et données de la place de marché - extensions de vaisseau de
/// l'atelier de la station, économie et rangs de réputation (scénario
/// Progression, bouton SHOP de la boîte DOCK STATION). Définis dans
/// `src/marketplace.rs`, un **fichier généré** par l'outil de gestion
/// `tools/marketplace-editor/index.html` : pour ajuster les objets vendus, les
/// prix ou les rangs (seuils, noms, remises), régénérez ce fichier depuis
/// l'éditeur - rien à modifier ici. Réexportés pour l'API publique du module
/// (types, rangs, trois lignes d'atelier et constantes économiques).
pub use crate::marketplace::{
    ReputationRank, ShipUpgrade, UpgradeTrack, DISCOUNT_PRECISION_WEIGHT, PROGRESSION_RANKS,
    AMMO_UPGRADE_TRACK, CARGO_UPGRADE_TRACK, FUEL_UPGRADE_TRACK, AMMO_PRICE, AMMO_STEP,
    ELEMENT_VALUES, FUEL_PRICE, FUEL_STEP, MODE_COSTS, mode_label,
};

/// Ligne d'amélioration du vaisseau à l'atelier (index des trois lignes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeTrackId {
    /// Réservoir de carburant.
    Fuel,
    /// Chargeur de munitions.
    Ammo,
    /// Soute (cargaison de minerais).
    Cargo,
}

/// Ligne d'amélioration vide (jeu libre, Survival : pas d'atelier).
const EMPTY_UPGRADE_TRACK: UpgradeTrack = UpgradeTrack {
    label: "",
    base: 0,
    tiers: &[],
};

/// Règles d'un scénario (données, ex `PROGRESSION_SCENARIO`).
#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    /// Nom affiché (écran titre).
    pub name: &'static str,
    /// Courte description (écran titre).
    pub description: &'static str,
    /// `true` : carburant/munitions/crédits actifs ; `false` : illimités et
    /// tous les modes débloqués (comportement historique).
    pub has_economy: bool,
    /// Rangs de réputation (paliers débloqués par la réputation, seuils
    /// croissants) - vide en jeu libre (aucun rang).
    pub ranks: &'static [ReputationRank],
    /// Carburant au départ (la capacité courante vient de `fuel_upgrades`,
    /// base + extensions d'atelier).
    pub start_fuel: f64,
    /// Consommation de carburant par seconde de poussée (moteur allumé).
    pub fuel_per_second: f64,
    /// Munitions au départ (capacité courante : `ammo_upgrades`).
    pub start_ammo: i32,
    /// Munitions consommées par tir.
    pub ammo_per_shot: i32,
    /// Coût en crédits pour débloquer chaque mode de déplacement (index
    /// `MOVING_MODE_*` ; 0 = déjà débloqué).
    pub mode_costs: [i32; MOVING_MODE_COUNT as usize],
    /// Réputation gagnée par astéroïde détruit (hors bonus de précision).
    pub reputation_per_asteroid: f64,
    /// Bonus de précision : gain × (1 + poids × précision), précision en 0..1.
    pub reputation_precision_weight: f64,
    /// Réputation gagnée par minerai déchargé à la station (commerce - les
    /// astéroïdes détruits récompensent le tir, la cargaison le commerce).
    pub reputation_per_mineral: f64,
    /// Poids de la précision de tir sur la remise de réputation : la remise
    /// du rang est multipliée par `1 + poids × précision` (voir
    /// `DISCOUNT_PRECISION_WEIGHT` de `src/marketplace.rs`).
    pub discount_precision_weight: f64,
    /// Prix (crédits) d.un plein par pas de `fuel_step` unités.
    pub fuel_price: i32,
    /// Pas de ravitaillement en carburant (unités par plein facturé).
    pub fuel_step: f64,
    /// Nombre de vies (scénario Survival ; 0 = illimité/classique).
    pub lives: i32,
    /// Capacité du bouclier (points absorbés avant la coque, scénario
    /// Survival) - 0 = pas de bouclier.
    pub shield_capacity: f64,
    /// Multiplicateur des dégâts subis (bouclier puis coque, scénario
    /// Survival) - 1.0 en classique.
    pub damage_multiplier: f64,
    /// Durée (secondes) d'invulnérabilité après un respawn (scénario
    /// Survival) : les impacts sont absorbés sans toucher au bouclier -
    /// 0.0 en classique.
    pub respawn_invulnerability: f64,
    /// Couleur ARGB des valeurs mises en évidence dans les lignes RULES / SAVE
    /// de l'écran titre (coûts, vies, bouclier, rangs…) - propre à chaque
    /// scénario, pour que le changement de stat saute aux yeux au basculement
    /// (N/B/1-3).
    pub rules_color: u32,
    /// Ligne « réservoir de carburant » de l'atelier de la station (scénario
    /// Progression) - extensions achetées en crédits ; vide ailleurs (pas
    /// d'atelier). La capacité courante (`fuel_capacity`) est la base + les
    /// bonus des extensions possédées.
    pub fuel_upgrades: UpgradeTrack,
    /// Ligne « chargeur de munitions » de l'atelier (scénario Progression).
    pub ammo_upgrades: UpgradeTrack,
    /// Ligne « soute » de l'atelier (scénario Progression).
    pub cargo_upgrades: UpgradeTrack,
}

/// Couleurs ARGB d'accent de l'écran titre (valeurs des lignes RULES / SAVE,
/// voir `Scenario::rules_color`) - une par scénario : jaune pour jeu libre /
/// Progression, cyan pour Survival (le changement de couleur marque aussi le
/// basculement).
pub const RULES_COLOR_YELLOW: u32 = 0xFFFFFF00;
pub const RULES_COLOR_CYAN: u32 = 0xFF00FFFF;

/// Règles du jeu libre (défaut) - aucune économie.
pub const FREE_PLAY_SCENARIO: Scenario = Scenario {
    name: "FREE PLAY",
    description: "classique, sans économie",
    has_economy: false,
    ranks: &[],
    start_fuel: 0.0,
    fuel_per_second: 0.0,
    start_ammo: 0,
    ammo_per_shot: 0,
    mode_costs: [0; MOVING_MODE_COUNT as usize],
    reputation_per_asteroid: 0.0,    reputation_precision_weight: 0.0,
    reputation_per_mineral: 0.0,
    discount_precision_weight: 0.0,
    fuel_price: 0,
    fuel_step: 10.0,
    lives: 0,
    shield_capacity: 0.0,
    damage_multiplier: 1.0,
    respawn_invulnerability: 0.0,
    rules_color: RULES_COLOR_YELLOW,
    fuel_upgrades: EMPTY_UPGRADE_TRACK,
    ammo_upgrades: EMPTY_UPGRADE_TRACK,
    cargo_upgrades: EMPTY_UPGRADE_TRACK,
};

/// Règles du scénario d'exemple « Progression » (voir l'en-tête du module).
pub const PROGRESSION_SCENARIO: Scenario = Scenario {
    name: "PROGRESSION",
    description: "économie : crédits, carburant, réputation",
    has_economy: true,
    ranks: PROGRESSION_RANKS,
    start_fuel: 100.0,
    fuel_per_second: 2.0, // ~50 s de poussée continue au départ
    start_ammo: 30,
    ammo_per_shot: 1,
    mode_costs: MODE_COSTS, // INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45, REALISTIC gratuit
    reputation_per_asteroid: 1.0,
    reputation_precision_weight: 2.0, // 100 % de précision → ×3 par astéroïde
    reputation_per_mineral: 0.1, // 10 minerais déchargés → +1 de réputation
    discount_precision_weight: DISCOUNT_PRECISION_WEIGHT, // précision sur la remise - src/marketplace.rs
    fuel_price: FUEL_PRICE, // 1 minerai pour 10 unités - src/marketplace.rs
    fuel_step: FUEL_STEP,
    lives: 0,
    shield_capacity: 0.0,
    damage_multiplier: 1.0,
    respawn_invulnerability: 0.0,
    rules_color: RULES_COLOR_YELLOW,
    fuel_upgrades: FUEL_UPGRADE_TRACK,
    ammo_upgrades: AMMO_UPGRADE_TRACK,
    cargo_upgrades: CARGO_UPGRADE_TRACK,
};

/// Règles du scénario « Survival » - vies, bouclier, dégâts (voir l'en-tête
/// du module) : ni économie ni verrous de modes.
pub const SURVIVAL_SCENARIO: Scenario = Scenario {
    name: "SURVIVAL",
    description: "vies, bouclier, dégâts majorés",
    has_economy: false,
    ranks: &[],
    start_fuel: 0.0,
    fuel_per_second: 0.0,
    start_ammo: 0,
    ammo_per_shot: 0,
    mode_costs: [0; MOVING_MODE_COUNT as usize],
    reputation_per_asteroid: 0.0,
    reputation_precision_weight: 0.0,
    reputation_per_mineral: 0.0,
    discount_precision_weight: 0.0,
    fuel_price: 0,
    fuel_step: 10.0,
    lives: 3,
    shield_capacity: 3.0,
    damage_multiplier: 1.0,
    respawn_invulnerability: 2.0, // 2 s de répit après chaque respawn
    rules_color: RULES_COLOR_CYAN,
    fuel_upgrades: EMPTY_UPGRADE_TRACK,
    ammo_upgrades: EMPTY_UPGRADE_TRACK,
    cargo_upgrades: EMPTY_UPGRADE_TRACK,
};

/// Règles du scénario `id`.
pub fn scenario(id: ScenarioId) -> Scenario {
    match id {
        ScenarioId::FreePlay => FREE_PLAY_SCENARIO,
        ScenarioId::Progression => PROGRESSION_SCENARIO,
        ScenarioId::Survival => SURVIVAL_SCENARIO,
        ScenarioId::Custom(i) => {
            *crate::scenario_loader::loaded_rules(i).unwrap_or(&FREE_PLAY_SCENARIO)
        }
    }
}

/// Nombre total de scénarios disponibles (3 built-in + N chargés depuis JSON).
pub fn total_scenario_count() -> usize {
    3 + crate::scenario_loader::loaded_count()
}

/// Convertit un index global (0 = FreePlay, 1 = Progression, 2 = Survival,
/// 3+ = custom) en `ScenarioId`. Renvoie `FreePlay` si l'index est hors bornes.
pub fn scenario_id_from_index(index: usize) -> ScenarioId {
    match index {
        0 => ScenarioId::FreePlay,
        1 => ScenarioId::Progression,
        2 => ScenarioId::Survival,
        i => ScenarioId::Custom(i - 3),
    }
}

/// Convertit un `ScenarioId` en index global (voir `scenario_id_from_index`).
pub fn scenario_index(id: ScenarioId) -> usize {
    match id {
        ScenarioId::FreePlay => 0,
        ScenarioId::Progression => 1,
        ScenarioId::Survival => 2,
        ScenarioId::Custom(i) => i + 3,
    }
}

/// Le scénario gère-t-il une économie ? (`false` = comportement historique :
/// ressources illimitées, modes tous débloqués.)
pub fn has_economy(state: &GameState) -> bool {
    scenario(state.scenario).has_economy
}

/// Le **radar de bord** est-il actif (minimap globale affichée) ? Hors
/// économie (jeu libre, Survival, custom sans économie) : toujours `true`
/// (comportement historique - la minimap est allumée par défaut). En
/// scénario à économie : `true` seulement si le radar a été **acheté au
/// magasin** (`Resources::radar_owned`) - éteint par défaut.
pub fn has_radar(state: &GameState) -> bool {
    !has_economy(state) || state.resources.radar_owned
}

/// Le scénario gère-t-il la survie (vies + bouclier) ? - déduit du nombre de
/// vies : `lives > 0` (Survival), sinon classique (FreePlay/Progression).
pub fn has_survival(state: &GameState) -> bool {
    scenario(state.scenario).lives > 0
}

/// Indique si un `ScenarioId` est un scénario custom (chargé depuis JSON).
#[allow(dead_code)]
pub fn is_custom(id: ScenarioId) -> bool {
    matches!(id, ScenarioId::Custom(_))
}

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
            label(&mut out, "aucun coût - carburant/munitions illimités, tous les modes débloqués");
        }
        ScenarioId::Custom(_) => {
            if s.lives > 0 {
                value(&mut out, s.lives.to_string());
                label(&mut out, &format!(
                    " vie{}, bouclier ",
                    if s.lives > 1 { "s" } else { "" }
                ));
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
            label(&mut out, " crédits ; carburant/munitions payants ; rangs : ");
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
            label(&mut out, &format!(
                " vie{}, bouclier ",
                if s.lives > 1 { "s" } else { "" }
            ));
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
fn mode_costs_text(s: &Scenario) -> String {
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
    match state.scenario {
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
    }
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

/// Mode de déplacement de départ du scénario `id` : REALISTIC en Progression,
/// DIRECTIONAL - le défaut historique - en jeu libre et en Survival. Utilisé
/// par `apply_start` (et par le magasin, qui ne doit jamais débloquer un mode
/// gratuitement : le RESET des réglages ne touche plus au mode).
pub fn start_mode(id: ScenarioId) -> i32 {
    match id {
        ScenarioId::FreePlay => MOVING_MODE_DIRECTIONAL,
        ScenarioId::Progression => MOVING_MODE_REALISTIC,
        ScenarioId::Survival => MOVING_MODE_DIRECTIONAL,
        ScenarioId::Custom(_) => MOVING_MODE_REALISTIC,
    }
}

/// Sélectionne un scénario donné (écran titre, touches 1/2/3) et applique
/// ses règles de départ (`apply_start`). La restauration/enregistrement de la
/// progression reste à la charge de l'appelant (écran titre).
pub fn select_scenario(state: &mut GameState, id: ScenarioId) {
    state.scenario = id;
    apply_start(state);
}

/// Bascule de scénario (écran titre, touche N) - jeu libre → Progression →
/// Survival → (scénarios custom) → jeu libre - et applique ses règles de
/// départ. Les scénarios JSON chargés depuis `scenarios/` sont inclus dans
/// la boucle après les 3 built-in.
pub fn cycle_scenario(state: &mut GameState) {
    let idx = scenario_index(state.scenario);
    let total = total_scenario_count();
    let next_idx = (idx + 1) % total;
    select_scenario(state, scenario_id_from_index(next_idx));
}

/// Bascule au scénario **précédent** (écran titre, touche B - inverse de N) :
/// jeu libre → (scénarios custom en inverse) → Survival → Progression →
/// jeu libre.
pub fn cycle_scenario_back(state: &mut GameState) {
    let idx = scenario_index(state.scenario);
    let total = total_scenario_count();
    let prev_idx = if idx == 0 { total - 1 } else { idx - 1 };
    select_scenario(state, scenario_id_from_index(prev_idx));
}

/// Applique les règles de départ du scénario courant : ressources initiales,
/// modes débloqués et (en Progression) mode de déplacement imposé (REALISTIC).
/// Appelé au lancement (après les réglages persistés) et au changement de
/// scénario. En jeu libre et en Survival, le mode mémorisé (fichier de
/// config) est conservé. Remet aussi `game_over` à faux (nouvelle partie).
pub fn apply_start(state: &mut GameState) {
    let s = scenario(state.scenario);
    state.game_over = false;
    state.invulnerable = 0.0; // pas d'invulnérabilité en début de partie
    // compteurs d'avancement des objectifs (météores détruits, accostages,
    // tirs) : remis à zéro pour une nouvelle partie - la progression
    // enregistrée (clés `prog_*`) est surimposée juste après par
    // `load_progression` au lancement
    state.meteors_destroyed = 0;
    state.docking_count = 0;
    state.bullets_fired = 0;
    state.bullets_lost = 0;
    match state.scenario {
        ScenarioId::FreePlay | ScenarioId::Survival => {
            // jeu libre : aucune ressource ; Survival : vies + bouclier pleins
            state.resources = Resources {
                lives: s.lives,
                shield: s.shield_capacity,
                ..Resources::default()
            };
            state.unlocked_modes = [true; MOVING_MODE_COUNT as usize];
        }
        ScenarioId::Custom(ci) if s.has_economy => {
            // Scénario custom avec économie : comme Progression (crédits,
            // carburant/munitions payants, armes à acheter). Les valeurs
            // initiales (crédits, réputation) viennent du JSON éditeur.
            let (json_credits, json_reputation) = crate::scenario_loader
                ::loaded_data(ci)
                .map(|d| (
                    d.json.initial_state.start_credits,
                    d.json.initial_state.start_reputation,
                ))
                .unwrap_or((0, 0.0));
            state.resources = Resources {
                fuel: s.start_fuel,
                credits: json_credits,
                reputation: json_reputation,
                lives: s.lives,
                shield: s.shield_capacity,
                fuel_level: 0,
                ammo_level: 0,
                cargo_level: 0,
                weapon_ammo: [0; WEAPON_SLOTS],
                weapon_owned: [false; WEAPON_SLOTS],
                radar_owned: false,
            };
            for i in 0..weapon_slot_count() {
                if weapon_spec(i).cost == 0 {
                    state.resources.weapon_owned[i] = true;
                    state.resources.weapon_ammo[i] = s.start_ammo;
                }
            }
            let start = start_mode(ScenarioId::Progression);
            state.unlocked_modes = [false; MOVING_MODE_COUNT as usize];
            for (i, unlocked) in state.unlocked_modes.iter_mut().enumerate() {
                *unlocked = MODE_COSTS[i] == 0 || i as i32 == start;
            }
            state.moving_mode = start;
            state.player.cargo_size = cargo_capacity(state);
        }
        ScenarioId::Custom(_) => {
            // Scénario custom sans économie : comme FreePlay/Survival
            state.resources = Resources {
                lives: s.lives,
                shield: s.shield_capacity,
                ..Resources::default()
            };
            state.unlocked_modes = [true; MOVING_MODE_COUNT as usize];
        }
        ScenarioId::Progression => {
            state.resources = Resources {
                fuel: s.start_fuel,
                credits: 0,
                reputation: 0.0,
                lives: 0,
                shield: 0.0,
                fuel_level: 0,
                ammo_level: 0,
                cargo_level: 0,
                weapon_ammo: [0; WEAPON_SLOTS],
                weapon_owned: [false; WEAPON_SLOTS],
                radar_owned: false,
            };
            // Armes équipées au départ : celles dont le coût configuré
            // (outil) est nul (0 = arme de base) - chargées à la capacité
            // courante. Les armes payantes s'achètent au magasin (bouton
            // SHOP de la boîte DOCK STATION).
            for i in 0..weapon_slot_count() {
                if weapon_spec(i).cost == 0 {
                    state.resources.weapon_owned[i] = true;
                    state.resources.weapon_ammo[i] = s.start_ammo;
                }
            }
            // Modes débloqués au départ : ceux dont le coût configuré (outil)
            // est nul (0 = déjà débloqué) - REALISTIC par défaut, INERTIAL
            // seulement si l'outil le laisse gratuit. Le mode de départ
            // (REALISTIC) reste toujours débloqué.
            let start = start_mode(ScenarioId::Progression);
            state.unlocked_modes = [false; MOVING_MODE_COUNT as usize];
            for (i, unlocked) in state.unlocked_modes.iter_mut().enumerate() {
                *unlocked = MODE_COSTS[i] == 0 || i as i32 == start;
            }
            state.moving_mode = start;
            // la soute démarre à la capacité de base (les extensions
            // s'achètent à l'atelier de la station)
            state.player.cargo_size = cargo_capacity(state);
        }
    }
    // Position / orientation / vitesse initiales du vaisseau (scénarios
    // custom, valeurs de l'éditeur). Les scénarios built-in démarrent toujours
    // au centre de la station (0,0, orientation 0, immobile) : on remet les
    // champs à zéro pour ne pas garder les valeurs d'un scénario custom
    // précédemment sélectionné.
    match state.scenario {
        ScenarioId::Custom(ci) => {
            if let Some(data) = crate::scenario_loader::loaded_data(ci) {
                let ist = &data.json.initial_state;
                state.initial_ship_x = ist.start_pos_x;
                state.initial_ship_y = ist.start_pos_y;
                state.initial_ship_orientation = ist.start_orientation;
                state.initial_ship_velocity = ist.start_velocity;
            }
        }
        _ => {
            state.initial_ship_x = 0.0;
            state.initial_ship_y = 0.0;
            state.initial_ship_orientation = 0.0;
            state.initial_ship_velocity = 0.0;
        }
    }
    // Initialiser le suivi des objectifs DAG pour les scénarios custom
    match state.scenario {
        ScenarioId::Custom(ci) => {
            state.objective_tracker.init_for_scenario(ci);
        }
        _ => {
            state.objective_tracker.reset();
        }
    }
}

/// Le vaisseau démarre-t-il **à quai** (liens d'accostage attachés, statut
/// « DOCKED ») ? Oui seulement s'il est **immobile au centre de la station** :
/// position initiale (0,0) ET vitesse nulle. Une position initiale différente
/// de 0 (scénario custom de l'éditeur) ou une vitesse initiale non nulle
/// signifient que le vaisseau démarre en vol, hors de la base - pas de liens,
/// pas d'accostage (la mire réapparaîtra au retour, voir
/// `docking::update_docking_guide`). Appliquée au lancement de la partie
/// (`main.rs`), où `dock_links` et `player_at_station` en sont dérivés.
pub fn start_docked(state: &GameState) -> bool {
    state.initial_ship_x == 0.0 && state.initial_ship_y == 0.0 && state.initial_ship_velocity == 0.0
}

// ─── Carburant et munitions ─────────────────────────────────────────────────

/// Carburant disponible ? (toujours `true` en jeu libre.) Bloque la poussée
/// quand le réservoir est vide - les rotations restent libres.
pub fn fuel_available(state: &GameState) -> bool {
    !has_economy(state) || state.resources.fuel > 0.0
}

/// Consomme le carburant du scénario quand le moteur est allumé (flamme avant
/// ou arrière : compteurs `thrusted`/`revert_thrusted` non nuls), `dt` en
/// secondes. Annonce « OUT OF FUEL » quand le réservoir se vide.
pub fn consume_fuel(state: &mut GameState, dt: f64) {
    if !has_economy(state) {
        return;
    }
    if state.player.thrusted == 0 && state.player.revert_thrusted == 0 {
        return;
    }
    let before = state.resources.fuel;
    let after = (before - scenario(state.scenario).fuel_per_second * dt).max(0.0);
    state.resources.fuel = after;
    if before > 0.0 && after == 0.0 {
        state.send_message("OUT OF FUEL");
    }
}

/// Consomme des munitions pour un tir (scénarios à économie) et renvoie le
/// **masque des armes qui ont tiré** (index de `VAISSEAU_WEAPONS`, borné à
/// `WEAPON_SLOTS`) : chaque arme possédée dont le stock couvre `ammo_per_shot`
/// tire (ses munitions sont consommées) ; une arme à court de munitions ne
/// tire pas, les autres continuent. Aucune arme ne peut tirer (toutes les
/// munitions épuisées) → masque tout faux, le tir est bloqué (cooldown non
/// réinitialisé - le tir part dès qu'une arme a des munitions ; aucun message
/// répété). Hors économie : toutes les armes possédées tirent, sans
/// consommation. Annonce « OUT OF AMMO » quand le dernier stock se vide.
pub fn try_fire(state: &mut GameState) -> [bool; WEAPON_SLOTS] {
    let s = scenario(state.scenario);
    let mut fired = [false; WEAPON_SLOTS];
    if !s.has_economy {
        // jeu libre / Survival : toutes les armes **possédées** tirent, sans
        // consommation - en jeu libre, seule l'arme 1 équipe le vaisseau
        // (masque de `weapon_owned`)
        for (i, slot) in fired.iter_mut().enumerate().take(weapon_slot_count()) {
            *slot = weapon_owned(state, i);
        }
        return fired;
    }
    let total_before = total_ammo(state);
    for (i, slot) in fired.iter_mut().enumerate().take(weapon_slot_count()) {
        if weapon_owned(state, i) && state.resources.weapon_ammo[i] >= s.ammo_per_shot {
            state.resources.weapon_ammo[i] -= s.ammo_per_shot;
            *slot = true;
        }
    }
    if total_before > 0 && total_ammo(state) == 0 {
        state.send_message("OUT OF AMMO");
    }
    fired
}

// ─── Armes du catalogue (achat et munitions par arme) ──────────────────────

/// Données économiques d'une arme du catalogue (index dans `VAISSEAU_WEAPONS`) :
/// nom, coût d'achat au magasin, prix et taille du paquet de munitions du
/// ravitaillement (ligne AMMO du magasin). **Catalogue vide = un seul
/// « canon classique »** (repli :
/// coût 0, toujours équipé, paquets aux valeurs globales `AMMO_PRICE` /
/// `AMMO_STEP`) - le comportement historique est préservé.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSpec {
    /// Nom de l'arme (magasin, messages HUD).
    pub name: &'static str,
    /// Coût d.achat en crédits (0 = arme de base, équipée au départ).
    pub cost: i32,
    /// Prix (crédits) d.un paquet de munitions.
    pub ammo_price: i32,
    /// Taille d'un paquet (munitions par paquet).
    pub ammo_pack: i32,
}

/// Spécification économique de l'arme `i` du catalogue (hors catalogue →
/// canon classique de repli). Pure (tests).
pub fn weapon_spec(i: usize) -> WeaponSpec {
    match crate::marketplace::VAISSEAU_WEAPONS.get(i) {
        Some(w) => WeaponSpec {
            name: w.name,
            cost: w.cost,
            ammo_price: w.ammo_price,
            ammo_pack: w.ammo_pack,
        },
        None => WeaponSpec {
            name: "CANON CLASSIQUE",
            cost: 0,
            ammo_price: AMMO_PRICE,
            ammo_pack: AMMO_STEP,
        },
    }
}

/// Nombre d'emplacements d'armes actifs : le nombre d'armes du catalogue
/// (`VAISSEAU_WEAPONS`), borné à `WEAPON_SLOTS` - **au moins 1** (le canon
/// classique de repli quand le catalogue est vide). Pure (tests).
pub fn weapon_slot_count() -> usize {
    crate::marketplace::VAISSEAU_WEAPONS.len().clamp(1, WEAPON_SLOTS)
}

/// L'arme `i` est-elle **possédée** (équipée) ? Hors économie : en **jeu
/// libre**, seule l'arme 1 (index 0, `ARME 1`) équipe le vaisseau - les
/// autres armes du catalogue ne sont ni construites sur le vaisseau ni
/// tirées ; en Survival (et custom sans économie), toutes les armes du
/// catalogue. En économie : achetée au magasin (`weapon_owned`), ou coût 0
/// (arme de base - comme les modes de déplacement gratuits). Le canon
/// classique (hors catalogue) est toujours possédé. Pure (tests).
pub fn weapon_owned(state: &GameState, i: usize) -> bool {
    match state.scenario {
        // jeu libre : le vaisseau n'est équipé que de l'arme 1
        ScenarioId::FreePlay => i == 0,
        _ if !has_economy(state) => i < weapon_slot_count(),
        _ => {
            state.resources.weapon_owned.get(i).copied().unwrap_or(false)
                || weapon_spec(i).cost == 0
        }
    }
}

/// Tarifs d'achat d'une arme pas encore possédée : tarif de base (prix
/// d'origine) et prix réellement payé (remise de réputation du rang courant
/// appliquée) - `None` = déjà possédée, coût nul ou pas d'économie. Comme
/// `mode_unlock_prices` : affichés dans le magasin de la station.
pub fn weapon_prices(state: &GameState, i: usize) -> Option<(i32, i32)> {
    if !has_economy(state) || weapon_owned(state, i) {
        return None;
    }
    let cost = weapon_spec(i).cost;
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits d.une arme pas encore possédée (`None` = déjà possédée,
/// coût nul ou pas d'économie) - le prix réellement payé (remisé).
pub fn weapon_cost(state: &GameState, i: usize) -> Option<i32> {
    weapon_prices(state, i).map(|(_, discounted)| discounted)
}

/// Résultat d'un achat d'arme au magasin (`buy_weapon`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponOutcome {
    /// Arme déjà possédée (ou pas d'économie).
    Owned,
    /// Arme achetée (coût en crédits déduit, équipée - livrée chargée).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète une arme du catalogue au magasin de la station : paie en crédits
/// (remise de réputation appliquée), l'équipe - son mesh apparaît sur le
/// vaisseau (`vaisseau::rebuild_player_vaisseau` côté jeu) - et la livre
/// **chargée** à la capacité courante. Hors scénario à économie : sans effet
/// (`Owned`). Appelé par le magasin (bouton SHOP de la boîte DOCK STATION).
pub fn buy_weapon(state: &mut GameState, i: usize) -> WeaponOutcome {
    if !has_economy(state) || weapon_owned(state, i) {
        return WeaponOutcome::Owned;
    }
    let Some(cost) = weapon_cost(state, i) else {
        return WeaponOutcome::Owned; // coût 0 → arme de base, déjà équipée
    };
    if state.resources.credits < cost {
        state.send_message(&format!(
            "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
            weapon_spec(i).name,
            cost
        ));
        return WeaponOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    if i < WEAPON_SLOTS {
        state.resources.weapon_owned[i] = true;
        state.resources.weapon_ammo[i] = ammo_capacity(state); // livrée chargée
    }
    state.send_message(&format!(
        "WEAPON {} PURCHASED: -{} CREDITS",
        weapon_spec(i).name,
        cost
    ));
    WeaponOutcome::Purchased(cost)
}

// ─── Radar de bord (minimap globale) ────────────────────────────────────────

/// Coût en crédits du **radar de bord** (`RADAR_COST` de `src/marketplace.rs`) :
/// acheté au magasin (onglet ÉQUIPEMENT) en scénario à économie ; hors
/// économie le radar est toujours allumé (gratuit, historique).
pub fn radar_price(state: &GameState) -> Option<(i32, i32)> {
    if !has_economy(state) || state.resources.radar_owned {
        return None;
    }
    let cost = crate::marketplace::RADAR_COST;
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits du radar non encore possédé (`None` = déjà possédé, hors
/// économie ou coût nul) - le prix réellement payé (remisé).
pub fn radar_cost(state: &GameState) -> Option<i32> {
    radar_price(state).map(|(_, discounted)| discounted)
}

/// Résultat d'un achat du radar au magasin (`buy_radar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarOutcome {
    /// Radar déjà possédé (ou pas d'économie).
    Owned,
    /// Radar acheté (coût en crédits déduit, minimap activée).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète le **radar de bord** au magasin de la station : paie en crédits
/// (remise de réputation appliquée) et active la minimap globale (points des
/// météores et des autres formes, `scenario::has_radar`). Hors scénario à
/// économie : sans effet (`Owned` - le radar y est déjà allumé). Appelé par
/// le magasin (bouton SHOP de la boîte DOCK STATION, onglet ÉQUIPEMENT).
pub fn buy_radar(state: &mut GameState) -> RadarOutcome {
    if !has_economy(state) || state.resources.radar_owned {
        return RadarOutcome::Owned;
    }
    let Some(cost) = radar_cost(state) else {
        return RadarOutcome::Owned; // coût 0 → déjà actif
    };
    if state.resources.credits < cost {
        state.send_message(&format!("NOT ENOUGH CREDITS FOR RADAR ({} NEEDED)", cost));
        return RadarOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    state.resources.radar_owned = true;
    state.send_message(&format!("RADAR PURCHASED: -{} CREDITS", cost));
    RadarOutcome::Purchased(cost)
}

/// Total des munitions restantes des armes **possédées** (toutes armes
/// confondues) - affiché au HUD (`AMMO:x/y`) et sur la télécommande.
/// Pure (tests).
pub fn total_ammo(state: &GameState) -> i32 {
    (0..weapon_slot_count())
        .filter(|&i| weapon_owned(state, i))
        .map(|i| state.resources.weapon_ammo[i])
        .sum()
}

/// Capacité totale des chargeurs des armes **possédées** (somme des
/// capacités courantes, extensions de chargeur comprises). Pure (tests).
pub fn total_ammo_capacity(state: &GameState) -> i32 {
    let cap = ammo_capacity(state);
    (0..weapon_slot_count())
        .filter(|&i| weapon_owned(state, i))
        .map(|_| cap)
        .sum()
}

// ─── Réputation et rangs ────────────────────────────────────────────────────

/// Précision de tir du joueur (0..1) : part de tirs **non perdus** - 1 = aucun
/// tir perdu (tous les tirs ont touché un astéroïde). Sans tir : 0. Sert au
/// gain de réputation (`on_meteor_destroyed`) et à la remise sur les coûts
/// (`current_discount`).
pub fn shooting_precision(state: &GameState) -> f64 {
    if state.bullets_fired > 0 {
        (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64).max(0.0)
    } else {
        0.0
    }
}

/// Réputation gagnée par un astéroïde détruit : le gain de base
/// (`reputation_per_asteroid`) est multiplié par `1 + poids × précision` - la
/// précision de tir (part de tirs non perdus) récompense donc les tirs
/// efficaces. Appelé par `game.rs` quand un météore meurt sous une balle.
pub fn on_meteor_destroyed(state: &mut GameState) {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return;
    }
    let precision = shooting_precision(state);
    let before = rank_at(s.ranks, state.resources.reputation);
    state.resources.reputation +=
        s.reputation_per_asteroid * (1.0 + s.reputation_precision_weight * precision);
    // un palier de réputation franchi débloque le rang suivant : annoncé au
    // HUD (ex « RANK UP: PILOT »)
    let after = rank_at(s.ranks, state.resources.reputation);
    if let (Some(after), Some(before)) = (after, before) {
        if after != before {
            state.send_message(&format!("RANK UP: {}", after.name));
        }
    }
}

/// Rang atteint pour une réputation donnée dans une table de rangs : le plus
/// haut palier dont le seuil est franchi - `None` si la table est vide (jeu
/// libre). Fonction pure (tests). La durée de vie du rang renvoyé est celle
/// de la table passée (`PROGRESSION_RANKS` est `'static`).
pub fn rank_at(ranks: &[ReputationRank], reputation: f64) -> Option<&ReputationRank> {
    ranks.iter().rev().find(|r| reputation >= r.threshold)
}

/// Nom du rang de réputation courant du scénario (dernier palier dont le
/// seuil est atteint), ou `None` si le scénario n'a pas de rangs - affiché au
/// HUD à côté du compteur de réputation.
pub fn current_rank(state: &GameState) -> Option<&'static str> {
    rank_at(scenario(state.scenario).ranks, state.resources.reputation).map(|r| r.name)
}

/// Remise (pourcentage 0..100) accordée sur les coûts de la station par la
/// réputation : celle du plus haut rang atteint (0 sans rang ou table vide).
/// Pure (tests).
pub fn reputation_discount(ranks: &[ReputationRank], reputation: f64) -> i32 {
    rank_at(ranks, reputation).map_or(0, |r| r.discount_percent.clamp(0, 100))
}

/// Coût après remise de réputation : `cost × (100 − remise) / 100`, arrondi à
/// l'entier inférieur (jamais négatif). Pure (tests).
pub fn discounted_cost(cost: i32, discount_percent: i32) -> i32 {
    (cost * (100 - discount_percent.clamp(0, 100))) / 100
}

/// Remise du scénario courant : la remise du rang atteint (`reputation_discount`),
/// **amplifiée par la précision de tir** - la remise est multipliée par
/// `1 + poids × précision` (voir `discount_precision_weight` de `Scenario`) et
/// bornée à 100 %. Sans rang ou poids nul, la précision ne change rien.
pub fn current_discount(state: &GameState) -> i32 {
    let s = scenario(state.scenario);
    let base = reputation_discount(s.ranks, state.resources.reputation);
    if base == 0 || s.discount_precision_weight <= 0.0 {
        return base;
    }
    let boosted = base as f64 * (1.0 + s.discount_precision_weight * shooting_precision(state));
    boosted.round().clamp(0.0, 100.0) as i32
}

// ─── Survie : vies, bouclier, dégâts ────────────────────────────────────────

/// Résultat d'un impact subi par le vaisseau (scénario Survival) : la décision
/// de la coque est renvoyée à `game.rs`, qui restaure le vaisseau (respawn)
/// ou fige la partie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerHit {
    /// Le bouclier a encaissé l'impact : vaisseau intact.
    Absorbed,
    /// Bouclier percé, vaisseau détruit : une vie perdue, respawn (vies
    /// restantes - `i32`).
    Destroyed(i32),
    /// Bouclier percé, dernière vie perdue : partie terminée (`game_over`).
    GameOver,
}

/// Le vaisseau subit un impact (scénario Survival) : le bouclier encaisse
/// `dégâts × multiplicateur` ; s'il est percé, l'impact détruit le vaisseau -
/// une vie est perdue et le bouclier est rechargé (respawn côté `game.rs`),
/// ou la partie est terminée en dernière vie (`game_over`). Sans effet (et
/// renvoie `Absorbed`) hors scénario de survie. Appelé par `game.rs` pour
/// chaque triangle du vaisseau percuté.
/// Dégâts effectifs d'un impact subi : `dégâts de base × multiplicateur` du
/// scénario (fonction pure - testée avec des scénarios sur mesure).
pub fn scaled_impact(s: Scenario, damage: f64) -> f64 {
    damage * s.damage_multiplier
}

pub fn player_hit(state: &mut GameState, damage: f64) -> PlayerHit {
    let s = scenario(state.scenario);
    if s.lives <= 0 {
        return PlayerHit::Absorbed; // classique : la coque est gérée ailleurs
    }
    if state.game_over {
        return PlayerHit::GameOver; // déjà terminée : pas de second message
    }
    // invulnérabilité post-respawn : les impacts sont absorbés sans toucher
    // au bouclier (répit accordé après chaque respawn)
    if state.invulnerable > 0.0 {
        return PlayerHit::Absorbed;
    }
    let dmg = scaled_impact(s, damage);
    if state.resources.shield >= dmg {
        state.resources.shield -= dmg;
        return PlayerHit::Absorbed;
    }
    // bouclier percé : la coque encaisse le reste → vaisseau détruit
    state.resources.shield = 0.0;
    state.resources.lives -= 1;
    if state.resources.lives <= 0 {
        state.resources.lives = 0;
        state.game_over = true;
        state.send_message("GAME OVER - ESC TO QUIT");
        return PlayerHit::GameOver;
    }
    // respawn : bouclier rechargé et invulnérabilité temporaire (la position
    // du vaisseau est restaurée par `game.rs` - `respawn_player`)
    state.resources.shield = s.shield_capacity;
    state.invulnerable = s.respawn_invulnerability;
    state.send_message(&format!(
        "SHIP DESTROYED - {} {} LEFT",
        state.resources.lives,
        if state.resources.lives > 1 { "LIVES" } else { "LIFE" }
    ));
    PlayerHit::Destroyed(state.resources.lives)
}

// ─── Minerais et ravitaillement ─────────────────────────────────────────────

/// Décharge la soute à la station : chaque minerai est converti en crédits
/// selon la valeur de son élément (`ELEMENT_VALUES`) et rapporte de la
/// **réputation** (`reputation_per_mineral` - le commerce est récompensé,
/// comme le tir l'est par les astéroïdes détruits). Appelé par `docking`
/// (déchargement automatique de l'original, au plus tard à la frame suivant
/// la fermeture de la boîte) et par le bouton UNLOAD de la boîte DOCK STATION
/// (déchargement immédiat - les crédits financent le ravitaillement
/// carburant/munitions acheté au magasin du même accostage).
pub fn unload_cargo(state: &mut GameState, elements: &[Element]) {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return;
    }
    let mut gained = 0;
    for (i, e) in elements.iter().enumerate() {
        if let Some(&value) = ELEMENT_VALUES.get(i) {
            gained += e.count * value;
        }
    }
    state.resources.credits += gained;
    if gained > 0 {
        state.send_message(&format!("CARGO UNLOADED: +{} CREDITS", gained));
        // réputation gagnée par minerai déchargé - un palier franchi est
        // annoncé comme pour les astéroïdes détruits
        let before = rank_at(s.ranks, state.resources.reputation);
        state.resources.reputation += gained as f64 * s.reputation_per_mineral;
        let after = rank_at(s.ranks, state.resources.reputation);
        if let (Some(after), Some(before)) = (after, before) {
            if after != before {
                state.send_message(&format!("RANK UP: {}", after.name));
            }
        }
    }
}

/// Résultat d'un ravitaillement à la station (`buy_fuel_qty` /
/// `buy_ammo_qty`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyOutcome {
    /// Réservoir(s) déjà plein(s) (rien à payer).
    Full,
    /// Ravitaillement payé (coût en crédits déduit).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Coût (crédits, remise de réputation appliquée) d'un **plein de
/// carburant** : le manque au réservoir courant est facturé au pas du
/// scénario (`fuel_price` par `fuel_step` unités, arrondi au pas supérieur).
/// Hors économie ou réservoir plein : 0. Équivalent à `fuel_qty_cost` sur
/// tout le manque - le plein complet (extrémité haute du curseur FUEL).
/// Réservé aux tests (le magasin achète à la quantité du curseur).
#[cfg(test)]
pub fn fuel_refill_cost(state: &GameState) -> i32 {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return 0;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let raw = (missing / s.fuel_step).ceil() as i32 * s.fuel_price;
    discounted_cost(raw, current_discount(state))
}

/// Coût (crédits, remise de réputation appliquée) du **rechargement des
/// munitions** : chaque arme possédée est facturée au paquet de l'arme
/// (`ammo_price` par paquet de `ammo_pack` munitions, arrondi au paquet
/// supérieur) - les armes non possédées ne se rechargent pas. Hors économie
/// ou toutes les armes pleines : 0. Réservé aux tests (le magasin achète à
/// la quantité des curseurs AMMO, un par arme possédée).
#[cfg(test)]
pub fn ammo_refill_cost(state: &GameState) -> i32 {
    if !has_economy(state) {
        return 0;
    }
    let max_ammo = ammo_capacity(state);
    let mut raw = 0;
    for i in 0..weapon_slot_count() {
        if !weapon_owned(state, i) {
            continue;
        }
        let spec = weapon_spec(i);
        let missing = (max_ammo - state.resources.weapon_ammo[i]).max(0);
        raw += ((missing + spec.ammo_pack - 1) / spec.ammo_pack) * spec.ammo_price;
    }
    discounted_cost(raw, current_discount(state))
}

/// Nombre de **paquets facturés** pour `qty` unités de carburant au magasin
/// (arrondi au paquet supérieur - tout achat paie au moins un paquet) ; 0 si
/// la quantité est nulle ou hors économie. Affiche la ligne FUEL (« +30
/// (3 paquets) »). Pure (tests).
pub fn fuel_pack_count(state: &GameState, qty: f64) -> i32 {
    let s = scenario(state.scenario);
    if !s.has_economy || qty <= 0.0 {
        return 0;
    }
    (qty / s.fuel_step).ceil() as i32
}

/// Coût (crédits, remise de réputation appliquée) de l'achat de `qty`
/// **unités** de carburant au magasin (ligne FUEL, curseur) : facturées au
/// paquet du scénario (`fuel_price` par `fuel_step` - voir `fuel_pack_count`),
/// puis remise appliquée. `qty <= 0` ou hors économie : 0. Pure (tests).
pub fn fuel_qty_cost(state: &GameState, qty: f64) -> i32 {
    discounted_cost(
        fuel_pack_count(state, qty) * scenario(state.scenario).fuel_price,
        current_discount(state),
    )
}

/// Nombre de **paquets facturés** pour `qty` munitions de l'arme `i` au
/// magasin (paquet de l'arme, arrondi au supérieur) ; 0 si la quantité est
/// nulle ou hors économie. Affiche la ligne AMMO de l'arme. Pure (tests).
pub fn ammo_pack_count(state: &GameState, i: usize, qty: i32) -> i32 {
    if !has_economy(state) || qty <= 0 {
        return 0;
    }
    let spec = weapon_spec(i);
    (qty + spec.ammo_pack - 1) / spec.ammo_pack
}

/// Coût (crédits, remise de réputation appliquée) de l'achat de `qty`
/// **unités** de munitions pour l'arme `i` (ligne AMMO de l'arme, curseur) :
/// facturées au paquet de l'arme (`ammo_price` par paquet de `ammo_pack` -
/// voir `ammo_pack_count`), puis remise appliquée. `qty <= 0` ou hors
/// économie : 0. Pure (tests).
pub fn ammo_qty_cost(state: &GameState, i: usize, qty: i32) -> i32 {
    discounted_cost(
        ammo_pack_count(state, i, qty) * weapon_spec(i).ammo_price,
        current_discount(state),
    )
}

/// Achète un **plein de carburant** à la station : remplit le réservoir à la
/// capacité courante et déduit les crédits (voir `buy_fuel_qty`). Équivaut
/// au curseur FUEL du magasin à son maximum. Réservé aux tests (le magasin
/// achète à la quantité du curseur).
#[cfg(test)]
pub fn purchase_fuel(state: &mut GameState) -> SupplyOutcome {
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    buy_fuel_qty(state, missing)
}

/// Achète `qty` unités de carburant à la station (ligne FUEL du magasin,
/// curseur) : facturées au paquet (`fuel_qty_cost` - un paquet minimum pour
/// tout achat) ; le réservoir reçoit exactement `qty` unités, bornées au
/// manque de la capacité courante. Minerais insuffisants → `Insufficient`
/// (message « NOT ENOUGH CREDITS FOR FUEL », non répété au même coût).
pub fn buy_fuel_qty(state: &mut GameState, qty: f64) -> SupplyOutcome {
    if !has_economy(state) {
        return SupplyOutcome::Full;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let qty = qty.clamp(0.0, missing);
    if qty <= 0.0 {
        return SupplyOutcome::Full;
    }
    let cost = fuel_qty_cost(state, qty);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        // le message n'est envoyé qu'au début du manque (pas à chaque clic
        // répété - `supplies_shortage_cost`)
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR FUEL ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    state.resources.fuel = (state.resources.fuel + qty).min(fuel_capacity(state));
    state.send_message(&format!("FUEL PURCHASED: -{} CREDITS", cost));
    SupplyOutcome::Purchased(cost)
}

/// Achète le **rechargement des munitions** à la station : chaque arme
/// possédée repart pleine à la capacité courante (`ammo_refill_cost`, par
/// paquet de l.arme) et les crédits sont déduits. Les munitions s'achètent
/// **indépendamment** du carburant. Réservé aux tests (le magasin achète à
/// la quantité des curseurs AMMO, un par arme possédée).
#[cfg(test)]
pub fn purchase_ammo(state: &mut GameState) -> SupplyOutcome {
    if !has_economy(state) {
        return SupplyOutcome::Full;
    }
    let cost = ammo_refill_cost(state);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR AMMO ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    let max_ammo = ammo_capacity(state);
    for i in 0..weapon_slot_count() {
        if weapon_owned(state, i) {
            state.resources.weapon_ammo[i] = max_ammo;
        }
    }
    state.send_message(&format!("AMMO PURCHASED: -{} CREDITS", cost));
    SupplyOutcome::Purchased(cost)
}

/// Achète `qty` unités de munitions pour l'arme `i` (ligne AMMO de l'arme,
/// curseur) : facturées au paquet de l'arme (`ammo_qty_cost` - un paquet
/// minimum pour tout achat) ; le chargeur reçoit exactement `qty` unités,
/// bornées au manque de la capacité courante. Arme non possédée ou quantité
/// nulle : sans effet (`Full`). Minerais insuffisants → `Insufficient`
/// (message « NOT ENOUGH CREDITS FOR AMMO », non répété au même coût).
pub fn buy_ammo_qty(state: &mut GameState, i: usize, qty: i32) -> SupplyOutcome {
    if !has_economy(state) || !weapon_owned(state, i) {
        return SupplyOutcome::Full;
    }
    let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0);
    let qty = qty.clamp(0, missing);
    if qty <= 0 {
        return SupplyOutcome::Full;
    }
    let cost = ammo_qty_cost(state, i, qty);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR AMMO ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    state.resources.weapon_ammo[i] += qty;
    state.send_message(&format!(
        "{} AMMO PURCHASED: -{} CREDITS",
        weapon_spec(i).name,
        cost
    ));
    SupplyOutcome::Purchased(cost)
}

/// Quantité maximale de carburant **achetable** avec les crédits courants :
/// le plus grand multiple du pas (`fuel_step`) dont le coût (remisé) ne
/// dépasse pas les crédits, borné au manque du réservoir - 0 si même un
/// paquet est hors de portée (ou hors économie). Positionne le curseur FUEL
/// du magasin à l'ouverture. Pure (tests).
pub fn affordable_fuel_qty(state: &GameState) -> f64 {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return 0.0;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let max_packs = (missing / s.fuel_step).ceil() as i32;
    for n in (1..=max_packs).rev() {
        if discounted_cost(n * s.fuel_price, current_discount(state)) <= state.resources.credits {
            return (n as f64 * s.fuel_step).min(missing);
        }
    }
    0.0
}

/// Quantité maximale de munitions **achetable** pour l'arme `i` avec les
/// crédits courants : le plus grand multiple du paquet de l.arme dont le
/// coût (remisé) ne dépasse pas les crédits, borné au manque du chargeur -
/// 0 si même un paquet est hors de portée (ou hors économie). Positionne le
/// curseur AMMO de l'arme à l'ouverture du magasin. Pure (tests).
pub fn affordable_ammo_qty(state: &GameState, i: usize) -> i32 {
    if !has_economy(state) {
        return 0;
    }
    let spec = weapon_spec(i);
    let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0);
    let max_packs = (missing + spec.ammo_pack - 1) / spec.ammo_pack;
    for n in (1..=max_packs).rev() {
        if discounted_cost(n * spec.ammo_price, current_discount(state)) <= state.resources.credits {
            return (n * spec.ammo_pack).min(missing);
        }
    }
    0
}

/// Borne les quantités des curseurs du magasin (carburant et munitions par
/// arme possédée) à ce que les crédits permettent (`affordable_fuel_qty` /
/// Aimanate une quantité de curseur au **multiple du paquet** le plus proche
/// (pour ne jamais payer un paquet sans en prendre les unités) - sauf le
/// **maximum** (`max`, le plein du réservoir), qui reste atteignable même
/// s'il ne tombe pas pile sur un multiple : le dernier paquet est alors pris
/// en entier (aucune unité perdue). `qty` est arrondi au multiple le plus
/// proche et borné à `max` (0 si le paquet ou le maximum est nul). Pure
/// (tests).
pub fn snap_to_pack(qty: f64, pack: f64, max: f64) -> f64 {
    if pack <= 0.0 || max <= 0.0 {
        return 0.0;
    }
    let qty = qty.clamp(0.0, max);
    // le maximum (plein du réservoir) reste une position valide : au-delà,
    // le dernier paquet payé est pris en entier
    if qty >= max {
        return max;
    }
    (qty / pack).round() * pack
}

/// Borne les quantités des curseurs du magasin (carburant et munitions par
/// arme possédée) à ce que les crédits permettent (`affordable_fuel_qty` /
/// `affordable_ammo_qty` - déjà bornées au manque des réservoirs) : jamais
/// une quantité dont le coût dépasserait les crédits disponibles - on ne
/// peut pas se retrouver avec un curseur hors de portée. Les quantités sont
/// aussi **aimantées aux multiples du paquet** (`snap_to_pack`) pour ne
/// jamais payer un paquet sans en prendre les unités en glissant à la
/// souris. Appelé à chaque frame par le magasin (`game.rs`). Pur (tests).
pub fn clamp_shop_quantities(state: &mut GameState) {
    if !has_economy(state) {
        state.shop_fuel_qty = 0.0;
        state.shop_ammo_qty = [0.0; WEAPON_SLOTS];
        return;
    }
    let missing_fuel = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    state.shop_fuel_qty = snap_to_pack(
        state.shop_fuel_qty,
        scenario(state.scenario).fuel_step,
        missing_fuel,
    );
    state.shop_fuel_qty = state.shop_fuel_qty.clamp(0.0, affordable_fuel_qty(state));
    for i in 0..weapon_slot_count() {
        if weapon_owned(state, i) {
            let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0) as f64;
            state.shop_ammo_qty[i] = snap_to_pack(
                state.shop_ammo_qty[i],
                weapon_spec(i).ammo_pack as f64,
                missing,
            );
            state.shop_ammo_qty[i] = state.shop_ammo_qty[i].clamp(
                0.0,
                affordable_ammo_qty(state, i) as f64,
            );
        }
    }
}

// ─── Modes de déplacement ───────────────────────────────────────────────────

/// Coûts de déblocage d'un mode pas encore débloqué : tarif de base (prix
/// d'origine) et prix réellement payé (remise de réputation du rang courant
/// appliquée) - `None` = débloqué, ou pas d'économie. Affichés dans le
/// magasin de la station (bouton SHOP de la boîte DOCK STATION).
pub fn mode_unlock_prices(state: &GameState, mode: i32) -> Option<(i32, i32)> {
    if !has_economy(state) {
        return None;
    }
    let m = mode as usize;
    if m >= state.unlocked_modes.len() || state.unlocked_modes[m] {
        return None;
    }
    let cost = scenario(state.scenario).mode_costs[m];
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits d.un mode pas encore débloqué (`None` = débloqué, ou pas
/// d'économie) - affiché dans le magasin de la station (bouton SHOP de la
/// boîte DOCK STATION). C'est le prix réellement payé (remise de réputation
/// du rang courant appliquée) ; voir `mode_unlock_prices` pour le tarif de
/// base.
pub fn locked_cost(state: &GameState, mode: i32) -> Option<i32> {
    mode_unlock_prices(state, mode).map(|(_, discounted)| discounted)
}

/// Sélectionne un mode de déplacement dans le magasin de la station :
/// débloqué → appliqué immédiatement ; verrouillé → payé en crédits (si
/// possible, sinon message « NOT ENOUGH CREDITS ») puis appliqué. Renvoie
/// `true` si le mode demandé est devenu le mode courant.
pub fn try_select_mode(state: &mut GameState, mode: i32) -> bool {
    match locked_cost(state, mode) {
        None => {
            state.moving_mode = mode;
            true
        }
        Some(cost) => {
            if state.resources.credits >= cost {
                state.resources.credits -= cost;
                state.unlocked_modes[mode as usize] = true;
                state.moving_mode = mode;
                state.send_message(&format!(
                    "MODE {} UNLOCKED ({} CREDITS)",
                    mode_label(mode),
                    cost
                ));
                true
            } else {
                state.send_message(&format!(
                    "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
                    mode_label(mode),
                    cost
                ));
                false
            }
        }
    }
}

// ─── Atelier d'amélioration du vaisseau ─────────────────────────────────────

/// Capacité d'une ligne d'amélioration au niveau `level` (0 = base) : base +
/// bonus des extensions achetées, niveau borné au nombre d'extensions.
/// Fonction pure (tests).
pub fn track_capacity(track: &UpgradeTrack, level: i32) -> i32 {
    let level = level.clamp(0, track.tiers.len() as i32);
    track.base + track.tiers.iter().take(level as usize).map(|t| t.bonus).sum::<i32>()
}

/// Prochaine extension d'une ligne (`None` = niveau max atteint).
pub fn next_upgrade(track: &UpgradeTrack, level: i32) -> Option<&ShipUpgrade> {
    track.tiers.get(level.clamp(0, track.tiers.len() as i32) as usize)
}

/// Capacité maximale du réservoir de carburant (base + extensions achetées).
pub fn fuel_capacity(state: &GameState) -> f64 {
    track_capacity(&scenario(state.scenario).fuel_upgrades, state.resources.fuel_level) as f64
}

/// Capacité maximale du chargeur de munitions (base + extensions achetées).
pub fn ammo_capacity(state: &GameState) -> i32 {
    track_capacity(&scenario(state.scenario).ammo_upgrades, state.resources.ammo_level)
}

/// Capacité maximale de la soute (base + extensions achetées).
pub fn cargo_capacity(state: &GameState) -> i32 {
    track_capacity(&scenario(state.scenario).cargo_upgrades, state.resources.cargo_level)
}

/// Ligne d'affichage d'une amélioration de l'atelier : libellé, capacité
/// actuelle et prochaine extension (`None` = au max) - pour l'écran atelier
/// (`shop_render::draw_shop_box`).
pub struct UpgradeLine {
    /// Libellé de la ligne (ex « FUEL TANK »).
    pub label: &'static str,
    /// Capacité actuelle.
    pub capacity: i32,
    /// Prochaine extension (nom, coût, bonus) - `None` = niveau max.
    pub next: Option<ShipUpgrade>,
}

/// Ligne d'affichage d'une amélioration pour l'atelier (voir `UpgradeLine`).
pub fn upgrade_line(state: &GameState, track: UpgradeTrackId) -> UpgradeLine {
    let s = scenario(state.scenario);
    let (upgrades, level, capacity) = match track {
        UpgradeTrackId::Fuel => (
            s.fuel_upgrades,
            state.resources.fuel_level,
            fuel_capacity(state) as i32,
        ),
        UpgradeTrackId::Ammo => (s.ammo_upgrades, state.resources.ammo_level, ammo_capacity(state)),
        UpgradeTrackId::Cargo => (s.cargo_upgrades, state.resources.cargo_level, cargo_capacity(state)),
    };
    let mut next = next_upgrade(&upgrades, level).copied();
    // le coût affiché à l'atelier est le coût réellement payé (remisé)
    if let Some(u) = &mut next {
        u.cost = discounted_cost(u.cost, current_discount(state));
    }
    UpgradeLine {
        label: upgrades.label,
        capacity,
        next,
    }
}

/// Résultat d'un achat à l'atelier (`buy_upgrade`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeOutcome {
    /// Ligne déjà au niveau maximum (ou pas d'atelier hors économie).
    Maxed,
    /// Extension achetée (coût en crédits déduit, niveau +1).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète la prochaine extension d'une ligne à l'atelier de la station : paie
/// en crédits et fait passer la ligne au niveau suivant - les réservoirs
/// montent à la nouvelle capacité (plein inclus) et la soute s'agrandit
/// immédiatement. Hors scénario à économie (pas d'atelier) ou ligne au max :
/// sans effet (`Maxed`). Appelé par le magasin (bouton SHOP de la
/// boîte DOCK STATION).
pub fn buy_upgrade(state: &mut GameState, track: UpgradeTrackId) -> UpgradeOutcome {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return UpgradeOutcome::Maxed;
    }
    let (upgrades, level) = match track {
        UpgradeTrackId::Fuel => (s.fuel_upgrades, state.resources.fuel_level),
        UpgradeTrackId::Ammo => (s.ammo_upgrades, state.resources.ammo_level),
        UpgradeTrackId::Cargo => (s.cargo_upgrades, state.resources.cargo_level),
    };
    let next = match next_upgrade(&upgrades, level) {
        Some(u) => u,
        None => return UpgradeOutcome::Maxed,
    };
    // la réputation remise les coûts de la station (atelier, ravitaillement,
    // modes) : le prix affiché et payé est le coût remisé
    let cost = discounted_cost(next.cost, current_discount(state));
    if state.resources.credits < cost {
        state.send_message(&format!(
            "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
            next.name, cost
        ));
        return UpgradeOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    match track {
        UpgradeTrackId::Fuel => {
            state.resources.fuel_level += 1;
            state.resources.fuel = fuel_capacity(state); // plein à la nouvelle capacité
        }
        UpgradeTrackId::Ammo => {
            state.resources.ammo_level += 1;
            // chargeur agrandi : chaque arme possédée passe à la nouvelle
            // capacité, pleine (les armes non possédées restent à 0)
            for i in 0..weapon_slot_count() {
                if weapon_owned(state, i) {
                    state.resources.weapon_ammo[i] = ammo_capacity(state);
                }
            }
        }
        UpgradeTrackId::Cargo => {
            state.resources.cargo_level += 1;
            state.player.cargo_size = cargo_capacity(state);
        }
    }
    state.send_message(&format!("{} PURCHASED: -{} CREDITS", next.name, cost));
    UpgradeOutcome::Purchased(cost)
}

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
fn weapons_owned_mask(state: &GameState) -> i32 {
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
/// d'atelier en Progression, vies et bouclier en Survival. Les valeurs sont
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
        PROG_LIVES_KEY,
        PROG_SHIELD_KEY,
        PROG_OBJECTIVES_KEY,
        PROG_METEORS_KEY,
        PROG_DOCKS_KEY,
        PROG_BULLETS_FIRED_KEY,
        PROG_BULLETS_LOST_KEY,
        PROG_SURVIVE_KEY,
        "moving_mode",
    ] {
        let _ = crate::persist::delete_key_from(path, key);
    }
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

#[cfg(test)]
mod tests {
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
        assert_eq!(highlighted, vec!["42", "3/4", "60", " (ACE)"]);
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
        assert_eq!(highlighted, vec!["2", "1.5"]);
        assert!(segs.iter().filter(|s| s.color.is_some()).all(|s| s.color == Some(RULES_COLOR_CYAN)));

        // jeu libre : aucune valeur à mettre en évidence
        assert!(save_summary_segments(&GameState::new())
            .iter()
            .all(|s| s.color.is_none()));
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
}
