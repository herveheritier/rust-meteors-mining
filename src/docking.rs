//! Accostage de la station : détection du retour, animation
//! d'accostage, boîte DOCK STATION, rétraction des liens néon et
//! guide d'accostage (mire) - portage de `src/game.rs`.

use macroquad::prelude::*;
use crate::audio::Sounds;
use crate::config::*;
use crate::geom::{Point, Triangle, World};
use crate::render::{choice_box_layout, mouse_to_game};
use crate::scenario;
use crate::shape::{compute_real_positions, Shape};
use crate::state::{DockHint, Element, GameState};
use crate::eva::start_eva_recovery;

/// Texte de l'aide « trop rapide » envoyée au pilote et affichée en
/// **clignotant rouge** au-dessus du vaisseau pendant le retour à la base
/// (voir `render::draw_dock_approach_message` et `docking`).
pub fn dock_slow_down_text() -> String {
    format!("DOCK: SLOW DOWN - MAX SPEED {:.1}", STATION_DOCK_SPEED)
}

/// Texte de l'aide « dans la zone » envoyée au pilote et affichée en
/// **clignotant vert** au-dessus du vaisseau quand l'approche est bonne
/// (quasi immobile, ou vaisseau capturé pendant l'animation d'accostage -
/// voir `render::draw_dock_approach_message`).
pub fn dock_in_range_text() -> &'static str {
    "DOCK: IN RANGE - CUT THRUST TO DOCK"
}

