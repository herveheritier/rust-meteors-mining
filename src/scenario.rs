//! Scénarios de jeu.
//!
//! Un scénario est un ensemble de règles (ressources économiques, verrous de
//! gameplay, récompenses) appliquées par des points d'accroche purs dans la
//! boucle de jeu — voir les appels à `crate::scenario` dans `game.rs`. Les
//! règles sont des **données** (`Scenario`) et les effets des **fonctions
//! pures** testables sans macroquad.
//!
//! Trois scénarios :
//! - `FreePlay` (défaut) : comportement historique du port — pas d'économie,
//!   tous les modes de déplacement disponibles, carburant et munitions
//!   illimités.
//! - `Progression` (l'exemple décrit) : le vaisseau démarre en mode REALISTIC
//!   (gratuit, coût 0 paramétré dans l'outil) ; il doit accumuler des minerais
//!   (gemmes collectées sur les astéroïdes, déchargées à la station) pour
//!   débloquer les modes payants (INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45) ;
//!   chaque poussée consomme du carburant et chaque tir des munitions
//!   (remplis à la station, contre minerais) ; détruire des astéroïdes
//!   augmente la réputation, d'autant plus que la précision de tir est bonne,
//!   et décharger de la cargaison en rapporte aussi (commerce récompensé).
//!   À la station, le **magasin** (bouton SHOP de la boîte DOCK STATION)
//!   permet d'acheter contre minerais des **extensions de vaisseau** :
//!   réservoir de carburant, chargeur de munitions et soute (capacités
//!   augmentées, persistées comme la progression).
//! - `Survival` (preuve que le système s'étend à d'autres mécaniques) : ni
//!   économie ni verrous — le vaisseau a des **vies** et un **bouclier** qui
//!   absorbe les impacts ; quand il est percé, l'impact suivant détruit le
//!   vaisseau : une vie est perdue et il respawne à la station (dernière vie
//!   perdue = fin de partie). Le **multiplicateur de dégâts** aggrave les
//!   impacts (bouclier vidé plus vite).
//!
//! La progression d'un scénario est persistée dans le fichier de config
//! (`persist.rs`, clés `scenario`, `prog_*`) et restaurée au lancement
//! suivant : minerais/modes/réputation/niveaux d'atelier en Progression,
//! vies/bouclier en Survival — chaque scénario n'écrit que ses propres clés.
//! Le carburant et les munitions, eux, repartent pleins à chaque lancement
//! (à la capacité courante, extensions comprises).

use std::io;
use std::path::Path;

use crate::config::{MOVING_MODE_COUNT, MOVING_MODE_DIRECTIONAL, MOVING_MODE_REALISTIC};
use crate::state::{Element, GameState};

/// Identifiant d'un scénario (choisi à l'écran titre, touche N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioId {
    /// Jeu libre : comportement historique, sans économie.
    FreePlay,
    /// Économie : minerais, carburant/munitions payants, réputation.
    Progression,
    /// Survie : vies, bouclier et multiplicateur de dégâts — sans économie.
    Survival,
}

/// Ressources économiques du joueur (scénarios à économie) et de survie
/// (vies, bouclier).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Resources {
    /// Carburant en unités (0 = réservoir vide, plus de poussée).
    pub fuel: f64,
    /// Munitions en unités (0 = plus de tirs).
    pub ammo: i32,
    /// Minerais — la monnaie : déchargés à la station, dépensés pour les
    /// modes de déplacement et le ravitaillement.
    pub minerals: i32,
    /// Réputation — augmente avec les astéroïdes détruits et la précision.
    pub reputation: f64,
    /// Vies restantes (scénario Survival) — 0 = partie terminée.
    pub lives: i32,
    /// Bouclier restant (scénario Survival) : absorbe les impacts avant la
    /// coque ; rechargé au respawn.
    pub shield: f64,
    /// Niveau du réservoir de carburant (atelier) : nombre d'extensions
    /// achetées (0 = capacité de base) — Progression.
    pub fuel_level: i32,
    /// Niveau du chargeur de munitions (atelier) — Progression.
    pub ammo_level: i32,
    /// Niveau de la soute (atelier) — Progression.
    pub cargo_level: i32,
}

/// Types et données de la place de marché — extensions de vaisseau de
/// l'atelier de la station, économie et rangs de réputation (scénario
/// Progression, bouton SHOP de la boîte DOCK STATION). Définis dans
/// `src/marketplace.rs`, un **fichier généré** par l'outil de gestion
/// `tools/marketplace-editor/index.html` : pour ajuster les objets vendus, les
/// prix ou les rangs (seuils, noms, remises), régénérez ce fichier depuis
/// l'éditeur — rien à modifier ici. Réexportés pour l'API publique du module
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
    /// Soute (cargaison de gemmes).
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
    /// `true` : carburant/munitions/minerais actifs ; `false` : illimités et
    /// tous les modes débloqués (comportement historique).
    pub has_economy: bool,
    /// Rangs de réputation (paliers débloqués par la réputation, seuils
    /// croissants) — vide en jeu libre (aucun rang).
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
    /// Coût en minerais pour débloquer chaque mode de déplacement (index
    /// `MOVING_MODE_*` ; 0 = déjà débloqué).
    pub mode_costs: [i32; MOVING_MODE_COUNT as usize],
    /// Réputation gagnée par astéroïde détruit (hors bonus de précision).
    pub reputation_per_asteroid: f64,
    /// Bonus de précision : gain × (1 + poids × précision), précision en 0..1.
    pub reputation_precision_weight: f64,
    /// Réputation gagnée par minerai déchargé à la station (commerce — les
    /// astéroïdes détruits récompensent le tir, la cargaison le commerce).
    pub reputation_per_mineral: f64,
    /// Poids de la précision de tir sur la remise de réputation : la remise
    /// du rang est multipliée par `1 + poids × précision` (voir
    /// `DISCOUNT_PRECISION_WEIGHT` de `src/marketplace.rs`).
    pub discount_precision_weight: f64,
    /// Prix (minerais) d'un plein par pas de `fuel_step` unités.
    pub fuel_price: i32,
    /// Pas de ravitaillement en carburant (unités par plein facturé).
    pub fuel_step: f64,
    /// Prix (minerais) d'un plein par pas de `ammo_step` unités.
    pub ammo_price: i32,
    /// Pas de ravitaillement en munitions.
    pub ammo_step: i32,
    /// Nombre de vies (scénario Survival ; 0 = illimité/classique).
    pub lives: i32,
    /// Capacité du bouclier (points absorbés avant la coque, scénario
    /// Survival) — 0 = pas de bouclier.
    pub shield_capacity: f64,
    /// Multiplicateur des dégâts subis (bouclier puis coque, scénario
    /// Survival) — 1.0 en classique.
    pub damage_multiplier: f64,
    /// Durée (secondes) d'invulnérabilité après un respawn (scénario
    /// Survival) : les impacts sont absorbés sans toucher au bouclier —
    /// 0.0 en classique.
    pub respawn_invulnerability: f64,
    /// Couleur ARGB des valeurs mises en évidence dans les lignes RULES / SAVE
    /// de l'écran titre (coûts, vies, bouclier, rangs…) — propre à chaque
    /// scénario, pour que le changement de stat saute aux yeux au basculement
    /// (N/B/1-3).
    pub rules_color: u32,
    /// Ligne « réservoir de carburant » de l'atelier de la station (scénario
    /// Progression) — extensions achetées en minerais ; vide ailleurs (pas
    /// d'atelier). La capacité courante (`fuel_capacity`) est la base + les
    /// bonus des extensions possédées.
    pub fuel_upgrades: UpgradeTrack,
    /// Ligne « chargeur de munitions » de l'atelier (scénario Progression).
    pub ammo_upgrades: UpgradeTrack,
    /// Ligne « soute » de l'atelier (scénario Progression).
    pub cargo_upgrades: UpgradeTrack,
}

