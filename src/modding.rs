//! Modding par **assets externes** : remplacement des textures et sons
//! embarqués par des fichiers présents dans un dossier `user_assets/` à
//! côté de l'exécutable (ou dans le répertoire courant).
//!
//! Le binaire reste autonome (tous les assets sont embarqués via
//! `include_bytes!`) : au démarrage, si un fichier `user_assets/<nom>`
//! existe, il est chargé **à la place** de l'asset embarqué - sinon le
//! repli sur l'asset intégré s'applique (aucun fichier requis).
//!
//! Sur wasm32 (pas de système de fichiers), le repli sur l'asset embarqué
//! est systématique.

/// Chemin du dossier d'assets utilisateur (relatif à l'exécutable ou au
/// répertoire courant).
pub const USER_ASSETS_DIR: &str = "user_assets";

/// Cherche le dossier `user_assets/` : à côté de l'exécutable, puis dans le
/// répertoire courant (comme `scenario_loader::find_scenarios_dir`).
fn find_user_assets_dir() -> Option<std::path::PathBuf> {
    // 1. À côté de l'exécutable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(USER_ASSETS_DIR);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    // 2. Répertoire courant
    let p = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(USER_ASSETS_DIR);
    p.is_dir().then_some(p)
}

/// Octets d'un asset : le contenu de `user_assets/<nom>` s'il existe, sinon
/// l'asset embarqué (`include_bytes!`). L'emplacement du dossier est loggé
/// au premier chargement (journal de bord du développeur - le jeu continue
/// sans lui).
#[cfg(not(target_arch = "wasm32"))]
pub fn asset_bytes(file: &str, embedded: &'static [u8]) -> Vec<u8> {
    let Some(dir) = find_user_assets_dir() else {
        return embedded.to_vec();
    };
    let path = dir.join(file);
    match std::fs::read(&path) {
        Ok(bytes) => {
            eprintln!("[modding] user_assets remplace {file} ({})", path.display());
            bytes
        }
        Err(_) => embedded.to_vec(),
    }
}

/// Repli wasm : pas de système de fichiers - l'asset embarqué fait foi.
#[cfg(target_arch = "wasm32")]
pub fn asset_bytes(_file: &str, embedded: &'static [u8]) -> Vec<u8> {
    embedded.to_vec()
}

/// Format d'image d'un fichier (déduit de son extension) - utilisé quand un
/// asset de `user_assets/` remplace une texture embarquée dont on connaît le
/// format mais pas celui du fichier remplaçant. `.jpg`/`.jpeg` → Jpeg,
/// sinon Png (défaut de la quasi-totalité des textures du jeu).
pub fn image_format_for(file: &str) -> macroquad::prelude::ImageFormat {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        macroquad::prelude::ImageFormat::Jpeg
    } else {
        macroquad::prelude::ImageFormat::Png
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_format_detected_from_extension() {
        use macroquad::prelude::ImageFormat;
        assert_eq!(image_format_for("texture.jpg"), ImageFormat::Jpeg);
        assert_eq!(image_format_for("TEXTURE.JPEG"), ImageFormat::Jpeg);
        assert_eq!(image_format_for("texture.png"), ImageFormat::Png);
        assert_eq!(image_format_for("texture.ogg"), ImageFormat::Png);
    }

    #[test]
    fn embedded_fallback_without_user_assets() {
        // sans dossier user_assets/ (cas courant des tests), l'asset embarqué
        // est renvoyé tel quel
        let embedded: &'static [u8] = &[1, 2, 3, 4];
        let bytes = asset_bytes("inexistant.png", embedded);
        assert_eq!(bytes, embedded.to_vec());
    }
}
