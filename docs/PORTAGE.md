# Plan de portage Rust + macroquad

> Objectif : une traduction **fidèle** de « Meteors Mining » (QB64) en Rust, avec le même
> comportement, les mêmes assets et un rendu au moins aussi bon, à 60 FPS stables.
> Le code QB64 de référence est dans `../reference/source/`.

## 0. Choix techniques

- **Langage** : Rust (édition 2021), canal stable.
- **Framework** : [macroquad](https://macroquad.rs) (`cargo add macroquad`). Rendu GPU, input,
  audio (`quad-snd`), polices via `quad-rng`/`fontdue`… macroquad embarque `miniquad` + `quad-snd`.
- **Audio** : les assets sont des **OGG**. macroquad/quad-snd gère le WAV natif ; pour l'OGG il faut
  décoder (crate `lemmings` ou `rodio`-style). **Plan A** : utiliser `rodio` (lecteur OGG/MP3/WAV
  mature) en parallèle de macroquad, ou `quad-snd` + décodeur OGG maison (`lewton`). **Plan B** :
  convertir les OGG en WAV/MP3 avec ffmpeg pendant la préparation (une seule fois) et tout jouer
  via `quad-snd`. La conversion est la solution la plus simple et robuste.
- **Structure du projet** : modules par domaine (voir §2), pas un monolithe.
- **Nombres** : `f64` partout pour la géométrie (comme en QB64), `f32` uniquement aux frontières
  GPU. Vecteurs via `macroquad::prelude::Vec2` ou un type `Point { x: f64, y: f64 }` maison —
  recommandé : type maison `Point` (coïncide avec `point_type`).

## 1. Phases de portage

### Phase 0 — Squelette et boucle (fait dans `src/main.rs`)
- Fenêtre 960×540, boucle macroquad 60 FPS avec `delta_time`.
- États d'écran : `Title` → `Game` (enum d'état).
- Input : flèches, Shift, ESC, touches A/C/D/F/G/I/K/M/P/S/T, F1.
- Plein écran (F) : macroquad `set_fullscreen(true/false)`.
- Le squelette livré ouvre déjà la fenêtre et dessine un fond noir + les étoiles (voir Phase 2).

### Phase 1 — Modèle de données ✅ (fait)

> État : **terminée** (août 2026). Modules créés : `src/config.rs` (constantes),
> `src/geom.rs` (Point, World, Segment, Triangle + géométrie), `src/shape.rs`
> (Shape, meshes station/alien/gemme/balle, fonctions de forme),
> `src/garbage.rs`, `src/state.rs` (Player, Element, GameState), `src/generate.rs`
> (`generate_shape`, `create_shape`, `create_alien/station/gem`, `fire_bullet`,
> `prepare`). 18 tests unitaires verts (`cargo test`). Le `pointsUsageIndicator`
> est un **bitmask `u64`** (bord_len = 3×nb triangles) ; les constantes sont des
> `const` dans `config.rs` ; les indices joueur/station sont explicites
> (`PLAYER_INDEX = 0`, `STATION_INDEX = 1`). Détail préservé : `meshes_to_shape`
> alloue `points_qty` emplacements (dont `2×nbPacks` morts), et le bug
> `createStation → computeShapeCenter(shapes[0])` est corrigé (calcul sur la
> station, résultat identique).

Traduire les types QB64 en structs Rust (`src/model.rs`) :
- `Point { x: f64, y: f64 }` (ex `point_type`), avec les helpers : `dot`, `rotation(axe, angle)`,
  `normalize_world(&World)`, `normalize_plan(&World, plan)`, `are_equal(eps)`, `generate_vertex_outside`.
- `World { width, height, minx, maxx, miny, maxy }` (ex `world_type`) + `define_world`.
- `Triangle` (ex `triangle_type`) : géométrie locale `a,b,c,center,demibase,angle,hauteur`,
  géométrie monde `real_a,b,c,center,min,max`, états `life,collid,collid_by,element,borders`,
  `texture_base_position`, `shape_index`, `id`.
