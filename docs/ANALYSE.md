# Analyse de l'application d'origine — « Meteors Mining » (QB64)

> Ce document décrit le jeu tel qu'il est dans `reference/source/`. Il sert de **spécification**
> au portage Rust. Tout est recopié fidèlement dans `reference/` ; ce document résume et structure
> l'information pour éviter de relire les 2 800 lignes à chaque décision.

## 1. Le jeu

Jeu d'arcade 2D de minage spatial :

- Le joueur pilote un vaisseau (`shapes(0)`) dans un **monde torique** (les bords se rebouclent).
- Des **météores** (formes polygonales générées procéduralement) dérivent dans le monde.
- On les détruit au canon (`Shift` gauche/droit) : chaque triangle détruit génère des **débris**
  et parfois une **gemme** (si le triangle contenait un élément).
- On récupère les gemmes (3 éléments : WATER, IRON, GOLD) dans la **cargaison** (5 emplacements).
- On **accoste la station** (au centre du monde, position (0,0)) pour décharger.
- HUD : FPS, réputation (météores détruits), précision (tirs), cargaison, messages.
- Écran titre animé (bannière ASCII arc-en-ciel), écran d'aide, pause, plein écran.

## 2. Fichiers sources (ordre d'inclusion dans `meteorsMining.bas`)

| Fichier | Rôle | Éléments clés |
|---|---|---|
| `context_type.bas` | État global + constantes | `context_type` (état jeu, monde, joueur, toutes les constantes) |
| `world_type.bas` | Définition du monde | `world_type`, `defineWorld` |
| `point_type.bas` | Géométrie 2D de base | `point_type`, `setPoint/Min/Max`, `dotProduct`, `normalizeWorldPosition` (rebouclage), `normalizePlanPosition`, `arePointsEqual`, `generateVertexOutsideTriangle`, `rotation` |
| `garbage_type.bas` | Débris | `garbage_type`, `generateGarbages`, `movingGarbage`, `drawGarbage` |
| `segment_type.bas` | Segments | `segment_type`, `checkSegmentsIntersect` |
| `triangle_type.bas` | Triangles | `triangle_type`, `trianglesCollide` (SAT), `isSegmentShared`, `isVertexInnerTriangle`, `createTriangle`, `generateTriangle` |
| `shape_type.bas` | Formes (assemblages de triangles) | `shape_type`, `freeShape`, `resolveElasticCollision`, `detectCollision`, `movingShape`, `computeRealPositions`, `computeShapeCenter`, `getBorderSegments`, `isTriangleValid`, `isVertexInnerShape`, `chooseBorderSegment`, `printTriangle`, `meshesToShape`, `resizeShape`, `createSpecificShape` |
| `player_type.bas` | Joueur | `player_type` (shapeIndex, thrust, thrusted, fire, cargo) |
| `mainLoop.bas` | Boucle de jeu | `mainLoop` (input, physique, collisions, rendu, HUD) |
| `meteorsMining.bas` | Point d'entrée + génération | Init (assets, sons, monde), `generateShape`, `innerDrawLimit`, `ejectionFlow`, `drawShape`, `drawTexturedTriangle`, `drawTriangle`, `drawShapeDirection`, `nextRainbowColor`, `createShape`, `createAlien`, `createStation`, `createGem`, `fireBullet`, `prepare`, `titleLoop`, `showBonus`, `sendMessage`, `drawMessage`, `help` |

## 3. Modèle de données central

### Formes et triangles partagés

Le cœur du modèle : deux tableaux globaux, `shapes()` et `triangles()`.

- Chaque `shape_type` possède `firstTriangleIndex` et `lastTriangleIndex` : sa plage de triangles
  dans le tableau `triangles()`.
- Les triangles morts (`life = 0`) restent dans le tableau ; les formes mortes sont **réutilisées**
  par `freeShape` (cherche une forme morte avec exactement le même nombre de triangles) avant
  d'étendre les tableaux (`redim _preserve`).
- `shapes(0)` = **vaisseau joueur**, `shapes(1)` = **station** (dur en dur dans `mainLoop`).
- `ctx.player.shapeIndex` n'est jamais initialisé explicitement (défaut 0 = joueur) mais est utilisé
  par `ejectionFlow`.