/// Couleurs ARGB d'accent de l'écran titre (valeurs des lignes RULES / SAVE,
/// voir `Scenario::rules_color`) — une par scénario : jaune pour jeu libre /
/// Progression, cyan pour Survival (le changement de couleur marque aussi le
/// basculement).
pub const RULES_COLOR_YELLOW: u32 = 0xFFFFFF00;
pub const RULES_COLOR_CYAN: u32 = 0xFF00FFFF;

/// Règles du jeu libre (défaut) — aucune économie.
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
    ammo_price: 0,
    ammo_step: 5,
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
    description: "économie : minerais, carburant, réputation",
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
    discount_precision_weight: DISCOUNT_PRECISION_WEIGHT, // précision sur la remise — src/marketplace.rs
    fuel_price: FUEL_PRICE, // 1 minerai pour 10 unités — src/marketplace.rs
    fuel_step: FUEL_STEP,
    ammo_price: AMMO_PRICE, // 1 minerai pour 5 munitions — src/marketplace.rs
    ammo_step: AMMO_STEP,
    lives: 0,
    shield_capacity: 0.0,
    damage_multiplier: 1.0,
    respawn_invulnerability: 0.0,
    rules_color: RULES_COLOR_YELLOW,
    fuel_upgrades: FUEL_UPGRADE_TRACK,
    ammo_upgrades: AMMO_UPGRADE_TRACK,
    cargo_upgrades: CARGO_UPGRADE_TRACK,
};

/// Règles du scénario « Survival » — vies, bouclier, dégâts (voir l'en-tête
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
    ammo_price: 0,
    ammo_step: 5,
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
    }
}

/// Le scénario gère-t-il une économie ? (`false` = comportement historique :
/// ressources illimitées, modes tous débloqués.)
pub fn has_economy(state: &GameState) -> bool {
    scenario(state.scenario).has_economy
}

/// Le scénario gère-t-il la survie (vies + bouclier) ? — déduit du nombre de
/// vies : `lives > 0` (Survival), sinon classique (FreePlay/Progression).
pub fn has_survival(state: &GameState) -> bool {
    scenario(state.scenario).lives > 0
}

// ─── Règles affichées (écran titre) ─────────────────────────────────────────

/// Segment de la ligne des règles (écran titre) : un libellé discret ou une
/// valeur chiffrée mise en évidence (coût, vies, bouclier, dégâts, durée,
/// rang) — colorée à l'affichage de la couleur du scénario (`color`) pour
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
/// l'écran titre : dérivées des données du scénario — coûts des modes,
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
            label(&mut out, "aucun coût — carburant/munitions illimités, tous les modes débloqués");
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
            label(&mut out, " minerais ; carburant/munitions payants ; rangs : ");
            if let Some(first) = PROGRESSION_RANKS.first() {
                value(&mut out, first.name.to_string());
            }
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

/// Texte complet des règles (segments concaténés, sans coloration) — réservé
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
        .filter(|(_, &cost)| cost > 0)
        .map(|(i, &cost)| (mode_label(i as i32), cost))
        .collect()
}

/// « 4 WAYS 30, DIRECTIONAL 45 minerais » — coûts des modes de déplacement
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
            + " minerais"
    }
}

/// Résumé segmenté de la progression **enregistrée** du scénario courant,
/// affiché à l'écran titre sous les règles : `state.resources` contient déjà
/// la sauvegarde restaurée (voir `load_progression`) — minerais, modes
/// débloqués et réputation (+ rang) en Progression, vies et bouclier en
/// Survival ; jeu libre : aucune sauvegarde. Découpé en segments comme
/// `scenario_rules` : les valeurs (minerais, modes, réputation, rang, vies,
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
        ScenarioId::Progression => {
            let unlocked = state.unlocked_modes.iter().filter(|&&u| u).count();
            let mut out = vec![
                label("minerais "),
                value(state.resources.minerals.to_string()),
                label(" — modes "),
                value(format!("{}/{}", unlocked, MOVING_MODE_COUNT)),
                label(" — réputation "),
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
                " vies — bouclier "
            } else {
                " vie — bouclier "
            }),
            value(format!("{:.1}", state.resources.shield)),
        ],
    }
}

/// Texte complet du résumé de sauvegarde (segments concaténés, sans
/// coloration) — réservé aux tests (l'écran titre affiche les segments
/// colorés, voir `save_summary_segments`).
#[cfg(test)]
pub fn save_summary(state: &GameState) -> String {
    save_summary_segments(state)
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}

