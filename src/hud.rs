//! HUD du jeu : jauge de carburant, munitions, crédits, réputation,
//! messages, objectifs DAG, cargo, statut d'accostage et écran I -
//! portage des blocs d'affichage de `mainLoop` (issu de `src/render.rs`).

use macroquad::prelude::*;
use crate::config::*;
use crate::render::*;
use crate::font::{draw_text, measure_text};
use crate::garbage::Garbage;
use crate::geom::{Point, Triangle};
use crate::scenario;
use crate::shape::Shape;
use crate::state::{Element, GameState};

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
    // distance la plus courte dans le monde torique (repliement cyclique)
    let dist = crate::geom::wrapped_distance(player_position, station_position, &state.world);
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

/// Abscisse (px) d'une colonne de la grille 8 px (x = 8+(col-1)*8).
pub fn hud_col_x(col: i32) -> f32 {
    8.0 + (col - 1) as f32 * 8.0
}

/// HUD : FPS, réputation (+ rang en scénario à économie), précision et
/// ressources du scénario (carburant, munitions, minerais - ou vies/bouclier
/// en Survival) sur une **seule ligne** en haut de l'écran, à des colonnes
/// fixes (anti-tremblement). Renvoie l'abscisse de l'emplacement fixe du
/// statut d'accostage pour que `draw_docking_hud` l'affiche sur la même
/// ligne. Police embarquée DejaVu Sans Mono dessinée à l'échelle 8 px (la
/// grille 8×16 de l'original est conservée, voir `font.rs`).
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
        let min_txt = format!(" CREDITS:{:>5}", state.resources.credits);
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
    for s in shapes.iter().skip(1) {
        if s.life <= 0 {
            continue;
        }
        let t = triangles[s.first_triangle..=s.last_triangle]
            .iter()
            .filter(|tri| tri.life > 0)
            .count();
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
    // `minerals` - libérés en minerais quand deux météores se détruisent)
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

/// Écran **PAUSE** (touche P) : le monde est gelé mais rien ne distingue
/// l'état d'une frame normale - l'overlay assombrit le cadre (le monde gelé
/// reste perceptible) et affiche un bandeau central + le rappel de la touche
/// de reprise. Appelé par la boucle de rendu quand `state.paused`, tant
/// qu'aucune fenêtre (accostage, magasin, aide, paramétrage) ne recouvre
/// l'écran.
pub fn draw_pause_overlay() {
    // assombrissement léger : le monde gelé derrière reste lisible
    draw_rectangle(
        0.0,
        0.0,
        VIEWPORT_WIDTH as f32,
        VIEWPORT_HEIGHT as f32,
        Color::new(0.0, 0.0, 0.0, 0.35),
    );
    // bandeau central (ombre portée pour le détacher du fond)
    let label = "PAUSE";
    let font_size = 48.0;
    let w = measure_text(label, None, font_size as u16, 1.0).width;
    draw_text_shadow(
        label,
        (VIEWPORT_WIDTH as f32 - w) / 2.0,
        VIEWPORT_HEIGHT as f32 * 0.40,
        font_size,
        argb_to_color(BOX_FG),
    );
    // rappel de la touche de reprise + état du radar (équipement - l'overlay
    // couvre la minimap, mais le rappel de reprise est l'essentiel)
    let hint = "APPUYER SUR P POUR REPRENDRE";
    let hw = measure_text(hint, None, 16, 1.0).width;
    draw_text_shadow(
        hint,
        (VIEWPORT_WIDTH as f32 - hw) / 2.0,
        VIEWPORT_HEIGHT as f32 * 0.40 + 64.0,
        16.0,
        argb_to_color(BOX_FG_DIM),
    );
}

/// Découpe `text` en plusieurs lignes qui tiennent dans `max_width` pixels à
/// la taille de police `font_size` (coupure aux espaces, sans couper les
/// mots). Permet d'afficher un objectif de scénario en entier, sans troncature.
pub fn wrap_text(text: &str, max_width: f32, font_size: u16) -> Vec<String> {
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
    let cond_lines = primary
        .map(|o| wrap_text(&format!("→ {}", format_condition_hud(&o.condition, state, o.active_time)), text_w, desc_font))
        .unwrap_or_default();
    let sub_lines = secondary
        .map(|o| wrap_text(&format!("→ {}", o.title), text_w, sub_font))
        .unwrap_or_default();

    let title_line_h = 20.0f32;
    let desc_line_h = 20.0f32;
    let sub_line_h = 20.0f32;

    // Hauteur dynamique : la boîte s'adapte à la longueur du texte complet
    let mut panel_h: f32 = 14.0; // padding haut
    panel_h += 20.0; // en-tête
    panel_h += title_lines.len() as f32 * title_line_h;
    panel_h += desc_lines.len() as f32 * desc_line_h;
    panel_h += cond_lines.len() as f32 * desc_line_h;
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
        for line in &cond_lines {
            draw_text_shadow(line, text_x, y, desc_font as f32, Color::new(0.0, 1.0, 1.0, 0.9));
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

        // Ligne 1 : OBJECTIF ATTEINT (« ✓ » : la police embarquée DejaVu
        // Sans Mono possède le glyphe)
        let check = "✓ OBJECTIF ATTEINT";
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
pub fn format_condition_hud(cond: &crate::scenario_loader::JsonCondition, state: &GameState, active_time: f64) -> String {
    match cond.condition_type.as_str() {
        "DestroyAsteroids" => {
            let current = state.meteors_destroyed.min(cond.required as i32);
            format!("Meteors: {}/{}", current, cond.required)
        }
        "CollectMinerals" => {
            let current = state.resources.credits.min(cond.required as i32);
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
        "SurviveTime" => {
            let target = if cond.seconds > 0.0 {
                cond.seconds
            } else if cond.required > 0 {
                cond.required as f64
            } else {
                30.0
            };
            let current = active_time.min(target);
            format!("Survive: {:.0}/{}s", current, target as u32)
        }
        "PrecisionShooting" => {
            let target_pct = (cond.min_precision * 100.0).round() as i32;
            if state.bullets_fired == 0 {
                format!("Precision: 0/{} hits, {}% min", cond.hits, target_pct)
            } else {
                let hits = state.bullets_fired - state.bullets_lost;
                let precision = 100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64);
                format!("Hits: {}/{} ({}% / {}% min)", hits, cond.hits, precision as i32, target_pct)
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
