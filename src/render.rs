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

use crate::audio::Sounds;
use crate::config::*;
use crate::garbage::Garbage;
use crate::geom::{Point, Triangle, World};
use crate::scenario;
use crate::shape::{get_border_segments, Shape};
use crate::state::{Element, GameState, RenderStyle, ViewMode};

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

/// Une couche d'étoiles : positions (dans la tuile) + alpha de chaque étoile.
///
/// NB : on ne garde PAS de texture par couche — dessiner 15 tuiles 1024² avec
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
    /// `include_bytes!`). NB : la texture météore est embarquée en JPEG — c'est
    /// l'asset d'origine (`reference/assets/meteor_surface_tile.jpg`) — d'où la
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
/// un pixel 1×1 (quadrant blanc batché — un seul draw call pour toutes les
/// étoiles d'une couche), au lieu de dessiner la tuile 1024² entière avec
/// blending : le rendu des tuiles coûtait ~60 % du temps de frame (fill rate
/// du GPU virtio) et plafonnait le FPS à ~95 (au lieu de ~220 sans étoiles).
/// Position écran d'une étoile : `(étoile + caméra) × plan` rebouclée dans la
/// tuile périodique (torique), ex `normalizePlanPosition` de l'original.
/// Renvoie `None` si l'étoile est hors viewport.
///
/// NB : la caméra recule quand le vaisseau avance (ex `W/2 - pos`) — le signe
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
/// Panneau interne de l'écran de paramétrage (les radio-boutons de mode) :
/// fond légèrement plus clair que la fenêtre, bordure discrète.
const BOX_PANEL_BG: u32 = 0xE01478DC;
const BOX_PANEL_BORDER: u32 = 0x801AB2FF;
const BOX_PADDING: f32 = 10.0;

/// Largeur de la boîte DOCK STATION : assez pour le titre et les boutons
/// (3 sans atelier, 4 avec) sans chevauchement — même formule pour la
/// géométrie (`choice_box_layout`) et le dessin (`draw_choice_box`).
fn choice_box_width(show_upgrades: bool) -> f32 {
    let msg_w = measure_text("*** DOCK STATION ***", None, 16, 1.0).width + 2.0 * BOX_PADDING;
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let labels: &[&str] = if show_upgrades {
        &["UNLOAD", "REFUEL/REARM", "UPGRADES", "CLOSE"]
    } else {
        &["UNLOAD", "REFUEL/REARM", "CLOSE"]
    };
    let buttons: f32 =
        labels.iter().map(|l| btn_w(l)).sum::<f32>() + (labels.len() as f32 - 1.0) * BOX_PADDING;
    300.0f32.max(msg_w).max(buttons + 2.0 * BOX_PADDING)
}

/// Géométrie de la boîte de choix DOCK STATION (ex `windowUtils_choiceBox`) :
/// fenêtre de 120 px de haut centrée sur l'écran, largeur assez grande pour
/// le titre et les boutons côte à côte en bas. Renvoie les rectangles écran
/// des boutons UNLOAD / REFUEL/REARM / [UPGRADES] / CLOSE (pour la détection
/// de clic côté logique). Le bouton UPGRADES (atelier d'amélioration) n'est
/// présent qu'en scénario à économie (`show_upgrades`) — rectangle vide sinon.
pub struct ChoiceBoxLayout {
    /// Bouton UNLOAD : décharge la soute (minerais disponibles pour
    /// REFUEL/REARM juste après — la boîte reste ouverte).
    pub unload: Rect,
    /// Bouton REFUEL/REARM : achète carburant + munitions contre minerais
    /// (`scenario::purchase_supplies`) — la boîte reste ouverte.
    pub refuel: Rect,
    /// Bouton UPGRADES : ouvre l'atelier d'amélioration du vaisseau
    /// (scénario à économie) — rectangle vide sinon (aucun clic).
    pub upgrades: Rect,
    /// Bouton CLOSE : ferme la boîte.
    pub close: Rect,
}