/// Détecte le retour à la base (ex « detect return to the base » de
/// `mainLoop`) : le vaisseau est docké quand son centre entre dans la zone
/// d'accostage - le cercle de rayon `STATION_DOCK_DISTANCE` autour du centre
/// de la station (vérification circulaire, comme la mire affichée à l'écran)
/// **et** qu'il est presque immobile (`STATION_DOCK_SPEED`) : il faut ralentir
/// pour terminer l'accostage (la mire passe du rouge au vert avec la qualité
/// de l'approche).
///
/// NB : comme l'original, le choix UNLOAD/CLOSE de la boîte était ignoré
/// (`r%` non utilisé) - le cargo reste vidé de toute façon à l'accostage
/// (au plus tard à la frame suivant la fermeture de la boîte ; le bouton
/// UNLOAD de la boîte le vide immédiatement). Le ravitaillement (carburant +
/// munitions), lui, n'est plus automatique : il s'achète indépendamment au
/// magasin (section RAVITAILLEMENT).
///
/// Messages d'aide au pilote : lors du **retour à la base** (guide
/// d'accostage actif, `state.docking_guide` - le vaisseau est **dans le
/// rayon de la base**, ~162), un message est envoyé à chaque **changement**
/// de situation (`state.dock_hint`), la vitesse étant jugée sur **tout le
/// rayon de la base** (comme la mire, rouge→vert) et non seulement dans le
/// petit cercle d'accostage au centre : « DOCK: SLOW DOWN » dès que le
/// vaisseau franchit l'anneau trop vite, « DOCK: IN RANGE » dès qu'il est
/// assez lent, « DOCK: ZONE LEFT » s'il ressort de la base sans accoster.
/// Hors retour (vol libre, à quai, animation d'accostage), aucune aide
/// n'est envoyée.
pub fn docking(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    elements: &mut [Element],
) {
    // vaisseau détruit : le cosmonaute EVA rejoint la base - dès qu'il atteint
    // la zone d'accostage (cercle de rayon `STATION_DOCK_DISTANCE` au centre,
    // la station est en (0,0)), la **récupération** démarre : un cordon
    // jaillit de l'anneau et le ramène sur l'anneau, puis le fondu enchaîné
    // fait apparaître le vaisseau reconstruit (le monde continue de tourner -
    // la suite est traitée en tête de `update` : `advance_eva_recovery` puis
    // `advance_eva_crossfade`, qui termine par `rescue_cosmonaut`)
    if state.cosmonaut_active {
        let c = &shapes[state.eva_cosmonaut as usize];
        if state.eva_recovery <= 0.0
            && crate::geom::wrapped_distance(c.position, shapes[STATION_INDEX].position, &state.world)
                < STATION_DOCK_DISTANCE
        {
            start_eva_recovery(state, shapes, triangles);
        }
        return;
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let delta = crate::geom::wrapped_delta(
        shapes[PLAYER_INDEX].position,
        shapes[STATION_INDEX].position,
        &state.world,
    );
    let in_zone = delta.x * delta.x + delta.y * delta.y < STATION_DOCK_DISTANCE * STATION_DOCK_DISTANCE;
    // messages d'aide au pilote : uniquement lors du RETOUR à la base (guide
    // d'accostage actif - le vaisseau est DANS le rayon de la base, ~162) et
    // au changement de situation (front montant - pas un message par frame).
    // La vitesse est jugée sur TOUT le rayon de la base (comme la mire,
    // rouge→vert), pas seulement dans le petit cercle d'accostage au centre :
    // « SLOW DOWN » dès qu'on franchit l'anneau trop vite, « IN RANGE » dès
    // qu'on est assez lent, « ZONE LEFT » si on ressort sans accoster
    let dock_held = state.dock_anim > 0.0
        || state.dock_box
        || state.shop_box
        || state.dock_retract > 0.0
        || state.dock_links;
    if !dock_held && state.docking_guide {
        let situation = if shapes[PLAYER_INDEX].velocity.abs() < STATION_DOCK_SPEED {
            DockHint::InRange
        } else {
            DockHint::TooFast
        };
        if situation != state.dock_hint {
            match situation {
                DockHint::TooFast => state.send_message(&dock_slow_down_text()),
                DockHint::InRange => {
                    state.send_message(dock_in_range_text());
                }
                _ => {}
            }
            state.dock_hint = situation;
        }
    } else if !dock_held
        && state.dock_was_outside
        && matches!(state.dock_hint, DockHint::TooFast | DockHint::InRange)
    {
        // le guide vient d'être coupé parce que le vaisseau est RESSORTI de
        // la base (limite extérieure franchie en sortant) sans avoir accosté
        state.send_message("DOCK: ZONE LEFT - HEAD BACK TO THE STATION");
        state.dock_hint = DockHint::Docked;
    } else if state.dock_hint != DockHint::Docked {
        // vaisseau tenu par la station (accostage, liens, boîtes) ou guide
        // coupé (vol libre…) : on repart d'un état propre pour le prochain
        // retour
        state.dock_hint = DockHint::Docked;
    }
    // l'accostage se termine seulement si le vaisseau est presque immobile
    if in_zone && shapes[PLAYER_INDEX].velocity.abs() < STATION_DOCK_SPEED {
        if state.player_at_station == 0 {
            state.player_at_station = -1;
            state.player_enter_station = -1;
            shapes[PLAYER_INDEX].velocity = 0.0;
            // animation d'accostage (3 s) avant la boîte DOCK STATION : le
            // vaisseau pivote vers la droite et se recentre au centre - la
            // boîte s'ouvre à la fin (`advance_dock_animation`)
            state.dock_anim = DOCK_ANIMATION_DURATION;
            state.dock_anim_from_pos = shapes[PLAYER_INDEX].position;
            state.dock_anim_from_orient = shapes[PLAYER_INDEX].orientation;
            // l'accostage démarre : le guide est coupé - il ne réapparaîtra
            // qu'à un prochain retour (et pas pendant qu'on quitte l'accostage)
            state.docking_guide = false;
            // compteur d'accostages (objectifs DAG) + journal de bord
            state.docking_count += 1;
            state.log_event("ACCOSTAGE À LA STATION");
        } else {
            // déchargement : la soute est convertie en crédits (scénario à
            // économie) puis vidée - le ravitaillement s'achète au magasin
            // (section RAVITAILLEMENT)
            let had_cargo = state.player.cargo_qty > 0;
            scenario::unload_cargo(state, elements);
            for e in elements.iter_mut() {
                e.count = 0;
            }
            state.player.cargo_qty = 0;
            state.player_enter_station = 0;
            state.player_at_station = -1;
            // la progression (crédits) n'est persistée que s'il y avait du
            // cargo (cette branche tourne à chaque frame à quai - pas
            // d'écriture du fichier de config à chaque frame)
            if had_cargo {
                let _ = scenario::save_progression(state);
            }
        }
    } else {
        if state.player_at_station == -1 {
            // NB : typo de l'original (« LIVING » pour « LEAVING ») conservée
            state.send_message("YOU ARE LIVING THE STATION");
        }
        state.player_at_station = 0;
    }
}

/// Fait avancer l'animation d'accostage d'une frame : le vaisseau pivote
/// vers la droite (orientation 0) tout en se recentrant **exactement** au
/// centre de la station (position 0,0), avec une interpolation lissée
/// (smoothstep) sur `DOCK_ANIMATION_DURATION`. À la fin de l'animation, la
/// boîte DOCK STATION s'ouvre et le message d'accostage est envoyé (comme
/// avant, mais repoussé après l'animation).
///
/// Le monde, lui, continue de tourner pendant l'animation (appelé par
/// `update`, qui fait avancer `collisions` juste après - le vaisseau qui
/// s'aligne est protégé) ; le trait d'accostage est dessiné par
/// `render::draw_docking_line`.
pub fn advance_dock_animation(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.dock_anim = (state.dock_anim - dt).max(0.0);
    // avancement 0..1 avec lissage (smoothstep) pour un mouvement fluide
    let t = (1.0 - state.dock_anim / DOCK_ANIMATION_DURATION).clamp(0.0, 1.0);
    let e = t * t * (3.0 - 2.0 * t);
    let p = &mut shapes[PLAYER_INDEX];
    // recentrage exact sur le centre de la station (0,0)
    p.position.x = state.dock_anim_from_pos.x + (0.0 - state.dock_anim_from_pos.x) * e;
    p.position.y = state.dock_anim_from_pos.y + (0.0 - state.dock_anim_from_pos.y) * e;
    // pivot vers la droite : orientation 0, par le chemin le plus court
    let delta = shortest_angle_delta(state.dock_anim_from_orient, 0.0);
    p.orientation = state.dock_anim_from_orient + delta * e;
    // vitesse nulle (le vaisseau est immobilisé pendant l'accostage)
    p.velocity = 0.0;
    p.rotation = 0.0;
    // recalcule les positions réelles des triangles du vaisseau
    for t in &mut triangles[p.first_triangle..=p.last_triangle] {
        compute_real_positions(t, p.position, p.center, p.orientation);
    }
    if state.dock_anim <= 0.0 {
        state.dock_anim = 0.0;
        state.send_message("YOU ARE DOCKED AT THE STATION");
        state.dock_box = true; // ouvre la boîte DOCK STATION (monde vivant)
    }
}

/// Le vaisseau quitte l'accostage (bouton CLOSE de la boîte DOCK STATION) :
/// ferme la boîte puis libère le vaisseau (rétraction des liens).
pub fn undock(state: &mut GameState) {
    state.dock_box = false;
    release_links(state);
}

/// Libère le vaisseau : détache les liens (s'ils étaient attachés à quai,
/// lancement/respawn) et démarre la **rétraction des liens** - le vaisseau
/// reste au centre de la station, les 4 traits néon se rétractent vers le
/// bord intérieur de l'anneau pendant `DOCK_RETRACT_DURATION` (le monde
/// continue de tourner, voir `advance_dock_retract`), puis il est libre. En
/// quittant la base, le **guide d'accostage est coupé** : la mire ne
/// réapparaîtra qu'au retour (franchissement de la limite extérieure en
/// entrant).
pub fn release_links(state: &mut GameState) {
    state.dock_links = false;
    state.docking_guide = false;
    state.dock_retract = DOCK_RETRACT_DURATION;
}

/// Met à jour le **guide d'accostage** (la mire au centre de la station) :
/// il ne s'affiche **que lors du retour à la base** - le vaisseau doit avoir
/// quitté la base (franchi la limite extérieure en sortant, `dock_was_outside`
/// repassé à vrai) puis la **recroiser en entrant** (front montant : la
/// distance passe de ≥ au rayon à < au rayon). Il ne s'affiche donc jamais
/// pendant que le vaisseau quitte l'accostage ni à quai (les états « tenu »
/// masquent de toute façon la mire dans `render::docking_marker_visible`).
pub fn update_docking_guide(
    state: &mut GameState,
    player_position: Point,
    station_position: Point,
    station_radius: f64,
) {
    // vaisseau détruit : le cosmonaute éjecté doit TOUJOURS voir la mire -
    // elle le guide vers la base (le « retour » classique ne s'applique pas)
    if state.cosmonaut_active {
        state.docking_guide = true;
        state.dock_was_outside = true;
        return;
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let dist = crate::geom::wrapped_distance(player_position, station_position, &state.world);
    let outside = dist >= station_radius;
    if outside {
        state.docking_guide = false;
    } else if state.dock_was_outside {
        // vient de franchir la limite extérieure de la base en entrant :
        // c'est le retour - le guide s'affiche
        state.docking_guide = true;
    }
    state.dock_was_outside = outside;
}

/// Fait avancer la rétraction des liens d'accostage d'une frame : le vaisseau
/// reste immobilisé exactement au centre de la station (position 0,0,
/// orientation 0) pendant `DOCK_RETRACT_DURATION` - les liens se rétractent
/// visuellement (voir `render::draw_docking_line`). À la fin, le vaisseau est
/// libre (le monde se dégèle, `docking` peut le faire repartir).
///
/// Le monde, lui, continue de tourner pendant la rétraction (appelé par
/// `update`, qui fait avancer `collisions` juste après - le vaisseau tenu au
/// centre est protégé).
pub fn advance_dock_retract(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.dock_retract = (state.dock_retract - dt).max(0.0);
    let p = &mut shapes[PLAYER_INDEX];
    // le vaisseau reste exactement au centre, pointant vers la droite
    p.position.x = 0.0;
    p.position.y = 0.0;
    p.orientation = 0.0;
    p.velocity = 0.0;
    p.rotation = 0.0;
    // recalcule les positions réelles des triangles du vaisseau
    for t in &mut triangles[p.first_triangle..=p.last_triangle] {
        compute_real_positions(t, p.position, p.center, p.orientation);
    }
    if state.dock_retract <= 0.0 {
        state.dock_retract = 0.0;
    }
}

/// Écart angulaire le plus court (radians, dans ]-π, π]) entre deux angles,
/// pour pivoter vers la droite (orientation 0) sans faire un tour complet.
pub fn shortest_angle_delta(from: f64, to: f64) -> f64 {
    let mut d = (to - from) % TAU;
    if d > std::f64::consts::PI {
        d -= TAU;
    }
    if d < -std::f64::consts::PI {
        d += TAU;
    }
    d
}

/// Supprime les balles sorties de la zone de dessin (ex « deletes bullets
/// outer of draw area » de `mainLoop`) : forme et triangles tués, compteur
/// `bullets_lost` incrémenté par triangle.
pub fn delete_out_of_range_bullets(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    camera: Point,
) {
    for i in 0..shapes.len() {
        if shapes[i].life <= 0 {
            continue;
        }
        if shapes[i].who_i_am == WHOIAM_BULLET {
            let mut pt = Point::new(shapes[i].position.x + camera.x, shapes[i].position.y + camera.y);
            pt.normalize_world(&state.world);
            if pt.x < DRAW_MINX || pt.x > DRAW_MAXX || pt.y < DRAW_MINY || pt.y > DRAW_MAXY {
                shapes[i].life = 0;
                for t in &mut triangles[shapes[i].first_triangle..=shapes[i].last_triangle] {
                    t.life = 0;
                    state.bullets_lost += 1;
                }
            }
        }
    }
}

/// Bouton cliqué sur la boîte de choix DOCK STATION (accostage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceClick {
    None,
    /// Décharge la soute (crédits disponibles pour le ravitaillement).
    Unload,
    /// Ouvre le magasin de la station (carburant, munitions, armes,
    /// extensions et modes de déplacement en scénario à économie).
    Shop,
    /// Ferme la boîte.
    Close,
}

/// Détecte un clic sur la boîte de choix DOCK STATION (ex
/// `windowUtils_choiceBox`) et renvoie le bouton cliqué (contrairement à
/// l'original, le choix n'est plus ignoré : UNLOAD et SHOP agissent). SHOP
/// ouvre le magasin de la station (le carburant et les munitions s'y
/// achètent indépendamment).
pub fn choice_box_click() -> ChoiceClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return ChoiceClick::None;
    }
    let l = choice_box_layout();
    let m = mouse_to_game();
    if l.unload.contains(m) {
        ChoiceClick::Unload
    } else if l.shop.contains(m) {
        ChoiceClick::Shop
    } else if l.close.contains(m) {
        ChoiceClick::Close
    } else {
        ChoiceClick::None
    }
}

