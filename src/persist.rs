//! Persistance des réglages dans un fichier de configuration.
//!
//! Format texte simple `clé=valeur`, une par ligne, dans
//! `meteors-mining/meteors_mining.cfg` sous le dossier de configuration
//! utilisateur (norme XDG : `$XDG_CONFIG_HOME`, ou `~/.config` à défaut).
//!
//! **Backend web (wasm32)** : `std::fs` n'a pas de système de fichiers sur
//! wasm32 - le même contenu texte est stocké dans le **localStorage** du
//! navigateur (clé `meteors-mining/config`), via deux imports JS bruts
//! (`env.mmcfg_read` / `env.mmcfg_write`, glue définie dans `web/index.html`,
//! adossés au localStorage). Même API publique, mêmes clés, même format : la
//! logique métier (réglages, progression des scénarios) ne voit aucune
//! différence entre les deux backends.
//!
//! Clés actuelles :
//! - `moving_mode` - mode de déplacement (0..3, au magasin de la station)
//! - `music` - musique en marche (0/1, touche M)
//! - `volume` - volume maître en pourcentage (0..100)
//! - `render_style` - style de rendu des triangles (0..2, écran O)
//! - `window_size` - index dans `WINDOW_SIZES` (0..3, écran O) - le mode
//!   d'affichage fenêtré/zoomé/natif n'est pas persisté (cycle F prévisible)
//! - `antialias` - MSAA 4× (0/1, écran O ; appliqué au lancement)
//! - `touch_ui` - interface tactile affichée (0/1, écran O)
//! - `scenario` - scénario choisi (0 = jeu libre, 1 = Progression,
//!   2 = Survival, touche N de l'écran titre)
//! - `prog_minerals` - minerais en banque (Progression)
//! - `prog_modes` - modes de déplacement débloqués (masque binaire,
//!   Progression)
//! - `prog_reputation` - réputation × 10 (entier, au dixième près,
//!   Progression)
//! - `prog_up_fuel` - extensions de réservoir achetées (Progression,
//!   atelier de la station)
//! - `prog_up_ammo` - extensions de chargeur achetées (Progression)
//! - `prog_up_cargo` - extensions de soute achetées (Progression)
//! - `prog_weapons` - armes du catalogue possédées (masque binaire,
//!   Progression - les munitions par arme repartent pleines à chaque lancement)
//! - `prog_lives` - vies restantes (Survival)
//! - `prog_shield` - bouclier restant × 10 (entier, Survival)
//! - `prog_objectives` - objectifs DAG complétés (IDs séparés par virgules,
//!   scénarios custom)
//! - `prog_meteors` / `prog_docks` / `prog_bullets_fired` /
//!   `prog_bullets_lost` / `prog_survive` - compteurs d'avancement des
//!   conditions d'objectifs (scénarios custom), restaurés au lancement
//!
//! Le fichier est lu au lancement du jeu (les valeurs enregistrées remplacent
//! les défauts) et réécrit à chaque modification d'un réglage ou de la
//! progression d'un scénario. Aucune dépendance externe - simple `std::fs`,
//! dans l'esprit « binaire autonome » du port.

use std::collections::HashMap;
// `std::fs` n'a pas de système de fichiers sur wasm32 : le backend web
// (`mod ls` + `read_all`/`write_all` wasm) n'y touche jamais.
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{MOVING_MODE_COUNT, RENDER_STYLE_COUNT, VIEW_MODE_COUNT, WINDOW_SIZES};

/// Nom du fichier de configuration (dans le dossier de configuration
/// utilisateur, sous `meteors-mining/`).
pub const CONFIG_FILE: &str = "meteors_mining.cfg";

/// Chemin du fichier de configuration : `meteors-mining/meteors_mining.cfg`
/// dans le dossier de configuration utilisateur (norme XDG - voir
/// `config_dir`). En mode test, un répertoire temporaire jetable par
/// processus : les tests (notamment les sauvegardes déclenchées par les
/// collisions de `game.rs`) ne touchent jamais au vrai fichier de config.
pub fn config_path() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir()
            .join(format!("meteors_mining_test_{}", std::process::id()))
            .join("meteors-mining")
            .join(CONFIG_FILE)
    }
    #[cfg(not(test))]
    {
        config_dir().join("meteors-mining").join(CONFIG_FILE)
    }
}

