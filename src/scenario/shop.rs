//! Le magasin de la station (bouton SHOP de la boîte DOCK STATION, scénarios
//! à économie - extrait de `scenario.rs`) : carburant et munitions payants,
//! armes du catalogue (achat et munitions par arme), radar de bord,
//! ravitaillement **à la quantité** (curseur du magasin), déchargement des
//! minerais en crédits et déblocage des modes de déplacement.
//!
//! Fonctions pures testables sans macroquad : l'état vient de `GameState`,
//! les règles et prix de `Scenario` et de `marketplace.rs` (via `super::*`).
//! Les remises de rang (`super::ranks`) s'appliquent à tous les coûts.

use super::*;

// ─── Carburant et munitions ─────────────────────────────────────────────────

/// Carburant disponible ? (toujours `true` en jeu libre.) Bloque la poussée
/// quand le réservoir est vide - les rotations restent libres.
pub fn fuel_available(state: &GameState) -> bool {
    !has_economy(state) || state.resources.fuel > 0.0
}

/// Consomme le carburant du scénario quand le moteur est allumé (flamme avant
/// ou arrière : compteurs `thrusted`/`revert_thrusted` non nuls), `dt` en
/// secondes. Annonce « OUT OF FUEL » quand le réservoir se vide.
pub fn consume_fuel(state: &mut GameState, dt: f64) {
    if !has_economy(state) {
        return;
    }
    if state.player.thrusted == 0 && state.player.revert_thrusted == 0 {
        return;
    }
    let before = state.resources.fuel;
    let after = (before - scenario(state.scenario).fuel_per_second * dt).max(0.0);
    state.resources.fuel = after;
    if before > 0.0 && after == 0.0 {
        state.send_message("OUT OF FUEL");
    }
}

/// Consomme des munitions pour un tir (scénarios à économie) et renvoie le
/// **masque des armes qui ont tiré** (index de `VAISSEAU_WEAPONS`, borné à
/// `WEAPON_SLOTS`) : chaque arme possédée dont le stock couvre `ammo_per_shot`
/// tire (ses munitions sont consommées) ; une arme à court de munitions ne
/// tire pas, les autres continuent. Aucune arme ne peut tirer (toutes les
/// munitions épuisées) → masque tout faux, le tir est bloqué (cooldown non
/// réinitialisé - le tir part dès qu'une arme a des munitions ; aucun message
/// répété). Hors économie : toutes les armes possédées tirent, sans
/// consommation. Annonce « OUT OF AMMO » quand le dernier stock se vide.
pub fn try_fire(state: &mut GameState) -> [bool; WEAPON_SLOTS] {
    let s = scenario(state.scenario);
    let mut fired = [false; WEAPON_SLOTS];
    if !s.has_economy {
        // jeu libre / Survival : toutes les armes **possédées** tirent, sans
        // consommation - en jeu libre, seule l'arme 1 équipe le vaisseau
        // (masque de `weapon_owned`)
        for (i, slot) in fired.iter_mut().enumerate().take(weapon_slot_count()) {
            *slot = weapon_owned(state, i);
        }
        return fired;
    }
    let total_before = total_ammo(state);
    for (i, slot) in fired.iter_mut().enumerate().take(weapon_slot_count()) {
        if weapon_owned(state, i) && state.resources.weapon_ammo[i] >= s.ammo_per_shot {
            state.resources.weapon_ammo[i] -= s.ammo_per_shot;
            *slot = true;
        }
    }
    if total_before > 0 && total_ammo(state) == 0 {
        state.send_message("OUT OF AMMO");
    }
    fired
}

// ─── Armes du catalogue (achat et munitions par arme) ──────────────────────

/// Données économiques d'une arme du catalogue (index dans `VAISSEAU_WEAPONS`) :
/// nom, coût d'achat au magasin, prix et taille du paquet de munitions du
/// ravitaillement (ligne AMMO du magasin). **Catalogue vide = un seul
/// « canon classique »** (repli :
/// coût 0, toujours équipé, paquets aux valeurs globales `AMMO_PRICE` /
/// `AMMO_STEP`) - le comportement historique est préservé.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSpec {
    /// Nom de l'arme (magasin, messages HUD).
    pub name: &'static str,
    /// Coût d.achat en crédits (0 = arme de base, équipée au départ).
    pub cost: i32,
    /// Prix (crédits) d.un paquet de munitions.
    pub ammo_price: i32,
    /// Taille d'un paquet (munitions par paquet).
    pub ammo_pack: i32,
}

