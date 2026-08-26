# Meteors Mining

Jeu d'arcade 2D de minage spatial écrit en **Rust** avec [macroquad](https://macroquad.rs).
Réimplémentation fidèle du jeu QB64 « Meteors Mining » (première version jouable : 12 nov. 2025).

## Le jeu

Pilotez votre vaisseau dans un champ de météores, détruisez-les au tir et
ramassez les minerais qu'ils laissent derrière eux. Remplissez la
soute, puis revenez à la station pour décharger et gagner des crédits.

- **Monde torique** : l'espace se reboucle sur lui-même (3960 × 3540), aucun bord.
- **Météores destructibles** : 6 à 16 triangles par météore, générés
  procéduralement, avec choc élastique entre eux et débris à chaque impact.
- **Minage** : les triangles minéraux (or, fer…) laissent des minerais à ramasser.
- **Soute** : 5 éléments maximum - pleine, il faut décharger à la station.
- **Station** : au lancement (et après respawn) le vaisseau est **à quai**
  au centre de la base, **tenu par 4 liens néon** (mire cachée) ; dès qu'on
  démarre (une flèche), les **liens se rétractent** (1,5 s, monde vivant) et
  le
  vaisseau est libre - hors de la base, ni lien ni cible, et **pas de mire
  tant qu'on quitte l'accostage** : la mire n'est affichée **que lors du
  retour**. Au **retour**, au moment où l'on **franchit la limite extérieure
  de la base en entrant** (après l'avoir franchie en sortant), la **mire**
  **néon** pulsante apparaît au centre (le guide d'accostage, cercle de
  15 px, dessiné sous le vaisseau) et **réagit dans tout le rayon de la
  base** : sa couleur passe **progressivement du rouge au vert** selon la
  distance au centre ET la vitesse (rouge au bord du rayon ou trop rapide,
  vert au centre et presque immobile) ; la distance est au HUD
  (`DOCK DIST: 123` - sans unité - / `DOCK: SLOW DOWN` / `DOCK: IN RANGE` /
  `DOCKED`).
  L'accostage se termine seulement **presque immobile dans la zone** : la
  mire **disparaît** et une **animation de 3 s** (monde vivant) **projette**
  les **4 liens** en diagonale (**NO, SO, SE, NE**) : ils jaillissent de
  l'anneau vers le vaisseau (onde qui court vers lui) et se branchent **près
  de son centre** (l'illusion qu'ils le touchent), puis le pivote vers la
  droite tout en le recentrant **exactement au centre** de la station, puis
  la boîte DOCK STATION s'ouvre (cargo déchargé, vaisseau réparé). Au départ
  (CLOSE), la **tension est relâchée** : les liens se **rétractent en
  ondulant** (une onde court du vaisseau vers l'anneau, l'extrémité libre
  fouette puis retombe - comme un câble qui se rentre), puis le vaisseau est
  libre.
- **Météores en continu** : génération automatique (limite 150) ou à la demande.
- **Minerais dans les météores** : chaque météore contient une quantité de
  minerai (un par triangle minéralisé - or, fer, eau - au départ, plus un
  par minerai absorbé). Le minerai n'est **jamais détruit** quand son météore
  l'est : qu'il soit détruit par un **autre météore** ou par un **missile du
  vaisseau**, ses minerais sont **libérés en minerais** à sa position. Le seul
  cas de destruction de minerai : un **missile touche directement le minerai**
  (il est détruit, sans nouveau minerai). Si un **météor percute un
  minerai**, il l'**absorbe** (il disparaît, sa quantité de minerai augmente)
  sans être endommagé - les minerais qu'il a mangés sont récupérables en le
  détruisant (missile ou collision).
- **Cosmonaute de secours** : quand le vaisseau est détruit (jeu libre ou
  Progression), le pilote est **éjecté** - un petit **cosmonaute EVA** (le
  personnage de `assets/cosmonaute.json`, en couleurs par face) apparaît à la
  position du crash et devient le personnage contrôlé : il se dirige **comme
  le vaisseau** mais avec **un seul propulseur** : la poussée est
  **vectorielle** (↑ ajoute la poussée au **vecteur de déplacement** - pour
  changer de direction, d'abord **s'orienter** avec ←/→, puis pousser), pas
  de frein ni de marche arrière ; la caméra, la mire et le HUD le suivent),
  dessiné **au premier plan** - uniquement pendant l'EVA (jamais de
  cosmonaute supplémentaire dans le monde) - avec un **petit propulseur sur
  le dos**
  (flamme animée orange/jaune, vacillante, visible quand il pousse) et des
  **membres animés** : bras et jambes **s'agitent** (bascule autour des
  épaules/hanches) pendant la poussée puis retombent au repos. Il peut
  **ramasser les minerais** par proximité (même soute que le vaisseau -
  déchargée en crédits à la station). Au crash, les minerais collectés
  sont **rejetés autour** du vaisseau détruit (un minerai par unité,
  éparpillé à proximité, soute vidée) - le cosmonaute, ou le vaisseau
  ressuscité en Survival, peut les ramasser à nouveau. Son **seul
  objectif** : **rejoindre la base** - dès qu'il atteint la zone d'accostage au centre de la
  station, la **récupération** s'anime : un **cordon orange** jaillit de
  l'anneau jusqu'à lui et le **ramène sur l'anneau** (~2,5 s, monde vivant,
  ondulation qui s'affaisse quand la tension monte), puis un **fondu
  enchaîné** (2 s) l'efface pendant que le **vaisseau reconstruit apparaît
  au centre de la station, liens attachés** (la caméra glisse de l'anneau
  vers le centre). En Survival, la destruction reste gérée par les
  vies/bouclier (respawn à la station).