/// Mode de déplacement de départ du scénario `id` : REALISTIC en Progression,
/// DIRECTIONAL — le défaut historique — en jeu libre et en Survival. Utilisé
/// par `apply_start` (et par le magasin, qui ne doit jamais débloquer un mode
/// gratuitement : le RESET des réglages ne touche plus au mode).
pub fn start_mode(id: ScenarioId) -> i32 {
    match id {
        ScenarioId::FreePlay => MOVING_MODE_DIRECTIONAL,
        ScenarioId::Progression => MOVING_MODE_REALISTIC,
        ScenarioId::Survival => MOVING_MODE_DIRECTIONAL,
    }
}

/// Sélectionne un scénario donné (écran titre, touches 1/2/3) et applique
/// ses règles de départ (`apply_start`). La restauration/enregistrement de la
/// progression reste à la charge de l'appelant (écran titre).
pub fn select_scenario(state: &mut GameState, id: ScenarioId) {
    state.scenario = id;
    apply_start(state);
}

/// Bascule de scénario (écran titre, touche N) — jeu libre → Progression →
/// Survival → jeu libre — et applique ses règles de départ.
pub fn cycle_scenario(state: &mut GameState) {
    let next = match state.scenario {
        ScenarioId::FreePlay => ScenarioId::Progression,
        ScenarioId::Progression => ScenarioId::Survival,
        ScenarioId::Survival => ScenarioId::FreePlay,
    };
    select_scenario(state, next);
}

/// Bascule au scénario **précédent** (écran titre, touche B — inverse de N) :
/// jeu libre → Survival → Progression → jeu libre.
pub fn cycle_scenario_back(state: &mut GameState) {
    let prev = match state.scenario {
        ScenarioId::FreePlay => ScenarioId::Survival,
        ScenarioId::Progression => ScenarioId::FreePlay,
        ScenarioId::Survival => ScenarioId::Progression,
    };
    select_scenario(state, prev);
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
        ScenarioId::Progression => {
            state.resources = Resources {
                fuel: s.start_fuel,
                ammo: s.start_ammo,
                minerals: 0,
                reputation: 0.0,
                lives: 0,
                shield: 0.0,
                fuel_level: 0,
                ammo_level: 0,
                cargo_level: 0,
            };
            // Modes débloqués au départ : ceux dont le coût configuré (outil)
            // est nul (0 = déjà débloqué) — REALISTIC par défaut, INERTIAL
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
}

// ─── Carburant et munitions ─────────────────────────────────────────────────

/// Carburant disponible ? (toujours `true` en jeu libre.) Bloque la poussée
/// quand le réservoir est vide — les rotations restent libres.
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

/// Consomme des munitions pour un tir (scénarios à économie). Renvoie `false`
/// si le chargeur est vide (le HUD affiche `AMMO:0` ; aucun message répété).
/// Annonce « OUT OF AMMO » quand le chargeur se vide.
pub fn try_fire(state: &mut GameState) -> bool {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return true;
    }
    if state.resources.ammo >= s.ammo_per_shot {
        state.resources.ammo -= s.ammo_per_shot;
        if state.resources.ammo == 0 {
            state.send_message("OUT OF AMMO");
        }
        true
    } else {
        false
    }
}

// ─── Réputation et rangs ────────────────────────────────────────────────────

/// Précision de tir du joueur (0..1) : part de tirs **non perdus** — 1 = aucun
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
/// (`reputation_per_asteroid`) est multiplié par `1 + poids × précision` — la
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
    if after.is_some() && after != before {
        state.send_message(&format!("RANK UP: {}", after.unwrap().name));
    }
}

/// Rang atteint pour une réputation donnée dans une table de rangs : le plus
/// haut palier dont le seuil est franchi — `None` si la table est vide (jeu
/// libre). Fonction pure (tests). La durée de vie du rang renvoyé est celle
/// de la table passée (`PROGRESSION_RANKS` est `'static`).
pub fn rank_at<'a>(ranks: &'a [ReputationRank], reputation: f64) -> Option<&'a ReputationRank> {
    ranks.iter().rev().find(|r| reputation >= r.threshold)
}

/// Nom du rang de réputation courant du scénario (dernier palier dont le
/// seuil est atteint), ou `None` si le scénario n'a pas de rangs — affiché au
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
/// **amplifiée par la précision de tir** — la remise est multipliée par
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
    /// restantes — `i32`).
    Destroyed(i32),
    /// Bouclier percé, dernière vie perdue : partie terminée (`game_over`).
    GameOver,
}

/// Le vaisseau subit un impact (scénario Survival) : le bouclier encaisse
/// `dégâts × multiplicateur` ; s'il est percé, l'impact détruit le vaisseau —
/// une vie est perdue et le bouclier est rechargé (respawn côté `game.rs`),
/// ou la partie est terminée en dernière vie (`game_over`). Sans effet (et
/// renvoie `Absorbed`) hors scénario de survie. Appelé par `game.rs` pour
/// chaque triangle du vaisseau percuté.
/// Dégâts effectifs d'un impact subi : `dégâts de base × multiplicateur` du
/// scénario (fonction pure — testée avec des scénarios sur mesure).
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
    // du vaisseau est restaurée par `game.rs` — `respawn_player`)
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

/// Décharge la soute à la station : chaque gemme est convertie en minerais
/// selon la valeur de son élément (`ELEMENT_VALUES`) et rapporte de la
/// **réputation** (`reputation_per_mineral` — le commerce est récompensé,
/// comme le tir l'est par les astéroïdes détruits). Appelé par `docking`
/// (déchargement automatique de l'original, au plus tard à la frame suivant
/// la fermeture de la boîte) et par le bouton UNLOAD de la boîte DOCK STATION
/// (déchargement immédiat — les minerais financent le REFUEL/REARM du même
/// accostage).
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
    state.resources.minerals += gained;
    if gained > 0 {
        state.send_message(&format!("CARGO UNLOADED: +{} MINERALS", gained));
        // réputation gagnée par minerai déchargé — un palier franchi est
        // annoncé comme pour les astéroïdes détruits
        let before = rank_at(s.ranks, state.resources.reputation);
        state.resources.reputation += gained as f64 * s.reputation_per_mineral;
        let after = rank_at(s.ranks, state.resources.reputation);
        if after.is_some() && after != before {
            state.send_message(&format!("RANK UP: {}", after.unwrap().name));
        }
    }
}

