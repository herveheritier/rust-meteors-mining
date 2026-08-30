//! Place de marché de la station - extensions de vaisseau et modes de
//! déplacement (bouton SHOP de la boîte DOCK STATION), réglages du vaisseau
//! joueur et économie (scénario Progression).
//!
//! Fichier **généré** par `tools/marketplace-editor/index.html` - ne pas
//! éditer à la main : régénérez-le depuis l'outil de gestion, puis
//! recompilez (`cargo build --release`). Les tests (`cargo test`) valident
//! les nouvelles valeurs.

use crate::config::{CARGO_SIZE, MOVING_MODE_COUNT};

/// Valeur en crédits d.un minerai par élément (index 1..4 = GOLD, IRON,
/// WATER, PLATINUM - voir `default_elements` ; 0 = sans valeur). Le
/// PLATINUM (10 CR, minerai rare du météore spécial) a été ajouté à la main
/// dans ce fichier généré : l'outil de gestion ne le connaît pas encore -
/// régénérer depuis l'outil réécrira cette constante (reporter la valeur).
pub const ELEMENT_VALUES: [i32; 5] = [0, 5, 3, 2, 10];

/// Modes de déplacement du vaisseau (index `MOVING_MODE_*` du jeu, ordre
/// historique - l'ordre d'affichage dans le magasin est `MOVING_MODE_ORDER`) :
/// nom et description affichés (magasin de la station, messages du scénario)
/// et coût de déblocage en crédits (0 = déjà débloqué au départ). Générés
/// par l'outil de gestion.
pub const MOVING_MODES: &[MovingMode] = &[
    MovingMode { name: "INERTIAL", description: "THRUST / REVERSE, TURN L/R", cost: 15 },
    MovingMode { name: "4 WAYS", description: "ARROWS PUSH IN CURRENT DIR", cost: 30 },
    MovingMode { name: "DIRECTIONAL", description: "ACCELERATE / BRAKE, TURN L/R", cost: 45 },
    MovingMode { name: "REALISTIC", description: "INERTIAL + ROTATION DRIFT", cost: 0 },
];

/// Coût en crédits pour débloquer chaque mode de déplacement (index
/// `MOVING_MODE_*` ; 0 = déjà débloqué) - dérivé de `MOVING_MODES`. Seuls
/// les modes à coût nul sont débloqués au départ en Progression (REALISTIC
/// par défaut) ; les autres s'achètent au magasin.
pub const MODE_COSTS: [i32; MOVING_MODE_COUNT as usize] = [15, 30, 45, 0];

/// Prix (crédits) d'un plein de carburant par pas de `fuel_step` unités.
pub const FUEL_PRICE: i32 = 1;

/// Pas de ravitaillement en carburant (unités par plein facturé).
pub const FUEL_STEP: f64 = 10.0;

/// Prix (crédits) d'un plein de munitions par pas de `ammo_step` unités.
pub const AMMO_PRICE: i32 = 1;

/// Pas de ravitaillement en munitions.
pub const AMMO_STEP: i32 = 5;

/// Coût en crédits du **radar de bord** (minimap globale affichée en scénario
/// à économie - éteinte par défaut, achetée au magasin, onglet ÉQUIPEMENT).
/// Hors économie le radar est toujours allumé (gratuit, comportement
/// historique). Généré par l'outil de gestion.
pub const RADAR_COST: i32 = 20;

/// Météores - collisions avec la station et génération (mise au point) : les
/// constantes ci-dessous sont générées par l'outil de gestion et lues par
/// `src/game.rs` (réaction à la base), `src/garbage.rs` (débris),
/// `src/generate.rs` et `src/state.rs` (génération et population).
///
/// Force de réaction d'un météore qui percute la **station** : le triangle
/// qui collisionne explose et le météore est repoussé - sa composante de
/// vitesse **radiale** (vers la base) est réfléchie avec cette restitution,
/// la composante tangentielle (glissement le long de l'anneau) est
/// conservée. 1.0 = rebond parfait (l'explosion rend au météore la vitesse
/// de l'impact) ; 0 = pas de réaction.
pub const METEOR_STATION_RESTITUTION: f64 = 0.2;

/// Débris générés par triangle détruit (l'explosion d'un triangle de
/// météore sur la base ou un autre météore).
pub const GARBAGE_PER_TRIANGLE: usize = 12;

/// Vitesse maximale des météores (`2*rnd` à l'origine).
pub const METEOR_VELOCITY_MAX: f64 = 2.0;

