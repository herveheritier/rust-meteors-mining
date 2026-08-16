# Manifeste des assets

> Tous les assets de `reference/assets/` (copie à l'identique de l'original).
> Ce document indique **qui** utilise **quoi**, pour que le portage sache exactement
> quels fichiers charger et lesquels sont inutiles (alternatives commentées dans le code).

## 1. Textures (chargées par `_loadimage` dans `meteorsMining.bas`)

| Fichier | Variable QB64 | Dimensions | Usage |
|---|---|---|---|
| `orange2.png` | `txtr&` | 32×32 | **Texture par défaut** : tous les triangles sans texture propre (`drawTriangle`). Le code mappe en dur `(511,511)-(0,511)-(255,0)` dans cette texture (carré 512 virtuel, peu importe la taille réelle). |
| `vaisseau.png` | `playerTexture&` | 512×512 | **Vaisseau joueur** (`shape.texture` du joueur dans `prepare`). |
| `meteor_surface_tile.png` | `meteorTexture&` | 1254×1254 | **Météores** (`shape.texture` dans `createShape`). Texture tuilable. |
| `station.png` | `stationTexture&` | 1163×1174 | **Station** (`shape.texture` dans `createStation`). |
| `whaoo.png` | commenté | 512×512 | Alternative vaisseau (`'playerTexture& = ...`) — **non utilisée**. |
| `meteor16x16.jpeg`, `meteor32x32.jpeg`, `meteor_surface_tile.jpg` | commentés | 1254×1254 | Alternatives météore — **non utilisées** (sauf `meteor_surface_tile.png`). |
| `untitled.png`, `metalRayures.png`, `meteor_reflets_bleu.png` (+`.jpg`) | commentés | 16×16 / 256×256 / 512×512 | Alternatives station — **non utilisées**. |
| `bandes.png`, `orange.png`, `pixil-frame-3.png`, `station16x16.png`, `station.jpeg` | — | 256 / 32 / 512 / 16 | **Non référencés** par le code actuel — conservés par sécurité, inutiles au portage. |

### Rendu des triangles texturés (rappel)

- `drawTexturedTriangle` (météores, vaisseau, station) : source UV calculée depuis la géométrie
  locale du triangle :
  ```
  ratio = tw / max(shape.width, shape.height)        // tw = largeur de la texture
  u = t.a.x * ratio - tw/2 ; v = t.a.y * ratio - tw/2   (idem pour b, c)
  ```
  puis mapping `_MapTriangle _seamless ... _smooth` vers les sommets écran (après caméra + wrap).
- `drawTriangle` (sans texture) : `_MapTriangle (511,511)-(0,511)-(255,0)` depuis `orange2.png`.

## 2. Sons (chargés par `_sndopen`)

| Variable | Fichier | Volume | Usage |
|---|---|---|---|
| `sh1&` | `mis4.ogg` | 1.0 | **Tir de balle** (`_sndplay sh1&` à chaque tir). |
| `sh2&` | `exp7.ogg` | 0.5 | **Non joué** dans le code actuel (héritage). |
| `sh3&` | `Retro Blop 07.wav` | — | **Commenté** (ligne désactivée). |
| `sh4&` | `exp7.ogg` | 0.05 | **Non joué** dans le code actuel (héritage). |
| `sh5&` | `gem1.ogg` | 0.05 | **Ramassage d'une gemme** (cargo). |
| `sh6&` | `bruitDeFond.ogg` | 1.0 | **Boucle de fond** pendant le jeu (`_sndloop sh6&` en début de `mainLoop`). |
| `sh7&` | `music1.ogg` | 0.1 | **Musique de l'écran titre** (seulement si `NO_MUSIC <> YES` — actuellement **désactivée**). Touche `M` pour la couper/relancer. |
| `sh8&` | `fffff.ogg` | 1.0 | **Boucle de poussée moteur** (avancer) — jouée en boucle tant que `player.thrusted`. |
| `sh9&` | `_sndcopy(sh8&)` | 1.0 | **Boucle de poussée inversée** (reculer) — tant que `player.revertThrusted`. |
| `shexp(0..9)` | `exp11.ogg` … `exp20.ogg` | variable | **Explosions** : `s% = int(rnd*10)`, volume `v = (1 - dist / hypot(960+3000, 540+3000))^3` (dist = distance au joueur). |

Les OGG : utilisables tels quels avec `rodio` en Rust, ou à **convertir en WAV** si l'on reste sur
`quad-snd` de macroquad (commande : `ffmpeg -i in.ogg -c:a pcm_s16le out.wav`).

## 3. Meshes et données inline (déclarés en `data` dans le code QB64)

Ces données sont **dans le code source**, pas dans des fichiers. Le portage doit les traduire en
données statiques Rust (constantes).

### Format `meshesToShape` (utilisé par station, alien)
```
data <nbPacks>, <taillePack1>, <taillePack2>, ...        ' 1re ligne
data <x1>,<y1>, <x2>,<y2>, <x3>,<y3>, ...                ' points d'un pack
```
Chaque pack est un **éventail de triangles** : les points sont lus 2 à 2 puis 1 par 1 →
triangles `(p1,p2,p3)`, `(p2,p3,p4)`, … (soit `taille - 2` triangles par pack).

### Meshes existants