- `Shape` (ex `shape_type`) : `id, first_triangle, last_triangle, life, is_collider, element,
  show_all_parts, who_i_am, width, height, radius, direction, velocity, orientation, rotation,
  position, top_left, bottom_right, center, target_center, shape_color, texture`.
  `pointsUsageIndicator` → **bitmask `u32`** (ou `Vec<bool>`) : voir §4.
- `Garbage` (ex `garbage_type`), `Player` (ex `player_type`), `Element { id, name, color, count }`.
- `GameState` (ex `context_type`) : toutes les constantes deviennent des `const` Rust (`src/config.rs`)
  au lieu de champs — elles sont fixes. Seul l'état dynamique reste dans `GameState`.

Structures de données dynamiques (au lieu des tableaux QB64 globaux) :
- `Vec<Shape>`, `Vec<Triangle>`, `Vec<Garbage>`, `Vec<Element>`.
- `free_shape(&mut shapes, nbr) -> Option<usize>` (même logique de réutilisation).
- Références : `player = shapes[0]`, `station = shapes[1]` — **rendre explicites** (`player_index = 0`,
  `station_index = 1`).

### Phase 2 — Rendu ✅ (fait)

> État : **terminée** (août 2026) dans `src/render.rs` : assets (`Assets::load`),
> étoiles précalculées (une tuile 1024² par couche, 15 textures, offset
> `camera × plan`), caméra centrée joueur (`camera_for`), triangles texturés
> (`draw_triangle_texture` — voir note ci-dessous), formes (`draw_shape`),
> minimap, poussée (`ejection_flow`), débris, cargo, HUD. Vérifié sur écran
> X11 (capture) : fond noir, ~400 étoiles, station + vaisseau texturés au
> centre, HUD et cargo affichés.
>
> **Notes** :
> - macroquad 0.4.16 n'a **pas** `draw_triangle_texture` (le plan l'assumait) :
>   il est implémenté via `models::Mesh` + `draw_mesh` (pipeline 2D, texture
>   incluse). `quad_gl` étant privé, c'est le seul accès public aux triangles
>   texturés. Les UV sont repliés par `rem_euclid(1.0)` (équivalent `_seamless`).
> - `reference/assets/meteor_surface_tile.png` est un **JPEG déguisé en .png**,
>   illisible par macroquad (image sans feature jpeg) → copie convertie en PNG
>   dans `assets/` (voir ASSETS.md §4). Les autres textures sont copiées telles
>   quelles.
> - `moving_shape` doit tourner chaque frame (même à vitesse nulle) pour
>   calculer les positions réelles des triangles ; en Phase 3 il sera assorti
>   de la pause.

Rendu (référence) :
- **Fond noir** : `clear(BLACK)`.
- **Étoiles (gros gain de perf)** : au lieu de 100 000 `draw_rectangle` par frame, précalculer
  15 images (une par couche) de points blancs sur fond transparent, ou un vertex buffer statique
  rejoué avec une translation. Simplest : pour chaque couche, une `Texture2D` générée une fois
  (`render_target`), dessinée avec offset `camera * plan`. **Fidèle et 100× plus rapide.**
- **Triangles texturés** : port de `drawTexturedTriangle` : `uv = t.a.x*ratio - tw/2` etc.,
  `ratio = tw / max(w,h)`. Le `_seamless` de QB64 (wrapping de texture) → repli des UV par
  modulo (`rem_euclid`) avant `draw_triangle_texture` (maison).
- **Triangles sans texture** : le code QB64 mappe en dur `(511,511)-(0,511)-(255,0)` de `orange2.png`.
  → mêmes coordonnées repliées sur la vraie taille de la texture (32×32).
- **Débris** : points 1 px — `draw_rectangle(x, y, 1, 1)`.
- **HUD texte** : macroquad `draw_text` (police par défaut ; police 8×16 en Phase 4).
- **Minimap** (option `SHOW_GLOBAL_MAP` activée) : cercles 1 px — `draw_circle`.
- **Bannière du titre** : rendu colonne par colonne en `draw_text` mono (largeur fixe), couleur HSV
  animée (port de `nextRainbowColor`).