/// Tournoiement des météores : la vitesse de rotation est inversement
/// proportionnelle à la taille — un petit débris tourne vite, un gros
/// astéroïde tourne lentement (comportement réaliste des débris).
/// Vitesse angulaire de base (rad/s) pour un météore au nombre minimal de
/// triangles (TRIANGLES_IN_SHAPE_MIN) ; pour les autres :
/// `rotation = METEOR_SPIN_BASE × TRIANGLES_IN_SHAPE_MIN / nbr`.
pub const METEOR_SPIN_BASE: f64 = 0.9;

/// Vitesse angulaire maximale (rad/s) : plafond pour les plus petits
/// éclats, pour que la rotation ne devienne pas un scintillement à 60 fps.
pub const METEOR_SPIN_MAX: f64 = 2.4;

/// Vitesse angulaire de rotation des débris (rad/s), signe aléatoire.
pub const GARBAGE_SPIN: f64 = 3.0;

/// Génération procédurale des météores : bornes du nombre de triangles
/// par météore et de la taille (base, hauteur) des triangles.
pub const TRIANGLES_IN_SHAPE_MIN: i32 = 6;
pub const TRIANGLES_IN_SHAPE_MAX: i32 = 16;
pub const TRIANGLE_BASE_MIN: i32 = 15;
pub const TRIANGLE_BASE_MAX: i32 = 40;
pub const TRIANGLE_HEIGHT_MIN: i32 = 11;
pub const TRIANGLE_HEIGHT_MAX: i32 = 22;

/// Population de météores : nombre initial maximal, plafonné à
/// `SHAPES_COUNT` (+1 par météore détruit).
pub const INITIAL_MAX_METEOR_SHAPES: i32 = 50;
pub const SHAPES_COUNT: i32 = 150;

/// Poids de la **précision de tir** sur la remise de réputation : la remise
/// du rang est multipliée par `1 + poids × précision` (précision en 0..1 =
/// part de tirs non perdus) - 1.0 = 100 % de précision → remise doublée.
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

/// Vaisseau joueur - mesh « meshes-designer » et réglages (échelle,
/// orientation, centre de rotation). Données générées par l'outil de
/// gestion : `src/vaisseau.rs` les lit pour construire le vaisseau.
///
/// Fichier mesh embarqué au compile (`include_str!`) : chemin relatif
/// à la racine du projet.
pub const VAISSEAU_JSON: &str = include_str!("../assets/vaisseau.json");

/// Échelle du vaisseau (multiplicateur des sommets du mesh) : 1.0 = 100 %.
pub const VAISSEAU_SCALE: f64 = 1.8;

/// Orientation de départ du vaisseau (degrés) : angle du nez du mesh
/// dans l'éditeur « meshes-designer » (0 = nez vers la droite, +90 =
/// vers le haut). Le mesh est tourné de −orientation à la construction.
pub const VAISSEAU_ORIENTATION_DEGREES: f64 = 0.0;

/// Centre de rotation du vaisseau : position du pivot en pourcentage de
/// la boîte englobante du mesh (50 = centre), axe x.
pub const VAISSEAU_CENTER_X_PERCENT: f64 = 54.0;

/// Centre de rotation du vaisseau (voir `VAISSEAU_CENTER_X_PERCENT`), axe y.
pub const VAISSEAU_CENTER_Y_PERCENT: f64 = 50.0;

/// Emplacements de départ des projectiles (tir Shift) : positions en
/// **pourcentage de la boîte englobante de la composition** (50/50 =
/// centre), dans le repère du mesh de l'éditeur (y vers le haut). Une
/// balle part de chaque emplacement, tourné avec le vaisseau. **Liste
/// vide = un seul emplacement au centre de rotation** (repli :
/// comportement d'origine). Générée par l'outil de gestion.
pub const VAISSEAU_BULLET_SPAWNS: &[(f64, f64)] = &[(90.0, 50.0), (43.0, 89.0), (53.0, 26.0), (41.0, 96.0)];

/// Mesh du propulseur « ARRIÈRE » (VAISSEAU_THRUSTERS[0]) - embarqué au compile.
pub const VAISSEAU_THRUSTER_MESH_0: &str = include_str!("../assets/propellerUp.json");

/// Mesh du propulseur « AVANT » (VAISSEAU_THRUSTERS[1]) - embarqué au compile.
pub const VAISSEAU_THRUSTER_MESH_1: &str = include_str!("../assets/propellerDown.json");

