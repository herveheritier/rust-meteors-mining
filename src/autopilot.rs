//! Pilote automatique (option AUTOPILOT de l'écran de paramétrage, touche X) :
//! l'ordinateur joue à la place du pilote - il **protège la station** (détruit
//! les météores et aliens qui s'en approchent), **détruit les météores**,
//! **récupère les minerais** libres dans l'espace, et **rentre à la station
//! quand la soute est pleine** pour décharger, avant de repartir miner. Il
//! conduit selon le **sens d'avancement réel** projeté sur la cible (un
//! vaisseau qui dérive dans un mode à inertie ne fonce plus à l'opposé de son
//! objectif) et **évite les hostiles** qui croisent sa trajectoire (esquive
//! latérale quand un passage au plus près devient dangereux - en freinant
//! pendant le virage si la menace est devant - tir maintenu).
//! Les décisions sont prises à chaque frame sous forme d'entrées identiques
//! aux touches du joueur (↑/↓/←/→ + tir) : `input::player_controls` les
//! consomme à la place du clavier / tactile / télécommande / manette quand
//! l'option est active. Quand le vaisseau est détruit, il pilote aussi le
//! **cosmonaute EVA** (`autopilot_eva_inputs`) : son seul objectif est de le
//! ramener à la station pour qu'il soit secouru - en **bornant sa vitesse**
//! (poussée vectorielle sans frein : on coupe avant la visée, et on
//! **contre-pousse** nez à l'opposé quand l'approche est trop rapide) pour
//! que le retour reste contrôlé.
//!
//! NB : l'autopilote joue avec les règles du scénario (carburant, munitions).
//! En scénario à économie, il consomme du carburant et des munitions comme le
//! joueur ; quand les réserves passent sous un seuil (et que les crédits le
//! permettent), il rentre à la station pour se **ravitailler au magasin**
//! (carburant + munitions achetés au maximum achetable - les crédits viennent
//! du déchargement de la soute) avant de repartir miner.

use crate::config::*;
use crate::docking::{release_links, shortest_angle_delta, undock};
use crate::geom::{wrapped_delta, wrapped_distance, Point};
use crate::scenario;
use crate::shape::Shape;
use crate::state::{Element, GameState};

/// Entrées de pilotage calculées par l'autopilote : mêmes primitives que les
/// touches du joueur (voir `input::player_controls`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PilotInputs {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub fire: bool,
}

/// Mission courante de l'autopilote (rechoisie à chaque frame).
#[derive(Debug, Clone, Copy)]
enum Goal {
    /// Rentrer accoster à la station (soute pleine) : on ralentit à l'approche
    /// pour déclencher l'accostage automatique.
    Dock,
    /// Détruire un hostile (météore / alien) à cette position - on s'arrête à
    /// distance de sécurité pour ne pas le percuter.
    Attack(Point),
    /// Récupérer un minerai libre à cette position.
    Collect(Point),
    /// Rien à faire : stationner près de la station sans accoster (on évite
    /// l'allée-et-venue accostage/départ à vide).
    Patrol,
}

// ─── réglages du comportement ────────────────────────────────────────────────
/// Vitesse de croisière visée (unités monde / frame, ×60 = unités/s).
const CRUISE_SPEED: f64 = 1.8;
/// Distance de sécurité à laquelle l'autopilote s'arrête devant un hostile :
/// le météore est détruit au tir, pas en le percutant.
const ATTACK_STANDOFF: f64 = 70.0;
/// Rayon dans lequel un hostile est jugé dangereux pour la station (mission
/// « protéger la station » - prioritaire).
const STATION_GUARD_RADIUS: f64 = 240.0;
/// Portée de tir : on ne tire que sur un hostile dans cette distance.
const FIRE_RANGE: f64 = 210.0;
/// Tolérance d'alignement avant de tirer (radians, ~8°).
const AIM_TOLERANCE: f64 = 0.14;
/// Tolérance d'alignement avant de pousser (radians, ~9°).
const THRUST_DEADBAND: f64 = 0.16;
/// Tolérance d'alignement avant de tourner (radians, ~6°).
const TURN_DEADBAND: f64 = 0.10;
/// Rayon de stationnement quand il n'y a plus rien à faire (hors zone
/// d'accostage : le vaisseau ne se re-accoste pas pour rien).
const PATROL_RADIUS: f64 = 120.0;
/// Rayon (autour de la station) à partir duquel l'approche d'accostage
/// ralentit franchement.
const DOCK_SLOW_ZONE: f64 = 90.0;
/// Seuil de ravitaillement (scénario à économie) : quand le carburant ou les
/// munitions passent sous cette fraction de leur capacité, l'autopilote rentre
/// à la station se ravitailler (si les crédits le permettent - la marge doit
/// couvrir le trajet de retour).
const LOW_SUPPLY_RATIO: f64 = 0.30;
/// Évitement de collision : rayon de détection autour du vaisseau (unités) -
/// couvre la distance maximale parcourue par une approche relative dans la
/// fenêtre de temps (vaisseau + météore au plus rapide).
/// Zone de sécurité autour d'un minerai : un hostile plus près que cette
/// distance (ex. un fragment survivant du météore détruit, à la position
/// duquel les minerais apparaissent) doit être détruit avant de ramasser -
/// la collecte au contact d'un météore vivant détruit le vaisseau
/// (« 1 impact = détruit »).
const MINERAL_CLEARANCE: f64 = 130.0;
/// Cible d'attaque **quasi détruite** (triangles restants ≤ ce seuil) : on
/// **retient le feu** - les balles déjà en vol l'achèvent, et une balle
/// supplémentaire traverserait le point d'apparition des minerais (position
/// du météore mort) et les **détruirait** (règle du jeu : une balle qui
/// touche un minerai le détruit - mesuré : les minerais de la graine 2,
/// détruits par les balles en vol du vaisseau, laissaient la soute vide).
/// La retenue est conditionnelle (`HOLD_FIRE_*`) : si les balles en vol
/// ratent, le feu reprend dès qu'elles sont passées (pas de blocage).
const HOLD_FIRE_LIFE: i32 = 2;
/// Rayon autour de la cible dans lequel une balle en vol compte comme
/// « déjà partie pour l'achever » : couvre la portée de tir (les balles
/// voyagent jusqu'à ~3,8 u/frame × ~35 frames depuis la distance de tir).
const HOLD_FIRE_RADIUS: f64 = 150.0;
/// Distance cible↔minerai sous laquelle on **retient le feu** : un météore
/// qui se fragmente libère ses minerais **à la position des fragments
/// voisins encore vivants** (ils étaient adjacents) - une balle tirée sur le
/// fragment traverserait le point d'apparition et détruirait les minerais.
/// En retenant le feu, le fragment **absorbe** les minerais (règle du jeu :
/// un météore qui percute un minerai l'avale - ils ressortiront à sa
/// destruction) et le tir reprend une fois la zone dégagée.
const HOLD_FIRE_MINERAL_RADIUS: f64 = 60.0;

