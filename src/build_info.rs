//! Version et numéro de build de l'application, rendus publics pour l'écran
//! titre (coin bas-droit de `title.rs`).
//!
//! Injectées à la compilation par `build.rs` (variables `APP_BUILD`,
//! `APP_COMMIT`, `APP_BUILD_DATE`, créées à partir du dépôt git et, en CI, des
//! compteurs de pipeline - voir `build.rs`). La **version sémantique** est le
//! champ `version` de `Cargo.toml` (`CARGO_PKG_VERSION`, lu directement, il
//! est toujours défini par Cargo). Les valeurs par défaut de `option_env!`
//! sont un simple filet de sécurité (le script de build écrit toujours ces
//! variables) pour permettre la compilation éventuelle d'un fichier isolé.

/// Version sémantique de l'application (champ `version` de `Cargo.toml`, ex
/// `0.1.0`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Numéro de build : compteur de pipeline CI (GitHub Actions
/// `GITHUB_RUN_NUMBER`, `BUILD_NUMBER`, GitLab `CI_PIPELINE_IID`) sinon le
/// nombre total de commits en développement, sinon `0`.
pub const BUILD: &str = match option_env!("APP_BUILD") {
    Some(s) => s,
    None => "0",
};

/// Empreinte courte (7 caractères) du commit compilé ; vide hors dépôt git
/// (archive de source).
pub const COMMIT: &str = match option_env!("APP_COMMIT") {
    Some(s) => s,
    None => "",
};

/// Date du commit compilé (format `YYYY-MM-DD`) ; vide hors dépôt git.
pub const BUILD_DATE: &str = match option_env!("APP_BUILD_DATE") {
    Some(s) => s,
    None => "",
};

/// Libellé compact affiché sur l'écran titre, ex `v0.1.0 build 42 2026-08-29
/// a1b2c3d` - la date et l'empreinte n'y figurent que si elles existent
/// (build hors dépôt git : juste « v0.1.0 build 0 »).
pub fn display() -> String {
    let mut s = format!("v{} build {}", VERSION, BUILD);
    if !BUILD_DATE.is_empty() {
        s.push(' ');
        s.push_str(BUILD_DATE);
    }
    if !COMMIT.is_empty() {
        s.push(' ');
        s.push_str(COMMIT);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_like() {
        // ex "0.1.0" - au minimum une forme non vide
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn display_compact() {
        let d = display();
        // en dépôt git complet : version - build - date - empreinte
        if !BUILD_DATE.is_empty() && !COMMIT.is_empty() {
            assert_eq!(
                d,
                format!("v{} build {} {} {}", VERSION, BUILD, BUILD_DATE, COMMIT)
            );
        } else {
            assert!(d.starts_with(&format!("v{} build {}", VERSION, BUILD)));
        }
    }
}