/// Mesh du propulseur « GAUCHE » (VAISSEAU_THRUSTERS[2]) - embarqué au compile.
pub const VAISSEAU_THRUSTER_MESH_2: &str = include_str!("../assets/propellerLeft.json");

/// Mesh du propulseur « DROITE » (VAISSEAU_THRUSTERS[3]) - embarqué au compile.
pub const VAISSEAU_THRUSTER_MESH_3: &str = include_str!("../assets/propellerRight.json");

/// Éjections de gaz du vaisseau - 4 propulseurs, un par touche de contrôle
/// (ordre fixe : ↑ arrière, ↓ avant, ← flanc gauche, → flanc droit). Chaque
/// propulseur est un mesh posé **sur le vaisseau** à sa position (le gaz sort
/// de là, dans la direction d'éjection correspondante - src/main.rs).
/// **Liste vide = repli** : pas de propulseur, le gaz classique sort du
/// centre de rotation (comportement d'origine). Générée par l'outil de gestion.
pub const VAISSEAU_THRUSTERS: &[VaisseauThruster] = &[
    VaisseauThruster {
        name: "ARRIÈRE",
        mesh: VAISSEAU_THRUSTER_MESH_0,
        scale: 2.0,
        orientation_degrees: 0.0,
        position: (-15.0, 50.0),
        ejection_angle_degrees: 180.0,
        color: 0xFFFF7902,
    },
    VaisseauThruster {
        name: "AVANT",
        mesh: VAISSEAU_THRUSTER_MESH_1,
        scale: 2.0,
        orientation_degrees: 180.0,
        position: (116.0, 50.0),
        ejection_angle_degrees: 0.0,
        color: 0xFF00A0FF,
    },
    VaisseauThruster {
        name: "GAUCHE",
        mesh: VAISSEAU_THRUSTER_MESH_2,
        scale: 1.0,
        orientation_degrees: 90.0,
        position: (88.0, 75.0),
        ejection_angle_degrees: 90.0,
        color: 0xFF97D3C4,
    },
    VaisseauThruster {
        name: "DROITE",
        mesh: VAISSEAU_THRUSTER_MESH_3,
        scale: 1.0,
        orientation_degrees: -90.0,
        position: (89.0, 25.0),
        ejection_angle_degrees: -90.0,
        color: 0xFFFF5AC8,
    },
];

/// Mesh de l'arme « ARME 1 » (VAISSEAU_WEAPONS[0]) - embarqué au compile.
pub const VAISSEAU_WEAPON_MESH_0: &str = include_str!("../assets/bulletWeapon.json");
/// Mesh de la munition de l'arme « ARME 1 » (VAISSEAU_WEAPONS[0]).
pub const VAISSEAU_WEAPON_AMMO_MESH_0: &str = include_str!("../assets/bullet.json");

/// Mesh de l'arme « ARME 2 » (VAISSEAU_WEAPONS[1]) - embarqué au compile.
pub const VAISSEAU_WEAPON_MESH_1: &str = include_str!("../assets/ballWeapon.json");
/// Mesh de la munition de l'arme « ARME 2 » (VAISSEAU_WEAPONS[1]).
pub const VAISSEAU_WEAPON_AMMO_MESH_1: &str = include_str!("../assets/duck-rocket.json");

/// Catalogue d'armes du vaisseau joueur : chaque arme est un mesh posé
/// **sur le vaisseau** à un emplacement de tir (`spawn_index` dans
/// `VAISSEAU_BULLET_SPAWNS` - la liste des emplacements possibles est
/// contrainte) et tire sa propre munition (mesh de la munition). Toutes
/// les armes du catalogue tirent ensemble au Shift. **Liste vide = tir
/// classique** (une balle rouge par emplacement, repli : comportement
/// d'origine). Générée par l'outil de gestion.
pub const VAISSEAU_WEAPONS: &[VaisseauWeapon] = &[
    VaisseauWeapon {
        name: "ARME 1",
        mesh: VAISSEAU_WEAPON_MESH_0,
        scale: 1.0,
        orientation_degrees: 0.0,
        spawn_index: 0,
        ammo_mesh: VAISSEAU_WEAPON_AMMO_MESH_0,
        ammo_scale: 2.0,
        ammo_orientation_degrees: 0.0,
        cost: 0,
        ammo_price: 1,
        ammo_pack: 5,
    },
    VaisseauWeapon {
        name: "ARME 2",
        mesh: VAISSEAU_WEAPON_MESH_1,
        scale: 0.73,
        orientation_degrees: -13.0,
        spawn_index: 1,
        ammo_mesh: VAISSEAU_WEAPON_AMMO_MESH_1,
        ammo_scale: 0.3,
        ammo_orientation_degrees: 0.0,
        cost: 15,
        ammo_price: 3,
        ammo_pack: 10,
    },
];