const AVOID_RADIUS: f64 = 360.0;
/// Évitement : séparation minimale au passage au plus près (unités) - en
/// dessous, l'hostile est jugé dangereux (rayons du vaisseau et d'un gros
/// météore + marge).
const AVOID_CLEARANCE: f64 = 90.0;
/// Évitement : fenêtre de temps du passage au plus près (secondes) - au-delà,
/// la trajectoire n'est pas encore dangereuse.
const AVOID_TIME: f64 = 1.5;
/// Évitement : au-dessus de cette vitesse (unités/frame), l'esquive **freine**
/// pendant le virage quand la menace est devant - la marche arrière le long du
/// nez qui tourne réduit la vitesse de fermeture au lieu de foncer dedans.
const DODGE_BRAKE_SPEED: f64 = 0.5;
/// Vitesse maximale de **croisière** du cosmonaute EVA (unités/frame, ×60 =
/// unités/s) : l'ordinateur borne la poussée pour ne jamais dépasser cette
/// vitesse **loin de la station** - le cosmonaute n'a qu'une poussée
/// vectorielle (↑), pas de frein ; sans limite, il accélérerait indéfiniment
/// et arriverait sur la station beaucoup trop vite pour être contrôlé.
/// Relevée à 1,5 (90 u/s, +50 %) pour raccourcir le retour : l'approche
/// finale reste maîtrisée par `EVA_ARRIVAL_SPEED` et le freinage **anticipé**
/// (`EVA_TURN_FRAMES` : on se retourne et on contre-pousse assez tôt pour
/// retomber sur la vitesse d'arrivée avant le cercle d'accostage).
const EVA_MAX_SPEED: f64 = 1.5;
/// Vitesse d'**arrivée** du cosmonaute EVA (unités/frame) : la visée de
/// croisière retombe sur cette valeur au cercle d'accostage, et c'est la
/// vitesse sous laquelle il n'y a **plus rien à freiner** - la dérive suffit
/// à déclencher la récupération (elle n'exige que la distance, pas la
/// vitesse).
const EVA_ARRIVAL_SPEED: f64 = 0.5;
/// Rayon (autour de la station) où l'approche du cosmonaute EVA ralentit :
/// la visée décroît **linéairement** de `EVA_MAX_SPEED` (au bord) à
/// `EVA_ARRIVAL_SPEED` (au cercle d'accostage). Assez large (240) pour que
/// le freinage anticipé (demi-tour ~1,75 s à vitesse constante + décélération
/// par contre-poussée) ait la place de s'exécuter après une croisière rapide.
const EVA_SLOW_ZONE: f64 = 240.0;
/// Demi-bande de vitesse du cosmonaute EVA (unités/frame) : entre la visée
/// moins et la visée plus cette bande, l'ordinateur ne pousse ni n'accélère -
/// il laisse dériver (hystérésis : pas de rafales ↑/contre-poussée à chaque
/// frame). Réglée **étroite** (9 u/s) : la croisière effective vaut ≈ visée −
/// bande (≈ 1,35 u/frame ≈ 81 u/s) - l'ancienne bande large (40 u/s) coupait
/// la poussée très tôt et le cosmonaute **dérivait à ~20 u/s** le plus clair
/// du trajet, ce qui allongeait le retour ; la bande d'alignement serrée
/// (`EVA_TURN_DEADBAND_*`) et le freinage tangentiel (`EVA_TANG_BRAKE_*`)
/// gardent la trajectoire droite malgré une poussée plus soutenue.
const EVA_SPEED_BAND: f64 = 0.15;
/// Durée d'un demi-tour du cosmonaute EVA (frames à 60 images/s) :
/// π / `PLAYER_ROTATION_SPEED` = 105 frames ≈ 1,75 s. Le **freinage anticipé**
/// en tient compte : pendant le demi-tour (gaz coupés, cf. plus bas) le
/// cosmonaute dérive à vitesse constante, il faut donc anticiper ce chemin
/// pour pouvoir contre-pousser assez tôt et retomber sur `EVA_ARRIVAL_SPEED`
/// avant la zone d'accostage.
const EVA_TURN_FRAMES: f64 = std::f64::consts::PI / crate::config::PLAYER_ROTATION_SPEED;
/// Bande d'alignement du cosmonaute EVA - **minimum** (radians) : l'écart de
/// visée sous lequel il se considère aligné (et pousse). Réglé **très serré**
/// (0,86°) contre `TURN_DEADBAND` du vaisseau (0,10 rad ≈ 5,7°) : une bande
/// lâche laisse pousser nez désaligné, la poussée acquiert une composante
/// tangentielle et le retour sans frottement se met en orbite autour de la
/// base au lieu d'entrer dans le cercle d'accostage. La bande **effective**
/// est `max(EVA_TURN_DEADBAND_MIN, EVA_TURN_DEADBAND_SCALE × rotation par
/// frame)` : elle ne peut pas être plus serrée qu'environ un demi-pas de
/// rotation par frame, sinon la commande tout-ou-rien oscille autour de la
/// visée sans jamais entrer dans la bande (cycle limite - mesuré à bas FPS).
const EVA_TURN_DEADBAND_MIN: f64 = 0.015;
/// Facteur de la bande d'alignement EVA par rapport au pas de rotation d'une
/// frame (`PLAYER_ROTATION_SPEED × 60 × dt`) : la bande effective vaut
/// `max(EVA_TURN_DEADBAND_MIN, rotation_par_frame × ce facteur)`. Un pas de
/// rotation par frame doit suffire à ramener le nez **dans** la bande : le
/// facteur est donc ≥ 0,5 (mesuré robuste à 0,75, tous FPS confondus).
const EVA_TURN_DEADBAND_SCALE: f64 = 0.75;
/// Seuil **haut** du freinage tangentiel du cosmonaute EVA (unités/frame) :
/// quand la composante de vitesse perpendiculaire à la direction de la
/// station dépasse ce seuil, l'ordinateur **contre-pousse nez à l'opposé de
/// la vitesse** (`π − direction`) pour casser l'orbite - la seule façon de
/// tuer une vitesse tangentielle sans frein. Le freinage sur la projection
/// radiale ne voit pas cette composante (elle vaut ~0 quand on orbite) : sans
/// ce correctif, un retour de très loin (≥ ~1200 u) finit en orbite stable
/// autour de la base au lieu d'entrer dans le cercle d'accostage.
const EVA_TANG_BRAKE_HI: f64 = 0.25;
/// Seuil **bas** du freinage tangentiel (unités/frame) : une fois le
/// freinage déclenché (`EVA_TANG_BRAKE_HI`), il reste actif tant que la
/// composante tangentielle n'est pas retombée sous ce seuil - l'**hystérésis**
/// (mémorisée dans `state.eva_tang_braking`) évite d'alterner poussée et
/// freinage à chaque frame quand la composante oscille autour du seuil.
const EVA_TANG_BRAKE_LO: f64 = 0.12;

/// Un hostile pour l'autopilote : météore (y compris le boss, `WHOIAM_METEOR`)
/// ou alien, vivant.
fn is_hostile(s: &Shape) -> bool {
    (s.who_i_am == WHOIAM_METEOR || s.who_i_am == WHOIAM_ALIEN) && s.life > 0
}

/// Centre du **corps** d'une forme (position + centre) : la position de
/// collision réelle. Les triangles sont tournés autour de `shape.center` puis
/// translatés par `shape.position` (`compute_real_positions`), et la détection
/// de collision pré-filtre sur `position + center` (`game.rs`) : le corps d'un
/// météore asymétrique (ou d'un fragment) est décalé de son `position`. Viser
/// `position` seule ratait la cible de la distance du centre - mesuré : 27/30
/// tirs ratés sur un météore à 72 u dont le corps était 37 u au-dessus du
/// point visé. La station et les minerais ont un centre nul (le point visé est
/// leur position) ; le vaisseau a un centre quasi nul (1,0).
fn body_center(s: &Shape) -> Point {
    Point::new(s.position.x + s.center.x, s.position.y + s.center.y)
}

/// Carburant ou munitions sous le seuil de ravitaillement (scénario à
/// économie) ? Les munitions sont le **total des armes possédées** (voir
/// `scenario::total_ammo` / `total_ammo_capacity`).
fn supplies_low(state: &GameState) -> bool {
    let fuel_cap = scenario::fuel_capacity(state);
    if fuel_cap > 0.0 && state.resources.fuel < fuel_cap * LOW_SUPPLY_RATIO {
        return true;
    }
    let ammo_cap = scenario::total_ammo_capacity(state);
    ammo_cap > 0 && (scenario::total_ammo(state) as f64) < ammo_cap as f64 * LOW_SUPPLY_RATIO
}

/// Les crédits courants couvrent au moins un paquet de carburant ou de
/// munitions d'une arme possédée ? Le coût doit être **strictement positif**
/// : une remise de réputation peut ramener le prix d'un paquet à 0 par
/// troncature entière (`discounted_cost(1, 8) = 0`), mais le magasin refuse
/// d.vendre à coût nul (`buy_fuel_qty` renvoie `Full`) - considérer ce
/// paquet comme « achetable » faisait rentrer le vaisseau se ravitailler
/// avec 0 crédit puis boucler magasin↔accostage sans rien acheter.
fn supplies_affordable(state: &GameState) -> bool {
    let fuel_qty = scenario::affordable_fuel_qty(state);
    if fuel_qty > 0.0 && scenario::fuel_qty_cost(state, fuel_qty) > 0 {
        return true;
    }
    (0..scenario::weapon_slot_count())
        .filter(|&i| scenario::weapon_owned(state, i))
        .any(|i| {
            let qty = scenario::affordable_ammo_qty(state, i);
            qty > 0 && scenario::ammo_qty_cost(state, i, qty) > 0
        })
}

/// À la station : un ravitaillement est-il utile **et** possible ? (réservoirs
/// pas pleins ET de quoi payer au moins un paquet - sinon repartir : miner
/// rapporte les crédits qui manquent)
fn supplies_need_buying(state: &GameState) -> bool {
    let fuel_missing = (scenario::fuel_capacity(state) - state.resources.fuel).max(0.0);
    let ammo_missing = (0..scenario::weapon_slot_count())
        .filter(|&i| scenario::weapon_owned(state, i))
        .any(|i| scenario::ammo_capacity(state) - state.resources.weapon_ammo[i] > 0);
    (fuel_missing > 0.0 || ammo_missing) && supplies_affordable(state)
}

/// Menace de collision imminente : premier hostile (météore / alien) dont le
/// **passage au plus près** du vaisseau - positions et vitesses projetées -
/// survient dans moins de `AVOID_TIME` secondes, à moins de `AVOID_CLEARANCE`
/// unités. Renvoie l'index de la menace la plus imminente (plus petit temps).
/// La cible d'attaque en cours est ignorée tant qu'elle est à distance de tir
/// (`ATTACK_STANDOFF`) : on s'en approche pour la détruire, on ne l'évite pas.
/// Elle ne redevient une menace que si elle referme la distance sous le
/// standoff (elle dérive vers le vaisseau).
fn collision_threat(
    state: &GameState,
    shapes: &[Shape],
    attack_idx: Option<usize>,
) -> Option<usize> {
    let player = &shapes[PLAYER_INDEX];
    // vitesses en unités/seconde (velocity est par frame, ×60)
    let pvx = player.direction.cos() * player.velocity * 60.0;
    let pvy = -player.direction.sin() * player.velocity * 60.0;
    let mut threat: Option<(f64, usize)> = None;
    for (i, s) in shapes.iter().enumerate() {
        if i == PLAYER_INDEX || !is_hostile(s) {
            continue;
        }
        let r = wrapped_delta(body_center(player), body_center(s), &state.world);
        let rlen = r.x.hypot(r.y);
        if attack_idx == Some(i) && rlen >= ATTACK_STANDOFF {
            continue; // cible d'attaque en approche : pas une menace
        }
        if rlen > AVOID_RADIUS {
            continue;
        }
        let tvx = s.direction.cos() * s.velocity * 60.0;
        let tvy = -s.direction.sin() * s.velocity * 60.0;
        let vrx = tvx - pvx;
        let vry = tvy - pvy;
        let v2 = vrx * vrx + vry * vry;
        // temps du passage au plus près (closest approach) : −(r·vr)/|vr|²
        let t = if v2 > 1e-9 { -(r.x * vrx + r.y * vry) / v2 } else { 0.0 };
        if t < 0.0 {
            continue; // déjà en train de s'éloigner
        }
        let cx = r.x + vrx * t;
        let cy = r.y + vry * t;
        let dmin = (cx * cx + cy * cy).sqrt();
        if dmin < AVOID_CLEARANCE && t < AVOID_TIME && threat.is_none_or(|(bt, _)| t < bt) {
            threat = Some((t, i));
        }
    }
    threat.map(|(_, i)| i)
}