/// Résultat d'un ravitaillement à la station (`purchase_supplies`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyOutcome {
    /// Réservoirs déjà pleins (rien à payer).
    Full,
    /// Ravitaillement payé (coût en minerais déduit).
    Purchased(i32),
    /// Pas assez de minerais (coût nécessaire).
    Insufficient(i32),
}

/// Achète du carburant et des munitions à la station : remplit les réservoirs
/// au prix du scénario (pas de `fuel_step`/`ammo_step`), déduit les minerais
/// et annonce le coût. Appelé par le bouton REFUEL/REARM de la boîte DOCK
/// STATION (plus d'achat automatique au déchargement).
pub fn purchase_supplies(state: &mut GameState) -> SupplyOutcome {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return SupplyOutcome::Full;
    }
    // capacités courantes (base + extensions d'atelier achetées)
    let max_fuel = fuel_capacity(state);
    let max_ammo = ammo_capacity(state);
    let missing_fuel = (max_fuel - state.resources.fuel).max(0.0);
    let missing_ammo = (max_ammo - state.resources.ammo).max(0);
    let fuel_cost = (missing_fuel / s.fuel_step).ceil() as i32 * s.fuel_price;
    let ammo_cost = ((missing_ammo + s.ammo_step - 1) / s.ammo_step) * s.ammo_price;
    // remise de réputation appliquée au total du ravitaillement
    let cost = discounted_cost(fuel_cost + ammo_cost, current_discount(state));
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.minerals < cost {
        // le message n'est envoyé qu'au début du manque (pas à chaque frame
        // tant que le joueur reste à quai — `supplies_shortage_cost`)
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH MINERALS FOR SUPPLIES ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.minerals -= cost;
    state.resources.fuel = max_fuel;
    state.resources.ammo = max_ammo;
    state.send_message(&format!("SUPPLIES PURCHASED: -{} MINERALS", cost));
    SupplyOutcome::Purchased(cost)
}

// ─── Modes de déplacement ───────────────────────────────────────────────────

/// Coûts de déblocage d'un mode pas encore débloqué : tarif de base (prix
/// d'origine) et prix réellement payé (remise de réputation du rang courant
/// appliquée) — `None` = débloqué, ou pas d'économie. Affichés dans le
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

/// Coût en minerais d'un mode pas encore débloqué (`None` = débloqué, ou pas
/// d'économie) — affiché dans le magasin de la station (bouton SHOP de la
/// boîte DOCK STATION). C'est le prix réellement payé (remise de réputation
/// du rang courant appliquée) ; voir `mode_unlock_prices` pour le tarif de
/// base.
pub fn locked_cost(state: &GameState, mode: i32) -> Option<i32> {
    mode_unlock_prices(state, mode).map(|(_, discounted)| discounted)
}

