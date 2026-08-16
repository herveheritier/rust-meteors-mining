# Meteors Mining

Jeu d'arcade 2D de minage spatial écrit en **Rust** avec [macroquad](https://macroquad.rs).
Réimplémentation fidèle du jeu QB64 « Meteors Mining » (première version jouable : 12 nov. 2025).

## Le jeu

Pilotez votre vaisseau dans un champ de météores, détruisez-les au tir et
ramassez les gemmes minérales qu'ils laissent derrière eux. Remplissez la
soute, puis revenez à la station pour décharger et faire réparer le vaisseau.

- **Monde torique** : l'espace se reboucle sur lui-même (3960 × 3540), aucun bord.
- **Météores destructibles** : 6 à 16 triangles par météore, générés
  procéduralement, avec choc élastique entre eux et débris à chaque impact.
- **Minage** : les triangles minéraux (or, fer…) laissent des gemmes à ramasser.
- **Soute** : 5 éléments maximum — pleine, il faut décharger à la station.
- **Station** : accostez à moins de 5 px pour ouvrir la boîte DOCK STATION ;
  le cargo est déchargé et le vaisseau réparé.
- **Météores en continu** : génération automatique (limite 150) ou à la demande.
- **Audio** : ambiance, musique, moteur avant/recul, tirs, gemmes et
  explosions à volume selon la distance au vaisseau.

## Compilation et lancement

```bash
cargo run --release
```

La fenêtre 960 × 540 s'ouvre sur l'écran titre — appuyez sur une touche
(autre que F) pour lancer la partie.

### Exécutable autonome

Les textures et sons sont **intégrés dans le binaire** (`include_bytes!`),
rien à copier à côté :

```bash
cargo build --release
./target/release/rust-meteors-mining   # lançable de n'importe où
```

Le binaire (~2,8 Mo compressé) est autonome : on peut le copier seul sur une
autre machine (même OS) et le lancer directement, sans dossier `assets/` ni
`cargo run`.

Optimisations de taille appliquées (profil `release` de `Cargo.toml`) :
`lto = true`, `codegen-units = 1`, `strip = true` ; la texture météore est
embarquée en **JPEG** (457 Ko au lieu de 3,1 Mo en PNG — feature `jpeg` de la
crate `image` activée). Compression finale facultative avec **UPX** (binaire
statique sur GitHub) — à relancer après chaque `cargo build --release` :

```bash
upx --best --lzma target/release/rust-meteors-mining
```

## Contrôles

| Touche | Action |
|---|---|
| ↑ | Accélérer |
| ← / → | Tourner |
| ↓ | Décélérer |
| Shift (gauche ou droit) | Tirer |
| P | Pause |
| S | Aide (liste des touches, fermeture au clic sur CLOSE) |
| G | Générer un météore près du vaisseau |
| A | Activer/désactiver la génération automatique des météores |
| C | Créer un alien |
| F | Cycler les modes d'affichage : fenêtré → plein écran zoomé → plein écran natif |
| M | Couper/relancer la musique |
| D | Afficher les données des formes (debug) |
| I | Afficher les informations (keycode, compteurs, formes/triangles vivants) |
| ESC | Quitter |

Au démarrage, le vaisseau est à la station : éloignez-vous pour commencer à
miner. Revenez à moins de 5 px de la station pour ouvrir la boîte
DOCK STATION (UNLOAD/CLOSE).

## Détails techniques

- Rust (édition 2021) + macroquad 0.4, sans vsync (boucle plafonnée, physique
  en `dt` indépendante du FPS).
- 100 000 étoiles précalculées sur 15 couches de parallaxe.
- Génération procédurale **déterministe** (PRNG ChaCha12 seedé) — parties
  reproductibles.
- Collisions par séparation de triangles (SAT) + choc élastique ; le centre
  des formes est recalculé après chaque impact.
- Plein écran : mode **zoomé** (vue 960 × 540 rendue dans une texture puis
  étirée, letterbox) ou **natif** (rendu direct à la définition réelle de
  l'écran) ; la bascule EWMH passe par `src/x11.rs` (ClientMessage
  `_NET_WM_STATE` direct, sans outil externe).
- Le jeu est testé : `cargo test` (37 tests unitaires — physique, collisions,
  minage, accostage, nettoyage des formes).

## Structure du projet

```
rust-meteors-mining/
├── Cargo.toml              ← projet Rust (macroquad, rand, image)
├── assets/                 ← textures (.png/.jpg) et sons (.ogg) intégrés au binaire
└── src/
    ├── main.rs             ← boucle principale (fenêtre 960×540, sans vsync)
    ├── config.rs           ← constantes (vue, monde torique, gameplay)
    ├── geom.rs             ← Point, World, Segment, Triangle + géométrie
    ├── shape.rs            ← Shape, meshes, collisions, mouvement
    ├── garbage.rs          ← débris
    ├── state.rs            ← Player, Element, GameState, messages
    ├── generate.rs         ← génération procédurale des météores, prepare
    ├── game.rs             ← boucle de jeu (input, déplacement, collisions, pause)
    ├── render.rs           ← rendu (étoiles, triangles texturés, HUD, aide, debug)
    ├── title.rs            ← écran titre (bannière arc-en-ciel, étoiles)
    ├── audio.rs            ← sons et musique (ambiance, moteur, explosions)
    └── x11.rs              ← plein écran EWMH (X11)
```
