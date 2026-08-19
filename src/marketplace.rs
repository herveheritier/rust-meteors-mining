//! Place de marché de la station — extensions de vaisseau de l'atelier,
//! réglages du vaisseau joueur et économie (scénario Progression, bouton
//! UPGRADES de la boîte DOCK STATION).
//!
//! Fichier **généré** par `tools/marketplace-editor/index.html` — ne pas
//! éditer à la main : régénérez-le depuis l'outil de gestion, puis
//! recompilez (`cargo build --release`). Les tests (`cargo test`) valident
//! les nouvelles valeurs.

use crate::config::{CARGO_SIZE, MOVING_MODE_COUNT};

/// Valeur en minerais d'une gemme par élément (index 1..3 = GOLD, IRON,
/// WATER — voir `default_elements` ; 0 = sans valeur).
pub const ELEMENT_VALUES: [i32; 4] = [0, 5, 3, 2];

/// Coût en minerais pour débloquer chaque mode de déplacement (index
/// `MOVING_MODE_*` ; 0 = déjà débloqué) : INERTIAL gratuit, 4 WAYS 20, DIRECTIONAL 50.
pub const MODE_COSTS: [i32; MOVING_MODE_COUNT as usize] = [0, 20, 50];

/// Prix (minerais) d'un plein de carburant par pas de `fuel_step` unités.
pub const FUEL_PRICE: i32 = 1;

/// Pas de ravitaillement en carburant (unités par plein facturé).
pub const FUEL_STEP: f64 = 10.0;

/// Prix (minerais) d'un plein de munitions par pas de `ammo_step` unités.
pub const AMMO_PRICE: i32 = 1;

/// Pas de ravitaillement en munitions.
pub const AMMO_STEP: i32 = 5;

/// Poids de la **précision de tir** sur la remise de réputation : la remise
/// du rang est multipliée par `1 + poids × précision` (précision en 0..1 =
/// part de tirs non perdus) — 1.0 = 100 % de précision → remise doublée.
/// 0 = la précision n'influe pas sur les coûts.
pub const DISCOUNT_PRECISION_WEIGHT: f64 = 1.0;

/// Rangs de réputation du scénario Progression (seuils croissants, remise
/// croissante) : CADET (0.0) → PILOT (10.0), −5 % → VETERAN (25.0), −10 % → ACE (50.0), −15 %.
/// Générés par l'outil de gestion.
pub const PROGRESSION_RANKS: &[ReputationRank] = &[
    ReputationRank { threshold: 0.0, name: "CADET", discount_percent: 0 },
    ReputationRank { threshold: 10.0, name: "PILOT", discount_percent: 5 },
    ReputationRank { threshold: 25.0, name: "VETERAN", discount_percent: 10 },
    ReputationRank { threshold: 50.0, name: "ACE", discount_percent: 15 },
];

/// Vaisseau joueur — mesh « meshes-designer » et réglages (échelle,
/// orientation, centre de rotation). Données générées par l'outil de
/// gestion : `src/vaisseau.rs` les lit pour construire le vaisseau.
///
/// Fichier mesh embarqué au compile (`include_str!`) : chemin relatif
/// à la racine du projet.
pub const VAISSEAU_JSON: &str = include_str!("../assets/vaisseau.json");

/// Échelle du vaisseau (multiplicateur des sommets du mesh) : 1.0 = 100 %.
pub const VAISSEAU_SCALE: f64 = 1.6;

/// Orientation de départ du vaisseau (degrés) : angle du nez du mesh
/// dans l'éditeur « meshes-designer » (0 = nez vers la droite, +90 =
/// vers le haut). Le mesh est tourné de −orientation à la construction.
pub const VAISSEAU_ORIENTATION_DEGREES: f64 = 0.0;

/// Centre de rotation du vaisseau : position du pivot en pourcentage de
/// la boîte englobante du mesh (50 = centre), axe x.
pub const VAISSEAU_CENTER_X_PERCENT: f64 = 50.0;

/// Centre de rotation du vaisseau (voir `VAISSEAU_CENTER_X_PERCENT`), axe y.
pub const VAISSEAU_CENTER_Y_PERCENT: f64 = 50.0;

/// Cosmonaute EVA (pilote éjecté, vaisseau détruit) — mesh « meshes-designer »
/// et réglages (échelle, orientation, centre de rotation). Données générées
/// par l'outil de gestion : `src/cosmonaut.rs` les lit pour construire le
/// pilote contrôlé en mode EVA (scénarios Progression et Survival).
///
/// Fichier mesh embarqué au compile (`include_str!`) : chemin relatif
/// à la racine du projet.
pub const COSMONAUTE_JSON: &str = include_str!("../assets/cosmonaute.json");

/// Échelle du cosmonaute EVA (multiplicateur des sommets du mesh) : 1.0 = 100 %.
pub const COSMONAUTE_EVA_SCALE: f64 = 1.5;