/// Dossier de configuration utilisateur (norme XDG) : `$XDG_CONFIG_HOME`
/// s'il est défini (chemin absolu), sinon `~/.config` sur Unix (variable
/// `HOME`) et `%APPDATA%` sur Windows. Repli sur le répertoire de travail
/// si aucune variable d'environnement n'est disponible. Inutilisé en mode
/// test (`config_path` renvoie alors un répertoire temporaire jetable).
#[cfg(not(test))]
fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(dir) = std::env::var("APPDATA") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home).join(".config");
        }
    }
    // dernier recours : répertoire de travail (comportement historique)
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

// ─── accès générique clé/valeur ─────────────────────────────────────────────

/// Backend localStorage (wasm uniquement).
///
/// Pas de wasm-bindgen ni de web-sys : deux fonctions importées du module
/// `env`, définies côté page dans `web/index.html` (`env.mmcfg_read` /
/// `env.mmcfg_write`, adossées au localStorage). Le chargeur `web/gl.js`
/// remplace silencieusement les imports `env` qu'il ne connaît pas par des
/// no-ops : sans la glue de la page, le jeu tourne simplement **sans
/// persistance** (comportement antérieur), jamais en erreur.
#[cfg(target_arch = "wasm32")]
mod ls {
    use std::io;

    /// Clé localStorage unique : le contenu texte complet de la configuration
    /// (même format `clé=valeur` que le fichier natif).
    pub const KEY: &str = "meteors-mining/config";

    // `wasm_import_module = "env"` : sans cet attribut, rustc émet des
    // symboles indéfinis au lieu d'imports - le lien échoue (même convention
    // que miniquad, voir `native/wasm.rs`)
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        /// Lit la valeur associée à `key` (UTF-8) : copie min(longueur, cap)
        /// octets dans `buf` et renvoie la **longueur totale** de la valeur,
        /// -1 si absente ou en erreur. Protocole en deux appels : `cap = 0`
        /// (buf nul) pour interroger la taille, puis lecture complète.
        fn mmcfg_read(key: *const u8, key_len: u32, buf: *mut u8, cap: u32) -> i32;
        /// Écrit `value` (UTF-8) pour `key` : 0 si OK, -1 en erreur (quota
        /// dépassé, navigation privée…).
        fn mmcfg_write(key: *const u8, key_len: u32, val: *const u8, val_len: u32) -> i32;
    }

    /// Lit une valeur du localStorage (absente, vide ou erreur → `None`).
    pub fn read(key: &str) -> Option<String> {
        let k = key.as_bytes();
        // 1er appel : capacité 0, renvoie la taille de la valeur (ou -1)
        let len = unsafe { mmcfg_read(k.as_ptr(), k.len() as u32, std::ptr::null_mut(), 0) };
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        // 2e appel : lecture complète (la valeur peut avoir rétréci entre les
        // deux appels - rare et sans gravité, on tronque)
        let n = unsafe { mmcfg_read(k.as_ptr(), k.len() as u32, buf.as_mut_ptr(), buf.len() as u32) };
        if n <= 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Écrit une valeur dans le localStorage (erreur I/O si l'écriture
    /// échoue).
    pub fn write(key: &str, value: &str) -> io::Result<()> {
        let k = key.as_bytes();
        let v = value.as_bytes();
        let ok = unsafe { mmcfg_write(k.as_ptr(), k.len() as u32, v.as_ptr(), v.len() as u32) };
        if ok == 0 {
            Ok(())
        } else {
            Err(io::Error::other("localStorage indisponible (glue web absente ?)"))
        }
    }
}

/// Analyse le contenu texte (`clé=valeur`, une par ligne ; ignorées : lignes
/// vides, sans `=`, et clés dupliquées après la première).
fn parse_config(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        map.entry(key.trim().to_string())
            .or_insert_with(|| value.trim().to_string());
    }
    map
}

/// Sérialise une table de configuration en contenu texte (clés triées).
fn config_content(map: &HashMap<String, String>) -> String {
    let mut lines: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>();
    lines.sort();
    lines.join("\n") + "\n"
}

/// Lit toutes les clés en une table (wasm : clé localStorage).
#[cfg(target_arch = "wasm32")]
fn read_all(_path: &Path) -> HashMap<String, String> {
    parse_config(&ls::read(ls::KEY).unwrap_or_default())
}