/// Spécification économique de l'arme `i` du catalogue (hors catalogue →
/// canon classique de repli). Pure (tests).
pub fn weapon_spec(i: usize) -> WeaponSpec {
    match crate::marketplace::VAISSEAU_WEAPONS.get(i) {
        Some(w) => WeaponSpec {
            name: w.name,
            cost: w.cost,
            ammo_price: w.ammo_price,
            ammo_pack: w.ammo_pack,
        },
        None => WeaponSpec {
            name: "CANON CLASSIQUE",
            cost: 0,
            ammo_price: AMMO_PRICE,
            ammo_pack: AMMO_STEP,
        },
    }
}

/// Nombre d'emplacements d'armes actifs : le nombre d'armes du catalogue
/// (`VAISSEAU_WEAPONS`), borné à `WEAPON_SLOTS` - **au moins 1** (le canon
/// classique de repli quand le catalogue est vide). Pure (tests).
pub fn weapon_slot_count() -> usize {
    crate::marketplace::VAISSEAU_WEAPONS.len().clamp(1, WEAPON_SLOTS)
}

/// L'arme `i` est-elle **possédée** (équipée) ? Hors économie : en **jeu
/// libre**, seule l'arme 1 (index 0, `ARME 1`) équipe le vaisseau - les
/// autres armes du catalogue ne sont ni construites sur le vaisseau ni
/// tirées ; en Survival (et custom sans économie), toutes les armes du
/// catalogue. En économie : achetée au magasin (`weapon_owned`), ou coût 0
/// (arme de base - comme les modes de déplacement gratuits). Le canon
/// classique (hors catalogue) est toujours possédé. Pure (tests).
pub fn weapon_owned(state: &GameState, i: usize) -> bool {
    match state.scenario {
        // jeu libre : le vaisseau n'est équipé que de l'arme 1
        ScenarioId::FreePlay => i == 0,
        _ if !has_economy(state) => i < weapon_slot_count(),
        _ => {
            state.resources.weapon_owned.get(i).copied().unwrap_or(false)
                || weapon_spec(i).cost == 0
        }
    }
}

