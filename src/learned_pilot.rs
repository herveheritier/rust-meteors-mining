//! Pilote **appris** (Phase 4) : la stratégie du vaisseau **entraînée
//! hors-ligne** par imitation de l'autopilote (`tools/trainer/ship_warmstart.py`)
//! est **portée dans le jeu** - c'est la stratégie « autopilote alternative »
//! sélectionnable à l'écran de paramétrage (case LEARNED PILOT, touche Y).
//!
//! Deux pièces sont rejouées ici, à l'identique de l'entraîneur :
//!
//! - l'**extraction de features** (`tools/trainer/nn.py::obs_features`, version
//!   8) : la même observation (`driver::Observation`) que celle servie à
//!   l'entraîneur est transformée en un vecteur de `FEATURE_COUNT` nombres -
//!   cinématique brute des deux corps, grandeurs dérivées que l'autopilote
//!   calcule pour décider (visée, erreur d'alignement, vitesse radiale et
//!   tangentielle), variables de décision EVA puis vaisseau (retenue de feu),
//!   **visée de mission de la conduite** (mission, cap effectif, vitesse visée,
//!   4 WAYS, esquive), économie, mode de déplacement, réserves payables,
//!   objectifs du scénario, objets proches et balles en vol (slots fixes) ;
//! - le **réseau** (`nn.py::MLP`) : perceptron multicouche à une couche cachée
//!   (tanh), trois sigmoïdes indépendantes (`up`/`down`/`fire`) et une tête
//!   **softmax** de rotation (`left`/`right`/`none`).
//!
//! Les poids sont **embarqués dans le binaire** (`include_str!`) : la politique
//! déployée est un asset du jeu (`assets/ship_pilot_policy.json`, écrit par
//! `ship_warmstart.py --output ../assets/ship_pilot_policy.json`) - pour la
//! mettre à jour, ré-entraîner **et** recompiler.
//!
//! La loi reste celle de l'entraînement : le réseau ne décide que des
//! **entrées de pilotage** ; les gestes d'accostage (décharger, se ravitailler,
//! repartir) restent à la machine à états de l'autopilote
//! (`autopilot_handle_dock` / `autopilot_handle_shop` / `autopilot_start_depart`),
//! exactement comme l'expert étiqueteur qui les exécutait hors du réseau.
//!
//! NB : la fidélité de ce portage est verrouillée par un test unitaire
//! (`tests` plus bas) sur des observations **réelles** enregistrées, et par
//! `tools/trainer/validate_learned_port.py` (comparaison, à chaque pas d'une
//! vraie partie, de l'action du portage et de celle de la politique Python).

use std::sync::OnceLock;

use macroquad::prelude::error;
use serde::Deserialize;

use crate::autopilot::{autopilot_inputs, PilotInputs};
use crate::driver::{self, Observation};
use crate::shape::Shape;
use crate::state::GameState;

/// Politique embarquée : sortie de `tools/trainer/ship_warmstart.py` (poids du
/// réseau + métadonnées d'entraînement, mêmes clés que `nn.py::save_nn`).
const POLICY_JSON: &str = include_str!("../assets/ship_pilot_policy.json");

/// Version du format des features (`nn.py::FEATURES_VERSION`) : le fichier de
/// politique porte la sienne et le portage refuse une version inconnue -
/// rejouer des poids entraînés sur d'anciennes features donnerait des
/// décisions silencieusement fausses.
pub const FEATURES_VERSION: u32 = 8;

// ─── paramètres du modèle (miroir de `tools/trainer/nn.py`) ─────────────────

/// Échelles de normalisation (`nn.py`) : le monde fait 3960 × 3540, les
/// vitesses quelques centaines d'unités/s, la vie d'une forme quelques
/// dizaines de triangles.
const SCALE_DIST: f64 = 2000.0;
const SCALE_SPEED: f64 = 300.0;
const SCALE_RADIUS: f64 = 100.0;
const SCALE_LIFE: f64 = 50.0;
const SCALE_CREDITS: f64 = 5000.0;

/// Types d'objets proches encodés en one-hot (`nn.py::NEARBY_KINDS`).
const NEARBY_KINDS: [&str; 5] = ["meteore", "minerai", "alien", "portail", "mine"];
/// Nombre d'objets proches pris en compte (`nn.py::NEARBY_SLOTS`) : les plus
/// proches, le reste de l'observation est ignoré.
const NEARBY_SLOTS: usize = 6;
/// Taille d'un slot d'objet proche : one-hot des types + 7 grandeurs.
const NEARBY_SLOT_LEN: usize = NEARBY_KINDS.len() + 7;

/// Modes de déplacement encodés en one-hot (`nn.py::MOVING_MODES`) : la
/// conduite de l'autopilote en dépend (4 WAYS pousse dans les axes de l'écran).
const MOVING_MODES: usize = 4;
/// Slots de balles en vol (`nn.py::BULLET_SLOTS`).
const BULLET_SLOTS: usize = 4;
/// Taille d'un slot de balle : `dx, dy, dist, vx, vy` (`nn.py::BULLET_SLOT_LEN`).
const BULLET_SLOT_LEN: usize = 5;

/// Taille du vecteur de features - un test la verrouille contre `nn.py`
/// (`FEATURE_COUNT` doit suivre toute évolution des features, comme
/// `FEATURES_VERSION`).
pub const FEATURE_COUNT: usize = 157;

/// Indices de sortie : `0..3` = sigmoïdes `up`/`down`/`fire`, `3..6` = softmax
/// de rotation `left`/`right`/`none` (`nn.py::SIGMOID_OUTPUTS` / `TURN_HEAD`).
const SIGMOID_OUTPUTS: usize = 3;
const TURN_HEAD: usize = 3;

