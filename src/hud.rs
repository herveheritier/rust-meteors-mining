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

/// Cargo : cercles à `x = 11*i + 5`, `y = 50`, remplis de la couleur de
/// l'élément (GOLD, IRON, WATER puis PLATINUM - tous les éléments de la
/// soute), vide = contour gris (ex mainLoop).
pub fn draw_cargo(state: &GameState, elements: &[Element]) {
    // bornes cumulées de chaque élément (le i-ème emplacement contient le
    // premier élément dont le cumul dépasse i)
    let mut cum: Vec<i32> = Vec::with_capacity(elements.len());
    let mut acc = 0;
    for e in 1..elements.len() {
        acc += elements[e].count;
        cum.push(acc);
    }
    // soute presque pleine : les baies occupées clignotent (elles alternent
    // leur couleur ↔ rouge tant que le cargo reste à `HUD_FULL_CARGO_RATIO`
    // de sa capacité - les emplacements vides gardent leur contour gris)
    let almost_full = state.player.cargo_size > 0
        && state.player.cargo_qty as f64 / state.player.cargo_size as f64 >= HUD_FULL_CARGO_RATIO;
    let blink_on = almost_full && (get_time() * HUD_BLINK_HZ) as i64 % 2 == 0;
    for i in 1..=state.player.cargo_size {
        let color = elements
            .iter()
            .enumerate()
            .skip(1)
            .find(|(e, _)| i <= cum[e - 1])
            .map(|(_, el)| el.color)
            .unwrap_or(0xFF808080);
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
    // ou vies + bouclier (Survival) - champs fixes : 3/3/2/2/5 chiffres.
    // Le score composite + record est affiché sur la **deuxième ligne**
    // (`draw_score_hud`) : la ligne principale est réservée au statut
    // d'accostage (distance à la base), prioritaire - il reprend sa place
    // juste après les ressources (colonnes de départ fixes, anti-tremblement)
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
        let x = hud_col_x(HUD_RESOURCES_COL);
        draw_text(
            &format!(
                "LIVES:{:>1} SHIELD:{:>1.0}",
                state.resources.lives, state.resources.shield
            ),
            x,
            14.0,
            16.0,
            WHITE,
        );
        HUD_RESOURCES_COL + HUD_RESOURCES_SURVIVAL_COLS + 1
    } else {
        // jeu libre : pas de ressources - l'accostage suit directement PRECISION
        HUD_RESOURCES_COL + 1
    };
    // fin de partie (Survival, dernière vie perdue) : récapitulatif de la
    // session (statistiques), GAME OVER au centre, rappel des touches et
    // deux boutons cliquables (R = nouvelle partie, T = écran titre - clic
    // détecté côté `game::game_over_button_click`)
    if state.game_over {
        draw_session_recap(state);
        let msg = "GAME OVER";
        let w = measure_text(msg, None, 32, 1.0).width;
        draw_text(
            msg,
            (VIEWPORT_WIDTH as f32 - w) / 2.0,
            VIEWPORT_HEIGHT as f32 / 2.0,
            32.0,
            argb_to_color(0xFFFF4040),
        );
        let hint = "R : NEW GAME   -   T : TITLE   -   ESC : QUIT";
        let hw = measure_text(hint, None, 16, 1.0).width;
        draw_text(
            hint,
            (VIEWPORT_WIDTH as f32 - hw) / 2.0,
            VIEWPORT_HEIGHT as f32 / 2.0 + 28.0,
            16.0,
            argb_to_color(BOX_FG),
        );
        let [restart, title] = game_over_buttons_layout();
        crate::shop_render::draw_box_button("NEW GAME", restart);
        crate::shop_render::draw_box_button("TITLE", title);
    }
    hud_col_x(dock_col)
}

/// Score composite + record, **en bas à droite** de l'écran (aligné à droite
/// sur la ligne du bas, avec une marge) : « SCORE:… BEST:… », valeurs
/// alignées à droite sur 5 chiffres (anti-tremblement). Position permanente
/// qui ne gêne ni la ligne principale du HUD (réservée au statut
/// d'accostage - distance à la base prioritaire) ni les messages du bas
/// (centrés, ils restent à gauche du score - voir `draw_hud`).
pub fn draw_score_hud(state: &GameState) {
    let score = scenario::composite_score(state);
    let score_txt = format!(
        " SCORE:{:>5} BEST:{:>5}",
        score.min(99999),
        state.high_score.min(99999)
    );
    let w = measure_text(&score_txt, None, 16, 1.0).width;
    draw_text_shadow(
        &score_txt,
        VIEWPORT_WIDTH as f32 - w - 8.0,
        VIEWPORT_HEIGHT as f32 - 16.0,
        16.0,
        WHITE,
    );
}

