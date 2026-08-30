//! Rendu du magasin de la station : fenêtre à onglets
//! (RAVITAILLEMENT / ÉQUIPEMENT / ATELIER / MODE DE VOL),
//! curseurs, pilules d'achat et aperçu du vaisseau
//! (issu de `src/render.rs`).

use macroquad::prelude::*;
use crate::config::*;
use crate::render::*;
use crate::font::measure_text;
use crate::geom::{Point, Triangle};
use crate::marketplace::MOVING_MODES;
use crate::scenario;
use crate::shape::Shape;
use crate::state::{Element, GameState};

/// Géométrie du magasin de la station (bouton MARCHÉ de la boîte DOCK
/// STATION) : fenêtre à **onglets** - un seul contenu affiché à la fois
/// (RAVITAILLEMENT par défaut), ce qui garde la fenêtre compacte et
/// compréhensible. En-tête : titre + porte-monnaie (crédits). Onglets :
/// RAVITAILLEMENT, ÉQUIPEMENT, ATELIER, MODE DE VOL, FABRICATION. Pied :
/// retour d'action (achat confirmé / refus) + bouton CLOSE. Chaque ligne a
/// un bouton « pilule » explicite (ACHETER / sélection / déblocage) à droite.
pub struct ShopBoxLayout {
    /// Rectangle de la fenêtre complète (fond, bordure, coordonnées).
    pub window: Rect,
    /// Onglets (RAVITAILLEMENT, ÉQUIPEMENT, ATELIER, MODE DE VOL,
    /// FABRICATION) - clic = bascule de `state.shop_tab` (`shop::shop_update`).
    pub tabs: [Rect; SHOP_TAB_COUNT],
    /// Ligne « carburant » du ravitaillement (étiquette à gauche, curseur au
    /// centre) - rectangle vide hors économie.
    pub supplies_fuel: Rect,
    /// Piste du curseur de carburant (glisser / molette = quantité à acheter).
    pub slider_fuel: Rect,
    /// Bouton « ACHETER » de la ligne carburant (achat de la quantité du
    /// curseur, `scenario::buy_fuel_qty`) - vide hors économie.
    pub buy_fuel: Rect,
    /// Lignes « munitions » du ravitaillement, **une par arme possédée**
    /// (index catalogue) - vides hors économie ou arme non possédée.
    pub supplies_ammo: [Rect; WEAPON_SLOTS],
    /// Pistes des curseurs de munitions par arme (glisser / molette).
    pub slider_ammo: [Rect; WEAPON_SLOTS],
    /// Boutons « ACHETER » des lignes munitions (`scenario::buy_ammo_qty`).
    pub buy_ammo: [Rect; WEAPON_SLOTS],
    /// Bouton « TOUT REMPLIR » (plein de carburant + munitions au maximum
    /// achetable) - vide hors économie.
    pub refill_all: Rect,
    /// Panneau d'**aperçu du vaisseau équipé** (onglet ÉQUIPEMENT) : le
    /// vaisseau réel y est redessiné à l'échelle, avec l'arme survolée en
    /// superposition - vide hors onglet ÉQUIPEMENT.
    pub preview: Rect,
    /// Lignes des armes du catalogue (onglet ÉQUIPEMENT, index dans
    /// `VAISSEAU_WEAPONS`).
    pub weapons: [Rect; WEAPON_SLOTS],
    /// Boutons d'achat des armes (achat contre crédits).
    pub buy_weapon: [Rect; WEAPON_SLOTS],
    /// Ligne du **radar de bord** (onglet ÉQUIPEMENT, sous les armes) :
    /// affiche la minimap globale - acheté contre crédits en scénario à
    /// économie, déjà actif (POSSÉDÉ) hors économie.
    pub radar: Rect,
    /// Bouton d'achat du radar.
    pub buy_radar: Rect,
    /// Lignes d'extension de l'atelier (réservoir, chargeur, soute).
    pub fuel: Rect,
    pub ammo: Rect,
    pub cargo: Rect,
    /// Boutons d'achat des extensions.
    pub buy_fuel_upgrade: Rect,
    pub buy_ammo_upgrade: Rect,
    pub buy_cargo_upgrade: Rect,
    /// Lignes des modes de déplacement (onglet MODE DE VOL, ordre visuel
    /// `MOVING_MODE_ORDER`).
    pub modes: [Rect; 4],
    /// Boutons de sélection / déblocage des modes.
    pub buy_mode: [Rect; 4],
    /// Lignes des consommables (onglet FABRICATION) : nom + recette
    /// (ingrédients) à gauche, bouton FABRIQUER à droite.
    pub craft: [Rect; CRAFT_COUNT],
    /// Boutons de fabrication des consommables.
    pub buy_craft: [Rect; CRAFT_COUNT],
    /// Bouton CLOSE : revient à la boîte DOCK STATION.
    pub close: Rect,
}

