//! Accostage de la station : détection du retour, animation
//! d'accostage, boîte DOCK STATION, rétraction des liens néon et
//! guide d'accostage (mire) - portage de `src/game.rs`.

use macroquad::prelude::*;
use crate::config::*;
use crate::geom::{Point, Triangle};
use crate::render::{choice_box_layout, mouse_to_game};
use crate::scenario;
use crate::shape::{compute_real_positions, Shape};
use crate::state::{Element, GameState};
use crate::eva::start_eva_recovery;

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
            // compteur d'accostages (objectifs DAG)
            state.docking_count += 1;
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