/// Direction d'**esquive** face à la menace `i` : perpendiculaire à la
/// trajectoire d'approche relative (vitesse relative unitaire - ligne de visée
/// en secours si les vitesses sont quasi nulles), côté qui **écarte déjà** le
/// vaisseau (plus grande projection de la vitesse courante, sinon un côté par
/// défaut). Renvoie l'angle écran de l'esquive et la vitesse visée.
fn avoid_aim(state: &GameState, shapes: &[Shape], i: usize) -> (f64, f64) {
    let player = &shapes[PLAYER_INDEX];
    let s = &shapes[i];
    let pvx = player.direction.cos() * player.velocity * 60.0;
    let pvy = -player.direction.sin() * player.velocity * 60.0;
    let r = wrapped_delta(body_center(player), body_center(s), &state.world);
    let rlen = r.x.hypot(r.y);
    let tvx = s.direction.cos() * s.velocity * 60.0;
    let tvy = -s.direction.sin() * s.velocity * 60.0;
    let vrx = tvx - pvx;
    let vry = tvy - pvy;
    let v2 = vrx * vrx + vry * vry;
    // trajectoire d'approche (relative) unitaire - ligne de visée en secours
    let (wx, wy) = if v2 > 1e-9 {
        (vrx / v2.sqrt(), vry / v2.sqrt())
    } else if rlen > 1e-9 {
        (r.x / rlen, r.y / rlen)
    } else {
        (1.0, 0.0)
    };
    // perpendiculaires : on prend le côté qui écarte déjà (projection de la
    // vitesse courante), sinon le premier
    let (px1, py1) = (wy, -wx);
    let (px2, py2) = (-wy, wx);
    let (ex, ey) = if pvx * px1 + pvy * py1 >= pvx * px2 + pvy * py2 {
        (px1, py1)
    } else {
        (px2, py2)
    };
    (ey.atan2(ex), CRUISE_SPEED)
}

/// Angle écran (radians, y vers le bas) du vecteur `from → to` le plus court
/// dans le monde torique : la direction dans laquelle le nez du vaisseau doit
/// pointer pour pousser / tirer vers la cible (l'orientation du vaisseau suit
/// cette convention - voir `docs/PORTAGE.md` §6).
pub fn screen_angle_to(from: Point, to: Point, state: &GameState) -> f64 {
    let d = wrapped_delta(from, to, &state.world);
    d.y.atan2(d.x)
}

/// Vitesse visée selon la mission et la distance à la cible (unités/frame).
fn desired_speed(goal: Goal, d: f64) -> f64 {
    match goal {
        // accostage : croisière au loin, puis ralentissement franc dans
        // l'anneau - sous ~12 unités, la vitesse cible passe sous
        // `STATION_DOCK_SPEED` et l'accostage automatique se déclenche
        Goal::Dock => {
            if d < DOCK_SLOW_ZONE {
                (d * 0.04 + 0.02).min(0.9)
            } else {
                CRUISE_SPEED
            }
        }
        // hostile : s'arrêter à distance de sécurité (le tir fait le travail)
        // hostile : s'arrêter à distance de sécurité (le tir fait le travail) -
        // rampe **consciente du freinage** (0,05/frame² ≈ décélération du
        // vaisseau) : à la croisière (1,8), la distance de freinage est
        // ≈ 32 u - la rampe 0,05 × d arrête le vaisseau juste à la limite de
        // tir (le profil 0,2 d'avant ne ralentissait que sur ~9 u et faisait
        // **survoler** la cible à pleine vitesse : le vaisseau percutait le
        // météore - contact = vaisseau détruit)
        Goal::Attack(_) => ((d - ATTACK_STANDOFF).max(0.0) * 0.05).min(CRUISE_SPEED),
        // minerai : se poser dessus **lentement** (ramassé par collision) -
        // la vitesse visée décroît avec la distance pour que le vaisseau
        // s'arrête sur le minerai au lieu de le traverser à vitesse de
        // croisière (distance de freinage ≈ v²/(2·0,05) : à 0,6 ≈ 3,6 u -
        // l'approche finale reste rattrapable même si le minerai dérive)
        Goal::Collect(_) => (d * 0.12).clamp(0.3, 1.4),
        // stationnement : approcher puis s'arrêter hors de la zone d'accostage
        Goal::Patrol => {
            if d < PATROL_RADIUS {
                0.0
            } else {
                (d * 0.08).clamp(0.3, CRUISE_SPEED)
            }
        }
    }
}

/// (tests) mission courante de l'autopilote, en texte - miroir du choix de
/// mission d'`autopilot_inputs`, utilisé par les tests unitaires pour
/// vérifier la priorité des missions.
pub fn debug_current_goal(state: &GameState, shapes: &[Shape]) -> String {
    let player = &shapes[PLAYER_INDEX];
    let station = &shapes[STATION_INDEX];
    let world = &state.world;
    let capacity = scenario::cargo_capacity(state);
    let cargo_full = capacity > 0 && state.player.cargo_qty >= capacity;
    let low_supplies =
        scenario::has_economy(state) && supplies_low(state) && supplies_affordable(state);
    let mut guard: Option<(f64, usize)> = None;
    let mut mineral: Option<(f64, usize)> = None;
    let mut hostile: Option<(f64, usize)> = None;
    for (i, s) in shapes.iter().enumerate() {
        if i == PLAYER_INDEX || s.life <= 0 {
            continue;
        }
        if is_hostile(s) {
            let ds = wrapped_distance(body_center(s), station.position, world);
            if ds < STATION_GUARD_RADIUS && guard.is_none_or(|(bd, _)| ds < bd) {
                guard = Some((ds, i));
            }
            let dp = wrapped_distance(body_center(s), body_center(player), world);
            if hostile.is_none_or(|(bd, _)| dp < bd) {
                hostile = Some((dp, i));
            }
        } else if s.who_i_am == WHOIAM_MINERAL {
            let dp = wrapped_distance(body_center(s), body_center(player), world);
            if mineral.is_none_or(|(bd, _)| dp < bd) {
                mineral = Some((dp, i));
            }
        }
    }
    let hostile_in_range = hostile.is_some_and(|(d, _)| d < FIRE_RANGE);
    let mineral_guarded = mineral.is_some_and(|(_, mi)| {
        hostile.is_some_and(|(_, hi)| {
            let d = wrapped_distance(body_center(&shapes[hi]), body_center(&shapes[mi]), world);
            d < MINERAL_CLEARANCE
        })
    });
    let goal = if cargo_full {
        "Dock(cargo_plein)".to_string()
    } else if guard.is_some() {
        "Attack(protège station)".to_string()
    } else if low_supplies {
        "Dock(ravitaillement)".to_string()
    } else if hostile_in_range || mineral_guarded {
        "Attack(hostile à portée / minerai gardé)".to_string()
    } else if mineral.is_some() {
        format!("Collect(@{:.0})", mineral.unwrap().0)
    } else if hostile.is_some() {
        format!("Attack(@{:.0})", hostile.unwrap().0)
    } else {
        "Patrol".to_string()
    };
    goal
}

