//! Cosmonaute EVA : secours du pilote éjecté (vaisseau détruit) - portage
//! de `src/game.rs`. Le cosmonaute rejoint la base sans ramasser de minerais
//! (ceux relâchés au crash restent dans l'espace, pour le vaisseau reconstruit) ;
//! la station le récupère (cordon), puis le vaisseau est reconstruit.

use crate::config::*;
use crate::cosmonaut::{COSMONAUTE_EVA_PARK};
use crate::geom::{Point, Triangle};
use crate::shape::{compute_real_positions, Shape};
use crate::state::GameState;




/// Restaure le vaisseau à la station après une destruction (scénario
/// Survival - `scenario::PlayerHit::Destroyed`) : position, rotation et
/// vitesse remises à zéro (comme au départ), coque et triangles réparés - le
/// bouclier est déjà rechargé par `scenario::player_hit`. Le vaisseau se
/// retrouve à quai, dans l'état « déjà docké » du lancement (pas de boîte
/// DOCK STATION).
pub fn respawn_player(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    let p = &mut shapes[PLAYER_INDEX];
    p.position = Point::new(0.0, 0.0); // la station est au centre du monde
    p.direction = 0.0;
    p.velocity = 0.0;
    p.orientation = 0.0;
    p.rotation = 0.0;
    // le vaisseau est un mesh multi-triangles reconstruit avec la composition
    // courante (les plans liés aux upgrades apparaissent selon les niveaux
    // d'atelier - `vaisseau::rebuild_player_vaisseau`, qui recale aussi les
    // positions réelles sur les cinématiques posées ci-dessus)
    crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
    // le vaisseau reconstruit redevient un collider (`activate_cosmonaut`
    // l'avait coupé dans la destruction EVA) : il ramasse à nouveau les
    // minerais par collision
    shapes[PLAYER_INDEX].is_collider = true;
    // flamme et cooldown de tir coupés (le moteur ne brûle plus au respawn)
    state.player.thrusted = 0;
    state.player.revert_thrusted = 0;
    state.player.rotate_left_thrusted = 0;
    state.player.rotate_right_thrusted = 0;
    state.player.fire = 0.0;
    state.player_at_station = -1; // docké à la station (comme au lancement)
    state.player_enter_station = 0;
    // à quai : les liens d'accostage se rattachent au vaisseau (mire cachée)
    // jusqu'à ce que le joueur reparte (rétraction, voir `release_links`)
    state.dock_links = true;
}

/// Le vaisseau est détruit (jeu libre/Progression) : le joueur devient le
/// **cosmonaute éjecté** - il apparaît à la position du crash (le vaisseau
/// détruit reste invisible sur place, ses triangles sont morts) et doit
/// rejoindre la base (voir `rescue_cosmonaut`). La caméra, la mire et le HUD
/// suivent le cosmonaute (`input::pilot_index`). Le vaisseau détruit cesse
/// d'être un collider : il ne doit plus **re-ramasser** (ni détruire) les
/// minerais rejetés autour du crash - ils restent dans l'espace, pour le
/// vaisseau reconstruit à son retour.
pub fn activate_cosmonaut(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return; // cosmonaute EVA absent (jamais créé) : rien à éjecter
    }
    // le vaisseau mort ne doit plus entrer en collision (notamment avec les
    // minerais éparpillés autour du crash) - restauré par `respawn_player`
    shapes[PLAYER_INDEX].is_collider = false;
    let crash = shapes[PLAYER_INDEX].position;
    let c = &mut shapes[idx];
    c.position = crash;
    c.direction = 0.0;
    c.velocity = 0.0;
    c.orientation = 0.0;
    c.rotation = 0.0;
    for t in &mut triangles[c.first_triangle..=c.last_triangle] {
        compute_real_positions(t, c.position, c.center, c.orientation);
    }
    state.cosmonaut_active = true;
    state.docking_guide = true; // la mire guide le retour
    state.send_message("SHIP DESTROYED - RETURN TO THE STATION");
}