// ─── constantes de la loi EVA rejouées par les features ─────────────────────
// (miroir de `src/autopilot.rs` ; `nn.py::_eva_decision_features`)

const EVA_ARRIVAL_SPEED: f64 = 0.5;
const EVA_SPEED_BAND: f64 = 0.15;
/// Demi-tour en frames : `π / (τ/210)` - la vitesse angulaire EVA est de
/// `τ/210` rad/frame.
const EVA_TURN_FRAMES: f64 = 105.0;
const EVA_ACCEL: f64 = 0.05;
const EVA_TANG_BRAKE_HI: f64 = 0.25;
const DOCK_DISTANCE: f64 = 15.0;

// ─── constantes de la retenue de feu de l'autopilote vaisseau ───────────────
// (miroir de `src/autopilot.rs` ; `nn.py::_ship_decision_features`)

const SHIP_FIRE_RANGE: f64 = 210.0;
const SHIP_HOLD_FIRE_LIFE: i32 = 2;
const SHIP_HOLD_FIRE_RADIUS: f64 = 150.0;
const SHIP_HOLD_FIRE_MINERAL_RADIUS: f64 = 60.0;
const SHIP_HOSTILE_KINDS: [&str; 2] = ["meteore", "alien"];

// ─── constantes de la conduite de l'autopilote vaisseau ─────────────────────
// (miroir de `src/autopilot.rs` ; `nn.py::_ship_drive_features`)

const SHIP_CRUISE_SPEED: f64 = 1.8;
const SHIP_ATTACK_STANDOFF: f64 = 70.0;
const SHIP_STATION_GUARD_RADIUS: f64 = 240.0;
const SHIP_MINERAL_CLEARANCE: f64 = 130.0;
const SHIP_PATROL_RADIUS: f64 = 120.0;
const SHIP_DOCK_SLOW_ZONE: f64 = 90.0;
const SHIP_LOW_SUPPLY_RATIO: f64 = 0.30;
const SHIP_AVOID_RADIUS: f64 = 360.0;
const SHIP_AVOID_CLEARANCE: f64 = 90.0;
const SHIP_AVOID_TIME: f64 = 1.5;
const SHIP_SETTLE_BAND: f64 = 0.4;
const SHIP_WORLD_W: f64 = 3960.0;
const SHIP_WORLD_H: f64 = 3540.0;

/// Nombre de features de la conduite (4 de mission + 10 de conduite) -
/// `nn.py::SHIP_DRIVE_FEATURES`. NB : une version 9 ajoutait 12 **seuils de
/// conduite** ; mesurée, elle rend la loi entièrement décidable (100 %
/// d'imitation) mais **régresse** la boucle fermée dans le jeu (3/12 contre
/// 7/12) - le facteur limitant est l'écart de distribution, pas la
/// représentation (`docs/AUTOENTRAINEMENT.md` §5 nonies ter).
const SHIP_DRIVE_FEATURES: usize = 14;

// ─── le fichier de politique ────────────────────────────────────────────────

/// Politique sérialisée (`nn.py::save_nn`) : métadonnées + poids.
#[derive(Debug, Deserialize)]
struct PolicyFile {
    policy: String,
    features_version: u32,
    inputs: usize,
    hidden: usize,
    outputs: usize,
    sigmoid_outputs: usize,
    turn_head: usize,
    /// Couche d'entrée : `inputs × hidden`.
    w1: Vec<Vec<f64>>,
    b1: Vec<f64>,
    /// Couche de sortie : `hidden × outputs`.
    w2: Vec<Vec<f64>>,
    b2: Vec<f64>,
}

/// Le réseau tel que le portage l'exécute (poids validés) : même calcul que
/// `nn.py::MLP.forward`.
#[derive(Debug)]
struct Policy {
    inputs: usize,
    hidden: usize,
    outputs: usize,
    w1: Vec<Vec<f64>>,
    b1: Vec<f64>,
    w2: Vec<Vec<f64>>,
    b2: Vec<f64>,
}

impl Policy {
    /// Désérialise **et valide** les poids : un fichier dont les dimensions ne
    /// se recoupent pas donnerait un accès hors bornes silencieux - on refuse
    /// plutôt que de piloter avec un réseau incohérent.
    fn parse(json: &str) -> Result<Self, String> {
        let f: PolicyFile =
            serde_json::from_str(json).map_err(|e| format!("JSON illisible : {e}"))?;
        if f.policy != "nn" {
            return Err(format!("politique de type « {} » (attendu « nn »)", f.policy));
        }
        if f.features_version != FEATURES_VERSION {
            return Err(format!(
                "features version {} (ce portage en implémente {FEATURES_VERSION} - ré-entraîner)",
                f.features_version
            ));
        }
        if f.inputs != FEATURE_COUNT {
            return Err(format!(
                "{} entrées (les features en produisent {FEATURE_COUNT})",
                f.inputs
            ));
        }
        if f.sigmoid_outputs != SIGMOID_OUTPUTS || f.turn_head != TURN_HEAD {
            return Err("têtes de sortie incompatibles (ré-entraîner)".to_string());
        }
        if f.outputs != SIGMOID_OUTPUTS + TURN_HEAD
            || f.w1.len() != f.inputs
            || f.w1.iter().any(|row| row.len() != f.hidden)
            || f.b1.len() != f.hidden
            || f.w2.len() != f.hidden
            || f.w2.iter().any(|row| row.len() != f.outputs)
            || f.b2.len() != f.outputs
        {
            return Err("dimensions des couches incohérentes".to_string());
        }
        Ok(Policy {
            inputs: f.inputs,
            hidden: f.hidden,
            outputs: f.outputs,
            w1: f.w1,
            b1: f.b1,
            w2: f.w2,
            b2: f.b2,
        })
    }