### Les types (résumé des champs)

```
point_type     { x: f64, y: f64 }
world_type     { width, height, minx, maxx, miny, maxy }   (int)
player_type    { shapeIndex, thrust: f64, thrusted, revertThrusted, fire, cargoSize, cargoQty }  (int sauf thrust)
element_type   { id, name[10], color: u32, count }
triangle_type  {
    id, shapeIndex, element, life, collid, collidBy, aShapeBorder, bShapeBorder, cShapeBorder,
    textureBasePosition, angle: f64, hauteur: f64,
    position, a, b, c, center,                 -- géométrie locale (triangles définis dans le repère de la forme)
    realA, realB, realC, realCenter,           -- géométrie monde (après position + rotation)
    realMin, realMax,                          -- AABB monde du triangle
    demibase: point_type
}
shape_type     {
    id, firstTriangleIndex, lastTriangleIndex, life, isCollider, element, showAllParts, whoIam,
    pointsUsageIndicator (string!), width, height: f64, radius, direction, velocity, orientation, rotation: f64,
    position, topLeft, bottomRight, center, targetCenter: point_type,
    shapeColor, texture: u32
}
garbage_type   { position, radius, direction, velocity, orientation, angle, life, rgbaColor }
context_type   { état du jeu + monde + joueur + TOUTES les constantes (voir §6) }
```

**Identifiants `whoIam`** : `METEOR=0, BULLET=1, PLAYER=2, GEM=3, STATION=4, ALIEN=5`.
**Modes de déplacement** : `INERTIAL=0, 4_WAYS=1, DIRECTIONAL=2` (celui utilisé actuellement).

## 4. Boucle de jeu (`mainLoop`)

Ordre exact des opérations à chaque frame :

1. **Timing** : `_limit 600` FPS visés ; mesure du FPS réel 1×/seconde (affiché au HUD).
2. **Input** : `keycode = inp(96)` (scan code clavier) + `k$ = ucase$(inkey$)` pour les lettres.
3. **Commandes** (voir mapping clavier §7).
4. **Contrôles joueur** selon `movingMode` (accélération, rotation, tir).
5. **Sons moteur** (boucles thrust / revert selon `player.thrusted` / `revertThrusted`).
6. **Reset `collid`** de tous les triangles.
7. **Déplacement** des formes (`movingShape`) puis des débris (`movingGarbage`), si pas en pause.
8. **Détection de collisions** : double boucle O(n²) sur les formes ; phase large AABB
   (`xDist <= sumRadius && yDist <= sumRadius`) puis `detectCollision` (AABB par triangle puis SAT).
9. **Résolution des collisions** : boucle sur tous les triangles `collid` :
   - gemme + joueur + place cargo → ramasse la gemme (son `sh5`), `elements[el].count++`, `cargoQty++`
   - triangle détruit → `life = 0`, `shape.life--`, message si le joueur est touché, si un météore est
     détruit par une balle → `meteorsDestroyed++`, bonus « R+1 », `maxMeteorShapes++` (plafonné à 150),
     son d'explosion avec volume selon la distance, génération de 12 débris, gemme si `element > 0`
   - `computeShapeCenter` recalculé une fois par forme touchée (pas par triangle)
10. **Accostage station** : `|player.position - station.position| < 5` → boîte de dialogue UNLOAD/CLOSE.
11. **Caméra** : centrée sur le joueur puis normalisée (monde torique).
12. **Nettoyage** : balles hors zone de dessin → supprimées (`bulletsLost++`).
13. **Génération automatique** : si activée et `aliveShapes < maxMeteorShapes` et `rnd > 0.95` → nouveau météore.
14. **Rendu** (ordre exact) :
    a. fond noir (`cls`)
    b. étoiles (100 000, 15 couches de parallaxe)
    c. formes (météores…), puis joueur séparément
    d. effet de poussée (`ejectionFlow`, 3 cercles)
    e. débris (`pset` 1 px blanc)
    f. HUD cargo (5 cercles colorés)
    g. FPS / réputation / précision (`locate`), messages, infos debug, aide
    h. `_display`