/// Le cosmonaute EVA a rejoint la base : il est **secouru** - le vaisseau est
/// reconstruit à la station (même état qu'au lancement, `respawn_player`), le
/// cosmonaute retourne à son poste (garé hors écran en bord de monde) et le
/// contrôle revient au vaisseau (qui démarre à quai, liens attachés).
pub fn rescue_cosmonaut(state: &mut GameState, shapes: &mut [Shape], triangles: &mut [Triangle]) {
    respawn_player(state, shapes, triangles);
    let idx = state.eva_cosmonaut as usize;
    let c = &mut shapes[idx];
    c.position = COSMONAUTE_EVA_PARK;
    c.direction = 0.0;
    c.velocity = 0.0;
    c.orientation = 0.0;
    c.rotation = 0.0;
    for t in &mut triangles[c.first_triangle..=c.last_triangle] {
        compute_real_positions(t, c.position, c.center, c.orientation);
    }
    state.cosmonaut_active = false;
    state.send_message("RESCUED - THE STATION REBUILT YOUR SHIP");
}

/// Le cosmonaute EVA a atteint la zone d'accostage (vaisseau détruit) : la
/// station le **récupère** - un cordon va jaillir de l'anneau jusqu'à lui et
/// le ramener sur l'anneau (voir `advance_eva_recovery` et
/// `render::draw_eva_recovery_cable`). Le monde continue de tourner :
/// `docking` est appelée dans la frame, la suite est traitée en tête de
/// `update` (les frames suivantes font avancer `collisions` juste après
/// l'animation). Après la récupération, le fondu enchaîné fait apparaître le
/// vaisseau reconstruit (`advance_eva_crossfade`, terminé par
/// `rescue_cosmonaut`).
pub fn start_eva_recovery(state: &mut GameState, shapes: &mut [Shape], _triangles: &mut [Triangle]) {
    let idx = state.eva_cosmonaut as usize;
    if idx >= shapes.len() {
        return; // cosmonaute EVA absent : rien à récupérer
    }
    let c = &shapes[idx];
    // point de l'anneau dans la direction du cosmonaute (le cordon le ramène
    // radialement sur le bord intérieur de l'anneau, comme les liens) -
    // recalculé au début de la traction, depuis la position atteinte pendant
    // le déploiement (voir `advance_eva_recovery`)
    let r = c.position.x.hypot(c.position.y);
    state.eva_recovery_to_pos = if r < 1.0 {
        Point::new(STATION_INNER_RADIUS, 0.0) // au centre : vers la droite
    } else {
        Point::new(
            c.position.x / r * STATION_INNER_RADIUS,
            c.position.y / r * STATION_INNER_RADIUS,
        )
    };
    // position de départ de la traction (mise à jour en phase 1, pendant le
    // déploiement - voir `advance_eva_recovery`)
    state.eva_recovery_from_pos = c.position;
    state.eva_recovery = EVA_RECOVERY_DURATION;
    // le cosmonaute **continue sur son élan** pendant que le cordon se déploie
    // vers lui : sa vitesse et son orientation sont conservées, il dérive avec
    // sa physique - il n'est immobilisé que lorsque le cordon, une fois tendu,
    // le tire (phase 2 de `advance_eva_recovery`)
    state.player.thrusted = 0; // flamme coupée : plus de poussée
    state.player.revert_thrusted = 0;
    state.player.rotate_left_thrusted = 0; // ni de jets latéraux
    state.player.rotate_right_thrusted = 0;
    state.send_message("STATION RECOVERY - HOLD ON");
}

