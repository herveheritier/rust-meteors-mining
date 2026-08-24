//! Magasin de la station (bouton SHOP de la boîte DOCK STATION) :
//! ravitaillement (carburant/munitions à la quantité, curseurs),
//! armes, radar, extensions d'atelier et modes de déplacement -
//! portage de `src/game.rs`.

use macroquad::prelude::*;
use crate::config::*;
use crate::geom::Triangle;
use crate::persist;
use crate::render::{mouse_to_game, shop_box_layout};
use crate::scenario;
use crate::shape::Shape;
use crate::state::GameState;

/// Bouton cliqué sur le magasin de la station (bouton SHOP de la boîte DOCK
/// STATION).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopClick {
    None,
    /// Sélectionne / débloque un mode de déplacement (index `MOVING_MODE_*`).
    Mode(i32),
    /// Achète une arme du catalogue (index dans `VAISSEAU_WEAPONS`).
    Weapon(usize),
    /// Achète le **radar de bord** (minimap globale - onglet ÉQUIPEMENT).
    BuyRadar,
    /// Achète la quantité du curseur de carburant (ligne FUEL du
    /// ravitaillement).
    Refuel,
    /// Achète la quantité du curseur de munitions de l'arme `i` (ligne AMMO
    /// de l'arme - une par arme possédée).
    Rearm(usize),
    /// Remplit le carburant et les munitions de toutes les armes possédées
    /// au maximum achetable (bouton TOUT REMPLIR).
    RefillAll,
    /// Achète l'extension de réservoir de carburant (atelier).
    BuyFuelUpgrade,
    /// Achète l'extension de chargeur de munitions (atelier).
    BuyAmmoUpgrade,
    /// Achète l'extension de soute (atelier).
    BuyCargoUpgrade,
    /// Revient à la boîte DOCK STATION (toujours accosté).
    Close,
}

/// Détecte un clic sur le magasin de la station : un **bouton « pilule »**
/// de l'onglet actif (achat d'arme, sélection/déblocage de mode, extension,
/// ravitaillement à la quantité du curseur, TOUT REMPLIR) ou le bouton
/// CLOSE. Les onglets et les pistes des curseurs ne sont PAS traités ici :
/// ils le sont par `shop_update` (état mutable : bascule d'onglet, début de
/// glisser).
pub fn shop_box_click(state: &GameState) -> ShopClick {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return ShopClick::None;
    }
    let l = shop_box_layout(state);
    let m = mouse_to_game();
    // onglets : la bascule d'onglet est traitée par `shop_update`
    if l.tabs.iter().any(|t| t.contains(m)) {
        return ShopClick::None;
    }
    // pistes des curseurs : le clic glisse la quantité (`shop_update`)
    if (l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m))
        || l.slider_ammo.iter().any(|t| t.w > 0.0 && t.contains(m))
    {
        return ShopClick::None;
    }
    // boutons d'action de l'onglet actif (un seul onglet affiché à la fois :
    // les rectangles des autres onglets sont vides)
    match state.shop_tab {
        crate::config::SHOP_TAB_WEAPONS => {
            for (i, r) in l.buy_weapon.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Weapon(i);
                }
            }
            if l.buy_radar.w > 0.0 && l.buy_radar.contains(m) {
                return ShopClick::BuyRadar;
            }
        }
        crate::config::SHOP_TAB_WORKSHOP => {
            if l.buy_fuel_upgrade.contains(m) {
                return ShopClick::BuyFuelUpgrade;
            }
            if l.buy_ammo_upgrade.contains(m) {
                return ShopClick::BuyAmmoUpgrade;
            }
            if l.buy_cargo_upgrade.contains(m) {
                return ShopClick::BuyCargoUpgrade;
            }
        }
        crate::config::SHOP_TAB_MODES => {
            for (i, r) in l.buy_mode.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Mode(MOVING_MODE_ORDER[i]);
                }
            }
        }
        _ => {
            if l.buy_fuel.w > 0.0 && l.buy_fuel.contains(m) {
                return ShopClick::Refuel;
            }
            for (i, r) in l.buy_ammo.iter().enumerate() {
                if r.w > 0.0 && r.contains(m) {
                    return ShopClick::Rearm(i);
                }
            }
            if l.refill_all.w > 0.0 && l.refill_all.contains(m) {
                return ShopClick::RefillAll;
            }
        }
    }
    if l.close.contains(m) {
        ShopClick::Close
    } else {
        ShopClick::None
    }
}