/// Lit toutes les clés du fichier en une table (natif ; fichier absent →
/// table vide).
#[cfg(not(target_arch = "wasm32"))]
fn read_all(path: &Path) -> HashMap<String, String> {
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_config(&content)
}

/// Écrit toutes les clés (wasm : clé localStorage unique - `setItem` est
/// atomique côté navigateur).
#[cfg(target_arch = "wasm32")]
fn write_all(_path: &Path, map: &HashMap<String, String>) -> io::Result<()> {
    ls::write(ls::KEY, &config_content(map))
}

/// Écrit toutes les clés (triées) dans le fichier de configuration ; crée le
/// dossier parent s'il n'existe pas (natif uniquement).
///
/// Écriture **atomique** : le contenu est d'abord écrit dans un fichier
/// temporaire du même dossier, puis `rename` par-dessus la cible - une
/// coupure de courant ou un crash en pleine écriture ne peut pas laisser un
/// fichier de configuration (ou de progression) tronqué : on retrouve soit
/// l'ancienne version complète, soit la nouvelle. `rename` est atomique sur
/// un même système de fichiers, et le fichier temporaire porte un nom
/// unique par processus (PID) pour que deux instances du jeu ne s'écrasent
/// pas mutuellement.
#[cfg(not(target_arch = "wasm32"))]
fn write_all(path: &Path, map: &HashMap<String, String>) -> io::Result<()> {
    let content = config_content(map);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("cfg"),
        std::process::id()
    ));
    fs::write(&tmp, &content)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        // Windows : `rename` échoue si la destination existe déjà (contrairement
        // à Unix où elle est remplacée atomiquement) - on supprime la cible
        // puis on réessaie. Le court intervalle sans fichier est acceptable
        // (repli Windows uniquement).
        Err(_) => {
            let _ = fs::remove_file(path);
            let result = fs::rename(&tmp, path);
            if result.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            result
        }
    }
}

/// Lit une clé entière (absente ou invalide → `None`).
pub fn get_i32(key: &str) -> Option<i32> {
    get_i32_from(&config_path(), key)
}

/// Lit une clé entière dans un fichier donné (version testable).
pub fn get_i32_from(path: &Path, key: &str) -> Option<i32> {
    read_all(path).get(key).and_then(|v| v.parse().ok())
}

/// Écrit une clé entière (les autres clés sont conservées).
pub fn set_i32(key: &str, value: i32) -> io::Result<()> {
    set_i32_to(&config_path(), key, value)
}

/// Écrit une clé entière dans un fichier donné (version testable).
pub fn set_i32_to(path: &Path, key: &str, value: i32) -> io::Result<()> {
    let mut map = read_all(path);
    map.insert(key.to_string(), value.to_string());
    write_all(path, &map)
}

/// Lit une clé chaîne de caractères (absente → `None`).
pub fn get_str_from(path: &Path, key: &str) -> Option<String> {
    read_all(path).get(key).cloned()
}

/// Écrit une clé chaîne de caractères (les autres clés sont conservées).
pub fn set_str_to(path: &Path, key: &str, value: &str) -> io::Result<()> {
    let mut map = read_all(path);
    map.insert(key.to_string(), value.to_string());
    write_all(path, &map)
}

/// Lit une clé booléenne (stockée `1`/`0` ; absente ou invalide → `None`).
pub fn get_bool(key: &str) -> Option<bool> {
    get_bool_from(&config_path(), key)
}

