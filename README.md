# rust-meteors-mining

Portage en **Rust + macroquad** du jeu QB64 **« Meteors Mining »** (2D, arcade, minage spatial).

Ce répertoire est **auto-suffisant** : il contient tout ce qui est nécessaire au portage, sans dépendre du
workspace d'origine (qui ne sera plus accessible une fois le port commencé).

## Contenu

```
rust-meteors-mining/
├── README.md              ← ce fichier
├── Cargo.toml             ← projet Rust (macroquad)
├── assets/                ← textures utilisées par le portage (copie convertie,
│                            voir docs/ASSETS.md §4)
├── src/
│   ├── main.rs            ← boucle principale (fenêtre 960×540, sans vsync)
│   ├── config.rs          ← constantes (vue, monde torique, gameplay)
│   ├── geom.rs            ← Point, World, Segment, Triangle + géométrie
│   ├── shape.rs           ← Shape, meshes, collisions, mouvement
│   ├── garbage.rs         ← débris
│   ├── state.rs           ← Player, Element, GameState, messages
│   ├── generate.rs        ← génération procédurale des météores, prepare
│   ├── game.rs            ← boucle de jeu (input, déplacement, pause, plein écran)
│   ├── render.rs          ← rendu (étoiles, triangles texturés, HUD, aide, debug)
│   └── title.rs           ← écran titre (bannière arc-en-ciel, étoiles)
├── docs/
│   ├── ANALYSE.md         ← analyse complète de l'application d'origine
│   ├── PORTAGE.md         ← plan de portage pas-à-pas, mapping, pièges
│   └── ASSETS.md          ← manifeste des assets (images, sons, meshes)
└── reference/             ← copie à l'identique du code QB64 d'origine
    ├── source/            ← 10 fichiers .bas (le jeu complet, ~2 800 lignes)
    ├── assets/            ← 38 fichiers (textures, sons OGG, meshes, données)
    ├── library/           ← windowUtils.bi/.bm/.qlb (dépendance externe QB64)
    └── build-v4.5.sh      ← script de build QB64 (référence)
```

## Prise en main rapide

```bash
cd rust-meteors-mining
cargo run --release
```

La fenêtre 960×540 (taille de la vue du jeu d'origine) s'ouvre avec une boucle sans vsync (FPS
réel ~225 en fenêtré sur GPU virtio, ~65 en plein écran — voir `docs/PORTAGE.md` Phase 5). État
actuel : Phases 0-2 + jalons M2 à M6 terminés — modèle de données complet, rendu (étoiles
précalculées, vaisseau, station, caméra centrée joueur, HUD, cargo), **déplacement du vaisseau**
(flèches : ↑ accélérer, ←/→ tourner, ↓ décélérer ; **P** pause, **F** fait cycler trois modes
— fenêtré, plein écran zoomé (vue 960×540 étirée) et **plein écran natif** (rendu direct à la
définition réelle de l'écran, sans buffer), voir `docs/PORTAGE.md` §4.1 ; **M** coupe la
musique ; **ESC** quitter),
**audio** (ambiance, musique, moteur, tirs, gemmes, explosions à volume selon la distance),
**météores en jeu** (**G** génère un météore, **A** active/désactive la génération automatique ;
**météores en jeu** (**G** génère un météore, **A** active/désactive la génération automatique ;
dérive, rendu texturé, collisions, débris, messages en bas d'écran) et **combat/ressources**
(**Shift** tire des balles qui détruisent les météores ; les triangles minéraux laissent des
gemmes à ramasser — la soute se remplit et se vide à la station). **Accostage** : revenez à
moins de 5 px de la station pour ouvrir la boîte DOCK STATION (UNLOAD/CLOSE). **Écran titre**
(bannière « METEORS MINING » arc-en-ciel, lancement sur une touche), **aide** (**S**,
fenêtre des touches, fermeture au clic sur CLOSE) et **debug** (**D** données des formes,
**I** informations : keycode, génération auto, compteurs, formes/triangles vivants).
Le portage suit le plan décrit dans `docs/PORTAGE.md`.

## Ordre de lecture conseillé

1. `docs/ANALYSE.md` — comprendre le jeu et son architecture
2. `docs/ASSETS.md` — savoir quels assets utiliser où
3. `docs/PORTAGE.md` — le plan d'action, phase par phase
4. `reference/source/*.bas` — la référence exacte quand un doute subsiste

## Notes importantes

- Le code d'origine est **en cours de refactor** (état du working tree au 15 août 2026) : les constantes
  ont été déplacées dans `ctx` (structure de contexte), les limites de dessin `DRAW_*` élargies de ±100,
  les triangles agrandis, la station mise à l'échelle 1 et le vaisseau joueur dessiné séparément des météores.
  Le portage doit reproduire **cet état**, pas une version plus ancienne.
- Les conventions Y (vers le bas, convention écran) et le monde torique (rebouclage des positions) sont
  des aspects subtils à respecter scrupuleusement — voir `docs/PORTAGE.md`.
- `reference/library/windowUtils.*` est la bibliothèque QB64 externe (fenêtre d'aide, boîte de dialogue
  d'accostage). Elle est **réimplémentable en Rust** (petite UI) — voir le plan.
- Les fichiers `reference/assets/meshMeteorMining.data` et `.msh` ne sont **référencés par aucun code**
  (format d'exemple pour un mesh de météore) — inutiles pour le portage mais conservés.
- `build/meteorsMining` (binaire QB64 compilé) n'a pas été capturé : inutile pour un portage en Rust.

## Version de référence

| Élément | Valeur |
|---|---|
| Langage d'origine | QB64 Phoenix Edition (v4.5) |
| Jeu | « Meteors Mining » (première version jouable : 12 nov. 2025) |
| Vue | 960 × 540, plein écran optionnel (F) |
| État capturé | working tree du 15 août 2026 (refactor en cours) |
| Cible | Rust (édition 2021) + macroquad |
