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

use crate::config::{
    MOVING_MODE_COUNT, MOVING_MODE_DIRECTIONAL, MOVING_MODE_REALISTIC, WEAPON_SLOTS,
};
use crate::state::{Element, GameState};

// ─── Sous-modules (découpage thématique, API réexportée) ────────────────────
//
// Le magasin, les rangs de réputation, l'atelier et la persistance de la
// progression vivent dans des fichiers dédiés ; tout est **réexporté** ici :
// les appels `crate::scenario::…` du reste du code et les tests ne changent
// pas. Les éléments privés de `scenario.rs` (définitions, règles affichées)
// restent accessibles aux sous-modules via `use super::*`.

mod craft;
mod progression;
mod ranks;
mod rules;
mod shop;
mod workshop;

pub use craft::*;
pub use progression::*;
pub use ranks::*;
pub use rules::*;
pub use shop::*;
pub use workshop::*;

#[cfg(test)]
mod tests;

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
    mode_label, ReputationRank, ShipUpgrade, UpgradeTrack, AMMO_PRICE, AMMO_STEP,
    AMMO_UPGRADE_TRACK, CARGO_UPGRADE_TRACK, DISCOUNT_PRECISION_WEIGHT, ELEMENT_VALUES, FUEL_PRICE,
    FUEL_STEP, FUEL_UPGRADE_TRACK, MODE_COSTS, PROGRESSION_RANKS,
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
    reputation_per_asteroid: 0.0,
    reputation_precision_weight: 0.0,
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
    reputation_per_mineral: 0.1,      // 10 minerais déchargés → +1 de réputation
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
    // annonce « NEW RECORD » réarmée pour la nouvelle partie (le record
    // enregistré lui-même survit - voir `load_progression`)
    state.score_record_announced = false;
    // statistiques de session, journal de bord, consommables et minuteurs
    // des vagues (difficulté, boss, portails) remis à zéro pour la nouvelle
    // partie
    state.reset_session();
    // l'écran de briefing pré-partie (scénarios custom avec objectifs) est
    // ré-armé - il s'affichera au lancement de la partie (`main.rs`)
    state.briefing_box = false;
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
            let (json_credits, json_reputation) = crate::scenario_loader::loaded_data(ci)
                .map(|d| {
                    (
                        d.json.initial_state.start_credits,
                        d.json.initial_state.start_reputation,
                    )
                })
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
        state.send_message("GAME OVER - R: NEW GAME - T: TITLE");
        return PlayerHit::GameOver;
    }
    // respawn : bouclier rechargé et invulnérabilité temporaire (la position
    // du vaisseau est restaurée par `game.rs` - `respawn_player`)
    state.resources.shield = s.shield_capacity;
    state.invulnerable = s.respawn_invulnerability;
    state.send_message(&format!(
        "SHIP DESTROYED - {} {} LEFT",
        state.resources.lives,
        if state.resources.lives > 1 {
            "LIVES"
        } else {
            "LIFE"
        }
    ));
    PlayerHit::Destroyed(state.resources.lives)
}

// ─── Score composite et record (high-score) ─────────────────────────────

/// Poids du score composite : crédits gagnés (déchargés à la station +
/// récompenses d'objectifs DAG) et astéroïdes détruits pèsent 1 point par
/// unité, chaque objectif DAG complété 50 points.
pub const SCORE_PER_OBJECTIVE: i32 = 50;

/// Score composite de la partie courante : crédits **gagnés** cumulés
/// (`credits_earned` - pas le solde courant, que les achats diminuent) +
/// astéroïdes détruits + 50 points par objectif DAG complété. Fonction pure
/// (tests). Affiché au HUD avec le record (`render.rs`).
pub fn composite_score(state: &GameState) -> i32 {
    state.credits_earned
        + state.meteors_destroyed
        + state.objective_tracker.completed_ids.len() as i32 * SCORE_PER_OBJECTIVE
}

/// Clé de config du record d'un scénario (index global - voir
/// `scenario_index`) : `highscore_0` = jeu libre, `highscore_1` = Progression,
/// etc. Les records des scénarios custom suivent l'ordre de chargement.
pub fn high_score_key(id: ScenarioId) -> String {
    format!("highscore_{}", scenario_index(id))
}

/// Met à jour le record du scénario courant si le score composite courant le
/// dépasse : `state.high_score` (affiché à l'écran titre) est relevé et la
/// clé `highscore_<index>` persistée dans le fichier de config. Appelé aux
/// mêmes moments que la sauvegarde de progression (déchargement, astéroïde
/// détruit, sortie du jeu). Renvoie `true` si le record a été battu.
pub fn maybe_update_high_score(state: &mut GameState) -> bool {
    let score = composite_score(state);
    if score > state.high_score {
        // « NEW RECORD » une seule fois par session, et seulement quand un
        // record **enregistré** (non nul) est dépassé : sans ça, l'annonce
        // se répéterait à chaque astéroïde détruit (chaque point bat le
        // record fraîchement relevé) - et le tout premier record d'un
        // scénario (record 0) reste silencieux
        let announce = !state.score_record_announced && state.high_score > 0;
        state.score_record_announced = true;
        state.high_score = score;
        let _ = crate::persist::set_i32(
            &high_score_key(state.scenario),
            score,
        );
        if announce {
            state.send_message(&format!("NEW RECORD: {}", score));
        }
        true
    } else {
        false
    }
}

/// Lit le record enregistré d'un scénario dans un fichier de config donné
/// (version testable ; clé absente → 0).
pub fn load_high_score_from(path: &std::path::Path, id: ScenarioId) -> i32 {
    crate::persist::get_i32_from(path, &high_score_key(id)).unwrap_or(0)
}