/// Les messages d'approche (et leurs bips) sont-ils **actifs** ? Oui quand le
/// vaisseau **revient à la base** : guide d'accostage allumé (`docking_guide`,
/// il vient de recroiser la limite extérieure de la base en entrant) ou
/// animation d'accostage en cours (`dock_anim`, le « ok » vert reste affiché
/// jusqu'à l'ouverture de la boîte DOCK STATION). Jamais au **départ** (le
/// vaisseau sort de la base, guide coupé par `release_links`), ni quand il
/// est tenu par la station (liens, boîtes, rétraction) ni en cosmonaute EVA
/// (il ne peut pas accoster : il est secouru - la mire seule le guide).
pub fn dock_approach_active(state: &GameState) -> bool {
    if state.cosmonaut_active
        || state.dock_box
        || state.shop_box
        || state.dock_links
        || state.dock_retract > 0.0
        || state.eva_recovery > 0.0
        || state.eva_crossfade > 0.0
    {
        return false;
    }
    state.docking_guide || state.dock_anim > 0.0
}

/// Période entre deux bips de proximité selon la distance au centre de la
/// station : **maximale** au bord du rayon de la base (le vaisseau vient
/// d'entrer, guide activé) et **minimale** au cercle d'accostage (sur le
/// point d'être capturé) - interpolation linéaire entre
/// `DOCK_APPROACH_BEEP_PERIOD_MAX` et `DOCK_APPROACH_BEEP_PERIOD_MIN` :
/// **plus on est près, plus les bips sont rapprochés**.
pub fn dock_approach_beep_period(dist: f64, station_radius: f64) -> f64 {
    let t = ((dist - STATION_DOCK_DISTANCE) / (station_radius - STATION_DOCK_DISTANCE))
        .clamp(0.0, 1.0);
    DOCK_APPROACH_BEEP_PERIOD_MIN
        + (DOCK_APPROACH_BEEP_PERIOD_MAX - DOCK_APPROACH_BEEP_PERIOD_MIN) * t
}