/// Entrées de pilotage de la frame pour le pilote automatique : mission
/// (protéger la station / miner / collecter / rentrer), puis conduite du
/// vaisseau vers la cible selon le mode de déplacement (rotation du nez pour
/// les modes DIRECTIONAL / INERTIAL / REALISTIC, poussée dans les 4 directions
/// de l'écran pour 4 WAYS), et tir sur tout hostile à portée et aligné.
pub fn autopilot_inputs(state: &GameState, shapes: &[Shape]) -> PilotInputs {
    let mut out = PilotInputs::default();
    let player = &shapes[PLAYER_INDEX];
    let station = &shapes[STATION_INDEX];
    let world = &state.world;

    // ── choix de la mission de la frame ─────────────────────────────────────
    let capacity = scenario::cargo_capacity(state);
    let cargo_full = capacity > 0 && state.player.cargo_qty >= capacity;
    // scénario à économie : rentrer se ravitailler quand carburant ou
    // munitions passent sous le seuil - mais seulement si les crédits
    // permettent d'acheter au moins un paquet (sinon on continue de miner
    // pour gagner de quoi payer, au lieu de faire l'aller-retour à vide)
    let low_supplies =
        scenario::has_economy(state) && supplies_low(state) && supplies_affordable(state);
    let mut guard: Option<(f64, usize)> = None;
    let mut mineral: Option<(f64, usize)> = None;
    let mut hostile: Option<(f64, usize)> = None;
    for (i, s) in shapes.iter().enumerate() {
        if i == PLAYER_INDEX || s.life <= 0 {
            continue;
        }
        if is_hostile(s) {
            // hostile menaçant la station (mission prioritaire)
            let ds = wrapped_distance(body_center(s), station.position, world);
            if ds < STATION_GUARD_RADIUS && guard.is_none_or(|(bd, _)| ds < bd) {
                guard = Some((ds, i));
            }
            // hostile le plus proche (pour miner, et pour la cible de tir)
            let dp = wrapped_distance(body_center(s), body_center(player), world);
            if hostile.is_none_or(|(bd, _)| dp < bd) {
                hostile = Some((dp, i));
            }
        } else if s.who_i_am == WHOIAM_MINERAL {
            // minerai libre à récupérer (soute pas pleine - sinon on rentre)
            let dp = wrapped_distance(body_center(s), body_center(player), world);
            if mineral.is_none_or(|(bd, _)| dp < bd) {
                mineral = Some((dp, i));
            }
        }
    }
    // cible d'attaque de la mission (si l'une des deux branches Attack est
    // prise) : ignorée par l'évitement tant qu'on s'en approche (voir
    // `collision_threat`)
    let mut attack_idx: Option<usize> = None;
    // hostile à portée de tir : le vaisseau le **détruit avant de ramasser** -
    // ramasser un minerai au contact d'un météore vivant détruit le vaisseau
    // (« 1 impact = détruit »), et un météore à moitié détruit laisse des
    // fragments ; finir la destruction (météore + fragments) libère les
    // minerais dans une zone sans collision
    let hostile_in_range = hostile.is_some_and(|(d, _)| d < FIRE_RANGE);
    // minerai à ramasser trop près d'un hostile : on détruit l'hostile
    // d'abord - ramasser au contact d'un fragment survivant (= position du
    // météore détruit, où les minerais apparaissent) est mortel ; on ne
    // s'approche d'un minerai que si sa zone est dégagée (`MINERAL_CLEARANCE`)
    let mineral_guarded = mineral.is_some_and(|(_, mi)| {
        hostile.is_some_and(|(_, hi)| {
            let d = wrapped_distance(body_center(&shapes[hi]), body_center(&shapes[mi]), world);
            d < MINERAL_CLEARANCE
        })
    });
    let goal = if cargo_full {
        Goal::Dock
    } else if let Some((_, i)) = guard {
        // protéger la station reste la mission prioritaire (le trajet de
        // retour se fera juste après)
        attack_idx = Some(i);
        Goal::Attack(body_center(&shapes[i]))
    } else if low_supplies {
        Goal::Dock
    } else if hostile_in_range || mineral_guarded {
        let (_, i) = hostile.expect("hostile_in_range/mineral_guarded impliquent un hostile");
        attack_idx = Some(i);
        Goal::Attack(body_center(&shapes[i]))
    } else if let Some((_, i)) = mineral {
        Goal::Collect(body_center(&shapes[i]))
    } else if let Some((_, i)) = hostile {
        attack_idx = Some(i);
        Goal::Attack(body_center(&shapes[i]))
    } else {
        Goal::Patrol
    };

    // ── tir : tout hostile à portée, si le nez est aligné ───────────────────
    let mut fire_target: Option<usize> = None;
    let mut best_fire = FIRE_RANGE;
    for (i, s) in shapes.iter().enumerate() {
        if i == PLAYER_INDEX || !is_hostile(s) {
            continue;
        }
        let d = wrapped_distance(body_center(s), body_center(player), world);
        if d < best_fire {
            best_fire = d;
            fire_target = Some(i);
        }
    }
    if let Some(i) = fire_target {
        // cible quasi détruite avec des balles déjà en vol vers elle : on
        // retient le feu (elles l'achèvent ; tirer encore ferait traverser
        // le point d'apparition des minerais et les détruirait - voir
        // `HOLD_FIRE_LIFE`)
        // minerais dans le corridor de tir (le fragment vivant va les
        // absorber - les détruire d'une balle les perdrait définitivement)
        let minerals_near = shapes.iter().any(|m| {
            m.who_i_am == WHOIAM_MINERAL
                && m.life > 0
                && wrapped_distance(body_center(m), body_center(&shapes[i]), world)
                    < HOLD_FIRE_MINERAL_RADIUS
        });
        let holding = minerals_near
            || (shapes[i].life <= HOLD_FIRE_LIFE
                && shapes.iter().any(|b| {
                    b.who_i_am == WHOIAM_BULLET
                        && b.life > 0
                        && wrapped_distance(body_center(b), body_center(&shapes[i]), world)
                            < HOLD_FIRE_RADIUS
                }));
        if !holding {
            let aim = shortest_angle_delta(
                player.orientation,
                screen_angle_to(body_center(player), body_center(&shapes[i]), state),
            );
            if aim.abs() < AIM_TOLERANCE {
                out.fire = true;
            }
        }
    }

    // ── conduite : évitement de collision ou cible de la mission ────────────
    // quand un hostile va passer au plus près dans la fenêtre de temps, la
    // conduite est remplacée par une **esquive latérale** (`collision_threat` /
    // `avoid_aim`) - le tir, lui, reste indépendant : on continue de détruire
    // l'hostile qui fonce sur nous pendant qu'on l'évite
    let threat = collision_threat(state, shapes, attack_idx);
    let avoid = threat.map(|i| avoid_aim(state, shapes, i));
    // la menace est-elle **devant** le vaisseau (dans son sens d'avancement) ?
    // Dans ce cas l'esquive freine pendant le virage (réduire la vitesse de
    // fermeture) ; si elle est derrière (rattrapage), freiner serait contre-
    // productif - on s'écarte, on n'attend pas le rattrapeur
    let threat_ahead = threat
        .map(|i| {
            let r = wrapped_delta(body_center(player), body_center(&shapes[i]), &state.world);
            let rlen = r.x.hypot(r.y);
            if rlen < 1e-9 {
                return false;
            }
            let vx = player.direction.cos();
            let vy = -player.direction.sin();
            (vx * r.x + vy * r.y) / rlen > 0.0
        })
        .unwrap_or(false);
    let target = match goal {
        Goal::Dock | Goal::Patrol => station.position,
        Goal::Attack(p) | Goal::Collect(p) => p,
    };
    let d = wrapped_distance(body_center(player), target, world);
    let (aim, desired) = match avoid {
        Some((a, s)) => (a, s),
        None => (
            screen_angle_to(body_center(player), target, state),
            desired_speed(goal, d),
        ),
    };
    match state.moving_mode {
        // 4 WAYS : poussée dans les 4 directions de l'écran - on pousse vers
        // le vecteur de vitesse visé (direction + module), ce qui dirige et
        // freine en même temps ; le nez suit automatiquement la trajectoire.
        // Convention écran (y vers le bas - même que `input.rs` pour ce mode :
        // ↑ pousse −y, ↓ pousse +y, → pousse +x) : les composantes de vitesse
        // sont (cos, −sin) de la direction ; `aim` est un angle écran.
        MOVING_MODE_4_WAYS => {
            let vx = player.direction.cos() * player.velocity;
            let vy = -player.direction.sin() * player.velocity;
            let tvx = aim.cos() * desired;
            let tvy = aim.sin() * desired;
            let ex = tvx - vx;
            let ey = tvy - vy;
            if ex.abs() > ey.abs() {
                if ex > 0.0 {
                    out.right = true; // → pousse vers +x (écran)
                } else {
                    out.left = true; // ← pousse vers -x
                }
            } else if ey > 0.0 {
                out.down = true; // ↓ pousse vers +y (écran)
            } else {
                out.up = true; // ↑ pousse vers -y
            }
        }
        // DIRECTIONAL / INERTIAL / REALISTIC : on oriente le nez vers la cible
        // (→ augmente l'orientation, ← la diminue - tous les modes) puis on
        // pousse quand on est aligné, et on freine au-delà de la vitesse visée
        _ => {
            let err = shortest_angle_delta(player.orientation, aim);
            if err > TURN_DEADBAND {
                out.right = true;
            } else if err < -TURN_DEADBAND {
                out.left = true;
            }
            // vitesse **projetée** sur la direction visée : le module brut ne
            // dit pas si l'on fonce vers la cible ou si l'on s'en éloigne - en
            // INERTIAL/REALISTIC la trajectoire peut être à l'opposé du nez
            // (freiner sur le module faisait **accélérer** la fuite : la marche
            // arrière poussait dans le sens de l'éloignement)
            let vx = player.direction.cos() * player.velocity;
            let vy = -player.direction.sin() * player.velocity;
            let v_along = vx * aim.cos() + vy * aim.sin();
            // freiner **net** quand on est sur la cible : la bande
            // d'hystérésis (`desired + 0,15`) laissait une vitesse résiduelle
            // de 0,15 u/frame (9 u/s) qui faisait **dériver** le vaisseau
            // droit dans sa cible (70 u → contact en ~7 s) ; sous ce seuil de
            // vitesse visée, on freine jusqu'à l'arrêt complet
            let overspeed = v_along > desired + 0.15;
            let settle = desired < 0.4 && v_along > 0.02;
            if err.abs() < THRUST_DEADBAND {
                if v_along < desired - 0.1 {
                    out.up = true;
                } else if overspeed || settle {
                    out.down = true;
                }
            } else if threat.is_some() && threat_ahead && player.velocity > DODGE_BRAKE_SPEED {
                // esquive active avec la menace **devant** : freiner pendant le
                // virage - la marche arrière le long du nez qui tourne réduit
                // la vitesse de fermeture au lieu de la subir de plein fouet
                // (c'est la touche ↓, absente de l'ancienne esquive qui ne
                // faisait que virer et pousser)
                out.down = true;
            } else if state.moving_mode != MOVING_MODE_4_WAYS
                && (player.velocity > desired + 0.15
                    || (desired < 0.4 && player.velocity > 0.02))
            {
                // DIRECTIONAL / INERTIAL / REALISTIC : la vitesse suit la
                // direction de déplacement (le nez vise la cible) - freiner
                // pendant le virage reste sûr quand le module dépasse la
                // visée (le frein aligné ne suffit pas : il n'agit que nez
                // pointé, ce qui laissait le vaisseau **survoler** les cibles
                // à pleine vitesse en REALISTIC). Proche de la cible, on
                // freine jusqu'à l'arrêt (voir `settle` ci-dessus).
                out.down = true;
            }
            // REALISTIC : la rotation vit sa vie après le relâchement - quand
            // on est presque aligné, on contre-commande pour l'arrêter
            if state.moving_mode == MOVING_MODE_REALISTIC
                && player.rotation.abs() > 0.06
                && err.abs() < TURN_DEADBAND
            {
                if player.rotation > 0.0 {
                    out.left = true;
                } else {
                    out.right = true;
                }
            }
        }
    }
    out
}