    /// Sorties pour un vecteur de features : trois sigmoïdes (`up`, `down`,
    /// `fire`) puis trois probabilités softmax (somme = 1) - même calcul que
    /// `nn.py::MLP.forward`.
    #[allow(clippy::needless_range_loop)] // accumulation indexée : miroir de nn.py
    fn forward(&self, x: &[f64]) -> Vec<f64> {
        // ordre des opérations **celui de `nn.py`** (somme des termes puis
        // ajout du biais) : l'addition flottante n'étant pas associative, un
        // ordre différent suffit à faire basculer une décision limite - le
        // portage doit être le même calcul, pas seulement la même formule
        let mut h = Vec::with_capacity(self.hidden);
        for j in 0..self.hidden {
            let mut acc = 0.0;
            for i in 0..self.inputs {
                acc += x[i] * self.w1[i][j];
            }
            h.push((acc + self.b1[j]).tanh());
        }
        let mut z = Vec::with_capacity(self.outputs);
        for k in 0..self.outputs {
            let mut acc = 0.0;
            for j in 0..self.hidden {
                acc += h[j] * self.w2[j][k];
            }
            z.push(acc + self.b2[k]);
        }
        let mut out: Vec<f64> = z[..SIGMOID_OUTPUTS]
            .iter()
            .map(|&v| 1.0 / (1.0 + (-v).exp()))
            .collect();
        // softmax de rotation numériquement stable (max soustrait), comme
        // `nn.py` - les trois classes sont mutuellement exclusives
        let turn = &z[SIGMOID_OUTPUTS..];
        let m = turn.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let exps: Vec<f64> = turn.iter().map(|&v| (v - m).exp()).collect();
        let sum: f64 = exps.iter().sum();
        out.extend(exps.iter().map(|&e| e / sum));
        out
    }

    /// Action gloutonne (la seule façon dont la politique est déployée) : les
    /// sigmoïdes au seuil 0,5 et la classe de rotation la plus probable -
    /// **premier indice en cas d'égalité**, comme `max(range(...))` en Python.
    fn greedy(&self, out: &[f64]) -> PilotInputs {
        let mut turn = 0;
        for k in 1..TURN_HEAD {
            if out[SIGMOID_OUTPUTS + k] > out[SIGMOID_OUTPUTS + turn] {
                turn = k;
            }
        }
        PilotInputs {
            up: out[0] >= 0.5,
            down: out[1] >= 0.5,
            fire: out[2] >= 0.5,
            left: turn == 0,
            right: turn == 1,
        }
    }

    /// Un pas complet : features → réseau → action (chemin utilisé par le jeu
    /// et par le test de fidélité).
    fn act(&self, obs: &Observation) -> PilotInputs {
        self.greedy(&self.forward(&features(obs)))
    }
}

/// La politique embarquée, désérialisée **une seule fois** (premier usage) et
/// conservée pour la durée du processus : le fichier est compilé dans le
/// binaire, le seul cas d'échec est un asset corrompu.
fn policy() -> Option<&'static Policy> {
    static CELL: OnceLock<Option<Policy>> = OnceLock::new();
    CELL.get_or_init(|| match Policy::parse(POLICY_JSON) {
        Ok(p) => Some(p),
        Err(e) => {
            error!("politique apprise illisible: {e}");
            None
        }
    })
    .as_ref()
}

/// La politique apprise est-elle **jouable** (poids embarqués valides) ? Faux
/// si l'asset a été modifié/corrompu : la stratégie apprise retombe alors sur
/// la loi scriptée au lieu de planter.
pub fn available() -> bool {
    policy().is_some()
}

/// Entrées de pilotage de la **stratégie apprise** pour l'état courant du jeu
/// (`input::player_controls`, quand la case LEARNED PILOT est active). Si la
/// politique embarquée est illisible, retombe sur la loi **scriptée** de
/// l'autopilote - un asset corrompu ne doit pas laisser le vaisseau sans
/// pilote.
pub fn inputs(state: &GameState, shapes: &[Shape]) -> PilotInputs {
    match policy() {
        Some(p) => p.act(&driver::observe(state, shapes)),
        None => autopilot_inputs(state, shapes),
    }
}

/// Action de la stratégie apprise sur une observation **déjà construite** -
/// sert à estampiller `Observation::learned` (le champ miroir de `expert`
/// utilisé par l'entraîneur pour vérifier ce portage) sans reconstruire
/// l'observation. `None` quand la politique embarquée est illisible.
pub fn inputs_from(obs: &Observation) -> Option<PilotInputs> {
    policy().map(|p| p.act(obs))
}

// ─── extraction de features (miroir de `nn.py::obs_features`) ───────────────

/// `nn.py::_clamp`.
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.clamp(lo, hi)
}

