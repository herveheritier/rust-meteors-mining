//! Rendu de la station : boîte de choix DOCK STATION, mire
//! d'accostage, liens néon, cordon de récupération EVA et
//! marqueurs d'accostage (issu de `src/render.rs`).

use macroquad::prelude::*;
use crate::config::*;
use crate::render::*;
use crate::font::measure_text;
use crate::geom::{Point, Triangle, World};
use crate::docking::{
    approach_traj_text, approach_trajectory_deviation, approach_trajectory_ok, dock_approach_active,
    dock_in_range_text, dock_slow_down_text,
};
use crate::scenario;
use crate::shape::Shape;
use crate::state::GameState;

pub fn choice_box_width() -> f32 {
    let msg_w = measure_text("*** DOCK STATION ***", None, 16, 1.0).width + 2.0 * BOX_PADDING;
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(70.0);
    let buttons: f32 = CHOICE_BOX_LABELS
        .iter()
        .map(|l| btn_w(l))
        .sum::<f32>()
        + (CHOICE_BOX_LABELS.len() as f32 - 1.0) * BOX_PADDING;
    340.0f32.max(msg_w).max(buttons + 2.0 * BOX_PADDING)
}

/// Géométrie de la boîte de choix DOCK STATION (ex `windowUtils_choiceBox`) :
/// fenêtre de 158 px de haut centrée sur l'écran - titre, ligne d'état
/// (soute et minerais, dessinée par `draw_choice_box`) et boutons côte à
/// côte en bas. Renvoie les rectangles écran des boutons DÉCHARGER / MARCHÉ
/// / QUITTER (pour la détection de clic côté logique). Le bouton
/// REFUEL/REARM n'existe plus : le carburant et les munitions s'achètent au
/// magasin (bouton MARCHÉ).
pub struct ChoiceBoxLayout {
    /// Bouton DÉCHARGER : décharge la soute (crédits disponibles pour le
    /// ravitaillement au magasin juste après - la boîte reste ouverte).
    pub unload: Rect,
    /// Bouton MARCHÉ : ouvre le magasin de la station (carburant, munitions,
    /// armes, extensions et modes de déplacement en scénario à économie).
    pub shop: Rect,
    /// Bouton QUITTER : ferme la boîte.
    pub close: Rect,
}

pub fn choice_box_layout() -> ChoiceBoxLayout {
    let h = 158.0;
    let btn_h = 30.0;
    let w = choice_box_width();
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    // boutons alignés à gauche dans la boîte (la largeur est calculée pour
    // qu'ils tiennent sans chevauchement, marges = padding)
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(70.0);
    let top_btn = top + h - 22.0 - btn_h;
    let mut x = left + BOX_PADDING;
    let mut rects = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for (i, &label) in CHOICE_BOX_LABELS.iter().enumerate() {
        rects[i] = Rect::new(x, top_btn, btn_w(label), btn_h);
        x += rects[i].w + BOX_PADDING;
    }
    ChoiceBoxLayout {
        unload: rects[0],
        shop: rects[1],
        close: rects[2],
    }
}

