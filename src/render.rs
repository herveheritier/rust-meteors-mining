//! Rendu.
//!
//! Portage de la partie rendu de `meteorsMining.bas` : chargement des assets
//! (textures + couches d'étoiles précalculées), dessin des étoiles, des
//! triangles (texturés ou non), des formes, de la poussée, des débris, du
//! cargo et du HUD.
//!
//! NB : macroquad 0.4 ne fournit pas de `draw_triangle_texture` (le plan
//! `docs/PORTAGE.md` le supposait) - on l'implémente ici via
//! `models::Mesh` + `draw_mesh`, qui utilisent la pipeline 2D et sa texture.

// Sous-modules issus du découpage de ce fichier (voir `main.rs`) : le rendu
// reste accessible par `render::…` pour tous les appelants (ré-export) ;
// `hud` = jauge/cargo/messages/objectifs, `dock_render` = station, liens et
// cordon EVA, `shop_render` = magasin à onglets, `ui_boxes` = aide et
// paramétrage.
pub use crate::dock_render::*;
pub use crate::hud::*;
pub use crate::shop_render::*;
pub use crate::ui_boxes::*;

use macroquad::models::{draw_mesh, Mesh, Vertex};
use macroquad::prelude::*;
use ::rand::Rng;

use crate::config::*;
use crate::font::draw_text;
use crate::garbage::Garbage;
use crate::geom::{Point, Triangle, World};
use crate::shape::{get_border_segments, Shape};
use crate::state::{Element, GameState, RadarEcho, RenderStyle, ViewMode};

/// Taille d'une tuile d'étoiles précalculée (pixels monde = pixels écran).
pub const STAR_TILE: u32 = 1024;

// ─── Couleurs ────────────────────────────────────────────────────────────────

/// Convertit une couleur ARGB 32 bits QB64 (AARRGGBB) en `Color` macroquad
/// (RGBA). NB : l'ordre des octets change - voir `docs/PORTAGE.md` §6.
pub fn argb_to_color(argb: u32) -> Color {
    Color::new(
        ((argb >> 16) & 0xFF) as f32 / 255.0,
        ((argb >> 8) & 0xFF) as f32 / 255.0,
        (argb & 0xFF) as f32 / 255.0,
        ((argb >> 24) & 0xFF) as f32 / 255.0,
    )
}

/// Dessine un texte avec une ombre portée pour améliorer la lisibilité sur
/// le fond de jeu (utilisé par le HUD d'objectifs et les boîtes de dialogue).
pub(crate) fn draw_text_shadow(text: &str, x: f32, y: f32, font_size: f32, color: Color) {
    draw_text(text, x + 1.0, y + 1.0, font_size, Color::new(0.0, 0.0, 0.0, 0.65));
    draw_text(text, x, y, font_size, color);
}

// ─── Assets ──────────────────────────────────────────────────────────────────

/// Une couche d'étoiles : positions (dans la tuile) + alpha de chaque étoile.
///
/// NB : on ne garde PAS de texture par couche - dessiner 15 tuiles 1024² avec
/// blending coûtait ~60 % du temps de frame (fill rate du GPU) ; chaque étoile
/// est dessinée individuellement en 1 px (quadrant blanc batché), comme les
/// `pset` de l'original, pour un coût GPU négligeable (voir `draw_stars`).
pub struct StarLayer {
    /// (x, y) dans la tuile [0, STAR_TILE), alpha [127, 255]
    pub stars: Vec<(f32, f32, f32)>,
}

/// Assets chargés au démarrage (ex `_loadimage` de `meteorsMining.bas`).
pub struct Assets {
    pub orange: Texture2D,
    pub player: Texture2D,
    pub meteor: Texture2D,
    pub station: Texture2D,
    /// 15 couches de parallaxe, précalculées une seule fois : c'est
    /// l'optimisation « étoiles » du plan (100× plus rapide que les
    /// 100 000 `pset` par frame de l'original).
    pub star_layers: Vec<StarLayer>,
}