/// Écart angulaire (radians, 0..=π) entre la **trajectoire** du vaisseau
/// (sa direction de déplacement, `shape.direction` - le vaisseau avance dans
/// `moving_shape` le long de `(cos, -sin)`) et la direction du **centre de la
/// station** (vecteur le plus court dans le monde torique, repliement
/// cyclique) : 0 = parfaitement aligné (le vaisseau fonce droit sur la zone
/// d'accostage), π = à l'opposé. Vaisseau immobile : 0 (pas de trajectoire à
/// corriger - c'est l'état idéal de l'accostage).
pub fn approach_trajectory_deviation(player: &Shape, station: &Shape, world: &World) -> f64 {
    if player.velocity.abs() < 1e-9 {
        return 0.0; // immobile : pas de trajectoire, alignement parfait
    }
    let to_station = crate::geom::wrapped_delta(player.position, station.position, world);
    let r = to_station.x.hypot(to_station.y);
    if r < 1e-9 {
        return 0.0; // au centre de la station : plus rien à aligner
    }
    // direction de déplacement (convention de `moving_shape` : x += cos, y -= sin)
    let ux = player.direction.cos();
    let uy = -player.direction.sin();
    // direction du vaisseau vers le centre de la station
    let vx = to_station.x / r;
    let vy = to_station.y / r;
    (ux * vx + uy * vy).clamp(-1.0, 1.0).acos()
}