15. **Quitter** : `keycode = 1` (ESC).

## 5. Physique et monde

- **Monde torique** : `normalizeWorldPosition` reboucle `p.x/p.y` dans `[minx..maxx]` / `[miny..maxy]`.
  Toutes les positions (formes, caméra, points de dessin) passent par là.
- **Déplacement** (`movingShape`) : `pos += cos(direction)*60*velocity/fps` (y : `- sin`) ;
  `orientation += 60*rotation/fps` ; `center` interpole vers `targetCenter` au facteur 1/100 ;
  puis `computeRealPositions` : rotation des 3 sommets + centre autour de `(position + center)`
  avec l'orientation, et mise à jour de `realA/B/C`, `realCenter`, `realMin/Max`.
- **Collision élastique** (`resolveElasticCollision`) : masse = nombre de triangles de la forme
  (`last-first+1`) ; choc élastique 1D le long de la normale ; vitesse/direction restent en polaires.
- **SAT triangle/triangle** (`trianglesCollide`) : projection sur les 6 axes (3 par triangle).
- **Couleur de forme** : `_rgba32(127+rnd*128, 127+rnd*128, 127+rnd*128, 64)` — semi-transparente.

## 6. Constantes (valeurs actuelles, portées dans `ctx`)

| Constante | Valeur | Remarque |
|---|---|---|
| `VIEWPORT_WIDTH` / `HEIGHT` | 960 / 540 | taille de la fenêtre et de la vue |
| `EXTERNAL_BORDER` | 1500 | marge autour de la vue = taille du monde hors écran |
| `WORLD_WIDTH` / `HEIGHT` | 3960 / 3540 | = vue + 2×marge |
| `WORLD_MINX` / `MAXX` | -1500 / 2460 | |
| `WORLD_MINY` / `MAXY` | -1500 / 2040 | |
| `DRAW_MINX` / `MAXX` | -100 / 1060 | = monde − marge ± 100 (élargis par le refactor) |
| `DRAW_MINY` / `MAXY` | -100 / 640 | |
| `STARS_COUNT` | 100000 | étoiles |
| `STARS_LAYERS` | 15 | couches de parallaxe |
| `SHAPES_COUNT` | 150 | plafond du nombre de météores |
| `TRIANGLES_IN_SHAPE_MIN/MAX` | 6 / 16 | triangles par météore |
| `TRIANGLE_BASE_MIN/MAX` | 15 / 40 | base du premier triangle |
| `TRIANGLE_HEIGHT_MIN/MAX` | 11 / 22 | hauteur des triangles ajoutés |
| `ATTEMPT_FPS` | 600 | limite FPS de la boucle QB64 (le rendu plafonne bien plus bas) |
| `FULL_SCREEN` | 0 | faux |
| `TAU` | 2π | constante globale |

Constantes de gameplay dérivées (dans le code) :
- `maxMeteorShapes` initial = 15, +1 par météore détruit, plafond `SHAPES_COUNT` (150).
- Joueur : accélération `60*0.05/fps`, rotation `60*(TAU/210)/fps`, cooldown tir `fps/3`, cargo 5.
- Météore : `velocity = 2*rnd`, `direction = TAU*rnd`, `rotation = 0.01 - 0.02*rnd`.
- Explosion : volume `v = (1 - dist / hypot(WORLD_WIDTH, WORLD_HEIGHT))^3`.
- Débris : 12 par triangle détruit, `life = rnd*255 \ 7`, `velocity = v_forme*(1+rnd*3)`, blanc.
- Éléments : 1=WATER `&HFF8080FF`, 2=IRON `&HFFC0C0C0`, 3=GOLD `&HFFD0D010`.
- Station : position (0,0), rayon 36, accostage si distance < 5.

## 7. Mapping clavier (à reproduire en Rust)

Scan codes (`INP(96)`) :
- `72` ↑ accélérer · `77` → tourner droite · `80` ↓ décélérer/retour · `75` ← tourner gauche
- `42` / `54` Shift gauche / droite = tirer
- `1` ESC = quitter