/// Hauteur de la fenêtre du magasin selon l'onglet actif : en-tête (titre +
/// porte-monnaie + onglets) + contenu de l'onglet + pied (retour d'action +
/// CLOSE). L'onglet RAVITAILLEMENT grandit avec le nombre d'armes possédées
/// (une ligne AMMO par arme + le bouton TOUT REMPLIR).
pub fn shop_box_height(state: &GameState, ammo_rows: usize) -> f32 {
    let content = match state.shop_tab {
        crate::config::SHOP_TAB_WEAPONS => {
            // panneau d'aperçu du vaisseau + lignes d'armes + ligne RADAR
            let n = scenario::weapon_slot_count().min(WEAPON_SLOTS);
            6.0 + SHOP_PREVIEW_H + 10.0 + (n + 1) as f32 * (SHOP_ROW_H + 2.0)
        }
        crate::config::SHOP_TAB_WORKSHOP => 8.0 + 3.0 * (SHOP_ROW_H + 2.0),
        crate::config::SHOP_TAB_MODES => 8.0 + 4.0 * (SHOP_ROW_H + 2.0),
        crate::config::SHOP_TAB_CRAFT => 8.0 + 3.0 * (SHOP_ROW_H + 2.0),
        _ => {
            // RAVITAILLEMENT : une ligne carburant + une ligne munitions par
            // arme possédée, puis le bouton TOUT REMPLIR
            let n = if scenario::has_economy(state) { ammo_rows } else { 0 };
            8.0 + (1 + n) as f32 * (SHOP_ROW_H + 2.0) + 36.0
        }
    };
    72.0 + content + 56.0
}

pub fn shop_box_layout(state: &GameState) -> ShopBoxLayout {
    let ammo_rows = (0..scenario::weapon_slot_count())
        .filter(|&i| scenario::weapon_owned(state, i))
        .count();
    let w = SHOP_W;
    let h = shop_box_height(state, ammo_rows);
    let left = ((VIEWPORT_WIDTH as f32 - w) / 2.0).round();
    let top = ((VIEWPORT_HEIGHT as f32 - h) / 2.0).round();
    let pad = BOX_PADDING;
    let row_w = w - 2.0 * pad;
    let right = left + w - pad;

    // onglets : SHOP_TAB_COUNT onglets égaux sous l'en-tête (titre +
    // porte-monnaie)
    let tab_gap = 6.0;
    let tab_w = (w - 2.0 * pad - (SHOP_TAB_COUNT - 1) as f32 * tab_gap) / SHOP_TAB_COUNT as f32;
    let tabs_top = top + 40.0;
    let mut tabs = [Rect::new(0.0, 0.0, 0.0, 0.0); SHOP_TAB_COUNT];
    for (i, tab) in tabs.iter_mut().enumerate() {
        *tab = Rect::new(
            left + pad + i as f32 * (tab_w + tab_gap),
            tabs_top,
            tab_w,
            SHOP_TAB_H,
        );
    }

    let content_top = top + 72.0;
    let mut y = content_top;
    // pilule d'action à droite d'une ligne (au niveau de `row_y`)
    let pill = |row_y: f32| Rect::new(right - SHOP_PILL_W, row_y + 7.0, SHOP_PILL_W, SHOP_PILL_H);

    let mut supplies_fuel = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut slider_fuel = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut buy_fuel = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut supplies_ammo = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    let mut slider_ammo = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    let mut buy_ammo = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    let mut refill_all = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut preview = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut weapons = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    let mut buy_weapon = [Rect::new(0.0, 0.0, 0.0, 0.0); WEAPON_SLOTS];
    let mut radar = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut buy_radar = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut fuel = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut ammo = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut cargo = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut buy_fuel_upgrade = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut buy_ammo_upgrade = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut buy_cargo_upgrade = Rect::new(0.0, 0.0, 0.0, 0.0);
    let mut modes = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    let mut buy_mode = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    let mut craft = [Rect::new(0.0, 0.0, 0.0, 0.0); CRAFT_COUNT];
    let mut buy_craft = [Rect::new(0.0, 0.0, 0.0, 0.0); CRAFT_COUNT];

    match state.shop_tab {
        crate::config::SHOP_TAB_WEAPONS => {
            // panneau d'aperçu du vaisseau (lignes d'armes en dessous)
            y += 6.0;
            preview = Rect::new(left + pad, y, row_w, SHOP_PREVIEW_H);
            y += SHOP_PREVIEW_H + 10.0;
            let n = scenario::weapon_slot_count().min(WEAPON_SLOTS);
            for i in 0..n {
                weapons[i] = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
                buy_weapon[i] = pill(y);
                y += SHOP_ROW_H + 2.0;
            }
            // radar de bord (équipement - minimap globale) sous les armes
            radar = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
            buy_radar = pill(y);
        }
        crate::config::SHOP_TAB_WORKSHOP => {
            y += 8.0;
            fuel = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
            buy_fuel_upgrade = pill(y);
            y += SHOP_ROW_H + 2.0;
            ammo = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
            buy_ammo_upgrade = pill(y);
            y += SHOP_ROW_H + 2.0;
            cargo = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
            buy_cargo_upgrade = pill(y);
        }
        crate::config::SHOP_TAB_MODES => {
            y += 8.0;
            for i in 0..4 {
                modes[i] = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
                buy_mode[i] = pill(y);
                y += SHOP_ROW_H + 2.0;
            }
        }
        crate::config::SHOP_TAB_CRAFT => {
            // FABRICATION : une ligne par consommable (bouclier, boost, mine)
            y += 8.0;
            for i in 0..CRAFT_COUNT {
                craft[i] = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
                buy_craft[i] = pill(y);
                y += SHOP_ROW_H + 2.0;
            }
        }
        _ => {
            // RAVITAILLEMENT (défaut) : une ligne carburant puis une ligne
            // munitions par arme possédée, puis le bouton TOUT REMPLIR
            if scenario::has_economy(state) {
                y += 2.0;
                supplies_fuel = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
                slider_fuel = Rect::new(left + 190.0, y + 12.0, 200.0, 14.0);
                buy_fuel = pill(y);
                y += SHOP_ROW_H + 2.0;
                for i in 0..scenario::weapon_slot_count() {
                    if scenario::weapon_owned(state, i) {
                        supplies_ammo[i] = Rect::new(left + pad, y, row_w, SHOP_ROW_H);
                        slider_ammo[i] = Rect::new(left + 190.0, y + 12.0, 200.0, 14.0);
                        buy_ammo[i] = pill(y);
                        y += SHOP_ROW_H + 2.0;
                    }
                }
                refill_all = Rect::new(left + pad, y + 2.0, row_w, 28.0);
            }
        }
    }

    ShopBoxLayout {
        window: Rect::new(left, top, w, h),
        tabs,
        supplies_fuel,
        slider_fuel,
        buy_fuel,
        supplies_ammo,
        slider_ammo,
        buy_ammo,
        refill_all,
        preview,
        weapons,
        buy_weapon,
        radar,
        buy_radar,
        fuel,
        ammo,
        cargo,
        buy_fuel_upgrade,
        buy_ammo_upgrade,
        buy_cargo_upgrade,
        modes,
        buy_mode,
        craft,
        buy_craft,
        close: Rect::new(right - 96.0, top + h - 20.0 - 28.0, 96.0, 28.0),
    }
}