pub fn choice_box_layout(show_upgrades: bool) -> ChoiceBoxLayout {
    let h = 120.0;
    let btn_h = 26.0;
    let w = choice_box_width(show_upgrades);
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    // boutons alignés à gauche dans la boîte (la largeur est calculée pour
    // qu'ils tiennent sans chevauchement, marges = padding)
    let btn_w = |label: &str| (measure_text(label, None, 16, 1.0).width + 2.0 * BOX_PADDING).max(60.0);
    let top_btn = top + h - 20.0 - btn_h;
    let mut x = left + BOX_PADDING;
    let mut rects = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    let labels: &[&str] = if show_upgrades {
        &["UNLOAD", "REFUEL/REARM", "UPGRADES", "CLOSE"]
    } else {
        &["UNLOAD", "REFUEL/REARM", "CLOSE"]
    };
    for (i, &label) in labels.iter().enumerate() {
        rects[i] = Rect::new(x, top_btn, btn_w(label), btn_h);
        x += rects[i].w + BOX_PADDING;
    }
    let close_i = if show_upgrades { 3 } else { 2 };
    ChoiceBoxLayout {
        unload: rects[0],
        refuel: rects[1],
        upgrades: if show_upgrades { rects[2] } else { Rect::new(0.0, 0.0, 0.0, 0.0) },
        close: rects[close_i],
    }
}

/// Dessine la boîte de choix DOCK STATION (accostage) avec ses boutons
/// UNLOAD / REFUEL/REARM / [UPGRADES] / CLOSE (hover = blanc, ex
/// `windowUtils_choiceBox`). Le bouton UPGRADES n'apparaît qu'en scénario à
/// économie (atelier disponible).
pub fn draw_choice_box(state: &GameState) {
    let show_upgrades = scenario::has_economy(state);
    let msg = "*** DOCK STATION ***";
    let w = choice_box_width(show_upgrades);
    let h = 120.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    // boutons avec survol
    let l = choice_box_layout(show_upgrades);
    draw_box_button("UNLOAD", l.unload);
    draw_box_button("REFUEL/REARM", l.refuel);
    if show_upgrades {
        draw_box_button("UPGRADES", l.upgrades);
    }
    draw_box_button("CLOSE", l.close);
}

// ─── Atelier d'amélioration du vaisseau (bouton UPGRADES) ───────────────────

/// Géométrie de l'atelier d'amélioration du vaisseau (bouton UPGRADES de la
/// boîte DOCK STATION, scénario à économie) : fenêtre centrée avec une ligne
/// cliquable par extension (réservoir, chargeur, soute) et un bouton CLOSE
/// (retour à la boîte DOCK STATION).
pub struct WorkshopBoxLayout {
    /// Ligne « réservoir de carburant » (clic = achat de l'extension).
    pub fuel: Rect,
    /// Ligne « chargeur de munitions » (clic = achat de l'extension).
    pub ammo: Rect,
    /// Ligne « soute » (clic = achat de l'extension).
    pub cargo: Rect,
    /// Bouton CLOSE : revient à la boîte DOCK STATION.
    pub close: Rect,
}

pub fn workshop_box_layout() -> WorkshopBoxLayout {
    let w = 540.0;
    let h = 200.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let row_h = 30.0;
    let row_w = w - 2.0 * BOX_PADDING;
    let rows_top = top + 3.0 * BOX_PADDING + 22.0;
    WorkshopBoxLayout {
        fuel: Rect::new(left + BOX_PADDING, rows_top, row_w, row_h),
        ammo: Rect::new(left + BOX_PADDING, rows_top + row_h + 8.0, row_w, row_h),
        cargo: Rect::new(left + BOX_PADDING, rows_top + 2.0 * (row_h + 8.0), row_w, row_h),
        close: Rect::new(left + w - BOX_PADDING - 90.0, top + h - 20.0 - 26.0, 90.0, 26.0),
    }
}