/// Sélectionne un mode de déplacement dans le magasin de la station :
/// débloqué → appliqué immédiatement ; verrouillé → payé en minerais (si
/// possible, sinon message « NOT ENOUGH MINERALS ») puis appliqué. Renvoie
/// `true` si le mode demandé est devenu le mode courant.
pub fn try_select_mode(state: &mut GameState, mode: i32) -> bool {
    match locked_cost(state, mode) {
        None => {
            state.moving_mode = mode;
            true
        }
        Some(cost) => {
            if state.resources.minerals >= cost {
                state.resources.minerals -= cost;
                state.unlocked_modes[mode as usize] = true;
                state.moving_mode = mode;
                state.send_message(&format!(
                    "MODE {} UNLOCKED ({} MINERALS)",
                    mode_label(mode),
                    cost
                ));
                true
            } else {
                state.send_message(&format!(
                    "NOT ENOUGH MINERALS FOR {} ({} NEEDED)",
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
/// actuelle et prochaine extension (`None` = au max) — pour l'écran atelier
/// (`render::draw_shop_box`).
pub struct UpgradeLine {
    /// Libellé de la ligne (ex « FUEL TANK »).
    pub label: &'static str,
    /// Capacité actuelle.
    pub capacity: i32,
    /// Prochaine extension (nom, coût, bonus) — `None` = niveau max.
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
    /// Extension achetée (coût en minerais déduit, niveau +1).
    Purchased(i32),
    /// Pas assez de minerais (coût nécessaire).
    Insufficient(i32),
}

/// Achète la prochaine extension d'une ligne à l'atelier de la station : paie
/// en minerais et fait passer la ligne au niveau suivant — les réservoirs
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
    if state.resources.minerals < cost {
        state.send_message(&format!(
            "NOT ENOUGH MINERALS FOR {} ({} NEEDED)",
            next.name, cost
        ));
        return UpgradeOutcome::Insufficient(cost);
    }
    state.resources.minerals -= cost;
    match track {
        UpgradeTrackId::Fuel => {
            state.resources.fuel_level += 1;
            state.resources.fuel = fuel_capacity(state); // plein à la nouvelle capacité
        }
        UpgradeTrackId::Ammo => {
            state.resources.ammo_level += 1;
            state.resources.ammo = ammo_capacity(state);
        }
        UpgradeTrackId::Cargo => {
            state.resources.cargo_level += 1;
            state.player.cargo_size = cargo_capacity(state);
        }
    }
    state.send_message(&format!("{} PURCHASED: -{} MINERALS", next.name, cost));
    UpgradeOutcome::Purchased(cost)
}

// ─── Persistance de la progression ──────────────────────────────────────────

/// Clés du fichier de config (voir `persist.rs`) portant la progression d'un
/// scénario — le scénario choisi et sa sauvegarde :
/// - `scenario`        — scénario choisi (0 = jeu libre, 1 = Progression,
///   2 = Survival)
/// - `prog_minerals`   — minerais en banque (Progression)
/// - `prog_modes`      — modes de déplacement débloqués (masque binaire : bit
///   i = mode i débloqué, Progression)
/// - `prog_reputation` — réputation × 10 (entier, au dixième près,
///   Progression)
/// - `prog_lives`      — vies restantes (Survival)
/// - `prog_shield`     — bouclier restant × 10 (entier, au dixième près,
///   Survival)
/// - `prog_up_fuel`    — extensions de réservoir achetées (Progression)
/// - `prog_up_ammo`    — extensions de chargeur achetées (Progression)
/// - `prog_up_cargo`   — extensions de soute achetées (Progression)
const SCENARIO_KEY: &str = "scenario";
const PROG_MINERALS_KEY: &str = "prog_minerals";
const PROG_MODES_KEY: &str = "prog_modes";
const PROG_REPUTATION_KEY: &str = "prog_reputation";
const PROG_LIVES_KEY: &str = "prog_lives";
const PROG_SHIELD_KEY: &str = "prog_shield";
const PROG_UP_FUEL_KEY: &str = "prog_up_fuel";
const PROG_UP_AMMO_KEY: &str = "prog_up_ammo";
const PROG_UP_CARGO_KEY: &str = "prog_up_cargo";

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

/// Enregistre la progression courante dans un fichier de config donné :
/// toujours le scénario choisi, et les ressources du scénario — minerais,
/// modes débloqués et réputation en Progression, vies et bouclier en
/// Survival. Chaque scénario n'écrit que ses propres clés : les clés `prog_*`
/// de l'autre scénario ne sont pas réécrites (une partie Progression ne vide
/// pas la sauvegarde Survival, et inversement). Version chemin explicite
/// (tests).
pub fn save_progression_to(path: &Path, state: &GameState) -> io::Result<()> {
    crate::persist::set_i32_to(path, SCENARIO_KEY, state.scenario as i32)?;
    if has_economy(state) {
        crate::persist::set_i32_to(path, PROG_MINERALS_KEY, state.resources.minerals)?;
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
    }
    if has_survival(state) {
        crate::persist::set_i32_to(path, PROG_LIVES_KEY, state.resources.lives)?;
        crate::persist::set_i32_to(
            path,
            PROG_SHIELD_KEY,
            (state.resources.shield * 10.0).round() as i32,
        )?;
    }
    Ok(())
}

/// Enregistre la progression courante dans le fichier de config utilisateur
/// (voir `save_progression_to`). Appelé à chaque modification de la
/// progression (déchargement, ravitaillement REFUEL/REARM, achat de mode,
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
        _ => None,
    }
}

/// Scénario enregistré dans le fichier de config utilisateur (voir
/// `load_scenario_from`).
pub fn load_scenario() -> Option<ScenarioId> {
    load_scenario_from(&crate::persist::config_path())
}

/// Surimpose la progression enregistrée sur l'état courant (après
/// `apply_start`) : minerais, modes débloqués, réputation et niveaux
/// d'atelier en Progression, vies et bouclier en Survival. Les valeurs sont
/// bornées par les règles du scénario (jamais plus de vies ni de bouclier
/// que la capacité, jamais plus d'extensions que le nombre défini). En
/// Survival, une sauvegarde à 0 vie (partie terminée) repart sur le départ
/// complet. Le mode de déplacement enregistré (`moving_mode`) est restauré
/// s'il est débloqué par la sauvegarde (sinon le mode de départ du scénario
/// reste — jamais un mode non payé). Ne touche pas au scénario courant ; les
/// réservoirs repartent pleins à la **capacité courante** (extensions
/// comprises) et la soute est agrandie selon le niveau restauré. Sans effet
/// en jeu libre. Version chemin explicite (tests).
pub fn load_progression_from(path: &Path, state: &mut GameState) {
    let s = scenario(state.scenario);
    if s.has_economy {
        if let Some(minerals) = crate::persist::get_i32_from(path, PROG_MINERALS_KEY) {
            state.resources.minerals = minerals.max(0);
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
        // réservoirs pleins à la capacité courante (extensions comprises) et
        // soute à la taille du niveau restauré
        state.resources.fuel = fuel_capacity(state);
        state.resources.ammo = ammo_capacity(state);
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
}

/// Surimpose la progression enregistrée dans le fichier de config utilisateur
/// (voir `load_progression_from`). Appelé au lancement (après `apply_start`)
/// et après un changement de scénario (écran titre, touche N).
pub fn load_progression(state: &mut GameState) {
    load_progression_from(&crate::persist::config_path(), state);
}

/// Remet la progression du scénario courant à zéro (bouton RESET PROGRESSION
/// de l'écran de paramétrage) : les clés `prog_*` du fichier de config
/// (minerais, modes payés, réputation, extensions d'atelier, vies/bouclier)
/// et le mode de déplacement choisi (`moving_mode`) sont supprimées, puis les
/// règles de départ du scénario sont réappliquées (`apply_start`) : minerais
/// 0, seuls les modes gratuits (coût 0) débloqués, réputation nulle,
/// réservoirs pleins, mode de départ (REALISTIC en Progression). Les réglages
/// (musique, volume, rendu, fenêtre) et le scénario choisi sont conservés.
pub fn reset_progression(state: &mut GameState) {
    reset_progression_from(&crate::persist::config_path(), state);
}

/// Version chemin explicite de `reset_progression` (tests) : supprime les
/// clés de progression du fichier donné puis réapplique `apply_start`.
pub fn reset_progression_from(path: &Path, state: &mut GameState) {
    for key in [
        PROG_MINERALS_KEY,
        PROG_MODES_KEY,
        PROG_REPUTATION_KEY,
        PROG_UP_FUEL_KEY,
        PROG_UP_AMMO_KEY,
        PROG_UP_CARGO_KEY,
        PROG_LIVES_KEY,
        PROG_SHIELD_KEY,
        "moving_mode",
    ] {
        let _ = crate::persist::delete_key_from(path, key);
    }
    apply_start(state);
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
        assert!(prog.contains("minerais"));

        let surv = scenario_rules_text(ScenarioId::Survival);
        assert!(surv.contains("3 vies"));
        assert!(surv.contains("bouclier 3"));
        assert!(surv.contains("×1"));
        assert!(surv.contains("2 s d'invulnérabilité"));
    }

    #[test]
    fn scenario_rules_mark_values_with_scenario_color() {
        // les valeurs chiffrées (coûts, vies, bouclier, dégâts, rangs) portent
        // `color = Some(...)` — la couleur propre du scénario — et les libellés
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
        // le résumé d'écran titre décrit la sauvegarde restaurée : minerais,
        // modes débloqués et réputation (+ rang) en Progression ; vies et
        // bouclier en Survival ; aucune en jeu libre
        let free = GameState::new();
        assert!(save_summary(&free).contains("aucune sauvegarde"));

        let mut prog = progression_state();
        prog.resources.minerals = 42;
        prog.resources.reputation = 60.0; // ACE
        prog.unlocked_modes = [true, true, false, true];
        let summary = save_summary(&prog);
        assert!(summary.contains("minerais 42"));
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
        // les valeurs du résumé (minerais, modes, réputation, rang, vies,
        // bouclier) portent `color = Some(couleur du scénario)`, les libellés
        // `None` — mêmes segments que `save_summary`, pour la coloration à
        // l'écran titre
        let mut prog = progression_state();
        prog.resources.minerals = 42;
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
            "INERTIAL 15, 4 WAYS 30, DIRECTIONAL 45 minerais"
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
        assert!(try_fire(&mut s)); // pas de consommation
        assert_eq!(s.resources.ammo, 0);
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
        // verrouillés ; réservoir et chargeur pleins, pas de minerais,
        // réputation nulle
        let s = progression_state();
        assert!(has_economy(&s));
        assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
        assert_eq!(s.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
        assert_eq!(s.resources.ammo, PROGRESSION_SCENARIO.start_ammo);
        assert_eq!(s.resources.minerals, 0);
        assert_eq!(s.resources.reputation, 0.0);
        assert_eq!(locked_cost(&s, MOVING_MODE_REALISTIC), None);
        assert_eq!(locked_cost(&s, MOVING_MODE_INERTIAL), Some(15));
        assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), Some(30));
        assert_eq!(locked_cost(&s, MOVING_MODE_DIRECTIONAL), Some(45));
    }

    #[test]
    fn cycle_scenario_toggles_and_reapplies_start() {
        // jeu libre → Progression → Survival → jeu libre (touche N) ; chaque
        // bascule réapplique les règles de départ du scénario
        let mut s = GameState::new();
        cycle_scenario(&mut s);
        assert_eq!(s.scenario, ScenarioId::Progression);
        assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
        cycle_scenario(&mut s);
        assert_eq!(s.scenario, ScenarioId::Survival);
        assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
        assert_eq!(s.resources.shield, SURVIVAL_SCENARIO.shield_capacity);
        assert!(s.unlocked_modes.iter().all(|&u| u));
        cycle_scenario(&mut s);
        assert_eq!(s.scenario, ScenarioId::FreePlay);
        assert_eq!(s.resources, Resources::default());
        assert!(s.unlocked_modes.iter().all(|&u| u));
    }

    #[test]
    fn cycle_scenario_back_goes_to_previous() {
        // touche B : inverse de N — jeu libre → Survival → Progression
        let mut s = GameState::new();
        cycle_scenario_back(&mut s);
        assert_eq!(s.scenario, ScenarioId::Survival);
        assert_eq!(s.resources.lives, SURVIVAL_SCENARIO.lives);
        cycle_scenario_back(&mut s);
        assert_eq!(s.scenario, ScenarioId::Progression);
        assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
        cycle_scenario_back(&mut s);
        assert_eq!(s.scenario, ScenarioId::FreePlay);
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
        // 4 WAYS coûte 30 minerais : payé, débloqué définitivement (la
        // re-sélection est ensuite gratuite) ; sans assez de minerais, refus
        let mut s = progression_state();
        s.resources.minerals = 30;
        assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
        assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
        assert_eq!(s.resources.minerals, 0);
        assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), None);
        // un mode gratuit (REALISTIC) reste re-sélectionnable sans frais
        assert!(try_select_mode(&mut s, MOVING_MODE_REALISTIC));
        assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
        assert_eq!(s.resources.minerals, 0);
        // INERTIAL coûte 15 : pas assez (0) → refus, mode inchangé
        assert!(!try_select_mode(&mut s, MOVING_MODE_INERTIAL));
        assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
        // DIRECTIONAL coûte 45 : pas assez (0) → refus, mode inchangé
        assert!(!try_select_mode(&mut s, MOVING_MODE_DIRECTIONAL));
        assert_eq!(s.moving_mode, MOVING_MODE_4_WAYS);
        assert!(s.message_queue.contains("NOT ENOUGH MINERALS"));
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
        let mut s = progression_state();
        assert!(try_fire(&mut s));
        assert_eq!(s.resources.ammo, PROGRESSION_SCENARIO.start_ammo - 1);
        s.resources.ammo = 1;
        assert!(try_fire(&mut s));
        assert!(s.message_queue.contains("OUT OF AMMO"));
        assert!(!try_fire(&mut s)); // chargeur vide
        assert_eq!(s.resources.ammo, 0);
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
        // CADET 0, PILOT 10, VETERAN 25, ACE 50 — le rang courant est le plus
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
        assert!(try_fire(&mut s.clone())); // pas de consommation
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
        assert_eq!(s.resources.minerals, 15);
        assert!(s.message_queue.contains("+15 MINERALS"));

        let mut f = GameState::new();
        unload_cargo(&mut f, &elements);
        assert_eq!(f.resources.minerals, 0);
        assert!(f.message_queue.is_empty());
    }

    #[test]
    fn cargo_unload_grants_reputation_and_rank_up() {
        // le commerce est récompensé : chaque minerai déchargé rapporte de la
        // réputation (0,1 en Progression) — 100 minerais → +10 → le seuil
        // PILOT (10) est franchi, « RANK UP: PILOT » est annoncé
        let mut s = progression_state();
        let mut elements = default_elements();
        elements[1].count = 20; // GOLD ×20 = 100 minerais
        unload_cargo(&mut s, &elements);
        assert_eq!(s.resources.minerals, 100);
        assert!(
            (s.resources.reputation - 10.0).abs() < 1e-9,
            "réputation {}",
            s.resources.reputation
        );
        assert!(s.message_queue.contains("RANK UP: PILOT"));

        // en jeu libre, pas d'économie : ni minerais ni réputation
        let mut f = GameState::new();
        unload_cargo(&mut f, &elements);
        assert_eq!(f.resources.reputation, 0.0);
    }

    #[test]
    fn supplies_are_purchased_and_charge_minerals() {
        // réservoir à moitié vide : plein payé au pas (10 carburant = 1
        // minerai, 5 munitions = 1) et déduit des minerais
        let mut s = progression_state();
        s.resources.minerals = 100;
        s.resources.fuel = 50.0;
        s.resources.ammo = 10;
        match purchase_supplies(&mut s) {
            SupplyOutcome::Purchased(cost) => {
                assert_eq!(cost, 9); // 5 × 1 (carburant) + 4 × 1 (munitions)
                assert_eq!(s.resources.minerals, 100 - cost);
            }
            _ => panic!("achat attendu"),
        }
        assert_eq!(s.resources.fuel, fuel_capacity(&s)); // 100 (base)
        assert_eq!(s.resources.ammo, ammo_capacity(&s)); // 30 (base)
        assert!(s.message_queue.contains("SUPPLIES PURCHASED"));
    }

    #[test]
    fn supplies_are_refused_without_enough_minerals() {
        // ravitaillement complet (100 carburant + 30 munitions = 16 minerais)
        // refusé avec seulement 2 : réservoirs inchangés, message envoyé une
        // seule fois (pas de répétition à chaque frame à quai)
        let mut s = progression_state();
        s.resources.minerals = 2;
        s.resources.fuel = 0.0;
        s.resources.ammo = 0;
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Insufficient(16));
        assert_eq!(s.resources.fuel, 0.0);
        assert!(s.message_queue.contains("NOT ENOUGH MINERALS"));
        let queue = s.message_queue.clone();
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Insufficient(16));
        assert_eq!(s.message_queue, queue); // pas de nouveau message
        // minerais obtenus : le ravitaillement est accepté et le manque effacé
        s.resources.minerals = 16;
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Purchased(16));
        assert_eq!(s.supplies_shortage_cost, 0);
    }

    #[test]
    fn full_tank_costs_nothing() {
        // réservoirs pleins : rien à payer, aucun message
        let mut s = progression_state();
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Full);
        assert!(s.message_queue.is_empty());
    }

    // ─── persistance de la progression ─────────────────────────────────────

    #[test]
    fn progression_save_and_restore_round_trips() {
        // une partie Progression (minerais, modes payés, réputation) est
        // enregistrée puis restaurée sur un départ neuf du scénario — les
        // réservoirs, eux, repartent pleins (non persistés)
        let p = temp_path("roundtrip.cfg");
        let _ = std::fs::remove_file(&p);
        let mut s = progression_state();
        s.resources.minerals = 42;
        s.resources.reputation = 3.5;
        s.unlocked_modes = [true, true, false, true];
        save_progression_to(&p, &s).unwrap();

        let mut fresh = progression_state();
        assert_eq!(fresh.resources.minerals, 0); // départ neuf
        load_progression_from(&p, &mut fresh);
        assert_eq!(fresh.resources.minerals, 42);
        assert_eq!(fresh.resources.reputation, 3.5);
        assert_eq!(fresh.unlocked_modes, [true, true, false, true]);
        assert_eq!(fresh.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
        assert_eq!(fresh.resources.ammo, PROGRESSION_SCENARIO.start_ammo);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn reset_progression_clears_saved_progression() {
        // RESET PROGRESSION : une progression (minerais, modes payés,
        // réputation, extensions, mode choisi) est remise à zéro — les clés
        // `prog_*` et `moving_mode` du fichier sont supprimées et l'état
        // repart sur les règles de départ du scénario (REALISTIC seul
        // débloqué, réservoirs pleins)
        let p = temp_path("resetprog.cfg");
        let _ = std::fs::remove_file(&p);
        let mut s = progression_state();
        s.resources.minerals = 77;
        s.resources.reputation = 50.0;
        s.resources.fuel_level = 2;
        s.unlocked_modes = [true, true, true, true];
        s.moving_mode = MOVING_MODE_4_WAYS;
        save_progression_to(&p, &s).unwrap();
        set_i32_to(&p, "moving_mode", MOVING_MODE_4_WAYS).unwrap();

        reset_progression_from(&p, &mut s);
        // état remis au départ : minerais 0, réputation nulle, extensions 0,
        // seul REALISTIC (gratuit) débloqué, mode de départ
        assert_eq!(s.resources.minerals, 0);
        assert_eq!(s.resources.reputation, 0.0);
        assert_eq!(s.resources.fuel_level, 0);
        assert_eq!(s.unlocked_modes, [false, false, false, true]);
        assert_eq!(s.moving_mode, MOVING_MODE_REALISTIC);
        assert_eq!(s.resources.fuel, PROGRESSION_SCENARIO.start_fuel);
        assert_eq!(s.resources.ammo, PROGRESSION_SCENARIO.start_ammo);

        // clés de progression supprimées du fichier (mode compris) : un
        // rechargement sur un départ neuf ne retrouve aucune progression
        assert_eq!(get_i32_from(&p, "prog_minerals"), None);
        assert_eq!(get_i32_from(&p, "prog_modes"), None);
        assert_eq!(get_i32_from(&p, "prog_reputation"), None);
        assert_eq!(get_i32_from(&p, "prog_up_fuel"), None);
        assert_eq!(get_i32_from(&p, "moving_mode"), None);
        let mut fresh = progression_state();
        load_progression_from(&p, &mut fresh);
        assert_eq!(fresh.resources.minerals, 0);
        assert_eq!(fresh.unlocked_modes, [false, false, false, true]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn refuel_spent_minerals_are_persisted() {
        // un ravitaillement déduit des minerais : la sauvegarde doit conserver
        // la valeur déduite — pas de ravitaillement gratuit au lancement
        // suivant (le jeu écrit la progression après chaque REFUEL/REARM)
        let p = temp_path("refuel.cfg");
        let _ = std::fs::remove_file(&p);
        let mut s = progression_state();
        s.resources.minerals = 100;
        s.resources.fuel = 50.0;
        s.resources.ammo = 10;
        let cost = match purchase_supplies(&mut s) {
            SupplyOutcome::Purchased(c) => c,
            _ => panic!("achat attendu"),
        };
        assert_eq!(s.resources.minerals, 100 - cost);
        save_progression_to(&p, &s).unwrap();

        let mut fresh = progression_state();
        load_progression_from(&p, &mut fresh);
        assert_eq!(fresh.resources.minerals, 100 - cost); // dépense conservée
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn free_play_save_does_not_clobber_progression() {
        // enregistrer une partie libre ne réécrit pas les clés `prog_*` : la
        // sauvegarde d'un scénario à économie survit (seul `scenario` change)
        let p = temp_path("freeplay.cfg");
        let _ = std::fs::remove_file(&p);
        let mut prog = progression_state();
        prog.resources.minerals = 77;
        save_progression_to(&p, &prog).unwrap();

        let free = GameState::new(); // jeu libre
        save_progression_to(&p, &free).unwrap();
        assert_eq!(get_i32_from(&p, "scenario"), Some(0));
        assert_eq!(get_i32_from(&p, "prog_minerals"), Some(77)); // conservés
        // seul REALISTIC est débloqué au départ (INERTIAL est payant)
        assert_eq!(get_i32_from(&p, "prog_modes"), Some(0b1000));
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
        prog.resources.minerals = 55;
        save_progression_to(&p, &prog).unwrap();

        let mut free = GameState::new();
        load_progression_from(&p, &mut free);
        assert_eq!(free.resources.minerals, 0);
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
        // sans réécrire celles de Progression (minerais, modes, réputation)
        let p = temp_path("survival.cfg");
        let _ = std::fs::remove_file(&p);
        let mut prog = progression_state();
        prog.resources.minerals = 33;
        save_progression_to(&p, &prog).unwrap();

        let mut surv = survival_state();
        surv.resources.lives = 2;
        surv.resources.shield = 1.5;
        save_progression_to(&p, &surv).unwrap();
        assert_eq!(get_i32_from(&p, "scenario"), Some(2));
        assert_eq!(get_i32_from(&p, "prog_lives"), Some(2));
        assert_eq!(get_i32_from(&p, "prog_shield"), Some(15)); // 1,5 × 10
        assert_eq!(get_i32_from(&p, "prog_minerals"), Some(33)); // conservés
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
        // l'extension de réservoir paie en minerais, monte le niveau et le
        // réservoir repart plein à la nouvelle capacité ; la soute s'agrandit
        // immédiatement
        let mut s = progression_state();
        s.resources.minerals = 50;
        assert_eq!(fuel_capacity(&s), 100.0);
        assert_eq!(
            buy_upgrade(&mut s, UpgradeTrackId::Fuel),
            UpgradeOutcome::Purchased(10)
        );
        assert_eq!(s.resources.fuel_level, 1);
        assert_eq!(s.resources.minerals, 40);
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
        // pas assez de minerais : refus, niveau et minerais inchangés ; au
        // niveau max : plus d'achat ; hors scénario à économie : pas d'atelier
        let mut s = progression_state();
        s.resources.minerals = 5;
        assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Insufficient(10));
        assert_eq!(s.resources.fuel_level, 0);
        assert!(s.message_queue.contains("NOT ENOUGH MINERALS"));

        s.resources.minerals = 1000;
        s.resources.fuel_level = 3;
        assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Maxed);
        assert_eq!(s.resources.minerals, 1000);

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
        // 100 % : 30 % — sans tir, la précision vaut 0
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
        s.resources.minerals = 1000;
        let outcome = buy_upgrade(&mut s, UpgradeTrackId::Ammo);
        assert_eq!(outcome, UpgradeOutcome::Purchased(7)); // 10 × 70 % = 7
    }

    #[test]
    fn discounted_cost_is_rounded_down_and_never_negative() {
        // coût × (100 − remise) / 100, arrondi à l'entier inférieur — la
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
        // station — atelier, ravitaillement et déblocage des modes
        let mut s = progression_state();
        s.resources.minerals = 1000;
        s.resources.reputation = 50.0;

        // atelier : extension de réservoir 10 → 8 (10 × 85 % = 8,5 → 8), et
        // l'atelier affiche le prix remisé
        assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Fuel), UpgradeOutcome::Purchased(8));
        assert_eq!(s.resources.minerals, 992);
        let line = upgrade_line(&s, UpgradeTrackId::Ammo);
        assert_eq!(line.next.map(|u| u.cost), Some(8), "prix affiché remisé (10 → 8)");

        // ravitaillement : 13 pas de carburant (130/10) + 6 pas de munitions
        // (30/5) = 19 → 16 remisés
        s.resources.fuel = 0.0;
        s.resources.ammo = 0;
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Purchased(16));
        assert_eq!(s.resources.minerals, 976);

        // modes payants : 4 WAYS 30 → 25 (tarif de base et prix remisé
        // exposés pour l'affichage du magasin)
        assert_eq!(locked_cost(&s, MOVING_MODE_4_WAYS), Some(25));
        assert_eq!(mode_unlock_prices(&s, MOVING_MODE_4_WAYS), Some((30, 25)));
        s.resources.minerals = 1000;
        assert!(try_select_mode(&mut s, MOVING_MODE_4_WAYS));
        assert_eq!(s.resources.minerals, 975);
    }

    #[test]
    fn supplies_fill_to_upgraded_capacity() {
        // après une extension de chargeur, le ravitaillement remplit la
        // nouvelle capacité (et le prix est recalculé sur ce qu'il manque)
        let mut s = progression_state();
        s.resources.minerals = 200;
        assert_eq!(buy_upgrade(&mut s, UpgradeTrackId::Ammo), UpgradeOutcome::Purchased(10));
        assert_eq!(ammo_capacity(&s), 40);
        s.resources.ammo = 20;
        // 20 munitions manquantes = 4 pas de 5 × 1 minerai (carburant plein)
        assert_eq!(purchase_supplies(&mut s), SupplyOutcome::Purchased(4));
        assert_eq!(s.resources.ammo, 40);
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
        s.resources.minerals = 77;
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
        assert_eq!(t.resources.ammo, 40); // 30 + 10
        assert_eq!(t.player.cargo_size, 7); // 5 + 2
        assert_eq!(t.resources.minerals, 77);

        std::fs::write(&p, "prog_up_fuel=99\n").unwrap();
        let mut u = progression_state();
        load_progression_from(&p, &mut u);
        assert_eq!(u.resources.fuel_level, 3); // borné au max d'extensions
        assert_eq!(u.resources.fuel, 220.0);
        let _ = std::fs::remove_file(&p);
    }
}
