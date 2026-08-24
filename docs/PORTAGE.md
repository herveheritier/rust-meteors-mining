# PORTAGE.md - Meteors Mining (QB64 → Rust)

Notes de portage du jeu « Meteors Mining » (version QB64, `meteorsMining.bas`
et sa bibliothèque `windowUtils.bm`) vers Rust + macroquad. Ce document décrit
les écarts volontaires, les conventions reprises du BASIC et les sections
référencées depuis le code (`docs/PORTAGE.md §N`).

## 1. Vue d'ensemble

Le portage suit la boucle du BASIC original (`mainLoop` / `titleLoop`) :

- **entrées** : clavier, joystick tactile (`touch.rs`), télécommande HTTP
  (`remote.rs`), manette de jeu (`gamepad.rs`) ;
- **logique** : `game.rs` (boucle), `scenario.rs` / `scenario_objectives.rs`
  (règles de jeu en données + points d'accroche purs, testables sans
  macroquad), `shape.rs` / `geom.rs` (formes, triangles, collisions SAT,
  monde torique) ;
- **rendu** : `render.rs` et ses sous-modules `hud`, `dock_render`,
  `shop_render`, `ui_boxes` (découpage maintenabilité du 20/08/2026),
  police embarquée `font.rs` ;
- **audio** : `audio.rs` (backends quad-snd/miniaudio, no-op sur wasm).

## 2. Structure du dépôt

```
src/             modules Rust (28 modules après découpage de game.rs/render.rs)
assets/          textures, sons .ogg, police DejaVu (include_bytes!)
scenarios/       scénarios JSON édités par le scenario-editor
tools/           éditeur de la place de marché + éditeur de scénarios DAG
docs/PORTAGE.md  ce document
.github/workflows/ CI (fmt+clippy+tests) et release multi-plateformes
```

## 3. Compilation

- Natif : `cargo run --release` (édition 2024, MSRV 1.85).
- Web : `cargo build --release --target wasm32-unknown-unknown` (la
  télécommande HTTP, le gamepad et X11 sont gated `cfg` hors wasm ; le son
  est silencieux sur le web - shims no-op `src/wasm_audio_shims.rs`).