/// Le **cosmonaute EVA** (vaisseau détruit) piloté par l'ordinateur : son seul
/// objectif est de rentrer à la station pour être secouru - il vise le centre,
/// s'oriente vers lui et pousse (poussée vectorielle, pas de frein - mêmes
/// primitives que le joueur) **en bornant sa vitesse** : la poussée est dosée
/// sur la vitesse d'approche **réelle** (vitesse projetée sur la direction de
/// la station, pas le module brut) - on accélère quand on est en dessous de la
/// visée (qui décroît avec la distance : croisière au loin, ralentissement
/// franc dans l'anneau), on laisse dériver dans la bande d'hystérésis, **on
/// coupe les gaz pendant les réorientations** (la poussée ne s'applique
/// qu'une fois le nez aligné : virer à inertie constante, puis pousser droit
/// devant la nouvelle direction), et on **contre-pousse** pour décélérer -
/// l'équivalent EVA de la marche arrière du vaisseau.
///
/// Deux réglages ont été changés par rapport au vaisseau pour fiabiliser le
/// retour depuis **très loin** (mesurés sur le simulateur, voir
/// `tools/trainer` et `docs/AUTOENTRAINEMENT.md`) :
/// - la **bande d'alignement est serrée** (`EVA_TURN_DEADBAND_*` ≈ 0,9° au
///   lieu des 5,7° du vaisseau) - pousser nez désaligné fait dériver la
///   trajectoire, et sans frottement cette dérive s'amplifie en orbite ;
/// - un **freinage tangentiel** (contre-poussée nez à l'opposé de la
///   **vitesse**, pas de la station) se déclenche quand la composante de
///   vitesse perpendiculaire à la station dépasse `EVA_TANG_BRAKE_HI`, avec
///   hystérésis (`state.eva_tang_braking`, relâché sous `EVA_TANG_BRAKE_LO`) -
///   c'est lui qui casse les orbites que le freinage radial (projection sur la
///   station) ne voit pas.
///
/// Dans la zone d'accostage, on coupe toute poussée : la dérive suffit à
/// déclencher la récupération (`docking` → `start_eva_recovery`, déclenchée à
/// la seule distance). Pas de tir ni d'évitement : le cosmonaute est un
/// non-collider.
pub fn autopilot_eva_inputs(state: &mut GameState, shapes: &[Shape], dt: f64) -> PilotInputs {
    let mut out = PilotInputs::default();
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return out; // cosmonaute EVA absent : rien à piloter
    }
    let c = &shapes[idx];
    let station = &shapes[STATION_INDEX];
    let d = wrapped_distance(c.position, station.position, &state.world);
    // viser le centre de la station (chemin le plus court, monde torique)
    let aim = screen_angle_to(c.position, station.position, state);
    // vitesse d'approche **réelle** : projection de la vitesse (coordonnées
    // écran, y vers le bas) sur la direction de la station - si la trajectoire
    // est à l'opposé du nez, c'est elle qui décide de pousser ou de freiner
    let vx = c.direction.cos() * c.velocity;
    let vy = -c.direction.sin() * c.velocity;
    let v_along = vx * aim.cos() + vy * aim.sin();
    // composante **tangentielle** de la vitesse (perpendiculaire à la
    // direction de la station) : ~0 quand on fonce droit dessus, grande quand
    // on tourne autour - c'est elle qui fait mettre en orbite
    let v_tang = (c.velocity * c.velocity - v_along * v_along).max(0.0).sqrt();
    // vitesse visée : croisière rapide **loin** de la station, puis visée qui
    // décroît linéairement dans l'anneau de ralentissement - au cercle
    // d'accostage elle retombe sur `EVA_ARRIVAL_SPEED` et la dérive déclenche
    // la récupération (elle n'exige que la distance, pas la vitesse)
    let desired = if d < EVA_SLOW_ZONE {
        (EVA_ARRIVAL_SPEED
            + (EVA_MAX_SPEED - EVA_ARRIVAL_SPEED)
                * ((d - STATION_DOCK_DISTANCE) / (EVA_SLOW_ZONE - STATION_DOCK_DISTANCE)))
            .min(EVA_MAX_SPEED)
    } else {
        EVA_MAX_SPEED
    };
    // approche trop rapide **pour pouvoir s'arrêter à temps** : on contre-pousse
    // (nez à l'opposé de la station) quand la vitesse réelle dépasse la vitesse
    // d'arrivée ET qu'il reste moins de distance que le freinage ne demande -
    // demi-tour à vitesse constante (`EVA_TURN_FRAMES`) puis décélération
    // jusqu'à la vitesse d'arrivée. Plus tôt que ce point, on continue de
    // croiser à vitesse rapide ; plus tard, le demi-tour ferait dépasser la zone
    let mut braking = v_along > EVA_ARRIVAL_SPEED + EVA_SPEED_BAND
        && (d - STATION_DOCK_DISTANCE)
            < v_along * EVA_TURN_FRAMES
                + (v_along * v_along - EVA_ARRIVAL_SPEED * EVA_ARRIVAL_SPEED)
                    / (2.0 * crate::config::PLAYER_ACCELERATION);
    // orbite (vitesse surtout tangentielle) : contre-poussée nez à l'opposé
    // de la **vitesse** - hystérésis mémorisée dans l'état pour ne pas
    // alterner poussée/freinage à chaque frame sur la frontière
    if state.eva_tang_braking {
        if v_tang < EVA_TANG_BRAKE_LO {
            state.eva_tang_braking = false; // orbite cassée : on relâche
        }
    } else if v_tang > EVA_TANG_BRAKE_HI {
        state.eva_tang_braking = true;
    }
    if state.eva_tang_braking {
        braking = true;
    }
    // direction de poussée : la station, ou l'opposé - de la vitesse quand on
    // casse une orbite (tuer la composante tangentielle), de la station quand
    // on décélère une approche trop rapide (le sens du retour)
    let thrust_dir = if state.eva_tang_braking {
        std::f64::consts::PI - c.direction
    } else if braking {
        aim + std::f64::consts::PI
    } else {
        aim
    };
    // bande d'alignement adaptative : au moins ~un demi-pas de rotation par
    // frame (sinon la commande tout-ou-rien oscille sans jamais entrer dans la
    // bande à bas FPS), mais jamais plus lâche que le minimum serré
    let turn_db =
        (PLAYER_ROTATION_SPEED * 60.0 * dt * EVA_TURN_DEADBAND_SCALE).max(EVA_TURN_DEADBAND_MIN);
    let err = shortest_angle_delta(c.orientation, thrust_dir);
    if err > turn_db {
        out.right = true;
    } else if err < -turn_db {
        out.left = true;
    }
    // poussée vectorielle (↑ seulement) **uniquement quand on est aligné** -
    // pas de réorientation en cours : pendant un changement de direction, les
    // gaz sont coupés (le virage se fait à inertie constante, puis la poussée
    // reprend droit devant la nouvelle direction - plus précis que de pousser
    // dans le virage). Encore **hors** de la zone d'accostage : dedans, on
    // coupe aussi - la dérive suffit à déclencher la récupération (elle
    // n'exige que la distance, pas la vitesse)
    if err.abs() <= turn_db && d > STATION_DOCK_DISTANCE {
        if braking {
            out.up = true; // contre-poussée : décélère (approche ou orbite)
        } else if v_along < desired - EVA_SPEED_BAND {
            out.up = true; // en dessous de la visée : accélère vers la station
        }
    }
    out
}

// ─── gestion de l'accostage et du ravitaillement ───────────────────────────
/// Ouvre le magasin de la station sur l'onglet ravitaillement, curseurs réglés
/// au **maximum achetable** (même état que le clic SHOP de la boîte DOCK
/// STATION).
fn open_shop(state: &mut GameState) {
    state.dock_box = false;
    state.shop_box = true;
    state.shop_drag = None;
    state.shop_tab = crate::config::SHOP_TAB_SUPPLIES;
    state.shop_feedback.clear();
    state.shop_fuel_qty = scenario::affordable_fuel_qty(state);
    for i in 0..scenario::weapon_slot_count() {
        if scenario::weapon_owned(state, i) {
            state.shop_ammo_qty[i] = scenario::affordable_ammo_qty(state, i) as f64;
        }
    }
}

/// L'ordinateur gère l'accostage (boîte DOCK STATION) quand le pilote
/// automatique est actif : il **décharge la soute** (les crédits financent le
/// ravitaillement), puis - scénario à économie avec des réserves manquantes
/// et payables - **ouvre le magasin** pour se ravitailler ; sinon il referme
/// la boîte et repart. Appelé à chaque frame de la boîte : après le passage au
/// magasin, il ne reste plus rien d'achetable (crédits épuisés) ni de
/// manquant (plein) - l'ordinateur repart donc au lieu de rouvrir le magasin.
pub fn autopilot_handle_dock(state: &mut GameState, elements: &mut [Element]) {
    // déchargement immédiat (comme le bouton UNLOAD) : les crédits financent
    // le ravitaillement de l'accostage courant
    let had_cargo = state.player.cargo_qty > 0;
    scenario::unload_cargo(state, elements);
    for e in elements.iter_mut() {
        e.count = 0;
    }
    state.player.cargo_qty = 0;
    if had_cargo {
        let _ = scenario::save_progression(state);
    }
    if scenario::has_economy(state) && supplies_need_buying(state) {
        open_shop(state);
    } else {
        undock(state);
    }
}

/// L'ordinateur fait ses achats au magasin (scénario à économie) quand le
/// pilote automatique est actif : il achète le **maximum achetable** de
/// carburant puis de munitions (par arme possédée - carburant d'abord, comme
/// le bouton de ravitaillement complet du magasin), persiste les crédits
/// dépensés, puis referme le magasin : la boîte DOCK STATION le fait repartir
/// à la frame suivante (`autopilot_handle_dock` n'a alors plus rien à acheter
/// et détache les liens).
pub fn autopilot_handle_shop(state: &mut GameState) {
    let fuel_qty = scenario::affordable_fuel_qty(state);
    let _ = scenario::buy_fuel_qty(state, fuel_qty);
    for i in 0..scenario::weapon_slot_count() {
        if scenario::weapon_owned(state, i) {
            let qty = scenario::affordable_ammo_qty(state, i);
            let _ = scenario::buy_ammo_qty(state, i, qty);
        }
    }
    let _ = scenario::save_progression(state);
    state.shop_box = false;
    state.dock_box = true;
    state.shop_feedback.clear();
}

