//! Rendu.
//!
//! Portage de la partie rendu de `meteorsMining.bas` : chargement des assets
//! (textures + couches d'étoiles précalculées), dessin des étoiles, des
//! triangles (texturés ou non), des formes, de la poussée, des débris, du
//! cargo et du HUD.
//!
//! NB : macroquad 0.4 ne fournit pas de `draw_triangle_texture` (le plan
//! `docs/PORTAGE.md` le supposait) — on l'implémente ici via
//! `models::Mesh` + `draw_mesh`, qui utilisent la pipeline 2D et sa texture.

use macroquad::models::{draw_mesh, Mesh, Vertex};
use macroquad::prelude::*;
use ::rand::{Rng, SeedableRng};
use ::rand_chacha::ChaCha12Rng;

use crate::config::*;
use crate::garbage::Garbage;
use crate::geom::{Point, Triangle, World};
use crate::shape::{get_border_segments, Shape};
use crate::state::{Element, GameState};

/// Taille d'une tuile d'étoiles précalculée (pixels monde = pixels écran).
pub const STAR_TILE: u32 = 1024;

// ─── Couleurs ────────────────────────────────────────────────────────────────

/// Convertit une couleur ARGB 32 bits QB64 (AARRGGBB) en `Color` macroquad
/// (RGBA). NB : l'ordre des octets change — voir `docs/PORTAGE.md` §6.
pub fn argb_to_color(argb: u32) -> Color {
    Color::new(
        ((argb >> 16) & 0xFF) as f32 / 255.0,
        ((argb >> 8) & 0xFF) as f32 / 255.0,
        (argb & 0xFF) as f32 / 255.0,
        ((argb >> 24) & 0xFF) as f32 / 255.0,
    )
}

// ─── Assets ──────────────────────────────────────────────────────────────────

/// Assets chargés au démarrage (ex `_loadimage` de `meteorsMining.bas`).
pub struct Assets {
    pub orange: Texture2D,
    pub player: Texture2D,
    pub meteor: Texture2D,
    pub station: Texture2D,
    /// Une texture par couche de parallaxe (15), précalculée une seule fois :
    /// c'est l'optimisation « étoiles » du plan (100× plus rapide que les
    /// 100 000 `pset` par frame de l'original).
    pub star_layers: Vec<Texture2D>,
}

impl Assets {
    /// Charge les 4 textures depuis `assets/` (copie convertie des assets de
    /// référence — NB : `reference/assets/meteor_surface_tile.png` est un JPEG
    /// déguisé en .png, non lisible par macroquad ; la conversion est faite
    /// une fois dans `assets/`, voir `docs/ASSETS.md` §4).
    pub async fn load() -> Assets {
        let orange = load_texture("assets/orange2.png")
            .await
            .expect("assets/orange2.png introuvable — lancer depuis la racine du projet");
        let player = load_texture("assets/vaisseau.png")
            .await
            .expect("assets/vaisseau.png introuvable");
        let meteor = load_texture("assets/meteor_surface_tile.png")
            .await
            .expect("assets/meteor_surface_tile.png introuvable");
        let station = load_texture("assets/station.png")
            .await
            .expect("assets/station.png introuvable");

        let star_layers = build_star_layers();

        Assets {
            orange,
            player,
            meteor,
            station,
            star_layers,
        }
    }
}