- **Audio** : ambiance, musique, moteur avant/recul, tirs, minerais et
  explosions à volume selon la distance au vaisseau.

## Compilation et lancement

```bash
cargo run --release
```

La fenêtre 960 × 540 s'ouvre sur l'écran titre - appuyez sur une touche
(autre que F/O/N) pour lancer la partie. **N** y change de scénario (jeu
libre ou Progression, voir « Scénarios » ci-dessous). Au lancement d'un
scénario qui a une **progression enregistrée**, le jeu propose de
**poursuivre** (1/ENTRÉE) ou de **repartir du début** (2/R - la sauvegarde est
remise à zéro) ; ESC annule et revient à l'écran titre.

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
embarquée en **JPEG** (457 Ko au lieu de 3,1 Mo en PNG - feature `jpeg` de la
crate `image` activée). Compression finale facultative avec **UPX** (binaire
statique sur GitHub) - à relancer après chaque `cargo build --release` :

```bash
upx --best --lzma target/release/rust-meteors-mining
```

### Jouer en ligne (WebAssembly)

Le jeu compile aussi vers **wasm32-unknown-unknown** et se joue dans le
navigateur : les assets et la police sont embarqués (`include_bytes!`),
`web/` contient le scaffolding minimal (`index.html` plein écran + `gl.js`,
le runtime web miniquad 0.4.11 versionné) et le binaire est publié sur
**GitHub Pages** par le workflow `deploy-wasm` (push sur `main`).

```bash
# test local
cargo build --release --target wasm32-unknown-unknown
cp web/index.html web/gl.js target/wasm32-unknown-unknown/release/rust-meteors-mining.wasm /tmp/site/
python3 -m http.server 8000 --directory /tmp/site   # ouvrir http://localhost:8000/
```

Sur le web, la télécommande HTTP, la manette et le son sont désactivés
(compatibilité plateforme) ; restent le clavier et le joystick tactile.

## Contrôles