/// Orientation du cosmonaute EVA (degrés) : angle de l'avant du mesh dans
/// l'éditeur « meshes-designer » (0 = face à droite, +90 = vers le haut).
/// Le mesh est tourné de −orientation à la construction.
pub const COSMONAUTE_ORIENTATION_DEGREES: f64 = 0.0;

/// Centre de rotation du cosmonaute EVA : position du pivot en pourcentage de
/// la boîte englobante du mesh (50 = centre), axe x.
pub const COSMONAUTE_CENTER_X_PERCENT: f64 = 51.0;

/// Centre de rotation du cosmonaute EVA (voir `COSMONAUTE_CENTER_X_PERCENT`),
/// axe y.
pub const COSMONAUTE_CENTER_Y_PERCENT: f64 = 59.0;

/// Une extension de vaisseau achetable à l'atelier de la station (scénario
/// Progression, bouton UPGRADES de la boîte DOCK STATION) : ajoute de la
/// capacité (réservoir, chargeur ou soute) au prix indiqué, payé en minerais.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShipUpgrade {
    /// Nom de l'extension (atelier, HUD).
    pub name: &'static str,
    /// Coût en minerais.
    pub cost: i32,
    /// Capacité ajoutée (carburant, munitions ou soute).
    pub bonus: i32,
}

/// Une ligne d'amélioration du vaisseau (atelier) : capacité de base et
/// extensions successives achetées **dans l'ordre** — le niveau est le nombre
/// d'extensions possédées (`tiers[i]` s'achète au niveau `i+1`).
#[derive(Clone, Copy, Debug)]
pub struct UpgradeTrack {
    /// Libellé de la ligne (atelier).
    pub label: &'static str,
    /// Capacité de base (niveau 0).
    pub base: i32,
    /// Extensions par niveau, achetées dans l'ordre.
    pub tiers: &'static [ShipUpgrade],
}

/// Rang de réputation : palier débloqué à partir d'un seuil de réputation,
/// avec le nom affiché (HUD, message « RANK UP ») et la **remise** accordée
/// sur les coûts de la station (pourcentage 0..100) dès ce rang atteint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReputationRank {
    /// Réputation minimale pour atteindre ce rang.
    pub threshold: f64,
    /// Nom du rang (HUD, message « RANK UP »).
    pub name: &'static str,
    /// Remise sur les coûts de la station (atelier, ravitaillement, modes de
    /// déplacement) accordée dès ce rang atteint.
    pub discount_percent: i32,
}

/// Extensions de réservoir (Progression) : 100 → 130 → 170 → 220 unités.
const FUEL_UPGRADES: &[ShipUpgrade] = &[
    ShipUpgrade { name: "RÉSERVOIR SUPPLÉMENTAIRE", cost: 10, bonus: 30 },
    ShipUpgrade { name: "RÉSERVOIR HAUTE CAPACITÉ", cost: 30, bonus: 40 },
    ShipUpgrade { name: "RÉSERVOIR DOUBLE", cost: 60, bonus: 50 },
];

/// Ligne « réservoir de carburant » (atelier, Progression) : 100 de base,
/// 3 extensions → 220 max.
pub const FUEL_UPGRADE_TRACK: UpgradeTrack = UpgradeTrack {
    label: "FUEL TANK",
    base: 100,
    tiers: FUEL_UPGRADES,
};

/// Extensions de chargeur (Progression) : 30 → 40 → 55 → 70 munitions.
const AMMO_UPGRADES: &[ShipUpgrade] = &[
    ShipUpgrade { name: "CHARGEUR ÉLARGI", cost: 10, bonus: 10 },
    ShipUpgrade { name: "CHARGEUR HAUTE CAPACITÉ", cost: 20, bonus: 15 },
    ShipUpgrade { name: "CHARGEUR DOUBLE", cost: 40, bonus: 15 },
];

/// Ligne « chargeur de munitions » (atelier, Progression) : 30 de base,
/// 3 extensions → 70 max.
pub const AMMO_UPGRADE_TRACK: UpgradeTrack = UpgradeTrack {
    label: "MAGAZINE",
    base: 30,
    tiers: AMMO_UPGRADES,
};

/// Extensions de soute (Progression) : 5 → 7 → 10 emplacements.
const CARGO_UPGRADES: &[ShipUpgrade] = &[
    ShipUpgrade { name: "SOUTE AGRANDIE", cost: 20, bonus: 2 },
    ShipUpgrade { name: "SOUTE HAUTE CAPACITÉ", cost: 40, bonus: 3 },
];

/// Ligne « soute » (atelier, Progression) : 5 de base,
/// 2 extensions → 10 max.
pub const CARGO_UPGRADE_TRACK: UpgradeTrack = UpgradeTrack {
    label: "CARGO BAY",
    base: CARGO_SIZE,
    tiers: CARGO_UPGRADES,
};