- **Poussée** : 3 cercles dégradés (orange/bleu).
- **Cargo** : 5 cercles remplis aux couleurs des éléments.
- `draw_shape_direction` (petit point direction) : seulement en debug — port facultatif.

### Phase 3 — Logique du jeu
- **Génération procédurale des météores** (port de `generateShape` + `generateTriangle` +
  `createTriangle` + `generateVertexOutsideTriangle` + `isTriangleValid` + `isVertexInnerShape` +
  `chooseBorderSegment`) — c'est le cœur « méchant » du portage : à faire avec soin et à tester
  isolément (voir §5). Remplacer la string `pointsUsageIndicator` par un bitmask.
- **Mouvement** : `moving_shape` (avec `fps` réel ou `delta_time` — choisir : le code QB64 utilise
  `60 * valeur / fps` pour rendre le jeu indépendant du FPS ; en Rust utiliser `delta_time` : équivaut
  à `valeur * 60 * dt`).
- **Collisions** : AABB large phase (par forme, `x_dist <= sum_radius && y_dist <= sum_radius`),
  puis SAT triangle/triangle (`triangles_collide`). **Amélioration permise** : spatial hash (grille
  de 64 px) pour la phase large → O(n) au lieu de O(n²). Garder la même sémantique de résultat.
- **Résolution** : boucle sur les triangles `collid` — ramassage gemme, destruction, débris,
  gemmes, `compute_shape_center` une fois par forme, messages.
- **Accostage** : `|player.pos - station.pos| < 5` → UI « UNLOAD / CLOSE » (remplace `windowUtils_choiceBox`).
- **Caméra** : `cam.x = W/2 - (player.pos.x + player.center.x)` normalisée au monde.
- **Compteurs** : `meteors_destroyed`, `bullets_fired`, `bullets_lost`, `cargo_qty`, `fps` réel.

### Phase 4 — Audio et UI
- Sons : tir (`mis4`), explosion (`exp11..20`, volume selon distance), gemme (`gem1`), boucle fond
  (`bruitDeFond`), musique titre (`music1`, option), boucles moteur avant/arrière (`fffff` ×2).
  NB : `NO_MUSIC = YES` → la musique du titre n'est pas jouée dans le build actuel.
- UI d'aide (remplace `windowUtils` / `help`) : petit overlay plein écran avec la liste des touches
  (P, S, T, A, D, F, G, K) + bouton CLOSE, ou simple affichage jusqu'à une touche. `windowUtils` est
  une lib externe simple (33+352 lignes) : inutile de la porter telle quelle.
- Messages (`sendMessage`/`drawMessage`) : 3 lignes de texte décalées avec délais (0.5 s puis 5 s),
  centrées en bas de l'écran.

### Phase 5 — Parité et validation
- Comparer : tailles/positions des météores générés (même seed → mêmes formes), collisions,
  HUD, comportement au clavier.