/// Précalcule les 15 couches d'étoiles.
///
/// Dans l'original, chaque étoile est un pixel blanc d'alpha aléatoire
/// `127..255`, positionné en « plan-espace » (`plan = (i mod 15) + 1`). Ici on
/// génère une tuile périodique par couche avec la **même densité** : un champ
/// aléatoire uniforme est statistiquement identique à l'original (le plan
/// PORTAGE.md préconise cette optimisation).
fn build_star_layers() -> Vec<Texture2D> {
    let mut rng = ChaCha12Rng::from_entropy();
    let mut layers = Vec::with_capacity(STARS_LAYERS as usize);
    for layer in 0..STARS_LAYERS {
        let plan = (layer + 1) as f64;
        // densité de l'original : STARS_COUNT/STARS_LAYERS étoiles réparties
        // sur le monde du plan (W×plan par H×plan), ramenée à la tuile.
        let area = WORLD_WIDTH * plan * WORLD_HEIGHT * plan;
        let n = ((STARS_COUNT as f64 / STARS_LAYERS as f64)
            * (STAR_TILE as f64 * STAR_TILE as f64)
            / area)
            .round() as usize;

        let mut pixels = vec![0u8; (STAR_TILE * STAR_TILE * 4) as usize];
        for _ in 0..n {
            let x = (rng.gen::<f64>() * STAR_TILE as f64) as u32;
            let y = (rng.gen::<f64>() * STAR_TILE as f64) as u32;
            let alpha = (127.0 + rng.gen::<f64>() * 128.0) as u8;
            let idx = ((y * STAR_TILE + x) * 4) as usize;
            pixels[idx] = 255;
            pixels[idx + 1] = 255;
            pixels[idx + 2] = 255;
            pixels[idx + 3] = alpha;
        }
        let texture = Texture2D::from_rgba8(STAR_TILE as u16, STAR_TILE as u16, &pixels);
        // filtre le plus proche : les étoiles font 1 texel — le filtre linéaire
        // (défaut) échantillonne entre les texels et les rend quasi invisibles
        // (luminosité ~1 au lieu de 127-255) ; le tile est dessiné à des
        // positions fractionnaires (offset de caméra).
        texture.set_filter(FilterMode::Nearest);
        layers.push(texture);
    }
    layers
}

// ─── Étoiles ─────────────────────────────────────────────────────────────────

/// Dessine les étoiles : chaque couche est une tuile répétée avec un offset
/// `camera × plan` (parallaxe), comme `pt = (star + camera) * plan` de
/// l'original. La périodicité de la tuile équivaut au rebouclage torique.
pub fn draw_stars(assets: &Assets, camera: Point) {
    for (layer, texture) in assets.star_layers.iter().enumerate() {
        let plan = (layer + 1) as f32;
        let tile = STAR_TILE as f32;
        let offset_x = (camera.x as f32 * plan).rem_euclid(tile);
        let offset_y = (camera.y as f32 * plan).rem_euclid(tile);

        // tuiles couvrant tout l'écran : partir de `offset - tile` (sinon la
        // zone avant `offset` — souvent la moitié de l'écran — reste sans
        // étoiles, voire l'écran entier pour les plans à grand offset)
        let mut ty = offset_y - tile;
        while ty < VIEWPORT_HEIGHT as f32 {
            let mut tx = offset_x - tile;
            while tx < VIEWPORT_WIDTH as f32 {
                draw_texture(texture, tx, ty, WHITE);
                tx += tile;
            }
            ty += tile;
        }
    }
}

// ─── Boîte de choix (accostage, ex windowUtils_choiceBox) ────────────────────

/// Couleurs de la boîte de choix (ex `windowUtils_choiceBox`) : fg = 0xFF99DFFF,
/// hover = 0xFFFFFFFF, fond = 0xD01AB2FF, bordure = 0xFF1AB2FF.
const BOX_FG: u32 = 0xFF99DFFF;
const BOX_HOVER: u32 = 0xFFFFFFFF;
const BOX_BG: u32 = 0xD01AB2FF;
const BOX_BORDER: u32 = 0xFF1AB2FF;
const BOX_PADDING: f32 = 10.0;

/// Géométrie de la boîte de choix UNLOAD/CLOSE (ex `windowUtils_choiceBox`) :
/// fenêtre de 120 px de haut centrée sur l'écran, largeur `max(300, msg+20)`,
/// deux boutons côte à côte en bas. Renvoie les rectangles écran des boutons
/// UNLOAD et CLOSE (pour la détection de clic côté logique).
pub fn choice_box_layout() -> (Rect, Rect) {
    let msg = "*** DOCK STATION ***";
    let msg_w = measure_text(msg, None, 16, 1.0).width + 2.0 * BOX_PADDING;
    let w = 300.0f32.max(msg_w);
    let h = 120.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // bouton : largeur = max(largeur texte + 2*padding, 60), hauteur = 26
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let btn_h = 26.0;
    let w1 = btn_w("UNLOAD");
    let w2 = btn_w("CLOSE");
    // positions : 1er sur la moitié gauche, 2e sur la moitié droite
    // (ex `(w\2-pw)\2 - padding` et `(3*w\2-pw)\2 - padding`)
    let left1 = left + (w / 2.0 - w1) / 2.0 - BOX_PADDING;
    let left2 = left + (3.0 * w / 2.0 - w2) / 2.0 - BOX_PADDING;
    let top_btn = top + h - 20.0 - btn_h;
    let unload = Rect::new(left1, top_btn, w1, btn_h);
    let close = Rect::new(left2, top_btn, w2, btn_h);
    (unload, close)
}