/// La trajectoire est-elle **bonne** ? Oui quand l'écart angulaire avec la
/// direction du centre de la station est sous `DOCK_APPROACH_TRAJ_OK_DEGREES`
/// (ou quand le vaisseau est immobile / au centre - écart nul).
pub fn approach_trajectory_ok(deviation_rad: f64) -> bool {
    deviation_rad.to_degrees() <= DOCK_APPROACH_TRAJ_OK_DEGREES
}

/// Texte de la trajectoire pour le message clignotant au-dessus du vaisseau :
/// « TRAJ: ON COURSE » (vert) quand l'approche est bonne, sinon « TRAJ: N° OFF »
/// (rouge) avec l'**écart en degrés** à corriger.
pub fn approach_traj_text(deviation_rad: f64) -> String {
    if approach_trajectory_ok(deviation_rad) {
        "TRAJ: ON COURSE".to_string()
    } else {
        format!("TRAJ: {:.0}° OFF", deviation_rad.to_degrees())
    }
}

/// Gain de volume du bip d'approche selon la trajectoire : **proportionnel à
/// la qualité** de la trajectoire (`1 - écart/π`) - le bip est **plus fort
/// quand la trajectoire est bonne** (alignée sur le centre de la station) et
/// s'atténue à mesure qu'elle s'écarte de l'optimum, avec le plancher
/// `DOCK_APPROACH_BEEP_TRAJ_MIN_GAIN` (il reste audible, discret).
pub fn approach_beep_traj_gain(deviation_rad: f64) -> f32 {
    let g = 1.0 - deviation_rad / std::f64::consts::PI;
    (g as f32).clamp(DOCK_APPROACH_BEEP_TRAJ_MIN_GAIN, 1.0)
}