/// Lit une clé booléenne dans un fichier donné (version testable).
pub fn get_bool_from(path: &Path, key: &str) -> Option<bool> {
    match read_all(path).get(key).map(|v| v.as_str()) {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}

/// Écrit une clé booléenne (les autres clés sont conservées).
pub fn set_bool(key: &str, value: bool) -> io::Result<()> {
    set_bool_to(&config_path(), key, value)
}

/// Écrit une clé booléenne dans un fichier donné (version testable).
pub fn set_bool_to(path: &Path, key: &str, value: bool) -> io::Result<()> {
    set_i32_to(path, key, if value { 1 } else { 0 })
}

// ─── clé dédiée : mode de déplacement ───────────────────────────────────────

/// Lit le mode de déplacement enregistré, borné à
/// `[0, MOVING_MODE_COUNT-1]` (sinon `None`).
pub fn load_moving_mode() -> Option<i32> {
    load_moving_mode_from(&config_path())
}

/// Lit `moving_mode` dans un fichier donné (version testable).
pub fn load_moving_mode_from(path: &Path) -> Option<i32> {
    let mode = get_i32_from(path, "moving_mode")?;
    (0..MOVING_MODE_COUNT).contains(&mode).then_some(mode)
}

/// Enregistre le mode de déplacement (les autres clés sont conservées).
pub fn save_moving_mode(mode: i32) -> io::Result<()> {
    save_moving_mode_to(&config_path(), mode)
}

// ─── clé dédiée : version de radar active ───────────────────────────────────

/// Lit la version de radar active enregistrée (index `RadarKind` :
/// 0 = minimap, 1 = contrôleur aérien), bornée à `[0, 1]` (sinon `None`).
pub fn load_radar_kind() -> Option<i32> {
    load_radar_kind_from(&config_path())
}

/// Lit `radar_kind` dans un fichier donné (version testable).
pub fn load_radar_kind_from(path: &Path) -> Option<i32> {
    let kind = get_i32_from(path, "radar_kind")?;
    (0..=1).contains(&kind).then_some(kind)
}

/// Enregistre la version de radar active (index `RadarKind` : 0 = minimap,
/// 1 = contrôleur aérien ; les autres clés sont conservées).
pub fn save_radar_kind(kind: i32) -> io::Result<()> {
    save_radar_kind_to(&config_path(), kind)
}

/// Écrit `radar_kind` dans un fichier donné (version testable).
pub fn save_radar_kind_to(path: &Path, kind: i32) -> io::Result<()> {
    set_i32_to(path, "radar_kind", kind)
}

// ─── clés dédiées : options graphiques ─────────────────────────────────────

/// Lit le style de rendu enregistré, borné à `[0, RENDER_STYLE_COUNT-1]`
/// (sinon `None`).
pub fn load_render_style() -> Option<i32> {
    load_render_style_from(&config_path())
}

/// Lit `render_style` dans un fichier donné (version testable).
pub fn load_render_style_from(path: &Path) -> Option<i32> {
    let style = get_i32_from(path, "render_style")?;
    (0..RENDER_STYLE_COUNT).contains(&style).then_some(style)
}

/// Enregistre le style de rendu (les autres clés sont conservées).
pub fn save_render_style(style: i32) -> io::Result<()> {
    set_i32("render_style", style)
}

/// Lit la définition de fenêtre enregistrée `(largeur, hauteur)` (index borné
/// dans `WINDOW_SIZES`, sinon `None`).
pub fn load_window_size() -> Option<(i32, i32)> {
    load_window_size_from(&config_path())
}

/// Lit `window_size` dans un fichier donné (version testable).
pub fn load_window_size_from(path: &Path) -> Option<(i32, i32)> {
    let index = get_i32_from(path, "window_size")?;
    WINDOW_SIZES.get(index as usize).copied()
}

/// Enregistre la définition de fenêtre (index dans `WINDOW_SIZES`, les autres
/// clés sont conservées).
pub fn save_window_size(index: i32) -> io::Result<()> {
    set_i32("window_size", index)
}

/// Lit la position de fenêtre enregistrée `(x, y)` (coin supérieur gauche,
/// relatif à l'écran ; absente → `None`). Mise à jour quand la fenêtre
/// fenêtrée est déplacée (voir `render::persist_window_geometry`).
pub fn load_window_pos() -> Option<(i32, i32)> {
    Some((get_i32("win_x")?, get_i32("win_y")?))
}

/// Enregistre la position de fenêtre (les autres clés sont conservées).
pub fn save_window_pos(x: i32, y: i32) -> io::Result<()> {
    set_i32("win_x", x)?;
    set_i32("win_y", y)
}

/// Lit la taille **réelle** de fenêtre enregistrée `(largeur, hauteur)` en
/// pixels (taille fenêtrée au dernier déplacement/redimensionnement ; absente
/// ou invalide → `None`). Complète `load_window_size` (l'index du réglage
/// SIZE) quand la fenêtre a été redimensionnée à la main.
pub fn load_window_px_size() -> Option<(i32, i32)> {
    load_window_px_size_from(&config_path())
}

/// Lit `win_w`/`win_h` dans un fichier donné (version testable).
pub fn load_window_px_size_from(path: &Path) -> Option<(i32, i32)> {
    let w = get_i32_from(path, "win_w")?;
    let h = get_i32_from(path, "win_h")?;
    (w > 0 && h > 0).then_some((w, h))
}

/// Enregistre la taille réelle de fenêtre en pixels (les autres clés sont
/// conservées).
pub fn save_window_px_size(w: i32, h: i32) -> io::Result<()> {
    set_i32("win_w", w)?;
    set_i32("win_h", h)
}

/// Lit le mode d'affichage enregistré (fenêtré / zoomé / natif), borné à
/// `[0, VIEW_MODE_COUNT-1]` (sinon `None`).
pub fn load_view_mode() -> Option<i32> {
    load_view_mode_from(&config_path())
}

/// Lit `view_mode` dans un fichier donné (version testable).
pub fn load_view_mode_from(path: &Path) -> Option<i32> {
    let mode = get_i32_from(path, "view_mode")?;
    (0..VIEW_MODE_COUNT).contains(&mode).then_some(mode)
}

/// Enregistre le mode d'affichage (les autres clés sont conservées).
pub fn save_view_mode(mode: i32) -> io::Result<()> {
    set_i32("view_mode", mode)
}

/// Écrit `moving_mode` dans un fichier donné (version testable).
pub fn save_moving_mode_to(path: &Path, mode: i32) -> io::Result<()> {
    set_i32_to(path, "moving_mode", mode)
}

// ─── télécommande HTTP ──────────────────────────────────────────────────────

/// Code PIN de la télécommande HTTP (clé `remote_pin`, 0 à 4 chiffres) -
/// vide (ou clé absente) = aucune protection. Tout autre contenu est ignoré
/// (repli sur aucun PIN).
pub fn load_remote_pin() -> Option<String> {
    load_remote_pin_from(&config_path())
}

/// Version testable de `load_remote_pin`.
pub fn load_remote_pin_from(path: &Path) -> Option<String> {
    let pin = read_all(path).get("remote_pin")?.trim().to_string();
    if !pin.is_empty() && pin.len() <= 4 && pin.chars().all(|c| c.is_ascii_digit()) {
        Some(pin)
    } else if pin.is_empty() {
        Some(String::new())
    } else {
        None
    }
}

/// Enregistre le code PIN de la télécommande (0 à 4 chiffres) ; un PIN vide
/// supprime la clé (aucune protection).
pub fn save_remote_pin(pin: &str) -> io::Result<()> {
    save_remote_pin_to(&config_path(), pin)
}

/// Version testable de `save_remote_pin`.
pub fn save_remote_pin_to(path: &Path, pin: &str) -> io::Result<()> {
    if pin.is_empty() {
        delete_key_from(path, "remote_pin")
    } else {
        set_str_to(path, "remote_pin", pin)
    }
}

// ─── suppression ────────────────────────────────────────────────────────────

/// Supprime une clé du fichier de configuration (les autres clés sont
/// conservées - le RESET de l'écran de paramétrage supprime ainsi les clés de
/// réglage sans toucher à la progression du scénario, clés `scenario`/`prog_*`).
/// Ne fait rien (OK) si la clé ou le fichier n'existe pas.
pub fn delete_key(key: &str) -> io::Result<()> {
    delete_key_from(&config_path(), key)
}

/// Supprime une clé d'un fichier donné (version testable).
pub fn delete_key_from(path: &Path, key: &str) -> io::Result<()> {
    let mut map = read_all(path);
    if map.remove(key).is_some() {
        write_all(path, &map)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MOVING_MODE_4_WAYS, MOVING_MODE_INERTIAL};

    /// Chemin temporaire unique par test (répertoire temporaire du système).
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "meteors_mining_cfg_test_{}_{}",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn save_then_load_round_trips() {
        let p = temp_path("roundtrip.cfg");
        let _ = fs::remove_file(&p);
        save_moving_mode_to(&p, MOVING_MODE_4_WAYS).unwrap();
        assert_eq!(load_moving_mode_from(&p), Some(MOVING_MODE_4_WAYS));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn parse_and_content_round_trip() {
        // helpers partagés natif/wasm : analyse puis re-génération du contenu
        let map = parse_config("foo=bar\n\nmoving_mode=1\npasuneclé\nmoving_mode=0\n");
        assert_eq!(map.get("foo").map(String::as_str), Some("bar"));
        // première occurrence d'une clé dupliquée conservée
        assert_eq!(map.get("moving_mode").map(String::as_str), Some("1"));
        // ligne sans '=' ignorée
        assert_eq!(map.get("pasuneclé"), None);
        let content = config_content(&map);
        assert_eq!(content, "foo=bar\nmoving_mode=1\n");
        // aller-retour : le contenu régénéré redonne la même table
        assert_eq!(parse_config(&content), map);
        // table vide (comportement historique de write_all : une ligne vide)
        assert_eq!(config_content(&HashMap::new()), "\n");
    }

    #[test]
    fn atomic_write_leaves_no_temp_file_and_replaces_existing() {
        // une sauvegarde remplace proprement un fichier existant, sans
        // laisser de fichier temporaire ni perdre les autres clés
        let p = temp_path("atomic.cfg");
        let _ = fs::remove_file(&p);
        let mut map = HashMap::new();
        map.insert("foo".to_string(), "bar".to_string());
        write_all(&p, &map).unwrap();
        // pas de .tmp résiduel dans le dossier
        let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".atomic.cfg."))
            .collect();
        assert!(leftovers.is_empty(), "fichiers temporaires résiduels: {leftovers:?}");
        // la réécriture remplace bien le contenu
        map.insert("foo".to_string(), "baz".to_string());
        map.insert("moving_mode".to_string(), MOVING_MODE_4_WAYS.to_string());
        write_all(&p, &map).unwrap();
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("foo=baz"));
        assert!(content.contains(&format!("moving_mode={MOVING_MODE_4_WAYS}")));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn load_missing_file_returns_none() {
        let p = temp_path("missing.cfg");
        let _ = fs::remove_file(&p);
        assert_eq!(load_moving_mode_from(&p), None);
    }

    #[test]
    fn load_invalid_value_returns_none() {
        // valeur hors bornes ou non numérique : ignorée (défaut conservé)
        let p = temp_path("invalid.cfg");
        fs::write(&p, "moving_mode=99\n").unwrap();
        assert_eq!(load_moving_mode_from(&p), None);
        fs::write(&p, "moving_mode=banane\n").unwrap();
        assert_eq!(load_moving_mode_from(&p), None);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn save_preserves_other_keys_and_replaces_mode() {
        // une ancienne valeur est remplacée, les autres clés sont conservées
        let p = temp_path("other.cfg");
        fs::write(&p, "foo=bar\nmoving_mode=1\n").unwrap();
        save_moving_mode_to(&p, MOVING_MODE_INERTIAL).unwrap();
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("foo=bar"));
        assert!(content.contains("moving_mode=0"));
        assert!(!content.contains("moving_mode=1"));
        assert_eq!(load_moving_mode_from(&p), Some(MOVING_MODE_INERTIAL));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn delete_key_removes_one_key_and_preserves_others() {
        // suppression ciblée d'une clé (bouton RESET des réglages) : les
        // autres clés - dont la progression du scénario - sont conservées
        let p = temp_path("deletekey.cfg");
        let _ = fs::remove_file(&p);
        set_i32_to(&p, "volume", 40).unwrap();
        set_i32_to(&p, "prog_minerals", 77).unwrap();
        delete_key_from(&p, "volume").unwrap();
        assert_eq!(get_i32_from(&p, "volume"), None);
        assert_eq!(get_i32_from(&p, "prog_minerals"), Some(77));
        // clé absente ou fichier absent : OK, rien ne change
        delete_key_from(&p, "volume").unwrap();
        delete_key_from(&temp_path("absent_deletekey.cfg"), "volume").unwrap();
        assert_eq!(get_i32_from(&p, "prog_minerals"), Some(77));
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(temp_path("absent_deletekey.cfg"));
    }

    #[test]
    fn save_creates_parent_directories() {
        // dossier de config inexistant (ex `~/.config/meteors-mining/`
        // avant la première sauvegarde) : créé à la volée
        let dir = std::env::temp_dir().join(format!(
            "meteors_mining_cfg_test_{}_nested",
            std::process::id()
        ));
        let p = dir.join("sub").join("meteors_mining.cfg");
        let _ = fs::remove_dir_all(&dir);
        save_moving_mode_to(&p, MOVING_MODE_4_WAYS).unwrap();
        assert!(p.exists());
        assert_eq!(load_moving_mode_from(&p), Some(MOVING_MODE_4_WAYS));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bool_keys_round_trip() {
        let p = temp_path("bool.cfg");
        let _ = fs::remove_file(&p);
        set_bool_to(&p, "music", true).unwrap();
        set_bool_to(&p, "auto_generate", false).unwrap();
        assert_eq!(get_bool_from(&p, "music"), Some(true));
        assert_eq!(get_bool_from(&p, "auto_generate"), Some(false));
        assert_eq!(get_bool_from(&p, "absente"), None);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn set_preserves_other_keys() {
        // les clés génériques coexistent : musique, volume et mode de
        // déplacement dans le même fichier
        let p = temp_path("multi.cfg");
        let _ = fs::remove_file(&p);
        set_i32_to(&p, "volume", 40).unwrap();
        set_bool_to(&p, "music", true).unwrap();
        set_i32_to(&p, "moving_mode", 1).unwrap();
        assert_eq!(get_i32_from(&p, "volume"), Some(40));
        assert_eq!(get_bool_from(&p, "music"), Some(true));
        assert_eq!(load_moving_mode_from(&p), Some(MOVING_MODE_4_WAYS));
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn remote_pin_round_trip_and_validation() {
        // aller-retour d'un PIN ; un PIN vide supprime la clé ; un contenu
        // invalide (trop long, non numérique) est ignoré
        let p = temp_path("pin.cfg");
        let _ = fs::remove_file(&p);
        save_remote_pin_to(&p, "1234").unwrap();
        assert_eq!(load_remote_pin_from(&p), Some("1234".to_string()));
        // PIN vide = aucune protection (clé supprimée)
        save_remote_pin_to(&p, "").unwrap();
        assert_eq!(load_remote_pin_from(&p), None);
        // contenu invalide ignoré (aucun PIN)
        fs::write(&p, "remote_pin=abcde5\n").unwrap();
        assert_eq!(load_remote_pin_from(&p), None);
        fs::write(&p, "remote_pin=12a3\n").unwrap();
        assert_eq!(load_remote_pin_from(&p), None);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn graphics_keys_round_trip_and_are_bounded() {
        // style de rendu et définition de fenêtre : aller-retour, et valeurs
        // hors bornes ignorées (défaut conservé)
        let p = temp_path("graphics.cfg");
        let _ = fs::remove_file(&p);
        set_i32_to(&p, "render_style", 2).unwrap();
        set_i32_to(&p, "window_size", 3).unwrap();
        assert_eq!(get_i32_from(&p, "render_style"), Some(2));
        assert_eq!(load_window_size_from(&p), Some((1920, 1080)));
        // hors bornes → None (comportement des wrappers de chargement)
        fs::write(&p, "render_style=9\nwindow_size=banane\n").unwrap();
        assert_eq!(load_render_style_from(&p), None);
        assert_eq!(load_window_size_from(&p), None);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn window_geom_keys_round_trip() {
        // position et taille réelles de la fenêtre fenêtrée : aller-retour,
        // et taille nulle/invalide ignorée (défaut conservé)
        let p = temp_path("wingeom.cfg");
        let _ = fs::remove_file(&p);
        set_i32_to(&p, "win_x", 120).unwrap();
        set_i32_to(&p, "win_y", 80).unwrap();
        set_i32_to(&p, "win_w", 1280).unwrap();
        set_i32_to(&p, "win_h", 720).unwrap();
        assert_eq!(get_i32_from(&p, "win_x"), Some(120));
        assert_eq!(get_i32_from(&p, "win_y"), Some(80));
        assert_eq!(load_window_px_size_from(&p), Some((1280, 720)));
        // taille nulle ou invalide → None
        fs::write(&p, "win_w=0\nwin_h=0\n").unwrap();
        assert_eq!(load_window_px_size_from(&p), None);
        fs::write(&p, "win_w=-5\nwin_h=banane\n").unwrap();
        assert_eq!(load_window_px_size_from(&p), None);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn view_mode_round_trip_and_is_bounded() {
        // mode d'affichage : aller-retour, et valeurs hors bornes ignorées
        let p = temp_path("viewmode.cfg");
        let _ = fs::remove_file(&p);
        set_i32_to(&p, "view_mode", 2).unwrap();
        assert_eq!(load_view_mode_from(&p), Some(2));
        // hors bornes → None (comportement des wrappers de chargement)
        fs::write(&p, "view_mode=banane\n").unwrap();
        assert_eq!(load_view_mode_from(&p), None);
        fs::write(&p, "view_mode=9\n").unwrap();
        assert_eq!(load_view_mode_from(&p), None);
        let _ = fs::remove_file(&p);
    }
}