/// `nn.py::_wrap_angle` : angle ramené dans ]−π, π] (le modulo positif de
/// Python est reproduit par `rem_euclid`).
fn wrap_angle(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// `nn.py::_angle_feature` : angle normalisé dans ]−1, 1].
fn angle_feature(a: f64) -> f64 {
    clamp(wrap_angle(a) / std::f64::consts::PI, -1.0, 1.0)
}

/// Features d'un pilote (`nn.py::_pilot_features`) : cinématique brute **plus**
/// les grandeurs que l'autopilote calcule pour décider - visée vers la station,
/// erreur d'alignement, vitesse projetée (radiale) et perpendiculaire
/// (tangentielle, l'orbite autour de la base).
fn pilot_features(body: &driver::Kinematic, station_dx: f64, station_dy: f64, out: &mut Vec<f64>) {
    let aim = station_dy.atan2(station_dx);
    let orientation = body.orientation;
    let v_along = body.vx * aim.cos() + body.vy * aim.sin();
    let v_tang = (body.speed * body.speed - v_along * v_along).max(0.0).sqrt();
    out.push(body.x / SCALE_DIST);
    out.push(body.y / SCALE_DIST);
    out.push(clamp(body.vx / SCALE_SPEED, -1.0, 1.0));
    out.push(clamp(body.vy / SCALE_SPEED, -1.0, 1.0));
    out.push(clamp(body.speed / SCALE_SPEED, -1.0, 1.0));
    out.push(angle_feature(body.direction));
    out.push(angle_feature(orientation));
    out.push(clamp(body.rotation / std::f64::consts::TAU, -1.0, 1.0));
    out.push(angle_feature(aim));
    out.push(clamp(wrap_angle(aim - orientation) / std::f64::consts::PI, -1.0, 1.0));
    out.push(clamp(v_along / SCALE_SPEED, -1.0, 1.0));
    out.push(clamp(v_tang / SCALE_SPEED, -1.0, 1.0));
}

/// Variables de décision de l'autopilote EVA (`nn.py::_eva_decision_features`) :
/// freinage anticipé, cassure d'orbite, et erreur d'alignement par rapport à la
/// direction de poussée **résultante** - sans cette dernière, deux états de
/// cinématique identique portent des actions opposées et l'imitation est
/// structurellement impossible.
fn eva_decision_features(obs: &Observation, station_dx: f64, station_dy: f64, out: &mut Vec<f64>) {
    let eva = &obs.eva;
    let d = obs.station_dist;
    let aim = station_dy.atan2(station_dx);
    let v_along = (eva.vx * aim.cos() + eva.vy * aim.sin()) / 60.0;
    let speed = eva.speed / 60.0;
    let v_tang = (speed * speed - v_along * v_along).max(0.0).sqrt();
    let tang = v_tang > EVA_TANG_BRAKE_HI;
    let braking = v_along > EVA_ARRIVAL_SPEED + EVA_SPEED_BAND
        && (d - DOCK_DISTANCE)
            < v_along * EVA_TURN_FRAMES
                + (v_along * v_along - EVA_ARRIVAL_SPEED * EVA_ARRIVAL_SPEED) / (2.0 * EVA_ACCEL);
    let thrust_dir = if tang {
        std::f64::consts::PI - eva.direction
    } else if braking {
        aim + std::f64::consts::PI
    } else {
        aim
    };
    out.push(if braking { 1.0 } else { 0.0 });
    out.push(if tang { 1.0 } else { 0.0 });
    out.push(angle_feature(wrap_angle(thrust_dir - eva.orientation)));
}

/// `nn.py::_finite` : une valeur **non finie** vaut zéro.
///
/// Un centre de forme dégénéré peut être NaN (le jeu le publie alors `null` en
/// JSON, `serde_json` sérialisant ainsi les flottants non finis). Sans ce
/// filtre, un seul NaN **contaminerait tout le vecteur de features** (il se
/// propage dans les sommes) - le portage doit neutraliser exactement comme
/// `nn.py`, sans quoi les deux calculs divergent.
fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// `nn.py::_body_distance` : distance entre les **centres de corps** de deux
/// objets de `nearby`/`bullets` (chacun porte son delta depuis le pilote et son
/// décalage de corps - la visée réelle de l'autopilote).
fn body_distance(a: &driver::NearbyObject, b: &driver::NearbyObject) -> f64 {
    let dx = (finite(b.dx) + finite(b.center_x)) - (finite(a.dx) + finite(a.center_x));
    let dy = (finite(b.dy) + finite(b.center_y)) - (finite(a.dy) + finite(a.center_y));
    (dx * dx + dy * dy).sqrt()
}

/// Grandeurs de décision de la **retenue de feu** de l'autopilote vaisseau
/// (`nn.py::_ship_decision_features`) : cible de tir quasi détruite déjà
/// achevée par des balles en vol, ou minerais dans le corridor de tir. Sans
/// elles, la proximité mutuelle balles/cible ne se lit pas et le même vecteur
/// de features porte `fire` et pas `fire`.
fn ship_decision_features(obs: &Observation, out: &mut Vec<f64>) {
    let mut target: Option<&driver::NearbyObject> = None;
    let mut best = SHIP_FIRE_RANGE;
    for o in &obs.nearby {
        if !SHIP_HOSTILE_KINDS.contains(&o.kind.as_str()) || o.life <= 0 {
            continue;
        }
        if o.dist < best {
            best = o.dist;
            target = Some(o);
        }
    }
    let Some(target) = target else {
        out.extend([0.0, 0.0]);
        return;
    };
    let hold_bullets = if target.life <= SHIP_HOLD_FIRE_LIFE
        && obs
            .bullets
            .iter()
            .any(|b| body_distance(b, target) < SHIP_HOLD_FIRE_RADIUS)
    {
        1.0
    } else {
        0.0
    };
    let hold_mineral = if obs.nearby.iter().any(|m| {
        m.kind == "minerai"
            && m.life > 0
            && body_distance(m, target) < SHIP_HOLD_FIRE_MINERAL_RADIUS
    }) {
        1.0
    } else {
        0.0
    };
    out.push(hold_bullets);
    out.push(hold_mineral);
}

/// Mission de la frame de l'autopilote vaisseau (`src/autopilot.rs::Goal`) : la
/// conduite vise la **cible de la mission**, pas l'objet le plus proche.
#[derive(Clone, Copy, PartialEq)]
enum ShipGoal {
    Dock,
    Attack,
    Collect,
    Patrol,
}

/// `nn.py::_wrap_tor` : delta écran ramené au plus court dans le monde torique.
fn wrap_tor(dx: f64, dy: f64) -> (f64, f64) {
    (
        dx - SHIP_WORLD_W * (dx / SHIP_WORLD_W).round(),
        dy - SHIP_WORLD_H * (dy / SHIP_WORLD_H).round(),
    )
}

/// `nn.py::_ship_body_delta` : delta **centre du vaisseau** → **centre de
/// corps** de l'objet (la visée réelle de l'autopilote).
fn ship_body_delta(obs: &Observation, b: &driver::NearbyObject) -> (f64, f64) {
    wrap_tor(
        (finite(b.dx) + finite(b.center_x)) - finite(obs.ship.center_x),
        (finite(b.dy) + finite(b.center_y)) - finite(obs.ship.center_y),
    )
}

/// `nn.py::_ship_station_delta` : delta centre du vaisseau → station.
fn ship_station_delta(obs: &Observation) -> (f64, f64) {
    wrap_tor(
        finite(obs.station_dx) - finite(obs.ship.center_x),
        finite(obs.station_dy) - finite(obs.ship.center_y),
    )
}

/// `nn.py::_station_body_delta` : delta station → centre de corps de l'objet
/// (la distance qui décide de la garde de la station).
fn station_body_delta(obs: &Observation, b: &driver::NearbyObject) -> (f64, f64) {
    wrap_tor(
        finite(obs.station_dx) - (finite(b.dx) + finite(b.center_x)),
        finite(obs.station_dy) - (finite(b.dy) + finite(b.center_y)),
    )
}

/// `nn.py::_ship_supplies_low` : carburant ou munitions sous le seuil de
/// ravitaillement.
fn ship_supplies_low(obs: &Observation) -> bool {
    if obs.fuel_cap > 0.0 && obs.fuel < obs.fuel_cap * SHIP_LOW_SUPPLY_RATIO {
        return true;
    }
    obs.ammo_cap > 0
        && f64::from(obs.ammo) < f64::from(obs.ammo_cap) * SHIP_LOW_SUPPLY_RATIO
}

/// `nn.py::_ship_mission` : mission de la frame (priorité incluse) et cible.
/// La cible est `None` pour l'accostage / le stationnement (la station) ; la
/// cible d'attaque sert à ignorer la cible en approche dans l'évitement.
#[allow(clippy::type_complexity)] // miroir de nn.py : (mission, cible, attaque)
fn ship_mission<'a>(
    obs: &Observation,
    nearby: &'a [driver::NearbyObject],
) -> (ShipGoal, Option<&'a driver::NearbyObject>, Option<&'a driver::NearbyObject>) {
    let cargo_full = obs.cargo_cap > 0 && obs.cargo_qty >= obs.cargo_cap;
    let low_supplies = obs.economy && ship_supplies_low(obs) && obs.supplies_affordable;
    let mut guard: Option<(f64, &driver::NearbyObject)> = None;
    let mut mineral: Option<(f64, &driver::NearbyObject)> = None;
    let mut hostile: Option<(f64, &driver::NearbyObject)> = None;
    for o in nearby {
        if !SHIP_HOSTILE_KINDS.contains(&o.kind.as_str()) || o.life <= 0 {
            continue;
        }
        let ds = {
            let (dx, dy) = station_body_delta(obs, o);
            dx.hypot(dy)
        };
        if ds < SHIP_STATION_GUARD_RADIUS && guard.is_none_or(|(bd, _)| ds < bd) {
            guard = Some((ds, o));
        }
        let dp = {
            let (dx, dy) = ship_body_delta(obs, o);
            dx.hypot(dy)
        };
        if hostile.is_none_or(|(bd, _)| dp < bd) {
            hostile = Some((dp, o));
        }
    }
    for o in nearby {
        if o.kind != "minerai" || o.life <= 0 {
            continue;
        }
        let dp = {
            let (dx, dy) = ship_body_delta(obs, o);
            dx.hypot(dy)
        };
        if mineral.is_none_or(|(bd, _)| dp < bd) {
            mineral = Some((dp, o));
        }
    }
    let hostile_in_range = hostile.is_some_and(|(d, _)| d < SHIP_FIRE_RANGE);
    let mineral_guarded = mineral.is_some_and(|(_, m)| {
        hostile.is_some_and(|(_, h)| body_distance(h, m) < SHIP_MINERAL_CLEARANCE)
    });
    if cargo_full {
        return (ShipGoal::Dock, None, None);
    }
    if let Some((_, o)) = guard {
        return (ShipGoal::Attack, Some(o), Some(o));
    }
    if low_supplies {
        return (ShipGoal::Dock, None, None);
    }
    if hostile_in_range || mineral_guarded {
        let o = hostile.map(|(_, o)| o);
        return (ShipGoal::Attack, o, o);
    }
    if let Some((_, o)) = mineral {
        return (ShipGoal::Collect, Some(o), None);
    }
    if let Some((_, o)) = hostile {
        return (ShipGoal::Attack, Some(o), Some(o));
    }
    (ShipGoal::Patrol, None, None)
}