Touches (`INKEY$`) :
- `F1` (chr$(0)+chr$(59)) = aide (`help`, fenêtre windowUtils) — en pratique la touche `S` fait pareil
- `A` = génération auto ON/OFF · `C` = créer un alien · `D` = afficher les données debug
- `F` = plein écran · `G` = générer un météore devant le joueur · `I` = infos debug
- `K` = tuer toutes les formes · `M` = couper la musique · `P` = pause
- `S` = écran d'aide · `T` = dump des triangles dans la console

## 8. Options de compilation ($LET — équivalent à des flags de build)

| Option | Valeur | Effet |
|---|---|---|
| `SHOW_INFOS` | NO | infos par forme (`id-life`) affichées |
| `SHOW_GLOBAL_MAP` | YES | **minimap** : petit point par forme en bas à droite de l'écran |
| `SHOW_RADIUS` | NO | cercles rayon/centre des formes |
| `SHOW_DEBUG` | NO | logs de génération dans la console |
| `NO_MUSIC` | YES | la musique du titre (`music1.ogg`) n'est **pas** jouée |

En Rust : constantes `const SHOW_GLOBAL_MAP: bool = true;` etc. (ou features Cargo).

## 9. Particularités / bugs connus à connaître avant de porter

- `shapes(O)` dans `mainLoop` (touche G) : `O` est une variable non déclarée = 0 → équivaut à `shapes(0)`.
- `deg$()` utilise la variable globale non déclarée `t.angle` → fonction de debug, sans importance.
- `sh2`/`sh4` (exp7.ogg ×2) sont chargés mais **jamais joués** dans le code actuel (héritage).
- `getBorderSegments` (contours des formes) est recalculé à **chaque frame** dans `drawShape` alors
  que les bordures sont stables tant que les triangles vivent → à calculer une seule fois en Rust.
- `pointsUsageIndicator` (string de « 0 »/« 1 ») gère la génération procédurale → remplacer par un
  bitmask/tableau d'entiers en Rust.
- `textureBasePosition` est un chut non initialisé dans `drawTriangle` (bug latent : la ligne
  d'initialisation `if t.textureBasePosition = 0 then ...` est en fait désactivée — le code actif
  utilise des coordonnées de texture en dur `(511,511)-(0,511)-(255,0)`).
- `createStation` référence `shapes(shapeId)` (non déclaré, = 0) puis `shapes(shape.id)` — en pratique
  la station est la 2e forme créée par `prepare`, donc `shapeId`/`shape.id` = 1. À rendre explicite.
- Le monde entier tourne en **double précision** en QB64 ; en Rust on peut rester en `f64` sans souci.
- Le HUD utilise `locate`/`print` (police 8×16) ; le reste du texte utilise `_printstring` (même police).

## 10. Rendus visuels à reproduire fidèlement

- **Étoiles** : 1 pixel, alpha aléatoire `127 + rnd*128`, position `(star + camera) * plan` rebouclée
  (`normalizePlanPosition` avec le monde × plan). Le plan = `(i mod 15) + 1`.
- **Météores** : triangles texturés (`meteor_surface_tile.png`), source UV dérivée des sommets locaux :
  `u = t.a.x*ratio - tw/2` avec `ratio = tw / max(shape.width, shape.height)` ; interpolation `_seamless`
  + filtrage `_smooth`. Couleur semi-transparente quand pas de texture.
- **Vaisseau** : texture `vaisseau.png`, dessiné en dernier (par-dessus les météores).
- **Débris** : pixels blancs 1 px.
- **Poussée** : 3 cercles dégradés (orange `&HFFFFA000` en avant, bleu `&HFF00A0FF` en recul).
- **Cargo** : 5 cercles rayon 5 à `x = 11*i + 5`, `y = 50`, remplis de la couleur de l'élément.
- **Minimap** (option) : cercle 1 px à `(p.x\10 + W/2 - W/20, p.y\10 + H/2 - H/20)`, rouge formes /
  vert joueur (id 0).
- **Titre** : bannière ASCII 8 lignes colorée colonne par colonne (dégradé HSV animé,
  `nextRainbowColor`), défilement vertical des étoiles, info « [F for fullscreen] », « [ESC to quit] »,
  « [Hit other key to launch] ».
