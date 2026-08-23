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

use macroquad::models::{draw_mesh, Mesh, Vertex};
use macroquad::prelude::*;
use ::rand::{Rng, SeedableRng};
use ::rand_chacha::ChaCha12Rng;

use crate::audio::Sounds;
use crate::config::*;
use crate::garbage::Garbage;
use crate::geom::{Point, Triangle, World};
use crate::marketplace::{MOVING_MODES, VAISSEAU_WEAPONS};
use crate::scenario;
use crate::shape::{get_border_segments, Shape};
use crate::state::{Element, GameState, RenderStyle, ViewMode};

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
fn draw_text_shadow(text: &str, x: f32, y: f32, font_size: f32, color: Color) {
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
        let orange = Texture2D::from_file_with_format(
            include_bytes!("../assets/orange2.png"),
            Some(ImageFormat::Png),
        );
        let player = Texture2D::from_file_with_format(
            include_bytes!("../assets/vaisseau.png"),
            Some(ImageFormat::Png),
        );
        let meteor = Texture2D::from_file_with_format(
            include_bytes!("../assets/meteor_surface_tile.jpg"),
            Some(ImageFormat::Jpeg),
        );
        let station = Texture2D::from_file_with_format(
            include_bytes!("../assets/station.png"),
            Some(ImageFormat::Png),
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

        let mut stars = Vec::with_capacity(n);
        for _ in 0..n {
            let x = (rng.gen::<f64>() * STAR_TILE as f64) as f32;
            let y = (rng.gen::<f64>() * STAR_TILE as f64) as f32;
            let alpha = (127.0 + rng.gen::<f64>() * 128.0) as f32;
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

pub fn draw_stars(assets: &Assets, camera: Point) {
    for (layer, plan_layer) in assets.star_layers.iter().enumerate() {
        let plan = (layer + 1) as f32;
        for &(sx, sy, alpha) in &plan_layer.stars {
            // position écran : (étoile + caméra) × plan, rebouclée dans la
            // tuile (torique), comme le `normalizePlanPosition` de l'original ;
            // on élimine les étoiles hors viewport.
            if let Some((x, y)) = star_screen_pos(sx, sy, camera, plan) {
                draw_rectangle(x.round(), y.round(), 1.0, 1.0, Color::new(1.0, 1.0, 1.0, alpha / 255.0));
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
const BOX_FG: u32 = 0xFFD6EEFF;
/// Texte secondaire (descriptions des modes, valeur du volume) : plus clair
/// que l'ancien `0xB099DFFF` (illisible), mais volontairement plus discret
/// que `BOX_FG`.
const BOX_FG_DIM: u32 = 0xFFC2E4FF;
const BOX_HOVER: u32 = 0xFFFFFFFF;
const BOX_BG: u32 = 0xD01478DC;
const BOX_BORDER: u32 = 0xFF1AB2FF;
/// Panneau interne de l'écran de paramétrage (panneau « GRAPHICS ») :
/// fond légèrement plus clair que la fenêtre, bordure discrète.
const BOX_PANEL_BG: u32 = 0xE01478DC;
const BOX_PANEL_BORDER: u32 = 0x801AB2FF;
const BOX_PADDING: f32 = 10.0;

/// Largeur de la boîte DOCK STATION : assez pour le titre et les boutons
/// (UNLOAD / SHOP / CLOSE) sans chevauchement - même formule pour la
/// géométrie (`choice_box_layout`) et le dessin (`draw_choice_box`).
fn choice_box_width() -> f32 {
    let msg_w = measure_text("*** DOCK STATION ***", None, 16, 1.0).width + 2.0 * BOX_PADDING;
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let labels: [&str; 3] = ["UNLOAD", "SHOP", "CLOSE"];
    let buttons: f32 =
        labels.iter().map(|l| btn_w(l)).sum::<f32>() + (labels.len() as f32 - 1.0) * BOX_PADDING;
    300.0f32.max(msg_w).max(buttons + 2.0 * BOX_PADDING)
}

/// Géométrie de la boîte de choix DOCK STATION (ex `windowUtils_choiceBox`) :
/// fenêtre de 120 px de haut centrée sur l'écran, largeur assez grande pour
/// le titre et les boutons côte à côte en bas. Renvoie les rectangles écran
/// des boutons UNLOAD / SHOP / CLOSE (pour la détection de clic côté
/// logique). Le bouton REFUEL/REARM n'existe plus : le carburant et les
/// munitions s'achètent au magasin (bouton SHOP).
pub struct ChoiceBoxLayout {
    /// Bouton UNLOAD : décharge la soute (minerais disponibles pour le
    /// ravitaillement au magasin juste après - la boîte reste ouverte).
    pub unload: Rect,
    /// Bouton SHOP : ouvre le magasin de la station (carburant, munitions,
    /// armes, extensions et modes de déplacement en scénario à économie).
    pub shop: Rect,
    /// Bouton CLOSE : ferme la boîte.
    pub close: Rect,
}

pub fn choice_box_layout() -> ChoiceBoxLayout {
    let h = 120.0;
    let btn_h = 26.0;
    let w = choice_box_width();
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    // boutons alignés à gauche dans la boîte (la largeur est calculée pour
    // qu'ils tiennent sans chevauchement, marges = padding)
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let top_btn = top + h - 20.0 - btn_h;
    let labels: [&str; 3] = ["UNLOAD", "SHOP", "CLOSE"];
    let mut x = left + BOX_PADDING;
    let mut rects = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for (i, &label) in labels.iter().enumerate() {
        rects[i] = Rect::new(x, top_btn, btn_w(label), btn_h);
        x += rects[i].w + BOX_PADDING;
    }
    ChoiceBoxLayout {
        unload: rects[0],
        shop: rects[1],
        close: rects[2],
    }
}

/// Dessine la boîte de choix DOCK STATION (accostage) avec ses boutons
/// UNLOAD / SHOP / CLOSE (hover = blanc, ex `windowUtils_choiceBox`).
pub fn draw_choice_box() {
    let msg = "*** DOCK STATION ***";
    let w = choice_box_width();
    let h = 120.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text_shadow(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    // boutons avec survol
    let l = choice_box_layout();
    draw_box_button("UNLOAD", l.unload);
    draw_box_button("SHOP", l.shop);
    draw_box_button("CLOSE", l.close);
}

// ─── Magasin de la station (bouton SHOP) ────────────────────────────────────

/// Géométrie du magasin de la station (bouton SHOP de la boîte DOCK
/// STATION) : fenêtre centrée avec la section « ARMES » (une ligne par arme
/// du catalogue - scénario à économie seulement), la section « MOVING MODE »
/// (une ligne cliquable par mode de déplacement, ordre `MOVING_MODE_ORDER`),
/// les lignes d'extension (réservoir, chargeur, soute - scénario à économie
/// seulement) et un bouton CLOSE (retour à la boîte DOCK STATION).
pub struct ShopBoxLayout {
    /// Lignes des armes du catalogue (index dans `VAISSEAU_WEAPONS`, scénario
    /// à économie seulement - clic = achat de l'arme contre minerais) ;
    /// rectangles vides hors économie ou catalogue vide.
    pub weapons: [Rect; WEAPON_SLOTS],
    /// Ligne « FUEL » du ravitaillement (clic = achat de la quantité du
    /// curseur contre minerais, `scenario::buy_fuel_qty`) - rectangle vide
    /// hors économie.
    pub supplies_fuel: Rect,
    /// Piste du curseur de carburant (glisser / molette = quantité à
    /// acheter) - rectangle vide hors économie.
    pub slider_fuel: Rect,
    /// Lignes « AMMO » du ravitaillement, **une par arme possédée** (index
    /// catalogue - clic = achat de la quantité du curseur de l'arme,
    /// `scenario::buy_ammo_qty`) ; rectangles vides hors économie ou arme
    /// non possédée.
    pub supplies_ammo: [Rect; WEAPON_SLOTS],
    /// Pistes des curseurs de munitions par arme (glisser / molette).
    pub slider_ammo: [Rect; WEAPON_SLOTS],
    /// Ligne « réservoir de carburant » de l'atelier (clic = achat de
    /// l'extension) - rectangle vide hors économie.
    pub fuel: Rect,
    /// Ligne « chargeur de munitions » de l'atelier - rectangle vide hors
    /// économie.
    pub ammo: Rect,
    /// Ligne « soute » de l'atelier - rectangle vide hors économie.
    pub cargo: Rect,
    /// Lignes cliquables des modes de déplacement (ordre visuel
    /// `MOVING_MODE_ORDER` - clic = sélection gratuite ou déblocage contre
    /// minerais).
    pub modes: [Rect; 4],
    /// Bouton CLOSE : revient à la boîte DOCK STATION.
    pub close: Rect,
}

/// Hauteur de la fenêtre du magasin (scénario à économie) : le contenu
/// s'empile - titre, section ARMES (une ligne par arme du catalogue),
/// section RAVITAILLEMENT (carburant + une ligne AMMO **par arme possédée**),
/// lignes d'atelier, section MOVING MODE, bouton CLOSE. Hors économie :
/// hauteur fixe (modes seuls).
fn shop_box_height(show_upgrades: bool, weapons_n: usize, ammo_rows: usize) -> f32 {
    if !show_upgrades {
        return 300.0;
    }
    let mut h = 3.0 * BOX_PADDING + 22.0; // titre
    if weapons_n > 0 {
        h += 14.0 + weapons_n as f32 * 36.0 + 2.0; // ARMES
    }
    h += 14.0 + (1 + ammo_rows) as f32 * 36.0 + 2.0; // RAVITAILLEMENT (FUEL + AMMO par arme)
    h += 3.0 * 30.0 + 2.0; // atelier (extensions de capacité)
    h += 14.0 + 4.0 * 36.0; // MOVING MODE
    h += 20.0 + 26.0 + 12.0; // bouton CLOSE + marge basse
    h
}

pub fn shop_box_layout(state: &GameState) -> ShopBoxLayout {
    // section « ARMES » (scénario à économie, catalogue non vide) : en-tête
    // + une ligne (34 px) par arme - la hauteur de la fenêtre grandit avec le
    // catalogue (pensé pour quelques armes, comme les 2 exportées par défaut)
    let show_upgrades = scenario::has_economy(state);
    let weapons_n = if show_upgrades {
        VAISSEAU_WEAPONS.len().min(WEAPON_SLOTS)
    } else {
        0
    };
    // lignes AMMO du ravitaillement : une par **arme possédée** (seule une
    // arme possédée se recharge) - le canon classique de repli (catalogue
    // vide) compte pour une ; les armes payantes s'ajoutent à leur achat
    let slots = scenario::weapon_slot_count();
    let ammo_rows = (0..slots).filter(|&i| scenario::weapon_owned(state, i)).count();
    let w = 540.0;
    let h = shop_box_height(show_upgrades, weapons_n, ammo_rows);
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let row_w = w - 2.0 * BOX_PADDING;
    let rows_top = top + 3.0 * BOX_PADDING + 22.0;
    // les lignes s'empilent : armes (en tête), ravitaillement (carburant et
    // munitions, indépendants), atelier (extensions), puis la section des
    // modes en dessous - tout n'existe qu'en scénario à économie
    let mut y = rows_top;
    let mut weapons = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    if weapons_n > 0 {
        y += 14.0; // en-tête « ARMES » (dessiné à y − 12)
        for i in 0..weapons_n {
            weapons[i] = Rect::new(left + BOX_PADDING, y, row_w, 34.0);
            y += 36.0;
        }
        y += 2.0;
    }
    let (supplies_fuel, slider_fuel, supplies_ammo, slider_ammo, fuel, ammo, cargo, modes_top) =
        if show_upgrades {
            y += 14.0; // en-tête « RAVITAILLEMENT »
            // ligne FUEL : libellé à gauche, piste du curseur au centre,
            // coût à droite - le clic hors piste achète la quantité choisie
            let sf = Rect::new(left + BOX_PADDING, y, row_w, 34.0);
            let sfu = Rect::new(left + 184.0, y + 10.0, row_w - 184.0 - 132.0, 14.0);
            y += 36.0;
            // lignes AMMO : une par arme possédée (index catalogue)
            let mut sa = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
            let mut sau = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
            for i in 0..slots {
                if scenario::weapon_owned(state, i) {
                    sa[i] = Rect::new(left + BOX_PADDING, y, row_w, 34.0);
                    sau[i] = Rect::new(left + 184.0, y + 10.0, row_w - 184.0 - 132.0, 14.0);
                    y += 36.0;
                }
            }
            y += 2.0;
            // atelier : trois lignes d'extension (réservoir, chargeur, soute)
            let af = Rect::new(left + BOX_PADDING, y, row_w, 28.0);
            let aa = Rect::new(left + BOX_PADDING, y + 30.0, row_w, 28.0);
            let ac = Rect::new(left + BOX_PADDING, y + 60.0, row_w, 28.0);
            y += 3.0 * 30.0 + 2.0;
            (sf, sfu, sa, sau, af, aa, ac, y)
        } else {
            (
                Rect::new(0.0, 0.0, 0.0, 0.0),
                Rect::new(0.0, 0.0, 0.0, 0.0),
                [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS],
                [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS],
                Rect::new(0.0, 0.0, 0.0, 0.0),
                Rect::new(0.0, 0.0, 0.0, 0.0),
                Rect::new(0.0, 0.0, 0.0, 0.0),
                rows_top,
            )
        };
    let modes = [
        Rect::new(left + BOX_PADDING, modes_top + 24.0, row_w, 34.0),
        Rect::new(left + BOX_PADDING, modes_top + 60.0, row_w, 34.0),
        Rect::new(left + BOX_PADDING, modes_top + 96.0, row_w, 34.0),
        Rect::new(left + BOX_PADDING, modes_top + 132.0, row_w, 34.0),
    ];
    ShopBoxLayout {
        weapons,
        supplies_fuel,
        slider_fuel,
        supplies_ammo,
        slider_ammo,
        fuel,
        ammo,
        cargo,
        modes,
        close: Rect::new(left + w - BOX_PADDING - 90.0, top + h - 20.0 - 26.0, 90.0, 26.0),
    }
}

/// Dessine le magasin de la station : la section « MOVING MODE » (une ligne
/// par mode de déplacement, ordre `MOVING_MODE_ORDER` - nom + description +
/// état SELECTED / coût de déblocage / FREE, clic = sélection ou achat), les
/// lignes d'extension (réservoir, chargeur, soute - scénario à économie) et
/// le bouton CLOSE.
pub fn draw_shop_box(state: &GameState) {
    let show_upgrades = scenario::has_economy(state);
    let l = shop_box_layout(state);
    let w = 540.0;
    // même hauteur que la géométrie : la section « ARMES » (économie) fait
    // grandir la fenêtre avec le catalogue, la section RAVITAILLEMENT avec
    // les armes possédées
    let weapons_n = if show_upgrades {
        VAISSEAU_WEAPONS.len().min(WEAPON_SLOTS)
    } else {
        0
    };
    let ammo_rows = (0..scenario::weapon_slot_count())
        .filter(|&i| scenario::weapon_owned(state, i))
        .count();
    let h = shop_box_height(show_upgrades, weapons_n, ammo_rows);
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré
    let title = "*** PLACE DE MARCHÉ ***";
    let text_w = measure_text(title, None, 16, 1.0).width;
    draw_text_shadow(
        title,
        left + (w - text_w) / 2.0,
        top + 2.0 * BOX_PADDING + 12.0,
        16.0,
        argb_to_color(BOX_FG),
    );

    let m = mouse_to_game();

    // section « ARMES » (scénario à économie, catalogue non vide) : une ligne
    // par arme - nom (16 px) + état à droite (OWNED / prix d'achat, comme les
    // modes : « base → prix remisé (RANG) » quand la réputation réduit le
    // coût) et prix/taille du paquet de munitions en dessous (12 px, sombre) -
    // survol blanc (clic = achat ; une arme possédée n'est pas cliquable)
    if show_upgrades && weapons_n > 0 {
        let header = l.weapons[0].y - 12.0;
        draw_text_shadow("ARMES", left + BOX_PADDING + 4.0, header, 16.0, argb_to_color(BOX_FG));
        for (i, rect) in l.weapons.iter().enumerate().take(weapons_n) {
            let spec = scenario::weapon_spec(i);
            let owned = scenario::weapon_owned(state, i);
            let color = argb_to_color(if !owned && rect.contains(m) { BOX_HOVER } else { BOX_FG });
            let status = if owned {
                "OWNED".to_string()
            } else {
                match scenario::weapon_prices(state, i) {
                    Some((base, discounted)) if discounted < base => format!(
                        "{} → {} MIN ({})",
                        base,
                        discounted,
                        scenario::current_rank(state).unwrap_or("")
                    ),
                    Some((base, _)) => format!("{} MIN", base),
                    None => "FREE".to_string(),
                }
            };
            draw_text_shadow(
                &format!("W{} {}", i + 1, spec.name),
                rect.x + 4.0,
                rect.y + 16.0,
                16.0,
                color,
            );
            let status_w = measure_text(&status, None, 16, 1.0).width;
            draw_text_shadow(&status, rect.x + rect.w - 4.0 - status_w, rect.y + 16.0, 16.0, color);
            draw_text_shadow(
                &format!("MUNITIONS: {} m / {} u", spec.ammo_price, spec.ammo_pack),
                rect.x + 4.0,
                rect.y + 32.0,
                16.0,
                argb_to_color(BOX_FG_DIM),
            );
        }
    }

    // section « RAVITAILLEMENT » (scénario à économie) : le carburant et les
    // munitions s'achètent **indépendamment** (plus de bouton REFUEL/REARM
    // dans la boîte DOCK STATION), **à la quantité** - une ligne par
    // ressource (FUEL, puis AMMO par arme possédée) : état courant à gauche,
    // curseur au centre (glisser / molette, même style que la barre de
    // volume des réglages), coût de la quantité choisie à droite ; survol =
    // blanc, clic sur la ligne hors piste = achat de la quantité
    if show_upgrades {
        let header = l.supplies_fuel.y - 12.0;
        draw_text_shadow(
            "RAVITAILLEMENT",
            left + BOX_PADDING + 4.0,
            header,
            16.0,
            argb_to_color(BOX_FG),
        );
        // FUEL : réservoir courant + curseur (manque du réservoir) + coût
        let fuel_cap = scenario::fuel_capacity(state);
        let fuel_missing = (fuel_cap - state.resources.fuel).max(0.0);
        let fuel_color = argb_to_color(if l.supplies_fuel.contains(m) { BOX_HOVER } else { BOX_FG });
        let fuel_txt = if fuel_missing <= 0.0 {
            format!("FUEL: {:.0}/{:.0} (PLEIN)", state.resources.fuel, fuel_cap)
        } else {
            format!("FUEL: {:.0}/{:.0}", state.resources.fuel, fuel_cap)
        };
        draw_text_shadow(&fuel_txt, l.supplies_fuel.x + 4.0, l.supplies_fuel.y + 22.0, 16.0, fuel_color);
        draw_supply_slider(l.slider_fuel, fuel_missing, state.shop_fuel_qty, m);
        let fuel_qty = state.shop_fuel_qty;
        let fuel_packs = (fuel_missing > 0.0 && fuel_qty > 0.0).then(|| {
            let n = scenario::fuel_pack_count(state, fuel_qty);
            format!("({} paquet{})", n, if n > 1 { "s" } else { "" })
        });
        draw_supply_cost(
            l.supplies_fuel,
            if fuel_missing <= 0.0 {
                "PLEIN".to_string()
            } else if fuel_qty <= 0.0 {
                "-".to_string()
            } else {
                format!("+{:.0} → {} MIN", fuel_qty, scenario::fuel_qty_cost(state, fuel_qty))
            },
            fuel_packs,
            fuel_color,
        );

        // AMMO : une ligne par arme possédée - chargeur courant + curseur
        // (manque de l'arme, paquet propre à l'arme) + coût
        let ammo_cap = scenario::ammo_capacity(state);
        for (i, rect) in l.supplies_ammo.iter().enumerate() {
            if rect.w <= 0.0 {
                continue;
            }
            let spec = scenario::weapon_spec(i);
            let missing = (ammo_cap - state.resources.weapon_ammo[i]).max(0);
            let qty = state.shop_ammo_qty[i] as i32;
            let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
            let ammo_txt = if missing <= 0 {
                format!("{}: {}/{} (PLEIN)", spec.name, state.resources.weapon_ammo[i], ammo_cap)
            } else {
                format!("{}: {}/{}", spec.name, state.resources.weapon_ammo[i], ammo_cap)
            };
            draw_text_shadow(&ammo_txt, rect.x + 4.0, rect.y + 22.0, 16.0, color);
            draw_supply_slider(l.slider_ammo[i], missing as f64, state.shop_ammo_qty[i], m);
            let ammo_packs = (missing > 0 && qty > 0).then(|| {
                let n = scenario::ammo_pack_count(state, i, qty);
                format!("({} paquet{})", n, if n > 1 { "s" } else { "" })
            });
            draw_supply_cost(
                *rect,
                if missing <= 0 {
                    "PLEIN".to_string()
                } else if qty <= 0 {
                    "-".to_string()
                } else {
                    format!("+{} → {} MIN", qty, scenario::ammo_qty_cost(state, i, qty))
                },
                ammo_packs,
                color,
            );
        }
    }

    // lignes d'extension (scénario à économie) : libellé, capacité,
    // prochaine extension (+bonus, coût) ou MAX - survol = blanc (clic =
    // achat)
    if show_upgrades {
        for (rect, track) in [
            (l.fuel, crate::scenario::UpgradeTrackId::Fuel),
            (l.ammo, crate::scenario::UpgradeTrackId::Ammo),
            (l.cargo, crate::scenario::UpgradeTrackId::Cargo),
        ] {
            let line = crate::scenario::upgrade_line(state, track);
            let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
            let text = match line.next {
                Some(u) => format!(
                    "{}: {} → {} (+{}) - {} MIN",
                    line.label, line.capacity, u.name, u.bonus, u.cost
                ),
                None => format!("{}: {} (MAX)", line.label, line.capacity),
            };
            draw_text_shadow(&text, rect.x + 4.0, rect.y + 18.0, 16.0, color);
        }
    }

    // section « MOVING MODE » : une ligne par mode (ordre visuel
    // `MOVING_MODE_ORDER`) - nom (16 px) + description (12 px, sombre) à
    // gauche, état à droite (SELECTED pour le mode courant, prix de
    // déblocage pour un mode verrouillé - « base → prix remisé (RANG) »
    // quand la réputation du rang courant réduit le coût, FREE sinon) -
    // survol blanc (clic = sélection gratuite ou déblocage contre minerais)
    let modes_header = l.modes[0].y - 12.0;
    draw_text_shadow("MOVING MODE", left + BOX_PADDING + 4.0, modes_header, 16.0, argb_to_color(BOX_FG));
    for (i, rect) in l.modes.iter().enumerate() {
        let mode = MOVING_MODE_ORDER[i];
        let catalog = MOVING_MODES[mode as usize];
        let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
        let status = if state.moving_mode == mode {
            "SELECTED".to_string()
        } else {
            match scenario::mode_unlock_prices(state, mode) {
                // mode verrouillé : tarif de base → prix remisé par la
                // réputation du rang courant (le rang est nommé quand une
                // remise s'applique réellement)
                Some((base, discounted)) if discounted < base => format!(
                    "{} → {} MIN ({})",
                    base,
                    discounted,
                    scenario::current_rank(state).unwrap_or("")
                ),
                Some((base, _)) => format!("{} MIN", base),
                None => "FREE".to_string(),
            }
        };
        draw_text_shadow(catalog.name, rect.x + 4.0, rect.y + 16.0, 16.0, color);
        let status_w = measure_text(&status, None, 16, 1.0).width;
        draw_text_shadow(&status, rect.x + rect.w - 4.0 - status_w, rect.y + 16.0, 16.0, color);
        draw_text_shadow(catalog.description, rect.x + 4.0, rect.y + 32.0, 16.0, argb_to_color(BOX_FG_DIM));
    }

    // retour à la boîte DOCK STATION
    draw_box_button("CLOSE", l.close);
}

/// Dessine la **quantité + coût** d'une ligne du ravitaillement (droite de
/// la ligne) : « +30 → 5 MIN » (quantité sélectionnée sur le curseur et son
/// montant) en couleur de la ligne (hover blanc), et le **nombre de
/// paquets** en dessous (« (3 paquets) », 12 px discret). « PLEIN » quand
/// rien ne manque, « - » quand rien n'est sélectionné (aucun minerai).
fn draw_supply_cost(rect: Rect, txt: String, packs: Option<String>, color: Color) {
    let w = measure_text(&txt, None, 16, 1.0).width;
    draw_text_shadow(&txt, rect.x + rect.w - 4.0 - w, rect.y + 22.0, 16.0, color);
    if let Some(packs) = packs {
        let pw = measure_text(&packs, None, 12, 1.0).width;
        draw_text_shadow(&packs, rect.x + rect.w - 4.0 - pw, rect.y + 32.0, 16.0, argb_to_color(BOX_FG_DIM));
    }
}

/// Dessine un **curseur de quantité** du ravitaillement du magasin (même
/// style que la barre de volume des réglages) : piste sombre, portion
/// remplie selon `value` sur un maximum `max` et pouce vertical - survol
/// blanc (glisser / molette = quantité à acheter, `game.rs`).
fn draw_supply_slider(track: Rect, max: f64, value: f64, m: Vec2) {
    if max <= 0.0 || track.w <= 0.0 {
        return; // réservoir plein : pas de curseur
    }
    let frac = (value / max).clamp(0.0, 1.0) as f32;
    let color = argb_to_color(if track.contains(m) { BOX_HOVER } else { BOX_FG });
    let bar_y = track.y + (track.h - 6.0) / 2.0;
    let fill = track.w * frac;
    draw_rectangle(track.x, bar_y, track.w, 6.0, argb_to_color(0x601AB2FF));
    draw_rectangle(track.x, bar_y, fill, 6.0, color);
    // pouce (ascenseur) : barre verticale de 14 px, centrée sur la piste
    let thumb_x = (track.x + fill - 2.0).clamp(track.x, track.x + track.w - 4.0);
    draw_rectangle(thumb_x, bar_y - 4.0, 4.0, 14.0, color);
}

/// Dessine un bouton de la boîte de choix (cadre + texte centré, hover blanc).
fn draw_box_button(label: &str, rect: Rect) {
    let m = mouse_to_game();
    let hovered = rect.contains(m);
    let color = argb_to_color(if hovered { BOX_HOVER } else { BOX_FG });
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, color);
    let text_w = measure_text(label, None, 16, 1.0).width;
    draw_text_shadow(label, rect.x + (rect.w - text_w) / 2.0, rect.y + 18.0, 16.0, color);
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
    // 16 px d'écart) - la touche T est listée mais non implémentée dans
    // l'original (bloc commenté), on la conserve telle quelle
    let labels = [
        "P : pause",
        "S : show keys (this screen)",
        "T : dump triangles to console",
        "A : switch automatic shape generation",
        "D : display data",
        "F : cycle window / zoomed / native fullscreen",
        "G : generate a shape",
        "O : settings (audio, graphics)",
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

// ─── Écran de paramétrage (touche O) ────────────────────────────────────────

/// Géométrie des contrôles de l'écran de paramétrage : fenêtre 560×280
/// centrée en deux colonnes - à gauche les cases MUSIC, AUTO GENERATE et
/// TOUCH UI, la barre horizontale du volume (ascenseur) et le bouton RESET
/// PROGRESSION (pleine largeur de la colonne) ; à droite le panneau
/// « GRAPHICS » (style de rendu, mode d'affichage fenêtré/plein écran,
/// définition de fenêtre, anticrénelage) ; les boutons RESET et CLOSE côte
/// à côte en bas. (Le mode de déplacement se choisit désormais au magasin de
/// la station - bouton SHOP de la boîte DOCK STATION.)
pub struct SettingsLayout {
    /// Ligne cliquable de la case MUSIC.
    pub music: Rect,
    /// Ligne cliquable de la case AUTO GENERATE.
    pub auto_generate: Rect,
    /// Barre horizontale du volume (ascenseur) : zone cliquable/glissable de
    /// 22 px de haut, avec la piste de 6 px centrée à l'intérieur.
    pub volume_track: Rect,
    /// Panneau des options graphiques (fond + bordure + libellé « GRAPHICS »).
    pub graphics_panel: Rect,
    /// Ligne RENDER : style de rendu des triangles (clic = cycle TEXTURED →
    /// COLORED → MESH).
    pub render: Rect,
    /// Ligne WINDOW : mode d'affichage (clic = cycle WINDOWED → ZOOMED →
    /// NATIVE).
    pub window_mode: Rect,
    /// Ligne SIZE : définition de la fenêtre (clic = cycle 960×540 → …).
    pub window_size: Rect,
    /// Ligne cliquable de la case ANTIALIAS (MSAA, appliquée au lancement).
    pub antialias: Rect,
    /// Bouton RESET PROGRESSION (remet à zéro la progression du scénario -
    /// minerais, modes payés, réputation, extensions, vies/bouclier ; visible
    /// seulement en scénario à économie ou à survie).
    pub reset_progress: Rect,
    /// Ligne cliquable de la case TOUCH UI (interface tactile bas-gauche /
    /// bas-droite, `touch.rs`).
    pub touch_ui: Rect,
    /// Bouton RESET (réglages par défaut).
    pub reset: Rect,
    /// Bouton RESTART (relance le jeu - affiché uniquement quand un réglage
    /// modifié exige un redémarrage, ex l'anticrénelage).
    pub restart: Rect,
    /// Bouton CLOSE (ferme l'écran).
    pub close: Rect,
}

/// Calcule la géométrie de l'écran de paramétrage (voir `SettingsLayout`).
pub fn settings_box_layout() -> SettingsLayout {
    let w = 560.0;
    let h = 280.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let col_w = 250.0;
    let col_left = left + 20.0;
    let col_right = left + w - 20.0 - col_w;

    // colonne gauche : cases audio + volume + bouton RESET PROGRESSION
    let music = Rect::new(col_left, top + 44.0, col_w, 26.0);
    let auto_generate = Rect::new(col_left, top + 76.0, col_w, 26.0);
    // volume : barre horizontale (ascenseur) sur la majeure partie de la
    // ligne, après le libellé VOLUME ; zone de clic de 22 px de haut
    let volume_track = Rect::new(col_left + 100.0, top + 110.0, col_w - 104.0, 22.0);
    // RESET PROGRESSION : bouton pleine largeur de la colonne gauche, sous le
    // volume (remet à zéro la progression du scénario courant)
    let reset_progress = Rect::new(col_left, top + 158.0, col_w, 26.0);
    // TOUCH UI : case à cocher sous RESET PROGRESSION (interface tactile
    // joystick + bouton de tir, `touch.rs`)
    let touch_ui = Rect::new(col_left, top + 184.0, col_w, 26.0);

    // colonne droite : panneau des options graphiques
    let graphics_panel = Rect::new(col_right, top + 44.0, col_w, 176.0);
    let row_w = col_w - 20.0;
    let render = Rect::new(col_right + 10.0, top + 66.0, row_w, 26.0);
    let window_mode = Rect::new(col_right + 10.0, top + 96.0, row_w, 26.0);
    let window_size = Rect::new(col_right + 10.0, top + 126.0, row_w, 26.0);
    let antialias = Rect::new(col_right + 10.0, top + 156.0, row_w, 26.0);

    // boutons en bas : RESET à gauche, CLOSE à droite (ex
    // `windowUtils_choiceBox` : 1er sur la moitié gauche, 2e sur la moitié
    // droite) et RESTART au centre - affiché seulement si un redémarrage est
    // nécessaire
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let btn_h = 26.0;
    let w1 = btn_w("RESET");
    let w2 = btn_w("CLOSE");
    let w3 = btn_w("RESTART");
    let left1 = left + (w / 2.0 - w1) / 2.0 - BOX_PADDING;
    let left2 = left + (3.0 * w / 2.0 - w2) / 2.0 - BOX_PADDING;
    let top_btn = top + h - 20.0 - btn_h;
    let reset = Rect::new(left1, top_btn, w1, btn_h);
    let close = Rect::new(left2, top_btn, w2, btn_h);
    let restart = Rect::new(left + (w - w3) / 2.0 - BOX_PADDING, top_btn, w3, btn_h);

    SettingsLayout {
        music,
        auto_generate,
        volume_track,
        graphics_panel,
        render,
        window_mode,
        window_size,
        antialias,
        reset_progress,
        touch_ui,
        reset,
        restart,
        close,
    }
}

/// Dessine l'écran de paramétrage (touche O) : fond, bordure, titre, les
/// deux colonnes (audio + RESET PROGRESSION à gauche, panneau « GRAPHICS » à
/// droite) et les boutons RESET / CLOSE (ex `windowUtils`). `sounds` fournit
/// l'état musique et le volume courant.
pub fn draw_settings_box(state: &GameState, sounds: &Sounds) {
    let w = 560.0;
    let h = 280.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let msg = "*** SETTINGS ***";
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text_shadow(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    let layout = settings_box_layout();
    let m = mouse_to_game();

    // cases à cocher MUSIC (état depuis les sons) et AUTO GENERATE
    draw_checkbox(layout.music, sounds.music_on, "MUSIC", m);
    draw_checkbox(layout.auto_generate, state.auto_generate, "AUTO GENERATE", m);

    // volume : barre horizontale (ascenseur) - piste, remplissage selon le
    // volume et curseur vertical ; valeur en % centrée sous la barre (hover
    // blanc sur toute la zone)
    let track = layout.volume_track;
    let vol_pct = (sounds.volume * 100.0).round() as i32;
    let color = argb_to_color(if track.contains(m) { BOX_HOVER } else { BOX_FG });
    draw_text("VOLUME", layout.music.x + 4.0, track.y + 15.0, 16.0, color);
    let bar_y = track.y + (track.h - 6.0) / 2.0;
    let fill = track.w * sounds.volume.clamp(0.0, 1.0);
    draw_rectangle(track.x, bar_y, track.w, 6.0, argb_to_color(0x601AB2FF));
    draw_rectangle(track.x, bar_y, fill, 6.0, color);
    // curseur (ascenseur) : barre verticale de 14 px, centrée sur la piste
    let thumb_x = (track.x + fill - 2.0).clamp(track.x, track.x + track.w - 4.0);
    draw_rectangle(thumb_x, bar_y - 4.0, 4.0, 14.0, color);
    let value = format!("{}%", vol_pct);
    let value_w = measure_text(&value, None, 16, 1.0).width;
    draw_text(
        &value,
        track.x + (track.w - value_w) / 2.0,
        track.y + track.h + 4.0,
        16.0,
        argb_to_color(BOX_FG_DIM),
    );

    // panneau GRAPHICS : fond + bordure + libellé en tête, puis les lignes
    // RENDER / WINDOW / SIZE (valeurs cyclables dans un cadre) et la case
    // ANTIALIAS ; note si l'anticrénelage n'est effectif qu'au lancement
    let g = layout.graphics_panel;
    draw_rectangle(g.x, g.y, g.w, g.h, argb_to_color(BOX_PANEL_BG));
    draw_rectangle_lines(g.x, g.y, g.w, g.h, 1.0, argb_to_color(BOX_PANEL_BORDER));
    draw_text("GRAPHICS", g.x + 10.0, g.y + 14.0, 16.0, argb_to_color(BOX_FG));
    draw_cycle_row(layout.render, "RENDER", render_style_label(state.render_style as i32), m);
    draw_cycle_row(layout.window_mode, "WINDOW", window_mode_label(state.view_mode as i32), m);
    draw_cycle_row(layout.window_size, "SIZE", &window_size_label(state.window_size), m);
    draw_checkbox(layout.antialias, state.antialias, "ANTIALIAS", m);

    // un réglage modifié qui n'est effectif qu'au lancement (l'anticrénelage)
    // et diffère de la valeur appliquée par la fenêtre : note + bouton
    // RESTART (relance le jeu, les réglages étant déjà enregistrés)
    if state.antialias != state.antialias_applied {
        draw_text(
            "RESTART REQUIRED",
            g.x + 30.0,
            layout.antialias.y + 40.0,
            16.0,
            argb_to_color(BOX_FG_DIM),
        );
        draw_box_button("RESTART", layout.restart);
    }

    // RESET PROGRESSION : remet à zéro la progression du scénario courant
    // (minerais, modes payés, réputation, extensions, vies/bouclier) - affiché
    // seulement quand il y a une progression à remettre (scénario à économie
    // ou à survie) ; en jeu libre, rien à réinitialiser
    if scenario::has_economy(state) || scenario::has_survival(state) {
        draw_box_button("RESET PROGRESSION", layout.reset_progress);
    }
    draw_checkbox(layout.touch_ui, state.touch_ui, "TOUCH UI", m);
    // télécommande : rappel de l'URL de la page de contrôle (le téléphone
    // pilote le vaisseau sur le réseau local - voir `remote.rs`), en bas de
    // la colonne gauche, au-dessus des boutons RESET / CLOSE
    if let Some(url) = crate::remote::url() {
        draw_text(
            &format!("REMOTE: {url}"),
            layout.music.x + 4.0,
            top + 218.0,
            16.0,
            argb_to_color(BOX_FG_DIM),
        );
    }
    draw_box_button("RESET", layout.reset);
    draw_box_button("CLOSE", layout.close);
}

/// Dessine une ligne de réglage cyclable (RENDER / WINDOW / SIZE) : libellé
/// à gauche, valeur dans un petit cadre à droite (clic = cycle, hover blanc).
fn draw_cycle_row(rect: Rect, label: &str, value: &str, m: Vec2) {
    let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
    draw_text(label, rect.x + 4.0, rect.y + 18.0, 16.0, color);
    let value_w = measure_text(value, None, 16, 1.0).width;
    let value_x = rect.x + rect.w - 4.0 - value_w;
    draw_rectangle_lines(value_x - 6.0, rect.y + 3.0, value_w + 12.0, 18.0, 1.0, color);
    draw_text(value, value_x, rect.y + 17.0, 16.0, color);
}

/// Dessine une case à cocher (carré 14×14 + libellé à droite, hover blanc) ;
/// cochée = croix de validation.
fn draw_checkbox(rect: Rect, checked: bool, label: &str, m: Vec2) {
    let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
    let x = rect.x + 4.0;
    let y = rect.y + 6.0;
    draw_rectangle_lines(x, y, 14.0, 14.0, 1.5, color);
    if checked {
        draw_line(x + 2.0, y + 7.0, x + 6.0, y + 11.0, 2.0, color);
        draw_line(x + 6.0, y + 11.0, x + 12.0, y + 3.0, 2.0, color);
    }
    draw_text(label, rect.x + 26.0, rect.y + 18.0, 16.0, color);
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
/// les triangles morts restent en pointillés.
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
    draw_triangle_lines(a, b, c, 1.0, fade_color(triangle_color(t, shape, elements), fade));
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
/// `game::cosmonaut_controls`) : une flamme extérieure orange semi-transparente
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
    // soute presque pleine : les baies occupées clignotent (elles alternent
    // leur couleur ↔ rouge tant que le cargo reste à `HUD_FULL_CARGO_RATIO`
    // de sa capacité - les emplacements vides gardent leur contour gris)
    let almost_full = state.player.cargo_size > 0
        && state.player.cargo_qty as f64 / state.player.cargo_size as f64 >= HUD_FULL_CARGO_RATIO;
    let blink_on = almost_full && (get_time() * HUD_BLINK_HZ) as i64 % 2 == 0;
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
            let fill = if blink_on { HUD_WARN_COLOR } else { color };
            draw_circle(x, 50.0, 5.0, argb_to_color(fill));
        } else {
            draw_circle_lines(x, 50.0, 5.0, 1.0, argb_to_color(color));
        }
    }
}

// ─── Accostage : mire au centre de la station + HUD d'approche ──────────────

/// Transparence (octet alpha ARGB) de la mire d'accostage : discrète (canal
/// alpha volontairement bas, comme les liens d'accostage) - l'anneau et la
/// croix sont plus légers que le point central (l'effet néon empile des
/// halos dérivés de ces alphas, voir `neon_ring`/`neon_line`/`neon_dot`).
const DOCK_MARKER_ALPHA: u32 = 0x66;
const DOCK_MARKER_DOT_ALPHA: u32 = 0x99;

/// Qualité de l'approche pour la mire (0 = rouge, 1 = vert) : interpolée sur
/// **tout le rayon de la base** (0 au bord du rayon, 1 au centre) et sur la
/// vitesse (0 à `DOCK_APPROACH_FULL_RED_SPEED` ou plus, 1 à l'arrêt) - la
/// mire réagit dès que le vaisseau entre dans le rayon de la station.
fn docking_approach_quality(dist: f64, speed: f64, station_radius: f64) -> f64 {
    let dist_q = 1.0 - (dist / station_radius).clamp(0.0, 1.0);
    let speed_q = 1.0 - (speed.abs() / DOCK_APPROACH_FULL_RED_SPEED).clamp(0.0, 1.0);
    dist_q * speed_q
}

/// Effet néon d'un anneau : halo (3 cercles concentriques d'alpha décroissant
/// - macroquad n'a pas de flou, on empile) + anneau principal + cœur clair.
fn neon_ring(x: f32, y: f32, radius: f32, color: Color) {
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
fn neon_line(x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
    let mut halo = color;
    halo.a = color.a * 0.3;
    draw_line(x1, y1, x2, y2, 3.0, halo);
    draw_line(x1, y1, x2, y2, 1.2, color);
    let bright = Color::new(1.0, 1.0, 1.0, color.a * 0.6);
    draw_line(x1, y1, x2, y2, 0.6, bright);
}

/// Effet néon d'un point : halo + cœur + point brillant.
fn neon_dot(x: f32, y: f32, radius: f32, color: Color) {
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
    let dist = (player_position.x - station_position.x).hypot(player_position.y - station_position.y);
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

/// Transparence (octet alpha ARGB) des liens d'accostage : canal alpha bas
/// (comme la mire) pour que les 4 liens simultanés restent discrets et
/// laissent voir le vaisseau - l'effet néon empile des halos dérivés de cet
/// alpha (voir `neon_line`).
const DOCK_LINE_ALPHA: u32 = 0x66;

/// Distance (unités monde, du centre du vaisseau) des points de branchement
/// des liens d'accostage : les 4 liens se connectent en diagonale (NO, SO,
/// SE, NE) sur un petit losange **proche du centre** - l'illusion qu'ils
/// touchent le vaisseau (ils sont dessinés dessous).
const DOCK_LINE_SHIP_ANCHOR: f64 = 5.0;

/// La mire d'accostage est-elle visible ? Elle n'est affichée **que lors du
/// retour à la base** (`state.docking_guide`, posé par
/// `game::update_docking_guide` quand le vaisseau recroise la limite
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
    let dist = (player_position.x - station_position.x).hypot(player_position.y - station_position.y);
    dist < station_radius
}

/// Déformation transversale (offset perpendiculaire, en pixels écran) d'un
/// point de câble à la fraction `t` (0 = anneau, 1 = extrémité mobile), pour
/// une intensité d'ondulation `wave` (0 = câble tendu) à l'instant `time` :
/// onde qui court vers le **vaisseau** (`toward_ship`, déploiement « en
/// projection ») ou vers l'**anneau** (rétraction), enveloppe croissante
/// vers l'extrémité mobile (`× t` - l'extrémité libre fouette).
fn cable_wave_offset(t: f32, wave: f32, time: f32, toward_ship: bool) -> f32 {
    let speed = if toward_ship { -18.0 } else { 18.0 };
    (t * 12.0 + time * speed).sin() * wave * t
}

/// Enveloppe d'ondulation du lien pendant le **désamarrage** (relâchement de
/// la tension) : maximale au largage (`r` = 0, le câble fouette), nulle une
/// fois le lien rentré (`r` = 1), légèrement pulsante entre les deux (la
/// tension se relâche par à-coups).
fn retract_envelope(r: f64) -> f64 {
    (1.0 - r) * (0.6 + 0.4 * (r * TAU * 1.5).cos())
}

/// Trace un lien d'accostage entre l'anneau (`a`) et l'extrémité mobile
/// (vers `b`), déployé à `prog` (0 = encore sur l'anneau, 1 = tendu jusqu'à
/// `b`) : pendant le **déploiement** (projection, `toward_ship = true`) ou
/// la **rétraction** (relâchement de la tension, `toward_ship = false`), le
/// câble **ondule** avec l'intensité `wave` (0 = câble tendu, voir
/// `cable_wave_offset`) ; une fois tendu, il est droit. Dessiné en segments
/// néon (même rendu que `neon_line`).
fn draw_docking_cable(a: Vec2, b: Vec2, prog: f32, wave: f32, toward_ship: bool, color: Color) {
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
/// cordon **orange** jaillit de l'anneau jusqu'au cosmonaute (déploiement sur
/// les ~30 % du début) puis, tendu, le **ramène sur l'anneau** - son
/// ondulation s'affaisse à mesure que la tension monte ; pendant le fondu
/// enchaîné (`state.eva_crossfade > 0`), il reste tendu et **s'efface avec le
/// cosmonaute**. Dessiné **sous le cosmonaute** (appelé avant son rendu).
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
        // vers le cosmonaute (qui reste sur place) pendant la fraction
        // `EVA_CABLE_DEPLOY_FRACTION`, puis, complètement tendu, le ramène
        // sur l'anneau - ondulation forte tant que le câble est lâche
        // (déploiement), nulle une fois la tension installée (traction)
        let t = (1.0 - state.eva_recovery / EVA_RECOVERY_DURATION).clamp(0.0, 1.0) as f32;
        let deploy = (t / EVA_CABLE_DEPLOY_FRACTION as f32).clamp(0.0, 1.0);
        let wave = 10.0 * (1.0 - t);
        draw_docking_cable(a, b, deploy, wave, true, color);
    }
}

/// HUD d'accostage, affiché à la **suite des stats** (même ligne, en haut de
/// l'écran) : distance du vaisseau au centre de la station (unités monde,
/// sans unité affichée) et invite - « DOCK DIST: 123 » en approche,
/// « DOCK: SLOW DOWN » (rouge) dans la zone mais trop rapide pour accoster,
/// « DOCK: IN RANGE » (vert) dans la zone et presque immobile, « DOCKED » à
/// quai (liens attachés au lancement/respawn ou boîte ouverte). La zone
/// elle-même est visible via la mire (`draw_docking_marker`). `x` est
/// l'abscisse de départ - l'emplacement fixe du statut, renvoyé par
/// `draw_hud`. La distance occupe une largeur fixe de 4 chiffres (alignée à
/// droite) : l'affichage ne tremble pas quand elle change.
pub fn draw_docking_hud(
    state: &GameState,
    player_position: Point,
    station_position: Point,
    player_speed: f64,
    x: f32,
) {
    let dist = (player_position.x - station_position.x).hypot(player_position.y - station_position.y);
    let in_zone = dist < STATION_DOCK_DISTANCE;
    // récupération du cosmonaute / fondu enchaîné : considéré comme accosté
    let (text, color) = if state.dock_box
        || state.shop_box
        || state.dock_links
        || state.eva_recovery > 0.0
        || state.eva_crossfade > 0.0
    {
        ("DOCKED".to_string(), 0xFF40FF40)
    } else if in_zone && player_speed.abs() < STATION_DOCK_SPEED {
        ("DOCK: IN RANGE".to_string(), 0xFF40FF40)
    } else if in_zone {
        ("DOCK: SLOW DOWN".to_string(), 0xFFFF3C00)
    } else {
        (format!("DOCK DIST: {:>4.0}", dist), 0xFFFFFFFF)
    };
    draw_text(&text, x, 14.0, 16.0, argb_to_color(color));
}

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
const HUD_FPS_COL: i32 = 1;
/// Colonne de départ de la réputation (+ rang) - champ fixe de 4 chiffres.
const HUD_REPUTATION_COL: i32 = 15;
/// Colonne de départ de la précision - champ fixe de 3 chiffres (max 100).
const HUD_PRECISION_COL: i32 = 42;
/// Colonne de départ des ressources du scénario (carburant, munitions,
/// minerais - ou vies/bouclier).
const HUD_RESOURCES_COL: i32 = 57;
/// Largeur maximale (en caractères) du bloc de ressources en économie → le
/// statut d'accostage démarre juste après (colonne 97).
const HUD_RESOURCES_ECONOMY_COLS: i32 = 39;
/// Largeur maximale (en caractères) du bloc de ressources en Survival
/// (LIVES/SHIELD) → le statut d'accostage démarre à la colonne 74.
const HUD_RESOURCES_SURVIVAL_COLS: i32 = 16;

/// Fréquence (Hz) du **clignotement d'alerte** des ressources du HUD :
/// carburant/munitions presque vides et baies de chargement presque pleines
/// alternent leur couleur (blanc ↔ rouge, ~3 cycles/s - même principe que
/// le flash des règles du menu titre, `title.rs`).
const HUD_BLINK_HZ: f64 = 3.0;
/// Couleur d'alerte (ARGB) du clignotement : rouge vif (même teinte que
/// « GAME OVER »).
const HUD_WARN_COLOR: u32 = 0xFFFF4040;
/// Seuil « réserve presque vide » : le carburant / les munitions clignotent
/// au HUD tant que la réserve est **sous** cette fraction de sa capacité.
const HUD_LOW_RESERVE_RATIO: f64 = 0.25;
/// Seuil « soute presque pleine » : les baies de chargement occupées
/// clignotent dès que le cargo atteint **au moins** cette fraction de la
/// capacité (`draw_cargo`).
const HUD_FULL_CARGO_RATIO: f64 = 0.8;

/// Abscisse (px) d'une colonne de la grille 8 px (x = 8+(col-1)*8).
fn hud_col_x(col: i32) -> f32 {
    8.0 + (col - 1) as f32 * 8.0
}

/// HUD : FPS, réputation (+ rang en scénario à économie), précision et
/// ressources du scénario (carburant, munitions, minerais - ou vies/bouclier
/// en Survival) sur une **seule ligne** en haut de l'écran, à des colonnes
/// fixes (anti-tremblement). Renvoie l'abscisse de l'emplacement fixe du
/// statut d'accostage pour que `draw_docking_hud` l'affiche sur la même
/// ligne. Police macroquad par défaut en attendant la police 8×16 (Phase 4).
pub fn draw_hud(state: &GameState) -> f32 {
    // FPS : champ fixe de 3 chiffres, aligné à droite
    draw_text(
        &format!("FPS:{:>3}", state.fps),
        hud_col_x(HUD_FPS_COL),
        14.0,
        16.0,
        WHITE,
    );
    // réputation : compteur d'astéroïdes détruits (jeu libre) ou réputation
    // du scénario (économie - croît avec les destructions et la précision) ;
    // en économie, le rang courant (palier débloqué par la réputation, ex
    // CADET → PILOT → ACE) est affiché à côté - champ fixe de 4 chiffres
    let economy = scenario::has_economy(state);
    let reputation = if economy {
        state.resources.reputation as i32
    } else {
        state.meteors_destroyed
    };
    let rep_text = match scenario::current_rank(state) {
        Some(rank) => format!("REPUTATION:{:>4} ({})", reputation, rank),
        None => format!("REPUTATION:{:>4}", reputation),
    };
    draw_text(&rep_text, hud_col_x(HUD_REPUTATION_COL), 14.0, 16.0, WHITE);
    // précision : champ fixe de 3 chiffres (max 100)
    if state.bullets_fired > 0 {
        let precision = 100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64);
        draw_text(
            &format!("PRECISION:{:>3}%", precision as i32),
            hud_col_x(HUD_PRECISION_COL),
            14.0,
            16.0,
            WHITE,
        );
    }
    // ressources du scénario, sur la même ligne : carburant/munitions/minerais
    // (économie - les capacités montrent les extensions d'atelier achetées)
    // ou vies + bouclier (Survival) - champs fixes : 3/3/2/2/5 chiffres
    let dock_col = if economy {
        // blocs dessinés séparément (mêmes champs fixes → même abscisse de
        // départ pour chacun, aucune dérive) pour pouvoir **clignoter** une
        // réserve presque vide sans décaler les blocs suivants : carburant
        // et munitions alternent blanc ↔ rouge tant qu'ils restent sous
        // `HUD_LOW_RESERVE_RATIO` de leur capacité
        let fuel_cap = scenario::fuel_capacity(state);
        // munitions : totaux des armes possédées (chaque arme a son stock,
        // le HUD en montre la somme - `scenario::total_ammo`)
        let ammo_cap = scenario::total_ammo_capacity(state);
        let fuel_txt = format!("FUEL:{:>3.0}/{:>3}", state.resources.fuel, fuel_cap);
        let ammo_txt = format!(" AMMO:{:>2}/{:>2}", scenario::total_ammo(state), ammo_cap);
        let min_txt = format!(" MINERALS:{:>5}", state.resources.minerals);
        let blink_on = (get_time() * HUD_BLINK_HZ) as i64 % 2 == 0;
        let fuel_low = state.resources.fuel <= fuel_cap * HUD_LOW_RESERVE_RATIO;
        let ammo_low = scenario::total_ammo(state) as f64 <= ammo_cap as f64 * HUD_LOW_RESERVE_RATIO;
        let fuel_color = if fuel_low && blink_on { HUD_WARN_COLOR } else { 0xFFFFFFFF };
        let ammo_color = if ammo_low && blink_on { HUD_WARN_COLOR } else { 0xFFFFFFFF };
        let x = hud_col_x(HUD_RESOURCES_COL);
        draw_text(&fuel_txt, x, 14.0, 16.0, argb_to_color(fuel_color));
        let x_ammo = x + measure_text(&fuel_txt, None, 16, 1.0).width;
        draw_text(&ammo_txt, x_ammo, 14.0, 16.0, argb_to_color(ammo_color));
        let x_minerals = x_ammo + measure_text(&ammo_txt, None, 16, 1.0).width;
        draw_text(&min_txt, x_minerals, 14.0, 16.0, WHITE);
        HUD_RESOURCES_COL + HUD_RESOURCES_ECONOMY_COLS + 1
    } else if scenario::has_survival(state) {
        draw_text(
            &format!(
                "LIVES:{:>1} SHIELD:{:>1.0}",
                state.resources.lives, state.resources.shield
            ),
            hud_col_x(HUD_RESOURCES_COL),
            14.0,
            16.0,
            WHITE,
        );
        HUD_RESOURCES_COL + HUD_RESOURCES_SURVIVAL_COLS + 1
    } else {
        // jeu libre : pas de ressources - l'accostage suit PRECISION
        HUD_RESOURCES_COL
    };
    // fin de partie (Survival, dernière vie perdue) : GAME OVER au centre
    if state.game_over {
        let msg = "GAME OVER";
        let w = measure_text(msg, None, 32, 1.0).width;
        draw_text(
            msg,
            (VIEWPORT_WIDTH as f32 - w) / 2.0,
            VIEWPORT_HEIGHT as f32 / 2.0,
            32.0,
            argb_to_color(0xFFFF4040),
        );
    }
    hud_col_x(dock_col)
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
    // ligne 4, colonne 1 : minerais contenus dans les météores (somme des
    // `minerals` - libérés en gemmes quand deux météores se détruisent)
    let meteor_minerals: i32 = shapes.iter().map(|s| s.minerals).sum();
    draw_text(
        &format!("meteor minerals:{}", meteor_minerals),
        8.0,
        14.0 + 3.0 * 16.0,
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

// ─── Objectifs DAG (scénarios custom) ──────────────────────────────────────

/// Découpe `text` en plusieurs lignes qui tiennent dans `max_width` pixels à
/// la taille de police `font_size` (coupure aux espaces, sans couper les
/// mots). Permet d'afficher un objectif de scénario en entier, sans troncature.
fn wrap_text(text: &str, max_width: f32, font_size: u16) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        if !current.is_empty() && measure_text(&candidate, None, font_size, 1.0).width > max_width {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Affiche les objectifs DAG du scénario custom en cours dans le coin
/// supérieur droit de l'écran : panneau semi-transparent avec l'objectif
/// courant en grand, un sous-objectif s'il y en a un, et une barre de
/// progression. Affiché uniquement si le tracker a des objectifs.
pub fn draw_objectives_hud(state: &GameState) {
    let tracker = &state.objective_tracker;
    if !tracker.has_objectives() {
        return;
    }

    let total = tracker.total_count();
    let completed = tracker.completed_count();

    // Lister les objectifs débloqués
    let unlocked = tracker.unlocked_objectives();
    let primary = unlocked.first();
    let secondary = unlocked.get(1);

    // ── Panneau principal (coin supérieur droit) ──────────────────────────
    let panel_w: f32 = 280.0;
    let text_w = panel_w - 22.0; // marges gauche/droite
    let title_font = 16u16;
    let desc_font = 16u16;
    let sub_font = 16u16;

    // Texte calculé une seule fois (wrapping pour ne jamais tronquer)
    let title_lines = primary
        .map(|o| wrap_text(&o.title, text_w, title_font))
        .unwrap_or_default();
    let desc_lines = primary
        .map(|o| wrap_text(&o.description, text_w, desc_font))
        .unwrap_or_default();
    let sub_lines = secondary
        .map(|o| wrap_text(&format!("> {}", o.title), text_w, sub_font))
        .unwrap_or_default();

    let title_line_h = 20.0f32;
    let desc_line_h = 20.0f32;
    let sub_line_h = 20.0f32;

    // Hauteur dynamique : la boîte s'adapte à la longueur du texte complet
    let mut panel_h: f32 = 14.0; // padding haut
    panel_h += 20.0; // en-tête
    panel_h += title_lines.len() as f32 * title_line_h;
    panel_h += desc_lines.len() as f32 * desc_line_h;
    panel_h += sub_lines.len() as f32 * sub_line_h;
    panel_h += 18.0; // barre de progression
    panel_h += 10.0; // padding bas

    let panel_x = VIEWPORT_WIDTH as f32 - panel_w - 12.0;
    let panel_y = 36.0;

    // Fond semi-transparent (ombre portée)
    draw_rectangle(
        panel_x + 2.0, panel_y + 2.0, panel_w, panel_h,
        Color::new(0.0, 0.0, 0.0, 0.5),
    );
    // Fond principal (plus transparent pour laisser voir le jeu)
    draw_rectangle(
        panel_x, panel_y, panel_w, panel_h,
        Color::new(0.05, 0.07, 0.10, 0.7),
    );
    // Bordure fine
    draw_rectangle_lines(
        panel_x, panel_y, panel_w, panel_h, 1.0,
        Color::new(0.22, 0.8, 0.53, 0.55),
    );

    let mut y = panel_y + 14.0;
    let text_x = panel_x + 11.0;

    // En-tête : OBJECTIFS 2/5
    draw_text_shadow(
        &format!("OBJECTIFS {}/{}", completed, total),
        text_x, y, 16.0,
        Color::new(0.92, 0.95, 0.96, 1.0),
    );
    y += 20.0;

    // Objectif principal (le plus important) : titre puis description
    // complète, éventuellement sur plusieurs lignes
    if !title_lines.is_empty() || !desc_lines.is_empty() {
        for line in &title_lines {
            draw_text_shadow(line, text_x, y, title_font as f32, Color::new(0.22, 1.0, 0.53, 1.0));
            y += title_line_h;
        }
        for line in &desc_lines {
            draw_text_shadow(line, text_x, y, desc_font as f32, Color::new(0.84, 0.88, 0.92, 1.0));
            y += desc_line_h;
        }
    }

    // Sous-objectif (plus discret)
    for line in &sub_lines {
        draw_text_shadow(line, text_x + 6.0, y, sub_font as f32, Color::new(1.0, 0.84, 0.0, 1.0));
        y += sub_line_h;
    }

    // Barre de progression
    if total > 0 {
        let bar_w = panel_w - 22.0;
        let bar_h = 8.0;
        let bar_x = text_x;
        let progress = completed as f64 / total as f64;
        draw_rectangle(bar_x, y, bar_w, bar_h, Color::new(0.2, 0.22, 0.28, 1.0));
        draw_rectangle(
            bar_x, y, bar_w * progress as f32, bar_h,
            Color::new(0.22, 1.0, 0.53, 0.9),
        );
    }

    // ── Notification de complétion (flash au centre) ─────────────────────
    if let Some(title) = &tracker.last_completed_title {
        let timer = tracker.notification_timer;
        // Fondu : plein pendant 2.5s, puis fondu sur 1.5s
        let alpha = if timer > 1.5 {
            1.0f32
        } else {
            (timer as f32 / 1.5).max(0.0)
        };
        // Légère oscillation d'échelle pour attirer l'œil
        let pulse = 1.0 + 0.03 * (get_time() * 4.0).sin() as f32;

        let banner_w = 340.0;
        let banner_h = 48.0;
        let bx = (VIEWPORT_WIDTH as f32 - banner_w) / 2.0;
        let by = VIEWPORT_HEIGHT as f32 - 120.0;

        // Ombre
        draw_rectangle(
            bx + 3.0, by + 3.0, banner_w, banner_h,
            Color::new(0.0, 0.0, 0.0, 0.5 * alpha),
        );
        // Fond vert sombre
        draw_rectangle(
            bx, by, banner_w, banner_h,
            Color::new(0.05, 0.25, 0.1, 0.9 * alpha),
        );
        // Bordure verte vive
        draw_rectangle_lines(
            bx, by, banner_w, banner_h, 2.0,
            Color::new(0.22, 1.0, 0.53, alpha),
        );

        // Ligne 1 : ✓ OBJECTIF ATTEINT
        let check = "✓  OBJECTIF ATTEINT";
        let cw = measure_text(check, None, 16, 1.0).width * pulse;
        draw_text(
            check,
            (VIEWPORT_WIDTH as f32 - cw) / 2.0,
            by + 20.0,
            16.0 * pulse,
            Color::new(0.22, 1.0, 0.53, alpha),
        );

        // Ligne 2 : nom de l'objectif
        let tw = measure_text(title, None, 16, 1.0).width * pulse;
        draw_text(
            title,
            (VIEWPORT_WIDTH as f32 - tw) / 2.0,
            by + 40.0,
            16.0 * pulse,
            Color::new(1.0, 1.0, 1.0, alpha),
        );
    }
}

/// Formate le texte d'une condition pour l'affichage HUD.
#[allow(dead_code)]
fn format_condition_hud(cond: &crate::scenario_loader::JsonCondition, state: &GameState) -> String {
    match cond.condition_type.as_str() {
        "DestroyAsteroids" => {
            let current = state.meteors_destroyed.min(cond.required as i32);
            format!("Meteors: {}/{}", current, cond.required)
        }
        "CollectMinerals" => {
            let current = state.resources.minerals.min(cond.required as i32);
            format!("Minerals: {}/{}", current, cond.required)
        }
        "ReachReputation" => {
            let current = state.resources.reputation.min(cond.required as f64);
            format!("Reputation: {:.0}/{}", current, cond.required)
        }
        "DockAtStation" => {
            format!("Dock: {}/{}", state.docking_count.min(cond.required as i32), cond.required)
        }
        "UnlockMovementMode" => {
            let unlocked = state
                .unlocked_modes
                .get(cond.mode as usize)
                .copied()
                .unwrap_or(false);
            format!("Mode {}: {}", cond.mode, if unlocked { "DONE" } else { "locked" })
        }
        "SurviveTime" => format!("Survive: play the game"),
        "PrecisionShooting" => {
            if state.bullets_fired == 0 {
                format!("Precision: 0 hits (need {})", cond.hits)
            } else {
                let hits = state.bullets_fired - state.bullets_lost;
                let precision = 100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64);
                format!("Hits: {} ({}%)", hits, precision as i32)
            }
        }
        "BuyUpgrade" => {
            let level = match cond.track.as_str() {
                "Fuel" => state.resources.fuel_level,
                "Ammo" => state.resources.ammo_level,
                "Cargo" => state.resources.cargo_level,
                _ => 0,
            };
            format!("{}: Lvl {}/{}", cond.track, level, cond.level)
        }
        _ => cond.condition_type.clone(),
    }
}

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
            assert!(length >= 0.0 && length <= 1.0, "longueur hors borne");
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