/// `nn.py::_ship_desired_speed` : vitesse visée selon la mission et la distance.
fn ship_desired_speed(goal: ShipGoal, d: f64) -> f64 {
    match goal {
        ShipGoal::Dock => {
            if d < SHIP_DOCK_SLOW_ZONE {
                (d * 0.04 + 0.02).min(0.9)
            } else {
                SHIP_CRUISE_SPEED
            }
        }
        ShipGoal::Attack => ((d - SHIP_ATTACK_STANDOFF).max(0.0) * 0.05).min(SHIP_CRUISE_SPEED),
        ShipGoal::Collect => (d * 0.12).clamp(0.3, 1.4),
        ShipGoal::Patrol => {
            if d < SHIP_PATROL_RADIUS {
                0.0
            } else {
                (d * 0.08).clamp(0.3, SHIP_CRUISE_SPEED)
            }
        }
    }
}

/// `nn.py::_ship_collision_threat` : premier hostile dont le passage au plus
/// près survient dans la fenêtre de temps sous le dégagement minimal.
fn ship_collision_threat<'a>(
    obs: &Observation,
    hostiles: &[&'a driver::NearbyObject],
    attack_target: Option<&driver::NearbyObject>,
) -> Option<&'a driver::NearbyObject> {
    let pvx = obs.ship.vx;
    let pvy = obs.ship.vy;
    let mut threat: Option<&driver::NearbyObject> = None;
    let mut threat_t = f64::INFINITY;
    for &s in hostiles {
        let (dx, dy) = ship_body_delta(obs, s);
        let rlen = dx.hypot(dy);
        if attack_target.is_some_and(|a| std::ptr::eq(a, s)) && rlen >= SHIP_ATTACK_STANDOFF {
            continue;
        }
        if rlen > SHIP_AVOID_RADIUS {
            continue;
        }
        let vrx = s.vx - pvx;
        let vry = s.vy - pvy;
        let v2 = vrx * vrx + vry * vry;
        let t = if v2 > 1e-9 {
            -(dx * vrx + dy * vry) / v2
        } else {
            0.0
        };
        if t < 0.0 {
            continue; // déjà en train de s'éloigner
        }
        if (dx + vrx * t).hypot(dy + vry * t) < SHIP_AVOID_CLEARANCE
            && t < SHIP_AVOID_TIME
            && t < threat_t
        {
            threat = Some(s);
            threat_t = t;
        }
    }
    threat
}