/// Dessine le magasin de la station (bouton MARCHÉ de la boîte DOCK STATION) :
/// en-tête avec titre + porte-monnaie (crédits toujours visibles), rangée
/// d'onglets (RAVITAILLEMENT / ÉQUIPEMENT / ATELIER / MODE DE VOL /
/// FABRICATION - un seul contenu à la fois, fenêtre compacte), le contenu de
/// l'onglet actif (lignes à boutons « ACHETER » explicites, prix colorés :
/// vert si abordable, rouge sinon), un **tooltip** au survol des lignes et un
/// pied avec le retour d'action (achat confirmé / refus) + le bouton CLOSE.
/// `elements` porte le contenu de la soute (ingrédients de l'onglet
/// FABRICATION).
pub fn draw_shop_box(state: &GameState, shapes: &[Shape], triangles: &[Triangle], elements: &[Element]) {
    let l = shop_box_layout(state);
    let win = l.window;
    let m = mouse_to_game();

    // fenêtre : fond + bordure
    draw_rectangle(win.x, win.y, win.w, win.h, argb_to_color(BOX_BG));
    draw_rectangle_lines(win.x, win.y, win.w, win.h, 2.0, argb_to_color(BOX_BORDER));

    // en-tête : titre à gauche, porte-monnaie à droite (le joueur voit son
    // budget en permanence pendant ses achats)
    draw_text_shadow(
        "PLACE DE MARCHÉ",
        win.x + BOX_PADDING + 4.0,
        win.y + 2.0 * BOX_PADDING + 12.0,
        16.0,
        argb_to_color(BOX_FG),
    );
    if scenario::has_economy(state) {
        let wallet = format!("CRÉDITS : {}", state.resources.credits);
        let ww = measure_text(&wallet, None, 16, 1.0).width;
        draw_text_shadow(
            &wallet,
            win.x + win.w - BOX_PADDING - 4.0 - ww,
            win.y + 2.0 * BOX_PADDING + 12.0,
            16.0,
            argb_to_color(SHOP_OK),
        );
    }

    // onglets
    draw_shop_tabs(state, &l);

    // contenu de l'onglet actif
    match state.shop_tab {
        crate::config::SHOP_TAB_WEAPONS => draw_shop_weapons_tab(state, &l, m, shapes, triangles),
        crate::config::SHOP_TAB_WORKSHOP => draw_shop_workshop_tab(state, &l, m),
        crate::config::SHOP_TAB_MODES => draw_shop_modes_tab(state, &l, m),
        crate::config::SHOP_TAB_CRAFT => draw_shop_craft_tab(state, &l, m, elements),
        _ => draw_shop_supplies_tab(state, &l, m),
    }

    // tooltip au survol d'une ligne (prix, effets, description)
    draw_shop_tooltip(state, &l, m, elements);

    // pied : retour d'action (vert = succès, rouge = refus) + CLOSE
    if !state.shop_feedback.is_empty() {
        let color = argb_to_color(if state.shop_feedback_ok { SHOP_OK } else { SHOP_ERR });
        draw_text_shadow(
            &state.shop_feedback,
            win.x + BOX_PADDING + 4.0,
            win.y + win.h - 58.0,
            15.0,
            color,
        );
    }
    draw_box_button("FERMER", l.close);

    // version + numéro de build (petit, coin bas-gauche de la fenêtre - voir
    // `build_info::display`, même discrétion que l'écran titre)
    let version_text = crate::build_info::display();
    draw_text_shadow(
        &version_text,
        win.x + BOX_PADDING + 4.0,
        win.y + win.h - 8.0,
        8.0,
        argb_to_color(0x6B6B7E),
    );
}

/// Dessine la rangée d'onglets du magasin : onglet actif rempli + vert,
/// survol blanc, autres en bleu clair (clic = bascule, `shop::shop_update`).
pub fn draw_shop_tabs(state: &GameState, l: &ShopBoxLayout) {
    let labels = ["RAVITAILLEMENT", "ÉQUIPEMENT", "ATELIER", "MODE DE VOL", "FABRICATION"];
    let m = mouse_to_game();
    for (i, rect) in l.tabs.iter().enumerate() {
        let active = state.shop_tab as usize == i;
        let hovered = rect.contains(m);
        let color = argb_to_color(if active {
            SHOP_OK
        } else if hovered {
            BOX_HOVER
        } else {
            BOX_FG
        });
        if active {
            draw_rectangle(rect.x, rect.y, rect.w, rect.h, argb_to_color(0x401AB2FF));
        }
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, color);
        let tw = measure_text(labels[i], None, 14, 1.0).width;
        draw_text_shadow(labels[i], rect.x + (rect.w - tw) / 2.0, rect.y + 18.0, 14.0, color);
    }
}

