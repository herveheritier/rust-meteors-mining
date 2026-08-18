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
- **Station** : au lancement (et après respawn) le vaisseau est **à quai**
  au centre de la base, **tenu par 4 liens néon** (mire cachée) ; dès qu'on
  démarre (une flèche), les **liens se rétractent** (1,5 s, monde gelé) et le
  vaisseau est libre — hors de la base, ni lien ni cible, et **pas de mire
  tant qu'on quitte l'accostage** : la mire n'est affichée **que lors du
  retour**. Au **retour**, au moment où l'on **franchit la limite extérieure
  de la base en entrant** (après l'avoir franchie en sortant), la **mire**
  **néon** pulsante apparaît au centre (le guide d'accostage, cercle de
  15 px, dessiné sous le vaisseau) et **réagit dans tout le rayon de la
  base** : sa couleur passe **progressivement du rouge au vert** selon la
  distance au centre ET la vitesse (rouge au bord du rayon ou trop rapide,
  vert au centre et presque immobile) ; la distance est au HUD
  (`DOCK DIST: 123` — sans unité — / `DOCK: SLOW DOWN` / `DOCK: IN RANGE` /
  `DOCKED`).
  L'accostage se termine seulement **presque immobile dans la zone** : la
  mire **disparaît** et une **animation de 3 s** (monde gelé) **projette**
  les **4 liens** en diagonale (**NO, SO, SE, NE**) : ils jaillissent de
  l'anneau vers le vaisseau (onde qui court vers lui) et se branchent **près
  de son centre** (l'illusion qu'ils le touchent), puis le pivote vers la
  droite tout en le recentrant **exactement au centre** de la station, puis
  la boîte DOCK STATION s'ouvre (cargo déchargé, vaisseau réparé). Au départ
  (CLOSE), la **tension est relâchée** : les liens se **rétractent en
  ondulant** (une onde court du vaisseau vers l'anneau, l'extrémité libre
  fouette puis retombe — comme un câble qui se rentre), puis le vaisseau est
  libre.
- **Météores en continu** : génération automatique (limite 150) ou à la demande.
- **Minerais dans les météores** : chaque météore contient une quantité de
  minerai (un par triangle minéralisé — or, fer, eau — au départ, plus un
  par gemme absorbée). Le minerai n'est **jamais détruit** quand son météore
  l'est : qu'il soit détruit par un **autre météore** ou par un **missile du
  vaisseau**, ses minerais sont **libérés en gemmes** à sa position. Le seul
  cas de destruction de minerai : un **missile touche directement la gemme**
  (elle est détruite, sans nouvelle gemme). Si un **météor percute une
  gemme**, il l'**absorbe** (elle disparaît, sa quantité de minerai augmente)
  sans être endommagé — les gemmes qu'il a mangées sont récupérables en le
  détruisant (missile ou collision).
- **Cosmonaute de secours** : quand le vaisseau est détruit (jeu libre ou
  Progression), le pilote est **éjecté** — un petit **cosmonaute EVA** (le
  personnage de `assets/cosmonaute.json`, en couleurs par face) apparaît à la
  position du crash et devient le personnage contrôlé : il se dirige **comme
  le vaisseau** mais avec **un seul propulseur** : la poussée est
  **vectorielle** (↑ ajoute la poussée au **vecteur de déplacement** — pour
  changer de direction, d'abord **s'orienter** avec ←/→, puis pousser), pas
  de frein ni de marche arrière ; la caméra, la mire et le HUD le suivent),
  dessiné **au premier plan** — uniquement pendant l'EVA (jamais de
  cosmonaute supplémentaire dans le monde) — avec un **petit propulseur sur
  le dos**
  (flamme animée orange/jaune, vacillante, visible quand il pousse) et des
  **membres animés** : bras et jambes **s'agitent** (bascule autour des
  épaules/hanches) pendant la poussée puis retombent au repos. Il peut
  **ramasser les gemmes** par proximité (même soute que le vaisseau —
  déchargée en minerais à la station). Son **seul objectif** : **rejoindre
  la base** — dès qu'il atteint la zone d'accostage au centre de la
  station, la **récupération** s'anime : un **cordon orange** jaillit de
  l'anneau jusqu'à lui et le **ramène sur l'anneau** (~2,5 s, monde gelé,
  ondulation qui s'affaisse quand la tension monte), puis un **fondu
  enchaîné** (2 s) l'efface pendant que le **vaisseau reconstruit apparaît
  au centre de la station, liens attachés** (la caméra glisse de l'anneau
  vers le centre). En Survival, la destruction reste gérée par les
  vies/bouclier (respawn à la station).
- **Audio** : ambiance, musique, moteur avant/recul, tirs, gemmes et
  explosions à volume selon la distance au vaisseau.

## Compilation et lancement

```bash
cargo run --release
```