/// Met à jour le magasin de la station à chaque frame : bascule d'onglet
/// (un clic sur un onglet change l'onglet actif et efface le retour
/// d'action), curseurs du ravitaillement (pression sur une piste = début de
/// glisser - la quantité saute au pointeur ; glisser bouton maintenu ;
/// molette = ± un paquet de la ressource) et bornage des quantités au
/// manque des réservoirs et aux crédits disponibles
/// (`scenario::clamp_shop_quantities`). Appelé avant `shop_box_click`.
pub fn shop_update(state: &mut GameState) {
    let l = shop_box_layout(state);
    let m = mouse_to_game();
    // bascule d'onglet : une pression sur un onglet change l'onglet actif
    // (et efface le retour d'action de l'onglet précédent)
    if is_mouse_button_pressed(MouseButton::Left) {
        for (i, tab) in l.tabs.iter().enumerate() {
            if tab.contains(m) && state.shop_tab as usize != i {
                state.shop_tab = i as u8;
                state.shop_feedback.clear();
                break;
            }
        }
    }
    // début de glisser : une pression sur une piste saisit le curseur
    if is_mouse_button_pressed(MouseButton::Left) {
        if l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m) {
            state.shop_drag = Some(0);
        } else {
            for (i, track) in l.slider_ammo.iter().enumerate() {
                if track.w > 0.0 && track.contains(m) && scenario::weapon_owned(state, i) {
                    state.shop_drag = Some(1 + i);
                    break;
                }
            }
        }
    }
    // glisser : la valeur suit le pointeur tant que le bouton est maintenu
    if let Some(target) = state.shop_drag {
        if is_mouse_button_down(MouseButton::Left) {
            if target == 0 {
                let track = l.slider_fuel;
                if track.w > 0.0 {
                    let missing = (scenario::fuel_capacity(state) - state.resources.fuel).max(0.0);
                    let frac = ((m.x - track.x) / track.w).clamp(0.0, 1.0) as f64;
                    state.shop_fuel_qty = frac * missing;
                }
            } else if let Some(&track) = l.slider_ammo.get(target - 1) {
                if track.w > 0.0 {
                    let missing =
                        (scenario::ammo_capacity(state) - state.resources.weapon_ammo[target - 1])
                            .max(0) as f64;
                    let frac = ((m.x - track.x) / track.w).clamp(0.0, 1.0) as f64;
                    state.shop_ammo_qty[target - 1] = frac * missing;
                }
            }
        } else {
            state.shop_drag = None; // bouton relâché
        }
    }
    // molette sur une piste : ± un paquet de la ressource (10 carburant,
    // le paquet de l'arme pour les munitions)
    let wheel = mouse_wheel().1;
    if wheel != 0.0 {
        if l.slider_fuel.w > 0.0 && l.slider_fuel.contains(m) {
            let step = crate::scenario::scenario(state.scenario).fuel_step;
            state.shop_fuel_qty += wheel as f64 * step;
        } else {
            for (i, track) in l.slider_ammo.iter().enumerate() {
                if track.w > 0.0 && track.contains(m) && scenario::weapon_owned(state, i) {
                    let step = scenario::weapon_spec(i).ammo_pack as f64;
                    state.shop_ammo_qty[i] += wheel as f64 * step;
                    break;
                }
            }
        }
    }
    scenario::clamp_shop_quantities(state);
}