/// Dessine la boîte de choix UNLOAD/CLOSE (accostage) avec ses deux boutons
/// (hover = blanc, ex `windowUtils_choiceBox`).
#[allow(dead_code)] // appelé par main.rs quand `state.dock_box`
pub fn draw_choice_box() {
    let msg = "*** DOCK STATION ***";
    let msg_w = measure_text(msg, None, 16, 1.0).width + 2.0 * BOX_PADDING;
    let w = 300.0f32.max(msg_w);
    let h = 120.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    // deux boutons avec survol
    let (unload, close) = choice_box_layout();
    draw_box_button("UNLOAD", unload);
    draw_box_button("CLOSE", close);
}

/// Dessine un bouton de la boîte de choix (cadre + texte centré, hover blanc).
fn draw_box_button(label: &str, rect: Rect) {
    let m = mouse_to_game();
    let hovered = rect.contains(m);
    let color = argb_to_color(if hovered { BOX_HOVER } else { BOX_FG });
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, color);
    let text_w = measure_text(label, None, 16, 1.0).width;
    draw_text(label, rect.x + (rect.w - text_w) / 2.0, rect.y + 18.0, 16.0, color);
}

// ─── Fenêtre d'aide (touche S, ex windowUtils_help) ─────────────────────────

/// Géométrie du bouton CLOSE de la fenêtre d'aide (ex `windowUtils_createButton`
/// avec `left=20, bottom=20`) : fenêtre 320×240 centrée, bouton en bas à
/// gauche. Renvoie le rectangle écran du bouton (pour la détection de clic
/// côté logique).
pub fn help_box_layout() -> Rect {
    let w = 320.0;
    let h = 240.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    // buttonWidth = max(len("CLOSE")*8 + 2*padding, 60) ; buttonHeight = 16+10
    let btn_w = (5.0 * 8.0 + 2.0 * BOX_PADDING).max(60.0);
    let btn_h = 26.0;
    let btn_left = left + 20.0;
    let btn_top = top + h - 20.0 - 20.0;
    Rect::new(btn_left, btn_top, btn_w, btn_h)
}

/// Dessine la fenêtre d'aide (touche S, ex `help` de windowUtils) : fond,
/// bordure, libellés des touches et bouton CLOSE (hover blanc).
pub fn draw_help_box() {
    let w = 320.0;
    let h = 240.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // libellés des touches (ex windowUtils_createLabel, à 10 px de gauche,
    // 16 px d'écart) — la touche T est listée mais non implémentée dans
    // l'original (bloc commenté), on la conserve telle quelle
    let labels = [
        "P : pause",
        "S : show keys (this screen)",
        "T : dump triangles to console",
        "A : switch automatic shape generation",
        "D : display data",
        "F : switch fullscreen",
        "G : generate a shape",
        "K : kill all shapes",
    ];
    for (i, label) in labels.iter().enumerate() {
        draw_text(
            label,
            left + 10.0,
            top + 10.0 + 16.0 * i as f32 + 12.0,
            16.0,
            argb_to_color(BOX_FG),
        );
    }

    // position de la souris dans la fenêtre (ex lbl2 mis à jour par la boucle)
    let m = mouse_to_game();
    let coords = format!("{},{}", (m.x - left) as i32, (m.y - top) as i32);
    draw_text(&coords, left + 240.0, top + 5.0 + 12.0, 16.0, argb_to_color(BOX_FG));

    // bouton CLOSE
    draw_box_button("CLOSE", help_box_layout());
}