- CI : `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
  `cargo test`, builds natif + wasm ; release sur tag `v*`.

## 4. Portage de la logique

Le BASIC travaille sur des tableaux de triangles (`shape(x).trian%` etc.), le
Rust sur `Vec<Shape>` + `Vec<Triangle>`. Correspondances importantes :

- `WHOIAM_*` (config.rs) = type des formes (météore, balle, joueur, minerai,
  station, alien, cosmonaute EVA).
- La « vie » d'une forme = vie de ses triangles ; un triangle minéralisé
  porte son `element` (or / fer / eau). La forme n'est nettoyée que quand
  tous ses triangles sont morts (`count_alive_shapes`).
- La génération procédurale (`generate.rs`) est seedée (`ChaCha12`) pour être
  déterministe et testable.

### §4.1 Monde torique

Le monde est un tore : sa largeur/hauteur `WORLD_WIDTH × WORLD_HEIGHT`
(3960 × 3540), aucune bordure. Toute distance (`wrapped_distance`) et tout
déplacement (`normalize_world`) se font avec repliement cyclique. La caméra
et `document*_POSITION` (station) sont recalculées à chaque frame (décalage
de `camera` pour le rendu `letterbox`).

Les positions « du monde » sont des flottants ; les positions écran sont
obtenues par la caméra (ex `camera_for`, `screen_to_game`).

## 5. Modes de déplacement

Quatre modes (`MOVING_MODE_*`) portés fidèlement des blocs `select case` de
`mainLoop` : **REALISTIC** (poussée vectorielle + inertie angulaire),
**INERTIAL** (même poussée, rotation imposée), **4 WAYS** (poussée dans 4
directions de l'écran), **DIRECTIONAL** (comportement historique QB64).
`thrust_vector` combine la vitesse et la poussée en polaires ; les formules
`60*valeur/fps` deviennent `valeur*60*dt` (équivalent à 60 FPS).

Économie : les coûts, capacités et formules sont des constantes
(`FREE_PLAY_SCENARIO`, `PROGRESSION_SCENARIO`, `SURVIVAL_SCENARIO`). La
progression est persistée dans le fichier de config (`scenario` + `prog_*`).

## 6. Conventions d'écran (y vers le bas)

Comme l'original (mode graphique écran), l'axe Y va **vers le bas** : les
formules utilisent `-sin` dans `moving_shape`, et `direction = atan2(dy, dx)`
sur l'axe écran. Ne PAS « corriger » les signes : l'ensemble s'annule et le
jeu tourne « comme le BASIC ».

Autres écarts connus (les `docs/PORTAGE.md §6` du code y font référence) :

- **Couleurs** : `argb_to_color` convertit AARRGGBB → RGBA (l'ordre des
  octets change entre QB64 et macroquad).
- **Seamless** : le wrapping torique est bloqué dans la boucle de dessin
  (`_seamless` → `inner_draw_limit`, `wrapped_world`), pas côté GPU.
- **Textures triangles** : macroquad 0.4 n'a pas `draw_triangle_texture` ;
  il est implémenté via `models::Mesh` + `draw_mesh` (pipeline 2D).
- **Étoiles** : l'original pset 100k étoiles/frame ; le port précacule 15
  couches de parallaxe en tuiles 1024² et ne dessine que l'écran (densité
  encore /3 sous les fenêtres modales - `STAR_DENSITY_REDUCTION`).
- **FPS** : `ATTEMPT_FPS` (600) n'est qu'une limite, le rendu plafonne plus
  bas ; `LIMIT_FPS` et `move_delta` de `mainLoop` sont remplacés par le
  `dt` de frame (`get_frame_time()`).
- **Affichage I/D** : `SHOW_INFOS` (débug QB64) est devenu des touches
  runtime (D = données, I = infos), vs `SHOW_GLOBAL_MAP` qui n'est plus une
  compilation : la minimap est un équipement **radar** achetable au magasin
  en scénario à économie (`scenario::has_radar`).

## 7. Fenêtrage / plein écran (EWMH)

Trois vues (`ViewMode`) : fenêtré, « zoomé » (plein écran, contenu rendu dans
une `RenderTarget` puis étiré - `render_target` + `draw_zoomed`), et « natif »
(rendu direct à la définition réelle de l'écran). La bascule vers le plein
écran *natif* passe par un `ClientMessage` `_NET_WM_STATE` direct (via
`libX11`, `src/x11.rs`) avec repli au redimensionnement quand l'EWMH n'est
pas disponible (certains affichages) ; la position/taille réelle de la
fenêtre est persistée (`persist_window_geometry`).

## 8. Audio

Fidèle à `mainLoop` (supports `_sndplay`/`_sndloop`/`_sndpause`) :

- `mis4.ogg` = tir ; `gem1.ogg` = minerai (volume 0,05) ; `exp11..20.ogg` =
  explosions (volume selon la distance au vaisseau) ; `bruitDeFond.ogg` =
  ambiance ; `music1.ogg` = musique (volume 0,1, touche M) ; `fffff.ogg` =
  moteur avant / arrière (boucles).
- Volumes : **maître** + sous-volumes **musique / effets / ambiance**
  (écran de paramétrage O, clés `volume`, `music_volume`, `effects_volume`,
  `ambient_volume`), appliqués aux boucles et aux effets (`apply_gains`).

## 9. Télécommande HTTP

`remote.rs` sert sur le réseau local (port 8642) une page mobile (D-pad +
FIRE, état du jeu en direct via `/state`). `POST /cmd` pilote le vaisseau ;
**PIN** optionnel (`REMOTE PIN` de l'écran de paramétrage, 4 chiffres) exigé
quand il est défini, limite de taille des requêtes.

## 10. Tests

`cargo test` : ~200 tests unitaires (géométrie/torique, collisions,
économies/scénarios, persist, objectifs DAG, télécommande). Les modules de
règles (`scenario.rs`, `geom.rs`, `shape.rs`, `objective_tracker.rs`…) sont
testables sans macroquad ; le round-trip des scénarios JSON est couvert;
la validation de la syntaxe JS des éditeurs est faite en CI (`node --check`).

## État du portage

Voir `README.md` pour la liste des fonctionnalités implémentées (mondes
vivants, EVA, économique, radar, DAG objectives…) et les divergences
volontaires (choix de l'interface, ergonomie du HUD, réglages persistés).