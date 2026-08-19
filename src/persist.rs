//! Persistance des réglages dans un fichier de configuration.
//!
//! Format texte simple `clé=valeur`, une par ligne, dans
//! `meteors-mining/meteors_mining.cfg` sous le dossier de configuration
//! utilisateur (norme XDG : `$XDG_CONFIG_HOME`, ou `~/.config` à défaut).
//!
//! Clés actuelles :
//! - `moving_mode`      — mode de déplacement (0..2, écran de paramétrage O)
//! - `music`            — musique en marche (0/1, touche M)
//! - `volume`           — volume maître en pourcentage (0..100)
//! - `render_style`     — style de rendu des triangles (0..2, écran O)
//! - `window_size`      — index dans `WINDOW_SIZES` (0..3, écran O)
//!   (le mode d'affichage fenêtré / zoomé / natif n'est, lui, **pas**
//!   persisté : le jeu démarre toujours fenêtré, cycle F prévisible)
//! - `antialias`        — MSAA 4× (0/1, écran O ; appliqué au lancement)
//! - `scenario`         — scénario choisi (0 = jeu libre, 1 = Progression,
//!   2 = Survival, touche N de l'écran titre)
//! - `prog_minerals`    — minerais en banque (Progression)
//! - `prog_modes`       — modes de déplacement débloqués (masque binaire,
//!   Progression)
//! - `prog_reputation`  — réputation × 10 (entier, au dixième près,
//!   Progression)
//! - `prog_up_fuel`     — extensions de réservoir achetées (Progression,
//!   atelier de la station)
//! - `prog_up_ammo`     — extensions de chargeur achetées (Progression)
//! - `prog_up_cargo`    — extensions de soute achetées (Progression)
//! - `prog_lives`       — vies restantes (Survival)
//! - `prog_shield`      — bouclier restant × 10 (entier, Survival)
//!
//! Le fichier est lu au lancement du jeu (les valeurs enregistrées remplacent
//! les défauts) et réécrit à chaque modification d'un réglage ou de la
//! progression d'un scénario. Aucune dépendance externe — simple `std::fs`,
//! dans l'esprit « binaire autonome » du port.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::{MOVING_MODE_COUNT, RENDER_STYLE_COUNT, WINDOW_SIZES};

/// Nom du fichier de configuration (dans le dossier de configuration
/// utilisateur, sous `meteors-mining/`).
pub const CONFIG_FILE: &str = "meteors_mining.cfg";

/// Chemin du fichier de configuration : `meteors-mining/meteors_mining.cfg`
/// dans le dossier de configuration utilisateur (norme XDG — voir
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

/// Lit toutes les clés du fichier en une table (ignorées : lignes vides,
/// sans `=`, et clés dupliquées après la première).
fn read_all(path: &Path) -> HashMap<String, String> {
    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
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

/// Écrit toutes les clés (triées) ; crée le dossier parent s'il n'existe pas.
fn write_all(path: &Path, map: &HashMap<String, String>) -> io::Result<()> {
    let mut lines: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>();
    lines.sort();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, lines.join("\n") + "\n")
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

/// Écrit `moving_mode` dans un fichier donné (version testable).
pub fn save_moving_mode_to(path: &Path, mode: i32) -> io::Result<()> {
    set_i32_to(path, "moving_mode", mode)
}

// ─── suppression ────────────────────────────────────────────────────────────

/// Supprime une clé du fichier de configuration (les autres clés sont
/// conservées — le RESET de l'écran de paramétrage supprime ainsi les clés de
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
        // autres clés — dont la progression du scénario — sont conservées
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
        let _ = fs::remove_file(&temp_path("absent_deletekey.cfg"));
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
}