/// Départ de la base au lancement / respawn (liens attachés) : l'ordinateur
/// démarre de lui-même - en scénario à économie, il passe d'abord par le
/// magasin si les réserves sont manquantes et payables (ex respawn après une
/// destruction avec réservoir presque vide), sinon il détache les liens
/// directement.
pub fn autopilot_start_depart(state: &mut GameState) {
    if scenario::has_economy(state) && supplies_need_buying(state) {
        open_shop(state);
    } else {
        release_links(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// État + formes minimales : vaisseau à `pos` orienté à `orientation`,
    /// station au centre (0,0) de rayon 162.
    fn scene(pos: Point, orientation: f64) -> (GameState, Vec<Shape>) {
        let state = GameState::new();
        let shapes = vec![
            Shape {
                position: pos,
                orientation,
                ..Default::default()
            },
            Shape {
                position: Point::new(0.0, 0.0),
                radius: 162.0,
                ..Default::default()
            },
        ];
        (state, shapes)
    }

    fn meteor_at(pos: Point) -> Shape {
        Shape {
            position: pos,
            who_i_am: WHOIAM_METEOR,
            is_collider: true,
            life: 1,
            ..Default::default()
        }
    }

    fn mineral_at(pos: Point) -> Shape {
        Shape {
            position: pos,
            who_i_am: WHOIAM_MINERAL,
            is_collider: true,
            life: 1,
            ..Default::default()
        }
    }

    #[test]
    fn hostile_is_a_live_meteor_or_alien() {
        let mut meteor = meteor_at(Point::new(0.0, 0.0));
        assert!(is_hostile(&meteor));
        meteor.life = 0;
        assert!(!is_hostile(&meteor));
        let alien = Shape {
            who_i_am: WHOIAM_ALIEN,
            life: 1,
            ..Default::default()
        };
        assert!(is_hostile(&alien));
        // minerais, portails, station, balles : pas des hostiles
        assert!(!is_hostile(&mineral_at(Point::new(0.0, 0.0))));
        assert!(!is_hostile(&Shape {
            who_i_am: WHOIAM_STATION,
            life: 1,
            ..Default::default()
        }));
        assert!(!is_hostile(&Shape {
            who_i_am: WHOIAM_WARP_GATE,
            life: 1,
            ..Default::default()
        }));
    }

    #[test]
    fn flies_and_fires_at_the_meteor_ahead() {
        // vaisseau à l'ouest orienté vers l'est, météore droit devant : il
        // pousse (aligné, loin) et tire (à portée et aligné)
        let (state, shapes) = scene(Point::new(100.0, 0.0), 0.0);
        let mut shapes = shapes;
        shapes.push(meteor_at(Point::new(200.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.up, "doit pousser vers le météore");
        assert!(inputs.fire, "doit tirer sur le météore aligné à portée");
        assert!(!inputs.down);
    }

    #[test]
    fn turns_toward_an_off_axis_meteor() {
        // météore au nord-est : le nez (orientation 0 = est) doit tourner vers
        // lui (→ augmente l'orientation en DIRECTIONAL)
        let (state, shapes) = scene(Point::new(0.0, 0.0), 0.0);
        let mut shapes = shapes;
        shapes.push(meteor_at(Point::new(100.0, -100.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.left, "doit tourner vers le météore (orientation décroît)");
        assert!(!inputs.right);
        assert!(!inputs.up, "pas aligné : ne pousse pas encore");
    }

    #[test]
    fn protects_the_station_before_mining() {
        // météore loin du vaisseau mais menaçant la station : il est visé en
        // priorité, même si un minerai est plus proche du vaisseau - la
        // mission reste « protéger la station », pas « collecter »
        let (state, shapes) = scene(Point::new(100.0, 0.0), 0.0);
        let mut shapes = shapes;
        shapes.push(meteor_at(Point::new(150.0, 0.0))); // dans le rayon de garde
        shapes.push(mineral_at(Point::new(120.0, 0.0))); // à 30 u de la cible
        let inputs = autopilot_inputs(&state, &shapes);
        assert_eq!(
            debug_current_goal(&state, &shapes),
            "Attack(protège station)",
            "le météore menaçant la station prime sur le minerai proche"
        );
        // déjà à la distance de tir (dans `ATTACK_STANDOFF`) : pas de poussée,
        // et le feu est **retenu** (`HOLD_FIRE_MINERAL_RADIUS`) - le minerai
        // est à moins de 60 u de la cible, une balle traverserait son point
        // d'apparition et le détruirait
        assert!(!inputs.up, "pas de poussée : déjà à portée de tir");
        assert!(!inputs.fire, "feu retenu : minerai à 30 u de la cible");
        // minerai éloigné (hors de la zone de rétention) : le tir reprend
        let last = shapes.len() - 1;
        shapes[last].position = Point::new(2460.0, -1500.0);
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.fire, "zone dégagée : le feu reprend sur le météore menaçant");
    }

    #[test]
    fn collects_a_free_mineral() {
        // aucun hostile : l'autopilote va récupérer le minerai libre
        let (state, shapes) = scene(Point::new(50.0, 0.0), 0.0);
        let mut shapes = shapes;
        shapes.push(mineral_at(Point::new(100.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.up, "doit pousser vers le minerai");
        assert!(!inputs.fire, "rien à tirer");
    }

    #[test]
    fn returns_to_dock_when_cargo_is_full() {
        // soute pleine : priorité au retour, même avec des minerais libres
        let (mut state, shapes) = scene(Point::new(300.0, 0.0), 0.0);
        state.player.cargo_qty = state.player.cargo_size;
        let mut shapes = shapes;
        shapes.push(mineral_at(Point::new(100.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        // nez à l'est, station à l'ouest (angle π) : il doit tourner
        assert!(inputs.right, "doit faire demi-tour vers la station");
        assert!(!inputs.up, "pas encore aligné sur la station");
    }

    #[test]
    fn patrols_near_the_station_when_nothing_to_do() {
        // monde vide : l'autopilote approche la station puis freine (il ne
        // s'accoste pas pour rien)
        let (state, shapes) = scene(Point::new(100.0, 0.0), 0.0);
        let inputs = autopilot_inputs(&state, &shapes);
        // nez à l'est, station à l'ouest : demi-tour (droite) et freinage
        assert!(inputs.right, "doit faire demi-tour vers la station");
        assert!(!inputs.up);
    }

    #[test]
    fn four_ways_steers_with_the_screen_axes() {
        // mode 4 WAYS : la poussée se fait dans les 4 directions de l'écran -
        // pour aller vers l'est (cible à droite), il pousse à droite
        let (mut state, shapes) = scene(Point::new(100.0, 0.0), 0.0);
        state.moving_mode = MOVING_MODE_4_WAYS;
        let mut shapes = shapes;
        shapes.push(meteor_at(Point::new(200.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.right, "doit pousser vers l'est (→)");
        assert!(!inputs.up && !inputs.down && !inputs.left);
        assert!(inputs.fire, "orienté vers la cible : peut tirer");
    }

    #[test]
    fn does_not_fire_at_a_meteor_behind() {
        // météore derrière le vaisseau : pas aligné, pas de tir
        let (state, shapes) = scene(Point::new(0.0, 0.0), 0.0);
        let mut shapes = shapes;
        shapes.push(meteor_at(Point::new(-100.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(!inputs.fire);
    }

    /// Passe l'état en scénario à économie (Progression) avec des crédits et
    /// des réserves réglés (carburant très bas, munitions vides).
    fn economy_state(state: &mut GameState) {
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.credits = 500;
        state.resources.fuel = 1.0; // très bas (capacité de base ≫ 30 %)
    }

    #[test]
    fn returns_to_dock_when_fuel_is_low_in_economy() {
        // scénario à économie, carburant sous le seuil et crédits pour payer :
        // il rentre se ravitailler, même avec un minerai libre en vue
        let (mut state, shapes) = scene(Point::new(300.0, 0.0), 0.0);
        economy_state(&mut state);
        let mut shapes = shapes;
        shapes.push(mineral_at(Point::new(100.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        // nez à l'est, station à l'ouest (angle π) : demi-tour à droite
        assert!(inputs.right, "doit rentrer se ravitailler");
        assert!(!inputs.up, "pas encore aligné sur la station");
    }

    #[test]
    fn returns_to_dock_when_ammo_is_low_in_economy() {
        // réservoir plein mais munitions vides : il rentre se réarmer
        let (mut state, shapes) = scene(Point::new(300.0, 0.0), 0.0);
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.credits = 500;
        state.resources.fuel = scenario::fuel_capacity(&state); // plein
        state.resources.weapon_ammo[0] = 0; // munitions vides
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.right, "doit rentrer se réarmer");
    }

    #[test]
    fn keeps_mining_when_refuel_is_unaffordable() {
        // carburant bas mais aucun crédit : continuer de miner (la soute
        // pleine rapportera de quoi payer) au lieu de l'aller-retour à vide
        let (mut state, shapes) = scene(Point::new(50.0, 0.0), 0.0);
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.fuel = 1.0; // bas
        state.resources.credits = 0; // rien à acheter
        let mut shapes = shapes;
        shapes.push(mineral_at(Point::new(100.0, 0.0)));
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.up, "doit continuer de miner");
        assert!(!inputs.right, "ne doit pas rentrer sans crédits");
    }

    #[test]
    fn dock_routine_refuels_at_the_shop_in_economy() {
        // accosté en scénario à économie : l'ordinateur décharge, ouvre le
        // magasin, achète le maximum achetable, referme puis repart - sans
        // rouvrir le magasin en boucle
        let mut state = GameState::new();
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.fuel = 5.0; // bas
        state.resources.credits = 200;
        state.dock_box = true;
        let mut elements = vec![Element {
            id: 1,
            name: String::new(),
            color: 0,
            count: 0,
        }];
        // à quai : déchargement (rien ici) puis ouverture du magasin
        autopilot_handle_dock(&mut state, &mut elements);
        assert!(!state.dock_box && state.shop_box, "doit ouvrir le magasin");
        // au magasin : achat du maximum achetable puis fermeture
        autopilot_handle_shop(&mut state);
        assert!(!state.shop_box && state.dock_box, "doit refermer le magasin");
        assert!(state.resources.fuel > 5.0, "doit avoir racheté du carburant");
        assert!(state.resources.credits < 200, "doit avoir payé");
        // retour à la boîte : plus rien à acheter (plein ou crédits épuisés) -
        // l'ordinateur repart (rétraction des liens), sans rouvrir le magasin
        autopilot_handle_dock(&mut state, &mut elements);
        assert!(!state.dock_box && !state.shop_box, "doit repartir");
        assert!(state.dock_retract > 0.0, "liens en rétraction");
    }

    #[test]
    fn dock_routine_leaves_directly_outside_economy() {
        // jeu libre : pas de ravitaillement payant - l'ordinateur referme la
        // boîte et repart directement
        let mut state = GameState::new();
        state.dock_box = true;
        let mut elements = vec![Element {
            id: 1,
            name: String::new(),
            color: 0,
            count: 0,
        }];
        autopilot_handle_dock(&mut state, &mut elements);
        assert!(!state.dock_box && !state.shop_box, "doit repartir");
        assert!(state.dock_retract > 0.0);
    }

    #[test]
    fn start_depart_opens_shop_when_supplies_low_at_respawn() {
        // respawn en scénario à économie avec réservoir presque vide et
        // crédits : l'ordinateur passe par le magasin avant de décoller
        let mut state = GameState::new();
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.fuel = 2.0;
        state.resources.credits = 100;
        state.dock_links = true;
        autopilot_start_depart(&mut state);
        assert!(state.shop_box, "doit ouvrir le magasin");
        // réserves pleines : départ direct
        let mut state = GameState::new();
        state.scenario = scenario::ScenarioId::Progression;
        state.resources.fuel = scenario::fuel_capacity(&state);
        state.resources.credits = 0;
        state.dock_links = true;
        autopilot_start_depart(&mut state);
        assert!(!state.shop_box && state.dock_retract > 0.0, "doit partir");
    }

    #[test]
    fn thrusts_toward_target_when_drifting_away_in_inertial() {
        // INERTIAL : le vaisseau fonce vers l'est (direction 0) à grande
        // vitesse alors que la cible (la station) est à l'ouest et que le nez
        // est déjà pointé dessus (orientation π). L'ancienne commande freinait
        // sur le module (vitesse > visée → marche arrière) : en poussée
        // vectorielle, la marche arrière poussait dans le sens de la fuite et
        // faisait **accélérer** le vaisseau à l'opposé de la cible. La nouvelle
        // commande projette le sens d'avancement réel sur la cible :
        // v_along = −4 → poussée avant pour revenir vers la station.
        let (mut state, mut shapes) = scene(Point::new(300.0, 0.0), std::f64::consts::PI);
        state.moving_mode = MOVING_MODE_INERTIAL;
        shapes[0].direction = 0.0; // avance vers l'est (loin de la station)
        shapes[0].velocity = 4.0;
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.up, "doit pousser vers la cible (freiner la fuite)");
        assert!(!inputs.down, "la marche arrière accélérerait la fuite");
    }

    #[test]
    fn brakes_when_flying_toward_target_too_fast_in_inertial() {
        // INERTIAL : fonce vers la cible (l'ouest, orientation = direction =
        // π) à 4 u/frame alors que la visée est 1,8 : marche arrière (frein
        // vectoriel), pas de poussée avant
        let (mut state, mut shapes) = scene(Point::new(300.0, 0.0), std::f64::consts::PI);
        state.moving_mode = MOVING_MODE_INERTIAL;
        shapes[0].direction = std::f64::consts::PI;
        shapes[0].velocity = 4.0;
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.down, "doit freiner");
        assert!(!inputs.up);
    }

    #[test]
    fn dodges_a_meteor_on_collision_course_while_attacking() {
        // Attaque d'un météore droit devant (A, cible la plus proche) pendant
        // qu'un autre météore (B, plus loin mais plus rapide) file vers le
        // vaisseau : l'esquive remplace la conduite (le vaisseau tourne au lieu
        // de foncer) mais le tir continue sur la cible
        let (state, mut shapes) = scene(Point::new(400.0, 0.0), 0.0);
        shapes[0].velocity = 1.8; // croisière vers l'est
        let a = meteor_at(Point::new(490.0, 0.0));
        let mut b = meteor_at(Point::new(520.0, -40.0));
        b.direction = std::f64::consts::PI; // file vers l'ouest (le vaisseau)
        b.velocity = 2.0;
        shapes.push(a);
        shapes.push(b);
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(
            inputs.right || inputs.left,
            "doit esquiver au lieu de foncer droit sur la cible"
        );
        assert!(!inputs.up, "pas aligné sur l'esquive : ne pousse pas encore");
        assert!(inputs.fire, "continue de tirer sur la cible en esquivant");
    }

    #[test]
    fn holds_position_at_standoff_without_dodging_the_target() {
        // à distance de tir de la cible (70 = standoff) : la cible n'est pas
        // une menace - le vaisseau tient sa position et tire, sans esquiver
        let (state, mut shapes) = scene(Point::new(400.0, 0.0), 0.0);
        shapes.push(meteor_at(Point::new(470.0, 0.0))); // 70 devant
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.fire, "doit tirer sur la cible à portée");
        assert!(!inputs.up && !inputs.down, "à l'arrêt au standoff");
        assert!(!inputs.right && !inputs.left, "aligné : ne tourne pas");
    }

    #[test]
    fn backs_away_when_the_attack_target_closes_in() {
        // la cible d'attaque referme la distance sous le standoff (elle dérive
        // vers le vaisseau) : elle redevient une menace - le vaisseau s'écarte
        // latéralement tout en continuant de tirer
        let (state, mut shapes) = scene(Point::new(400.0, 0.0), 0.0);
        shapes.push(meteor_at(Point::new(460.0, 0.0))); // 60 devant (< standoff)
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.right || inputs.left, "doit s'écarter de la cible qui ferme");
        assert!(inputs.fire, "continue de tirer");
    }

    #[test]
    fn brakes_while_dodging_a_head_on_threat() {
        // menace de face (la cible d'attaque referme la distance sous le
        // standoff) et vaisseau en mouvement : l'esquive vire **et freine** (↓)
        // - la marche arrière le long du nez qui tourne réduit la vitesse de
        // fermeture au lieu de foncer dedans - tout en continuant de tirer
        let (state, mut shapes) = scene(Point::new(400.0, 0.0), 0.0);
        shapes[0].velocity = 1.5; // fonce vers l'est
        shapes.push(meteor_at(Point::new(440.0, 0.0))); // 40 devant (< standoff)
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.down, "doit freiner pendant l'esquive");
        assert!(inputs.right, "doit virer latéralement (esquive)");
        assert!(inputs.fire, "continue de tirer sur la menace");
    }

    #[test]
    fn does_not_brake_when_the_threat_is_behind() {
        // menace par l'arrière (un météore rattrape le vaisseau) : l'esquive
        // vire mais **ne freine pas** - freiner laisserait le rattrapeur
        // gagner du terrain
        let (state, mut shapes) = scene(Point::new(400.0, 0.0), 0.0);
        shapes[0].velocity = 0.6; // avance lentement vers l'est
        let mut m = meteor_at(Point::new(380.0, 0.0)); // derrière
        m.direction = 0.0; // file vers l'est aussi, plus vite (rattrapage)
        m.velocity = 2.0;
        shapes.push(m);
        let inputs = autopilot_inputs(&state, &shapes);
        assert!(inputs.left, "doit virer pour s'écarter");
        assert!(!inputs.down, "ne doit pas freiner (le rattrapeur gagnerait)");
    }

    /// Scène EVA : vaisseau détruit (index 0, non piloté), station au centre,
    /// cosmonaute à `pos` orienté à `orientation` (index 2).
    fn eva_scene(pos: Point, orientation: f64) -> (GameState, Vec<Shape>) {
        let mut state = GameState::new();
        state.cosmonaut_active = true;
        state.eva_cosmonaut = 2;
        let shapes = vec![
            Shape::default(), // vaisseau détruit (ignoré)
            Shape {
                position: Point::new(0.0, 0.0),
                radius: 162.0,
                ..Default::default()
            },
            Shape {
                position: pos,
                orientation,
                ..Default::default()
            },
        ];
        (state, shapes)
    }

    /// dt de test : 1/60 s (comme le simulateur de `tools/trainer`).
    const DT: f64 = 1.0 / 60.0;

    #[test]
    fn eva_turns_toward_the_station() {
        // cosmonaute à l'est orienté vers l'est : demi-tour vers la station
        let (mut state, shapes) = eva_scene(Point::new(300.0, 0.0), 0.0);
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.right, "doit tourner vers la station (à l'ouest)");
        assert!(!inputs.up, "pas encore aligné : ne pousse pas");
        assert!(!inputs.fire, "le cosmonaute n'a pas d'arme");
    }

    #[test]
    fn eva_thrusts_when_aligned_and_far() {
        // aligné sur la station et hors de la zone d'accostage : poussée
        let (mut state, shapes) = eva_scene(Point::new(300.0, 0.0), std::f64::consts::PI);
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "doit pousser vers la station");
        assert!(!inputs.down, "pas de frein en EVA");
        assert!(!inputs.fire);
    }

    #[test]
    fn eva_cuts_thrust_inside_the_docking_zone() {
        // dans la zone d'accostage (rayon STATION_DOCK_DISTANCE) : on coupe la
        // poussée - la dérive suffit à déclencher la récupération
        let (mut state, shapes) = eva_scene(Point::new(10.0, 0.0), std::f64::consts::PI);
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(!inputs.up, "doit couper la poussée dans la zone");
    }

    #[test]
    fn eva_returns_from_the_west() {
        // à l'ouest, déjà orienté vers l'est (le centre) : poussée directe
        let (mut state, shapes) = eva_scene(Point::new(-300.0, 0.0), 0.0);
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "doit pousser vers la station");
    }

    #[test]
    fn eva_coasts_at_the_speed_cap_without_overthrusting() {
        // à la vitesse maximale visée (EVA_MAX_SPEED) vers la station : ni
        // poussée ni frein - il laisse dériver (la poussée accélérerait au-delà
        // de la limite, le frein n'est pas nécessaire)
        let (mut state, mut shapes) = eva_scene(Point::new(300.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI; // avance vers la station
        shapes[2].velocity = EVA_MAX_SPEED;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(!inputs.up, "ne doit pas accélérer au-delà de la limite");
        assert!(!inputs.down, "pas de frein en EVA");
        assert!(!inputs.left && !inputs.right, "aligné : ne tourne pas");
    }

    #[test]
    fn eva_turns_around_to_brake_when_approaching_too_fast() {
        // approche trop rapide (vitesse > visée + bande) : il fait demi-tour
        // (nez à l'opposé de la station) pour pouvoir contre-pousser -
        // l'équivalent EVA de la marche arrière
        let (mut state, mut shapes) = eva_scene(Point::new(150.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI; // fonce vers la station
        shapes[2].velocity = 1.8; // bien au-dessus de la visée + bande
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(
            inputs.left || inputs.right,
            "doit faire demi-tour pour freiner (demi-tour = π, un sens ou l'autre)"
        );
        assert!(!inputs.up, "pas encore aligné sur l'opposé : ne pousse pas");
    }

    #[test]
    fn eva_counter_thrusts_when_braking() {
        // déjà nez à l'opposé de la station et approche trop rapide : la
        // contre-poussée (↑) décélère l'approche
        let (mut state, mut shapes) = eva_scene(Point::new(150.0, 0.0), 0.0); // face à l'est
        shapes[2].direction = std::f64::consts::PI; // mais avance vers l'ouest
        shapes[2].velocity = 1.8;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "doit contre-pousser pour décélérer");
        assert!(!inputs.left && !inputs.right, "aligné sur l'opposé : ne tourne pas");
    }

    #[test]
    fn eva_slows_down_inside_the_slow_zone() {
        // dans l'anneau de ralentissement (visée < vitesse de croisière) : la
        // poussée est coupée, puis le frein se déclenche quand la visée tombe
        // sous la vitesse courante - le retour ralentit à l'approche
        let (mut state, mut shapes) = eva_scene(Point::new(20.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI; // fonce vers la station
        shapes[2].velocity = 1.8; // bien au-dessus de la visée (≈ 0,68) + bande
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(
            inputs.left || inputs.right,
            "doit se retourner pour freiner à l'approche"
        );
        assert!(!inputs.up, "ne doit pas continuer de pousser vers la station");
    }

    #[test]
    fn eva_cuts_thrust_while_reorienting() {
        // légèrement désaligné (écart de 0,12 rad, bien au-delà de la bande
        // d'alignement serrée EVA) : le cosmonaute tourne pour corriger **les
        // gaz coupés** - la poussée n'est pas maintenue pendant la
        // réorientation (elle ne revient qu'une fois le nez aligné)
        let (mut state, shapes) = eva_scene(Point::new(300.0, 0.0), std::f64::consts::PI + 0.12);
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.left, "doit corriger sa direction (écart de 0,12 rad)");
        assert!(!inputs.up, "les gaz sont coupés pendant la réorientation");
    }

    #[test]
    fn eva_cruises_fast_far_from_the_station() {
        // hors de l'anneau de ralentissement (d = 300 > EVA_SLOW_ZONE) : la
        // visée vaut EVA_MAX_SPEED (1,5) et la bande est étroite - la croisière
        // effective vaut ≈ visée − bande ≈ 1,35 u/frame (81 u/s), bien au-dessus
        // de l'ancienne dérive (~20 u/s) : on accélère jusqu'à elle puis on
        // laisse dériver, sans frein
        let (mut state, mut shapes) = eva_scene(Point::new(300.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI; // avance vers la station
        shapes[2].velocity = 1.3; // en dessous de la croisière effective
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "doit accélérer jusqu'à la croisière rapide");
        assert!(!inputs.left && !inputs.right, "aligné : ne tourne pas");
        shapes[2].velocity = 1.4; // au-dessus de visée − bande : croisière atteinte
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(!inputs.up, "croisière atteinte : laisse dériver");
        assert!(!inputs.left && !inputs.right, "pas de frein ici : trop loin pour freiner");
    }

    #[test]
    fn eva_brakes_only_when_the_distance_requires_it() {
        // le freinage est **anticipé par la distance** : même vitesse (1,2) et
        // même direction, c'est la distance restante qui décide - à 230 u (dans
        // l'anneau mais loin), il reste assez de place pour freiner plus tard :
        // on continue de pousser vers la station ; à 150 u, la distance restante
        // passe sous le besoin (demi-tour à vitesse constante + décélération) et
        // le demi-tour de freinage se déclenche
        let (mut state, mut shapes) = eva_scene(Point::new(230.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI;
        shapes[2].velocity = 1.2;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "loin de la zone : continue de pousser vers la station");
        assert!(!inputs.left && !inputs.right, "pas encore besoin de freiner");
        let (mut state, mut shapes) = eva_scene(Point::new(150.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI;
        shapes[2].velocity = 1.2;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(
            inputs.left || inputs.right,
            "plus assez de distance pour freiner plus tard : demi-tour maintenant"
        );
        assert!(!inputs.up, "pas encore aligné sur l'opposé : ne pousse pas");
    }

    #[test]
    fn eva_tracks_the_ramp_down_to_arrival_speed() {
        // dans l'anneau (d = 100 < EVA_SLOW_ZONE), lent : la visée rampe de la
        // croisière vers EVA_ARRIVAL_SPEED (0,5) - on pousse pour rejoindre la
        // visée locale (≈ 0,88 − bande ≈ 0,73), et la vitesse d'arrivée visée
        // reste basse pour que la récupération par câble n'ait rien à chasser
        let (mut state, mut shapes) = eva_scene(Point::new(100.0, 0.0), std::f64::consts::PI);
        shapes[2].direction = std::f64::consts::PI;
        shapes[2].velocity = 0.6;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(inputs.up, "doit accélérer vers la visée locale de l'anneau");
        assert!(!inputs.left && !inputs.right, "aligné : ne tourne pas");
    }

    #[test]
    fn eva_brakes_tangentially_when_orbiting() {
        // cosmonaute en **orbite** autour de la station : sa vitesse est
        // presque perpendiculaire à la direction de la base (composante
        // tangentielle > EVA_TANG_BRAKE_HI) alors que la projection radiale
        // est quasi nulle - le freinage radial classique ne voit rien, le
        // freinage tangentiel doit s'enclencher et viser l'**opposé de la
        // vitesse** (π − direction)
        // cosmonaute au nord (0, 300) : la station est « au-dessus » (écran) -
        // une vitesse plein est (direction 0) est donc **tangentielle** à
        // l'orbite (perpendiculaire à la direction de la station)
        let (mut state, mut shapes) = eva_scene(Point::new(0.0, 300.0), 0.0);
        shapes[2].direction = 0.0; // vitesse tangentielle (plein est)
        shapes[2].velocity = 1.0;
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(state.eva_tang_braking, "doit verrouiller le freinage tangentiel");
        // nez à l'opposé de la vitesse (π − direction = π/2) : pas encore aligné
        assert!(
            inputs.left || inputs.right,
            "doit tourner vers l'opposé de la vitesse"
        );
        assert!(!inputs.up, "pas encore aligné sur l'opposé : ne pousse pas");
    }

    #[test]
    fn eva_releases_tangential_braking_below_the_low_threshold() {
        // orbite cassée : la composante tangentielle est retombée sous
        // EVA_TANG_BRAKE_LO - le verrou se relâche et la conduite normale
        // (viser la station) reprend
        let (mut state, mut shapes) = eva_scene(Point::new(0.0, 300.0), std::f64::consts::PI / 2.0);
        shapes[2].direction = std::f64::consts::PI / 2.0; // vitesse tangentielle faible
        shapes[2].velocity = 0.1; // tangentielle ≈ 0,1 < EVA_TANG_BRAKE_LO
        state.eva_tang_braking = true; // verrou posé par une frame précédente
        let inputs = autopilot_eva_inputs(&mut state, &shapes, DT);
        assert!(!state.eva_tang_braking, "doit relâcher le freinage tangentiel");
        assert!(!inputs.up, "vitesse très basse : ne pousse pas encore");
    }

    #[test]
    fn eva_returns_from_long_range_without_orbiting() {
        // retour depuis **très loin** (800 u, là où l'ancienne commande mettait
        // le cosmonaute en orbite) : simulé en boucle tête nue (physique EVA
        // exacte, même pas de temps 1/60 que le jeu) jusqu'à la récupération -
        // le cosmonaute doit entrer dans le cercle d'accostage sans dérive ni
        // vitesse d'entrée excessive
        for (spawn, timeout) in [(300.0, 40.0), (800.0, 90.0), (1500.0, 150.0)] {
            let (mut state, mut shapes) = eva_scene(Point::new(0.0, spawn), 0.0);
            let dt = 1.0 / 60.0;
            let mut max_speed: f64 = 0.0;
            let mut recovered = false;
            for _ in 0..(timeout * 60.0) as usize {
                let c = &shapes[2];
                if wrapped_distance(
                    c.position,
                    shapes[STATION_INDEX].position,
                    &state.world,
                ) < crate::config::STATION_DOCK_DISTANCE
                {
                    recovered = true;
                    break;
                }
                max_speed = max_speed.max(c.velocity);
                let pilot = autopilot_eva_inputs(&mut state, &shapes, dt);
                // mêmes formules que `input::cosmonaut_apply_inputs` +
                // `shape::moving_shape` (EVA : pas de frein, poussée ↑ le long
                // de l'orientation)
                let orientation = shapes[2].orientation;
                if pilot.up {
                    crate::input::thrust_vector(
                        &mut shapes[2],
                        crate::config::PLAYER_ACCELERATION * 60.0 * dt,
                        orientation,
                        1.0,
                        -1.0,
                    );
                }
                if pilot.right {
                    shapes[2].orientation +=
                        crate::config::PLAYER_ROTATION_SPEED * 60.0 * dt;
                }
                if pilot.left {
                    shapes[2].orientation -=
                        crate::config::PLAYER_ROTATION_SPEED * 60.0 * dt;
                }
                // un triangle suffit pour `moving_shape` (la forme n'a aucune
                // géométrie à recalculer : on ne vérifie que la trajectoire)
                let mut triangles = vec![crate::geom::Triangle::default()];
                crate::shape::moving_shape(&mut shapes[2], &mut triangles, &state.world, dt);
            }
            assert!(
                recovered,
                "spawn {spawn} : le cosmonaute doit rentrer à la station sans orbiter"
            );
            assert!(
                max_speed <= EVA_MAX_SPEED + 1e-9,
                "spawn {spawn} : vitesse max {max_speed:.2} u/frame - le retour doit rester borné"
            );
        }
    }
}