| Touche | Action |
|---|---|
| ↑ | Accélérer |
| ← / → | Tourner |
| ↓ | Décélérer |
| Shift (gauche ou droit) | Tirer |
| Manette (stick gauche / croix / A ou gâchette droite) | Déplacement / Tir (en plus du clavier, du tactile et de la télécommande) |
| P | Pause (overlay PAUSE + rappel de la touche P) |
| S | Aide (liste des touches, fermeture au clic sur CLOSE) |
| O | Écran de paramétrage (aussi accessible depuis l'écran titre) : cases MUSIC / AUTO GENERATE / ANTIALIAS / TOUCH UI / SAVE POSITION, volume maître + sous-volumes MUSIQUE / EFFETS / AMBIENCE (barres horizontales cliquables/glissables), ligne REMOTE PIN (code de la télécommande) et panneau « GRAPHICS » (RENDER texturé/colorisé/mesh, WINDOW fenêtré/plein écran zoomé/natif, SIZE 960×540 à 1920×1080 - clic = cycle) ; si un réglage exige un redémarrage (anticrénelage), note « RESTART REQUIRED » et bouton RESTART (relance le jeu) ; RESET revient aux défauts des réglages (la progression du scénario est conservée) ; en PROGRESSION/Survival, le bouton RESET PROGRESSION (colonne gauche) remet à zéro la progression du scénario - crédits, modes payés, réputation, extensions d'atelier, vies/bouclier et mode de déplacement choisi - puis réapplique les règles de départ (seuls les réglages et le scénario choisi sont conservés) ; fermer avec CLOSE ou ESC. Le mode de déplacement se choisit désormais au magasin de la station (bouton SHOP de la boîte DOCK STATION) |
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
(UNLOAD / SHOP / CLOSE) : UNLOAD décharge
la soute, SHOP ouvre le **magasin de la station** (section « MOVING MODE » :
choisir un mode de déplacement, ou le débloquer contre crédits en scénario
à économie - dans tous les scénarios ; en Progression, s'y ajoutent les
lignes d'extension de vaisseau et le **ravitaillement** - carburant et
munitions achetés indépendamment, **à la quantité** : chaque ligne porte un
**curseur** (glisser ou molette) qui choisit combien de carburant (ou de
munitions pour l'arme) acheter, le coût s'affiche à droite - clic sur la
ligne pour acheter la quantité choisie), CLOSE ferme - la boîte reste
ouverte après UNLOAD et les achats du magasin pour tout faire avant de
partir.

## Scénarios

Les scénarios (choisis à l'écran titre, touches N/B ou 1-3 - l'écran titre
affiche leurs **règles** (`[ RULES : … ]`, dérivées des données par
`scenario::scenario_rules`, avec les **valeurs chiffrées en surbrillance**
dans la **couleur propre du scénario** - jaune pour Progression, cyan pour
Survival (coûts, vies, bouclier, dégâts, rangs) - pour faire ressortir ce
qui change au basculement ; juste après un changement (N/B/1-3), toute la
ligne **clignote dans cette couleur** ~1,2 s pour attirer l'œil) et la
**progression enregistrée** du scénario (`[ SAVE : … ]`,
crédits/modes/réputation ou vies/bouclier, avec les **valeurs en
surbrillance** dans la couleur du scénario elles aussi -
`scenario::save_summary_segments`)) encapsulent des règles de jeu en
**données +
points d'accroche purs** (`src/scenario.rs`) - la boucle (`game.rs`) ne fait
qu'appeler des fonctions testables sans macroquad :

- **FREE PLAY** (défaut) - le comportement historique : aucun coût, tous les
  modes de déplacement disponibles, carburant et munitions illimités, et le
  **radar** (minimap globale des météores) **allumé par défaut** - il ne
  s'achète qu'en scénario à économie (Progression / custom).
- **PROGRESSION** - l'exemple d'économie :
  - le vaisseau démarre gratuitement en mode **REALISTIC**, identique à
    **INERTIAL** pour la poussée vectorielle ; ses propulseurs latéraux
    accélèrent progressivement la rotation, le relâchement la conserve et la
    poussée opposée permet de la compenser jusqu'à l'arrêt ; seuls les modes
    dont le coût configuré (outil) est nul sont débloqués au départ
    (REALISTIC par défaut) ; les modes payants - **INERTIAL** (15 crédits),
    **4 WAYS** (30) et **DIRECTIONAL** (45) - se débloquent dans le
    **magasin de la station** (bouton SHOP de la boîte DOCK STATION) en
    payant des crédits (coût affiché à côté du mode) ;
  - les crédits s'obtiennent en minant : chaque minerai déchargé à la station
    vaut selon son élément (or 5, fer 3, eau 2) ;
  - **carburant** et **munitions** sont payants : chaque poussée consomme du
    carburant (moteur éteint, plus de poussée - rotations libres), chaque tir
    une munition par arme ; ils s'achètent au magasin (section
    RAVITAILLEMENT), **indépendamment** et **à la quantité** : un **curseur**
    par ressource (glisser à la souris, molette = ± un paquet) choisit les
    unités à acheter - tout achat paie au moins un paquet (10 carburant = 1
    crédit ; les munitions par **paquet** propre à chaque arme, ex 1 crédit
    pour 5 munitions, via une ligne AMMO par arme possédée) ; le curseur
    part d'office sur le **maximum achetable** avec les crédits courants,
    pour ne jamais rester bloqué faute d'un plein complet - plus d'achat
    automatique au déchargement ;
  - les **armes du catalogue** s'achètent au magasin (bouton SHOP) : une
    arme payante (coût en crédits, 0 = arme de base toujours équipée) ne
    tire et ne s'affiche sur le vaisseau qu'une fois achetée ; chaque arme a
    son propre stock de munitions (un tir consomme 1 munition de chaque arme
    qui tire ; une arme à court de munitions s'arrête, les autres
    continuent) et son propre paquet de ravitaillement ;
  - **le magasin** (bouton SHOP de la boîte DOCK STATION - la place de
    marché/atelier) permet d'acheter contre crédits des extensions
    de vaisseau, persistées avec la progression : **réservoir** (100 de base,
    3 extensions de +50 → 250 max), **chargeur** (30 de base, 3 extensions
    → 70 max) et **soute** (5 emplacements de base, 2 extensions → 10 max) ;
    à l'achat, le réservoir/chargeur repart plein à la nouvelle capacité et
    la soute s'agrandit immédiatement ; le HUD affiche les capacités courantes
    (`FUEL:50/150 AMMO:20/45`) ;
  - le **radar de bord** (la minimap globale qui affiche la position des
    météores et des autres formes sur une carte au centre de l'écran) est
    **éteint par défaut** : il s'achète au magasin (onglet ÉQUIPEMENT, 20
    crédits, ligne RADAR sous les armes) et s'allume dès l'achat
    (persisté avec la progression) ;
  - la **réputation** croît à chaque astéroïde détruit, d'autant plus que la
    précision de tir est bonne (gain × (1 + 2 × précision)) - affichée au HUD
    avec FUEL / AMMO / CREDITS ; elle débloque des **rangs** (paliers
    affichés au HUD, ex `REPUTATION:37 (ACE)`) : CADET (0) → PILOT (10) →
    VETERAN (25) → ACE (50), chaque palier franchi est annoncé (« RANK UP:
    PILOT ») ;
- **SURVIVAL** - preuve que le système s'étend hors de l'économie : ni
  crédits ni verrous (tous les modes disponibles), mais le vaisseau a des
  **vies** (3) et un **bouclier** qui absorbe les impacts (3 points) ; quand
  il est percé, l'impact suivant détruit le vaisseau - une vie est perdue et
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
automatiquement) : crédits, modes payés et réputation en Progression
(`prog_credits`, `prog_modes`, `prog_reputation`), **vies et bouclier en
Survival** (`prog_lives`, `prog_shield` - bornés aux capacités du scénario,
une sauvegarde à 0 vie repart au départ complet) ; chaque scénario n'écrit
que ses propres clés, sans écraser la sauvegarde de l'autre. Le carburant et
les munitions (par arme), eux, repartent pleins à chaque lancement ; les
armes achetées, elles, sont persistées (`prog_weapons`), de même que le
**radar de bord** (`prog_radar`).

## Outil de gestion : la place de marché

Une **application de gestion dédiée** accompagne le jeu : son but est la
**mise au point des objets vendus sur la place de marché** accessible depuis
la base - les extensions de vaisseau et les modes de déplacement (bouton
`SHOP` de la boîte DOCK STATION, scénario Progression).

`tools/marketplace-editor/index.html` est une **page unique, autonome** (HTML
+ CSS + JS embarqués, aucune dépendance, fonctionne hors ligne en ouvrant le
fichier dans un navigateur - comme l'éditeur « meshes-designer » pour les
meshes) qui permet de : les cartes s'affichent **une à la fois** (navigation
**◀/▶** en haut de page, **flèches ← / →** du clavier, ou clic direct dans
la **liste des cartes** de l'encart de gauche, qui regroupe aussi les boutons
d'ouverture, d'enregistrement et d'export/import JSON) ;

- **éditer les trois lignes d'amélioration** (réservoir `FUEL TANK`, chargeur
  `MAGAZINE`, soute `CARGO BAY`) : libellé Rust, capacité de base et
  extensions successives (nom, coût en crédits, bonus de capacité), avec
  ajout / suppression / réordonnancement des extensions ;