// ─── Caméra ──────────────────────────────────────────────────────────────────

/// Caméra centrée sur le joueur (ex mainLoop) : `W/2 - (pos + center)` puis
/// rebouclée dans le monde torique.
pub fn camera_for(state: &GameState, player: &Shape) -> Point {
    let mut camera = Point::new(
        VIEWPORT_WIDTH / 2.0 - (player.position.x + player.center.x),
        VIEWPORT_HEIGHT / 2.0 - (player.position.y + player.center.y),
    );
    camera.normalize_world(&state.world);
    camera
}

// ─── Zoom plein écran (touche F) ────────────────────────────────────────────

/// Échelle du zoom : la vue 960×540 est étirée pour remplir la fenêtre en
/// conservant le ratio (letterbox), ex l'exemple `letterbox.rs` de macroquad.
pub fn zoom_scale() -> f32 {
    (screen_width() / VIEWPORT_WIDTH as f32).min(screen_height() / VIEWPORT_HEIGHT as f32)
}

/// Rectangle occupé par la vue 960×540 dans la fenêtre (après zoom, centré).
pub fn zoom_rect() -> Rect {
    let scale = zoom_scale();
    let w = VIEWPORT_WIDTH as f32 * scale;
    let h = VIEWPORT_HEIGHT as f32 * scale;
    Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h)
}

/// Position souris de la fenêtre convertie en coordonnées du jeu (960×540),
/// pour que les clics/hovers des boîtes restent corrects en plein écran.
pub fn mouse_to_game() -> Vec2 {
    let scale = zoom_scale();
    let r = zoom_rect();
    vec2(
        (mouse_position().0 - r.x) / scale,
        (mouse_position().1 - r.y) / scale,
    )
}

/// Caméra de rendu vers la vue virtuelle 960×540 (ex `letterbox.rs`).
pub fn virtual_camera(rt: &RenderTarget) -> Camera2D {
    Camera2D {
        render_target: Some(rt.clone()),
        ..Camera2D::from_display_rect(Rect::new(
            0.0,
            0.0,
            VIEWPORT_WIDTH as f32,
            VIEWPORT_HEIGHT as f32,
        ))
    }
}

/// Affiche la texture de la vue virtuelle dans la fenêtre, zoomée (letterbox,
/// `flip_y` obligatoire : le render target est stocké à l'envers).
pub fn draw_zoomed(rt: &RenderTarget) {
    set_default_camera();
    clear_background(BLACK); // couleur des bandes letterbox
    let r = zoom_rect();
    draw_texture_ex(
        &rt.texture,
        r.x,
        r.y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(r.w, r.h)),
            flip_y: true,
            ..Default::default()
        },
    );
}

// ─── Primitives ──────────────────────────────────────────────────────────────

/// Limite de dessin (ex `innerDrawLimit` de `meteorsMining.bas`).
pub fn inner_draw_limit(p: Point) -> bool {
    p.x >= DRAW_MINX && p.x <= DRAW_MAXX && p.y >= DRAW_MINY && p.y <= DRAW_MAXY
}

/// Position écran d'un point monde : translation par la caméra puis
/// rebouclage torique.
fn screen_point(p: Point, camera: Point, world: &World) -> Vec2 {
    let mut q = Point::new(p.x + camera.x, p.y + camera.y);
    q.normalize_world(world);
    vec2(q.x as f32, q.y as f32)
}

/// Triangle texturé (équivalent `_MapTriangle _seamless ... _smooth` de QB64).
///
/// Les UV sont normalisés dans [0,1] et repliés par modulo (`rem_euclid`),
/// ce qui reproduit le wrapping `_seamless` (voir `docs/PORTAGE.md` §6).
pub fn draw_triangle_texture(
    texture: &Texture2D,
    v1: Vec2,
    v2: Vec2,
    v3: Vec2,
    uv1: Vec2,
    uv2: Vec2,
    uv3: Vec2,
    color: Color,
) {
    let mesh = Mesh {
        vertices: vec![
            Vertex::new(v1.x, v1.y, 0.0, uv1.x, uv1.y, color),
            Vertex::new(v2.x, v2.y, 0.0, uv2.x, uv2.y, color),
            Vertex::new(v3.x, v3.y, 0.0, uv3.x, uv3.y, color),
        ],
        indices: vec![0, 1, 2],
        texture: Some(texture.clone()),
    };
    draw_mesh(&mesh);
}