/// Dessine un bouton « pilule » d'action du magasin : fond surbrillé au
/// survol, bordure colorée selon `tone` (`None` = neutre, `Some(true)` =
/// vert abordable, `Some(false)` = rouge insuffisant). Renvoie `true` si
/// survolé (pour surligner la ligne).
pub fn draw_shop_pill(rect: Rect, label: &str, tone: Option<bool>, m: Vec2) -> bool {
    let hovered = rect.contains(m);
    let color = argb_to_color(match tone {
        Some(true) => SHOP_OK,
        Some(false) => SHOP_ERR,
        None => BOX_FG,
    });
    if hovered {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, argb_to_color(0x401AB2FF));
    }
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, color);
    let tw = measure_text(label, None, 13, 1.0).width;
    draw_text_shadow(
        label,
        rect.x + (rect.w - tw) / 2.0,
        rect.y + 17.0,
        13.0,
        if hovered { argb_to_color(BOX_HOVER) } else { color },
    );
    hovered
}

/// Onglet RAVITAILLEMENT : une ligne par ressource (carburant, puis
/// munitions par arme possédée) - étiquette + état à gauche, curseur de
/// quantité au centre (glisser / molette), montant et coût au-dessus de la
/// piste, bouton ACHETER à droite - puis le bouton TOUT REMPLIR.
pub fn draw_shop_supplies_tab(state: &GameState, l: &ShopBoxLayout, m: Vec2) {
    if !scenario::has_economy(state) {
        return;
    }
    // carburant
    let fuel_cap = scenario::fuel_capacity(state);
    draw_supply_row(
        state,
        l.supplies_fuel,
        l.slider_fuel,
        l.buy_fuel,
        "CARBURANT",
        state.resources.fuel,
        fuel_cap,
        state.shop_fuel_qty,
        scenario::fuel_qty_cost(state, state.shop_fuel_qty) as f64,
        m,
    );
    // munitions par arme possédée
    let ammo_cap = scenario::ammo_capacity(state);
    for i in 0..scenario::weapon_slot_count() {
        let rect = l.supplies_ammo[i];
        if rect.w <= 0.0 {
            continue;
        }
        let spec = scenario::weapon_spec(i);
        draw_supply_row(
            state,
            rect,
            l.slider_ammo[i],
            l.buy_ammo[i],
            spec.name,
            state.resources.weapon_ammo[i] as f64,
            ammo_cap as f64,
            state.shop_ammo_qty[i],
            scenario::ammo_qty_cost(state, i, state.shop_ammo_qty[i] as i32) as f64,
            m,
        );
    }
    // TOUT REMPLIR : plein de carburant + munitions au maximum achetable
    let hovered = l.refill_all.contains(m);
    let color = argb_to_color(if hovered { BOX_HOVER } else { SHOP_OK });
    if hovered {
        draw_rectangle(l.refill_all.x, l.refill_all.y, l.refill_all.w, l.refill_all.h, argb_to_color(0x401AB2FF));
    }
    draw_rectangle_lines(l.refill_all.x, l.refill_all.y, l.refill_all.w, l.refill_all.h, 1.5, color);
    let label = "TOUT REMPLIR (MAX ACHETABLE)";
    let tw = measure_text(label, None, 15, 1.0).width;
    draw_text_shadow(
        label,
        l.refill_all.x + (l.refill_all.w - tw) / 2.0,
        l.refill_all.y + 19.0,
        15.0,
        color,
    );
}

/// Dessine une ligne du ravitaillement (carburant ou munitions d'une arme) :
/// étiquette + état à gauche, curseur de quantité au centre, montant et coût
/// au-dessus de la piste (vert si abordable, rouge sinon), bouton ACHETER à
/// droite (PLEIN quand rien ne manque). `qty`/`cost` portent la quantité du
/// curseur et son coût.
#[allow(clippy::too_many_arguments)]
pub fn draw_supply_row(
    state: &GameState,
    row: Rect,
    slider: Rect,
    pill: Rect,
    name: &str,
    current: f64,
    capacity: f64,
    qty: f64,
    cost: f64,
    m: Vec2,
) {
    let missing = (capacity - current).max(0.0);
    let label = if missing <= 0.0 {
        format!("{} : {:.0}/{:.0} - PLEIN", name, current, capacity)
    } else {
        format!("{} : {:.0}/{:.0}", name, current, capacity)
    };
    draw_text_shadow(&label, row.x + 4.0, row.y + 13.0, 14.0, argb_to_color(BOX_FG));
    // montant + coût au-dessus de la piste du curseur
    if missing > 0.0 && qty > 0.0 {
        let affordable = cost <= state.resources.credits as f64;
        let val = format!("+{:.0}  {} CR", qty, cost as i32);
        let vw = measure_text(&val, None, 14, 1.0).width;
        draw_text_shadow(
            &val,
            slider.x + slider.w - vw,
            row.y + 13.0,
            14.0,
            argb_to_color(if affordable { SHOP_OK } else { SHOP_ERR }),
        );
    }
    draw_supply_slider(slider, missing, qty, m);
    let affordable = qty > 0.0 && cost <= state.resources.credits as f64;
    draw_shop_pill(
        pill,
        if missing <= 0.0 { "PLEIN" } else { "ACHETER" },
        if missing > 0.0 { Some(affordable) } else { None },
        m,
    );
}