- **suivre la mise au point en direct** : pour chaque ligne, la progression
  des capacités par niveau, le coût cumulé et l'efficacité (crédits par
  unité de bonus) ; en synthèse, le coût total pour tout maxer (converti en
  minerais d'or/fer/eau à ramasser) et les capacités finales ;
- **éditer l'économie de la station** : valeur des minerais en crédits
  (`ELEMENT_VALUES` : or/fer/eau), modes de déplacement (`MOVING_MODES` :
  nom, description et coût de déblocage de chaque mode, vendus au magasin de
  la station - `MODE_COSTS` en est dérivé ; seuls les modes à coût nul
  (REALISTIC par défaut) sont débloqués au départ, les autres, INERTIAL
  compris, s'achètent au magasin) et prix du ravitaillement
  (`FUEL_PRICE`/`FUEL_STEP`, `AMMO_PRICE`/`AMMO_STEP`) ;
- **régler les météores & collisions** : force de réaction d'un météore qui
  percute la base (`METEOR_STATION_RESTITUTION` - le triangle qui collisionne
  explose et le météore est repoussé : seule sa trajectoire est réfléchie,
  la vitesse est conservée, 1.0 = rebond parfait (miroir), 0 = pas de
  réaction), débris par explosion (`GARBAGE_PER_TRIANGLE`), vitesse maximale
  (`METEOR_VELOCITY_MAX`), génération procédurale (`TRIANGLES_IN_SHAPE_*`,
  `TRIANGLE_BASE_*`, `TRIANGLE_HEIGHT_*`) et population
  (`INITIAL_MAX_METEOR_SHAPES`, `SHAPES_COUNT`) - constantes de
  `src/marketplace.rs`, lues par `src/game.rs`, `src/garbage.rs`,
  `src/generate.rs` et `src/state.rs` ;
- **régler la réputation** : rangs du scénario Progression (seuil de
  réputation, nom affiché au HUD et **remise en %** sur les coûts de la
  station - `PROGRESSION_RANKS` de `src/marketplace.rs`, lus par
  `src/scenario.rs`) avec ajout / suppression / réordonnancement des rangs ;
  la remise du rang atteint s'applique à **tous les coûts** : extensions
  d'atelier, ravitaillement (carburant, munitions) et déblocage des modes de
  déplacement - la synthèse montre le coût pour tout maxer remisé à chaque
  rang ; la **précision de tir** amplifie la remise (poids
  `DISCOUNT_PRECISION_WEIGHT` : remise × `1 + poids × précision`, 1.0 =
  100 % de précision → remise doublée - la synthèse montre aussi le coût à
  100 % de précision) ; le gain de réputation reste codé dans
  `src/scenario.rs` : astéroïdes détruits (bonus de précision) et minerais
  déchargés à la station (`reputation_per_mineral` - le commerce est
  récompensé) ;
- **régler le vaisseau joueur** : choix du mesh (`assets/*.json`, format
  « meshes-designer », avec **aperçu en direct** sur la page - le vaisseau
  tel qu'il volera, pivot marqué d'une croix), échelle en %, orientation en
  degrés (angle du nez du mesh dans l'éditeur : 0 = à droite, 90 = en haut),
  centre de rotation en % de la boîte englobante du mesh (50/50 = centre)
  et **emplacements de départ des projectiles** : positions en % de la même
  boîte englobante (marquées d'un losange doré sur l'aperçu), **une balle
  part de chaque emplacement au tir** (Shift), tournée avec le vaisseau -
  liste vide = un seul emplacement au centre de rotation ; et les
  **propulseurs des éjections de gaz** des 4 touches de contrôle (marqués
  d'un losange coloré + la touche, glissables sur l'aperçu) : **un mesh par
  propulseur** (choisi dans le catalogue `assets/*.json`, comme le vaisseau,
  ex propellerUp.json - la flamme du gaz), affiché **seulement quand il
  tire** (scintillant, teinté de la couleur configurée, allongé le long de
  la direction d'éjection) - sinon il n'apparaît pas en jeu - avec son
  échelle, son orientation, sa position en % de la même boîte englobante
  (valeurs libres, négatives ou > 100 %), sa **couleur de gaz** et sa
  **direction d'éjection** - ordre fixe ↑ (poussée avant, gaz orange à
  l'arrière), ↓ (frein/recul, bleu à l'avant), ← et → (jets latéraux des
  rotations) ; la flamme est tournée avec le vaisseau ; sur l'aperçu, la
  **molette** tourne le mesh (orientation), **Ctrl/Cmd + molette** zoome
  (échelle) et le **clic** place le centre de rotation au point visé ;
  constantes `VAISSEAU_JSON`, `VAISSEAU_SCALE`,
  `VAISSEAU_ORIENTATION_DEGREES`, `VAISSEAU_CENTER_X/Y_PERCENT`,
  `VAISSEAU_BULLET_SPAWNS`, `VAISSEAU_THRUSTERS` (+ meshes de propulseurs
  `VAISSEAU_THRUSTER_MESH_i`, `include_str!`) de
  `src/marketplace.rs`, lues par `src/vaisseau.rs`, `src/generate.rs` et
  `src/main.rs` ; le mesh choisi est embarqué au compile (`include_str!`),
  il doit exister dans le projet ;
- **gérer un catalogue d'armes** : chaque arme est un mesh posé **sur le
  vaisseau** à un emplacement de tir (`spawnIndex` - liste **contrainte**
  aux emplacements de la section « Départ des projectiles ») et tire sa
  propre **munition** (mesh) ; l'arme est dessinée sur l'aperçu à son
  emplacement ; chaque arme a son échelle, son orientation, et sa munition
  sa propre échelle/orientation ; **toutes les armes tirent ensemble au
  Shift**, depuis leur emplacement ; catalogue vide = tir classique (une
  balle rouge par emplacement, repli) ; constantes `VAISSEAU_WEAPONS` (type
  `VaisseauWeapon` - `name`, `mesh`, `scale`, `orientation_degrees`,
  `spawn_index`, `ammo_mesh`, `ammo_scale`, `ammo_orientation_degrees`,
  `cost` - coût d'achat au magasin (0 = arme de base) -, `ammo_price` /
  `ammo_pack` - prix d'un paquet de munitions et sa taille) et
  meshes d'armes/munitions embarqués (`VAISSEAU_WEAPON_MESH_i` /
  `VAISSEAU_WEAPON_AMMO_MESH_i`, `include_str!`), lues par `src/vaisseau.rs`
  (l'arme est construite avec le vaisseau, elle tourne avec lui - seules les
  armes possédées sont dessinées), `src/generate.rs` (`fire_bullet` - une
  arme ne tire que si elle est possédée et a des munitions) et
  `src/scenario.rs` (achat au magasin, ravitaillement par paquets) ; les
  fichiers mesh doivent exister dans le projet ;
- **composer le mesh** (vaisseau et cosmonaute) : chaque **plan** du fichier
  (le mesh « meshes-designer » est une liste de plans) porte une règle -
  *toujours visible*, *exclu* (jamais construit) ou, pour le vaisseau
  uniquement, **lié à une ligne d'atelier** (FUEL TANK / MAGAZINE /
  CARGO BAY + niveau minimal) ; l'aperçu (et le centre de rotation) ne
  montre que les plans retenus, avec actions « Tout visible / Tout exclure /
  Inverser » ; en jeu, un plan lié **n'apparaît qu'à partir du niveau
  indiqué** : l'achat d'une extension reconstruit le vaisseau
  (`vaisseau::rebuild_player_vaisseau`) - le centre de rotation, calculé sur
  la composition complète, reste stable à tous les niveaux ; un plan exclu
  du cosmonaute n'est pas animé (bras/jambes) ; constantes
  `VAISSEAU_PLANES_ALWAYS` + `VAISSEAU_PLANE_LINKS` (types
  `PlaneUpgradeTrack` + `PlaneUpgradeLink`) et `COSMONAUTE_PLANES` -
  **listes vides = tous les plans** (repli sûr) ;
- **régler le cosmonaute EVA** (le pilote éjecté quand le vaisseau est
  détruit) : même principe que le vaisseau - choix du mesh (`assets/*.json`,
  aperçu en direct, pivot marqué d'une croix), échelle en % (150 % par
  défaut : ~17 unités éditeur → ~26 unités monde), orientation en degrés et
  centre de rotation en % de la boîte englobante (mêmes réglages souris :
  molette = orientation, Ctrl/Cmd + molette = zoom, clic = centre) - constantes
  `COSMONAUTE_JSON`, `COSMONAUTE_EVA_SCALE`, `COSMONAUTE_ORIENTATION_DEGREES`,
  `COSMONAUTE_CENTER_X/Y_PERCENT` de `src/marketplace.rs`, lues par
  `src/cosmonaut.rs` (l'animation bras/jambes suit le mesh) ;
- **charger / enregistrer directement** : `src/marketplace.rs` est **chargé
  automatiquement à l'ouverture** (mode serveur) et « 💾 Enregistrer le
  fichier » l'écrit dans le projet (fetch GET/PUT en mode serveur, API File
  System Access avec Chrome/Edge en fichier local, sinon sélecteur de
  fichiers et téléchargement en repli). Le fichier **complet** est régénéré
  (documentation, constantes économiques, constantes « météores &
  collisions » (`METEOR_STATION_RESTITUTION`, `GARBAGE_PER_TRIANGLE`,
  `METEOR_VELOCITY_MAX`, `TRIANGLES_IN_SHAPE_*` / `TRIANGLE_*`,
  `INITIAL_MAX_METEOR_SHAPES`, `SHAPES_COUNT`), rangs de réputation
  `PROGRESSION_RANKS`, constantes `VAISSEAU_*` du vaisseau (réglages +
  emplacements de tir `VAISSEAU_BULLET_SPAWNS` + propulseurs d'éjection de
  gaz `VAISSEAU_THRUSTERS` (+ meshes `VAISSEAU_THRUSTER_MESH_i`) +
  catalogue d'armes `VAISSEAU_WEAPONS` + meshes d'armes
  `VAISSEAU_WEAPON_MESH_i` / `VAISSEAU_WEAPON_AMMO_MESH_i` + composition
  `VAISSEAU_PLANES_ALWAYS` / `VAISSEAU_PLANE_LINKS`) et `COSMONAUTE_*` du
  cosmonaute EVA (dont `COSMONAUTE_PLANES`), types `ShipUpgrade` +
  `UpgradeTrack` + `ReputationRank` + `PlaneUpgradeTrack` +
  `PlaneUpgradeLink` + `VaisseauWeapon` + `VaisseauThruster` et lignes
  `FUEL_UPGRADE_TRACK` … `CARGO_UPGRADE_TRACK`)
  dans le style exact du code du jeu ; on recompile ensuite
  (`cargo build --release` - les tests `cargo test` valident les nouvelles
  valeurs) et `src/scenario.rs` n'a plus besoin d'être modifié : il importe
  déjà ces données depuis `src/marketplace.rs` ;
- **sauvegarder / restaurer** ses réglages : enregistrement automatique dans
  le navigateur (localStorage) et export/import d'un fichier
  `marketplace.json` ; la **carte ouverte** (vaisseau, soute, réputation…)
  est aussi mémorisée et **retrouvée au prochain lancement** de la page.

La seule constante liée non éditée par l'outil est `CARGO_SIZE`
(`src/config.rs`, capacité de base de la soute) - rappelée en pied de page.

**Serveur local** : pour charger et enregistrer `src/marketplace.rs` du projet
directement (sans copier/coller ni API de navigateur), lancez

```bash
node tools/marketplace-editor/server.mjs
```

puis ouvrez `http://localhost:8123` - le fichier `src/marketplace.rs` est
**chargé automatiquement** à l'ouverture (aucune action nécessaire) et
« 💾 Enregistrer le fichier » l'écrit directement (fetch GET/PUT). Le serveur
expose aussi la **liste des meshes** (`GET /list-assets`,
`GET /assets/<fichier>`) pour le choix de l'asset du vaisseau et son aperçu,
ainsi qu'une **console cargo** (`POST /api/cargo` avec `{ "command": "test"
| "run" | "run-release" | "wasm" }` - la sortie est renvoyée en flux,
terminée par le code de sortie - et `POST /api/cargo-stop` pour arrêter) : la
barre en bas de la page lance `cargo test` (vérifie les constantes exportées),
`cargo run` ou `cargo run --release` (compile puis ouvre le jeu) et affiche la
sortie en direct, avec « ■ Arrêter » pour couper. Le bouton **WASM local**
compile la version web (`cargo build --release --target wasm32-unknown-unknown`)
puis la sert par le même serveur : ouvrez `http://localhost:8123/wasm/` pour
tester le jeu dans le navigateur. Le serveur **se redémarre tout seul** quand
`server.mjs` change (watch du fichier, redémarrage à l'identique - désactivable
avec `AUTO_RESTART=0`) : après une mise à jour de l'outil, un simple
rechargement de la page suffit.
C'est la méthode recommandée avec le navigateur intégré de VSCode, qui
n'expose pas l'API File System Access. En ouvrant la page en fichier local
(`file://`), Chrome/Edge utilisent l'API File System Access, les autres
navigateurs le sélecteur de fichiers classique (la console cargo, qui dépend
du serveur, y est masquée).

## Détails techniques

- Rust (édition 2021) + macroquad 0.4, sans vsync (boucle plafonnée, physique
  en `dt` indépendante du FPS).
- 100 000 étoiles précalculées sur 15 couches de parallaxe.
- Génération procédurale **déterministe** (PRNG ChaCha12 seedé) - parties
  reproductibles.
- Collisions par séparation de triangles (SAT) + choc élastique ; le centre
  des formes est recalculé après chaque impact.
- Plein écran : mode **zoomé** (vue 960 × 540 rendue dans une texture puis
  étirée, letterbox) ou **natif** (rendu direct à la définition réelle de
  l'écran) ; la bascule EWMH passe par `src/x11.rs` (ClientMessage
  `_NET_WM_STATE` direct, sans outil externe).
- **Police embarquée** (`src/font.rs`) : DejaVu Sans Mono est intégrée au
  binaire (`include_bytes!`, licences dans
  `assets/fonts/LICENSE-DejaVuSansMono.txt`) - aucune dépendance aux polices
  du système, portable sur toutes les plateformes. Elle est rendue à
  l'échelle 0.831 pour conserver la grille 8 px du HUD et les lignes de
  16 px, et apporte un jeu de caractères étendu (Latin-1 accentué, `→`, `✓`
  …) que la police par défaut de macroquad ne possède pas.
- Réglages persistants (`meteors_mining.cfg`, dossier de configuration
  utilisateur - norme XDG, ex `~/.config/meteors-mining/meteors_mining.cfg`) :
  mode de déplacement (choisi au magasin de la station, bouton SHOP de la
  boîte DOCK STATION), musique (touche M), volume maître + sous-volumes
  MUSIQUE / EFFETS / AMBIANCE, style de rendu, mode d'affichage, définition
  de fenêtre, anticrénelage, interface tactile (TOUCH UI), PIN de la
  télécommande (REMOTE PIN) et option SAVE POSITION (le vaisseau repart de
  sa dernière position à la sortie) - modifiables dans l'écran de
  paramétrage (touche O) ou par les touches M/A, rechargés au lancement
  suivant. NB : la génération automatique des météores (touche A ou
  case AUTO GENERATE) n'est **pas** persistée - elle repart **toujours
  active** à chaque lancement, pour que le monde ne soit jamais vide au
  démarrage. S'y ajoutent le scénario choisi et la progression d'une partie à
  économie (`scenario`, `prog_*`), sauvegardés à chaque changement
  (déchargement, ravitaillement, mode payé, achat au magasin, astéroïde
  détruit) et restaurés au lancement (une sauvegarde finale a aussi lieu à
  la sortie du jeu). Le
  RESET de l'écran de paramétrage ne supprime que les clés de réglage (le
  mode de déplacement n'étant plus un réglage, il n'est pas touché) - la
  progression du scénario survit. En fenêtré, une définition plus grande que
  960×540 étire la vue (letterbox) ; l'anticrénelage MSAA est appliqué à la
  création de la fenêtre (effectif au lancement suivant).