/// Ligne en pointillés (approximation du motif `&B1010101010101010` de QB64
/// pour les triangles morts — un pixel sur deux).
fn draw_dashed_line(a: Vec2, b: Vec2, color: Color) {
    let d = b - a;
    let len = d.length();
    let steps = len as i32;
    if steps <= 0 {
        return;
    }
    let dir = d / len;
    for i in (0..=steps).step_by(2) {
        draw_rectangle(a.x + dir.x * i as f32, a.y + dir.y * i as f32, 1.0, 1.0, color);
    }
}

// ─── Formes ──────────────────────────────────────────────────────────────────

/// Dessine une forme : minimap (option), puis ses triangles vivants
/// (texturés ou non) (ex `drawShape`).
///
/// NB : l'original recalcule `getBorderSegments` à chaque frame (inutile :
/// les bords ne servent qu'à la génération et au debug) — on ne le fait pas.
pub fn draw_shape(
    state: &GameState,
    assets: &Assets,
    shape: &Shape,
    triangles: &mut [Triangle],
    camera: Point,
    elements: &[Element],
    show_data: bool,
) {
    if shape.life <= 0 {
        return;
    }

    // mode D (ex options = "D" de drawShape) : les indicateurs de bord des
    // triangles sont recalculés (comme l'original qui appelle
    // getBorderSegments à chaque frame — on ne le fait que si affiché)
    if show_data {
        get_border_segments(shape, triangles);
    }

    // minimap (option SHOW_GLOBAL_MAP)
    if SHOW_GLOBAL_MAP {
        let mut p = Point::new(shape.position.x + camera.x, shape.position.y + camera.y);
        p.normalize_world(&state.world);
        // NB : l'original calcule une couleur `c&` (inutilisée) ; le point est
        // dessiné avec `shape.shapeColor`.
        let x = (p.x / 10.0) as i32 + (VIEWPORT_WIDTH / 2.0 - VIEWPORT_WIDTH / 20.0) as i32;
        let y = (p.y / 10.0) as i32 + (VIEWPORT_HEIGHT / 2.0 - VIEWPORT_HEIGHT / 20.0) as i32;
        draw_circle(x as f32, y as f32, 1.0, argb_to_color(shape.shape_color));
    }

    for i in shape.first_triangle..=shape.last_triangle {
        let t = &triangles[i];
        let p = screen_point(t.real_center, camera, &state.world);
        if shape.show_all_parts
            || (t.life > 0
                && inner_draw_limit(Point::new(p.x as f64, p.y as f64)))
        {
            if shape.texture != TEXTURE_NONE {
                draw_textured_triangle(assets, t, shape, camera, elements, &state.world);
            } else {
                draw_triangle(assets, t, shape, camera, elements, &state.world);
            }
        }
    }

    // mode D (ex options = "D" de drawShape) : id, indices de triangles puis
    // vie/bords de chaque triangle, à la position écran de la forme
    if show_data {
        let mut p = Point::new(shape.position.x + camera.x, shape.position.y + camera.y);
        p.normalize_world(&state.world);
        let x = p.x as f32;
        let y = p.y as f32;
        let header = format!(
            "{}:{},{}",
            shape.id, shape.first_triangle, shape.last_triangle
        );
        draw_text(&header, x, y + 12.0, 16.0, WHITE);
        for (k, i) in (shape.first_triangle..=shape.last_triangle).enumerate() {
            let t = &triangles[i];
            let border = |b: bool| if b { -1 } else { 0 };
            let line = format!(
                "{}/{}/{}/{}",
                t.life,
                border(t.a_shape_border),
                border(t.b_shape_border),
                border(t.c_shape_border),
            );
            draw_text(&line, x, y + 12.0 + (k + 1) as f32 * 10.0, 16.0, WHITE);
        }
    }
}