/// Dessine la boîte de choix DOCK STATION (accostage) : titre, ligne d'état
/// (soute et minerais - le joueur voit ce qu'il déchargera et ce qu'il
/// pourra dépenser au marché) et boutons DÉCHARGER / MARCHÉ / QUITTER avec
/// survol (fond surbrillé + texte blanc, ex `windowUtils_choiceBox`).
pub fn draw_choice_box(state: &GameState) {
    let msg = "*** DOCK STATION ***";
    let w = choice_box_width();
    let h = 158.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text_shadow(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    // ligne d'état : soute + crédits (le joueur voit ce qu'il déchargera
    // et le budget disponible pour le marché)
    let mut status = format!(
        "SOUTE : {}/{}",
        state.player.cargo_qty, state.player.cargo_size
    );
    if scenario::has_economy(state) {
        status.push_str(&format!("      CRÉDITS : {}", state.resources.credits));
    }
    draw_text_shadow(
        &status,
        left + BOX_PADDING + 4.0,
        top + 2.0 * BOX_PADDING + 40.0,
        16.0,
        argb_to_color(BOX_FG_DIM),
    );

    // boutons avec survol
    let l = choice_box_layout();
    draw_box_button(CHOICE_BOX_LABELS[0], l.unload);
    draw_box_button(CHOICE_BOX_LABELS[1], l.shop);
    draw_box_button(CHOICE_BOX_LABELS[2], l.close);
}

/// Qualité de l'approche pour la mire (0 = rouge, 1 = vert) : interpolée sur
/// **tout le rayon de la base** (0 au bord du rayon, 1 au centre) et sur la
/// vitesse (0 à `DOCK_APPROACH_FULL_RED_SPEED` ou plus, 1 à l'arrêt) - la
/// mire réagit dès que le vaisseau entre dans le rayon de la station.
pub fn docking_approach_quality(dist: f64, speed: f64, station_radius: f64) -> f64 {
    let dist_q = 1.0 - (dist / station_radius).clamp(0.0, 1.0);
    let speed_q = 1.0 - (speed.abs() / DOCK_APPROACH_FULL_RED_SPEED).clamp(0.0, 1.0);
    dist_q * speed_q
}

/// Effet néon d'un anneau : halo (3 cercles concentriques d'alpha décroissant
/// - macroquad n'a pas de flou, on empile) + anneau principal + cœur clair.
pub fn neon_ring(x: f32, y: f32, radius: f32, color: Color) {
    for i in 1..=3 {
        let mut halo = color;
        halo.a = color.a * (0.5 - 0.13 * i as f32);
        draw_circle_lines(x, y, radius + i as f32 * 2.0, 1.0, halo);
    }
    draw_circle_lines(x, y, radius, 1.5, color);
    let bright = Color::new(1.0, 1.0, 1.0, color.a * 0.6);
    draw_circle_lines(x, y, radius, 0.75, bright);
}

/// Effet néon d'un trait (croix de visée) : halo large + trait principal +
/// cœur clair.
pub fn neon_line(x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
    let mut halo = color;
    halo.a = color.a * 0.3;
    draw_line(x1, y1, x2, y2, 3.0, halo);
    draw_line(x1, y1, x2, y2, 1.2, color);
    let bright = Color::new(1.0, 1.0, 1.0, color.a * 0.6);
    draw_line(x1, y1, x2, y2, 0.6, bright);
}

/// Effet néon d'un point : halo + cœur + point brillant.
pub fn neon_dot(x: f32, y: f32, radius: f32, color: Color) {
    let mut halo = color;
    halo.a = color.a * 0.35;
    draw_circle(x, y, radius * 2.5, halo);
    draw_circle(x, y, radius, color);
    let bright = Color::new(1.0, 1.0, 1.0, color.a * 0.7);
    draw_circle(x, y, radius * 0.5, bright);
}

/// Mire d'accostage au centre de la station : la **zone d'accostage** (cercle
/// de rayon `STATION_DOCK_DISTANCE`) est affichée - cercle + croix de visée +
/// point central, semi-transparents, légèrement pulsants et avec un **effet
/// néon** (halo + cœur clair) - pour montrer où poser le vaisseau. La couleur
/// passe **progressivement du rouge au vert selon la qualité de l'approche**,
/// interpolée sur **tout le rayon de la base** (`station_radius`) : rouge au
/// bord du rayon de la station ou trop rapide, vert au centre et presque
/// immobile (disque clignotant = prêt à accoster). Dessinée **sous le
/// vaisseau** (appelée avant son rendu).
pub fn draw_docking_marker(
    camera: Point,
    world: &World,
    station_position: Point,
    station_radius: f64,
    player_position: Point,
    player_speed: f64,
) {
    let center = screen_point(station_position, camera, world);
    if !inner_draw_limit(Point::new(center.x as f64, center.y as f64)) {
        return; // la zone est hors écran (la distance est au HUD)
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let dist = crate::geom::wrapped_distance(player_position, station_position, world);
    let in_zone = dist < STATION_DOCK_DISTANCE;
    // qualité de l'approche sur tout le rayon de la base (voir
    // `docking_approach_quality`) : interpolation continue rouge → vert
    let q = docking_approach_quality(dist, player_speed, station_radius);
    let r = (255.0 - 195.0 * q) as u32;
    let g = (60.0 + 195.0 * q) as u32;
    let ring = argb_to_color((DOCK_MARKER_ALPHA << 24) | (r << 16) | (g << 8));
    let dot = argb_to_color((DOCK_MARKER_DOT_ALPHA << 24) | (r << 16) | (g << 8));
    // respiration : le rayon oscille légèrement pour attirer l'œil
    let radius = STATION_DOCK_DISTANCE as f32 + 1.5 * (get_time() * 4.0).sin() as f32;
    // anneau néon + croix de visée + point central (où poser le vaisseau)
    neon_ring(center.x, center.y, radius, ring);
    neon_line(center.x - radius, center.y, center.x + radius, center.y, ring);
    neon_line(center.x, center.y - radius, center.x, center.y + radius, ring);
    neon_dot(center.x, center.y, 2.0, dot);
    // prêt à accoster (dans la zone, presque immobile) : disque clignotant néon
    if in_zone && player_speed.abs() < STATION_DOCK_SPEED && (get_time() * 8.0) as i32 % 2 == 0 {
        let mut halo = ring;
        halo.a = ring.a * 0.3;
        draw_circle(center.x, center.y, radius * 0.8, halo);
        draw_circle(center.x, center.y, radius * 0.6, ring);
    }
}

/// La mire d'accostage est-elle visible ? Elle n'est affichée **que lors du
/// retour à la base** (`state.docking_guide`, posé par
/// `docking::update_docking_guide` quand le vaisseau recroise la limite
/// extérieure en entrant) : jamais pendant que le vaisseau quitte
/// l'accostage, à quai, pendant l'animation d'accostage (il est tiré vers le
/// centre), tant que la boîte DOCK STATION / l'atelier est ouvert (accosté)
/// et pendant la rétraction des liens au départ - dans tous ces cas, le
/// vaisseau est tenu par les liens ou le guide est coupé. En plus du guide,
/// la distance au centre de la base doit être sous `station_radius` (le
/// guide n'est actif que dans le rayon, garde défensive).
pub fn docking_marker_visible(
    state: &GameState,
    player_position: Point,
    station_position: Point,
    station_radius: f64,
) -> bool {
    if state.dock_anim > 0.0
        || state.dock_box
        || state.shop_box
        || state.dock_retract > 0.0
        || state.dock_links
        || state.eva_recovery > 0.0
        || state.eva_crossfade > 0.0
        || !state.docking_guide
    {
        return false; // tenu par les liens, ou pas encore revenu à la base
    }
    // distance la plus courte dans le monde torique (repliement cyclique)
    let dist = crate::geom::wrapped_distance(player_position, station_position, &state.world);
    dist < station_radius
}

/// Déformation transversale (offset perpendiculaire, en pixels écran) d'un
/// point de câble à la fraction `t` (0 = anneau, 1 = extrémité mobile), pour
/// une intensité d'ondulation `wave` (0 = câble tendu) à l'instant `time` :
/// onde qui court vers le **vaisseau** (`toward_ship`, déploiement « en
/// projection ») ou vers l'**anneau** (rétraction), enveloppe croissante
/// vers l'extrémité mobile (`× t` - l'extrémité libre fouette).
pub fn cable_wave_offset(t: f32, wave: f32, time: f32, toward_ship: bool) -> f32 {
    let speed = if toward_ship { -18.0 } else { 18.0 };
    (t * 12.0 + time * speed).sin() * wave * t
}

/// Enveloppe d'ondulation du lien pendant le **désamarrage** (relâchement de
/// la tension) : maximale au largage (`r` = 0, le câble fouette), nulle une
/// fois le lien rentré (`r` = 1), légèrement pulsante entre les deux (la
/// tension se relâche par à-coups).
pub fn retract_envelope(r: f64) -> f64 {
    (1.0 - r) * (0.6 + 0.4 * (r * TAU * 1.5).cos())
}

/// Trace un lien d'accostage entre l'anneau (`a`) et l'extrémité mobile
/// (vers `b`), déployé à `prog` (0 = encore sur l'anneau, 1 = tendu jusqu'à
/// `b`) : pendant le **déploiement** (projection, `toward_ship = true`) ou
/// la **rétraction** (relâchement de la tension, `toward_ship = false`), le
/// câble **ondule** avec l'intensité `wave` (0 = câble tendu, voir
/// `cable_wave_offset`) ; une fois tendu, il est droit. Dessiné en segments
/// néon (même rendu que `neon_line`).
pub fn draw_docking_cable(a: Vec2, b: Vec2, prog: f32, wave: f32, toward_ship: bool, color: Color) {
    const SEGMENTS: usize = 14;
    // extrémité mobile à l'avancement prog (de l'anneau vers b)
    let ex = a.x + (b.x - a.x) * prog;
    let ey = a.y + (b.y - a.y) * prog;
    let dx = ex - a.x;
    let dy = ey - a.y;
    let len = dx.hypot(dy);
    if len < 1.0 {
        return; // câble pas encore déployé / entièrement rétracté
    }
    // direction perpendiculaire normalisée (déformation transversale)
    let nx = -dy / len;
    let ny = dx / len;
    let time = get_time() as f32;
    let mut prev = a;
    for k in 1..=SEGMENTS {
        let t = k as f32 / SEGMENTS as f32;
        let offset = cable_wave_offset(t, wave, time, toward_ship);
        let p = vec2(a.x + dx * t + nx * offset, a.y + dy * t + ny * offset);
        neon_line(prev.x, prev.y, p.x, p.y, color);
        prev = p;
    }
}

/// Traits d'accostage dessinés quand le vaisseau est **tenu par les liens** :
/// à quai (`state.dock_links`, lancement/respawn - vaisseau au centre, liens
/// tendus), **pendant l'animation d'accostage** (avant l'ouverture de la boîte
/// DOCK STATION, `state.dock_anim > 0`) et **pendant la rétraction au
/// départ** (`state.dock_retract > 0`, après CLOSE ou au démarrage) : **4
/// liens néon verts simultanés** qui relient le bord intérieur de la station
/// (rayon `STATION_INNER_RADIUS`) au vaisseau - un par **diagonale** (NO,
/// SO, SE, NE, angle ±π/4, plus crédible que les cardinaux), chacun partant
/// de l'anneau de la station vers le point diagonal correspondant du
/// vaisseau, **près de son centre** (`DOCK_LINE_SHIP_ANCHOR`, l'illusion
/// qu'ils le touchent).
///
/// À l'accostage, les liens se **déploient en projection** : ils jaillissent
/// de l'anneau vers le vaisseau pendant les ~35 % de l'animation (onde qui
/// court vers le vaisseau), puis, tendus, le tirent au centre tout en le
/// pivotant vers la droite. Au départ, la tension est relâchée : les liens se
/// **rétractent en ondulant** vers l'anneau (`DOCK_RETRACT_DURATION`) jusqu'à
/// disparaître. À quai, ils sont tendus (droits). Discrets (`DOCK_LINE_ALPHA`),
/// dessinés **sous le vaisseau** (appelés avant son rendu, après la mire).
pub fn draw_docking_line(
    state: &GameState,
    camera: Point,
    world: &World,
    station: &Shape,
    player: &Shape,
    fade: f32,
) {
    // à quai (lancement/respawn) : liens tendus jusqu'au vaisseau au centre
    let docked = state.dock_links;
    let retracting = state.dock_retract > 0.0;
    let docking = state.dock_anim > 0.0;
    if !docked && !retracting && !docking {
        return;
    }
    // avancement de la rétraction 0..1 (lissé) : 0 = liens tendus jusqu'au
    // vaisseau, 1 = rétractés sur le bord intérieur de l'anneau (disparus) -
    // à quai (`docked`) : liens tendus, r = 0
    let r = if retracting {
        let t = (1.0 - state.dock_retract / DOCK_RETRACT_DURATION).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    } else {
        0.0
    };
    // avancement du déploiement « en projection » pendant l'accostage : les
    // liens jaillissent de l'anneau vers le vaisseau pendant les ~35 % de
    // l'animation (0 = au niveau de l'anneau, 1 = tendus jusqu'au vaisseau),
    // puis restent tendus pendant que le vaisseau est tiré au centre
    let deploy = if docking {
        let t = ((1.0 - state.dock_anim / DOCK_ANIMATION_DURATION) / 0.35).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    } else {
        1.0
    };
    // bord intérieur de la station aux 4 points DIAGONAUX (NO, SO, SE, NE -
    // angle ±π/4, plus crédible que les 4 points cardinaux), avant rotation
    let diag = std::f64::consts::FRAC_1_SQRT_2; // cos/sin de ±π/4 ≈ 0,7071
    let inner = [
        Point::new(-STATION_INNER_RADIUS * diag, -STATION_INNER_RADIUS * diag), // NO
        Point::new(-STATION_INNER_RADIUS * diag, STATION_INNER_RADIUS * diag),  // SO
        Point::new(STATION_INNER_RADIUS * diag, STATION_INNER_RADIUS * diag),   // SE
        Point::new(STATION_INNER_RADIUS * diag, -STATION_INNER_RADIUS * diag),  // NE
    ];
    // côté correspondant du vaisseau : mêmes diagonales mais **près du
    // centre** (petit losange à ~`DOCK_LINE_SHIP_ANCHOR` du centre -
    // l'illusion que les liens touchent le vaisseau, dessinés dessous)
    let anchor = DOCK_LINE_SHIP_ANCHOR * diag;
    let sides = [
        Point::new(-anchor, -anchor), // NO
        Point::new(-anchor, anchor),  // SO
        Point::new(anchor, anchor),   // SE
        Point::new(anchor, -anchor),  // NE
    ];
    let mut from = Point::new(0.0, 0.0);
    let mut to = Point::new(0.0, 0.0);
    let mut color = argb_to_color((DOCK_LINE_ALPHA << 24) | 0x0040FF40);
    // fondu enchaîné de la récupération EVA : les liens apparaissent avec le
    // vaisseau reconstruit (`fade` = opacité du vaisseau)
    color.a *= fade;
    for (inner_local, side_local) in inner.iter().zip(sides.iter()) {
        from.x = inner_local.x;
        from.y = inner_local.y;
        from.rotate_around(station.center, station.orientation);
        let from_world = Point::new(station.position.x + from.x, station.position.y + from.y);
        to.x = side_local.x;
        to.y = side_local.y;
        to.rotate_around(player.center, player.orientation);
        let to_world = Point::new(player.position.x + to.x, player.position.y + to.y);
        let a = screen_point(from_world, camera, world);
        let b = screen_point(to_world, camera, world);
        if retracting {
            // la tension est relâchée : le câble part TENDU (longueur 1 - r,
            // maximale au largage) et se rétracte vers l'anneau en ondulant
            // (onde qui court vers l'anneau, enveloppe qui retombe par à-coups)
            draw_docking_cable(a, b, (1.0 - r) as f32, (16.0 * retract_envelope(r)) as f32, false, color);
        } else if docking {
            // déploiement « en projection » : les liens jaillissent de
            // l'anneau vers le vaisseau (onde qui court vers le vaisseau,
            // plus forte tant qu'ils ne sont pas tendus)
            draw_docking_cable(a, b, deploy as f32, 6.0 * (1.0 - deploy as f32), true, color);
        } else {
            // à quai : câble tendu (ligne droite)
            neon_line(a.x, a.y, b.x, b.y, color);
        }
    }
}

/// Cordon de **récupération** du cosmonaute EVA (vaisseau détruit, il a
/// rejoint la base) : pendant la récupération (`state.eva_recovery > 0`), un
/// cordon **orange** jaillit de l'anneau vers le cosmonaute (déploiement sur
/// les ~30 % du début, pendant lesquels il **continue sur son élan** - le
/// cordon suit sa position qui dérive) puis, tendu, le **ramène sur l'anneau** ;
/// son ondulation s'affaisse à mesure que la tension monte. Pendant le fondu
/// enchaîné (`state.eva_crossfade > 0`), il reste tendu et **s'efface avec le
/// cosmonaute**. Dessiné **sous le cosmonaute** (appelé avant son rendu).
/// Couleur **verte** du message d'approche clignotant (« c'est bon » :
/// quasi immobile dans le rayon de la base, ou capturé par l'animation
/// d'accostage) - même vert que le HUD d'accostage.
const DOCK_APPROACH_OK_COLOR: u32 = 0xFF40FF40;
/// Couleur **rouge** du message d'approche clignotant (« à corriger » :
/// approche trop rapide pour accoster) - même rouge que le HUD d'accostage.
const DOCK_APPROACH_WARN_COLOR: u32 = 0xFFFF3C00;
/// Marge (px écran) entre le haut de la coque du vaisseau et le message.
const DOCK_APPROACH_MARGIN: f32 = 6.0;
/// Interligne (px écran) entre les deux lignes de message (vitesse puis
/// trajectoire, empilées au-dessus du vaisseau).
const DOCK_APPROACH_LINE_GAP: f32 = 4.0;

/// Messages d'information au pilote pendant l'accostage : textes **clignotants**
/// affichés **au-dessus du vaisseau** (ancrés sur sa boîte englobante écran) et
/// **centrés horizontalement** sur lui, empilés en deux lignes :
///
/// - la **vitesse** (« DOCK: SLOW DOWN… » : approche trop rapide, ou
///   « DOCK: IN RANGE… » : quasi immobile ou vaisseau capturé par l'animation
///   d'accostage) - **rouge** pour ce qui doit être corrigé, **vert** quand
///   c'est bon ;
/// - la **trajectoire** au-dessus (« TRAJ: ON COURSE » : alignée sur le
///   centre de la zone d'accostage, ou « TRAJ: N° OFF » : l'**écart en
///   degrés** à corriger, voir `docking::approach_traj_text`) - même code de
///   couleurs.
///
/// Affichés dès que le vaisseau **entre dans la station** (guide d'accostage
/// actif) et jusqu'à la fin de l'animation d'accostage (`dock_approach_active`,
/// le vert reste clignotant pendant la capture). Désactivés une fois
/// l'accostage réussi (boîte DOCK STATION) et jamais affichés au départ (le
/// vaisseau sort de la base, guide coupé). La **partie sonore** (bips
/// d'autant plus rapprochés que le vaisseau est près, plus forts quand la
/// trajectoire est bonne + son distinct à la capture) est gérée par
/// `docking::update_dock_approach`. Dessiné par-dessus le vaisseau (appelé
/// après son rendu, avant le HUD).
pub fn draw_dock_approach_message(
    state: &GameState,
    player: &Shape,
    station: &Shape,
    triangles: &[Triangle],
    camera: Point,
) {
    if !dock_approach_active(state) {
        return;
    }
    // clignotement : moitié du temps visible, comme les alertes du HUD
    // (les deux lignes clignotent ensemble)
    if (get_time() * HUD_BLINK_HZ) as i64 % 2 != 0 {
        return;
    }
    // vert si l'approche est bonne (quasi immobile, ou déjà capturé par
    // l'animation d'accostage), rouge pour ce qui doit être corrigé
    let ok = state.dock_anim > 0.0 || player.velocity.abs() < STATION_DOCK_SPEED;
    let speed_text = if ok {
        dock_in_range_text().to_string()
    } else {
        dock_slow_down_text()
    };
    let speed_color = argb_to_color(if ok { DOCK_APPROACH_OK_COLOR } else { DOCK_APPROACH_WARN_COLOR });
    // trajectoire : alignée sur le centre de la zone d'accostage (vert) ou
    // écart en degrés à corriger (rouge)
    let traj_dev = approach_trajectory_deviation(player, station, &state.world);
    let traj_ok = approach_trajectory_ok(traj_dev);
    let traj_text = approach_traj_text(traj_dev);
    let traj_color = argb_to_color(if traj_ok { DOCK_APPROACH_OK_COLOR } else { DOCK_APPROACH_WARN_COLOR });
    // ancre « au-dessus du vaisseau » : boîte englobante écran des triangles
    // du vaisseau (positions réelles de la frame) - le texte est centré
    // horizontalement sur le vaisseau (la caméra suit le pilote)
    let mut top = f32::MAX;
    let mut left = f32::MAX;
    let mut right = f32::MIN;
    for t in &triangles[player.first_triangle..=player.last_triangle] {
        let a = screen_point(t.real_min, camera, &state.world);
        let b = screen_point(t.real_max, camera, &state.world);
        top = top.min(a.y);
        left = left.min(a.x);
        right = right.max(b.x);
    }
    if top == f32::MAX {
        return; // aucun triangle vivant : pas de vaisseau à survoler
    }
    // ligne du bas (vitesse), juste au-dessus de la coque : hauteur de ligne
    // (~16 px à la police 8 px du jeu) + petite marge, sans chevaucher le
    // vaisseau ; ligne du haut (trajectoire) empilée par-dessus
    let y_speed = top - 16.0 - DOCK_APPROACH_MARGIN;
    let w_speed = measure_text(&speed_text, None, 16, 1.0).width;
    let x_speed = ((left + right) / 2.0 - w_speed / 2.0).round();
    draw_text_shadow(&speed_text, x_speed, y_speed, 16.0, speed_color);
    let y_traj = y_speed - 16.0 - DOCK_APPROACH_LINE_GAP;
    let w_traj = measure_text(&traj_text, None, 16, 1.0).width;
    let x_traj = ((left + right) / 2.0 - w_traj / 2.0).round();
    draw_text_shadow(&traj_text, x_traj, y_traj, 16.0, traj_color);
}

pub fn draw_eva_recovery_cable(
    state: &GameState,
    camera: Point,
    world: &World,
    cosmonaut: &Shape,
) {
    if state.eva_recovery <= 0.0 && state.eva_crossfade <= 0.0 {
        return;
    }
    let pos = cosmonaut.position;
    let r = pos.x.hypot(pos.y);
    if r < 1.0 {
        return; // cosmonaute au centre : le cordon n'a pas encore d'ancrage
    }
    // ancrage sur l'anneau, dans la direction du cosmonaute (il est ramené
    // radialement - le cordon reste tendu le long du rayon)
    let anchor = Point::new(pos.x / r * STATION_INNER_RADIUS, pos.y / r * STATION_INNER_RADIUS);
    let a = screen_point(anchor, camera, world);
    let b = screen_point(pos, camera, world);
    let mut color = argb_to_color(EVA_RECOVERY_CABLE_COLOR);
    if state.eva_crossfade > 0.0 {
        // fondu enchaîné : le cordon s'efface avec le cosmonaute
        let t = (1.0 - state.eva_crossfade / EVA_CROSSFADE_DURATION).clamp(0.0, 1.0) as f32;
        color.a *= 1.0 - t;
        draw_docking_cable(a, b, 1.0, 0.0, true, color); // tendu
    } else {
        // récupération en deux phases : le cordon se déploie de l'anneau
        // vers le cosmonaute (qui **continue sur son élan** - sa position
        // dérive, l'extrémité du cordon le suit) pendant la fraction
        // `EVA_CABLE_DEPLOY_FRACTION`, puis, complètement tendu, le ramène
        // sur l'anneau - ondulation forte tant que le câble est lâche
        // (déploiement), nulle une fois la tension installée (traction)
        let t = (1.0 - state.eva_recovery / EVA_RECOVERY_DURATION).clamp(0.0, 1.0) as f32;
        let deploy = (t / EVA_CABLE_DEPLOY_FRACTION as f32).clamp(0.0, 1.0);
        let wave = 10.0 * (1.0 - t);
        draw_docking_cable(a, b, deploy, wave, true, color);
    }
}