- Le jeu est testé : `cargo test` (103 tests unitaires - physique, collisions,
  minage, accostage, paramétrage, options graphiques, persistance, scénarios,
  atelier d'amélioration).


### Application d'Édition de Scénarios et Objectifs (DAG)

Une **application dédiée à la création de scénarios et d'enchaînements d'objectifs** (`tools/scenario-editor/index.html`) complète l'outillage du jeu :

- **Édition visuelle en graphe DAG** : création, glisser-déplacer de nœuds d'objectifs et tracé interactif de liens de dépendances (flèches SVG Bézier) entre prérequis et étapes suivantes ;
- **Détection automatique de cycles & validation** : vérification en temps réel de l'acyclicité du graphe (algorithme DFS), identification des nœuds orphelins et validation des identifiants ;
- **Inspecteur d'objectifs complet** : paramétrage fin des conditions de réussite (*Détruire météores, Collecter crédits, Atteindre réputation, Accostages station, Amélioration atelier, Mode de vol, Survie chronométrée, Tir de précision*) et des récompenses associées (*Crédits, Réputation, Carburant, Munitions, Victoire*) ;
- **Double vue** : vue Graphe DAG interactif + vue Séquence / Chronologie des étapes ;
- **Export Rust autonome & Persistance JSON** : enregistrement des scénarios au format `.scenario.json` dans le dossier `scenarios/` et génération du module Rust `src/scenario_objectives.rs` compilé directement par `cargo test` / `cargo run` ;
- **Lancement rapide** :
  ```bash
  tools/scenario-editor/launch-editor.sh
  ```
  Le serveur local (port 8124) s'exécute et ouvre automatiquement l'application dans le navigateur par défaut. Sa **console cargo** (panneau latéral « Actions & Tests Cargo ») lance `cargo test`, `cargo run`, `cargo run --release` ou le build **WASM local** (`cargo build --release --target wasm32-unknown-unknown` puis version web servie sous `http://localhost:8124/wasm/` pour tester le jeu dans le navigateur). Comme celui de la place de marché, le serveur **se redémarre tout seul** quand `server.mjs` change (`AUTO_RESTART=0` pour désactiver).

## Structure du projet

```
rust-meteors-mining/
├── Cargo.toml              ← projet Rust (macroquad, rand, image)
├── assets/                 ← textures (.png/.jpg), sons (.ogg) et meshes (.json) intégrés au binaire
├── scenarios/              ← fichiers de scénarios JSON (.scenario.json)
├── tools/
│   ├── marketplace-editor/ ← application de gestion de la place de marché (page unique, export Rust)
│   └── scenario-editor/    ← application d'édition de scénarios et objectifs DAG (graphe, export Rust)
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
    ├── scenario_objectives.rs ← structures DAG, conditions, récompenses et validation des objectifs
    ├── title.rs            ← écran titre (bannière arc-en-ciel, étoiles, choix du scénario)
    ├── audio.rs            ← sons et musique (ambiance, moteur, explosions)
    ├── cosmonaut.rs        ← cosmonaute EVA (mesh `cosmonaute.json`, couleurs par face)
    ├── vaisseau.rs         ← vaisseau joueur (mesh `vaisseau.json`, couleurs par face)
    └── x11.rs              ← plein écran EWMH (X11)
```
