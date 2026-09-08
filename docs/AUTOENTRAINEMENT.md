# Auto-entraînement du pilote (cosmonaute) — démarche

But : mettre en place un système **d'auto-entraînement de pilotage** du
cosmonaute. Le système entraîné doit être **indépendant de l'application** :
son rôle est de **piloter** le vaisseau (et le cosmonaute EVA quand le
vaisseau est détruit), c'est-à-dire d'utiliser le jeu comme un
**environnement** - il faut donc d'abord mettre en place dans le jeu une
**interface de contrôle** qui permette au système d'auto-entraînement
d'utiliser l'application.

Ce document décrit la démarche, ce qui est **réalisé** (milestone 1 :
interface + entraîneur + ligne de base) et la suite.

## 1. Décisions de cadrage

| Question | Réponse | Conséquence |
|---|---|---|
| Qu'est-ce que le système pilote ? | **Les deux entités**, comme le pilote automatique : le vaisseau quand il est intact, le **cosmonaute EVA** quand le vaisseau est détruit (l'entité contrôlée suit la même règle que le joueur et l'autopilote) | L'interface expose la cinématique des deux, et l'épisode choisit l'entité de départ (`target`) |
| Comment le système indépendant communique-t-il ? | **HTTP/JSON d'abord** (même filière que la télécommande `remote.rs`), un mode accéléré sans rendu plus tard | Interface = serveur localhost sur un port dédié ; observation + actions au format JSON documenté |
| Périmètre du milestone 1 | **Interface de contrôle dans le jeu + entraîneur externe + ligne de base** (pilote automatique du jeu comme référence, politiques idle/aléatoire/contrôleur entraîné) | Le tout livré, testé et documenté ; l'entraînement « profond » et l'accélération headless viennent ensuite |

## 2. Architecture