/// Dessine l'atelier d'amélioration du vaisseau : une ligne par extension
/// (réservoir, chargeur, soute) — capacité actuelle, prochaine extension avec
/// son bonus et son coût, ou « MAX » — et le bouton CLOSE. Cliquer une ligne
/// achète l'extension (`scenario::buy_upgrade`).
pub fn draw_workshop_box(state: &GameState) {
    let l = workshop_box_layout();
    let w = 540.0;
    let h = 200.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré
    let title = "*** SHIP WORKSHOP ***";
    let text_w = measure_text(title, None, 16, 1.0).width;
    draw_text(
        title,
        left + (w - text_w) / 2.0,
        top + 2.0 * BOX_PADDING + 12.0,
        16.0,
        argb_to_color(BOX_FG),
    );

    // lignes d'extension : libellé, capacité, prochaine extension (+bonus,
    // coût) ou MAX — survol = blanc (clic = achat)
    let m = mouse_to_game();
    for (rect, track) in [
        (l.fuel, crate::scenario::UpgradeTrackId::Fuel),
        (l.ammo, crate::scenario::UpgradeTrackId::Ammo),
        (l.cargo, crate::scenario::UpgradeTrackId::Cargo),
    ] {
        let line = crate::scenario::upgrade_line(state, track);
        let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
        let text = match line.next {
            Some(u) => format!(
                "{}: {} → {} (+{}) — {} MIN",
                line.label, line.capacity, u.name, u.bonus, u.cost
            ),
            None => format!("{}: {} (MAX)", line.label, line.capacity),
        };
        draw_text(&text, rect.x + 4.0, rect.y + 18.0, 16.0, color);
    }

    // retour à la boîte DOCK STATION
    draw_box_button("CLOSE", l.close);
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
        "F : cycle window / zoomed / native fullscreen",
        "G : generate a shape",
        "O : settings (moving mode, graphics)",
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

/// Géométrie des contrôles de l'écran de paramétrage : fenêtre 560×440
/// centrée en deux colonnes — à gauche le panneau « MOVING MODE » (les trois
/// radio-boutons de déplacement, cercle + libellé + description cliquables),
/// la case MUSIC, la case AUTO GENERATE et la barre horizontale du volume
/// (ascenseur) ; à droite le panneau « GRAPHICS » (style de rendu, mode
/// d'affichage fenêtré/plein écran, définition de fenêtre, anticrénelage) ;
/// les boutons RESET et CLOSE côte à côte en bas.
pub struct SettingsLayout {
    /// Panneau des modes : fond + bordure + libellé « MOVING MODE » en tête,
    /// qui regroupe les trois radio-boutons de déplacement.
    pub modes_panel: Rect,
    /// Lignes cliquables des radio-boutons de mode.
    pub modes: [Rect; 3],
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
    /// Bouton RESET (réglages par défaut).
    pub reset: Rect,
    /// Bouton RESTART (relance le jeu — affiché uniquement quand un réglage
    /// modifié exige un redémarrage, ex l'anticrénelage).
    pub restart: Rect,
    /// Bouton CLOSE (ferme l'écran).
    pub close: Rect,
}

/// Calcule la géométrie de l'écran de paramétrage (voir `SettingsLayout`).
pub fn settings_box_layout() -> SettingsLayout {
    let w = 560.0;
    let h = 440.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let col_w = 250.0;
    let col_left = left + 20.0;
    let col_right = left + w - 20.0 - col_w;

    // colonne gauche : panneau des modes + audio
    let modes_panel = Rect::new(col_left, top + 44.0, col_w, 168.0);
    let modes = [
        Rect::new(col_left, modes_panel.y + 22.0, col_w, 40.0),
        Rect::new(col_left, modes_panel.y + 70.0, col_w, 40.0),
        Rect::new(col_left, modes_panel.y + 118.0, col_w, 40.0),
    ];
    let music = Rect::new(col_left, top + 220.0, col_w, 26.0);
    let auto_generate = Rect::new(col_left, top + 252.0, col_w, 26.0);
    // volume : barre horizontale (ascenseur) sur la majeure partie de la
    // ligne, après le libellé VOLUME ; zone de clic de 22 px de haut
    let volume_track = Rect::new(col_left + 100.0, top + 286.0, col_w - 104.0, 22.0);

    // colonne droite : panneau des options graphiques
    let graphics_panel = Rect::new(col_right, top + 44.0, col_w, 176.0);
    let row_w = col_w - 20.0;
    let render = Rect::new(col_right + 10.0, top + 66.0, row_w, 26.0);
    let window_mode = Rect::new(col_right + 10.0, top + 96.0, row_w, 26.0);
    let window_size = Rect::new(col_right + 10.0, top + 126.0, row_w, 26.0);
    let antialias = Rect::new(col_right + 10.0, top + 156.0, row_w, 26.0);

    // boutons en bas : RESET à gauche, CLOSE à droite (ex
    // `windowUtils_choiceBox` : 1er sur la moitié gauche, 2e sur la moitié
    // droite) et RESTART au centre — affiché seulement si un redémarrage est
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
        modes_panel,
        modes,
        music,
        auto_generate,
        volume_track,
        graphics_panel,
        render,
        window_mode,
        window_size,
        antialias,
        reset,
        restart,
        close,
    }
}