/// Onglet ÉQUIPEMENT : en haut, l'**aperçu du vaisseau équipé** (le vaisseau
/// réel avec les armes possédées, l'arme survolée superposée sur son
/// emplacement) ; en dessous, une ligne par arme du catalogue - nom + paquet
/// de munitions, prix à gauche de la pilule (vert si abordable, rouge sinon)
/// et bouton ACHETER / POSSÉDÉE à droite - puis une ligne **RADAR** (minimap
/// globale, achetée contre crédits en économie, POSSÉDÉ hors économie).
pub fn draw_shop_weapons_tab(state: &GameState, l: &ShopBoxLayout, m: Vec2, shapes: &[Shape], triangles: &[Triangle]) {
    let n = scenario::weapon_slot_count().min(WEAPON_SLOTS);
    // arme survolée (non possédée) : son aperçu se superpose au vaisseau
    let hovered = (0..n).find(|&i| {
        l.weapons[i].w > 0.0
            && l.weapons[i].contains(m)
            && !scenario::weapon_owned(state, i)
    });
    draw_ship_preview(shapes, triangles, l.preview, hovered);
    for i in 0..n {
        let rect = l.weapons[i];
        if rect.w <= 0.0 {
            continue;
        }
        let spec = scenario::weapon_spec(i);
        let owned = scenario::weapon_owned(state, i);
        draw_text_shadow(spec.name, rect.x + 4.0, rect.y + 14.0, 15.0, argb_to_color(BOX_FG));
        draw_text_shadow(
            &format!("MUNITIONS : {} CR / {} u", spec.ammo_price, spec.ammo_pack),
            rect.x + 4.0,
            rect.y + 31.0,
            12.0,
            argb_to_color(BOX_FG_DIM),
        );
        if owned {
            draw_shop_pill(l.buy_weapon[i], "POSSÉDÉE", None, m);
        } else {
            // arme non équipée et non achetable (hors économie - ex jeu
            // libre où seule l'arme 1 équipe le vaisseau) : pas de prix,
            // pas d'achat possible
            let Some((_, discounted)) = scenario::weapon_prices(state, i) else {
                draw_shop_pill(l.buy_weapon[i], "NON ÉQUIPÉE", None, m);
                continue;
            };
            let affordable = discounted <= state.resources.credits;
            let price = format!("{} CR", discounted);
            let pw = measure_text(&price, None, 14, 1.0).width;
            draw_text_shadow(
                &price,
                rect.x + rect.w - SHOP_PILL_W - 12.0 - pw,
                rect.y + 14.0,
                14.0,
                argb_to_color(if affordable { SHOP_OK } else { SHOP_ERR }),
            );
            draw_shop_pill(l.buy_weapon[i], "ACHETER", Some(affordable), m);
        }
    }
    // radar de bord : sous les armes, une ligne d'équipement - allumé par
    // défaut hors économie (POSSÉDÉ), acheté contre crédits en économie
    if l.radar.w <= 0.0 {
        return;
    }
    draw_text_shadow("RADAR", l.radar.x + 4.0, l.radar.y + 14.0, 15.0, argb_to_color(BOX_FG));
    draw_text_shadow(
        "MINIMAP GLOBALE : POSITION DES MÉTÉORES",
        l.radar.x + 4.0,
        l.radar.y + 31.0,
        12.0,
        argb_to_color(BOX_FG_DIM),
    );
    if !scenario::has_economy(state) || scenario::has_radar(state) {
        // hors économie, le radar est allumé par défaut : déjà en place
        draw_shop_pill(l.buy_radar, "POSSÉDÉ", None, m);
    } else {
        let (_, discounted) = scenario::radar_price(state).unwrap_or((0, 0));
        let affordable = discounted <= state.resources.credits;
        let price = format!("{} CR", discounted);
        let pw = measure_text(&price, None, 14, 1.0).width;
        draw_text_shadow(
            &price,
            l.radar.x + l.radar.w - SHOP_PILL_W - 12.0 - pw,
            l.radar.y + 14.0,
            14.0,
            argb_to_color(if affordable { SHOP_OK } else { SHOP_ERR }),
        );
        draw_shop_pill(l.buy_radar, "ACHETER", Some(affordable), m);
    }
}