/// Achète une arme du catalogue au magasin (bouton MARCHÉ de la boîte DOCK
/// STATION) puis persiste la progression (crédits, armes possédées). Le
/// mesh de l'arme achetée apparaît sur le vaisseau : reconstruction avec la
/// nouvelle composition (`vaisseau::rebuild_player_vaisseau`). Le résultat
/// (achat / refus) s'affiche dans le pied de la fenêtre (`shop_feedback`).
pub fn buy_weapon_and_save(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    i: usize,
) {
    match scenario::buy_weapon(state, i) {
        scenario::WeaponOutcome::Purchased(cost) => {
            crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
            state.shop_feedback = format!("Arme achetée (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::WeaponOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::WeaponOutcome::Owned => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Achète le **radar de bord** au magasin (bouton MARCHÉ de la boîte DOCK
/// STATION, onglet ÉQUIPEMENT) puis persiste la progression (crédits, radar
/// possédé) : la minimap globale (positions des météores) s'affiche dès
/// l'achat (`scenario::has_radar`). Le résultat (achat / refus) s'affiche
/// dans le pied de la fenêtre (`shop_feedback`).
pub fn buy_radar_and_save(state: &mut GameState) {
    match scenario::buy_radar(state) {
        scenario::RadarOutcome::Purchased(cost) => {
            state.shop_feedback = format!("Radar installé (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::RadarOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::RadarOutcome::Owned => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Achète une extension du magasin (réservoir, chargeur ou soute) puis persiste
/// la progression (crédits, niveaux d'extension) - les réservoirs montent à
/// la nouvelle capacité et la soute s'agrandit dans `buy_upgrade`. Un plan du
/// vaisseau lié à la ligne achetée peut apparaître : le mesh est reconstruit
/// avec la nouvelle composition (`vaisseau::rebuild_player_vaisseau`). Le
/// résultat (achat / refus) s'affiche dans le pied de la fenêtre.
pub fn buy_upgrade_and_save(
    state: &mut GameState,
    shapes: &mut [Shape],
    triangles: &mut [Triangle],
    track: scenario::UpgradeTrackId,
) {
    match scenario::buy_upgrade(state, track) {
        scenario::UpgradeOutcome::Purchased(cost) => {
            crate::vaisseau::rebuild_player_vaisseau(state, shapes, triangles);
            state.shop_feedback = format!("Extension achetée (-{} CR)", cost);
            state.shop_feedback_ok = true;
        }
        scenario::UpgradeOutcome::Insufficient(_) => {
            state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
            state.shop_feedback_ok = false;
        }
        scenario::UpgradeOutcome::Maxed => state.shop_feedback.clear(),
    }
    let _ = scenario::save_progression(state);
}

/// Sélectionne un mode de déplacement dans le magasin (bouton MARCHÉ de la
/// boîte DOCK STATION) : la sélection passe par le scénario (un mode
/// verrouillé est payé en crédits, refusé si insuffisant - messages HUD) ;
/// le mode devenu courant est annoncé au HUD, et le mode + la progression
/// (crédits, modes débloqués) sont persistés immédiatement. Le résultat
/// s'affiche dans le pied de la fenêtre (`shop_feedback`).
pub fn select_mode_and_save(state: &mut GameState, mode: i32) {
    if scenario::try_select_mode(state, mode) {
        state.send_message(&format!("MOVING MODE: {}", crate::marketplace::mode_label(mode)));
        let _ = persist::save_moving_mode(state.moving_mode);
        let _ = scenario::save_progression(state);
        state.shop_feedback = format!("Mode de vol : {}", crate::marketplace::mode_label(mode));
        state.shop_feedback_ok = true;
    } else {
        state.shop_feedback = "PAS ASSEZ DE CRÉDITS".to_string();
        state.shop_feedback_ok = false;
    }
}