**Station** — `reference/assets/station.bas` (32 triangles, taille ~300×320) :
```
data 1,34
data -150,-60, -110,-20, -160,0, -110,20, -150,60, -90,60, -120,110, -60,90,
     -60,150, -20,110, 0,160, 20,110, 60,150, 60,90, 120,110, 90,60, 150,60,
     110,20, 160,0, 110,-20, 150,-60, 90,-60, 120,-110, 60,-90, 60,-150,
     20,-110, 0,-160, -20,-110, -60,-150, -60,-90, -120,-110, -90,-60, -150,-60, -110,-20
```
Station : `resizeShape 1` (pas de redimensionnement), `whoIam = STATION`, couleur `&HFF808000`,
position (0,0), rayon 36, texture `station.png`.

**Alien** — `reference/assets/gripper-meshes.bas` (4 packs : 16+16+5+8 points → 14+14+3+6 = 37 triangles) :
```
data 4,16,16,5,8
pack1 (16 pts) : 170,50 140,120 110,90 130,140 60,100 10,160 20,100 -40,110 0,80
                -60,40 0,0 -110,40 -140,0 -140,110 -170,0 -200,0
pack2 (16 pts) : 170,-50 140,-120 110,-90 130,-140 60,-100 10,-160 20,-100 -40,-110 0,-80
                -60,-40 0,0 -110,-40 -140,0 -140,-110 -170,0 -200,0
pack3 (5 pts)  : -180,40 -200,0 -250,40 -290,0 -320,40
pack4 (8 pts)  : -180,-40 -200,0 -250,-40 -290,0 -320,-40 -320,40 -370,-80 -370,80
```
Alien : `resizeShape 1/5`, `whoIam = ALIEN`, couleur `&H80FFFF00`, position (100,100), rayon 10.
Créé par la touche `C`.

### Données inline (dans le code)

**Joueur** (`prepare`) : 1 triangle — `data 1, -10,-10, -10,10, 10,0, 0`
→ triangle `(-10,-10) (-10,10) (10,0)`, élément 0. Texture `vaisseau.png`, couleur `&H80FFFFFF`.

**Balle** (`fireBullet`) : 1 triangle — `data 1,-2,-2, -2,2, 2,0`
→ triangle `(-2,-2) (-2,2) (2,0)`. Couleur `&HFFFF0000`. Tirée depuis
`player.position + player.targetCenter`, `direction = -player.orientation`, `velocity = player.velocity + 2`.

**Gemme** (`createGem`) : `data 1,4` puis `data 2,-2, -2,-2, -2,2, 2,2`
→ pack de 4 points `(2,-2) (-2,-2) (-2,2) (2,2)` → 2 triangles. `whoIam = GEM`,
couleur = couleur de l'élément, hérite position/vitesse/direction de la forme source.

**Éléments** (`prepare`) : `data 3, 0,WATER,&HFF8080FF, 1,IRON,&HFFC0C0C0, 2,GOLD,&HFFD0D010`
→ 3 éléments (id, nom, couleur). Compteurs initialisés à 0.

## 4. Copie convertie pour le portage (`assets/`)

> Le dossier `assets/` à la racine du projet est la copie **utilisée par le portage
> Rust** : `reference/` reste la référence non modifiée.

| Fichier | Origine | Statut |
|---|---|---|
| `orange2.png` | `reference/assets/orange2.png` | copie directe (32×32 RGBA) |
| `vaisseau.png` | `reference/assets/vaisseau.png` | copie directe (512×512 RGBA) |
| `meteor_surface_tile.png` | `reference/assets/meteor_surface_tile.png` | **converti** : le fichier d'origine est un **JPEG déguisé en .png** (1254×1254), illisible par macroquad (crate image sans feature jpeg) → converti en vrai PNG avec `convert` |
| `station.png` | `reference/assets/station.png` | copie directe (1163×1174 RGBA) |

Les sons OGG (Phase 4, voir `docs/PORTAGE.md` §4.2) sont copiés **tels quels** dans
`assets/` — `quad-snd`/miniaudio (feature `audio` de macroquad) décode l'Ogg Vorbis
directement, aucune conversion nécessaire : `mis4.ogg`, `gem1.ogg`, `exp11.ogg`…`exp20.ogg`,
`fffff.ogg`, `bruitDeFond.ogg`, `music1.ogg` (15 fichiers, sources dans `reference/assets/`).

## 5. Fichiers de mesh « exemple » (non utilisés)

| Fichier | Contenu | Statut |
|---|---|---|
| `meshMeteorMining.data` | `data 2,3,6` + points (2 packs de 3 et 6 pts → 1 et 4 triangles) | **Non référencé** par le code — exemple de format |
| `meshMeteorMining.msh` | mêmes triangles, format texte `"x,y;x,y;..."` par ligne | **Non référencé** — exemple de format export |

Conservés pour référence ; **pas besoin de les porter**.

## 6. Récapitulatif « chargés au démarrage »

À l'initialisation (`meteorsMining.bas`), le jeu charge **4 textures** (orange2, vaisseau,
meteor_surface_tile, station) et **16 sons** (mis4, exp7×2, gem1, bruitDeFond, music1, fffff,
exp11..exp20, Retro Blop 07 commenté). En Rust : charger au démarrage dans une struct `Assets`
(`Texture2D` + sons), partagée avec l'état du jeu.