/// Triangle texturé (ex `drawTexturedTriangle`) : UV dérivés de la géométrie
/// locale (`u = t.a.x*ratio - tw/2`, `ratio = tw / max(shape.width, height)`),
/// mapping vers les sommets écran après caméra + wrap.
fn draw_textured_triangle(
    assets: &Assets,
    t: &Triangle,
    shape: &Shape,
    camera: Point,
    elements: &[Element],
    world: &World,
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);
    if t.life <= 0 {
        return;
    }

    let texture = match shape.texture {
        TEXTURE_PLAYER => &assets.player,
        TEXTURE_METEOR => &assets.meteor,
        TEXTURE_STATION => &assets.station,
        _ => &assets.orange,
    };
    let tw = texture.width() as f64;
    let larger = shape.width.max(shape.height);
    let ratio = tw / larger;
    let uv = |x: f64, y: f64| {
        if shape.who_i_am == WHOIAM_STATION {
            // NB station : `station.png` est un anneau fin (bord intérieur UV
            // ~0.34, extérieur ~0.5), plus étroit que la bande du mesh (rayon
            // 90-163). Une simple échelle fait tomber les dents cardinales
            // sur le pixel vide du bord (défauts à droite et en bas) ou les
            // creux dans le trou. Mapping radial : la bande du mesh est
            // compressée dans la bande pleine de la texture (UV 0.36-0.48).
            let r = x.hypot(y);
            if r < 1.0 {
                return vec2(0.5, 0.5);
            }
            let t = ((r - STATION_UV_R_INNER)
                / (STATION_UV_R_OUTER - STATION_UV_R_INNER))
            .clamp(0.0, 1.0);
            let rho = STATION_UV_INNER + t * (STATION_UV_OUTER - STATION_UV_INNER);
            vec2(
                (0.5 + (x / r) * rho).rem_euclid(1.0) as f32,
                (0.5 + (y / r) * rho).rem_euclid(1.0) as f32,
            )
        } else {
            vec2(
                ((x * ratio - tw / 2.0) / tw).rem_euclid(1.0) as f32,
                ((y * ratio - tw / 2.0) / tw).rem_euclid(1.0) as f32,
            )
        }
    };
    let uv_a = uv(t.a.x, t.a.y);
    let uv_b = uv(t.b.x, t.b.y);
    let uv_c = uv(t.c.x, t.c.y);
    draw_triangle_texture(texture, a, b, c, uv_a, uv_b, uv_c, WHITE);

    if t.element > 0 {
        let center = screen_point(t.real_center, camera, world);
        draw_circle(
            center.x,
            center.y,
            1.2,
            argb_to_color(elements[t.element as usize].color),
        );
    }
}

/// Triangle sans texture (ex `drawTriangle`) : mappe en dur le triangle
/// `(511,511)-(0,511)-(255,0)` de `orange2.png` (replié sur la vraie taille
/// de la texture, équivalent `_seamless`). Les triangles morts sont dessinés
/// en pointillés.
fn draw_triangle(
    assets: &Assets,
    t: &Triangle,
    shape: &Shape,
    camera: Point,
    elements: &[Element],
    world: &World,
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);

    if t.life > 0 {
        let texture = &assets.orange;
        let tw = texture.width() as f64;
        let th = texture.height() as f64;
        // coordonnées en dur de l'original, repliées par _seamless
        let uv = |x: f64, y: f64| {
            vec2(
                (x.rem_euclid(tw) / tw) as f32,
                (y.rem_euclid(th) / th) as f32,
            )
        };
        draw_triangle_texture(
            texture,
            a,
            b,
            c,
            uv(511.0, 511.0),
            uv(0.0, 511.0),
            uv(255.0, 0.0),
            WHITE,
        );
    } else {
        let color = if t.collid {
            argb_to_color(shape.shape_color & 0x70FFFFFF)
        } else {
            argb_to_color(shape.shape_color)
        };
        draw_dashed_line(a, b, color);
        draw_dashed_line(b, c, color);
        draw_dashed_line(c, a, color);
    }

    if t.element > 0 {
        let center = screen_point(t.real_center, camera, world);
        draw_circle(
            center.x,
            center.y,
            1.2,
            argb_to_color(elements[t.element as usize].color),
        );
    }
}