/// Géométrie des deux boutons de l'écran GAME OVER (côte à côte sous le
/// bandeau) : index 0 = NEW GAME (touche R), 1 = TITLE (touche T). Le clic
/// est détecté côté `game::game_over_button_click` avec ces rectangles.
pub fn game_over_buttons_layout() -> [Rect; 2] {
    let labels = ["NEW GAME", "TITLE"];
    let btn_h = 26.0;
    let gap = 12.0;
    // même formule que les boutons de boîte (largeur du libellé + padding,
    // minimum 60)
    let widths = labels.map(|l| (l.len() as f32 * 8.0 + 2.0 * BOX_PADDING).max(60.0));
    let left = (VIEWPORT_WIDTH as f32 - widths[0] - gap - widths[1]) / 2.0;
    let top = VIEWPORT_HEIGHT as f32 / 2.0 + 44.0;
    [
        Rect::new(left, top, widths[0], btn_h),
        Rect::new(left + widths[0] + gap, top, widths[1], btn_h),
    ]
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

/// Récapitulatif de **fin de partie** (écran GAME OVER) : panneau sombre
/// avec les statistiques de la session - temps de vol, distance parcourue,
/// précision de tir, météores et triangles minéralisés détruits, accostages,
/// valeur de la cargaison déchargée et score composite.
pub fn draw_session_recap(state: &GameState) {
    let st = &state.session_stats;
    let mins = (st.flight_time / 60.0) as i32;
    let secs = (st.flight_time % 60.0) as i32;
    let time_txt = if mins > 0 {
        format!("{} min {} s", mins, secs)
    } else {
        format!("{:.0} s", st.flight_time)
    };
    let precision = if state.bullets_fired > 0 {
        (100.0 * (1.0 - state.bullets_lost as f64 / state.bullets_fired as f64)) as i32
    } else {
        0
    };
    let lines = [
        "RÉCAPITULATIF DE SESSION".to_string(),
        format!("Temps de vol : {}", time_txt),
        format!("Distance parcourue : {:.0} u", st.distance),
        format!("Précision de tir : {} % ({}/{})", precision, state.bullets_fired - state.bullets_lost, state.bullets_fired),
        format!("Météores détruits : {}", state.meteors_destroyed),
        format!("Triangles minéralisés détruits : {}", st.minerals_destroyed),
        format!("Accostages : {}", state.docking_count),
        format!("Cargaison déchargée : {} CR", st.cargo_value_unloaded),
        format!("Score : {}", crate::scenario::composite_score(state)),
    ];
    // panneau centré au-dessus du bandeau GAME OVER
    let w = 460.0;
    let line_h = 20.0;
    let h = 12.0 + lines.len() as f32 * line_h + 12.0;
    let x = (VIEWPORT_WIDTH as f32 - w) / 2.0;
    let y = VIEWPORT_HEIGHT as f32 / 2.0 - h - 70.0;
    draw_rectangle(x + 2.0, y + 2.0, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(x, y, w, h, Color::new(0.05, 0.07, 0.10, 0.85));
    draw_rectangle_lines(x, y, w, h, 1.5, argb_to_color(BOX_BORDER));
    for (i, line) in lines.iter().enumerate() {
        let color = if i == 0 { SHOP_OK } else { BOX_FG };
        let wl = measure_text(line, None, 16, 1.0).width;
        draw_text_shadow(
            line,
            x + (w - wl) / 2.0,
            y + 16.0 + i as f32 * line_h,
            16.0,
            argb_to_color(color),
        );
    }
}

/// Journal de bord (touche L) : panneau semi-transparent dans le coin
/// inférieur droit listant les `EVENT_LOG_LEN` derniers événements (le plus
/// récent en tête) - tirs, minerais, accostages, achats, destructions…
pub fn draw_log_box(state: &GameState) {
    if !state.log_box {
        return;
    }
    let w = 420.0;
    let pad = 10.0;
    let line_h = 18.0;
    let max_lines = crate::config::EVENT_LOG_LEN;
    let h = pad + 24.0 + max_lines as f32 * line_h + pad;
    let x = VIEWPORT_WIDTH as f32 - w - 12.0;
    let y = VIEWPORT_HEIGHT as f32 - h - 40.0;
    draw_rectangle(x + 2.0, y + 2.0, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(x, y, w, h, Color::new(0.05, 0.07, 0.10, 0.88));
    draw_rectangle_lines(x, y, w, h, 1.5, argb_to_color(BOX_BORDER));
    draw_text_shadow("JOURNAL DE BORD (L : FERMER)", x + pad, y + 16.0, 16.0, argb_to_color(SHOP_OK));
    if state.event_log.is_empty() {
        draw_text_shadow("(vide)", x + pad, y + 40.0, 14.0, argb_to_color(BOX_FG_DIM));
        return;
    }
    // chaque événement est replié à la largeur du panneau (les événements
    // longs ne débordent pas) - seuls les EVENT_LOG_LEN premiers comptent
    let mut rows: Vec<String> = Vec::new();
    for ev in &state.event_log {
        for line in crate::hud::wrap_text(ev, w - 2.0 * pad, 14) {
            rows.push(line);
            if rows.len() >= max_lines {
                break;
            }
        }
        if rows.len() >= max_lines {
            break;
        }
    }
    for (i, row) in rows.iter().enumerate() {
        draw_text_shadow(row, x + pad, y + 40.0 + i as f32 * line_h, 14.0, argb_to_color(BOX_FG));
    }
}

/// Consommables actifs (HUD) : petite ligne sous le HUD - bouclier
/// temporaire restant, boost en cours et mines en stock (touches 1/2/3).
/// Rien n'est affiché tant que rien n'est actif.
pub fn draw_consumables_hud(state: &GameState) {
    let mut parts: Vec<(String, u32)> = Vec::new();
    if state.temp_shield > 0.0 {
        parts.push((format!("SHLD:{:.0}", state.temp_shield), SHOP_OK));
    }
    if state.boost_timer > 0.0 {
        parts.push((format!("BOOST:{:.0}s", state.boost_timer.ceil()), 0xFF40C0FF));
    }
    if state.consumables[CRAFT_MINE] > 0 {
        parts.push((format!("MINE:{}", state.consumables[CRAFT_MINE]), 0xFFFF8040));
    }
    if parts.is_empty() {
        return;
    }
    let mut x = hud_col_x(HUD_FPS_COL);
    for (p, color) in &parts {
        draw_text(p, x, 30.0, 16.0, argb_to_color(*color));
        x += measure_text(p, None, 16, 1.0).width + 12.0;
    }
}

/// Géométrie du panneau de **briefing pré-partie** (scénarios custom) :
/// panneau centré de largeur fixe, bouton CLOSE en bas au centre. Renvoie
/// (panneau, bouton CLOSE).
pub fn briefing_layout() -> (Rect, Rect) {
    let w = 640.0;
    let h = 400.0;
    let x = (VIEWPORT_WIDTH as f32 - w) / 2.0;
    let y = (VIEWPORT_HEIGHT as f32 - h) / 2.0;
    (
        Rect::new(x, y, w, h),
        Rect::new(x + w / 2.0 - 60.0, y + h - 44.0, 120.0, 28.0),
    )
}

/// Clic sur le bouton CLOSE du briefing pré-partie (ENTRÉE/ÉCHAP ferment
/// aussi - voir `game.rs`).
pub fn briefing_close_clicked() -> bool {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return false;
    }
    let (_, close) = briefing_layout();
    close.contains(mouse_to_game())
}

/// Lignes de texte du briefing pré-partie : titre du scénario, description,
/// objectifs DAG (titre + description), contraintes (fuel/ammo/credits ou
/// vies/bouclier) et un conseil.
pub fn briefing_lines(state: &GameState) -> Vec<String> {
    let mut lines = Vec::new();
    match state.scenario {
        crate::scenario::ScenarioId::Custom(ci) => {
            if let Some(data) = crate::scenario_loader::loaded_data(ci) {
                lines.push(format!("SCÉNARIO : {}", data.json.name));
                if !data.json.description.is_empty() {
                    lines.push(data.json.description.clone());
                }
                lines.push(String::new());
                lines.push("OBJECTIFS :".to_string());
                for o in &data.json.objectives {
                    let title = if o.title.is_empty() {
                        o.id.clone()
                    } else {
                        o.title.clone()
                    };
                    lines.push(format!("• {}", title));
                    if !o.description.is_empty() {
                        lines.push(format!("    {}", o.description));
                    }
                }
            }
        }
        _ => {}
    }
    lines.push(String::new());
    let s = crate::scenario::scenario(state.scenario);
    if s.has_economy {
        lines.push(format!(
            "CONTRAINTES : carburant {:.0} u, munitions {}, crédits {}",
            s.start_fuel,
            s.start_ammo,
            state.resources.credits
        ));
    } else if s.lives > 0 {
        lines.push(format!(
            "CONTRAINTES : {} vies, bouclier {:.0} points",
            s.lives, s.shield_capacity
        ));
    } else {
        lines.push("CONTRAINTES : aucune - carburant et munitions illimités".to_string());
    }
    lines.push(String::new());
    lines.push("CONSEIL : les objectifs s'enchaînent selon leurs prérequis (DAG).".to_string());
    lines.push("Suivez le panneau OBJECTIFS du HUD et revenez à la station pour".to_string());
    lines.push("décharger, ravitailler et fabriquer entre deux étapes.".to_string());
    lines
}

/// Hauteur verticale (px) d'une ligne du contenu du briefing.
pub const BRIEFING_LINE_H: f32 = 19.0;
/// Largeur (px) de la piste de l'ascenseur du briefing.
pub const BRIEFING_TRACK_W: f32 = 8.0;

/// Contenu du briefing découpé en **lignes rendues** (repliées à la largeur
/// du panneau), chacune avec sa couleur : sert au dessin ET au calcul de la
/// hauteur totale pour l'ascenseur (`draw_briefing_box`).
fn briefing_wrapped_lines(state: &GameState) -> Vec<(String, u32)> {
    let text_w = briefing_layout().0.w - 40.0;
    let mut out = Vec::new();
    for line in briefing_lines(state) {
        for wrapped in wrap_text(&line, text_w, 15) {
            let color = if wrapped.starts_with("SCÉNARIO")
                || wrapped.starts_with("OBJECTIFS")
                || wrapped.starts_with("CONTRAINTES")
                || wrapped.starts_with("CONSEIL")
            {
                SHOP_OK
            } else {
                BOX_FG
            };
            out.push((wrapped, color));
        }
    }
    out
}

/// Zone **défilante** du briefing : (haut, bas) en px, entre le titre et le
/// rappel des touches / bouton CLOSE - rien ne peut déborder sous le bas
/// tant que le défilement est borné par `briefing_scroll_max`.
fn briefing_scroll_rect() -> (f32, f32) {
    let (panel, close) = briefing_layout();
    let top = panel.y + 62.0;
    // le bas s'arrête bien au-dessus du rappel des touches (`close.y - 34`,
    // une ligne de marge) : une ligne qui commence au bas finit à
    // `bottom + BRIEFING_LINE_H` < la position du rappel.
    let bottom = close.y - 34.0;
    (top, bottom)
}

/// Défilement maximal du briefing (px) : le bas de la dernière ligne s'aligne
/// avec le bas de la zone défilante (0 = tout tient, pas d'ascenseur).
pub fn briefing_scroll_max(state: &GameState) -> f32 {
    let (top, bottom) = briefing_scroll_rect();
    let visible = bottom - top;
    let content_h = briefing_wrapped_lines(state).len() as f32 * BRIEFING_LINE_H;
    (content_h - visible).max(0.0)
}

/// Défilement demandé ce frame pour le briefing (px) : molette de la souris,
/// flèches haut/bas et PgPréc/PgSuiv du clavier. À ajouter à l'offset puis à
/// borner par `briefing_scroll_max` (`game.rs`).
pub fn briefing_scroll_delta() -> f32 {
    let mut d = 0.0;
    // molette : un « cran » = 3 lignes, borné pour rester stable quelle que
    // soit la granularité reportée par l'OS (certains renvoient ±1, d'autres
    // ±120 par cran)
    let wheel = mouse_wheel().1;
    if wheel != 0.0 {
        d += wheel.signum() * wheel.abs().min(3.0) * 3.0;
    }
    // clavier : flèches (ligne par ligne) et PgPréc/PgSuiv (6 lignes)
    if is_key_down(KeyCode::Down) {
        d += 1.0;
    }
    if is_key_down(KeyCode::Up) {
        d -= 1.0;
    }
    if is_key_pressed(KeyCode::PageDown) {
        d += 6.0;
    }
    if is_key_pressed(KeyCode::PageUp) {
        d -= 6.0;
    }
    d * BRIEFING_LINE_H
}

/// Géométrie de l'ascenseur du briefing : zone cliquable de la **piste**,
/// hauteur du **curseur** et **défilement maximal**. `None` quand le contenu
/// tient entièrement (pas d'ascenseur). Source unique partagée par le dessin
/// (`draw_briefing_box`) et l'interaction souris (`briefing_mouse_scroll`) -
/// le curseur dessiné et la zone clut puis atteignable à la souris coïncident.
fn briefing_scrollbar(state: &GameState) -> Option<(Rect, f32, f32)> {
    let (panel, _) = briefing_layout();
    let (top, bottom) = briefing_scroll_rect();
    let visible = bottom - top;
    let content_h = briefing_wrapped_lines(state).len() as f32 * BRIEFING_LINE_H;
    let max_scroll = (content_h - visible).max(0.0);
    if max_scroll <= 0.5 {
        return None;
    }
    let track_x = panel.x + panel.w - BRIEFING_TRACK_W - 10.0;
    // la piste cliquable inclut la bordure (`BRIEFING_TRACK_W + 2`)
    let track = Rect::new(track_x, top, BRIEFING_TRACK_W + 2.0, visible);
    let thumb_h = (visible * visible / content_h).clamp(20.0, visible - 8.0);
    Some((track, thumb_h, max_scroll))
}

/// Interaction **souris** sur l'ascenseur du briefing : saisie et déplacement
/// du curseur, ou clic sur la piste (saut + saisie), avec le bouton gauche
/// maintenu. Renvoie le **nouvel offset absolu** de défilement quand la
/// souris le pilote (à affecter tel quel à `state.briefing_scroll`), `None`
/// sinon (à laisser au clavier/molette). Appelé par `game.rs` quand le
/// briefing est ouvert.
pub fn briefing_mouse_scroll(state: &mut GameState) -> Option<f32> {
    let Some((track, thumb_h, max_scroll)) = briefing_scrollbar(state) else {
        state.briefing_drag_anchor = None;
        return None;
    };
    // bouton relâché : fin de la saisie
    if !is_mouse_button_down(MouseButton::Left) {
        state.briefing_drag_anchor = None;
        return None;
    }
    let m = mouse_to_game();
    let top = track.y;
    let scroll = state.briefing_scroll.clamp(0.0, max_scroll);
    let thumb_y = top + scroll / max_scroll * (track.h - thumb_h);
    let frac_for = |handle_y: f32| ((handle_y - top) / (track.h - thumb_h)).clamp(0.0, 1.0);
    // saisie en cours : le curseur suit verticalement la souris
    if let Some(anchor) = state.briefing_drag_anchor {
        return Some(frac_for(m.y - anchor) * max_scroll);
    }
    // nouveau clic sur la piste : on saisit le curseur (ou on saute dedans)
    if is_mouse_button_pressed(MouseButton::Left) && track.contains(m) {
        let anchor = if m.y < thumb_y {
            0.0
        } else if m.y > thumb_y + thumb_h {
            thumb_h
        } else {
            m.y - thumb_y // saisie du curseur : pas de saut, saisie à l'endroit du clic
        };
        state.briefing_drag_anchor = Some(anchor);
        return Some(frac_for(m.y - anchor) * max_scroll);
    }
    None
}

/// Écran de **briefing pré-partie** (scénarios custom avec objectifs,
/// affiché au lancement de la partie avant de jouer) : panneau sombre au
/// centre listant les objectifs DAG, les contraintes (fuel/ammo ou
/// vies/bouclier) et un conseil - fermé par ENTRÉE / ÉCHAP / clic sur CLOSE
/// (`game.rs`). Le contenu est **borné à la zone défilante** du panneau :
/// s'il est trop long, un **ascenseur** (piste + curseur) apparaît sur le
/// bord droit et on ne dessine que les lignes visibles - rien ne dépasse le
/// panneau ni l'écran.
pub fn draw_briefing_box(state: &GameState) {
    if !state.briefing_box {
        return;
    }
    let (panel, close) = briefing_layout();
    let (view_top, view_bottom) = briefing_scroll_rect();
    // assombrissement du monde derrière
    draw_rectangle(
        0.0,
        0.0,
        VIEWPORT_WIDTH as f32,
        VIEWPORT_HEIGHT as f32,
        Color::new(0.0, 0.0, 0.0, 0.55),
    );
    draw_rectangle(panel.x + 3.0, panel.y + 3.0, panel.w, panel.h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle(panel.x, panel.y, panel.w, panel.h, Color::new(0.05, 0.07, 0.10, 0.92));
    draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 2.0, argb_to_color(SHOP_OK));

    // titre
    draw_text_shadow(
        "BRIEFING DE MISSION",
        panel.x + 20.0,
        panel.y + 30.0,
        22.0,
        argb_to_color(SHOP_OK),
    );

    // contenu défilant, borné à la zone visible
    let lines = briefing_wrapped_lines(state);
    let visible = view_bottom - view_top;
    let content_h = lines.len() as f32 * BRIEFING_LINE_H;
    let max_scroll = (content_h - visible).max(0.0);
    let scroll = state.briefing_scroll.clamp(0.0, max_scroll);
    let first = (scroll / BRIEFING_LINE_H).floor() as usize;
    let mut y = view_top - (scroll - first as f32 * BRIEFING_LINE_H);
    for (text, color) in &lines[first..] {
        // on ne dessine qu'une ligne entièrement dans la zone : au-dessus du
        // haut (ligne partiellement coupée) ou sous le bas, on l'ignore
        if y + BRIEFING_LINE_H - 0.5 > view_bottom {
            break;
        }
        if y >= view_top - 0.5 {
            draw_text_shadow(text, panel.x + 20.0, y, 15.0, argb_to_color(*color));
        }
        y += BRIEFING_LINE_H;
    }

    // ascenseur (piste + curseur) si le contenu dépasse la zone visible -
    // même géométrie que l'interaction souris (`briefing_mouse_scroll`)
    if let Some((track, thumb_h, _)) = briefing_scrollbar(state) {
        let thumb_y = track.y + scroll / max_scroll * (track.h - thumb_h);
        draw_rectangle(track.x + 1.0, track.y + 1.0, BRIEFING_TRACK_W, track.h, Color::new(0.0, 0.0, 0.0, 0.45));
        draw_rectangle_lines(track.x, track.y, track.w, track.h, 1.0, argb_to_color(BOX_BORDER));
        draw_rectangle(track.x, thumb_y, track.w, thumb_h, argb_to_color(SHOP_OK));
    }

    // bouton CLOSE + rappel des touches
    let hint = "ENTRÉE / ÉCHAP : LANCER LA MISSION";
    let hw = measure_text(hint, None, 13, 1.0).width;
    draw_text_shadow(
        hint,
        panel.x + (panel.w - hw) / 2.0,
        close.y - 8.0,
        13.0,
        argb_to_color(BOX_FG_DIM),
    );
    draw_box_button("CLOSE", close);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_over_buttons_are_within_viewport_and_disjoint() {
        // les deux boutons tiennent dans la vue et sont côte à côte, sans
        // chevauchement (cliquables sans ambiguïté)
        let [restart, title] = game_over_buttons_layout();
        for r in [&restart, &title] {
            assert!(r.x >= 0.0 && r.x + r.w <= VIEWPORT_WIDTH as f32);
            assert!(r.y >= 0.0 && r.y + r.h <= VIEWPORT_HEIGHT as f32);
        }
        assert!(restart.x + restart.w <= title.x);
    }

    #[test]
    fn briefing_panel_is_fully_visible_and_content_is_clipped() {
        // la fenêtre de briefing est entièrement dans la vue (jamais
        // tronquée par le bord de l'écran)
        let (panel, close) = briefing_layout();
        assert!(panel.x >= 0.0 && panel.x + panel.w <= VIEWPORT_WIDTH as f32);
        assert!(panel.y >= 0.0 && panel.y + panel.h <= VIEWPORT_HEIGHT as f32);
        assert!(close.x >= panel.x && close.x + close.w <= panel.x + panel.w);
        assert!(close.y >= panel.y && close.y + close.h <= panel.y + panel.h);

        // le bas de la zone défilante laisse la place à une ligne : une ligne
        // qui y commence ne recouvre jamais le rappel des touches ni le
        // bouton CLOSE (le contenu ne déborde pas de la fenêtre)
        let (top, bottom) = briefing_scroll_rect();
        assert!(top < bottom, "la zone défilante doit avoir une hauteur positive");
        assert!(
            bottom + BRIEFING_LINE_H <= close.y,
            "le contenu du briefing ne doit pas recouvrir le bouton CLOSE"
        );
        assert!(bottom + BRIEFING_LINE_H <= VIEWPORT_HEIGHT as f32);
    }
}