La fenêtre 960 × 540 s'ouvre sur l'écran titre — appuyez sur une touche
(autre que F/O/N) pour lancer la partie. **N** y change de scénario (jeu
libre ou Progression, voir « Scénarios » ci-dessous).

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
| O | Écran de paramétrage (aussi accessible depuis l'écran titre) : panneau « MOVING MODE » (3 modes de déplacement en radio-boutons, clic ou flèches ↑/↓ + Entrée), cases MUSIC / AUTO GENERATE / ANTIALIAS, volume (barre horizontale cliquable/glissable) et panneau « GRAPHICS » (RENDER texturé/colorisé/mesh, WINDOW fenêtré/plein écran zoomé/natif, SIZE 960×540 à 1920×1080 — clic = cycle) ; si un réglage exige un redémarrage (anticrénelage), note « RESTART REQUIRED » et bouton RESTART (relance le jeu) ; RESET revient aux défauts des réglages (seule la progression du scénario — minerais, modes payés, réputation — est conservée) ; fermer avec CLOSE ou ESC (le HUD annonce le mode activé s'il a changé) |
| G | Générer un météore près du vaisseau |
| A | Activer/désactiver la génération automatique des météores |
| C | Créer un alien |
| F | Cycler les modes d'affichage : fenêtré → plein écran zoomé → plein écran natif |
| M | Couper/relancer la musique |
| D | Afficher les données des formes (debug) |
| I | Afficher les informations (keycode, compteurs, formes/triangles vivants) |
| ESC | Quitter |
| N / B / 1-3 (écran titre) | Changer de scénario : N suit le cycle (jeu libre → Progression → Survival → jeu libre), B le parcourt en sens inverse, et 1/2/3 sélectionnent directement (1 = jeu libre, 2 = Progression, 3 = Survival) |

Au démarrage, le vaisseau est à la station : éloignez-vous pour commencer à
miner. Revenez dans la zone d'accostage en **ralentissant** : la mire au
centre de la station passe du rouge au vert avec la vitesse (vert = prêt,
`DOCK: IN RANGE` au HUD) pour ouvrir la boîte DOCK STATION
(UNLOAD / REFUEL/REARM / [UPGRADES] / CLOSE) : UNLOAD décharge
la soute, REFUEL/REARM achète carburant + munitions contre minerais,
UPGRADES ouvre l'atelier d'amélioration du vaisseau (scénario Progression),
CLOSE ferme — la boîte reste ouverte après UNLOAD, REFUEL/REARM et les
achats de l'atelier pour tout faire avant de partir.

## Scénarios

Les scénarios (choisis à l'écran titre, touches N/B ou 1-3 — l'écran titre
affiche leurs **règles** (`[ RULES : … ]`, dérivées des données par
`scenario::scenario_rules`, avec les **valeurs chiffrées en surbrillance**
dans la **couleur propre du scénario** — jaune pour Progression, cyan pour
Survival (coûts, vies, bouclier, dégâts, rangs) — pour faire ressortir ce
qui change au basculement ; juste après un changement (N/B/1-3), toute la
ligne **clignote dans cette couleur** ~1,2 s pour attirer l'œil) et la
**progression enregistrée** du scénario (`[ SAVE : … ]`,
minerais/modes/réputation ou vies/bouclier, avec les **valeurs en
surbrillance** dans la couleur du scénario elles aussi —
`scenario::save_summary_segments`)) encapsulent des règles de jeu en
**données +
points d'accroche purs** (`src/scenario.rs`) — la boucle (`game.rs`) ne fait
qu'appeler des fonctions testables sans macroquad :

- **FREE PLAY** (défaut) — le comportement historique : aucun coût, tous les
  modes de déplacement disponibles, carburant et munitions illimités.
- **PROGRESSION** — l'exemple d'économie :
  - le vaisseau démarre en mode **INERTIAL** ; les modes **4 WAYS** (20
    minerais) et **DIRECTIONAL** (50 minerais) se débloquent dans l'écran de
    paramétrage (O) en payant des minerais (affichés à côté du mode) ;
  - les minerais s'obtiennent en minant : chaque gemme déchargée à la station
    vaut selon son élément (or 5, fer 3, eau 2) ;
  - **carburant** et **munitions** sont payants : chaque poussée consomme du
    carburant (moteur éteint, plus de poussée — rotations libres), chaque tir
    une munition ; les pleins s'achètent à la station (10 carburant = 1
    minerai, 5 munitions = 1) via le bouton REFUEL/REARM de la boîte DOCK
    STATION (plus d'achat automatique au déchargement) ;
  - **l'atelier** (bouton UPGRADES de la boîte DOCK STATION — une sorte de
    place de marché/atelier) permet d'acheter contre minerais des extensions
    de vaisseau, persistées avec la progression : **réservoir** (100 de base,
    3 extensions de +50 → 250 max), **chargeur** (30 de base, 3 extensions
    → 70 max) et **soute** (5 emplacements de base, 2 extensions → 10 max) ;
    à l'achat, le réservoir/chargeur repart plein à la nouvelle capacité et
    la soute s'agrandit immédiatement ; le HUD affiche les capacités courantes
    (`FUEL:50/150 AMMO:20/45`) ;
  - la **réputation** croît à chaque astéroïde détruit, d'autant plus que la
    précision de tir est bonne (gain × (1 + 2 × précision)) — affichée au HUD
    avec FUEL / AMMO / MINERALS ; elle débloque des **rangs** (paliers
    affichés au HUD, ex `REPUTATION:37 (ACE)`) : CADET (0) → PILOT (10) →
    VETERAN (25) → ACE (50), chaque palier franchi est annoncé (« RANK UP:
    PILOT ») ;