impl Assets {
    /// Charge les 4 textures depuis `assets/` (intégrées au binaire via
    /// `include_bytes!`). NB : la texture météore est embarquée en JPEG - c'est
    /// l'asset d'origine (`reference/assets/meteor_surface_tile.jpg`) - d'où la
    /// feature `jpeg` de la crate `image` dans `Cargo.toml`.
    pub async fn load() -> Assets {
        // Textures intégrées dans le binaire (`include_bytes!`) : l'exécutable
        // est autonome, le dossier `assets/` n'est plus nécessaire au runtime.
        // Modding : un fichier `user_assets/<nom>` remplace l'asset embarqué
        // (voir `modding.rs` - le format est déduit de l'extension du fichier).
        let orange = Texture2D::from_file_with_format(
            &crate::modding::asset_bytes("orange2.png", include_bytes!("../assets/orange2.png")),
            Some(crate::modding::image_format_for("orange2.png")),
        );
        let player = Texture2D::from_file_with_format(
            &crate::modding::asset_bytes("vaisseau.png", include_bytes!("../assets/vaisseau.png")),
            Some(crate::modding::image_format_for("vaisseau.png")),
        );
        let meteor = Texture2D::from_file_with_format(
            &crate::modding::asset_bytes(
                "meteor_surface_tile.jpg",
                include_bytes!("../assets/meteor_surface_tile.jpg"),
            ),
            Some(crate::modding::image_format_for("meteor_surface_tile.jpg")),
        );
        let station = Texture2D::from_file_with_format(
            &crate::modding::asset_bytes("station.png", include_bytes!("../assets/station.png")),
            Some(crate::modding::image_format_for("station.png")),
        );

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
/// génère un champ uniforme par couche avec la **même densité** : un champ
/// aléatoire uniforme est statistiquement identique à l'original (le plan
/// PORTAGE.md préconise cette optimisation). Chaque étoile est stockée par sa
/// position dans une tuile périodique (équivalent au rebouclage torique).
fn build_star_layers() -> Vec<StarLayer> {
    let mut rng = crate::generate::seeded_rng();
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

        let mut stars = Vec::with_capacity(n);
        for _ in 0..n {
            let x = (rng.r#gen::<f64>() * STAR_TILE as f64) as f32;
            let y = (rng.r#gen::<f64>() * STAR_TILE as f64) as f32;
            let alpha = (127.0 + rng.r#gen::<f64>() * 128.0) as f32;
            stars.push((x, y, alpha));
        }
        layers.push(StarLayer { stars });
    }
    layers
}

// ─── Étoiles ─────────────────────────────────────────────────────────────────

/// Dessine les étoiles : chaque couche est décalée de `camera × plan`
/// (parallaxe), comme `pt = (star + camera) * plan` de l'original ; la
/// périodicité de la tuile équivaut au rebouclage torique. Chaque étoile est
/// un pixel 1×1 (quadrant blanc batché - un seul draw call pour toutes les
/// étoiles d'une couche), au lieu de dessiner la tuile 1024² entière avec
/// blending : le rendu des tuiles coûtait ~60 % du temps de frame (fill rate
/// du GPU virtio) et plafonnait le FPS à ~95 (au lieu de ~220 sans étoiles).
/// Position écran d'une étoile : `(étoile + caméra) × plan` rebouclée dans la
/// tuile périodique (torique), ex `normalizePlanPosition` de l'original.
/// Renvoie `None` si l'étoile est hors viewport.
///
/// NB : la caméra recule quand le vaisseau avance (ex `W/2 - pos`) - le signe
/// `+` fait donc défiler les étoiles en sens INVERSE du vaisseau (parallaxe
/// correcte) ; un `-` les ferait défiler dans son sens (bug historique du
/// port, voir `docs/PORTAGE.md` §6).
fn star_screen_pos(sx: f32, sy: f32, camera: Point, plan: f32) -> Option<(f32, f32)> {
    let tile = STAR_TILE as f32;
    let offset_x = (camera.x as f32 * plan).rem_euclid(tile);
    let offset_y = (camera.y as f32 * plan).rem_euclid(tile);
    let mut x = sx + offset_x;
    if x >= tile {
        x -= tile;
    }
    if x >= VIEWPORT_WIDTH as f32 {
        return None;
    }
    let mut y = sy + offset_y;
    if y >= tile {
        y -= tile;
    }
    if y >= VIEWPORT_HEIGHT as f32 {
        return None;
    }
    Some((x, y))
}

/// Dessine les étoiles de fond. `reduced` diminue la densité d'un facteur
/// `STAR_DENSITY_REDUCTION` : utilisé quand une fenêtre modale (magasin,
/// paramètres, aide, boîte DOCK) couvre l'écran - le monde continue de
/// tourner derrière (monde vivant), mais l'œil est sur la fenêtre : la
/// densité réduite est imperceptible et économise la part GPU du fond
/// (~50 % du temps de frame sur GPU lents, ex virtio - voir la doc de
/// `StarLayer`). L'échantillonnage régulier (`step_by`) garde une répartition
/// uniforme des étoiles restantes. `size` est la taille d'une étoile en px :
/// 1×1 par défaut, 3×3 quand la case STARS 3x3 de l'écran de paramétrage est
/// cochée (visibilité du champ d'étoiles selon la qualité de l'écran).
pub fn draw_stars(assets: &Assets, camera: Point, reduced: bool, size: f32) {
    let step = if reduced { STAR_DENSITY_REDUCTION } else { 1 };
    for (layer, plan_layer) in assets.star_layers.iter().enumerate() {
        let plan = (layer + 1) as f32;
        for (i, &(sx, sy, alpha)) in plan_layer.stars.iter().enumerate() {
            if i % step != 0 {
                continue;
            }
            // position écran : (étoile + caméra) × plan, rebouclée dans la
            // tuile (torique), comme le `normalizePlanPosition` de l'original ;
            // on élimine les étoiles hors viewport.
            if let Some((x, y)) = star_screen_pos(sx, sy, camera, plan) {
                draw_rectangle(x.round(), y.round(), size, size, Color::new(1.0, 1.0, 1.0, alpha / 255.0));
            }
        }
    }
}

// ─── Boîte de choix (accostage, ex windowUtils_choiceBox) ────────────────────

/// Couleurs des fenêtres (boîte de choix, aide, paramétrage ; ex
/// `windowUtils_choiceBox`). NB dérive volontaire pour le contraste : le
/// texte passe de `0xFF99DFFF` à un bleu presque blanc `0xFFD6EEFF` et le
/// fond est assombri (`0xD01478DC` au lieu de `0xD01AB2FF`) pour que les
/// libellés se détachent nettement.
pub(crate) const BOX_FG: u32 = 0xFFD6EEFF;
/// Texte secondaire (descriptions des modes, valeur du volume) : plus clair
/// que l'ancien `0xB099DFFF` (illisible), mais volontairement plus discret
/// que `BOX_FG`.
pub(crate) const BOX_FG_DIM: u32 = 0xFFC2E4FF;
pub(crate) const BOX_HOVER: u32 = 0xFFFFFFFF;
pub(crate) const BOX_BG: u32 = 0xD01478DC;
pub(crate) const BOX_BORDER: u32 = 0xFF1AB2FF;
/// Panneau interne de l'écran de paramétrage (panneau « GRAPHICS ») :
/// fond légèrement plus clair que la fenêtre, bordure discrète.
pub(crate) const BOX_PANEL_BG: u32 = 0xE01478DC;
pub(crate) const BOX_PANEL_BORDER: u32 = 0x801AB2FF;
pub(crate) const BOX_PADDING: f32 = 10.0;

/// Largeur de la boîte DOCK STATION : assez pour le titre et les boutons
/// (DÉCHARGER / MARCHÉ / QUITTER) sans chevauchement - même formule pour la
/// géométrie (`choice_box_layout`) et le dessin (`draw_choice_box`).
pub(crate) const CHOICE_BOX_LABELS: [&str; 3] = ["DÉCHARGER", "MARCHÉ", "QUITTER"];

// ─── Magasin de la station (bouton MARCHÉ) ──────────────────────────────────

/// Couleurs d'état du magasin : prix abordable / achat confirmé (vert),
/// refus / crédits insuffisants (rouge).
pub(crate) const SHOP_OK: u32 = 0xFF39FF88;
pub(crate) const SHOP_ERR: u32 = 0xFFFF5A5A;
/// Dimensions de la fenêtre du magasin : largeur fixe, hauteur selon l'onglet
/// actif (le contenu tient toujours à l'écran).
pub(crate) const SHOP_W: f32 = 580.0;
/// Hauteur d'une ligne de contenu du magasin (étiquette + détail/curseur).
pub(crate) const SHOP_ROW_H: f32 = 38.0;
/// Largeur/hauteur d'un bouton « pilule » d'action (ACHETER / sélection).
pub(crate) const SHOP_PILL_W: f32 = 118.0;
pub(crate) const SHOP_PILL_H: f32 = 24.0;
/// Hauteur de la rangée d'onglets.
pub(crate) const SHOP_TAB_H: f32 = 26.0;
/// Hauteur du panneau d'**aperçu du vaisseau** (onglet ÉQUIPEMENT).
pub(crate) const SHOP_PREVIEW_H: f32 = 120.0;

// ─── Fenêtre d'aide (touche S, ex windowUtils_help) ─────────────────────────

// ─── Écran de paramétrage (touche O) ────────────────────────────────────────


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

/// La fenêtre est-elle plus grande que la vue 960×540 ? (définition choisie
/// dans l'écran de paramétrage, ou plein écran). En fenêtré, la vue est alors
/// rendue dans la texture virtuelle puis étirée (letterbox), comme en plein
/// écran zoomé - sinon elle est dessinée 1:1. `screen_width/height` renvoie
/// des pixels logiques (dpi divisé), la comparaison est donc directe.
pub fn window_scaled() -> bool {
    screen_width() != VIEWPORT_WIDTH as f32 || screen_height() != VIEWPORT_HEIGHT as f32
}

/// Position écran (px fenêtre) convertie en coordonnées du jeu (960×540) -
/// même mapping letterbox que `mouse_to_game` : utilisé par les touches de
/// l'interface tactile (`touch.rs`), dont les positions sont aussi en px.
pub fn screen_to_game(pos: Vec2) -> Vec2 {
    let scale = zoom_scale();
    let r = zoom_rect();
    vec2((pos.x - r.x) / scale, (pos.y - r.y) / scale)
}

/// Position souris de la fenêtre convertie en coordonnées du jeu (960×540),
/// pour que les clics/hovers des boîtes restent corrects en plein écran.
pub fn mouse_to_game() -> Vec2 {
    screen_to_game(vec2(mouse_position().0, mouse_position().1))
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

/// Caméra de rendu direct du plein écran **natif** (touche F, 3e mode) : la
/// vue 960×540 est affichée à la définition réelle de l'écran SANS passer par
/// un render target - un seul passage de rendu à la résolution native (plus
/// net, moins de fill que le double passage rendu + étirement).
///
/// La rect monde visible = écran/scale (`scale = min(W/960, H/540)`, uniforme
/// avec letterbox) : sur un écran 16:9 c'est exactement 960×540 ; sinon la
/// vue reste au centre avec des bandes noires, comme en mode `Zoomed`.
///
/// NB - signe de `zoom.y` : le rendu direct à l'écran (render target = None)
/// inverse l'axe y (`invert_y = -1` dans `camera.rs`), contrairement au rendu
/// dans un render target (`invert_y = +1`, ex `virtual_camera`). Pour un écran
/// (y=0 en haut), `zoom.y` doit donc être POSITIF - un `-` (copié de
/// `from_display_rect`) retourne la scène verticalement.
pub fn native_camera() -> Camera2D {
    let scale = zoom_scale();
    let view_w = screen_width() / scale;
    let view_h = screen_height() / scale;
    Camera2D {
        render_target: None,
        zoom: vec2(2.0 / view_w, 2.0 / view_h),
        target: vec2(view_w / 2.0, view_h / 2.0),
        offset: vec2(0.0, 0.0),
        ..Default::default()
    }
}

/// Sort du plein écran EWMH : ClientMessage REMOVE via libX11 - miniquad
/// 0.4.11 ne sait PAS sortir du plein écran sur X11 (`set_fullscreen(false)`
/// envoie un ADD avec un atome vide, sans effet - TODO de miniquad, toujours
/// présent en master) → on envoie nous-mêmes le REMOVE EWMH
/// (`crate::x11::set_fullscreen(false)`), sans outil externe (`wmctrl`). Sans
/// WM EWMH, repli sur un simple redimensionnement à la définition de la vue
/// (avec un WM non EWMH, la fenêtre resterait plein écran).
///
/// NB : on n'appelle PAS `set_fullscreen(false)` de miniquad : en plus
/// d'envoyer un ADD avec un atome vide (sans effet), il fait un
/// `XUnmapWindow/XMapWindow` de la fenêtre qui interfère avec notre
/// ClientMessage REMOVE (le WM peut re-appliquer le plein écran au remap).
fn exit_fullscreen() {
    if !crate::x11::set_fullscreen(false) {
        request_new_screen_size(VIEWPORT_WIDTH as f32, VIEWPORT_HEIGHT as f32);
    }
}

/// Entre en plein écran EWMH **proprement** : ClientMessage `_NET_WM_STATE`
/// ADD via libX11 (`crate::x11::set_fullscreen(true)`), sans unmap/remap -
/// celui de miniquad (`set_fullscreen(true)`) fait `XUnmapWindow`/
/// `XMapWindow` + `XSync` : la fenêtre vacille (le focus peut quitter le jeu
/// le temps de la bascule) et la touche F relâchée pendant cette fenêtre est
/// perdue - la pression suivante est alors avalée par le filtre de répétition
/// de macroquad (il faut presser F deux fois pour changer de mode). Repli sur
/// miniquad si l'X11 direct n'est pas joignable (hors Linux, display absent).
pub fn enter_fullscreen() {
    if !crate::x11::set_fullscreen(true) {
        set_fullscreen(true);
    }
}

/// Fait cycler le mode d'affichage (touche F) : fenêtré → plein écran zoomé
/// (EWMH, render target étirée) → plein écran natif (EWMH, définition réelle
/// de l'écran, sans buffer) → fenêtré. NB : la bascule zoomé → natif ne
/// change rien de visible (deux pleins écrans, même image) - le HUD annonce
/// le mode (« FULLSCREEN (ZOOMED) » / « FULLSCREEN (NATIVE) »).
///
/// Entrée dans les pleins écrans : `enter_fullscreen` (ClientMessage EWMH
/// ADD propre, repli miniquad). Retour fenêtré : `exit_fullscreen` (REMOVE
/// EWMH via libX11, repli redimensionnement).
pub fn cycle_view_mode(state: &mut GameState) {
    state.view_mode = match state.view_mode {
        ViewMode::Windowed => {
            enter_fullscreen();
            ViewMode::Zoomed
        }
        // le plein écran EWMH est déjà actif : seul le chemin de rendu change
        // (render target étirée → rendu direct natif)
        ViewMode::Zoomed => ViewMode::Native,
        ViewMode::Native => {
            exit_fullscreen();
            ViewMode::Windowed
        }
    };
    // le dernier mode utilisé est persisté : le jeu redémarre dedans
    let _ = crate::persist::save_view_mode(state.view_mode as i32);
}

/// Persiste la position et la taille **réelles** de la fenêtre fenêtrée quand
/// elles changent (déplacement ou redimensionnement par le WM) : écriture du
/// fichier de config une seule fois par changement. La position est lue via
/// X11 (`x11::window_position`, indisponible hors Linux : seule la taille est
/// alors persistée). Appelé périodiquement par les boucles titre et jeu (au
/// plus une vérification par seconde - l'ouverture du display X coûte).
pub fn persist_window_geometry(state: &GameState) {
    if state.view_mode != ViewMode::Windowed {
        return;
    }
    static LAST_CHECK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now_ms = (get_time() * 1000.0) as u64;
    if now_ms.saturating_sub(LAST_CHECK.load(std::sync::atomic::Ordering::Relaxed)) < 1000 {
        return;
    }
    LAST_CHECK.store(now_ms, std::sync::atomic::Ordering::Relaxed);

    let w = screen_width() as i32;
    let h = screen_height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let size_changed = crate::persist::load_window_px_size() != Some((w, h));
    if let Some((x, y)) = crate::x11::window_position() {
        let pos_changed = crate::persist::load_window_pos() != Some((x, y));
        if size_changed {
            let _ = crate::persist::save_window_px_size(w, h);
        }
        if pos_changed {
            let _ = crate::persist::save_window_pos(x, y);
        }
    } else if size_changed {
        let _ = crate::persist::save_window_px_size(w, h);
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
pub(crate) fn screen_point(p: Point, camera: Point, world: &World) -> Vec2 {
    let mut q = Point::new(p.x + camera.x, p.y + camera.y);
    q.normalize_world(world);
    vec2(q.x as f32, q.y as f32)
}

/// Triangle texturé (équivalent `_MapTriangle _seamless ... _smooth` de QB64).
///
/// Les UV sont normalisés dans [0,1] et repliés par modulo (`rem_euclid`),
/// ce qui reproduit le wrapping `_seamless` (voir `docs/PORTAGE.md` §6).
#[allow(clippy::too_many_arguments)]
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
/// pour les triangles morts - un pixel sur deux).
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
/// les bords ne servent qu'à la génération et au debug) - on ne le fait pas.
///
/// `fade` (0..1) est l'opacité globale de la forme - utilisée par le fondu
/// enchaîné de la récupération EVA : le cosmonaute s'efface pendant que le
/// vaisseau reconstruit apparaît (`main.rs`). 1.0 pour toutes les autres
/// formes.
#[allow(clippy::too_many_arguments)]
pub fn draw_shape(
    state: &GameState,
    assets: &Assets,
    shape: &Shape,
    triangles: &mut [Triangle],
    camera: Point,
    elements: &[Element],
    show_data: bool,
    fade: f32,
) {
    if shape.life <= 0 {
        return;
    }

    // invulnérabilité post-respawn (scénario Survival) : le vaisseau
    // clignote (~5 alternances/s) pendant la durée restante
    if shape.who_i_am == WHOIAM_PLAYER && state.invulnerable > 0.0
        && (state.invulnerable * 10.0) as i32 % 2 == 0
    {
        return; // frame « éteinte » : le vaisseau n'est pas dessiné
    }

    // mode D (ex options = "D" de drawShape) : les indicateurs de bord des
    // triangles sont recalculés (comme l'original qui appelle
    // getBorderSegments à chaque frame - on ne le fait que si affiché)
    if show_data {
        get_border_segments(shape, triangles);
    }

    // minimap (radar de bord - `scenario::has_radar` : allumée par défaut en
    // jeu libre / Survival, achetée au magasin en scénario à économie).
    // **Zones colorées** : la station en vert, les météores en rouge, les
    // minerais en jaune, le vaisseau en blanc, les portails en cyan et les
    // mines en orange - les balles ne sont pas dessinées (elles passeraient
    // chaque frame). Dessinée **seulement quand la version active est la
    // minimap** : avec le radar de contrôleur aérien (`RadarKind::Atc`),
    // c'est le scope à balayage de `draw_atc_radar` qui s'affiche à la place
    // (un seul radar actif à la fois). **Pas de radar en mode cosmonaute
    // EVA** (vaisseau détruit) : seul le HUD d'accostage affiche la distance
    // à la base (`draw_docking_hud`).
    if !state.cosmonaut_active
        && crate::scenario::has_radar(state)
        && crate::scenario::active_radar_kind(state) == crate::scenario::RadarKind::Minimap
    {
        let mut p = Point::new(shape.position.x + camera.x, shape.position.y + camera.y);
        p.normalize_world(&state.world);
        let x = (p.x / 10.0) as i32 + (VIEWPORT_WIDTH / 2.0 - VIEWPORT_WIDTH / 20.0) as i32;
        let y = (p.y / 10.0) as i32 + (VIEWPORT_HEIGHT / 2.0 - VIEWPORT_HEIGHT / 20.0) as i32;
        let color = match shape.who_i_am {
            WHOIAM_STATION => 0x3F40FF80,   // base : vert
            WHOIAM_METEOR => 0x3FFF5050,    // météores : rouge
            WHOIAM_MINERAL => 0x3FFFFF60,   // minerais : jaune
            WHOIAM_PLAYER => 0x3FFFFFFF,    // vaisseau : blanc
            WHOIAM_WARP_GATE => 0x3F60FFFF, // portails : cyan
            WHOIAM_MINE => 0x3FFF9050,      // mines : orange
            WHOIAM_BULLET => 0x00000000,    // balles : invisibles
            _ => shape.shape_color,
        };
        if color != 0x00000000 {
            draw_circle(x as f32, y as f32, 1.0, argb_to_color(color));
            // highlight de la zone d'accostage quand le guide est actif
            // (retour à la base) : anneau vert pulsant autour de la station
            if shape.who_i_am == WHOIAM_STATION && state.docking_guide {
                let r = 3.0 + (get_time() * 2.0).sin().abs() as f32;
                draw_circle_lines(x as f32, y as f32, r, 1.0, argb_to_color(0x3F40FF80));
            }
        }
    }

    for t in &triangles[shape.first_triangle..=shape.last_triangle] {
        let p = screen_point(t.real_center, camera, &state.world);
        if shape.show_all_parts
            || (t.life > 0
                && inner_draw_limit(Point::new(p.x as f64, p.y as f64)))
        {
            // style de rendu (écran de paramétrage) : texturé (défaut),
            // colorisé (remplissage uni) ou mesh (arêtes seules)
            match state.render_style {
                RenderStyle::Textured => {
                    if shape.texture != TEXTURE_NONE {
                        draw_textured_triangle(assets, t, shape, camera, elements, &state.world, fade);
                    } else {
                        draw_triangle(assets, t, shape, camera, elements, &state.world, fade);
                    }
                }
                RenderStyle::Colored => {
                    draw_colored_triangle(t, shape, camera, elements, &state.world, fade)
                }
                RenderStyle::Mesh => draw_mesh_triangle(t, shape, camera, elements, &state.world, fade),
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

/// Radar de **contrôleur aérien** (version ATC du radar de bord - achetée au
/// magasin en scénario à économie, `scenario::RadarKind::Atc`) : un **scope
/// circulaire** collé au bord gauche de l'écran (au-dessus du score, le coin
/// bas gauche est occupé par le joystick tactile) avec **anneaux de
/// distance**, croix cardinales et repères N/E/S/W, un **balayage rotatif**
/// lent et flouté (ligne vive + traînée qui s'estompe derrière) et les
/// **échos** des formes proches du vaisseau - station verte, météores
/// rouges, minerais jaunes, portails cyan, mines orange, autres en couleur
/// de forme (mêmes codes que la minimap) - positionnés à l'échelle de la
/// minimap (1/30, **portée ×3**, centrés sur le vaisseau), peints **au
/// passage du balayage** puis **figés** à cette position jusqu'au
/// rafraîchissement suivant (un écho ne bouge que quand le balayage est
/// repassé dessus), fondus en persistance décroissante (lueur exponentielle),
/// sans scintillement. Un seul radar actif à la fois
/// (`scenario::active_radar_kind`) : quand la minimap est active, le scope
/// n'est pas dessiné (et inversement - voir `draw_shape`). Dessiné par
/// `main.rs` après les formes, avant le HUD.
pub fn draw_atc_radar(state: &mut GameState, camera: Point, shapes: &[Shape]) {
    // pas de radar en mode cosmonaute EVA (vaisseau détruit) : seul le HUD
    // d'accostage affiche la distance à la base (`draw_docking_hud`)
    if state.cosmonaut_active
        || !crate::scenario::has_radar(state)
        || crate::scenario::active_radar_kind(state) != crate::scenario::RadarKind::Atc
    {
        return;
    }
    // scope : instrument circulaire **collé au bord gauche** de l'écran (le
    // coin bas gauche est occupé par le joystick tactile - `touch.rs`,
    // translucide et dessiné par-dessus) - un point monde = 1/30 px écran
    // (portée ×3). Teintes du jeu : vert « radar » (vert d'accostage /
    // succès du magasin `SHOP_OK`) sur un disque bleu sombre.
    let r = 55.0; // 35 % plus petit que l'ancien scope (85)
    let cx = r; // bord gauche de l'écran
    let cy = 435.0;
    let tau = std::f32::consts::TAU;
    let green = |alpha: f32| Color::new(0.22, 1.00, 0.53, alpha); // 0xFF39FF88
    let dim = |alpha: f32| Color::new(0.20, 0.68, 0.40, alpha);
    let phosphor = |alpha: f32| Color::new(0.65, 1.00, 0.80, alpha);

    // fond du scope (disque translucide, bleu nuit) + halo + bordure +
    // anneaux de distance - bordures volontairement transparentes
    draw_circle(cx, cy, r, Color::new(0.02, 0.07, 0.10, 0.45));
    draw_circle_lines(cx, cy, r + 3.0, 1.0, green(0.20)); // halo extérieur
    draw_circle_lines(cx, cy, r, 2.0, green(0.55));
    for i in 1..=3 {
        let rr = r * i as f32 / 3.0;
        draw_circle_lines(cx, cy, rr, 1.0, dim(0.35));
    }
    // croix cardinales (horizontale + verticale, atténuées)
    draw_line(cx - r, cy, cx + r, cy, 1.0, dim(0.22));
    draw_line(cx, cy - r, cx, cy + r, 1.0, dim(0.22));
    // repères cardinaux N/E/S/W sur le bord extérieur
    for (dx, dy) in [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        draw_line(
            cx + dx * (r - 5.0),
            cy + dy * (r - 5.0),
            cx + dx * r,
            cy + dy * r,
            1.5,
            green(0.5),
        );
    }

    // balayage rotatif lent (~0,19 tour/s - moitié de la vitesse précédente) :
    // traînée floutée (double passe : épaisse et translucide sous la nette),
    // le tout très transparent, derrière une tête blanc-vert (phosphore)
    let sweep_speed = 1.2; // rad/s
    let angle = get_time() as f32 * sweep_speed;
    let trail = 0.8; // largeur angulaire de la traînée (rad)
    const SEGS: usize = 24;
    // passe « flou » : mêmes segments, plus épais et à peine visibles
    for i in (0..=SEGS).rev() {
        let a = angle - trail * (i as f32 / SEGS as f32);
        let alpha = 0.25 * (1.0 - i as f32 / SEGS as f32);
        let end = Vec2::new(cx + r * a.sin(), cy - r * a.cos());
        draw_line(cx, cy, end.x, end.y, 4.5, green(alpha));
    }
    // passe nette : la traînée proprement dite
    for i in (0..=SEGS).rev() {
        let a = angle - trail * (i as f32 / SEGS as f32);
        let alpha = 0.55 * (1.0 - i as f32 / SEGS as f32);
        let end = Vec2::new(cx + r * a.sin(), cy - r * a.cos());
        draw_line(cx, cy, end.x, end.y, 1.5, green(alpha));
    }
    // tête du balayage (phosphore, floutée par une triple passe)
    let head = Vec2::new(cx + r * angle.sin(), cy - r * angle.cos());
    draw_line(cx, cy, head.x, head.y, 5.0, phosphor(0.22));
    draw_line(cx, cy, head.x, head.y, 3.0, phosphor(0.40));
    draw_line(cx, cy, head.x, head.y, 1.5, phosphor(0.65));

    // le vaisseau est le centre du scope : petite croix blanche (pas d'écho)
    draw_line(cx - 4.0, cy, cx + 4.0, cy, 1.5, Color::new(1.0, 1.0, 1.0, 0.75));
    draw_line(cx, cy - 4.0, cx, cy + 4.0, 1.5, Color::new(1.0, 1.0, 1.0, 0.75));

    // échos : les formes proches du vaisseau, positionnées comme sur la
    // minimap (1/10) mais centrées sur le scope. Un écho est peint **au
    // passage du balayage** puis reste **figé** à cette position tant que le
    // balayage n'est pas repassé dessus (« rafraîchi ») : entre deux
    // passages il ne bouge pas et s'estompe progressivement (persistance
    // décroissante - lueur exponentielle, sans scintillement)
    let mut pp = Point::new(
        shapes[PLAYER_INDEX].position.x + camera.x,
        shapes[PLAYER_INDEX].position.y + camera.y,
    );
    pp.normalize_world(&state.world);
    // échelle du scope : 1/30 (portée ×3 par rapport à la minimap 1/10) -
    // les formes jusqu'à ~3 fois plus loin tiennent dans le disque
    let scale = 1.0 / 30.0;
    let base_x = pp.x * scale + VIEWPORT_WIDTH / 2.0 - VIEWPORT_WIDTH / 20.0;
    let base_y = pp.y * scale + VIEWPORT_HEIGHT / 2.0 - VIEWPORT_HEIGHT / 20.0;
    // garde les échos alignés sur le tableau des formes (les formes sont
    // stables en cours de partie - la réallocation ne se produit qu'au besoin)
    if state.radar_echoes.len() != shapes.len() {
        state.radar_echoes.resize_with(shapes.len(), RadarEcho::default);
    }
    let dt = get_frame_time(); // vieillissement des échos (secondes)
    let decay = 1.8; // persistance de l'écho (radians de balayage avant extinction)
    for (i, shape) in shapes.iter().enumerate() {
        // rafraîchissement : le balayage vient de passer sur la forme - son
        // écho **saute à sa position courante** (la seule fois où il bouge)
        if shape.life > 0 && shape.who_i_am != WHOIAM_BULLET && shape.who_i_am != WHOIAM_PLAYER {
            let mut p = Point::new(shape.position.x + camera.x, shape.position.y + camera.y);
            p.normalize_world(&state.world);
            let dx = (p.x * scale + VIEWPORT_WIDTH / 2.0 - VIEWPORT_WIDTH / 20.0 - base_x) as f32;
            let dy = (p.y * scale + VIEWPORT_HEIGHT / 2.0 - VIEWPORT_HEIGHT / 20.0 - base_y) as f32;
            if dx.hypot(dy) < r - 1.0 {
                // 0 = nord, sens horaire (comme le balayage) : la forme est
                // rafraîchie tant que le balayage est dans la traînée derrière elle
                let blip_angle = dx.atan2(-dy);
                let since = (angle - blip_angle).rem_euclid(tau);
                if since <= trail {
                    let e = &mut state.radar_echoes[i];
                    e.x = dx;
                    e.y = dy;
                    e.age = 0.0;
                }
            }
        }
        // vieillissement + dessin : l'écho reste à sa position figée et fond
        // progressivement (lueur exponentielle, aucun scintillement)
        let e = &mut state.radar_echoes[i];
        e.age += sweep_speed * dt;
        let light = (-e.age / decay).exp();
        if light < 0.04 || e.x.hypot(e.y) >= r - 1.0 {
            continue; // écho éteint, ou position sortie du disque
        }
        let color = match shape.who_i_am {
            WHOIAM_STATION => 0x3F40FF80,
            WHOIAM_METEOR => 0x3FFF5050,
            WHOIAM_MINERAL => 0x3FFFFF60,
            WHOIAM_WARP_GATE => 0x3F60FFFF,
            WHOIAM_MINE => 0x3FFF9050,
            _ => shape.shape_color,
        };
        let c = argb_to_color(color);
        let size = if shape.who_i_am == WHOIAM_STATION { 2.4 } else { 1.7 };
        let ex = cx + e.x;
        let ey = cy + e.y;
        // écho flouté : deux halos concentriques translucides + le cœur
        draw_circle(ex, ey, size * 2.6, Color::new(c.r, c.g, c.b, light * 0.15));
        draw_circle(ex, ey, size * 1.7, Color::new(c.r, c.g, c.b, light * 0.35));
        draw_circle(ex, ey, size, Color::new(c.r, c.g, c.b, light));
    }

    // légende au-dessus du scope (le bas de l'écran est occupé par le score)
    let label = "RADAR ATC";
    let w = crate::font::measure_text(label, None, 12, 1.0).width;
    draw_text_shadow(label, cx - w / 2.0, cy - r - 10.0, 12.0, dim(0.9));
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
    fade: f32,
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
        if shape.who_i_am == WHOIAM_PLAYER {
            // NB vaisseau : `vaisseau.png` (512×512) est une image complète du
            // vaisseau, pas une texture tuilable. La formule générique ci-dessous
            // (fidèle à l'original) dégénère pour le triangle joueur : les trois
            // sommets donnent u = 0 → le GPU interpole u = 0 partout → tout le
            // triangle échantillonne la colonne 0 de la texture (le fond gris).
            // On étale donc la texture complète sur la boîte du vaisseau :
            // u = (x − top_left.x)/width, v = (y − top_left.y)/height.
            vec2(
                ((x - shape.top_left.x) / shape.width) as f32,
                ((y - shape.top_left.y) / shape.height) as f32,
            )
        } else if shape.who_i_am == WHOIAM_METEOR {
            // NB météores : la formule générique (une tuile par météore,
            // fidèle à l'original) compresse la tuile 1254 px dans la taille
            // de la forme → détail sub-pixel, rendu « zoom arrière ». On
            // magnifie avec `METEOR_TEXTURE_ZOOM` pour que le motif de roche
            // soit visible (région centrale 1/M de la tuile).
            let r = tw / (larger * METEOR_TEXTURE_ZOOM);
            vec2(
                ((x * r - tw / 2.0) / tw).rem_euclid(1.0) as f32,
                ((y * r - tw / 2.0) / tw).rem_euclid(1.0) as f32,
            )
        } else if shape.who_i_am == WHOIAM_STATION {
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
    // teinte blanche à l'opacité demandée (fondu enchaîné de la récupération)
    draw_triangle_texture(texture, a, b, c, uv_a, uv_b, uv_c, Color::new(1.0, 1.0, 1.0, fade));

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
/// de la texture, équivalent `_seamless`). Les triangles portant une couleur
/// par face (`t.color != 0`, ex le cosmonaute de `cosmonaut.rs`) sont dessinés
/// en remplissage uni à la place de la texture. Les triangles morts sont
/// dessinés en pointillés.
fn draw_triangle(
    assets: &Assets,
    t: &Triangle,
    shape: &Shape,
    camera: Point,
    elements: &[Element],
    world: &World,
    fade: f32,
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);

    if t.life > 0 {
        if t.color != 0 {
            // face à couleur propre (cosmonaute) : remplissage uni, atténué
            // par l'opacité demandée (fondu enchaîné de la récupération)
            macroquad::shapes::draw_triangle(
                a,
                b,
                c,
                fade_color(triangle_color(t, shape, elements), fade),
            );
        } else {
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
                Color::new(1.0, 1.0, 1.0, fade),
            );
        }
    } else {
        draw_dead_triangle(a, b, c, t, shape);
    }

    draw_element_dot(t, camera, elements, world);
}

/// Triangle colorisé (style « COLORED » de l'écran de paramétrage) :
/// remplissage uni avec la couleur de l'élément (sinon celle de la forme), à
/// la place de la texture ; les triangles morts restent en pointillés.
fn draw_colored_triangle(
    t: &Triangle,
    shape: &Shape,
    camera: Point,
    elements: &[Element],
    world: &World,
    fade: f32,
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);
    if t.life <= 0 {
        draw_dead_triangle(a, b, c, t, shape);
        return;
    }
    // NB : chemin complet - `draw_triangle` (macroquad) est masqué par la
    // fonction locale du même nom (triangles non texturés de l'original)
    macroquad::shapes::draw_triangle(a, b, c, fade_color(triangle_color(t, shape, elements), fade));
    draw_element_dot(t, camera, elements, world);
}

/// Triangle en fil de fer (style « MESH » de l'écran de paramétrage) :
/// arêtes seules, dans la couleur de l'élément (sinon celle de la forme) ;
/// les triangles morts restent en pointillés. Les triangles de la **base**
/// portent leur niveau de dégâts : les arêtes rougissent et s'épaississent à
/// mesure que les impacts s'accumulent (`t.damage` / `STATION_TRIANGLE_DAMAGE_MAX`).
fn draw_mesh_triangle(
    t: &Triangle,
    shape: &Shape,
    camera: Point,
    elements: &[Element],
    world: &World,
    fade: f32,
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);
    if t.life <= 0 {
        draw_dead_triangle(a, b, c, t, shape);
        return;
    }
    // niveau de dégâts de la base (style MESH) : rouge d'autant plus vif et
    // arêtes d'autant plus épaisses que le triangle est endommagé
    let (line_color, width) = if shape.who_i_am == WHOIAM_STATION && t.damage > 0 {
        let f = (t.damage as f32 / STATION_TRIANGLE_DAMAGE_MAX as f32).min(1.0);
        (
            Color::new(1.0, 0.30 * (1.0 - f), 0.20 * (1.0 - f), 1.0),
            1.0 + 2.5 * f,
        )
    } else {
        (triangle_color(t, shape, elements), 1.0)
    };
    draw_triangle_lines(a, b, c, width, fade_color(line_color, fade));
    draw_element_dot(t, camera, elements, world);
}

/// Applique un facteur d'opacité (0..1) à une couleur - utilisé par le fondu
/// enchaîné de la récupération EVA (`draw_shape`, paramètre `fade`) : le
/// cosmonaute s'efface pendant que le vaisseau reconstruit apparaît.
fn fade_color(color: Color, fade: f32) -> Color {
    Color::new(color.r, color.g, color.b, color.a * fade)
}

/// Couleur d'affichage d'un triangle vivant : celle de son élément minéral
/// (ex `elements[t.element].color`), sinon sa couleur par face (`t.color`, ex
/// le cosmonaute de `cosmonaut.rs`), sinon la couleur de la forme.
fn triangle_color(t: &Triangle, shape: &Shape, elements: &[Element]) -> Color {
    if t.element > 0 {
        argb_to_color(elements[t.element as usize].color)
    } else if t.color != 0 {
        argb_to_color(t.color)
    } else {
        argb_to_color(shape.shape_color)
    }
}

/// Arêtes en pointillés d'un triangle mort (approximation du motif `&B…` de
/// QB64), dans la couleur de la forme (atténuée si le triangle a été touché).
fn draw_dead_triangle(a: Vec2, b: Vec2, c: Vec2, t: &Triangle, shape: &Shape) {
    let color = if t.collid {
        argb_to_color(shape.shape_color & 0x70FFFFFF)
    } else {
        argb_to_color(shape.shape_color)
    };
    draw_dashed_line(a, b, color);
    draw_dashed_line(b, c, color);
    draw_dashed_line(c, a, color);
}

/// Point central d'un triangle minéral (élément > 0), commun à tous les
/// styles de rendu (ex `drawTexturedTriangle` de l'original).
fn draw_element_dot(t: &Triangle, camera: Point, elements: &[Element], world: &World) {
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

/// Effet de poussée **de repli** (liste `VAISSEAU_THRUSTERS` vide - sinon le
/// jeu dessine le mesh configuré de chaque propulseur, `draw_thruster_gas`) :
/// 3 cercles dégradés le long de `orientation + angle` (ex `ejectionFlow`),
/// partant du **point local** (au centre de rotation en repli) tourné avec le
/// vaisseau autour de son centre de rotation comme les sommets du mesh
/// (`compute_real_positions`). Angle `TAU/2` = arrière (gaz de poussée,
/// orange), `0` = avant (gaz de frein, bleu), `±TAU/4` = jets latéraux des
/// rotations (← / →). L'éjection est courte : les cercles partent du point
/// (pas de loin au-dessus de la forme).
pub fn ejection_flow(
    shape: &Shape,
    local: Point,
    angle: f64,
    flow_color: u32,
    camera: Point,
    world: &World,
) {
    // NB : comme l'original, les rayons et le jitter sont tronqués en entiers
    let u01 = rand::rand() as f64 / u32::MAX as f64; // [0,1)
    let f = (u01 * 2.0 - 1.0).trunc();
    let r1 = (4.0 + u01 * 3.0).trunc();
    let r2 = r1 + 4.0;
    let r3 = r2 + 4.0;
    let c = (shape.orientation + angle).cos();
    let s = (shape.orientation + angle).sin();
    // point local → monde : tourné autour du centre de rotation puis translaté
    // (même calcul que les balles de `fire_bullet`, generate.rs)
    let mut local = local;
    local.rotate_around(Point::new(shape.center.x, shape.center.y), shape.orientation);
    let x = shape.position.x + local.x + camera.x;
    let y = shape.position.y + local.y + camera.y;
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

/// Gaz d'éjection d'un propulseur : son **mesh configuré** (la flamme du
/// propulseur, ex propellerUp.json) dessiné **scintillant** - le mesh
/// n'apparaît que quand le propulseur tire, sinon il n'est pas affiché
/// (remplace l'ancien `ejection_flow`, cercles calculés). Les triangles
/// (`vaisseau::thruster_mesh_triangles`, repère local du vaisseau) sont
/// teintés de la **couleur configurée** (`tint`, ARGB) puis tournés avec le
/// vaisseau autour de son centre de rotation comme les sommets du mesh ; la
/// flamme **vacille** : allongée le long de la direction d'éjection
/// (`flow_angle`, repère du jeu) et opacité animées (sinus rapide + bruit).
pub fn draw_thruster_gas(
    shape: &Shape,
    tris: &[([f64; 2], [f64; 2], [f64; 2])],
    local: Point,
    flow_angle: f64,
    tint: u32,
    camera: Point,
    world: &World,
) {
    if tris.is_empty() {
        return;
    }
    // scintillement : la flamme pulse le long de l'axe du flux (sinus + bruit)
    let t = get_time();
    let n = rand::rand() as f64 / u32::MAX as f64;
    let stretch = 0.12 * (t * 35.0).sin() + 0.05 * n;
    let alpha = 0.85 + 0.15 * (t * 42.0 + 1.0).sin();
    let (fx, fy) = (flow_angle.cos(), flow_angle.sin());
    let (qx, qy) = (-fy, fx); // perpendiculaire au flux
    let axis = Point::new(shape.center.x, shape.center.y);
    // échelle anisotrope autour du point d'éjection (repère local) : allongée
    // le long du flux, rétractée en travers
    let scal = |p: &[f64; 2]| {
        let dx = p[0] - local.x;
        let dy = p[1] - local.y;
        let along = dx * fx + dy * fy;
        let across = dx * qx + dy * qy;
        Point::new(
            local.x + fx * along * (1.0 + stretch) + qx * across * (1.0 - 0.35 * stretch),
            local.y + fy * along * (1.0 + stretch) + qy * across * (1.0 - 0.35 * stretch),
        )
    };
    // couleur de la flamme : la couleur configurée, opacité animée
    let mut col = argb_to_color(tint);
    col.a = alpha as f32;
    for (a, b, c) in tris {
        // puis rotation avec le vaisseau et translation monde (comme les
        // sommets du mesh - `compute_real_positions`)
        let mut pa = scal(a);
        let mut pb = scal(b);
        let mut pc = scal(c);
        pa.rotate_around(axis, shape.orientation);
        pb.rotate_around(axis, shape.orientation);
        pc.rotate_around(axis, shape.orientation);
        let wa = Point::new(shape.position.x + pa.x, shape.position.y + pa.y);
        let wb = Point::new(shape.position.x + pb.x, shape.position.y + pb.y);
        let wc = Point::new(shape.position.x + pc.x, shape.position.y + pc.y);
        macroquad::shapes::draw_triangle(
            screen_point(wa, camera, world),
            screen_point(wb, camera, world),
            screen_point(wc, camera, world),
            col,
        );
    }
}

/// Petit **propulseur de la combinaison EVA** : une flamme animée sur le dos
/// du cosmonaute (dans l'axe `orientation + π`, comme la flamme du vaisseau -
/// le dos est opposé au déplacement), dessinée quand il pousse (`thrusted`).
/// Le cosmonaute n'a qu'**un seul propulseur** (pas de marche arrière - voir
/// `input::cosmonaut_controls`) : une flamme extérieure orange semi-transparente
/// et un cœur jaune, dont la longueur et la largeur **vacillent** (sinus
/// rapide + bruit) pour un effet de combustion animé.
pub fn draw_cosmonaut_thruster(shape: &Shape, camera: Point, world: &World) {
    // dos du cosmonaute : opposé à l'orientation (déplacement inverse) - la
    // flamme tourne avec la figure, toujours dans son dos
    let back = shape.orientation + TAU / 2.0;
    let (dx, dy) = (back.cos(), back.sin());
    let (px, py) = (-dy, dx); // perpendiculaire : largeur de la flamme
    let x = shape.position.x + shape.center.x;
    let y = shape.position.y + shape.center.y;

    // la flamme danse : longueur et demi-largeur animées (sinus rapide + bruit)
    let t = get_time();
    let n = rand::rand() as f64 / u32::MAX as f64;
    let len = 5.0 + 2.0 * (t * 22.0).sin() + 1.5 * n;
    let half = 1.6 + 0.5 * (t * 15.0 + 1.0).sin() + 0.4 * n;

    // base de la flamme sur le dos du corps, pointe dans l'axe arrière
    let nozzle = shape.radius * 0.9;
    let to_screen = |p: Point| {
        let mut q = Point::new(p.x + camera.x, p.y + camera.y);
        q.normalize_world(world);
        vec2(q.x as f32, q.y as f32)
    };
    let base = Point::new(x + dx * nozzle, y + dy * nozzle);
    let tip = Point::new(x + dx * (nozzle + len), y + dy * (nozzle + len));
    let l1 = Point::new(base.x + px * half, base.y + py * half);
    let l2 = Point::new(base.x - px * half, base.y - py * half);
    // flamme extérieure orange (semi-transparente) - chemin complet : la
    // fonction locale `draw_triangle` (triangles non texturés) masque celle de
    // macroquad
    macroquad::shapes::draw_triangle(
        to_screen(l1),
        to_screen(l2),
        to_screen(tip),
        argb_to_color(0xA0FF9020),
    );
    // cœur jaune, plus court et plus étroit (le centre de la flamme)
    let inner_len = nozzle + len * 0.55;
    let inner_half = half * 0.45;
    let itip = Point::new(x + dx * inner_len, y + dy * inner_len);
    let i1 = Point::new(base.x + px * inner_half, base.y + py * inner_half);
    let i2 = Point::new(base.x - px * inner_half, base.y - py * inner_half);
    macroquad::shapes::draw_triangle(
        to_screen(i1),
        to_screen(i2),
        to_screen(itip),
        argb_to_color(0xFFFFD050),
    );
}

/// Dessine un débris : pixel blanc 1×1 (ex `drawGarbage` du portage QB64).
/// Fidélité à l'original : la phase/rotation propre (`angle`/`spin_rate`,
/// avancée par `moving_garbage`) reste dans le modèle mais n'est pas rendue,
/// un pixel étant visuellement identique quel que soit l'angle.
pub fn draw_garbage(g: &Garbage, camera: Point, world: &World) {
    if g.life == 0 {
        return;
    }
    let p = screen_point(g.position, camera, world);
    if !inner_draw_limit(Point::new(p.x as f64, p.y as f64)) {
        return;
    }
    draw_rectangle(p.x, p.y, 1.0, 1.0, argb_to_color(g.rgba_color));
}

// ─── Accostage : mire au centre de la station + HUD d'approche ──────────────

/// Transparence (octet alpha ARGB) de la mire d'accostage : discrète (canal
/// alpha volontairement bas, comme les liens d'accostage) - l'anneau et la
/// croix sont plus légers que le point central (l'effet néon empile des
/// halos dérivés de ces alphas, voir `neon_ring`/`neon_line`/`neon_dot`).
pub(crate) const DOCK_MARKER_ALPHA: u32 = 0x66;
pub(crate) const DOCK_MARKER_DOT_ALPHA: u32 = 0x99;

/// Transparence (octet alpha ARGB) des liens d'accostage : canal alpha bas
/// (comme la mire) pour que les 4 liens simultanés restent discrets et
/// laissent voir le vaisseau - l'effet néon empile des halos dérivés de cet
/// alpha (voir `neon_line`).
pub(crate) const DOCK_LINE_ALPHA: u32 = 0x66;

/// Distance (unités monde, du centre du vaisseau) des points de branchement
/// des liens d'accostage : les 4 liens se connectent en diagonale (NO, SO,
/// SE, NE) sur un petit losange **proche du centre** - l'illusion qu'ils
/// touchent le vaisseau (ils sont dessinés dessous).
pub(crate) const DOCK_LINE_SHIP_ANCHOR: f64 = 5.0;

// ─── HUD : emplacements fixes (anti-tremblement) ────────────────────────────
//
// Chaque segment de la ligne du haut démarre à une **colonne fixe** (grille
// 8 px de la police 8×16 de l'original, x = 8+(col-1)*8) : contrairement à un
// positionnement dynamique (somme des largeurs des segments précédents), un
// segment ne bouge pas quand une valeur change de largeur. Les nombres sont
// en outre alignés à droite sur une **largeur fixe** (`{:>n}`) : les chiffres
// eux-mêmes ne bougent pas non plus (FPS, réputation, distance…).

/// Colonne de départ du FPS (champ fixe de 3 chiffres : boucle plafonnée à
/// 600 fps).
pub(crate) const HUD_FPS_COL: i32 = 1;
/// Colonne de départ de la réputation (+ rang) - champ fixe de 4 chiffres.
pub(crate) const HUD_REPUTATION_COL: i32 = 15;
/// Colonne de départ de la précision - champ fixe de 3 chiffres (max 100).
pub(crate) const HUD_PRECISION_COL: i32 = 42;
/// Colonne de départ des ressources du scénario (carburant, munitions,
/// minerais - ou vies/bouclier).
pub(crate) const HUD_RESOURCES_COL: i32 = 57;
/// Largeur maximale (en caractères) du bloc de ressources en économie → le
/// statut d'accostage démarre juste après (colonne 97).
pub(crate) const HUD_RESOURCES_ECONOMY_COLS: i32 = 39;
/// Largeur maximale (en caractères) du bloc de ressources en Survival
/// (LIVES/SHIELD) → le statut d'accostage démarre à la colonne 74.
pub(crate) const HUD_RESOURCES_SURVIVAL_COLS: i32 = 16;
// Le score composite + record n'est PAS sur la ligne principale (réservée
// au statut d'accostage - distance à la base prioritaire) : il est affiché
// **en bas à droite** de l'écran (voir `draw_score_hud`).

/// Fréquence (Hz) du **clignotement d'alerte** des ressources du HUD :
/// carburant/munitions presque vides et baies de chargement presque pleines
/// alternent leur couleur (blanc ↔ rouge, ~3 cycles/s - même principe que
/// le flash des règles du menu titre, `title.rs`).
pub(crate) const HUD_BLINK_HZ: f64 = 3.0;
/// Couleur d'alerte (ARGB) du clignotement : rouge vif (même teinte que
/// « GAME OVER »).
pub(crate) const HUD_WARN_COLOR: u32 = 0xFFFF4040;
/// Seuil « réserve presque vide » : le carburant / les munitions clignotent
/// au HUD tant que la réserve est **sous** cette fraction de sa capacité.
pub(crate) const HUD_LOW_RESERVE_RATIO: f64 = 0.25;
/// Seuil « soute presque pleine » : les baies de chargement occupées
/// clignotent dès que le cargo atteint **au moins** cette fraction de la
/// capacité (`draw_cargo`).
pub(crate) const HUD_FULL_CARGO_RATIO: f64 = 0.8;

// ─── Affichages de debug (touches D et I) ───────────────────────────────────

// ─── Objectifs DAG (scénarios custom) ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parallaxe : quand le vaisseau avance (la caméra recule, ex `W/2 - pos`),
    /// les étoiles défilent en sens INVERSE du vaisseau.
    #[test]
    fn star_parallax_moves_against_ship_direction() {
        // caméra immobile → étoile à sa position de base
        let (x0, y0) = star_screen_pos(500.0, 300.0, Point::new(0.0, 0.0), 1.0).unwrap();
        assert_eq!(x0, 500.0);
        assert_eq!(y0, 300.0);

        // le vaisseau avance vers la droite → la caméra recule (x décroît)
        let camera = Point::new(-120.0, -60.0);
        let (x1, y1) = star_screen_pos(500.0, 300.0, camera, 1.0).unwrap();
        // l'étoile se déplace à gauche et en haut : sens inverse du vaisseau
        assert!(x1 < x0);
        assert!(y1 < y0);
        assert_eq!(x0 - x1, 120.0);
        assert_eq!(y0 - y1, 60.0);
    }

    /// Parallaxe : la vitesse de défilement croît avec le plan (×1, ×2, ×3),
    /// comme `(étoile + caméra) × plan` de l'original.
    #[test]
    fn star_parallax_speed_scales_with_plan() {
        let camera = Point::new(-120.0, 0.0);
        let (x1, _) = star_screen_pos(500.0, 300.0, camera, 1.0).unwrap();
        let (x2, _) = star_screen_pos(500.0, 300.0, camera, 2.0).unwrap();
        let (x3, _) = star_screen_pos(500.0, 300.0, camera, 3.0).unwrap();
        // plan 1 : décalage de 120 px ; plan 2 : 240 ; plan 3 : 360
        assert_eq!(500.0 - x1, 120.0);
        assert_eq!(500.0 - x2, 240.0);
        assert_eq!(500.0 - x3, 360.0);
    }

    /// Rebouchage torique : une étoile qui sort par la droite (ou le bas) de la
    /// tuile réapparaît à gauche (ou en haut) - ex `normalizePlanPosition`.
    #[test]
    fn star_parallax_wraps_around_tile() {
        // étoile près du bord droit de la tuile, caméra qui recule
        let (x, _) = star_screen_pos(1000.0, 300.0, Point::new(-50.0, 0.0), 1.0).unwrap();
        // 1000 - 50 = 950 : pas encore de rebouclage
        assert_eq!(x, 950.0);

        let (x, _) = star_screen_pos(1000.0, 300.0, Point::new(-80.0, 0.0), 1.0).unwrap();
        // 1000 - 80 = 920 → x = 920 + 1024 = 1944 → ≥ 1024 → 920 (torique)
        assert_eq!(x, 920.0);

        // déplacement vers le haut : même chose pour y (200 + 944 = 1144 →
        // rebouclé → 120, dans le viewport)
        let (_, y) = star_screen_pos(500.0, 200.0, Point::new(0.0, -80.0), 1.0).unwrap();
        assert_eq!(y, 120.0);
    }

    /// Culling : les étoiles rebouclées qui restent hors viewport sont ignorées.
    #[test]
    fn star_parallax_culls_off_screen() {
        // x = 500 + offset 500 = 1000 ≥ 960 (viewport) → hors écran à droite
        assert!(star_screen_pos(500.0, 300.0, Point::new(500.0, 0.0), 1.0).is_none());
        // même chose côté y : y = 300 + 500 = 800 ≥ 540 → hors écran en bas
        assert!(star_screen_pos(500.0, 300.0, Point::new(0.0, 500.0), 1.0).is_none());
        // mais rebouclé : camera.x = -500 → offset 524 → x = 1024 → 0 → visible
        assert!(star_screen_pos(500.0, 300.0, Point::new(-500.0, 0.0), 1.0).is_some());
    }

    /// La mire réagit dans TOUT le rayon de la base : la qualité est nulle
    /// (rouge) au bord du rayon de la station, pleine (vert) au centre à
    /// l'arrêt, et interpolée entre les deux - plus seulement dans la zone
    /// d'accostage de 15 px.
    #[test]
    fn docking_quality_spans_the_whole_station_radius() {
        let station_radius = 160.0;
        // au bord du rayon de la base → rouge (qualité 0), même à l'arrêt
        assert_eq!(docking_approach_quality(station_radius, 0.0, station_radius), 0.0);
        // au centre, à l'arrêt → vert (qualité 1)
        assert_eq!(docking_approach_quality(0.0, 0.0, station_radius), 1.0);
        // à mi-rayon (80 px), à l'arrêt → qualité 0.5
        assert_eq!(
            docking_approach_quality(station_radius / 2.0, 0.0, station_radius),
            0.5
        );
        // au centre mais trop rapide → rouge (qualité 0)
        assert_eq!(
            docking_approach_quality(0.0, DOCK_APPROACH_FULL_RED_SPEED, station_radius),
            0.0
        );
        // au-delà du rayon de la base → rouge, quel que soit le rayon passé
        assert_eq!(docking_approach_quality(200.0, 0.0, station_radius), 0.0);
    }

    /// La mire est pilotée par la distance au centre (sur tout le rayon de la
    /// base) et par la vitesse de façon **multiplicative** : trop loin OU trop
    /// rapide fait retomber la qualité vers le rouge.
    #[test]
    fn docking_quality_combines_distance_and_speed() {
        let station_radius = 160.0;
        // mi-rayon + mi-vitesse → 0.5 × 0.5 = 0.25
        assert_eq!(
            docking_approach_quality(
                station_radius / 2.0,
                DOCK_APPROACH_FULL_RED_SPEED / 2.0,
                station_radius
            ),
            0.25
        );
        // vitesse négative (recul) traitée comme positive
        assert_eq!(
            docking_approach_quality(
                station_radius / 2.0,
                -DOCK_APPROACH_FULL_RED_SPEED / 2.0,
                station_radius
            ),
            0.25
        );
    }

    /// La mire d'accostage **disparaît quand le vaisseau est tenu par les
    /// liens** : à quai (liens attachés), pendant l'animation d'accostage,
    /// accosté (boîte ouverte, atelier) et pendant la rétraction des liens au
    /// départ. Elle n'est visible que **lors du retour à la base** (`docking_guide`
    /// actif) et dans le rayon de la station.
    #[test]
    fn docking_marker_hidden_while_held_by_links() {
        let mut state = GameState::new();
        state.dock_links = false; // le vaisseau a quitté la base
        state.docking_guide = true; // … puis est revenu (guide actif)
        let player = Point::new(50.0, 0.0);
        let station = Point::new(0.0, 0.0);
        let radius = 160.0;
        // retour à la base : visible (guide d'accostage)
        assert!(docking_marker_visible(&state, player, station, radius));
        // à quai (lancement/respawn) : liens attachés → cachée
        state.dock_links = true;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.dock_links = false;
        // tenu par les liens : animation d'accostage en cours → cachée
        state.dock_anim = 1.0;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.dock_anim = 0.0;
        // accosté : boîte DOCK STATION ouverte → cachée
        state.dock_box = true;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.dock_box = false;
        // accosté : magasin ouvert → cachée
        state.shop_box = true;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.shop_box = false;
        // départ : rétraction des liens en cours → cachée
        state.dock_retract = 1.0;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.dock_retract = 0.0;
        // libre, dans le rayon, guide actif : visible
        assert!(docking_marker_visible(&state, player, station, radius));
    }

    /// Le câble d'accostage **ondule en se rétractant** (et en se déployant) :
    /// la déformation est nulle à l'anneau (ancré, t = 0) et quand l'intensité
    /// est nulle (câble tendu) ; l'extrémité libre (t = 1) fouette (amplitude
    /// bornée par l'intensité, croissante vers l'extrémité libre × t) ; l'onde
    /// court dans le temps - vers l'anneau en rétraction, vers le vaisseau en
    /// déploiement (sens opposés).
    #[test]
    fn docking_cable_undulates_in_both_directions() {
        // ancré à l'anneau (t = 0) : jamais déformé
        assert_eq!(cable_wave_offset(0.0, 10.0, 0.0, false), 0.0);
        // câble tendu (intensité nulle) : plus d'ondulation
        assert_eq!(cable_wave_offset(0.5, 0.0, 3.0, false), 0.0);
        assert_eq!(cable_wave_offset(1.0, 0.0, 3.0, true), 0.0);
        // l'extrémité libre (t = 1) fouette : amplitude bornée par
        // l'intensité (10 px), et qui croît vers l'extrémité libre (× t)
        for time in [0.0f32, 0.05, 0.2, 0.8, 1.5] {
            let free = cable_wave_offset(1.0, 10.0, time, false);
            let mid = cable_wave_offset(0.5, 10.0, time, false);
            assert!(free.abs() <= 10.0, "fouet hors borne: {}", free);
            assert!(free.abs() >= mid.abs(), "l'onde doit croître vers l'extrémité libre");
        }
        // l'onde court dans le temps (phase croissante) : l'offset varie
        let a = cable_wave_offset(0.5, 10.0, 0.0, false);
        let b = cable_wave_offset(0.5, 10.0, 0.1, false);
        assert!((a - b).abs() > 1e-3, "l'onde doit se déplacer ({} vs {})", a, b);
        // déploiement : l'onde court vers le vaisseau (sens inverse de la
        // rétraction, qui court vers l'anneau)
        let c = cable_wave_offset(0.5, 10.0, 0.0, true);
        let d = cable_wave_offset(0.5, 10.0, 0.1, true);
        assert!((c - d).abs() > 1e-3, "l'onde doit se déplacer en déploiement");
        assert!(
            (b - a) * (d - c) < 0.0,
            "rétraction et déploiement doivent propager en sens inverse"
        );
    }

    /// Au **désamarrage**, le lien part **tendu** (longueur 1 - r = 1 au
    /// largage) et se rétracte vers l'anneau (longueur 0 à la fin), pendant
    /// que l'ondulation (relâchement de la tension) est maximale au largage
    /// et s'éteint une fois le lien rentré.
    #[test]
    fn undocking_releases_the_cable_from_taut_to_retracted() {
        // longueur du lien = 1 - r : tendu au largage, replié à la fin
        for r in [0.0f64, 0.25, 0.5, 0.75, 1.0] {
            let length = 1.0 - r;
            assert!((0.0..=1.0).contains(&length), "longueur hors borne");
        }
        // enveloppe : maximale au largage (r = 0), nulle une fois rentré
        // (r = 1), strictement entre les deux (relâchement par à-coups)
        assert_eq!(retract_envelope(0.0), 1.0);
        assert_eq!(retract_envelope(1.0), 0.0);
        assert!(retract_envelope(0.25) > 0.0 && retract_envelope(0.25) < 1.0);
        assert!(retract_envelope(0.5) > 0.0 && retract_envelope(0.5) < 1.0);
    }

    /// La mire n'est affichée **que lors du retour à la base** : pas tant que
    /// le guide est inactif (quitter l'accostage, en vol), et pas hors du
    /// rayon de la station même si le guide est actif (garde défensive).
    #[test]
    fn docking_marker_only_when_returning() {
        let mut state = GameState::new();
        state.dock_links = false;
        let station = Point::new(0.0, 0.0);
        let radius = 160.0;
        // guide inactif (quitte l'accostage, en vol) : cachée même dans le
        // rayon de la base
        assert!(!docking_marker_visible(&state, Point::new(50.0, 0.0), station, radius));
        // guide actif : visible dans le rayon…
        state.docking_guide = true;
        assert!(docking_marker_visible(&state, Point::new(50.0, 0.0), station, radius));
        // … mais pas hors du rayon (garde défensive)
        assert!(!docking_marker_visible(&state, Point::new(radius + 1.0, 0.0), station, radius));
    }
}