- Valider les constantes (§6 d'ANALYSE.md) une à une.
- Perf : objectif ≥ 200 FPS en release ; profiler si besoin (le seul point chaud potentiel est le
  rendu des étoiles → résolu par les textures précalculées).

## 2. Mapping fichier QB64 → module Rust (proposé)

| QB64 | Module Rust | Contenu |
|---|---|---|
| `point_type.bas` | `src/geom.rs` | `Point`, `dot`, `rotation`, wraps monde/plan, égalité, `generate_vertex_outside` |
| `world_type.bas` | `src/geom.rs` | `World`, `define_world` |
| `segment_type.bas` | `src/geom.rs` | `Segment`, `segments_intersect` |
| `triangle_type.bas` | `src/geom.rs` | `Triangle`, `triangles_collide` (SAT), `is_vertex_in_triangle`, `create_triangle`, `generate_triangle`, `is_segment_shared` |
| `shape_type.bas` | `src/shape.rs` | `Shape`, `free_shape`, `resolve_elastic_collision`, `detect_collision`, `moving_shape`, `compute_real_positions`, `compute_shape_center`, `get_border_segments` (caché !), `is_triangle_valid`, `is_vertex_in_shape`, `choose_border_segment`, `meshes_to_shape`, `resize_shape`, `create_specific_shape` |
| `context_type.bas` / `player_type.bas` | `src/config.rs` + `src/state.rs` | constantes + `GameState`, `Player` |
| `garbage_type.bas` | `src/garbage.rs` | `Garbage`, `generate_garbages`, `moving_garbage`, `draw_garbage` |
| `meteorsMining.bas` (point d'entrée, génération, rendu, titre) | `src/main.rs` + `src/generate.rs` + `src/render.rs` + `src/title.rs` | init, `generate_shape`, `draw_shape`, `draw_textured_triangle`, `draw_triangle`, `ejection_flow`, `next_rainbow_color`, `create_shape`, `create_alien/station/gem`, `fire_bullet`, `prepare`, `title_loop` |
| `mainLoop.bas` | `src/game.rs` | `main_loop` (input, physique, collisions, résolution, rendu HUD) |
| `windowUtils.*` | `src/ui.rs` | aide + boîte UNLOAD/CLOSE (réimplémentation) |
| `assets/*.bas` (meshes) | `src/mesh.rs` ou données statiques | données `station`, `alien`, `player`, `bullet`, `gem` (voir ASSETS.md) |

## 3. Mapping des fonctions critiques (signature → Rust)

- `normalizeWorldPosition(p, world)` → `p.normalize_world(&world)` (modifie p).
- `movingShape(shape, triangles, world, fps)` → `moving_shape(shape, triangles, world, dt)`.
- `computeRealPositions(t, p, axe, angle)` → rotation des sommets locaux autour de `axe` puis
  translation par `p` ; recalcule `realMin/realMax`.
- `computeShapeCenter(shape, triangles)` → centre = moyenne des centroïdes des triangles vivants ;
  rayon = max(`hypot(tri.center - targetCenter) + t.hauteur`) ; bbox topLeft/bottomRight ;
  `width/height` = bbox ; `ratio` pour la texture.
- `trianglesCollide(A, B)` → SAT sur les axes des 2 triangles (projections via `dot`).
- `detectCollision(shapeA, shapeB, triangles)` → double boucle, AABB triangle d'abord, puis SAT ;
  pose `collid`/`collidBy` des 2 côtés.
- `resolveElasticCollision(a, b)` → polaire → cartésien → normale/tangente → masses (nb triangles) →
  → recomposition polaire (direction ramenée dans [0, TAU[).
- `generateShape(...)` → boucle : premier triangle `generateTriangle` + `n-1` ajouts sur bords
  libres via `chooseBorderSegment` (bitmask) + `isTriangleValid` (intersections de segments) +
  `isVertexInnerShape` (point-dans-triangle barycentrique).
- `drawTexturedTriangle(t, shape, camera, ...)` → `draw_triangle_texture` avec UV locaux.
- `titleLoop` → boucle d'écran titre (étoiles défilantes + bannière).

## 4. Remplacer les idiomes QB64 par l'équivalent Rust

| QB64 | Rust |
|---|---|
| `redim _preserve arr(n)` | `vec.push(...)` / `vec.resize` |
| tableau indexé par plage `first..last` | `Vec<Shape>` + indices (ou `Arc` partagé des triangles) |
| `pointsUsageIndicator` (string) | `u32` bitmask : bit `i` = bord `i` utilisé ; `choose_border` = `trailing_zeros(!mask)` aléatoire |
| `_iif(cond, a, b)` | `if cond { a } else { b }` |
| `_hypot`, `_atan2` | `f64::hypot`, `f64::atan2` |
| `_rgba32(r,g,b,a)` | `Color::from_rgba(r, g, b, a)` (attention alpha : QB64 32 bits = AARRGGBB) |
| `_MapTriangle _seamless ... _smooth` | `draw_triangle_texture` (UV) ; wrap via `TEXTURE_WRAP` ou shader |
| `circle(x,y),r,c` / `paint` | `draw_circle` / `draw_circle` rempli (rayon - 1) |
| `line (x,y)-(x,y), c` (1 px) | point 1 px (ou buffer de points) |
| `_printstring (x,y), s` | `draw_text(s, x, y, size, color)` (police mono) |
| `locate r, c : print` | position calculée `(c-1)*8, (r-1)*16` (police 8×16) |
| `_sndopen/_sndplay/_sndloop/_sndvol` | `quad-snd` (WAV) ou `rodio` (OGG) — voir §0 |
| `_fullscreen , _smooth` | zoom plein écran : render target 960×540 + vrai plein écran EWMH (`set_fullscreen` + wmctrl, voir §4.1) |
| `_limit fps` / `timer` | boucle 60 FPS via `delta_time` / `get_time` |
| `INP(96)` scan codes | macroquad `is_key_down(KeyCode::Up/...)` (pas besoin de scan codes) |
| `TAU = 8*atn(1)` | `std::f64::consts::TAU` |
| `_width(tex)` / `_height(tex)` | `texture.width()` / `texture.height()` |

### 4.1 Plein écran = zoom (touche F)

Le plein écran est un **vrai plein écran X11/EWMH** (`_NET_WM_STATE_FULLSCREEN`, ex `wmctrl -b
add,fullscreen`) : la fenêtre couvre l'écran sans décorations, et le contenu 960×540 est zoomé
(letterbox) pour la remplir — même contenu, juste plus grand.

- **Entrée** : `set_fullscreen(true)` de miniquad (ClientMessage `_NET_WM_STATE` ADD, standard
  EWMH — ne détruit ni ne recrée la fenêtre, le contexte GLX survit ; les gels observés
  autrefois étaient la boucle `keys_pressed`, voir §6). Le WM agrandit la fenêtre à la taille
  de l'écran.
- **Sortie** : miniquad 0.4.11 ne peut PAS sortir du plein écran sur X11 (TODO dans
  `linux_x11.rs` — `set_fullscreen(false)` envoie un ADD avec un atome vide, sans effet) →
  `render::toggle_fullscreen` complète par `wmctrl -r … -b remove,fullscreen` si présent,
  sinon par un simple `request_new_screen_size(960, 540)` (avec un WM non EWMH, la fenêtre
  resterait plein écran).
- Toute la vue 960×540 est rendue dans un **render target** (`render_target(960, 540)`, caméra
  `Camera2D::from_display_rect` + `render_target`, ex l'exemple officiel `letterbox.rs` de
  macroquad), puis affichée étirée dans la fenêtre (`draw_texture_ex` avec `flip_y: true` —
  le render target est stocké à l'envers).
- En fenêtré : affichage 1:1. En plein écran : la vue est étirée pour remplir la fenêtre
  (letterbox, `zoom_rect`/`zoom_scale`/`draw_zoomed` dans `src/render.rs`). Même contenu,
  juste plus grand.
- **Souris** : les boîtes (accostage UNLOAD/CLOSE, aide CLOSE) convertissent la position
  fenêtre en coordonnées jeu via `mouse_to_game()` (inverse du zoom) — sinon les clics
  seraient décalés en plein écran.
- **Piège macroquad** : ne jamais faire `continue` (ni boucler sans `next_frame`) en attendant
  une touche — `keys_pressed` n'est vidé qu'à `end_frame`, atteint seulement quand la
  coroutine rend la main à `next_frame`. Un `continue` sur F reteste `is_key_pressed(F)`
  à l'infini : boucle sans rendu, cadre figé (et ~600 `clock_nanosleep`/s du pacing).
  `title_loop` cède donc une frame (`next_frame().await`) après avoir traité F, comme
  l'original qui relit `inkey$` (consommant) à chaque itération. Idem : `clear_input_queue()`
  après le titre pour que la touche de lancement ne soit pas revue par la première frame de jeu.

## 5. Stratégie de test pendant le portage

- **Unit tests Rust** sur `geom.rs` : SAT, intersection de segments, point-dans-triangle,
  `normalize_world` (rebouclage), `rotation`, génération d'un météore (déterministe avec seed fixe :
  forme valide = triangles connexes, pas d'intersection, nb de bords cohérent).
- **Snapshot visuel** : comparer une frame du jeu QB64 (capture d'écran) avec le rendu Rust
  (même seed de météores) pour valider l'aspect.
- **Test de parité** : reproduire la séquence d'entrée (accélérer, tirer, détruire un météore)
  et vérifier les compteurs (`meteors_destroyed`, `cargo_qty`…).
- **Perf** : `cargo build --release` + mesure FPS affichée au HUD (déjà prévu par le jeu).

## 6. Pièges spécifiques au portage (checklist)

- [ ] **Y vers le bas** : toute la géométrie suppose `y` croissant vers le bas (convention écran).
      Ne pas « corriger » : reproduire à l'identique (y − sin(direction)…, rotation standard).
- [ ] **Monde torique** : chaque position dessinée et chaque déplacement passe par le wrap
      (formes, caméra, débris, points de dessin).
- [ ] **`shapes[0]` joueur / `shapes[1]` station** : expliciter ces indices, ne pas les casser en
      réutilisant des formes mortes (l'ordre de création dans `prepare` détermine les indices).
- [ ] **Alpha des couleurs** : QB64 `_rgba32` = AARRGGBB ; macroquad = RGBA. Convertir !
- [ ] **`_MapTriangle _seamless`** : répète la texture ; sans wrap, les UV hors [0,1] se répètent
      aussi dans macroquad (`draw_triangle_texture` n'accepte que des UV [0,1]) → décaler par
      modulo (`fract`) avant de passer les UV, c'est l'équivalent `_seamless`.
- [ ] **Déterministe vs aléatoire** : QB64 utilise `rnd` (seed via `randomize timer`) ; en Rust,
      utiliser un PRNG seedé (rand_chacha) — le seed aléatoire au lancement suffit.
- [ ] **`fps` dans les formules de mouvement** : `60*valeur/fps` en QB64 devient `valeur*60*dt` en Rust.
      Vérifier que la vitesse perçue est identique à 60 FPS.
- [ ] **`getBorderSegments`** : ne PAS recalculer par frame (perf) ; calculer au changement de forme.
- [ ] **Débris et balles hors limites** : même règle `DRAW_*` (±100 autour de la vue) pour la
      suppression des balles et le filtrage de dessin (`inner_draw_limit`).
- [ ] **Pause** : geler déplacements ET collisions, mais continuer à dessiner (et à lire l'input).
- [ ] **`windowUtils`** : remplacer (UI maison) — ne pas tenter de la porter en Rust.
- [ ] **Conversion OGG** : si `quad-snd` est retenu, convertir les OGG en WAV (voir ASSETS.md §4).
      Volume des explosions = `(1 - dist/hypot(W,H))^3`, boucle moteur = son en boucle.
- [ ] **`continue` sur une touche = gel** : ne jamais boucler sur `is_key_pressed` sans passer
      par `next_frame` (le keypress n'est consommé qu'à la fin de frame) — voir §4.1.
- [ ] **Fan vs bande glissante dans `meshesToShape`** : l'original fait glisser les deux points
      (`p1 = p2: p2 = p3`) → triangles consécutifs (1,2,3),(2,3,4)… qui dessinent l'anneau de la
      station. Un éventail fixe depuis `pack[0]` (bug de portage, corrigé au jalon M6) remplit
      le trou central : la station devient un foutoir de triangles au lieu d'un anneau.
- [ ] **UV de la station (`station.png`)** : la texture est un anneau fin (bord intérieur UV
      ~0.34, extérieur ~0.5) plus étroit que la bande du mesh (rayon 90-163). À l'échelle
      normale (÷320), les dents cardinales (rayon 160 → UV 0.0/0.5) tombent sur le pixel vide
      du bord → anneau troué à droite (0°) et en bas (90°). Correction : mapping radial
      (`STATION_UV_*` dans config.rs) qui compresse la bande du mesh dans la bande pleine de
      la texture. NB : l'original était dégradé ici aussi — `createStation` appelle
      `computeShapeCenter shapes(shapeId)` (variable indéfinie = 0) → la largeur de la station
      n'était jamais calculée → ratio UV divisé par 0.
- [ ] **Étoiles quasi invisibles/absentes** : deux pièges dans `draw_stars` — (1) les étoiles font
      1 texel dans la tuile 1024² : le filtre **linéaire** (défaut) échantillonne entre les texels
      (offsets de caméra fractionnaires) et écrase la luminosité (~1 au lieu de 127-255) →
      `set_filter(Nearest)` sur les tuiles ; (2) la boucle de tuiles doit partir de
      `offset - tile` (pas `offset`) sinon la zone avant l'offset — souvent la moitié de l'écran,
      voire tout l'écran pour les plans à grand offset — reste sans étoiles. Les deux corrigés
      au jalon M6 (vérifié : couverture 65/65 cellules, luminosité 127-255).

## 7. Jalons (ordre de livraison)

1. **M1** ✅ : fenêtre + fond noir + étoiles (15 couches précalculées) + vaisseau immobile + caméra.
2. **M2** ✅ (août 2026) : déplacement du vaisseau (3 modes portés, `DIRECTIONAL` actif comme
   l'original), monde torique, caméra centrée, pause (P, gèle déplacements + collisions, rendu et
   input vivants), plein écran (F). `src/game.rs` (`update`, `player_controls`, `thrust_vector`),
   20 tests verts. Vérifié à l'écran : dérive de la station (mouvement), flamme orange,
   pause (monde figé, seul le FPS du HUD change), plein écran 960×540 ↔ 1920×1080
   (fenêtré à l'époque ; vrai plein écran EWMH depuis, voir §4.1).
   NB : `fps` de l'original (mesuré 1×/s) est remplacé par `get_fps()` ; la formule
   `60*valeur/fps` est devenue `valeur*60*dt` (indépendante du FPS, cf. §6).
   Limite de boucle ajoutée : pacing manuel à `ATTEMPT_FPS` (600) — macroquad 0.4 n'a ni
   `set_target_fps` ni vsync configurable (miniquad 0.4.11). Mesure : sans pacing le FPS
   réel est déjà ~60 sur cette machine (vsync pilote GLX) ; le cap borne le FPS à 600 sur
   les machines sans vsync, comme le `_limit` de l'original.
3. **M3** ✅ (août 2026) : météores en jeu. `src/game.rs` : touches **A** (génération
   automatique, ex `autoGenerateShape%`) et **G** (météore à `VIEWPORT_WIDTH/4` à droite du
   vaisseau, immobile), détection de collisions par paires (pré-filtre de distance + SAT,
   ex `detectCollision`), choc élastique (ex `resolveElasticCollision`, sans élastique pour
   gemme/vaisseau/météore ni station), résolution (destruction de triangles, `life` des
   formes, messages « YOUR SPACESHIP IS DAMAGED… », débris via `generateGarbages`, recalcul
   du centre, compteurs `meteors_destroyed`/`max_meteor_shapes` pour les balles — M4),
   comptage des formes vivantes + nettoyage des formes « oubliées », génération auto à 5 %
   par frame (`rnd > 0.95`) tant que `aliveShapes < maxMeteorShapes`. `src/render.rs` :
   `draw_message` (file de messages, 3 lignes en bas, opacités 0x70/0xA0/0xFF).
   La caméra est désormais renvoyée par `update` (calculée après la résolution, comme
   l'original). Fidélité : pause ne gèle QUE les déplacements des formes (débris,
   collisions et génération auto continuent — comportement exact de l'original).
   Bug corrigé : `t.element` était borné à `elements.len()+1` (hors bornes) au lieu de
   `int(rnd * (ubound+1))` = 0..len-1. 25 tests verts, vérifié à l'écran : météores G à
   x≈720, génération auto (météores qui dérivent et explosent contre la station), messages
   verts affichés, débris éparpillés après impact.
4. **M4** ✅ (août 2026) : tirs, gemmes, cargo, accostage + UNLOAD. `src/game.rs` :
   **tir** (Shift gauche/droit dans les 3 modes, ex `case 42, 54` ; cooldown `fire` 1/3 s,
   `bullets_fired++`, `fire_bullet`), **suppression des balles hors zone de dessin**
   (`bullets_lost` par triangle, ex « deletes bullets outer of draw area »), **accostage**
   (ex « detect return to the base » : à < 5 px de la station → docké, vitesse 0, message
   « YOU ARE DOCKED AT THE STATION », boîte de choix UNLOAD/CLOSE). `src/render.rs` :
   `choice_box_layout`/`draw_choice_box` (port de `windowUtils_choiceBox` : fenêtre 300×120
   centrée, fond 0xD01AB2FF, bordure 0xFF1AB2FF, titre 0xFF99DFFF, boutons avec hover
   0xFFFFFFFF ; `Rect::contains` + `is_mouse_button_pressed` pour le clic). Les gemmes et
   le cargo étaient déjà câblés en M3 (ramassage vaisseau → `elements[i].count++`,
   `cargo_qty++`, message « LOADING BAY FULL » ; balle → `createGem` si élément).
   Fidélité : la boîte gèle le jeu (boucle bloquante de l'original) et, comme l'original,
   **le choix UNLOAD/CLOSE est ignoré** (`r%` non utilisé) — le cargo est vidé de toute
   façon à la frame suivante ; typo « YOU ARE LIVING THE STATION » conservée. 32 tests
   verts, vérifié à l'écran : balles rouges, météore détruit (REPUTATION 0→1), boîte
   DOCK STATION affichée (fond/bordure/titre) et fermée au clic, jeu dégelé après.
5. **M5** ✅ (août 2026) : écran titre, aide, HUD complet et minimap.
   `src/title.rs` (nouveau) : `title_loop` (port de `titleLoop` — bannière « METEORS
   MINING » en ASCII art 8×125 avec couleurs arc-en-ciel rotatives `nextRainbowColor`,
   étoiles avec caméra qui dérive, invites ; F = plein écran, toute autre touche =
   lancement). `src/render.rs` : `help_box_layout`/`draw_help_box` (port de `windowUtils_help` :
   fenêtre 320×240 centrée, 8 libellés de touches, bouton CLOSE en bas à gauche, position
   souris en direct — la touche T est listée mais non implémentée, comme l'original où le
   bloc est commenté), `draw_info` (port de `showInfo` : keycode, génération auto, compteurs
   shapes/triangles/garbages = `ubound` = `len-1`, formes/triangles vivants, niveaux des
   éléments), et mode **D** dans `draw_shape` (port de `options = "D"` : id:premier,dernier
   + vie/bords de chaque triangle, avec `get_border_segments` recalculé comme l'original).
   `src/game.rs` : touches **S** (aide, gèle le jeu comme la boucle bloquante de l'original),
   **D** et **I** (toggle debug), `qb_keycode` (conversion KeyCode macroquad → keycode QB64
   `inp(96)` : ASCII lettres, 72/75/77/80 flèches, 42/54 shifts), `state.last_keycode`.
   `src/main.rs` : `title_loop` avant la boucle de jeu ; `show_data` passé à chaque
   `draw_shape` ; aide et info dessinées par-dessus le jeu. La minimap (SHOW_GLOBAL_MAP)
   et les messages existaient déjà (Phase 2 / M3). **Plein écran = zoom** (§4.1) : vue
   rendue dans une texture 960×540 puis étirée (fenêtré 1:1, F → 1920×1080 zoomé, même
   contenu juste plus grand ; F sur le titre bascule et reste, comme l'original).
   32 tests verts, vérifié à l'écran :
   bannière arc-en-ciel animée sur fond d'étoiles (2743 px entre 2 frames), lancement au
   clavier, fenêtre d'aide (fond 0xD01AB2FF, texte 0xFF99DFFF, position souris en direct),
   fermeture au clic sur CLOSE, modes D et I affichés puis désactivés.
6. **M6** : parité/perf finale (release, 60 FPS garantis), tests unitaires verts.