/// Messages d'information au pilote pendant l'accostage (voir
/// `dock_approach_active`) - la **partie sonore**, appelée en tête de chaque
/// frame de `game::update` (avant les retours anticipés des boîtes et des
/// animations, pour couvrir aussi les frames de l'animation d'accostage) :
///
/// - au moment où le vaisseau est **capturé** (l'animation d'accostage
///   démarre, `dock_anim > 0`), un **son distinct** annonce que c'est bon -
///   une seule fois par accostage (`dock_approach_ok_sounded`) ; une fois
///   accosté (boîte DOCK STATION ouverte), **plus aucun son** ;
/// - pendant le **retour à la base** (guide d'accostage actif), un **bip**
///   accompagne le message clignotant : sa fréquence est liée à la distance
///   au centre de la station (`dock_approach_beep_period`) et son **volume**
///   à la trajectoire (`approach_beep_traj_gain`) - plus le vaisseau est
///   aligné sur le centre de la zone d'accostage, plus le bip est fort.
pub fn update_dock_approach(state: &mut GameState, shapes: &[Shape], mut sounds: Option<&mut Sounds>) {
    // son « accostage réussi » : au moment où le vaisseau est capturé (front
    // montant de `dock_anim`, posé par `docking` à la frame précédente) -
    // une seule fois tant que l'animation d'accostage dure
    if state.dock_anim > 0.0 {
        if !state.dock_approach_ok_sounded {
            state.dock_approach_ok_sounded = true;
            if let Some(s) = &mut sounds {
                s.play_dock_ok();
            }
        }
    } else {
        state.dock_approach_ok_sounded = false;
    }
    // bips de proximité : uniquement lors du RETOUR à la base (guide actif,
    // le vaisseau est dans le rayon de la station) et hors pause - pas au
    // départ (le guide est coupé), ni quand le vaisseau est tenu par la
    // station, ni en cosmonaute EVA (voir `dock_approach_active`)
    if !state.docking_guide || state.paused || !dock_approach_active(state) {
        return;
    }
    let now = get_time();
    if state.dock_approach_beep_at <= now {
        // volume lié à la trajectoire : plus le vaisseau est aligné sur le
        // centre de la zone d'accostage, plus le bip est fort (voir
        // `approach_beep_traj_gain`)
        let dev = approach_trajectory_deviation(
            &shapes[PLAYER_INDEX],
            &shapes[STATION_INDEX],
            &state.world,
        );
        if let Some(s) = &mut sounds {
            s.play_approach_beep(approach_beep_traj_gain(dev));
        }
        // fréquence liée à la distance au centre de la station (repliement
        // torique) : plus on est près, plus les bips se rapprochent
        let dist = crate::geom::wrapped_distance(
            shapes[PLAYER_INDEX].position,
            shapes[STATION_INDEX].position,
            &state.world,
        );
        state.dock_approach_beep_at =
            now + dock_approach_beep_period(dist, shapes[STATION_INDEX].radius);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beep_period_is_inversely_proportional_to_distance() {
        let r = 162.0;
        // au cercle d'accostage (centre) : période minimale (bips rapprochés)
        let at_center = dock_approach_beep_period(STATION_DOCK_DISTANCE, r);
        assert_eq!(at_center, DOCK_APPROACH_BEEP_PERIOD_MIN);
        // au bord du rayon de la base (entrée) : période maximale
        let at_edge = dock_approach_beep_period(r, r);
        assert_eq!(at_edge, DOCK_APPROACH_BEEP_PERIOD_MAX);
        // plus on est près, plus les bips sont rapprochés (période plus courte)
        let mid = dock_approach_beep_period((STATION_DOCK_DISTANCE + r) / 2.0, r);
        assert!(mid > at_center && mid < at_edge);
        // borné : hors de la plage, la période est saturée (jamais négative)
        assert_eq!(dock_approach_beep_period(0.0, r), DOCK_APPROACH_BEEP_PERIOD_MIN);
        assert_eq!(dock_approach_beep_period(r * 2.0, r), DOCK_APPROACH_BEEP_PERIOD_MAX);
    }

    #[test]
    fn approach_messages_are_informative() {
        // rouge : ce qui doit être corrigé (la vitesse limite est annoncée)
        assert!(dock_slow_down_text().contains("SLOW DOWN"));
        assert!(dock_slow_down_text().contains(&format!("{:.1}", STATION_DOCK_SPEED)));
        // vert : quand c'est bon (il n'y a plus qu'à couper les gaz)
        assert!(dock_in_range_text().contains("IN RANGE"));
    }

    #[test]
    fn approach_is_active_only_when_returning_to_base() {
        let mut state = GameState::new();
        // vol libre : ni guide ni animation - aucun message ni bip
        assert!(!dock_approach_active(&state));
        // départ : le guide est coupé (vaisseau sort de la base) - inactif
        state.dock_links = true;
        assert!(!dock_approach_active(&state));
        state.dock_links = false;
        // retour : le guide vient de s'allumer en entrant dans la base
        state.docking_guide = true;
        assert!(dock_approach_active(&state));
        // vert clignotant pendant toute l'animation d'accostage (capture)
        state.docking_guide = false;
        state.dock_anim = 1.0;
        assert!(dock_approach_active(&state));
        // accostage réussi : boîte DOCK STATION ouverte - plus rien
        state.dock_anim = 0.0;
        state.dock_box = true;
        assert!(!dock_approach_active(&state));
    }

    #[test]
    fn trajectory_deviation_measures_the_angle_to_the_station() {
        let world = World::define(1000.0, 1000.0, -500.0, -500.0, 500.0, 500.0);
        let mut station = Shape::default();
        station.position = Point::new(0.0, 0.0);
        let mut player = Shape::default();
        player.position = Point::new(100.0, 0.0); // à l'est de la station
        player.velocity = 1.0;
        // fonce droit sur le centre (convention moving_shape : x += cos, y -= sin)
        player.direction = std::f64::consts::PI; // vers l'ouest
        assert_eq!(approach_trajectory_deviation(&player, &station, &world), 0.0);
        // à l'opposé (vers l'est) : écart maximal π
        player.direction = 0.0;
        assert_eq!(
            approach_trajectory_deviation(&player, &station, &world),
            std::f64::consts::PI
        );
        // perpendiculaire (vers le nord) : écart de 90°
        player.direction = std::f64::consts::FRAC_PI_2;
        assert!(
            (approach_trajectory_deviation(&player, &station, &world) - std::f64::consts::FRAC_PI_2)
                .abs()
                < 1e-12
        );
        // immobile : pas de trajectoire à corriger (alignement parfait)
        player.velocity = 0.0;
        assert_eq!(approach_trajectory_deviation(&player, &station, &world), 0.0);
    }

    #[test]
    fn trajectory_message_and_volume_follow_the_deviation() {
        // aligné : « ON COURSE » (vert), bip au volume maximal
        assert!(approach_trajectory_ok(0.0));
        assert_eq!(approach_traj_text(0.0), "TRAJ: ON COURSE");
        assert_eq!(approach_beep_traj_gain(0.0), 1.0);
        // sous le seuil (45°) : toujours bon, mais le bip baisse un peu
        let ok_rad = 30.0f64.to_radians();
        assert!(approach_trajectory_ok(ok_rad));
        assert!(approach_beep_traj_gain(ok_rad) < 1.0 && approach_beep_traj_gain(ok_rad) > 0.75);
        // au-delà du seuil : « N° OFF » (rouge) avec l'écart en degrés
        let bad_rad = 60.0f64.to_radians();
        assert!(!approach_trajectory_ok(bad_rad));
        assert_eq!(approach_traj_text(bad_rad), "TRAJ: 60° OFF");
        // volume proportionnel à la qualité : 90° → moitié, 180° → plancher
        assert!((approach_beep_traj_gain(std::f64::consts::FRAC_PI_2) - 0.5).abs() < 1e-6);
        assert_eq!(
            approach_beep_traj_gain(std::f64::consts::PI),
            DOCK_APPROACH_BEEP_TRAJ_MIN_GAIN
        );
        // écart arrondi à l'unité dans le texte
        let rounded = 45.5f64.to_radians();
        assert_eq!(approach_traj_text(rounded), "TRAJ: 46° OFF");
    }
}