/// Plans du vaisseau **toujours visibles** (composition de base - indices
/// des plans du fichier mesh `VAISSEAU_JSON`). Un plan lié à une ligne
/// d'atelier (`VAISSEAU_PLANE_LINKS`) n'est construit qu'à partir de son
/// niveau ; un plan absent des deux listes n'est jamais construit.
/// **Listes vides = tous les plans** (repli : composition non définie).
/// Générées par l'outil de gestion.
pub const VAISSEAU_PLANES_ALWAYS: &[usize] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];

/// Plans du vaisseau liés aux lignes d'atelier (Progression) : un plan
/// apparaît quand la ligne `track` atteint le niveau `min_level`. Générés
/// par l'outil de gestion.
pub const VAISSEAU_PLANE_LINKS: &[PlaneUpgradeLink] = &[
    PlaneUpgradeLink { plane_index: 14, track: PlaneUpgradeTrack::Cargo, min_level: 2 },
];

/// Cosmonaute EVA (pilote éjecté, vaisseau détruit) - mesh « meshes-designer »
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

/// Plans du cosmonaute EVA construits (composition - indices des plans du
/// fichier mesh `COSMONAUTE_JSON`) : un plan absent n'est jamais construit
/// (ni animé). **Liste vide = tous les plans** (repli : composition non
/// définie). Générée par l'outil de gestion.
pub const COSMONAUTE_PLANES: &[usize] = &[];

/// Un mode de déplacement du vaisseau (index `MOVING_MODE_*` du jeu) : nom
/// et description affichés (magasin de la station, messages du scénario) et
/// coût de déblocage en crédits (0 = déjà débloqué au départ). Généré par
/// l'outil de gestion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingMode {
    /// Nom du mode (magasin, messages HUD).
    pub name: &'static str,
    /// Courte description (magasin de la station).
    pub description: &'static str,
    /// Coût en crédits pour débloquer (0 = gratuit au départ).
    pub cost: i32,
}

/// Nom d'affichage d'un mode de déplacement (magasin de la station, messages
/// du scénario) - lu dans `MOVING_MODES` (généré par l'outil de gestion).
pub fn mode_label(mode: i32) -> &'static str {
    MOVING_MODES
        .get(mode as usize)
        .map(|m| m.name)
        .unwrap_or("?")
}

/// Une extension de vaisseau achetable à l'atelier de la station (scénario
/// Progression, bouton SHOP de la boîte DOCK STATION) : ajoute de la
/// capacité (réservoir, chargeur ou soute) au prix indiqué, payé en crédits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShipUpgrade {
    /// Nom de l'extension (atelier, HUD).
    pub name: &'static str,
    /// Coût en crédits.
    pub cost: i32,
    /// Capacité ajoutée (carburant, munitions ou soute).
    pub bonus: i32,
}

/// Une ligne d'amélioration du vaisseau (atelier) : capacité de base et
/// extensions successives achetées **dans l'ordre** - le niveau est le nombre
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

/// Ligne d'atelier liée à un plan du vaisseau (composition des plans,
/// scénario Progression) - la ligne dont le niveau révèle le plan.
/// Les variantes ne sont construites que quand l'outil exporte un lien
/// (`VAISSEAU_PLANE_LINKS` non vide) - `dead_code` quand la liste est vide.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneUpgradeTrack {
    /// Réservoir de carburant (FUEL TANK).
    Fuel,
    /// Chargeur de munitions (MAGAZINE).
    Ammo,
    /// Soute (CARGO BAY).
    Cargo,
}

/// Plan du vaisseau lié à une ligne d'atelier : le plan `plane_index` du
/// fichier mesh n'est construit qu'à partir du niveau `min_level` de la
/// ligne `track` (niveau = nombre d'extensions achetées). Généré par l'outil
/// de gestion.
#[derive(Clone, Copy, Debug)]
pub struct PlaneUpgradeLink {
    /// Indice du plan dans le fichier mesh (`VAISSEAU_JSON`).
    pub plane_index: usize,
    /// Ligne d'atelier qui révèle le plan.
    pub track: PlaneUpgradeTrack,
    /// Niveau minimal de la ligne pour que le plan soit construit.
    pub min_level: i32,
}

