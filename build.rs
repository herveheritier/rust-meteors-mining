//! Script de build : capture un **numéro de build**, l'**empreinte git** et la
//! **date** du commit compilé, et les injecte en variables d'environnement de
//! compilation (`APP_BUILD`, `APP_COMMIT`, `APP_BUILD_DATE`) lues par
//! `src/build_info.rs` pour l'affichage sur l'écran titre. La version
//! sémantique, elle, vient de `Cargo.toml` (`CARGO_PKG_VERSION`).
//!
//! Le **numéro de build** provient de, dans l'ordre :
//!   1. un compteur fourni par la CI - `GITHUB_RUN_NUMBER` (GitHub Actions),
//!      `BUILD_NUMBER` (Jenkins/TeamCity…), `CI_PIPELINE_IID` (GitLab) : le
//!      build est alors un numéro globalement croissant, propre au pipeline ;
//!   2. à défaut, le **nombre total de commits** de l'historique git local
//!      (`git rev-list --count HEAD`) : un numéro qui croît naturellement à
//!      chaque commit en développement ;
//!   3. en dernier recours (pas de CI et hors dépôt git - archive de source),
//!      `0`.
//!
//! L'**empreinte** est le hash court du commit HEAD (`git rev-parse --short
//! HEAD`) et la **date** celle du commit (`git log --format=%cs`) ; toutes
//! deux vides hors dépôt git (le libellé de l'écran titre s'en passe alors).

use std::env;
use std::path::Path;
use std::process::Command;

/// Sortie stdout d'une commande `git` réussie (trimée), sinon `None`.
fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(repo).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let repo = Path::new(&manifest_dir);

    // Numéro de build : compteur CI en priorité, sinon nb de commits, sinon 0.
    let build = env::var("GITHUB_RUN_NUMBER")
        .or_else(|_| env::var("BUILD_NUMBER"))
        .or_else(|_| env::var("CI_PIPELINE_IID"))
        .ok()
        .or_else(|| git(repo, &["rev-list", "--count", "HEAD"]))
        .unwrap_or_else(|| "0".to_string());

    // Empreinte courte et date du commit compilé (vides hors dépôt git).
    let commit = git(repo, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let date = git(repo, &["log", "-1", "--format=%cs"]).unwrap_or_default();

    // `cargo:rustc-env` rend ces variables visibles via `option_env!` au
    // moment de la compilation des crates qui en dépendent (build.rs du
    // package racine - toujours exécuté, même sur wasm).
    println!("cargo:rustc-env=APP_BUILD={build}");
    println!("cargo:rustc-env=APP_COMMIT={commit}");
    println!("cargo:rustc-env=APP_BUILD_DATE={date}");

    // Reconstruire quand l'HEAD avance (nouveau commit → nouvelle empreinte
    // et numéro de build). `rerun-if-changed` attend des fichiers : on cible
    // `.git/HEAD` (qui porte le pointeur de branche ou, en détaché, la valeur
    // du commit) et, en branché, le fichier de la ref de la branche.
    let head = repo.join(".git").join("HEAD");
    if let Ok(content) = std::fs::read_to_string(&head) {
        if let Some(ref_file) = content.trim().strip_prefix("ref: ") {
            let ref_path = repo.join(".git").join(ref_file);
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
        println!("cargo:rerun-if-changed={}", head.display());
    }
}