```
┌─────────────────────────────── jeu (Rust, macroquad) ───────────────────────────────┐
│  main.rs (boucle)  ──▶  game.rs (update : physique, collisions, accostage)          │
│     ▲                        ▲                                                     │
│     │ publish_state          │ input::player_controls (vaisseau ET cosmonaute EVA)  │
│     │ (1 obs / frame)        │   └─ driver engagé ? ──▶ actions du pilote externe   │
│  src/driver.rs  ─────────────┘                                                      │
│   │  interface de contrôle : état partagé + serveur HTTP localhost (thread dédié)   │
└───┼─────────────────────────────────────────────────────────────────────────────────┘
    │  GET /obs     observation de la frame (JSON)
    │  POST /cmd    actions {up,down,left,right,fire} + bascules driver / autopilot
    │  POST /reset  épisode {seed, target:"ship"|"eva", x, y, auto_generate}
    ▼
┌────────────────────────── système d'auto-entraînement (indépendant) ────────────────┐
│  tools/trainer/  (Python standard, aucune dépendance)                                │
│   client.py   — client du protocole                                                  │
│   eva_env.py  — micro-simulateur (mêmes lois que le jeu) + géométrie partagée        │
│   policies.py — idle / random / seek (contrôleur paramétré, la politique entraînée)  │
│   evaluate.py — lignes de base (idle, random, seek, autopilot du jeu en live)        │
│   cem.py      — entraînement par croix-entropie des paramètres de `seek`             │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

Trois « coutures » préexistaient dans le jeu, qui ont rendu l'interface
simple à brancher :

1. **Un canal d'actions normalisé** : `input::player_controls`
   (`src/input.rs`) centralise déjà le pilotage - quand `state.autopilot` est
   actif il consomme des primitives `PilotInputs {up,down,left,right,fire}`
   calculées par `autopilot.rs` (pures, testables sans fenêtre) ; sinon il
   lit clavier/tactile/télécommande/manette. Le chemin EVA fait de même via
   `cosmonaut_apply_inputs` (up/left/right seulement). Le pilote externe se
   branche **exactement à cet endroit**, en priorité comme l'autopilote, pour
   les deux entités.
2. **Un précédent de transport** : `remote.rs` sert déjà du HTTP sur
   localhost, accepte des commandes et publie un état JSON à chaque frame,
   avec tests purs sans fenêtre. `driver.rs` suit le même patron sur un port
   distinct.
3. **Une génération de monde déterministe** : `generate::prepare` (PRNG
   ChaCha12 seedé) + les fonctions d'état (`scenario::apply_start`,
   `eva::activate_cosmonaut`, accostage/secours) fournissent les briques des
   remises à zéro d'épisode reproductibles et de la détection de fin.

## 3. L'interface de contrôle dans le jeu — `src/driver.rs`

- **Observation** (`Observation`, construite par `observe(state, shapes)` -
  fonction **pure**, testable sans macroquad) : entité contrôlée
  (`"vaisseau"`/`"eva"`), cinématique des deux (position, vitesse
  vectorielle, direction/orientation/rotation), **deltas toriques** vers la
  station (l'entraîneur n'a pas à gérer le repliement du monde), état
  d'accostage / récupération EVA / pause / game over, ressources (économie
  ou non), soute, compteurs, et les **objets proches** (météores, minerais,
  aliens, mines, portails - jusqu'à 32, triés par distance, positions et
  vitesses **relatives**).
- **Actions** (`Shared`, servies par le serveur) : mêmes primitives que les
  touches ; quand `engaged` est vrai, `input.rs` les consomme à la place du
  clavier et de l'autopilote (pour le vaisseau **et** le cosmonaute EVA) et
  le jeu gère l'accostage/magasin à la place du pilote (branches driver dans
  `game.rs`).
- **Bascule de l'autopilote** (`autopilot` dans `/cmd`) : la ligne de base de
  référence reste jouable depuis l'entraîneur (l'autopilote et le pilote
  externe sont mutuellement exclusifs).
- **Épisodes** (`POST /reset`) : monde régénéré à la **graine**
  (déterministe : mêmes formes, même contenu), vaisseau reconstruit à quai
  (cible `ship`) ou détruit à `(x, y)` avec le cosmonaute EVA éjecté (cible
  `eva`) - consommé par la boucle de `main.rs` avant la frame. Le scénario
  repart en **jeu libre** (règles de départ, aucune sauvegarde du joueur) :
  chaque épisode s'entraîne sur la même base.
- **Publication** : une observation par frame de jeu, après `update` (l'état
  vu est celui d'après les actions de la frame). Le `frame` du serveur
  s'incrémente à chaque publication : l'entraîneur détecte ainsi les
  nouveaux pas. Rien n'est publié ni servi sur wasm (ni serveur ni
  entraînement dans le bac à sable navigateur).

### Protocole (localhost, JSON)

| Requête | Corps | Effet |
|---|---|---|
| `GET /obs` | – | dernière observation publiée |
| `POST /cmd` | `{"up":bool,"down":bool,"left":bool,"right":bool,"fire":bool,"driver":bool?,"autopilot":bool?}` | actions de la frame + engagement du pilote externe / bascule de l'autopilote (exclusifs) |
| `POST /reset` | `{"seed":u64,"target":"ship"\|"eva","x":f64,"y":f64,"auto_generate":bool?}` | pose une remise à zéro d'épisode (consommée à la frame suivante) |

Exemple de dialogue (outil `curl`) :

```bash
# monde neuf (graine 42), vaisseau détruit en (400, 250) → le cosmonaute EVA pilote
curl -s -X POST localhost:8643/reset -d '{"seed":42,"target":"eva","x":400,"y":250}'
# lire l'observation de la frame
curl -s localhost:8643/obs
# pousser (↑) vers la station
curl -s -X POST localhost:8643/cmd -d '{"up":true,"driver":true}'
# ligne de base : laisser l'ordinateur du jeu piloter
curl -s -X POST localhost:8643/cmd -d '{"autopilot":true}'
```

Tests : `driver.rs` embarque 10 tests (observation, application des
commandes/remises à zéro - purs -, et un test **bout en bout** sans fenêtre :
serveur sur port éphémère, `GET /obs`, `POST /cmd`, `POST /reset`).

## 4. L'entraîneur indépendant — `tools/trainer/`

Détail complet dans `tools/trainer/README.md`. L'idée :

- **Tâche** : le cosmonaute EVA éjecté doit rentrer dans le cercle
  d'accostage de la station. Épisode = `reset(seed, "eva", x, y)` +
  boucle `obs → cmd` jusqu'à la récupération (`eva_recovery > 0`) ou le
  délai. Récompense : +1000 récupéré, − temps, − pénalité d'arrivée trop
  rapide.
- **Lignes de base mesurées** (`evaluate.py`) : `idle`, `random`, `seek`
  (contrôleur homing paramétré, réglage robuste de départ) et, contre la
  vraie partie, `autopilot` (l'ordinateur du jeu - la référence à battre).
- **Entraînement** (`cem.py`) : méthode de la croix-entropie sur les 5
  paramètres du contrôleur `seek`. Un **micro-simulateur** (`eva_env.py`,
  mêmes formules que le jeu à 60 Hz) rend l'itération instantanée ; un
  `--backend live` rejoue chaque candidat contre la vraie partie.
- **Ce que l'entraînement apprend** : la bande d'alignement `turn_db` est le
  réglage critique - trop lâche, l'approche dérive et le cosmonaute se met
  en orbite autour de la base (l'échec du départ naïf que la CEM surmonte).
  La politique entraînée dépasse le réglage manuel sur la récompense (≈ 975
  contre ≈ 900 au départ par défaut).

Résultat typique du milestone 1 (simulateur, départ à 300 unités) :
idle et aléatoire échouent, le contrôleur réglé réussit en ~7 s
(récompense ~900), la politique entraînée réussit avec une arrivée plus
maîtrisée (récompense ~975). `policy.json` (sortie de `cem.py`) se rejoue
avec `evaluate.py --policy policy.json`.

**Validé en conditions réelles** (jeu lancé sur X11) : l'interface a servi
l'observation, les commandes et les remises à zéro sans interruption sur
plusieurs minutes ; la politique entraînée en simulateur **se transfère
telle quelle dans le jeu** et ramène le cosmonaute EVA à la station en
~11,6 s depuis 300 unités.

Ces essais réels ont mis au jour **deux défauts du jeu lui-même**, tous
deux corrigés :

1. **Bug latent** : débordement du bitmask de génération des gros météores
   (`border_mask` `u128` → 256 bits), déclenché par le météore spécial de
   60 triangles après ~150 s de partie (`src/shape.rs`).
2. **Loi de pilotage EVA de l'autopilote** : sans amortissement, la poussée
   radiale seule pompait l'énergie tangentielle et le cosmonaute se mettait
   en **orbite** autour de la base au lieu d'entrer dans le cercle
d'accostage (à 300 unités, l'autopilote s'éloignait en accélérant, dist
finale ~1300 après 130 s). Le simulateur avait prédit exactement cette
limite - la fidélité de ses lois est confirmée - et la correction a été
développée et vérifiée en simulateur (toutes fréquences 30-230 fps, toutes
distances 300-1500 unités, aucune orbite) avant d'être portée dans
`autopilot.rs` : bande d'alignement adaptative (`max(0.015, pas de
rotation × 0.75)`), croisière 60 u/s, bande de poussée 40 u/s, et **frein
tangentiel avec hystérésis** (poussée anti-vitesse quand la composante
tangentielle dépasse 0.25 u/frame, relâchée sous 0.12) - voir
`autopilot_eva_inputs` et le test de trajectoire dans `src/autopilot.rs`.
   **Re-validé en live** : l'autopilote corrigé ramène le cosmonaute depuis
   300 (15,6 s), 800 (40,5 s) et 1500 unités (76,9 s), arrivées à ~20 u/s
   et vitesse de pointe ≤ 25 u/s - plus aucune orbite. Le vaisseau
   (branche `autopilot_ship_inputs`) n'a pas été modifié.

## 5. Ce qui est réalisé (milestone 1)

| Élément | Où | Statut |
|---|---|---|
| Observation pure (vaisseau + EVA, station, ressources, objets proches) | `src/driver.rs` | ✅ (testée) |
| Serveur HTTP `GET /obs` / `POST /cmd` / `POST /reset` (natif, wasm désactivé) | `src/driver.rs` | ✅ (test bout en bout) |
| Branchement du pilote externe dans les entrées (vaisseau + EVA), accostage/magasin auto | `src/input.rs`, `src/game.rs` | ✅ |
| Remise à zéro d'épisode déterministe (graine, cible vaisseau/EVA) | `src/driver.rs` (reset_episode) | ✅ (testée) |
| Serveur démarré au lancement, publication par frame, consommation des épisodes | `src/main.rs` | ✅ |
| Client protocole + simulateur + politiques + évaluation + entraînement CEM | `tools/trainer/` | ✅ (validé en simulateur) |
| Ligne de base : pilote automatique du jeu mesurable depuis l'entraîneur | `POST /cmd {"autopilot":true}` | ✅ (en live) |
| Documentation | ce document + `tools/trainer/README.md` | ✅ |

## 5 bis. Phase 2 — épisodes accélérés sans rendu (mode headless, réalisé)

Le premier jalon de la Phase 2 est en place : un **mode d'exécution headless**
qui lance la **vraie physique du jeu** (`game::update`) à **pas fixe** (dt
constant 1/60 par défaut) **sans fenêtre macroquad** - ni rendu, ni audio,
ni lecture du clavier/souris. Le protocole de l'interface (`driver.rs`) est
**inchangé** (`GET /obs`, `POST /cmd`, `POST /reset`) : un entraîneur externe
s'y connecte exactement comme au jeu, mais sans le temps réel.

```bash
cargo run --release -- --headless [--port 8643] [--fps 60] [--seed 0]
#   (le jeu normal démarre sans argument ; wasm : pas de mode headless)
```

Fonctionnement (`src/headless.rs`) :

- **une observation par pas** publiée après chaque `update` (comme la boucle
  réelle) ;
- **`POST /reset`** fait toujours avancer d'un pas : l'observation du départ
  de l'épisode est servie immédiatement ;
- pilote externe **engagé** (`driver` vrai) : **chaque `POST /cmd` fait
  avancer d'un pas** - l'épisode est piloté pas à pas par l'entraîneur, à la
  cadence des allers-retours HTTP (au lieu du temps réel) ;
- **autopilote du jeu** (`autopilot` vrai, sans pilote externe) : la ligne de
  base **file en continu** à cadence bornée (480 pas/s par défaut) - assez
  rapide pour accélérer la référence, assez lente pour que l'entraîneur
  puisse échantillonner les fenêtres transitoires de l'observation
  (récupération EVA, accostage…) ;
- aucune décision en attente : la boucle attend (le serveur HTTP vit dans son
  propre thread).

Pour tenir sans fenêtre, `game::update` a été rendu **headless-compatible** :
ses lectures clavier/souris et son horloge (`is_key_pressed`, `get_time`…)
passent par des relais (`crate::headless::*`) qui les éteignent quand le mode
est actif (le pilote externe agit via `driver.rs`) et relaient exactement
macroquad sinon - le jeu fenêtré ne change pas. L'entrée native de `main.rs`
aiguille `--headless` vers la boucle headless avant toute initialisation de
macroquad (l'entrée wasm, elle, est inchangée). La configuration persistée du
joueur est **isolée** : `XDG_CONFIG_HOME` pointe sur un dossier jetable -
l'entraînement ne lit ni n'écrit jamais la sauvegarde réelle.

**Validé** (`tools/trainer/evaluate.py --backend live` contre le mode
headless) : autopilot et `seek` **tous réussis** (3/3 sur graines 1..3,
8/8 sur graines 100..107) depuis 300 unités, avec des temps de simulation
cohérents avec le micro-simulateur (~7,7 s pour `seek`, ~5 s autopilot) et
~1-2 s **mur** pour plusieurs épisodes au lieu du temps réel - le coût
restant est celui de la boucle physique (et des allers-retours HTTP du
pilotage pas à pas). `evaluate.py --backend live` a été rendu compatible
pas-à-pas : quand le pilote externe est engagé, une **commande vide**
déclenche le pas d'attente (`wait_frame`). Tests dans `src/headless.rs` : la
boucle tourne **sans fenêtre**, de façon **déterministe** (même graine →
même déroulé) et les graines produisent des mondes différents.

## 6. Suite (phases suivantes)

- **Phase 2 (suite) — épisodes plus riches côté jeu.** Le mode headless
  accélère le protocole existant. Reste à enrichir les épisodes eux-mêmes :
  boucle complète du vaisseau (décoller → miner → décharger, cible `ship`),
  missions portées par les objectifs DAG (`objective_tracker.rs`,
  `.scenario.json`) comme langage de tâche/récompense, termination explicite
  (accosté / EVA secouru / détruit / délai) exposée dans l'observation, et
  exécution d'épisodes **en continu dans le processus headless** (au-delà du
  pas-à-pas HTTP) pour viser des centaines d'épisodes à la seconde.
- **Phase 3 — vrais apprenants.** La politique `seek` paramétrée est une
  preuve de la boucle. La suite : politiques plus riches (réseau de neurones,
  RL : DQN/PPO sur l'observation complète avec les objets proches),
  enregistrement des trajectoires, entraînement sur la **vraie partie**
  accélérée, benchmark systématique contre l'autopilote du jeu.
- **Phase 4 — politique apprise dans le jeu.** Persister la politique
  entraînée et la charger comme stratégie « autopilote » alternative
  (l'interface expose déjà `autopilot` comme ligne de base ; il s'agira
  d'injecter la politique apprise au même endroit) ; durcissement : version
  du protocole, reproductibilité des graines, documentation.

## 7. Conventions et reproductibilité

- L'épisode est déterministe à la graine : même `seed` → mêmes formes,
  mêmes positions de départ ; l'orientation du cosmonaute éjecté est
  réinitialisée (0 = est) par `activate_cosmonaut`.
- Le micro-simulateur `eva_env.py` reproduit **les formules du jeu**
  (`thrust_vector`, `moving_shape`, `PLAYER_*`, `STATION_DOCK_DISTANCE`) ;
  l'observation simulée a le même format JSON que `/obs` - les politiques et
  l'entraîneur sont interchangeables entre simulation et partie réelle.
- La **vraie partie** fait foi : la simulation est un outil de développement
  (60 Hz, pas de collisions ni d'aliens autour de l'EVA, qui est un
  non-collider).