/// Fait avancer la **récupération** du cosmonaute EVA d'une frame, en deux
/// phases (le monde continue de tourner, voir `update`) : pendant la fraction
/// `EVA_CABLE_DEPLOY_FRACTION` de `EVA_RECOVERY_DURATION`, le cordon se
/// déploie de l'anneau vers le cosmonaute qui **continue sur son élan** (sa
/// position dérive avec sa vitesse - la physique `moving_shape`, appelée par
/// `collisions` juste après, le déplace) ; une fois complètement déployé
/// (tendu), il le **ramène sur l'anneau** - position interpolée (smoothstep)
/// de `eva_recovery_from_pos` (position au début de la traction, mémorisée
/// pendant le déploiement) vers `eva_recovery_to_pos` sur la phase restante.
/// À la fin, le **fondu enchaîné** démarre : le vaisseau est reconstruit au
/// centre de la station (`respawn_player`, liens attachés) et le cosmonaute
/// s'efface pendant que le vaisseau apparaît (`advance_eva_crossfade`).
pub fn advance_eva_recovery(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) {
    state.eva_recovery = (state.eva_recovery - dt).max(0.0);
    // avancement global 0..1 sur toute la durée de la récupération
    let t = (1.0 - state.eva_recovery / EVA_RECOVERY_DURATION).clamp(0.0, 1.0);
    let idx = state.eva_cosmonaut as usize;
    let c = &mut shapes[idx];
    if t < EVA_CABLE_DEPLOY_FRACTION {
        // Phase 1 : le cordon se déploie de l'anneau vers le cosmonaute, qui
        // **continue sur son élan** : sa position n'est pas figée, elle dérive
        // avec sa vitesse (la physique `moving_shape`, appelée juste après par
        // `collisions`, le déplace) et le cordon « le chasse », sa longueur
        // suit sa position courante (voir `render::draw_eva_recovery_cable`).
        // On mémorise la position courante : ce sera le point de départ de la
        // traction une fois le cordon tendu (début de phase 2).
        state.eva_recovery_from_pos = c.position;
    } else {
        // Phase 2 : cordon complètement déployé (tendu), il ramène le
        // cosmonaute sur l'anneau - interpolation lissée (smoothstep) de
        // `eva_recovery_from_pos` (position au moment où le cordon s'est
        // tendu, mémorisée en phase 1) vers `eva_recovery_to_pos`, recalculé
        // depuis ce point (le cordon reste radial : point de l'anneau dans la
        // direction du cosmonaute au début de la traction)
        let r = state.eva_recovery_from_pos.x.hypot(state.eva_recovery_from_pos.y);
        state.eva_recovery_to_pos = if r < 1.0 {
            Point::new(STATION_INNER_RADIUS, 0.0) // au centre : vers la droite
        } else {
            Point::new(
                state.eva_recovery_from_pos.x / r * STATION_INNER_RADIUS,
                state.eva_recovery_from_pos.y / r * STATION_INNER_RADIUS,
            )
        };
        let u = ((t - EVA_CABLE_DEPLOY_FRACTION) / (1.0 - EVA_CABLE_DEPLOY_FRACTION)).clamp(0.0, 1.0);
        let e = u * u * (3.0 - 2.0 * u);
        c.position.x = state.eva_recovery_from_pos.x
            + (state.eva_recovery_to_pos.x - state.eva_recovery_from_pos.x) * e;
        c.position.y = state.eva_recovery_from_pos.y
            + (state.eva_recovery_to_pos.y - state.eva_recovery_from_pos.y) * e;
        // le cordon est tendu : le cosmonaute est tiré, plus d'élan propre
        c.velocity = 0.0;
        c.rotation = 0.0;
    }
    for t in &mut triangles[c.first_triangle..=c.last_triangle] {
        compute_real_positions(t, c.position, c.center, c.orientation);
    }
    if state.eva_recovery <= 0.0 {
        state.eva_recovery = 0.0;
        // le cosmonaute est sur l'anneau : le vaisseau est reconstruit au
        // centre de la station (liens attachés) - le fondu enchaîné le fait
        // apparaître pendant que le cosmonaute s'efface
        respawn_player(state, shapes, triangles);
        state.eva_crossfade = EVA_CROSSFADE_DURATION;
    }
}

/// Fait avancer le **fondu enchaîné** de la récupération d'une frame : le
/// cosmonaute ramené sur l'anneau s'efface (alpha décroissant, rendu par
/// `main.rs`) pendant que le vaisseau reconstruit apparaît au centre de la
/// station, liens attachés (alpha croissant) - la caméra glisse de l'anneau
/// vers le centre (renvoyée à `update`). À la fin, le secours est terminé :
/// le cosmonaute retourne à son poste et le contrôle revient au vaisseau
/// (`rescue_cosmonaut`).
pub fn advance_eva_crossfade(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    dt: f64,
) -> Point {
    state.eva_crossfade = (state.eva_crossfade - dt).max(0.0);
    // caméra : glisse du cosmonaute (sur l'anneau) vers le centre de la
    // station où le vaisseau apparaît - interpolée (smoothstep) sur la durée
    let idx = state.eva_cosmonaut as usize;
    let pos = shapes[idx].position;
    let t = (1.0 - state.eva_crossfade / EVA_CROSSFADE_DURATION).clamp(0.0, 1.0);
    let e = t * t * (3.0 - 2.0 * t);
    let mut camera = Point::new(
        VIEWPORT_WIDTH / 2.0 - pos.x * (1.0 - e),
        VIEWPORT_HEIGHT / 2.0 - pos.y * (1.0 - e),
    );
    camera.normalize_world(&state.world);
    if state.eva_crossfade <= 0.0 {
        state.eva_crossfade = 0.0;
        rescue_cosmonaut(state, shapes, triangles);
    }
    camera
}