/// Dessine l'écran de paramétrage (touche O) : fond, bordure, titre, les
/// deux colonnes (panneau « MOVING MODE » + audio à gauche, panneau
/// « GRAPHICS » à droite) et les boutons RESET / CLOSE (ex `windowUtils`).
/// `sounds` fournit l'état musique et le volume courant.
pub fn draw_settings_box(state: &GameState, sounds: &Sounds) {
    let w = 560.0;
    let h = 440.0;
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();

    // fenêtre : fond + bordure
    draw_rectangle(left, top, w, h, argb_to_color(BOX_BG));
    draw_rectangle_lines(left, top, w, h, 2.0, argb_to_color(BOX_BORDER));

    // titre centré (ex drawTextLeftTop au milieu de la largeur)
    let msg = "*** SETTINGS ***";
    let text_w = measure_text(msg, None, 16, 1.0).width;
    draw_text(msg, left + (w - text_w) / 2.0, top + 2.0 * BOX_PADDING + 12.0, 16.0, argb_to_color(BOX_FG));

    let layout = settings_box_layout();
    let m = mouse_to_game();

    // panneau des modes : fond légèrement plus clair que la fenêtre, bordure
    // discrète et libellé « MOVING MODE » en tête (explicite la finalité des
    // trois radio-boutons qu'il regroupe)
    let panel = layout.modes_panel;
    draw_rectangle(panel.x, panel.y, panel.w, panel.h, argb_to_color(BOX_PANEL_BG));
    draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 1.0, argb_to_color(BOX_PANEL_BORDER));
    draw_text("MOVING MODE", panel.x + 10.0, panel.y + 14.0, 12.0, argb_to_color(BOX_FG));

    // les trois modes (ordre `MOVING_MODE_*`) en radio-boutons : anneau +
    // point central quand sélectionné, libellé + description à droite
    // (hover blanc, description en plus petit et plus sombre)
    let labels = ["INERTIAL", "4 WAYS", "DIRECTIONAL"];
    let descriptions = [
        "THRUST / REVERSE, TURN L/R",
        "ARROWS PUSH IN CURRENT DIR",
        "ACCELERATE / BRAKE, TURN L/R",
    ];
    for (i, rect) in layout.modes.iter().enumerate() {
        let selected = state.moving_mode == i as i32;
        let color = argb_to_color(if rect.contains(m) { BOX_HOVER } else { BOX_FG });
        // focus clavier (flèches ↑/↓ + Entrée) : ligne surlignée
        if state.settings_focus == i as i32 {
            draw_rectangle(rect.x, rect.y, rect.w, rect.h, argb_to_color(0x301AB2FF));
        }
        // anneau radio aligné sur le libellé (bord clair, intérieur =
        // couleur de la fenêtre) + point central quand le mode est actif
        let cx = rect.x + 13.0;
        let cy = rect.y + 12.0;
        draw_circle(cx, cy, 7.0, color);
        draw_circle(cx, cy, 4.5, argb_to_color(BOX_PANEL_BG));
        if selected {
            draw_circle(cx, cy, 2.5, color);
        }
        // un mode verrouillé (scénario à économie) affiche son prix en
        // minerais à côté du libellé
        let label = match scenario::locked_cost(state, i as i32) {
            Some(cost) => format!("{} ({} MIN)", labels[i], cost),
            None => labels[i].to_string(),
        };
        draw_text(&label, rect.x + 30.0, rect.y + 18.0, 16.0, color);
        draw_text(descriptions[i], rect.x + 30.0, rect.y + 34.0, 12.0, argb_to_color(BOX_FG_DIM));
    }

    // cases à cocher MUSIC (état depuis les sons) et AUTO GENERATE
    draw_checkbox(layout.music, sounds.music_on, "MUSIC", m);
    draw_checkbox(layout.auto_generate, state.auto_generate, "AUTO GENERATE", m);

    // volume : barre horizontale (ascenseur) — piste, remplissage selon le
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
    let value_w = measure_text(&value, None, 12, 1.0).width;
    draw_text(
        &value,
        track.x + (track.w - value_w) / 2.0,
        track.y + track.h + 12.0,
        12.0,
        argb_to_color(BOX_FG_DIM),
    );

    // panneau GRAPHICS : fond + bordure + libellé en tête, puis les lignes
    // RENDER / WINDOW / SIZE (valeurs cyclables dans un cadre) et la case
    // ANTIALIAS ; note si l'anticrénelage n'est effectif qu'au lancement
    let g = layout.graphics_panel;
    draw_rectangle(g.x, g.y, g.w, g.h, argb_to_color(BOX_PANEL_BG));
    draw_rectangle_lines(g.x, g.y, g.w, g.h, 1.0, argb_to_color(BOX_PANEL_BORDER));
    draw_text("GRAPHICS", g.x + 10.0, g.y + 14.0, 12.0, argb_to_color(BOX_FG));
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
            12.0,
            argb_to_color(BOX_FG_DIM),
        );
        draw_box_button("RESTART", layout.restart);
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
/// écran zoomé — sinon elle est dessinée 1:1. `screen_width/height` renvoie
/// des pixels logiques (dpi divisé), la comparaison est donc directe.
pub fn window_scaled() -> bool {
    screen_width() != VIEWPORT_WIDTH as f32 || screen_height() != VIEWPORT_HEIGHT as f32
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

/// Caméra de rendu direct du plein écran **natif** (touche F, 3e mode) : la
/// vue 960×540 est affichée à la définition réelle de l'écran SANS passer par
/// un render target — un seul passage de rendu à la résolution native (plus
/// net, moins de fill que le double passage rendu + étirement).
///
/// La rect monde visible = écran/scale (`scale = min(W/960, H/540)`, uniforme
/// avec letterbox) : sur un écran 16:9 c'est exactement 960×540 ; sinon la
/// vue reste au centre avec des bandes noires, comme en mode `Zoomed`.
///
/// NB — signe de `zoom.y` : le rendu direct à l'écran (render target = None)
/// inverse l'axe y (`invert_y = -1` dans `camera.rs`), contrairement au rendu
/// dans un render target (`invert_y = +1`, ex `virtual_camera`). Pour un écran
/// (y=0 en haut), `zoom.y` doit donc être POSITIF — un `-` (copié de
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

/// Fait cycler le mode d'affichage (touche F) : fenêtré → plein écran zoomé
/// (EWMH, render target étirée) → plein écran natif (EWMH, définition réelle
/// de l'écran, sans buffer) → fenêtré.
///
/// Entrée dans les pleins écrans : `set_fullscreen(true)` de miniquad
/// (ClientMessage `_NET_WM_STATE` ADD, standard EWMH). Retour fenêtré :
/// miniquad 0.4.11 ne sait PAS sortir du plein écran sur X11 (`set_fullscreen
/// (false)` envoie un ADD avec un atome vide, sans effet — TODO de miniquad,
/// toujours présent en master) → on envoie nous-mêmes le ClientMessage EWMH
/// REMOVE via libX11 (`crate::x11::set_fullscreen(false)`), sans outil
/// externe (`wmctrl`). Sans WM EWMH, repli sur un simple redimensionnement
/// (avec un WM non EWMH, la fenêtre resterait plein écran).
pub fn cycle_view_mode(state: &mut GameState) {
    state.view_mode = match state.view_mode {
        ViewMode::Windowed => {
            set_fullscreen(true);
            ViewMode::Zoomed
        }
        // le plein écran EWMH est déjà actif : seul le chemin de rendu change
        // (render target étirée → rendu direct natif)
        ViewMode::Zoomed => ViewMode::Native,
        ViewMode::Native => {
            // NB : on n'appelle PAS `set_fullscreen(false)` de miniquad : en
            // plus d'envoyer un ADD avec un atome vide (sans effet), il fait
            // un `XUnmapWindow/XMapWindow` de la fenêtre qui interfère avec
            // notre ClientMessage REMOVE (le WM peut re-appliquer le plein
            // écran au remap). On envoie directement le REMOVE EWMH.
            if !crate::x11::set_fullscreen(false) {
                request_new_screen_size(VIEWPORT_WIDTH as f32, VIEWPORT_HEIGHT as f32);
            }
            ViewMode::Windowed
        }
    };
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

    // invulnérabilité post-respawn (scénario Survival) : le vaisseau
    // clignote (~5 alternances/s) pendant la durée restante
    if shape.who_i_am == WHOIAM_PLAYER && state.invulnerable > 0.0
        && (state.invulnerable * 10.0) as i32 % 2 == 0
    {
        return; // frame « éteinte » : le vaisseau n'est pas dessiné
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
            // style de rendu (écran de paramétrage) : texturé (défaut),
            // colorisé (remplissage uni) ou mesh (arêtes seules)
            match state.render_style {
                RenderStyle::Textured => {
                    if shape.texture != TEXTURE_NONE {
                        draw_textured_triangle(assets, t, shape, camera, elements, &state.world);
                    } else {
                        draw_triangle(assets, t, shape, camera, elements, &state.world);
                    }
                }
                RenderStyle::Colored => {
                    draw_colored_triangle(t, shape, camera, elements, &state.world)
                }
                RenderStyle::Mesh => draw_mesh_triangle(t, shape, camera, elements, &state.world),
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
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);
    if t.life <= 0 {
        draw_dead_triangle(a, b, c, t, shape);
        return;
    }
    // NB : chemin complet — `draw_triangle` (macroquad) est masqué par la
    // fonction locale du même nom (triangles non texturés de l'original)
    macroquad::shapes::draw_triangle(a, b, c, triangle_color(t, shape, elements));
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
) {
    let a = screen_point(t.real_a, camera, world);
    let b = screen_point(t.real_b, camera, world);
    let c = screen_point(t.real_c, camera, world);
    if t.life <= 0 {
        draw_dead_triangle(a, b, c, t, shape);
        return;
    }
    draw_triangle_lines(a, b, c, 1.0, triangle_color(t, shape, elements));
    draw_element_dot(t, camera, elements, world);
}

/// Couleur d'affichage d'un triangle vivant : celle de son élément minéral
/// (ex `elements[t.element].color`) sinon la couleur de la forme.
fn triangle_color(t: &Triangle, shape: &Shape, elements: &[Element]) -> Color {
    if t.element > 0 {
        argb_to_color(elements[t.element as usize].color)
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

// ─── Accostage : mire au centre de la station + HUD d'approche ──────────────

/// Transparence (octet alpha ARGB) de la mire d'accostage : discrète (canal
/// alpha volontairement bas, comme les liens d'accostage) — l'anneau et la
/// croix sont plus légers que le point central (l'effet néon empile des
/// halos dérivés de ces alphas, voir `neon_ring`/`neon_line`/`neon_dot`).
const DOCK_MARKER_ALPHA: u32 = 0x66;
const DOCK_MARKER_DOT_ALPHA: u32 = 0x99;

/// Qualité de l'approche pour la mire (0 = rouge, 1 = vert) : interpolée sur
/// **tout le rayon de la base** (0 au bord du rayon, 1 au centre) et sur la
/// vitesse (0 à `DOCK_APPROACH_FULL_RED_SPEED` ou plus, 1 à l'arrêt) — la
/// mire réagit dès que le vaisseau entre dans le rayon de la station.
fn docking_approach_quality(dist: f64, speed: f64, station_radius: f64) -> f64 {
    let dist_q = 1.0 - (dist / station_radius).clamp(0.0, 1.0);
    let speed_q = 1.0 - (speed.abs() / DOCK_APPROACH_FULL_RED_SPEED).clamp(0.0, 1.0);
    dist_q * speed_q
}

/// Effet néon d'un anneau : halo (3 cercles concentriques d'alpha décroissant
/// — macroquad n'a pas de flou, on empile) + anneau principal + cœur clair.
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
/// de rayon `STATION_DOCK_DISTANCE`) est affichée — cercle + croix de visée +
/// point central, semi-transparents, légèrement pulsants et avec un **effet
/// néon** (halo + cœur clair) — pour montrer où poser le vaisseau. La couleur
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
/// laissent voir le vaisseau — l'effet néon empile des halos dérivés de cet
/// alpha (voir `neon_line`).
const DOCK_LINE_ALPHA: u32 = 0x66;

/// Distance (unités monde, du centre du vaisseau) des points de branchement
/// des liens d'accostage : les 4 liens se connectent en diagonale (NO, SO,
/// SE, NE) sur un petit losange **proche du centre** — l'illusion qu'ils
/// touchent le vaisseau (ils sont dessinés dessous).
const DOCK_LINE_SHIP_ANCHOR: f64 = 5.0;

/// La mire d'accostage est-elle visible ? Elle n'est affichée **que lors du
/// retour à la base** (`state.docking_guide`, posé par
/// `game::update_docking_guide` quand le vaisseau recroise la limite
/// extérieure en entrant) : jamais pendant que le vaisseau quitte
/// l'accostage, à quai, pendant l'animation d'accostage (il est tiré vers le
/// centre), tant que la boîte DOCK STATION / l'atelier est ouvert (accosté)
/// et pendant la rétraction des liens au départ — dans tous ces cas, le
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
        || state.workshop_box
        || state.dock_retract > 0.0
        || state.dock_links
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
/// vers l'extrémité mobile (`× t` — l'extrémité libre fouette).
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
/// à quai (`state.dock_links`, lancement/respawn — vaisseau au centre, liens
/// tendus), **pendant l'animation d'accostage** (avant l'ouverture de la boîte
/// DOCK STATION, `state.dock_anim > 0`) et **pendant la rétraction au
/// départ** (`state.dock_retract > 0`, après CLOSE ou au démarrage) : **4
/// liens néon verts simultanés** qui relient le bord intérieur de la station
/// (rayon `STATION_INNER_RADIUS`) au vaisseau — un par **diagonale** (NO,
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
) {
    // à quai (lancement/respawn) : liens tendus jusqu'au vaisseau au centre
    let docked = state.dock_links;
    let retracting = state.dock_retract > 0.0;
    let docking = state.dock_anim > 0.0;
    if !docked && !retracting && !docking {
        return;
    }
    // avancement de la rétraction 0..1 (lissé) : 0 = liens tendus jusqu'au
    // vaisseau, 1 = rétractés sur le bord intérieur de l'anneau (disparus) —
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
    // bord intérieur de la station aux 4 points DIAGONAUX (NO, SO, SE, NE —
    // angle ±π/4, plus crédible que les 4 points cardinaux), avant rotation
    let diag = std::f64::consts::FRAC_1_SQRT_2; // cos/sin de ±π/4 ≈ 0,7071
    let inner = [
        Point::new(-STATION_INNER_RADIUS * diag, -STATION_INNER_RADIUS * diag), // NO
        Point::new(-STATION_INNER_RADIUS * diag, STATION_INNER_RADIUS * diag),  // SO
        Point::new(STATION_INNER_RADIUS * diag, STATION_INNER_RADIUS * diag),   // SE
        Point::new(STATION_INNER_RADIUS * diag, -STATION_INNER_RADIUS * diag),  // NE
    ];
    // côté correspondant du vaisseau : mêmes diagonales mais **près du
    // centre** (petit losange à ~`DOCK_LINE_SHIP_ANCHOR` du centre —
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
    let color = argb_to_color((DOCK_LINE_ALPHA << 24) | 0x0040FF40);
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

/// HUD d'accostage (3e ligne) : distance du vaisseau au centre de la station
/// (unités monde) et invite — « DOCK: 123 px » en approche, « DOCK: SLOW DOWN »
/// (rouge) dans la zone mais trop rapide pour accoster, « DOCK: IN RANGE »
/// (vert) dans la zone et presque immobile, « DOCKED » à quai (liens attachés
/// au lancement/respawn ou boîte ouverte). La zone elle-même est visible via
/// la mire (`draw_docking_marker`).
pub fn draw_docking_hud(
    state: &GameState,
    player_position: Point,
    station_position: Point,
    player_speed: f64,
) {
    let dist = (player_position.x - station_position.x).hypot(player_position.y - station_position.y);
    let in_zone = dist < STATION_DOCK_DISTANCE;
    let (text, color) = if state.dock_box || state.workshop_box || state.dock_links {
        ("DOCKED".to_string(), 0xFF40FF40)
    } else if in_zone && player_speed.abs() < STATION_DOCK_SPEED {
        ("DOCK: IN RANGE".to_string(), 0xFF40FF40)
    } else if in_zone {
        ("DOCK: SLOW DOWN".to_string(), 0xFFFF3C00)
    } else {
        (format!("DOCK: {:.0} px", dist), 0xFFFFFFFF)
    };
    draw_text(&text, 8.0, 46.0, 16.0, argb_to_color(color));
}

/// HUD : FPS, réputation (+ rang en scénario à économie), précision (ex
/// `locate 1,1 / 1,15 / 1,30` de mainLoop) + ligne des ressources en scénario
/// à économie (carburant, munitions, minerais). Police macroquad par défaut
/// en attendant la police 8×16 (Phase 4).
pub fn draw_hud(state: &GameState) {
    draw_text(&format!("FPS:{}", state.fps), 8.0, 14.0, 16.0, WHITE);
    // réputation : compteur d'astéroïdes détruits (jeu libre) ou réputation
    // du scénario (économie — croît avec les destructions et la précision) ;
    // en économie, le rang courant (palier débloqué par la réputation, ex
    // CADET → PILOT → ACE) est affiché à côté
    let economy = scenario::has_economy(state);
    let reputation = if economy {
        state.resources.reputation as i32
    } else {
        state.meteors_destroyed
    };
    let rep_text = match scenario::current_rank(state) {
        Some(rank) => format!("REPUTATION:{} ({})", reputation, rank),
        None => format!("REPUTATION:{}", reputation),
    };
    draw_text(&rep_text, 8.0 + 14.0 * 8.0, 14.0, 16.0, WHITE);
    if state.bullets_fired > 0 {
        let precision = 100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64);
        // en économie, le rang allonge la ligne : PRECISION est décalée à
        // droite (col 40 au lieu de 30) pour rester lisible
        let precision_x = if economy { 8.0 + 40.0 * 8.0 } else { 8.0 + 29.0 * 8.0 };
        draw_text(
            &format!("PRECISION:{}%", precision as i32),
            precision_x,
            14.0,
            16.0,
            WHITE,
        );
    }
    // ressources du scénario sur la 2e ligne : carburant/munitions/minerais
    // (économie — les capacités montrent les extensions d'atelier achetées)
    // ou vies + bouclier (Survival)
    if scenario::has_economy(state) {
        draw_text(
            &format!(
                "FUEL:{:.0}/{} AMMO:{}/{} MINERALS:{}",
                state.resources.fuel,
                scenario::fuel_capacity(state),
                state.resources.ammo,
                scenario::ammo_capacity(state),
                state.resources.minerals
            ),
            8.0,
            30.0,
            16.0,
            WHITE,
        );
    } else if scenario::has_survival(state) {
        draw_text(
            &format!(
                "LIVES:{} SHIELD:{:.0}",
                state.resources.lives, state.resources.shield
            ),
            8.0,
            30.0,
            16.0,
            WHITE,
        );
    }
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
    /// tuile réapparaît à gauche (ou en haut) — ex `normalizePlanPosition`.
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
    /// l'arrêt, et interpolée entre les deux — plus seulement dans la zone
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
        // accosté : atelier ouvert → cachée
        state.workshop_box = true;
        assert!(!docking_marker_visible(&state, player, station, radius));
        state.workshop_box = false;
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
    /// court dans le temps — vers l'anneau en rétraction, vers le vaisseau en
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