/// `nn.py::_ship_avoid_aim` : cap d'esquive (perpendiculaire à l'approche
/// relative, côté qui écarte déjà) et vitesse visée.
fn ship_avoid_aim(obs: &Observation, s: &driver::NearbyObject) -> (f64, f64) {
    let pvx = obs.ship.vx;
    let pvy = obs.ship.vy;
    let (dx, dy) = ship_body_delta(obs, s);
    let rlen = dx.hypot(dy);
    let vrx = s.vx - pvx;
    let vry = s.vy - pvy;
    let v2 = vrx * vrx + vry * vry;
    let (wx, wy) = if v2 > 1e-9 {
        let vlen = v2.sqrt();
        (vrx / vlen, vry / vlen)
    } else if rlen > 1e-9 {
        (dx / rlen, dy / rlen)
    } else {
        (1.0, 0.0)
    };
    let (px1, py1) = (wy, -wx);
    let (px2, py2) = (-wy, wx);
    let (ex, ey) = if pvx * px1 + pvy * py1 >= pvx * px2 + pvy * py2 {
        (px1, py1)
    } else {
        (px2, py2)
    };
    (ey.atan2(ex), SHIP_CRUISE_SPEED)
}

/// **Visée de mission** de la conduite vaisseau (`nn.py::_ship_drive_features`) :
/// mission en one-hot, cap effectif (esquive comprise) et son erreur
/// d'alignement, vitesse visée, vitesse projetée, écarts du mode 4 WAYS et
/// drapeaux de conduite. Sans elle, le signe de rotation de l'expert n'est pas
/// décidable depuis les features.
#[allow(clippy::too_many_lines)] // un miroir linéaire de nn.py
fn ship_drive_features(obs: &Observation, out: &mut Vec<f64>) {
    if obs.pilot != "vaisseau" {
        out.extend(std::iter::repeat_n(0.0, SHIP_DRIVE_FEATURES));
        return;
    }
    let nearby = &obs.nearby;
    let (goal, target, attack) = ship_mission(obs, nearby);
    let hostiles: Vec<&driver::NearbyObject> = nearby
        .iter()
        .filter(|o| SHIP_HOSTILE_KINDS.contains(&o.kind.as_str()) && o.life > 0)
        .collect();
    let threat = ship_collision_threat(obs, &hostiles, attack);
    let (aim, desired) = match threat {
        Some(t) => ship_avoid_aim(obs, t),
        None => {
            let (dx, dy) = match target {
                Some(t) => ship_body_delta(obs, t),
                None => ship_station_delta(obs),
            };
            (dy.atan2(dx), ship_desired_speed(goal, dx.hypot(dy)))
        }
    };
    let orientation = obs.ship.orientation;
    let direction = obs.ship.direction;
    let err = wrap_angle(aim - orientation);
    // vitesse **par frame** (l'observation rapporte des unités/s)
    let velocity = obs.ship.speed / 60.0;
    let vx = direction.cos() * velocity;
    let vy = -direction.sin() * velocity;
    let v_along = vx * aim.cos() + vy * aim.sin();
    let ex = aim.cos() * desired - vx;
    let ey = aim.sin() * desired - vy;
    let threat_ahead = threat.is_some_and(|t| {
        let (dx, dy) = ship_body_delta(obs, t);
        let rlen = dx.hypot(dy);
        rlen >= 1e-9 && (direction.cos() * dx - direction.sin() * dy) / rlen > 0.0
    });
    let overspeed = v_along > desired + 0.15;
    let settle = desired < SHIP_SETTLE_BAND && v_along > 0.02;
    out.push(if goal == ShipGoal::Dock { 1.0 } else { 0.0 });
    out.push(if goal == ShipGoal::Attack { 1.0 } else { 0.0 });
    out.push(if goal == ShipGoal::Collect { 1.0 } else { 0.0 });
    out.push(if goal == ShipGoal::Patrol { 1.0 } else { 0.0 });
    out.push(angle_feature(aim));
    out.push(clamp(err / std::f64::consts::PI, -1.0, 1.0));
    out.push(clamp(desired / SHIP_CRUISE_SPEED, 0.0, 1.0));
    out.push(clamp(v_along / SHIP_CRUISE_SPEED, -1.0, 1.0));
    out.push(clamp(ex / SHIP_CRUISE_SPEED, -1.0, 1.0));
    out.push(clamp(ey / SHIP_CRUISE_SPEED, -1.0, 1.0));
    out.push(if threat.is_some() { 1.0 } else { 0.0 });
    out.push(if threat_ahead { 1.0 } else { 0.0 });
    out.push(if overspeed { 1.0 } else { 0.0 });
    out.push(if settle { 1.0 } else { 0.0 });
}