/// Dessine l'**aperçu du vaisseau équipé** dans le panneau de l'onglet
/// ÉQUIPEMENT : le vaisseau réel (armes possédées + extensions, tel qu'à
/// quai) est redessiné à l'échelle dans le panneau, et l'arme `hovered`
/// (non possédée, survolée) s'y superpose sur son emplacement - le joueur
/// voit ce que donnerait l'achat avant de cliquer. Légende à droite du
/// vaisseau : « VAISSEAU ACTUEL » ou « APERÇU AVEC : {arme} ».
pub fn draw_ship_preview(
    shapes: &[Shape],
    triangles: &[Triangle],
    panel: Rect,
    hovered: Option<usize>,
) {
    if panel.w <= 0.0 {
        return; // hors onglet ÉQUIPEMENT
    }
    let ship = &shapes[PLAYER_INDEX];
    // boîte englobante des triangles vivants du vaisseau (à quai, position
    // 0,0 et orientation 0 : repère monde = repère local)
    let mut minx = f64::MAX;
    let mut miny = f64::MAX;
    let mut maxx = f64::MIN;
    let mut maxy = f64::MIN;
    let mut any = false;
    for tri in &triangles[ship.first_triangle..=ship.last_triangle] {
        if tri.life <= 0 {
            continue;
        }
        any = true;
        minx = minx.min(tri.real_a.x).min(tri.real_b.x).min(tri.real_c.x);
        miny = miny.min(tri.real_a.y).min(tri.real_b.y).min(tri.real_c.y);
        maxx = maxx.max(tri.real_a.x).max(tri.real_b.x).max(tri.real_c.x);
        maxy = maxy.max(tri.real_a.y).max(tri.real_b.y).max(tri.real_c.y);
    }
    if !any {
        return;
    }
    let bw = (maxx - minx).max(1.0);
    let bh = (maxy - miny).max(1.0);
    // échelle pour tenir dans le panneau (le vaisseau occupe ~55 % de la
    // hauteur ; l'arme survolée peut déborder un peu sur les bords)
    let scale = ((panel.h as f64 * 0.55) / bh).min((panel.w as f64 * 0.45) / bw);
    let cx = panel.x as f64 + panel.w as f64 * 0.32; // vaisseau à gauche, légende à droite
    let cy = panel.y as f64 + panel.h as f64 * 0.60;
    let to_screen = |p: &Point| {
        Vec2::new(
            (cx + (p.x - ship.position.x) * scale) as f32,
            (cy + (p.y - ship.position.y) * scale) as f32,
        )
    };

    // panneau : fond + bordure
    draw_rectangle(panel.x, panel.y, panel.w, panel.h, argb_to_color(0x301AB2FF));
    draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 1.0, argb_to_color(BOX_BORDER));

    // vaisseau réel (armes possédées + extensions)
    for tri in &triangles[ship.first_triangle..=ship.last_triangle] {
        if tri.life <= 0 {
            continue;
        }
        let color = argb_to_color(if tri.color != 0 { tri.color } else { ship.shape_color });
        // chemin complet : `draw_triangle` (macroquad) est masqué par le
        // helper local du rendu des formes (7 arguments)
        macroquad::shapes::draw_triangle(
            to_screen(&tri.real_a),
            to_screen(&tri.real_b),
            to_screen(&tri.real_c),
            color,
        );
    }

    // arme survolée (non possédée) : superposée sur son emplacement
    if let Some(i) = hovered {
        if let Some(tris) = crate::vaisseau::weapon_preview_triangles(i) {
            for t in tris {
                macroquad::shapes::draw_triangle(
                    to_screen(&t.a),
                    to_screen(&t.b),
                    to_screen(&t.c),
                    argb_to_color(t.color),
                );
            }
        }
    }

    // légende à droite du vaisseau
    let (caption, color) = match hovered {
        Some(i) => {
            let name = scenario::weapon_spec(i).name.to_string();
            (format!("APERÇU AVEC : {}", name), SHOP_OK)
        }
        None => ("VAISSEAU ACTUEL".to_string(), BOX_FG),
    };
    draw_text_shadow(
        &caption,
        panel.x + panel.w * 0.60,
        panel.y + panel.h / 2.0 + 5.0,
        14.0,
        argb_to_color(color),
    );
}

/// Onglet ATELIER : une ligne par extension (réservoir, chargeur, soute) -
/// libellé + capacité + prochaine extension, coût à gauche de la pilule et
/// bouton ACHETER / MAX à droite.
pub fn draw_shop_workshop_tab(state: &GameState, l: &ShopBoxLayout, m: Vec2) {
    for (rect, pill, track) in [
        (l.fuel, l.buy_fuel_upgrade, crate::scenario::UpgradeTrackId::Fuel),
        (l.ammo, l.buy_ammo_upgrade, crate::scenario::UpgradeTrackId::Ammo),
        (l.cargo, l.buy_cargo_upgrade, crate::scenario::UpgradeTrackId::Cargo),
    ] {
        if rect.w <= 0.0 {
            continue;
        }
        let line = crate::scenario::upgrade_line(state, track);
        match line.next {
            Some(u) => {
                let cost =
                    crate::scenario::discounted_cost(u.cost, crate::scenario::current_discount(state));
                let affordable = cost <= state.resources.credits;
                // « → » : la police embarquée (DejaVu Sans Mono) possède le glyphe
                let label = format!(
                    "{} : {} → {} (+{})",
                    line.label, line.capacity, u.name, u.bonus
                );
                draw_text_shadow(&label, rect.x + 4.0, rect.y + 14.0, 15.0, argb_to_color(BOX_FG));
                let price = format!("{} CR", cost);
                let pw = measure_text(&price, None, 14, 1.0).width;
                draw_text_shadow(
                    &price,
                    rect.x + rect.w - SHOP_PILL_W - 12.0 - pw,
                    rect.y + 14.0,
                    14.0,
                    argb_to_color(if affordable { SHOP_OK } else { SHOP_ERR }),
                );
                draw_shop_pill(pill, "ACHETER", Some(affordable), m);
            }
            None => {
                let label = format!("{} : {} (MAX)", line.label, line.capacity);
                draw_text_shadow(&label, rect.x + 4.0, rect.y + 14.0, 15.0, argb_to_color(BOX_FG));
                draw_shop_pill(pill, "MAX", None, m);
            }
        }
    }
}