/// Tarifs d'achat d'une arme pas encore possédée : tarif de base (prix
/// d'origine) et prix réellement payé (remise de réputation du rang courant
/// appliquée) - `None` = déjà possédée, coût nul ou pas d'économie. Comme
/// `mode_unlock_prices` : affichés dans le magasin de la station.
pub fn weapon_prices(state: &GameState, i: usize) -> Option<(i32, i32)> {
    if !has_economy(state) || weapon_owned(state, i) {
        return None;
    }
    let cost = weapon_spec(i).cost;
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits d.une arme pas encore possédée (`None` = déjà possédée,
/// coût nul ou pas d'économie) - le prix réellement payé (remisé).
pub fn weapon_cost(state: &GameState, i: usize) -> Option<i32> {
    weapon_prices(state, i).map(|(_, discounted)| discounted)
}

/// Résultat d'un achat d'arme au magasin (`buy_weapon`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponOutcome {
    /// Arme déjà possédée (ou pas d'économie).
    Owned,
    /// Arme achetée (coût en crédits déduit, équipée - livrée chargée).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète une arme du catalogue au magasin de la station : paie en crédits
/// (remise de réputation appliquée), l'équipe - son mesh apparaît sur le
/// vaisseau (`vaisseau::rebuild_player_vaisseau` côté jeu) - et la livre
/// **chargée** à la capacité courante. Hors scénario à économie : sans effet
/// (`Owned`). Appelé par le magasin (bouton SHOP de la boîte DOCK STATION).
pub fn buy_weapon(state: &mut GameState, i: usize) -> WeaponOutcome {
    if !has_economy(state) || weapon_owned(state, i) {
        return WeaponOutcome::Owned;
    }
    let Some(cost) = weapon_cost(state, i) else {
        return WeaponOutcome::Owned; // coût 0 → arme de base, déjà équipée
    };
    if state.resources.credits < cost {
        state.send_message(&format!(
            "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
            weapon_spec(i).name,
            cost
        ));
        return WeaponOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    if i < WEAPON_SLOTS {
        state.resources.weapon_owned[i] = true;
        state.resources.weapon_ammo[i] = ammo_capacity(state); // livrée chargée
    }
    state.send_message(&format!(
        "WEAPON {} PURCHASED: -{} CREDITS",
        weapon_spec(i).name,
        cost
    ));
    WeaponOutcome::Purchased(cost)
}

// ─── Radar de bord (minimap globale) ────────────────────────────────────────

/// Coût en crédits du **radar de bord** (`RADAR_COST` de `src/marketplace.rs`) :
/// acheté au magasin (onglet ÉQUIPEMENT) en scénario à économie ; hors
/// économie le radar est toujours allumé (gratuit, historique).
pub fn radar_price(state: &GameState) -> Option<(i32, i32)> {
    if !has_economy(state) || state.resources.radar_owned {
        return None;
    }
    let cost = crate::marketplace::RADAR_COST;
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits du radar non encore possédé (`None` = déjà possédé, hors
/// économie ou coût nul) - le prix réellement payé (remisé).
pub fn radar_cost(state: &GameState) -> Option<i32> {
    radar_price(state).map(|(_, discounted)| discounted)
}

/// Résultat d'un achat du radar au magasin (`buy_radar`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarOutcome {
    /// Radar déjà possédé (ou pas d'économie).
    Owned,
    /// Radar acheté (coût en crédits déduit, minimap activée).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Achète le **radar de bord** au magasin de la station : paie en crédits
/// (remise de réputation appliquée) et active la minimap globale (points des
/// météores et des autres formes, `scenario::has_radar`). Hors scénario à
/// économie : sans effet (`Owned` - le radar y est déjà allumé). Appelé par
/// le magasin (bouton SHOP de la boîte DOCK STATION, onglet ÉQUIPEMENT).
pub fn buy_radar(state: &mut GameState) -> RadarOutcome {
    if !has_economy(state) || state.resources.radar_owned {
        return RadarOutcome::Owned;
    }
    let Some(cost) = radar_cost(state) else {
        return RadarOutcome::Owned; // coût 0 → déjà actif
    };
    if state.resources.credits < cost {
        state.send_message(&format!("NOT ENOUGH CREDITS FOR RADAR ({} NEEDED)", cost));
        return RadarOutcome::Insufficient(cost);
    }
    state.resources.credits -= cost;
    state.resources.radar_owned = true;
    state.send_message(&format!("RADAR PURCHASED: -{} CREDITS", cost));
    RadarOutcome::Purchased(cost)
}

/// Total des munitions restantes des armes **possédées** (toutes armes
/// confondues) - affiché au HUD (`AMMO:x/y`) et sur la télécommande.
/// Pure (tests).
pub fn total_ammo(state: &GameState) -> i32 {
    (0..weapon_slot_count())
        .filter(|&i| weapon_owned(state, i))
        .map(|i| state.resources.weapon_ammo[i])
        .sum()
}

/// Capacité totale des chargeurs des armes **possédées** (somme des
/// capacités courantes, extensions de chargeur comprises). Pure (tests).
pub fn total_ammo_capacity(state: &GameState) -> i32 {
    let cap = ammo_capacity(state);
    (0..weapon_slot_count())
        .filter(|&i| weapon_owned(state, i))
        .map(|_| cap)
        .sum()
}



// ─── Minerais et ravitaillement ─────────────────────────────────────────────

/// Décharge la soute à la station : chaque minerai est converti en crédits
/// selon la valeur de son élément (`ELEMENT_VALUES`) et rapporte de la
/// **réputation** (`reputation_per_mineral` - le commerce est récompensé,
/// comme le tir l'est par les astéroïdes détruits). Appelé par `docking`
/// (déchargement automatique de l'original, au plus tard à la frame suivant
/// la fermeture de la boîte) et par le bouton UNLOAD de la boîte DOCK STATION
/// (déchargement immédiat - les crédits financent le ravitaillement
/// carburant/munitions acheté au magasin du même accostage).
pub fn unload_cargo(state: &mut GameState, elements: &[Element]) {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return;
    }
    // NB : `has_economy` contrôle aussi le score - hors économie (jeu libre,
    // Survival), le score ne compte que les astéroïdes et les objectifs.
    let mut gained = 0;
    for (i, e) in elements.iter().enumerate() {
        if let Some(&value) = ELEMENT_VALUES.get(i) {
            gained += e.count * value;
        }
    }
    state.resources.credits += gained;
    // crédits gagnés cumulés (score composite - voir `composite_score`) : le
    // commerce enrichit le score, pas seulement le solde
    state.credits_earned += gained;
    // le record (high-score) suit : relevé et persisté si le score composite
    // vient de dépasser l'ancien (`maybe_update_high_score`)
    maybe_update_high_score(state);
    if gained > 0 {
        state.send_message(&format!("CARGO UNLOADED: +{} CREDITS", gained));
        // réputation gagnée par minerai déchargé - un palier franchi est
        // annoncé comme pour les astéroïdes détruits
        let before = rank_at(s.ranks, state.resources.reputation);
        state.resources.reputation += gained as f64 * s.reputation_per_mineral;
        let after = rank_at(s.ranks, state.resources.reputation);
        if let (Some(after), Some(before)) = (after, before) {
            if after != before {
                state.send_message(&format!("RANK UP: {}", after.name));
            }
        }
    }
}

/// Résultat d'un ravitaillement à la station (`buy_fuel_qty` /
/// `buy_ammo_qty`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyOutcome {
    /// Réservoir(s) déjà plein(s) (rien à payer).
    Full,
    /// Ravitaillement payé (coût en crédits déduit).
    Purchased(i32),
    /// Pas assez de crédits (coût nécessaire).
    Insufficient(i32),
}

/// Coût (crédits, remise de réputation appliquée) d'un **plein de
/// carburant** : le manque au réservoir courant est facturé au pas du
/// scénario (`fuel_price` par `fuel_step` unités, arrondi au pas supérieur).
/// Hors économie ou réservoir plein : 0. Équivalent à `fuel_qty_cost` sur
/// tout le manque - le plein complet (extrémité haute du curseur FUEL).
/// Réservé aux tests (le magasin achète à la quantité du curseur).
#[cfg(test)]
pub fn fuel_refill_cost(state: &GameState) -> i32 {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return 0;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let raw = (missing / s.fuel_step).ceil() as i32 * s.fuel_price;
    discounted_cost(raw, current_discount(state))
}

/// Coût (crédits, remise de réputation appliquée) du **rechargement des
/// munitions** : chaque arme possédée est facturée au paquet de l'arme
/// (`ammo_price` par paquet de `ammo_pack` munitions, arrondi au paquet
/// supérieur) - les armes non possédées ne se rechargent pas. Hors économie
/// ou toutes les armes pleines : 0. Réservé aux tests (le magasin achète à
/// la quantité des curseurs AMMO, un par arme possédée).
#[cfg(test)]
pub fn ammo_refill_cost(state: &GameState) -> i32 {
    if !has_economy(state) {
        return 0;
    }
    let max_ammo = ammo_capacity(state);
    let mut raw = 0;
    for i in 0..weapon_slot_count() {
        if !weapon_owned(state, i) {
            continue;
        }
        let spec = weapon_spec(i);
        let missing = (max_ammo - state.resources.weapon_ammo[i]).max(0);
        raw += ((missing + spec.ammo_pack - 1) / spec.ammo_pack) * spec.ammo_price;
    }
    discounted_cost(raw, current_discount(state))
}

/// Nombre de **paquets facturés** pour `qty` unités de carburant au magasin
/// (arrondi au paquet supérieur - tout achat paie au moins un paquet) ; 0 si
/// la quantité est nulle ou hors économie. Affiche la ligne FUEL (« +30
/// (3 paquets) »). Pure (tests).
pub fn fuel_pack_count(state: &GameState, qty: f64) -> i32 {
    let s = scenario(state.scenario);
    if !s.has_economy || qty <= 0.0 {
        return 0;
    }
    (qty / s.fuel_step).ceil() as i32
}

/// Coût (crédits, remise de réputation appliquée) de l'achat de `qty`
/// **unités** de carburant au magasin (ligne FUEL, curseur) : facturées au
/// paquet du scénario (`fuel_price` par `fuel_step` - voir `fuel_pack_count`),
/// puis remise appliquée. `qty <= 0` ou hors économie : 0. Pure (tests).
pub fn fuel_qty_cost(state: &GameState, qty: f64) -> i32 {
    discounted_cost(
        fuel_pack_count(state, qty) * scenario(state.scenario).fuel_price,
        current_discount(state),
    )
}

/// Nombre de **paquets facturés** pour `qty` munitions de l'arme `i` au
/// magasin (paquet de l'arme, arrondi au supérieur) ; 0 si la quantité est
/// nulle ou hors économie. Affiche la ligne AMMO de l'arme. Pure (tests).
pub fn ammo_pack_count(state: &GameState, i: usize, qty: i32) -> i32 {
    if !has_economy(state) || qty <= 0 {
        return 0;
    }
    let spec = weapon_spec(i);
    (qty + spec.ammo_pack - 1) / spec.ammo_pack
}

/// Coût (crédits, remise de réputation appliquée) de l'achat de `qty`
/// **unités** de munitions pour l'arme `i` (ligne AMMO de l'arme, curseur) :
/// facturées au paquet de l'arme (`ammo_price` par paquet de `ammo_pack` -
/// voir `ammo_pack_count`), puis remise appliquée. `qty <= 0` ou hors
/// économie : 0. Pure (tests).
pub fn ammo_qty_cost(state: &GameState, i: usize, qty: i32) -> i32 {
    discounted_cost(
        ammo_pack_count(state, i, qty) * weapon_spec(i).ammo_price,
        current_discount(state),
    )
}

/// Achète un **plein de carburant** à la station : remplit le réservoir à la
/// capacité courante et déduit les crédits (voir `buy_fuel_qty`). Équivaut
/// au curseur FUEL du magasin à son maximum. Réservé aux tests (le magasin
/// achète à la quantité du curseur).
#[cfg(test)]
pub fn purchase_fuel(state: &mut GameState) -> SupplyOutcome {
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    buy_fuel_qty(state, missing)
}

/// Achète `qty` unités de carburant à la station (ligne FUEL du magasin,
/// curseur) : facturées au paquet (`fuel_qty_cost` - un paquet minimum pour
/// tout achat) ; le réservoir reçoit exactement `qty` unités, bornées au
/// manque de la capacité courante. Minerais insuffisants → `Insufficient`
/// (message « NOT ENOUGH CREDITS FOR FUEL », non répété au même coût).
pub fn buy_fuel_qty(state: &mut GameState, qty: f64) -> SupplyOutcome {
    if !has_economy(state) {
        return SupplyOutcome::Full;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let qty = qty.clamp(0.0, missing);
    if qty <= 0.0 {
        return SupplyOutcome::Full;
    }
    let cost = fuel_qty_cost(state, qty);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        // le message n'est envoyé qu'au début du manque (pas à chaque clic
        // répété - `supplies_shortage_cost`)
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR FUEL ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    state.resources.fuel = (state.resources.fuel + qty).min(fuel_capacity(state));
    state.send_message(&format!("FUEL PURCHASED: -{} CREDITS", cost));
    SupplyOutcome::Purchased(cost)
}

/// Achète le **rechargement des munitions** à la station : chaque arme
/// possédée repart pleine à la capacité courante (`ammo_refill_cost`, par
/// paquet de l.arme) et les crédits sont déduits. Les munitions s'achètent
/// **indépendamment** du carburant. Réservé aux tests (le magasin achète à
/// la quantité des curseurs AMMO, un par arme possédée).
#[cfg(test)]
pub fn purchase_ammo(state: &mut GameState) -> SupplyOutcome {
    if !has_economy(state) {
        return SupplyOutcome::Full;
    }
    let cost = ammo_refill_cost(state);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR AMMO ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    let max_ammo = ammo_capacity(state);
    for i in 0..weapon_slot_count() {
        if weapon_owned(state, i) {
            state.resources.weapon_ammo[i] = max_ammo;
        }
    }
    state.send_message(&format!("AMMO PURCHASED: -{} CREDITS", cost));
    SupplyOutcome::Purchased(cost)
}

/// Achète `qty` unités de munitions pour l'arme `i` (ligne AMMO de l'arme,
/// curseur) : facturées au paquet de l'arme (`ammo_qty_cost` - un paquet
/// minimum pour tout achat) ; le chargeur reçoit exactement `qty` unités,
/// bornées au manque de la capacité courante. Arme non possédée ou quantité
/// nulle : sans effet (`Full`). Minerais insuffisants → `Insufficient`
/// (message « NOT ENOUGH CREDITS FOR AMMO », non répété au même coût).
pub fn buy_ammo_qty(state: &mut GameState, i: usize, qty: i32) -> SupplyOutcome {
    if !has_economy(state) || !weapon_owned(state, i) {
        return SupplyOutcome::Full;
    }
    let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0);
    let qty = qty.clamp(0, missing);
    if qty <= 0 {
        return SupplyOutcome::Full;
    }
    let cost = ammo_qty_cost(state, i, qty);
    if cost == 0 {
        return SupplyOutcome::Full;
    }
    if state.resources.credits < cost {
        if state.supplies_shortage_cost != cost {
            state.supplies_shortage_cost = cost;
            state.send_message(&format!("NOT ENOUGH CREDITS FOR AMMO ({} NEEDED)", cost));
        }
        return SupplyOutcome::Insufficient(cost);
    }
    state.supplies_shortage_cost = 0;
    state.resources.credits -= cost;
    state.resources.weapon_ammo[i] += qty;
    state.send_message(&format!(
        "{} AMMO PURCHASED: -{} CREDITS",
        weapon_spec(i).name,
        cost
    ));
    SupplyOutcome::Purchased(cost)
}

/// Quantité maximale de carburant **achetable** avec les crédits courants :
/// le plus grand multiple du pas (`fuel_step`) dont le coût (remisé) ne
/// dépasse pas les crédits, borné au manque du réservoir - 0 si même un
/// paquet est hors de portée (ou hors économie). Positionne le curseur FUEL
/// du magasin à l'ouverture. Pure (tests).
pub fn affordable_fuel_qty(state: &GameState) -> f64 {
    let s = scenario(state.scenario);
    if !s.has_economy {
        return 0.0;
    }
    let missing = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    let max_packs = (missing / s.fuel_step).ceil() as i32;
    for n in (1..=max_packs).rev() {
        if discounted_cost(n * s.fuel_price, current_discount(state)) <= state.resources.credits {
            return (n as f64 * s.fuel_step).min(missing);
        }
    }
    0.0
}

/// Quantité maximale de munitions **achetable** pour l'arme `i` avec les
/// crédits courants : le plus grand multiple du paquet de l.arme dont le
/// coût (remisé) ne dépasse pas les crédits, borné au manque du chargeur -
/// 0 si même un paquet est hors de portée (ou hors économie). Positionne le
/// curseur AMMO de l'arme à l'ouverture du magasin. Pure (tests).
pub fn affordable_ammo_qty(state: &GameState, i: usize) -> i32 {
    if !has_economy(state) {
        return 0;
    }
    let spec = weapon_spec(i);
    let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0);
    let max_packs = (missing + spec.ammo_pack - 1) / spec.ammo_pack;
    for n in (1..=max_packs).rev() {
        if discounted_cost(n * spec.ammo_price, current_discount(state)) <= state.resources.credits {
            return (n * spec.ammo_pack).min(missing);
        }
    }
    0
}

/// Borne les quantités des curseurs du magasin (carburant et munitions par
/// arme possédée) à ce que les crédits permettent (`affordable_fuel_qty` /
/// Aimanate une quantité de curseur au **multiple du paquet** le plus proche
/// (pour ne jamais payer un paquet sans en prendre les unités) - sauf le
/// **maximum** (`max`, le plein du réservoir), qui reste atteignable même
/// s'il ne tombe pas pile sur un multiple : le dernier paquet est alors pris
/// en entier (aucune unité perdue). `qty` est arrondi au multiple le plus
/// proche et borné à `max` (0 si le paquet ou le maximum est nul). Pure
/// (tests).
pub fn snap_to_pack(qty: f64, pack: f64, max: f64) -> f64 {
    if pack <= 0.0 || max <= 0.0 {
        return 0.0;
    }
    let qty = qty.clamp(0.0, max);
    // le maximum (plein du réservoir) reste une position valide : au-delà,
    // le dernier paquet payé est pris en entier
    if qty >= max {
        return max;
    }
    (qty / pack).round() * pack
}

/// Borne les quantités des curseurs du magasin (carburant et munitions par
/// arme possédée) à ce que les crédits permettent (`affordable_fuel_qty` /
/// `affordable_ammo_qty` - déjà bornées au manque des réservoirs) : jamais
/// une quantité dont le coût dépasserait les crédits disponibles - on ne
/// peut pas se retrouver avec un curseur hors de portée. Les quantités sont
/// aussi **aimantées aux multiples du paquet** (`snap_to_pack`) pour ne
/// jamais payer un paquet sans en prendre les unités en glissant à la
/// souris. Appelé à chaque frame par le magasin (`game.rs`). Pur (tests).
pub fn clamp_shop_quantities(state: &mut GameState) {
    if !has_economy(state) {
        state.shop_fuel_qty = 0.0;
        state.shop_ammo_qty = [0.0; WEAPON_SLOTS];
        return;
    }
    let missing_fuel = (fuel_capacity(state) - state.resources.fuel).max(0.0);
    state.shop_fuel_qty = snap_to_pack(
        state.shop_fuel_qty,
        scenario(state.scenario).fuel_step,
        missing_fuel,
    );
    state.shop_fuel_qty = state.shop_fuel_qty.clamp(0.0, affordable_fuel_qty(state));
    for i in 0..weapon_slot_count() {
        if weapon_owned(state, i) {
            let missing = (ammo_capacity(state) - state.resources.weapon_ammo[i]).max(0) as f64;
            state.shop_ammo_qty[i] = snap_to_pack(
                state.shop_ammo_qty[i],
                weapon_spec(i).ammo_pack as f64,
                missing,
            );
            state.shop_ammo_qty[i] = state.shop_ammo_qty[i].clamp(
                0.0,
                affordable_ammo_qty(state, i) as f64,
            );
        }
    }
}

// ─── Modes de déplacement ───────────────────────────────────────────────────

/// Coûts de déblocage d'un mode pas encore débloqué : tarif de base (prix
/// d'origine) et prix réellement payé (remise de réputation du rang courant
/// appliquée) - `None` = débloqué, ou pas d'économie. Affichés dans le
/// magasin de la station (bouton SHOP de la boîte DOCK STATION).
pub fn mode_unlock_prices(state: &GameState, mode: i32) -> Option<(i32, i32)> {
    if !has_economy(state) {
        return None;
    }
    let m = mode as usize;
    if m >= state.unlocked_modes.len() || state.unlocked_modes[m] {
        return None;
    }
    let cost = scenario(state.scenario).mode_costs[m];
    (cost > 0).then(|| (cost, discounted_cost(cost, current_discount(state))))
}

/// Coût en crédits d.un mode pas encore débloqué (`None` = débloqué, ou pas
/// d'économie) - affiché dans le magasin de la station (bouton SHOP de la
/// boîte DOCK STATION). C'est le prix réellement payé (remise de réputation
/// du rang courant appliquée) ; voir `mode_unlock_prices` pour le tarif de
/// base.
pub fn locked_cost(state: &GameState, mode: i32) -> Option<i32> {
    mode_unlock_prices(state, mode).map(|(_, discounted)| discounted)
}

/// Sélectionne un mode de déplacement dans le magasin de la station :
/// débloqué → appliqué immédiatement ; verrouillé → payé en crédits (si
/// possible, sinon message « NOT ENOUGH CREDITS ») puis appliqué. Renvoie
/// `true` si le mode demandé est devenu le mode courant.
pub fn try_select_mode(state: &mut GameState, mode: i32) -> bool {
    match locked_cost(state, mode) {
        None => {
            state.moving_mode = mode;
            true
        }
        Some(cost) => {
            if state.resources.credits >= cost {
                state.resources.credits -= cost;
                state.unlocked_modes[mode as usize] = true;
                state.moving_mode = mode;
                state.send_message(&format!(
                    "MODE {} UNLOCKED ({} CREDITS)",
                    mode_label(mode),
                    cost
                ));
                true
            } else {
                state.send_message(&format!(
                    "NOT ENOUGH CREDITS FOR {} ({} NEEDED)",
                    mode_label(mode),
                    cost
                ));
                false
            }
        }
    }
}