/// Mode de déplacement du vaisseau en one-hot (`nn.py::_moving_mode_features`).
/// Un mode absent ou inconnu laisse le one-hot à zéro (le défaut de
/// l'observation côté jeu est INERTIAL, 0).
fn moving_mode_features(obs: &Observation, out: &mut Vec<f64>) {
    for m in 0..MOVING_MODES {
        out.push(if obs.moving_mode == m as i32 { 1.0 } else { 0.0 });
    }
}

/// **Balles en vol** (les `BULLET_SLOTS` plus proches) : nombre normalisé puis
/// cinématique relative de chacune (`nn.py::_bullets_features`). L'autopilote
/// vaisseau s'en sert pour retenir son feu.
fn bullets_features(obs: &Observation, out: &mut Vec<f64>) {
    let used = obs.bullets.len().min(BULLET_SLOTS);
    out.push(clamp(
        used as f64 / BULLET_SLOTS as f64,
        0.0,
        1.0,
    ));
    for b in obs.bullets.iter().take(BULLET_SLOTS) {
        out.push(clamp(b.dx / SCALE_DIST, -1.0, 1.0));
        out.push(clamp(b.dy / SCALE_DIST, -1.0, 1.0));
        out.push(clamp(b.dist / SCALE_DIST, 0.0, 1.0));
        out.push(clamp(b.vx / SCALE_SPEED, -1.0, 1.0));
        out.push(clamp(b.vy / SCALE_SPEED, -1.0, 1.0));
    }
    for _ in used..BULLET_SLOTS {
        out.extend(std::iter::repeat_n(0.0, BULLET_SLOT_LEN));
    }
}

/// Features des **objectifs DAG** du scénario courant
/// (`nn.py::_objective_features`) : part complétée, mission en cours, rang dans
/// la chaîne et progression chiffrée. Zéros hors scénario à objectifs.
fn objective_features(obs: &Observation, out: &mut Vec<f64>) {
    if obs.objectives.is_empty() {
        out.extend([0.0, 0.0, 0.0, 0.0, 0.0]);
        return;
    }
    let total = obs.objectives.len() as f64;
    let completed = obs.objectives.iter().filter(|o| o.completed).count() as f64;
    let done_part = clamp(completed / total, 0.0, 1.0);
    let mut mission: Option<&driver::ObjectiveInfo> = None;
    let mut mission_idx = 0.0;
    for (i, o) in obs.objectives.iter().enumerate() {
        if o.unlocked && !o.completed {
            mission = Some(o);
            mission_idx = i as f64 / total;
            break;
        }
    }
    let progress = match mission {
        Some(m) if m.required > 0.0 => clamp(m.current / m.required, 0.0, 1.0),
        _ => 0.0,
    };
    out.push(done_part);
    out.push(1.0 - done_part);
    out.push(if mission.is_some() { 1.0 } else { 0.0 });
    out.push(mission_idx);
    out.push(progress);
}