/// Onglet MODE DE VOL : une ligne par mode (ordre `MOVING_MODE_ORDER`) avec
/// nom, description et bouton SÉLECTIONNÉ / DÉBLOQUER {prix} / GRATUIT à
/// droite (vert si le déblocage est abordable, rouge sinon).
pub fn draw_shop_modes_tab(state: &GameState, l: &ShopBoxLayout, m: Vec2) {
    for (i, rect) in l.modes.iter().enumerate() {
        if rect.w <= 0.0 {
            continue;
        }
        let mode = MOVING_MODE_ORDER[i];
        let catalog = MOVING_MODES[mode as usize];
        let selected = state.moving_mode == mode;
        draw_text_shadow(
            catalog.name,
            rect.x + 4.0,
            rect.y + 14.0,
            15.0,
            argb_to_color(if selected { SHOP_OK } else { BOX_FG }),
        );
        draw_text_shadow(
            catalog.description,
            rect.x + 4.0,
            rect.y + 31.0,
            12.0,
            argb_to_color(BOX_FG_DIM),
        );
        let (pill_label, tone) = if selected {
            ("SÉLECTIONNÉ".to_string(), None)
        } else {
            match scenario::mode_unlock_prices(state, mode) {
                Some((_, discounted)) => {
                    let affordable = discounted <= state.resources.credits;
                    (format!("DÉBLOQUER {} CR", discounted), Some(affordable))
                }
                None => ("GRATUIT".to_string(), Some(true)),
            }
        };
        draw_shop_pill(l.buy_mode[i], &pill_label, tone, m);
    }
}