- **SURVIVAL** — preuve que le système s'étend hors de l'économie : ni
  minerais ni verrous (tous les modes disponibles), mais le vaisseau a des
  **vies** (3) et un **bouclier** qui absorbe les impacts (3 points) ; quand
  il est percé, l'impact suivant détruit le vaisseau — une vie est perdue et
  il respawne à la station (bouclier rechargé + **2 s d'invulnérabilité**, le
  vaisseau clignote), la dernière vie perdue termine la partie (HUD « GAME
  OVER », seule la touche ESC quitte) ; le **multiplicateur de dégâts**
  aggrave chaque impact (bouclier vidé plus vite). Le HUD affiche
  `LIVES:3 SHIELD:3`.

Les coûts, capacités et formules sont des constantes (`FREE_PLAY_SCENARIO`,
`PROGRESSION_SCENARIO`, `SURVIVAL_SCENARIO`) : un nouveau scénario = une
nouvelle constante + les accroches qu'il lui faut. La progression d'une
partie est **persistée** dans le fichier de config (clés `scenario` +
`prog_*`) et restaurée au lancement suivant (le dernier scénario joué reprend
automatiquement) : minerais, modes payés et réputation en Progression
(`prog_minerals`, `prog_modes`, `prog_reputation`), **vies et bouclier en
Survival** (`prog_lives`, `prog_shield` — bornés aux capacités du scénario,
une sauvegarde à 0 vie repart au départ complet) ; chaque scénario n'écrit
que ses propres clés, sans écraser la sauvegarde de l'autre. Le carburant et
les munitions, eux, repartent pleins à chaque lancement.

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
- Réglages persistants (`meteors_mining.cfg`, dossier de configuration
  utilisateur — norme XDG, ex `~/.config/meteors-mining/meteors_mining.cfg`) :
  mode de déplacement, musique (touche M), volume, style de rendu, mode
  d'affichage, définition de fenêtre et anticrénelage — modifiables dans
  l'écran de paramétrage (touche O) ou par les touches M/A, rechargés au
  lancement suivant. NB : la génération automatique des météores (touche A ou
  case AUTO GENERATE) n'est **pas** persistée — elle repart **toujours
  active** à chaque lancement, pour que le monde ne soit jamais vide au
  démarrage. S'y ajoutent le scénario choisi et la progression d'une partie à
  économie (`scenario`, `prog_*`), sauvegardés à chaque changement
  (déchargement, ravitaillement, mode payé, achat à l'atelier, astéroïde
  détruit) et restaurés au lancement (une sauvegarde finale a aussi lieu à
  la sortie du jeu). Le
  RESET de l'écran de paramétrage ne supprime que les clés de réglage — la
  progression du scénario survit. En fenêtré, une définition plus grande que
  960×540 étire la vue (letterbox) ; l'anticrénelage MSAA est appliqué à la
  création de la fenêtre (effectif au lancement suivant).
- Le jeu est testé : `cargo test` (103 tests unitaires — physique, collisions,
  minage, accostage, paramétrage, options graphiques, persistance, scénarios,
  atelier d'amélioration).

## Structure du projet

```
rust-meteors-mining/
├── Cargo.toml              ← projet Rust (macroquad, rand, image)
├── assets/                 ← textures (.png/.jpg), sons (.ogg) et meshes (.json) intégrés au binaire
└── src/
    ├── main.rs             ← boucle principale (fenêtre 960×540, sans vsync)
    ├── config.rs           ← constantes (vue, monde torique, gameplay)
    ├── geom.rs             ← Point, World, Segment, Triangle + géométrie
    ├── persist.rs          ← fichier de config XDG (mode, musique, volume, graphismes, génération auto)
    ├── shape.rs            ← Shape, meshes, collisions, mouvement
    ├── garbage.rs          ← débris
    ├── state.rs            ← Player, Element, GameState, messages
    ├── generate.rs         ← génération procédurale des météores, prepare
    ├── game.rs             ← boucle de jeu (input, déplacement, collisions, pause)
    ├── render.rs           ← rendu (étoiles, triangles texturés, HUD, aide, debug)
    ├── scenario.rs         ← scénarios (règles économiques, modes, réputation, atelier d'amélioration)
    ├── title.rs            ← écran titre (bannière arc-en-ciel, étoiles, choix du scénario)
    ├── audio.rs            ← sons et musique (ambiance, moteur, explosions)
    ├── cosmonaut.rs        ← cosmonaute EVA (mesh `cosmonaute.json`, couleurs par face)
    ├── vaisseau.rs         ← vaisseau joueur (mesh `vaisseau.json`, couleurs par face)
    └── x11.rs              ← plein écran EWMH (X11)
```