/// Vecteur de features d'une observation - **l'entrée du réseau**, dans
/// l'ordre exact de `nn.py::obs_features` (mêmes champs manquants = zéro, même
/// ordre). Toute modification ici doit être répercutée dans `nn.py` **et** le
/// numéro de `FEATURES_VERSION` incrémenté, sinon les poids embarqués ne
/// correspondent plus.
#[allow(clippy::too_many_lines)] // un miroir linéaire de l'ordre des features
pub fn features(obs: &Observation) -> Vec<f64> {
    let dx = obs.station_dx;
    let dy = obs.station_dy;
    let mut f: Vec<f64> = Vec::with_capacity(FEATURE_COUNT);
    // ── contexte global ────────────────────────────────────────────────────
    f.push(if obs.eva_active { 1.0 } else { 0.0 });
    f.push(if obs.docked { 1.0 } else { 0.0 });
    f.push(if obs.economy { 1.0 } else { 0.0 });
    // hystérésis du frein tangentiel de l'autopilote EVA (état interne : deux
    // cinématiques identiques peuvent porter des actions contradictoires)
    f.push(if obs.eva_tang_braking { 1.0 } else { 0.0 });
    f.push(dx / SCALE_DIST);
    f.push(dy / SCALE_DIST);
    f.push(obs.station_dist / SCALE_DIST);
    // ── cinématique du vaisseau puis du cosmonaute EVA ─────────────────────
    pilot_features(&obs.ship, dx, dy, &mut f);
    pilot_features(&obs.eva, dx, dy, &mut f);
    // ── décisions de l'autopilote EVA ──────────────────────────────────────
    eva_decision_features(obs, dx, dy, &mut f);
    // ── décisions de l'autopilote vaisseau (retenue de feu) ────────────────
    ship_decision_features(obs, &mut f);
    // ── visée de mission de la conduite (cap, vitesse visée, esquive) ──────
    ship_drive_features(obs, &mut f);
    // ── économie (boucle de minage du vaisseau) ────────────────────────────
    f.push(obs.fuel / obs.fuel_cap.max(1.0));
    f.push(f64::from(obs.ammo) / f64::from(obs.ammo_cap.max(1)));
    f.push(f64::from(obs.credits) / SCALE_CREDITS);
    f.push(f64::from(obs.cargo_qty) / f64::from(obs.cargo_cap.max(1)));
    // ── mode de déplacement (la conduite en dépend) ────────────────────────
    moving_mode_features(obs, &mut f);
    // ── réserves payables au magasin (mission de ravitaillement) ───────────
    f.push(if obs.supplies_affordable { 1.0 } else { 0.0 });
    // ── objectifs DAG du scénario ──────────────────────────────────────────
    objective_features(obs, &mut f);
    // ── objets proches (slots fixes, les plus proches) ─────────────────────
    let mut used = 0;
    for o in obs.nearby.iter().take(NEARBY_SLOTS) {
        for kind in NEARBY_KINDS {
            f.push(if o.kind == kind { 1.0 } else { 0.0 });
        }
        f.push(clamp(o.dx / SCALE_DIST, -1.0, 1.0));
        f.push(clamp(o.dy / SCALE_DIST, -1.0, 1.0));
        f.push(clamp(o.dist / SCALE_DIST, 0.0, 1.0));
        f.push(clamp(o.vx / SCALE_SPEED, -1.0, 1.0));
        f.push(clamp(o.vy / SCALE_SPEED, -1.0, 1.0));
        f.push(clamp(o.radius / SCALE_RADIUS, 0.0, 1.0));
        f.push(clamp(f64::from(o.life) / SCALE_LIFE, 0.0, 1.0));
        used += 1;
    }
    // slots restants : zéros (aucun objet à cette distance)
    for _ in used..NEARBY_SLOTS {
        f.extend(std::iter::repeat_n(0.0, NEARBY_SLOT_LEN));
    }
    // ── balles en vol (retenue de feu de l'autopilote vaisseau) ────────────
    bullets_features(obs, &mut f);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fenêtres d'une **vraie partie** enregistrées côté jeu : observation,
    /// features et action de la **politique Python** sur cette observation
    /// (`tools/trainer/validate_learned_port.py --record`). Le portage doit
    /// reproduire les deux.
    #[derive(Deserialize)]
    struct Window {
        obs: Observation,
        features: Vec<f64>,
        action: PilotInputs,
    }

    const WINDOWS: &str = include_str!("../tools/trainer/fixtures/learned_pilot_windows.jsonl");

    fn windows() -> Vec<Window> {
        WINDOWS
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("fenêtre de fidélité illisible"))
            .collect()
    }

    /// Le vecteur de features a la taille attendue et le fichier de politique
    /// embarqué est bien formé (sinon la stratégie apprise retomberait
    /// silencieusement sur la loi scriptée).
    #[test]
    fn embedded_policy_is_valid_and_feature_count_matches() {
        assert!(available(), "la politique embarquée doit être jouable");
        assert_eq!(features(&Observation::default()).len(), FEATURE_COUNT);
    }

    /// Le réseau embarqué est exécutable et produit des probabilités de
    /// rotation valides (somme = 1, une classe choisie).
    #[test]
    fn embedded_policy_forward_is_a_distribution() {
        let p = policy().expect("politique embarquée");
        let out = p.forward(&features(&Observation::default()));
        assert_eq!(out.len(), p.outputs);
        let turn: f64 = out[SIGMOID_OUTPUTS..].iter().sum();
        assert!((turn - 1.0).abs() < 1e-9, "softmax non normalisé : {turn}");
        for v in &out[..SIGMOID_OUTPUTS] {
            assert!((0.0..=1.0).contains(v), "sigmoïde hors [0, 1] : {v}");
        }
    }

    /// Cœur du portage : sur chaque observation enregistrée, les features
    /// calculées ici égalent celles de `nn.py` (à la précision machine) et
    /// l'action gloutonne est **exactement** celle de la politique Python.
    #[test]
    fn port_matches_python_reference() {
        let p = policy().expect("politique embarquée");
        let windows = windows();
        assert!(!windows.is_empty(), "fixture de fidélité vide");
        let mut worst = 0.0f64;
        let mut mismatches = 0;
        for (i, w) in windows.iter().enumerate() {
            let ours = features(&w.obs);
            assert_eq!(ours.len(), w.features.len(), "fenêtre {i} : taille");
            for (a, b) in ours.iter().zip(&w.features) {
                worst = worst.max((a - b).abs());
            }
            if p.act(&w.obs) != w.action {
                mismatches += 1;
            }
        }
        assert!(
            worst < 1e-9,
            "features divergentes : erreur absolue max {worst:e}"
        );
        assert_eq!(mismatches, 0, "{mismatches} décisions sur {} diffèrent de la politique Python", windows.len());
    }
}