/// Dessine un **curseur de quantité** du ravitaillement du magasin (même
/// style que la barre de volume des réglages) : piste sombre, portion
/// remplie selon `value` sur un maximum `max` et pouce vertical - survol
/// blanc (glisser / molette = quantité à acheter, `game.rs`).
pub fn draw_supply_slider(track: Rect, max: f64, value: f64, m: Vec2) {
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

/// Onglet FABRICATION : une ligne par consommable (bouclier temporaire,
/// boost de vitesse, mine) - nom + recette (ingrédients GOLD/IRON/WATER
/// prélevés dans la **soute**) + inventaire à gauche, bouton FABRIQUER à
/// droite (vert si la soute couvre la recette, rouge sinon). Les
/// consommables s'utilisent en vol avec les touches 1/2/3.
pub fn draw_shop_craft_tab(state: &GameState, l: &ShopBoxLayout, m: Vec2, elements: &[Element]) {
    const NAMES: [&str; 3] = ["GOLD", "IRON", "WATER"];
    const KEYS: [&str; CRAFT_COUNT] = ["1", "2", "3"];
    for i in 0..CRAFT_COUNT {
        let rect = l.craft[i];
        if rect.w <= 0.0 {
            continue;
        }
        let spec = crate::scenario::craft_recipe(i);
        let affordable = crate::scenario::craft_affordable(state, elements, i);
        draw_text_shadow(
            spec.name,
            rect.x + 4.0,
            rect.y + 14.0,
            15.0,
            argb_to_color(BOX_FG),
        );
        // recette : « 2 IRON · 1 WATER » (ingrédients non nuls) + inventaire
        let mut parts = Vec::new();
        for (e, &need) in spec.ingredients.iter().enumerate() {
            if need > 0 {
                parts.push(format!("{} {}", need, NAMES[e]));
            }
        }
        draw_text_shadow(
            &format!("{}  |  EN STOCK : {} (touche {})", parts.join(" · "), state.consumables[i], KEYS[i]),
            rect.x + 4.0,
            rect.y + 31.0,
            12.0,
            argb_to_color(BOX_FG_DIM),
        );
        draw_shop_pill(l.buy_craft[i], "FABRIQUER", Some(affordable), m);
    }
}

/// Tooltip au survol d'une ligne du magasin : petit panneau sombre près du
/// pointeur avec le **prix**, les **effets** et la **description** de la
/// ligne survolée - pour chaque onglet (RAVITAILLEMENT, ÉQUIPEMENT, ATELIER,
/// MODE DE VOL, FABRICATION). Aucun état mutable : pur affichage.
fn draw_shop_tooltip(state: &GameState, l: &ShopBoxLayout, m: Vec2, elements: &[Element]) {
    let mut lines: Vec<String> = Vec::new();
    let mut hovered = false;
    match state.shop_tab {
        crate::config::SHOP_TAB_SUPPLIES => {
            if scenario::has_economy(state) {
                if l.supplies_fuel.w > 0.0 && l.supplies_fuel.contains(m) {
                    hovered = true;
                    let cap = scenario::fuel_capacity(state);
                    let missing = (cap - state.resources.fuel).max(0.0);
                    lines.push("CARBURANT : remplit le réservoir".to_string());
                    lines.push(format!(
                        "MANQUE : {:.0}/{} u - {} CR / {} u",
                        missing,
                        cap,
                        scenario::scenario(state.scenario).fuel_price,
                        scenario::scenario(state.scenario).fuel_step
                    ));
                    lines.push("Chaque poussée consomme du carburant".to_string());
                }
                for i in 0..scenario::weapon_slot_count() {
                    if l.supplies_ammo[i].w > 0.0 && l.supplies_ammo[i].contains(m) {
                        hovered = true;
                        let spec = scenario::weapon_spec(i);
                        lines.push(format!("{} : munitions de l'arme", spec.name));
                        lines.push(format!(
                            "{} CR / paquet de {} u",
                            spec.ammo_price, spec.ammo_pack
                        ));
                    }
                }
                if l.refill_all.w > 0.0 && l.refill_all.contains(m) {
                    hovered = true;
                    lines.push("TOUT REMPLIR : carburant + munitions".to_string());
                    lines.push("au maximum achetable avec les crédits courants".to_string());
                }
            }
        }
        crate::config::SHOP_TAB_WEAPONS => {
            for i in 0..scenario::weapon_slot_count().min(WEAPON_SLOTS) {
                if l.weapons[i].w > 0.0 && l.weapons[i].contains(m) {
                    hovered = true;
                    let spec = scenario::weapon_spec(i);
                    if let Some((_, discounted)) = scenario::weapon_prices(state, i) {
                        lines.push(format!("{} : {} CR", spec.name, discounted));
                    } else {
                        lines.push(format!("{} : équipée", spec.name));
                    }
                    lines.push(format!(
                        "Munition : {} CR / {} u - tire depuis son emplacement",
                        spec.ammo_price, spec.ammo_pack
                    ));
                }
            }
            if l.radar.w > 0.0 && l.radar.contains(m) {
                hovered = true;
                let (_, discounted) = scenario::radar_price(state).unwrap_or((0, 0));
                lines.push(format!("RADAR : minimap globale des météores ({} CR)", discounted));
                lines.push("Éteint par défaut en scénario à économie".to_string());
            }
        }
        crate::config::SHOP_TAB_WORKSHOP => {
            for (rect, track) in [
                (l.fuel, crate::scenario::UpgradeTrackId::Fuel),
                (l.ammo, crate::scenario::UpgradeTrackId::Ammo),
                (l.cargo, crate::scenario::UpgradeTrackId::Cargo),
            ] {
                if rect.w > 0.0 && rect.contains(m) {
                    hovered = true;
                    let line = crate::scenario::upgrade_line(state, track);
                    lines.push(format!("{} : capacité {}", line.label, line.capacity));
                    match line.next {
                        Some(u) => {
                            let cost = crate::scenario::discounted_cost(
                                u.cost,
                                crate::scenario::current_discount(state),
                            );
                            lines.push(format!(
                                "Extension : {} (+{} u) - {} CR",
                                u.name, u.bonus, cost
                            ));
                        }
                        None => lines.push("Niveau maximum atteint".to_string()),
                    }
                }
            }
        }
        crate::config::SHOP_TAB_MODES => {
            for (i, rect) in l.modes.iter().enumerate() {
                if rect.w > 0.0 && rect.contains(m) {
                    hovered = true;
                    let mode = MOVING_MODE_ORDER[i];
                    let catalog = MOVING_MODES[mode as usize];
                    lines.push(format!("{} : {}", catalog.name, catalog.description));
                    match scenario::mode_unlock_prices(state, mode) {
                        Some((_, discounted)) => lines.push(format!("Déblocage : {} CR", discounted)),
                        None => lines.push("Déjà débloqué".to_string()),
                    }
                }
            }
        }
        _ => {
            // FABRICATION
            for i in 0..CRAFT_COUNT {
                if l.craft[i].w > 0.0 && l.craft[i].contains(m) {
                    hovered = true;
                    let spec = crate::scenario::craft_recipe(i);
                    lines.push(format!("{} : {}", spec.name, spec.description));
                    const NAMES: [&str; 3] = ["GOLD", "IRON", "WATER"];
                    let mut parts = Vec::new();
                    for (e, &need) in spec.ingredients.iter().enumerate() {
                        if need > 0 {
                            let have = elements.get(e + 1).map_or(0, |el| el.count);
                            parts.push(format!("{} {} ({} en soute)", need, NAMES[e], have));
                        }
                    }
                    lines.push(parts.join(" · "));
                }
            }
        }
    }
    if hovered && !lines.is_empty() {
        draw_tooltip_box(&lines, m);
    }
}

/// Dessine un panneau de tooltip (fond sombre + bordures) près du pointeur,
/// avec les lignes de texte données (chaque ligne = une entrée du tooltip).
fn draw_tooltip_box(lines: &[String], m: Vec2) {
    let font = 12u16;
    let line_h = 16.0f32;
    let pad = 6.0f32;
    let mut w = 0.0f32;
    for l in lines {
        let tw = measure_text(l, None, font, 1.0).width;
        w = w.max(tw);
    }
    w += 2.0 * pad;
    let h = lines.len() as f32 * line_h + 2.0 * pad;
    // position près du pointeur, sans déborder de l'écran
    let mut x = m.x + 14.0;
    let mut y = m.y + 14.0;
    if x + w > VIEWPORT_WIDTH as f32 {
        x = (m.x - w - 14.0).max(0.0);
    }
    if y + h > VIEWPORT_HEIGHT as f32 {
        y = (m.y - h - 14.0).max(0.0);
    }
    draw_rectangle(x, y, w, h, Color::new(0.03, 0.04, 0.07, 0.95));
    draw_rectangle_lines(x, y, w, h, 1.0, argb_to_color(SHOP_OK));
    for (i, l) in lines.iter().enumerate() {
        draw_text(l, x + pad, y + pad + 12.0 + i as f32 * line_h, font as f32, WHITE);
    }
}

/// Dessine un bouton de boîte (cadre + texte centré, hover : fond surbrillé
/// + texte blanc).
pub fn draw_box_button(label: &str, rect: Rect) {
    let m = mouse_to_game();
    let hovered = rect.contains(m);
    let color = argb_to_color(if hovered { BOX_HOVER } else { BOX_FG });
    if hovered {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, argb_to_color(0x401AB2FF));
    }
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.5, color);
    let text_w = measure_text(label, None, 16, 1.0).width;
    draw_text_shadow(label, rect.x + (rect.w - text_w) / 2.0, rect.y + 18.0, 16.0, color);
}