/// Une arme du catalogue du vaisseau joueur : mesh de l'arme posé **sur le
/// vaisseau** à un emplacement de tir (`spawn_index` dans
/// `VAISSEAU_BULLET_SPAWNS` - la liste des emplacements possibles est
/// contrainte) et munition tirée par l'arme (mesh de la munition, avec sa
/// propre échelle et orientation). Toutes les armes **possédées** du
/// catalogue tirent ensemble au Shift, chacune tant que son propre stock de
/// munitions n'est pas vide. Chaque arme porte son **coût d'achat** au
/// magasin (0 = arme de base, toujours équipée) et le prix/taille de son
/// **paquet de munitions** (le magasin facture par paquet de
/// `ammo_pack` munitions au prix `ammo_price`, par arme). Générée par
/// l'outil de gestion. Les champs ne sont lus que quand le catalogue est
/// rempli (`VAISSEAU_WEAPONS` non vide) - `dead_code` quand la liste est
/// vide.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct VaisseauWeapon {
    /// Nom de l'arme (tool, export).
    pub name: &'static str,
    /// Mesh de l'arme - fichier « meshes-designer » embarqué au compile.
    pub mesh: &'static str,
    /// Échelle de l'arme (multiplicateur des sommets du mesh) : 1.0 = 100 %.
    pub scale: f64,
    /// Orientation de l'arme (degrés) : angle de l'avant du mesh dans
    /// l'éditeur (0 = avant vers la droite, +90 = vers le haut).
    pub orientation_degrees: f64,
    /// Emplacement de l'arme sur le vaisseau : index dans
    /// `VAISSEAU_BULLET_SPAWNS` (liste contrainte).
    pub spawn_index: usize,
    /// Mesh de la munition tirée par l'arme - embarqué au compile.
    pub ammo_mesh: &'static str,
    /// Échelle de la munition (multiplicateur des sommets du mesh).
    pub ammo_scale: f64,
    /// Orientation de la munition (degrés) : angle de l'avant du mesh dans
    /// l'éditeur - la munition part nez en avant.
    pub ammo_orientation_degrees: f64,
    /// Coût d'achat de l'arme au magasin de la station (crédits ; 0 = arme
    /// de base, toujours équipée au départ en Progression).
    pub cost: i32,
    /// Prix (crédits) d'un **paquet** de munitions de l'arme (magasin,
    /// section RAVITAILLEMENT).
    pub ammo_price: i32,
    /// Taille d'un paquet de munitions de l'arme (munitions par paquet).
    pub ammo_pack: i32,
}

/// Une éjection de gaz du vaisseau (les 4 touches de contrôle de la
/// poussée/orientation) : mesh du **propulseur** posé **sur le vaisseau** à
/// une position (en **pourcentage de la boîte englobante de la composition**,
/// 50/50 = centre, repère du mesh de l'éditeur, y vers le haut), avec sa
/// propre échelle et orientation. Le gaz (flamme) sort de la position, dans
/// la direction d'éjection correspondant à la touche - index 0 = **↑**
/// (poussée avant, gaz orange à l'arrière), 1 = **↓** (frein/recul, gaz bleu
/// à l'avant), 2 = **←** (rotation gauche), 3 = **→** (rotation droite).
/// Générée par l'outil de gestion. Le champ `name` n'est lu que par les
/// tests et l'outil - `dead_code` quand la liste est vide.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct VaisseauThruster {
    /// Nom du propulseur (outil, export).
    pub name: &'static str,
    /// Mesh du propulseur - fichier « meshes-designer » embarqué au compile.
    pub mesh: &'static str,
    /// Échelle du propulseur (multiplicateur des sommets du mesh) : 1.0 = 100 %.
    pub scale: f64,
    /// Orientation du propulseur (degrés) : angle de l'avant du mesh dans
    /// l'éditeur (0 = avant vers la droite, +90 = vers le haut) - le mesh est
    /// tourné de −orientation à la construction.
    pub orientation_degrees: f64,
    /// Position sur le vaisseau : en % de la boîte englobante de la
    /// composition (50/50 = centre), dans le repère de l'éditeur.
    pub position: (f64, f64),
    /// Direction d'éjection du gaz (degrés, repère de l'éditeur y vers le
    /// haut : 0 = avant, +90 = haut) - convertie par le jeu pour
    /// `ejection_flow` (src/main.rs).
    pub ejection_angle_degrees: f64,
    /// Couleur du gaz d'éjection (ARGB, ex 0xFFFFA000) - `ejection_flow`
    /// (src/main.rs).
    pub color: u32,
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