// ─── Poussée, débris, cargo, HUD ─────────────────────────────────────────────

/// Effet de poussée : 3 cercles dégradés le long de `orientation + angle`
/// (ex `ejectionFlow`). Angle `TAU/2` = avant (orange), `0` = recul (bleu).
pub fn ejection_flow(
    shape: &Shape,
    angle: f64,
    flow_color: u32,
    camera: Point,
    world: &World,
) {
    // NB : comme l'original, les rayons et le jitter sont tronqués en entiers
    let u01 = rand::rand() as f64 / u32::MAX as f64; // [0,1)
    let f = (u01 * 2.0 - 1.0).trunc();
    let r1 = (shape.radius + 3.0 + u01 * 3.0).trunc();
    let r2 = r1 + 6.0;
    let r3 = r2 + 4.0;
    let c = (shape.orientation + angle).cos();
    let s = (shape.orientation + angle).sin();
    let x = shape.position.x + shape.center.x + camera.x;
    let y = shape.position.y + shape.center.y + camera.y;
    let color = argb_to_color(flow_color);

    let mut p = Point::new(r1 * c + x, r1 * s + y);
    p.normalize_world(world);
    draw_circle(p.x as f32, p.y as f32, (1.0 + f) as f32, color);
    let mut p = Point::new(r2 * c + x, r2 * s + y);
    p.normalize_world(world);
    draw_circle(p.x as f32, p.y as f32, (2.0 + f) as f32, color);
    let mut p = Point::new(r3 * c + x, r3 * s + y);
    p.normalize_world(world);
    draw_circle(p.x as f32, p.y as f32, (3.0 + f) as f32, color);
}

/// Dessine un débris : pixel blanc 1×1 (ex `drawGarbage`).
pub fn draw_garbage(g: &Garbage, camera: Point, world: &World) {
    if g.life == 0 {
        return;
    }
    let p = screen_point(g.position, camera, world);
    if inner_draw_limit(Point::new(p.x as f64, p.y as f64)) {
        draw_rectangle(p.x, p.y, 1.0, 1.0, argb_to_color(g.rgba_color));
    }
}

/// Cargo : 5 cercles à `x = 11*i + 5`, `y = 50`, remplis de la couleur de
/// l'élément (GOLD, IRON puis WATER), vide = contour gris (ex mainLoop).
pub fn draw_cargo(state: &GameState, elements: &[Element]) {
    let e1 = elements[1].count;
    let e2 = e1 + elements[2].count;
    let e3 = e2 + elements[3].count;
    for i in 1..=state.player.cargo_size {
        let color = if i <= e1 {
            elements[1].color
        } else if i <= e2 {
            elements[2].color
        } else if i <= e3 {
            elements[3].color
        } else {
            0xFF808080
        };
        let x = 11.0 * i as f32 + 5.0;
        if color != 0xFF808080 {
            draw_circle(x, 50.0, 5.0, argb_to_color(color));
        } else {
            draw_circle_lines(x, 50.0, 5.0, 1.0, argb_to_color(color));
        }
    }
}

/// HUD : FPS, réputation, précision (ex `locate 1,1 / 1,15 / 1,30` de
/// mainLoop). Police macroquad par défaut en attendant la police 8×16
/// (Phase 4).
pub fn draw_hud(state: &GameState) {
    draw_text(&format!("FPS:{}", state.fps), 8.0, 14.0, 16.0, WHITE);
    draw_text(
        &format!("REPUTATION:{}", state.meteors_destroyed),
        8.0 + 14.0 * 8.0,
        14.0,
        16.0,
        WHITE,
    );
    if state.bullets_fired > 0 {
        let precision = 100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64);
        draw_text(
            &format!("PRECISION:{}%", precision as i32),
            8.0 + 29.0 * 8.0,
            14.0,
            16.0,
            WHITE,
        );
    }
}

// ─── Affichages de debug (touches D et I) ───────────────────────────────────

/// Affiche les informations de debug (touche I, ex `showInfo` de `mainLoop`) :
/// keycode, génération automatique, compteurs de formes/triangles/débris,
/// formes vivantes et niveaux des éléments.
///
/// NB : `ubound` de QB64 = `len-1` ; `locate r, c` = ligne r, colonne c avec
/// la police 8×16 (x = 8+(c-1)*8, y = 14+(r-1)*16, comme `draw_hud`).
pub fn draw_info(
    state: &GameState,
    shapes: &[Shape],
    triangles: &[Triangle],
    garbages: &[Garbage],
    elements: &[Element],
) {
    // formes vivantes : au moins un triangle vivant (ex boucle de dessin de
    // `mainLoop`, sans le nettoyage)
    let mut alive_shapes = 0;
    for i in 1..shapes.len() {
        if shapes[i].life <= 0 {
            continue;
        }
        let mut t = 0;
        for j in shapes[i].first_triangle..=shapes[i].last_triangle {
            if triangles[j].life > 0 {
                t += 1;
            }
        }
        if t > 0 {
            alive_shapes += 1;
        }
    }
    let alive_triangles = triangles.iter().filter(|t| t.life > 0).count();

    let white = WHITE;
    // ligne 1, colonne 10 : keycode
    draw_text(
        &format!("keycode:{}", state.last_keycode),
        8.0 + 9.0 * 8.0,
        14.0,
        16.0,
        white,
    );
    // ligne 2, colonne 1 : génération automatique
    draw_text(
        &format!("auto generate shape:{}", if state.auto_generate { "ON" } else { "OFF" }),
        8.0,
        14.0 + 16.0,
        16.0,
        white,
    );
    // ligne 1, colonne 30 : compteurs (ubound = len-1)
    draw_text(
        &format!(
            "shapes:{} - triangles:{} - garbages:{}",
            shapes.len() - 1,
            triangles.len() - 1,
            garbages.len() - 1,
        ),
        8.0 + 29.0 * 8.0,
        14.0,
        16.0,
        white,
    );
    // ligne 2, colonne 30 : formes et triangles vivants
    draw_text(
        &format!("alive shapes:{} - alive triangles:{}", alive_shapes, alive_triangles),
        8.0 + 29.0 * 8.0,
        14.0 + 16.0,
        16.0,
        white,
    );
    // ligne 3, colonne 1 : niveaux des éléments
    draw_text(
        &format!("{} {} {}", elements[1].count, elements[2].count, elements[3].count),
        8.0,
        14.0 + 2.0 * 16.0,
        16.0,
        white,
    );
}

/// Affiche les messages en bas de l'écran (ex `drawMessage` de `mainLoop`).
///
/// La file avance d'un message toutes les ~5 s : le message courant descend
/// d'une ligne (`message2`/`message1`/`message`) avec une opacité croissante
/// (0x70/0xA0/0xFF), comme l'original.
pub fn draw_message(state: &mut GameState) {
    // décrémente le délai (ex `1 / ctx.fps` par frame)
    state.message_delay -= 1.0 / state.fps.max(1) as f64;
    if state.message_delay < 0.0 {
        state.message_delay = 5.0;
        // extrait le prochain message de la file (séparateur '/')
        if let Some(p) = state.message_queue.find('/') {
            state.message2 = state.message1.clone();
            state.message1 = state.message.clone();
            state.message = state.message_queue[..p].to_string();
            state.message_queue = state.message_queue[p + 1..].to_string();
        }
    }

    // trois lignes en bas de l'écran, centrées horizontalement
    let lines = [
        (state.message2.as_str(), 0x7080FF80u32),
        (state.message1.as_str(), 0xA080FF80u32),
        (state.message.as_str(), 0xFF80FF80u32),
    ];
    for (i, (text, color)) in lines.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let width = measure_text(text, None, 16, 1.0).width;
        let x = (VIEWPORT_WIDTH as f32 - width) / 2.0;
        let y = VIEWPORT_HEIGHT as f32 - 16.0 * (3 - i) as f32;
        draw_text(text, x, y, 16.0, argb_to_color(*color));
    }
}
