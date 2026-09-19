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
- **Étiquette experte** (`expert`, calculée à la publication, pas dans
  `observe`) : l'action que prendrait l'**autopilote du jeu sur l'état de la
  frame** (mêmes primitives que les touches) - la cible de l'apprentissage
  par imitation, et le label de **DAgger** (étiqueter les états visités par
  la politique elle-même, voir §5 quater). Le drapeau d'**hystérésis du
  frein tangentiel EVA** (`eva_tang_braking`) est exposé dans l'observation :
  c'est un état interne de l'autopilote qui, sans lui, rendrait des
  observations identiques portées par des étiquettes expert contradictoires
  (la bande 7-15 u/s de vitesse tangentielle, le régime d'orbite).
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
- **banc d'essai en continu** (`POST /bench`) : le processus exécute un lot
  d'épisodes **de bout en bout dans le processus** à pleine vitesse - aucun
  aller-retour HTTP par pas (voir §5 ter) ;
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

## 5 ter. Banc d'essai en continu — des centaines d'épisodes/s dans le processus

Le pas-à-pas HTTP borne la cadence aux allers-retours locaux. Pour mesurer la
**ligne de base à pleine vitesse**, le processus headless exécute maintenant
les épisodes **dans le processus lui-même** : un lot d'épisodes enchaînés de
bout en bout (remise à zéro → autopilote du jeu → terminaison explicite),
**sans aucune publication ni commande HTTP par pas** - le coût d'un épisode se
réduit au temps de la physique (`game::update`). C'est l'accélération au-delà
du pas-à-pas.

### Protocole (`POST /bench`, `GET /bench`)

| Requête | Corps | Effet |
|---|---|---|
| `POST /bench` | `{"episodes":N,"seed":S,"target":"ship"\|"eva","x":..,"y":..,"scenario":"free"\|"economy"\|\"<id>\"?,"auto_generate":bool?,"max_steps":N?,"trajectories":bool?}` | pose un lot : `episodes` épisodes (graines `S..S+N-1`) exécutés en continu dans le processus par l'autopilote du jeu |
| `GET /bench` | – | rapport du lot : déroulé par épisode (graine, dénouement `delivered`/`eva_recovered`/`destroyed`/`objectives_complete`/`delai`, pas, secondes, vitesse d'entrée, **récompense**, **objectifs complétés**) + agrégats (temps mur, **cadence en épisodes/s**, répartition des dénouements, temps simulé moyen, **récompense moyenne**, chemin du fichier de trajectoires) |

Le lot est consommé par la boucle headless, qui **bloque** le temps de
l'exécuter (le serveur HTTP continue de répondre dans son thread) ; le rapport
est servi dès qu'il est prêt. `max_steps` est un garde-fou par épisode
(défaut 120 s de simulation) : au-delà, l'épisode est compté `delai` (aucune
terminaison atteinte).

### Récompense du banc d'essai (les mêmes règles que l'entraîneur)

Chaque épisode du rapport expose sa **récompense**, calculée **côté jeu** avec
**les mêmes règles que l'entraîneur** (`tools/trainer/eva_env.py::episode_reward`) :

- dénouement **réussi** (livraison du vaisseau, secours du cosmonaute EVA,
  objectifs DAG complétés) : `+1000 − 2·s − max(0, vitesse d'entrée − 30)·5 +
  bonus d'objectifs` - la vitesse d'entrée (celle du pilote au moment du
  dénouement) est exposée, la pénalité sanctionne un retour trop rapide ;
- **échec** (vaisseau détruit, garde-fou atteint) : `−2·s − 50 − distance
  finale × 0,1 + bonus d'objectifs`.

Le **bonus d'objectifs** (Phase 2, §5 quinquies) récompense la progression des
missions DAG d'un scénario à objectifs : chaque objectif complété pendant
l'épisode rapporte `OBJECTIVE_BONUS` (200), que l'épisode se termine ou non -
c'est la récompense partielle d'une mission accomplie.

La politique entraînée peut ainsi comparer directement sa récompense à celle
de l'autopilote sur des épisodes identiques, sans recalculer côté Python.

### Trajectoires pour l'entraînement RL (`trajectories`)

Avec `"trajectories":true`, le banc d'essai écrit en plus un fichier **JSONL**
(une entrée par ligne, dans le dossier temporaire headless) : une borne
`episode` (graine, cible, scénario, position), une entrée `step` par pas
(l'**observation** au format `/obs` + l'**action de l'autopilote** appliquée
ce pas - la ligne de base de référence), et une entrée `episode_end`
(dénouement, secondes, récompense). C'est le matériau d'un entraînement RL
**hors-ligne** sur les décisions de l'autopilote (`client.load_trajectories`
du côté Python). Le chemin du fichier est exposé dans le rapport
(`trajectory_file`).

### Ligne de commande (`--bench N`)

```bash
# tâche EVA (départ à 300 u à l'est, comme evaluate.py) :
cargo run --release -- --headless --bench 200 --target eva
# boucle de minage du vaisseau (économie) :
cargo run --release -- --headless --bench 20 --target ship --scenario economy
# options utiles : --seed S (graine du premier épisode), --max-steps N (garde-fou)
```

Le lot s'exécute au démarrage, le rapport est imprimé puis l'interface
continue de servir (l'entraîneur peut le relire via `GET /bench`).

### Côté entraîneur (`tools/trainer/bench.py`)

```bash
python3 bench.py --episodes 200 --target eva        # tâche EVA
python3 bench.py --episodes 20 --target ship --scenario economy  # boucle de minage
python3 bench.py --episodes 20 --target eva --trajectories  # + trajectoires RL (fichier JSONL)
```

### Mode hybride : politique externe vs autopilote sur les mêmes épisodes

`tools/trainer/evaluate.py --backend hybrid` mesure une **politique externe**
(seek, random, idle) contre l'**autopilote du jeu** sur des **épisodes
identiques** : l'autopilote joue d'abord le lot en continu dans le processus
(`POST /bench`, centaines d'épisodes/s), puis la politique externe rejoue les
mêmes épisodes pas à pas (HTTP) - même graine, même cible, même position,
même scénario. Le rapport compare épisode par épisode (dénouement et
récompense des deux côtés) et résume les moyennes.

```bash
python3 evaluate.py --backend hybrid --strategy seek --episodes 10 --target eva
```

Mesure réelle (5 épisodes, graines 1..5, départ 300 u à l'est) : autopilote
**5/5** (récompense moyenne 943,8, ~136 épisodes/s au bench) contre `seek`
**4/5** (moyenne 705,0) - la politique paramétrée perd sur la maîtrise de
l'arrivée (plus lente, une graine détruite), l'écart que l'entraînement CEM
vise à combler.

Mesures réelles (release, machine de dev) : **~140-230 épisodes/s** pour la
tâche EVA (départ 300 u, ~5,6 s simulées et ~335 pas par épisode, secours
systématique) ; **~4-11 épisodes/s** pour la boucle complète de minage du
vaisseau en économie (aller-retour vers le champ minier, tir, collecte,
déchargement - ~30-50 s simulées et ~1500-3300 pas par épisode, livraison
systématique sur la plage de graines testée). Le lot est **déterministe à la
graine** : même demande → même déroulé (dénouements, pas, temps simulé,
récompense, trajectoires).

### Imitation hors-ligne de l'autopilote (`tools/trainer/imitate.py`)

Les trajectoires enregistrées (`trajectories` ci-dessus) servent à un
**apprentissage hors-ligne par imitation** : `imitate.py` entraîne un petit
**réseau de neurones** (Python standard uniquement, `nn.py` - aucune
dépendance) à reproduire les décisions de l'autopilote, épisode par épisode
(observation → action, séparation train/validation **par graine**).

```bash
python3 bench.py --episodes 30 --target eva --trajectories   # 1. trajectoires (JSONL)
python3 imitate.py --trajectories /tmp/meteors_mining_headless_*/trajectories_*.jsonl  # 2. entraîner
# 3. comparer la politique `nn` à l'autopilote sur les mêmes épisodes :
python3 evaluate.py --backend hybrid --strategy nn --policy nn_policy.json --target eva --episodes 8
```

Les **features** données au réseau sont les grandeurs que l'autopilote
calcule pour décider (visée vers la station, erreur d'alignement, vitesses
radiale et tangentielle, objets proches, économie) en plus de la cinématique
brute - fournir ces dérivées est ce qui rend le clonage **stable en boucle
fermée** (sans elles, une exactitude hors-ligne de 99 % échoue en conditions
réelles : chaque erreur déplace la trajectoire hors de la distribution
apprise).

Résultats réels (tâche EVA, départ 300 u à l'est) : exactitude hors-ligne
**~98 %**, et en boucle fermée contre le jeu **7-8/8** réussis (récompense
moyenne ~820-840, contre 943,8 à l'autopilote - la politique est plus lente,
~27 s contre 5,6 s, elle hésite plus). Limites du clonage pur, connues et
mesurées : pas de généralisation aux **départs hors distribution** (le
micro-simulateur, qui tire un angle de départ aléatoire par graine, échoue)
ni à la **boucle de minage complète** du vaisseau en économie (~0/6 :
décisions discrètes séquentielles, états internes de l'autopilote invisibles
dans l'observation) - la suite (DAgger / RL, §6) doit combler ces écarts.

## 5 quater. DAgger — étiquetage expert des états visités par la politique

Le clonage pur (`imitate.py`) n'apprend que sur les trajectoires de
l'autopilote : en boucle fermée, la moindre erreur déplace la trajectoire
hors de la distribution apprise, et la politique n'a jamais appris à s'en
rattraper (mesuré : ~98 % d'exactitude hors-ligne, mais 0/8 sur des départs
**hors distribution** - angle d'éjection aléatoire). **DAgger** corrige cela
en déployant la politique elle-même et en étiquetant chaque état qu'elle
visite avec l'action de l'expert - rendu possible par le champ `expert` de
l'observation (§3).

`tools/trainer/dagger.py` : à chaque itération, `episodes` épisodes de la
politique courante (départs variés autour de la station - distances et
angles tirés des graines), chaque pas → `(features, action experte)` agrégé
au jeu de données (qui s'amorce avec les trajectoires de l'autopilote via
`--init-trajectories`), puis ré-entraînement du MLP sur tout l'agrégat
(séparation train/validation par graine, comme `imitate.py`).

```bash
python3 bench.py --episodes 12 --target eva --trajectories          # 1. amorce
python3 imitate.py --trajectories /tmp/.../trajectories_*.jsonl     # 2. clone de départ
python3 dagger.py --init-trajectories /tmp/.../trajectories_*.jsonl \
    --init-policy nn_policy.json --iterations 3 --episodes 3 \
    --spawn-dists 300,500 --target eva                              # 3. DAgger
python3 evaluate.py --backend hybrid --strategy nn --policy dagger_policy.json   # 4. vs autopilote
```

Deux corrections structurelles ont accompagné la mise en place :

1. **Observabilité de l'expert** : l'action de l'autopilote est publiée avec
   l'observation (`expert`), et son **hystérésis de frein tangentiel EVA**
   (`eva_tang_braking`) est exposée en feature - sans elle, deux
   cinématiques identiques portent des étiquettes contradictoires dans la
   bande 7-15 u/s de vitesse tangentielle.
2. **Tête de rotation mutuellement exclusive** (`nn.py`) : l'autopilote
   n'appuie jamais gauche **et** droite ensemble ; deux sigmoïdes
   indépendantes laissaient la politique entraînée coincée à les enfoncer
   toutes les deux (aucune rotation nette). La rotation est désormais un
   **softmax {gauche, droite, rien}**, `up`/`down`/`fire` restant des
   sigmoïdes indépendantes.

**Mesures** (release, mode headless, graines 101..109, départs 300/500 u) :
la politique DAgger apprend à prédire l'expert sur ses propres états
(exactitude de validation sur les graines de déploiement : ~12 % à la
première itération → ~98 % à la troisième) et corrige l'action initiale sur
les départs hors distribution (le clone partait dans le mauvais sens et
poussait en étant désaligné). Le **bouclage complet reste ouvert** : en
boucle fermée la politique entraînée tourne vers la station mais ne tient
pas encore le rythme de poussée/freinage de l'expert (0/8 en simulateur sur
angles aléatoires, comme le clone) - il faut davantage d'itérations DAgger
(chaque itération coûte quelques minutes en Python pur), puis les variantes
DART/DAgger avec replanification, avant le RL (DQN/PPO, §6).

### DAgger sur épisodes à objectifs (cible vaisseau) — mesures et corrections

Le premier lancement de DAgger sur la **boucle du vaisseau** (scénario à
objectifs `campaign_prospector`, cible `ship`) a révélé deux bugs côté jeu -
corrigés et testés :

1. **Épisodes vaisseau bloqués dans l'attente du départ** (`dagger.py`) : la
   boucle d'attente du début d'épisode attendait `station_dist ≥ 15`, une
   condition **impossible pour un vaisseau qui démarre à quai** (distance 0,
   et le pilote externe engagé coupe l'autopilote - rien ne déverrouille le
   vaisseau). Le lancement tournait en boucle sur ~2 000 connexions HTTP/s
   (épuisement des ports éphémères : `connect` en SYN-SENT) pendant ~2 h sans
   produire un seul pas d'entraînement. La garde de distance ne s'applique
   désormais qu'aux épisodes EVA (l'EVA est éjecté loin de la station) ; un
   épisode vaisseau démarre à quai et c'est la politique qui doit déverrouiller
   (vérifié : elle appuie une commande de déplacement → les liens se
   rétractent).
2. **Banc d'essai faussé par un pilote externe resté engagé** (`driver.rs` +
   `headless.rs`) : un épisode rejoué pas à pas se termine pilote externe
   **engagé, boutons encore enfoncés** ; or `input::player_controls` donne
   priorité au pilote externe **même quand l'autopilote est allumé** - le
   banc suivant (mesure de la référence) était donc piloté par les actions
   restées enfoncées : le vaisseau poussait en permanence, brûlait son
   carburant et mourait avant la première mission (0/5 objectifs au lieu de
   2/5, reproductible). `run_bench` **dégage le pilote externe et relâche ses
   actions** au départ (`clear_driver`, testé par
   `bench_disengages_a_stuck_external_driver`).

Mesures après corrections (release, headless, mêmes épisodes - graines 1..3,
`campaign_prospector`, 60 s) :

| stratégie | ép. 1 | ép. 2 | ép. 3 | moyenne |
|---|---|---|---|---|
| autopilote (référence) | delai · 80,4 (2 obj.) | delai · 84,2 (2 obj.) | delai · 39,8 (2 obj.) | **68,1** |
| clone (imitation pure) | delai · −170 | delai · −170 | delai · −170 | **−170,0** |
| DAgger (2 itér. × 3 ép.) | **detruit · +249,4 (2 obj.)** | delai · −170 | delai · −170 | **−30,2** |

DAgger améliore nettement le clone : la politique déverrouille, décolle,
**mine, rapporte et accoste** sur l'épisode 1 (2 objectifs complétés avant
d'être détruite - récompense +249 vs −170), mais ne tient pas encore la
boucle complète sur les trois graines (elle pousse trop peu après le départ
et tire en continu - dérive de distribution résiduelle, exactitude de
validation par graine de déploiement 16,8 % → 0,0 % au fil des itérations :
le sur-apprentissage des états majoritaires de l'amorce). C'est l'écart
que les itérations DAgger supplémentaires et le RL (§6) doivent combler.

## 5 quinquies. Épisodes à objectifs DAG — les missions comme langage de tâche/récompense

Les épisodes de l'auto-entraînement se jouaient jusqu'ici sur deux tâches
codées en dur (EVA → station, ou boucle de minage du vaisseau). Les
**scénarios à objectifs** de l'éditeur DAG (`scenarios/*.scenario.json`,
`objective_tracker.rs`) les enrichissent : les **missions** du scénario
(chaîne de prérequis, conditions chiffrées, récompenses) deviennent la tâche
et la récompense de l'épisode.

### Protocole : choisir un scénario à objectifs

`POST /reset` et `POST /bench` acceptent `"scenario": "<id>"` en plus de
`free` / `economy` : `"scenario":"campaign_prospector"` (ou tout id d'un
scénario chargé depuis `scenarios/*.scenario.json`). Côté jeu, le scénario
est résolu en `EpisodeScenario::Custom(index)`, appliqué par
`reset_episode` (`scenario::apply_start` initialise les ressources ET le
suivi des objectifs DAG) ; un id inconnu est refusé (400).

### Observation : la mission et sa progression

L'observation `/obs` expose le **langage de tâche** : `objectives_total`,
`objectives_completed`, `objective_bonus` (bonus cumulé de l'épisode) et la
liste `objectives` - chaque objectif avec `id`, `title`, `unlocked` (mission
en cours : prérequis satisfaits et pas complété), `completed` et la
progression chiffrée de sa condition (`current` / `required` : météores
détruits, crédits, accostages, secondes de survie… - mêmes valeurs que
l'évaluation du jeu, via `objective_tracker::progress_of`).

### Terminaison et récompense

- **Terminaison** : quand tous les objectifs du scénario sont complétés,
l'épisode vaisseau se termine en `objectives_complete` (mission accomplie) -
la livraison de soute, elle, n'est plus qu'une étape de la boucle et ne
termine plus l'épisode (seuls la destruction ou le garde-fou le stoppent
sinon).
- **Récompense** : chaque complétion d'objectif pendant l'épisode rapporte
`OBJECTIVE_BONUS` (200, côté jeu `src/driver.rs` comme côté entraîneur
`tools/trainer/eva_env.py`), ajouté à la récompense d'épisode, que
l'épisode se termine ou non - la progression partielle d'une mission paie
(`+200` par objectif, `+1000` pour la mission accomplie).
- **Rapport** : chaque épisode du banc d'essai expose ses `objectives_completed`
/ `objectives_total` et son `objective_bonus` ; le rapport agrège les
épisodes gagnés par objectifs (`objectives_complete`).

### Côté entraîneur

`bench.py`, `evaluate.py` et `dagger.py` acceptent `--scenario <id>` ; les
features du réseau (`nn.py`, version 7) incluent la progression des
objectifs (part complétée, mission courante et son avancement) - la politique
apprend sur le langage de tâche du scénario. Le simulateur EVA reste sans
objectifs (features à zéro, comportement inchangé).

**Mesure** (release, mode headless, scénario `test` - survivre 30 s puis
débloquer le mode inertiel) : l'autopilote complète les **2/2 objectifs** en
~30 s simulées et l'épisode se termine en `objectives_complete`
(récompense 1340 = 1000 − 2·30 + 2·200). Sur `campaign_prospector` (5
missions chaînées, dont 50 météores), la boucle complète dépasse le garde-fou
de 120 s : l'épisode s'arrête en `delai` avec 1-2/5 objectifs complétés -
c'est la tâche longue que la suite (DAgger / RL) doit apprendre à boucler.

## 5 sexies. RL — PPO sur l'observation complète (Phase 3)

L'étape « RL » de la Phase 3 : un apprenant qui dépasse l'autopilote de
référence en apprenant de la **seule observation**. Exercée sur la tâche EVA
du simulateur (`eva_env.py`, graines hors entraînement 11..15). Références :
autopilote du jeu **943,8** (6/6 en conditions réelles, headless) ; contrôleur
`seek` (réglage robuste de l'entraînement CEM) **≈ 893** (5/5 en simulateur,
entrée ~48 u/s - la pénalité d'arrivée trop rapide lui coûte ~90 points).
L'autopilote du jeu est **porté en Python** (`autopilot_ref.py`, mêmes
formules et constantes) et reproduit la référence en simulateur : **941,3**
(6/6, graines 1..6) - c'est lui qui sert de barre hors-ligne (mesure sans
processus headless, `evaluate.py --reference`, `test_reward_gap.py`).

### DQN : essayé, écarté - l'horizon long tue le bootstrap

Un premier `dqn.py` (Q-net MLP, relecture d'expérience, ε-gourmandise,
réseau cible gelé, TD(0) MSE) n'apprend pas, quels que soient l'échelle des
récompenses, le bornage de l'erreur TD, l'amorce experte du replay ou le
mélange expert/aléatoire des mini-lots. La cause est **structurelle** :
~1100 pas pour rentrer de 300 u, et avec γ = 0,99 le +1000 de la
récupération n'atteint jamais les états de départ (0,99¹¹⁰⁰ ≈ 0) - le
Q-learning bootstrapé ne reçoit aucun signal sur les premiers pas, et
l'exploration aléatoire ne trouve jamais le cercle d'accostage (réussite
~0 %) pour amorcer la propagation. PPO est **on-policy** : le retour réel
d'un rollout atteint le départ, pas besoin de bootstrap ni de réussite
préalable - le script DQN est retiré au profit de `ppo.py`.

### PPO : l'infrastructure, livrée et branchée

`ppo.py` : PPO sur l'observation complète (157 features, `nn.py` v8) avec

- une politique **factorisée** comme les décisions de l'autopilote -
  sigmoïde poussée × softmax rotation {←, →, rien} (la structure éprouvée
  du MLP d'imitation `nn.py` : une softmax conjointe sur les 6 combinaisons
  plafonne à ~95 % d'exactitude et dérive en boucle fermée, la factorisation
  atteint ~99 % sur la poussée) + une tête de **valeur** (la ligne de base
  des avantages, sans bootstrap max) ;
- une **amorce experte par perturbation des états de départ**
  (`warmstart.py`, `--expert autopilot`) : imitation supervisée de
  l'**autopilote du jeu** (le portage Python, la référence) depuis des
  départs **perturbés** - au repos nez aligné (l'état qui gelait), cap et
  distance décalés, approches trop rapides à freiner, orbites à casser. Le
  rééquilibrage des classes est **adaptatif** (il ne duplique les pas rares
  que si l'expert est majoritairement inactif - l'amorce perturbée, elle,
  produit surtout de l'action) ;
- les **features de décision EVA** (`nn.py` v6) : `eva_braking`,
  `eva_tang_brake`, `eva_thrust_err` (erreur d'alignement par rapport à la
  direction de poussée **résultante**). Sans elles, l'imitation de
  l'autopilote est **structurellement impossible** : quand l'expert freine,
  il vise l'opposé de la station, et la feature d'alignement existante vaut
  alors ≈ 0 - deux cinématiques identiques portent des actions opposées et
  le réseau ne peut pas trancher (mesuré : la politique poussait nez
  désaligné, gagnait de la vitesse tangentielle et fuyait) ;
- retours actualisés **γ = 0,9995** (l'horizon long atteint le départ),
  avantages `G − V` normalisés par lot et **bornés (±2)**, objectif **clipé**
  (le rapport des probabilités est borné à [1−ε, 1+ε] : une mise à jour ne
  peut pas dégrader la politique d'un coup), bonus d'entropie, et **prime de
  progression** (fermer la distance vers la station paie, même dans les
  épisodes perdus - sans elle, le « moins mauvais échec » des rollouts est
  l'immobilité et PPO y converge) ;
- mesure en boucle fermée toutes les 5 itérations (graines hors
  entraînement, récompense `episode_reward` sans prime), meilleur point
  sauvegardé dans `ppo_policy.json` (métadonnées : version des features,
  évaluation), rejouable par `evaluate.py --strategy ppo --policy
  ppo_policy.json` (backend sim, live ou hybride).

### Le gel de la boucle fermée est levé

L'échec documenté était précis : l'amorce imitait `seek` sur **ses propres**
trajectoires, où l'état « nez aligné, loin de la station, au repos » n'est
traversé qu'une frame avant de pousser (p(↑) ≈ 0,35, sous-représenté) ; la
politique y gelait (−200,0), les rollouts ne produisaient **jamais**
d'approche complète et le +1000 n'entrait jamais dans les retours.

Deux correctifs, mesurés ensemble :

1. **départs perturbés** (`warmstart.py`) : l'expert est déroulé depuis des
   états de départ qui incluent explicitement l'état qui gelait (au repos,
   nez aligné, à plusieurs distances), des caps/distance décalés, des
   approches radiales trop rapides et des orbites ;
2. **features de décision EVA** (`nn.py` v6, cf. plus haut) : sans elles, le
   freinage de l'expert contredit la feature d'alignement et l'erreur de
   composition reste fatale quel que soit le jeu de données.

**Mesure (simulateur, graines d'évaluation 11..15, départ 300 u) :**

| amorce | rollouts atteignant la station | meilleure somme de récompenses de pas |
|---|---|---|
| ancienne (`seek`, départs nominaux) | 0/20 | ≈ 793 |
| **perturbée + features v6** | **16-17/20** | **≈ 1015** (réussite complète) |

La politique gloutonne réussit selon le tirage (0-4/5) : l'amorce n'est pas
encore un contrôleur déterministe fiable, mais le **signal d'apprentissage
existe** - les rollouts atteignent enfin le +1000, ce qui était la condition
manquante. L'**affinage PPO reste ouvert** : en l'état, les mises à jour
PPO dégradent l'amorce (retour au plateau ≈ −220 dès la première itération,
même à lr réduit) - la sauvegarde du **meilleur point** conservée par
`ppo.py` protège le résultat (la politique produite reste l'amorce).
L'infrastructure RL est livrée et branchée (entraînement, courbe
d'évaluation, sauvegarde/rejeu, évaluation sim/live/hybride), et la piste
maintenant ouverte est l'**affinage prudent** de l'amorce (learning rate,
ancrage KL / contrainte à la politique experte, plus de rollouts) et
l'extension à la boucle de minage du vaisseau.

## 5 septies. Référence **vaisseau** et extension de l'observation

La boucle de minage du vaisseau a désormais son **portage de référence**
(`tools/trainer/ship_autopilot_ref.py`) : la loi vaisseau de l'autopilote du
jeu (`src/autopilot.rs::autopilot_inputs`), portée telle quelle - mission de
la frame (soute pleine → accoster, hostile menaçant la station, ravitaillement
si réserves basses **et** payables, minerai à collecter, hostile à détruire,
stationnement), tir avec **retenue de feu** (cible achevée par des balles en
vol, ou minerais dans le corridor de tir), et conduite (cap, vitesse visée
selon la distance, esquive d'hostiles, poussée 4 directions en mode 4 WAYS).

Le portage vaisseau lit plus d'état que l'EVA : l'observation a donc été
**étendue** (`src/driver.rs`) avec exactement ce qui manquait à un portage
fidèle :

- **`moving_mode`** (la conduite 4 WAYS diffère des modes « nez ») ;
- le **centre du corps** de chaque objet proche (`center_x`/`center_y`) - la
  visée réelle (`body_center`), un météore asymétrique a son corps décalé de
  `position` ;
- la liste des **balles en vol** (`bullets`, **séparée** de `nearby` pour ne
  pas changer les slots des features du réseau) - la retenue de feu ne se lit
  pas dans la cinématique ;
- **`supplies_affordable`** (les prix du magasin ne sont pas dans
  l'observation) : l'autopilote ne rentre se ravitailler que si les réserves
  sont basses **et** qu'un paquet est payable.

**Fidélité mesurée** contre la loi réelle : l'autopilote du jeu est engagé sur
une vraie partie headless et le portage doit rendre, à **chaque frame**, la
même commande que le champ `expert` de l'observation
(`validate_ship_port.py`). Résultat : **100 % d'accord** sur un épisode
complet de boucle de minage (graine 7, scénario `economy`, **4 377 pas**
comparés, dénouement `delivered`). `test_ship_port.py` verrouille hors-ligne
les décisions clés sur des observations synthétiques (cap, 4 WAYS, soute
pleine, garde de la station, retenue de feu, ravitaillement).

### Micro-simulateur vaisseau **hybride** (`ship_env.py`)

La boucle de minage existe maintenant côté simulateur : `ship_env.py` est le
pendant vaisseau d'`eva_env.py`, avec une **frontière de fidélité assumée**
(le choix « hybride » de la phase de conception) :

- **fidèle** : cinématique du vaisseau (les quatre modes de déplacement,
  `thrust_vector`, `realistic_rotation_after_input`), tir (cadence, balle au
  pivot, `vitesse + 2`), minage (un tir = un triangle ; minerais libérés à la
  destruction), collecte, économie Progression (carburant, munitions, soute,
  prix) ;
- **fidèle depuis le chantier « représentatif »** (§5 nonies quater) : la
  **séquence de l'épisode** - vaisseau à quai **liens attachés**, rétraction de
  1,5 s au décollage (vaisseau figé au centre, **entrées ignorées**, tir
  compris), animation d'accostage de 3 s avant la boîte DOCK STATION,
  déchargement puis ravitaillement au **maximum achetable**, rétraction avant de
  repartir - et le **champ minier de la graine**, **importé du jeu**
  (`fixtures/ship_mining_fields.json` : même graine → même monde - positions,
  rayons et triangles vivants des météores enregistrés) ;
- **approché** (la frontière hybride restante) : la **géométrie** des météores
  est un **cercle** (centre + rayon) au lieu d'un mesh de triangles et les
  collisions sont cercle/cercle au lieu du SAT triangle à triangle ; un **rayon
  de collision effectif** et une **tolérance de ramassage** compensent la
  différence entre la borne `radius` et la surface réelle (mesurés contre la
  vraie partie de référence) ; les graines **sans champ enregistré** retombent
  sur un champ **synthétisé** (mêmes règles que le jeu, flux aléatoire
  différent - monde qui n'est pas celui de la partie).

**Fidélité de la cinématique : mesurée exacte.** `fixtures/ship_physics_windows.json`
est un extrait d'une **vraie partie** headless (état du vaisseau, action de
l'autopilote, état suivant, 90 fenêtres) ; le simulateur reproduit chaque pas à
la **précision machine** (erreur max ≈ 1,4 × 10⁻¹⁴) - c'est ce que verrouille
`test_ship_env.py`, sans processus de jeu.

**Fidélité de l'épisode : mesurée, à la piste près.** Rejoué **pas à pas**
contre une vraie partie (mêmes actions, même graine, champ importé), le
simulateur suit le jeu **exactement pendant toute la rétraction des liens**
(86 pas identiques, la première divergence n'étant que la frame d'arrondi de
fin de rétraction), reste sous **1 u d'écart** pendant ~170 pas (~3 s) et sous
10 u ensuite : le résidu est la géométrie en cercles et la tolérance de
ramassage. Le tir suit aussi (munitions identiques sur 899/900 pas). Mesure
complète de représentativité : `validate_ship_env.py` (§5 nonies quater).

**La référence dans le simulateur** : avec le champ **importé** (celui du jeu),
l'autopilote porté livre sur **les 12 graines enregistrées**, à des temps
comparables à la partie (graine 1 : 22,1 s hors ligne contre 22,1 s dans le
jeu ; graine 3 : 18,8 s contre 18,9 s) ; avec le champ **synthétisé**, il ne
bouclait que sur une partie des graines - l'instabilité venait des
quasi-manqués de minerai sur un monde qui n'est pas celui du jeu, pas de la
physique. La **vraie partie fait foi** : l'autopilote du jeu livre **10/12**
sur les graines 1..12 (temps moyen 31,4 s) dans cette même mesure d'épisode.

### Amorce par départs perturbés de la cible `ship` (`ship_warmstart.py`)

`ship_warmstart.py` est le pendant vaisseau de `warmstart.py` : il fabrique des
**départs perturbés** (le jeu démarre toujours le vaisseau **à quai** : le nez
désaligné, une vitesse initiale, une soute entamée, des réserves basses sont
donc des états hors distribution), déroule l'**expert autopilote** (`ship_autopilot_ref.py`)
depuis chacun, étiquette chaque pas visité (mêmes features et mêmes cibles que
le réseau de `nn.py`) et rééquilibre les classes. La **boucle fermée contre
l'autopilote** se mesure hors ligne (`sim_comparison_ship`, `--measure-sim`).

**Résultat honnête** (artefact par défaut : 10 graines × 4 distances, 60 000
pas étiquetés, 25 époques, **features v8**) : l'infrastructure est en place et
le champ de départ est couvert ; la **v8 relève nettement l'imitation**
(exactitude train **65,0 %**, validation **57,9 %** contre 39,4 % / 31,6 % en
v7) mais la boucle de minage **ne se ferme pas encore** - écart de récompense
**−447,8** contre l'autopilote dans le simulateur (**1/6 livraisons** contre
3/6 ; −566,2 et 0/6 en v7). C'est le même écart que le côté EVA - l'écart
restant est l'**affinage**, pas l'infrastructure. Le déploiement de cette
politique dans le jeu (§5 octies) rend l'écart **mesurable dans la partie**
elle-même : 0/6 livraisons contre 6/6 pour la loi scriptée.

Les **grandeurs de décision** du vaisseau (§5 nonies) sont dans les features
(v7 : balles en vol, retenue de feu, `supplies_affordable`, `moving_mode` ;
v8 : la **visée de mission de la conduite**) ; mesurées, elles rendent la
rotation **décidable** et relèvent l'imitation d'un cran, sans encore boucler
la boucle - le seuil d'alignement de la bande morte reste à apprendre.

### DAgger **dans le simulateur** (`--dagger-iterations`) — le clonage ne suffit pas

L'écart ci-dessus n'est ni un manque d'infrastructure ni de capacité : c'est
une **dérive de distribution**. Diagnostic mesuré sur la trappe exacte où le
clone se gare : le vaisseau **s'arrête** à 401,7 u de la station, soute vide,
position **identique pendant 80 s**, et la politique y **décide délibérément de
ne rien faire** (les trois sigmoïdes sous 0,5, rotation « none » à 1,0). Cette
décision est **fidèle à la distribution d'imitation** (l'expert ne stationne
jamais là : ses trajectoires sont toujours en vol), et c'est la boucle fermée
qui punit : la même exactitude d'imitation (98,3 % train / 95,9 % validation)
correspond à **0/6 livraisons** hors ligne contre **6/6** pour la loi scriptée.

> La trappe a depuis été **expliquée et corrigée** : l'état est franchissable
> (l'expert y **livre en 17,9 s**, en **ouvrant le feu**) et l'étiquette qui y
> était enregistrée ne contenait pas `fire` - le format des cibles était faux.
> Voir §5 nonies quinquies ; ce qui suit décrit le chemin qui y a mené.

Le remède est **DAgger**, et il se joue maintenant **hors ligne** :
`ship_warmstart.py --dagger-iterations N` fait jouer la **politique** dans le
micro-simulateur **représentatif**, étiquette les états qu'elle visite avec
l'expert (`ship_autopilot_ref.py`), les **agrège** au jeu de données et
ré-entraîne. Même principe que `dagger.py` (§5 quater), sans processus de jeu
ni requête HTTP par pas - ce que le simulateur représentatif vient de rendre
possible.

Coût : le roulage d'une politique **qui cale** va jusqu'au délai de l'épisode
(~1,3 ms par pas en Python pur) - d'où deux réglages : `--dagger-stall` coupe
un roulage **figé** (un point fixe n'apprend rien de neuf) et
`--dagger-starts dock` fait rouler depuis les départs **du jeu**, la seule
distribution où la politique atteint ses propres points fixes (§5 nonies
quinquies). `test_ship_env.py` verrouille le mécanisme : les étiquettes sont
celles de l'**expert** (pas celles de la politique), la politique visite bien
des états **hors** de la distribution de l'expert, et le coupe-circuit ne
tronque pas une politique qui commande.

## 5 nonies. Les grandeurs de décision du vaisseau (v7 puis v8)

Le diagnostic du §5 octies (les features n'exposent pas tout ce dont
l'autopilote vaisseau se sert pour décider) est traité en **version 7** de
`nn.py::obs_features` (143 entrées au lieu de 115) :

- **balles en vol** (`bullets`, jusqu'à `BULLET_SLOTS` = 4, + leur nombre) :
  la **retenue de feu** (`HOLD_FIRE_*`) ne se lit pas dans la cinématique du
  vaisseau ;
- les deux **indicateurs de retenue de feu** (`_ship_decision_features`) :
  cible quasi détruite achevée par une balle en vol, minerais dans le
  corridor de tir. Sans eux, la proximité **mutuelle** cible/balles ne se lit
  pas et le même vecteur porte `fire` et pas `fire` - même conflit que
  `eva_braking` (§5 sexies) ;
- **`supplies_affordable`** : la mission « rentrer se ravitailler » en dépend ;
- **`moving_mode`** en one-hot : en 4 WAYS la conduite pousse dans les axes de
  l'écran au lieu d'orienter le nez.

**Mesure** (mêmes épisodes que ci-dessus) : exactitude train 38,7 % → **39,4 %**,
validation 32,6 % → **31,6 %**, écart de récompense −582,5 → **−566,2**,
toujours **0/6 livraisons** dans le jeu. Les trois entrées sont donc
**nécessaires mais pas suffisantes** : le portage Rust est fidèle à **100 %**
(23 981 pas comparés, fixture ré-enregistré) et la politique reste mesurable,
mais elle ne boucle pas.

**Cause identifiée (mesurée).** La **conduite** de l'autopilote vaisseau vise
la **cible de sa mission** (minerai, hostile ou station selon la soute, les
réserves et la garde de la station), pas la station ; or les features n'exposent
d'erreur d'alignement que vers la **station**. Mesure sur 113 706 pas
d'expert : le sens de rotation de l'expert est expliqué à **50,3 %** par
l'erreur vers la station (le hasard) et **60,5 %** par celle vers l'objet le
plus proche - aucune des deux ne porte la visée suivie par la loi. C'est
l'analogue vaisseau de `eva_thrust_err` : la suite naturelle est d'exposer les
**variables de décision de conduite** (visée `aim` de la mission,
`desired_speed`, branche 4 WAYS, esquive), symétriquement à la v6, puis de
ré-entraîner.

### La visée de mission de la conduite (v8) - faite

La version 8 de `nn.py::obs_features` (**157 entrées**) livre exactement cela :
`_ship_drive_features` **rejoue la mission de la frame** (le choix de `Goal` de
`src/autopilot.rs`, priorité incluse : soute pleine, garde de la station,
ravitaillement, hostile à portée, minerai gardé) puis la **conduite** que
l'autopilote en tire, comme `_ship_decision_features` rejoue déjà la retenue de
feu :

- la **mission** en one-hot (`dock` / `attack` / `collect` / `patrol`) ;
- le **cap effectif** `aim` (l'**esquive** remplace le cap de mission quand un
  hostile va passer trop près - `collision_threat` / `avoid_aim`) et son
  **erreur d'alignement** `err = aim − orientation`, le seuil de rotation et de
  poussée se lisant dessus ;
- la **vitesse visée** (`desired_speed` : croisière, rampes d'arrêt de
  l'attaque et de la collecte, ralentissement d'accostage, rayon de
  stationnement) et la vitesse **projetée** sur le cap ;
- les composantes du mode **4 WAYS** (`ex`, `ey` : l'écart au vecteur de
  vitesse visé, qui décide des quatre directions de poussée à l'écran) ;
- les drapeaux de conduite : **esquive** active, menace **devant** (freinage
  pendant l'esquive), **survitesse** et **arrêt** (`settle`).

Hors vaisseau (le cosmonaute EVA pilote) le bloc vaut zéro, comme la loi.
Porté à l'identique dans `src/learned_pilot.rs` (`FEATURE_COUNT = 157`, mêmes
ordres d'opérations ; le format est en **v10** depuis la correction des
**cibles**, §5 nonies quinquies - ces blocs de features, eux, sont inchangés).

**Mesure - la rotation devient décidable.** Sur une vraie partie (graine 1,
autopilote scripté, 999 pas, mode DIRECTIONAL), la règle de rotation **rejouée
depuis le cap effectif v8** reproduit exactement la rotation de l'expert :
**100,0 %** des pas (3 classes), contre **63,3 %** pour la même règle appliquée
à l'erreur vers la **station** - l'information que la loi suit est bien livrée.

**Mesure - l'imitation relève d'un cran, la boucle ne se ferme pas.** Artefact
par défaut (60 000 pas étiquetés, 24 cachés, 25 époques) : exactitude train
39,4 % → **65,0 %**, validation 31,6 % → **57,9 %** ; micro-simulateur : écart
de récompense −566,2 → **−447,8**, livraisons 0/6 → **1/6** (autopilote 3/6) ;
dans le jeu : **0/6** livraisons contre **6/6** pour la loi scriptée. Portage
Rust **fidèle à 100 %** (mesuré alors ; **4 941 pas** avec l'artefact élargi de
§5 nonies bis, fixture ré-enregistré à chaque changement de poids).

**Résidu mesuré : la bande morte d'alignement.** Dans la partie, le réseau
d'arrive à **42,9 %** d'exactitude toutes touches, et son échec se **concentre
dans la bande morte** : **45,2 %** des pas justes pour `|err| ≤ 0,10 rad`,
contre **84,6 %** au-delà et **100 %** au-delà de 0,35 rad - or cette bande
couvre **68 %** de la trajectoire (le nez reste pointé, l'autopilote ne corrige
que par rafales). L'information est dans les features, mais le **seuil**
(0,10 rad ≈ 0,032 en feature normalisée par π) n'est pas appris : la prochaine
étape est d'exposer les **seuils de conduite** (drapeaux « aligné pour tourner
/ pousser », vitesse visée atteinte), voire d'élargir la capacité
d'apprentissage, puis de ré-entraîner.

La seconde piste a été suivie et **elle ferme la boucle** : c'était un
symptôme de capacité, pas un mur (§5 nonies bis).

## 5 nonies bis. L'élargissement de la capacité d'apprentissage - fait

L'hypothèse restante après la v8 était la **capacité** du réseau (24 cachés,
25 époques, 6 000 pas). L'élargir butait sur un fait : le MLP en **Python
pur** est lent - mesuré, `hidden=64` sur 6 000 pas coûte ~17 s par époque, et
plusieurs heures pour les réglages visés. L'élargissement commence donc par
le **rendre possible**.

### Un second moteur d'entraînement, optionnel (`nn.py`)

`MLP.train(..., backend=...)` accepte `python` (**défaut, aucune
dépendance** - la convention du répertoire), `numpy` (vectorisé) ou `auto`
(numpy s'il est importable, repli Python pur). Les deux chemins font **le même
apprentissage** (même perte BCE + entropie croisée de rotation, même élan,
même arrêt précoce, mêmes bornes numériques) ; seul le chemin vectorisé rend
les gros réglages atteignables : **~33 × plus rapide** (mesuré : 1,0 s contre
33 s sur 1 500 pas × 40 époques). Ce qui est **rejouable** n'en dépend jamais :
l'inférence (`forward`, Python pur) et le portage Rust restent les mêmes, les
poids sont sauvegardés en **listes Python** (`save_nn` les écrit en JSON), et
`backend="python"` reste le défaut. `test_nn_backend.py` verrouille
l'équivalence (les cas numpy se **sautent** sans numpy - la CI n'installe
rien).

### Les réglages, élargis

| réglage | avant | après |
|---|---|---|
| graines | 10 | **12** |
| sous-échantillonnage (`--stride`) | 10 | **4** |
| plafond par graine (`--balance-cap`) | 6 000 | **20 000** |
| pas étiquetés | 60 000 | **240 000** |
| jeu d'entraînement / validation | 6 000 / 15 000 | **24 000 / 60 000** |
| couche cachée | 24 | **64** |
| époques | 25 | **300** (arrêt précoce, patience 25) |

### Mesure - et une mise en garde

| | avant (24 cachés, 6 000 pas) | **élargi (64 cachés, 24 000 pas)** |
|---|---|---|
| exactitude train / validation | 65,0 % / 57,9 % | **78,9 % / 69,8 %** |
| micro-simulateur : écart de récompense | −447,8 (1/6) | **−574,7 (0/6)** |
| **dans le jeu**, livraisons (graines 1..6) | **0/6** | **5/6** |
| **dans le jeu**, livraisons (graines 1..12) | - | **7/12** contre **10/12** |

> ⚠ **Lecture corrigée (§5 nonies quinquies).** Les livraisons « apprises » de
> cette table datent du **format v8** : le **tir n'y était pas appris**. La
> sortie lue comme `fire` par le jeu était la sigmoïde d'un **virage à gauche**
> (`action_target` découpait `ACTIONS[:3]`, soit `up`, `down`, `left`), `fire`
> était **absent** de la cible et `left` comptait double. Les conclusions
> ci-dessous restent vraies (l'exactitude d'imitation ne prédit pas la boucle
> fermée), mais elles se lisaient sur une politique dont le **tir** était un
> accident : le format v10 corrige la cible et les poids v8 sont maintenant
> **refusés**.

Le micro-simulateur **pénalise** le réseau élargi alors que la partie le
**récompense** : sur cette cible, l'écart de récompense hors ligne n'est pas un
critère de sélection - il est même **anti-corrélé** au résultat réel. La raison
est connue depuis la construction du simulateur : il est **hybride**, avec des
météores **en cercles** et un champ minier **synthétisé**, alors que le jeu
engendre son champ ($\S$5 septies). La mesure qui fait foi reste celle **dans
le jeu** (`measure_in_game.py`). Ce résidu est **traité** en §5 nonies quater
(le simulateur rejoue désormais le monde de la graine et la séquence de
l'épisode, et son classement suit celui de la partie).

> **Correctif de mesure (16 septembre 2026).** L'artefact **embarqué** n'est
> plus celui de cette table : un ré-entraînement postérieur
> (`assets/ship_pilot_policy.json`, 134 633 pas étiquetés, 98,8 % d'exactitude
> d'entraînement) l'a remplacé, et **il ne livre aucune des 12 graines** dans le
> jeu (0/12 mesuré par `measure_in_game.py`, contre 10/12 pour la loi scriptée).
> La leçon de la table tient - ni l'exactitude d'imitation ni l'écart de
> récompense hors ligne ne prédisent la boucle fermée - et elle est désormais
> **mesurable sans lancer le jeu** : le simulateur représentatif reproduit ce
> verdict (12/12 scripté contre 0/12 pour le cerveau embarqué, §5 nonies
> quater). La suite est donc de **ré-entraîner** l'artefact sur ce monde-là.
>
> **Suite (18 septembre 2026).** Ré-entraînement fait sur le monde de la partie,
> mais c'est **une autre cause** qui expliquait ces 0/12 : la cible
> d'apprentissage ne contenait **pas le tir** (§5 nonies quinquies). L'artefact
> embarqué est depuis un **v10** (cibles corrigées, DAgger depuis le quai) qui
> livre **7/12** dans le jeu - comme l'artefact élargi ci-dessus, mais pour de
> bonnes raisons cette fois (le tir est **appris**).

**Ablation - la capacité est bien le levier.** Mêmes données élargies avec la
capacité d'**avant** (24 cachés, 25 époques) : validation 60,6 %,
micro-simulateur **−369,1** (1/6, donc *meilleur* hors ligne) mais **1/12
livraisons dans le jeu** seulement. Le gros réseau passe donc de **1/12 à
7/12** : ce n'est pas le volume de données qui ferme la boucle, c'est la
**capacité**. La bande morte d'alignement n'était pas un mur mais un symptôme.

Le résidu change de nature : le réseau **livre** désormais, mais **plus
lentement** que la loi (49,2 s contre 31,4 s de temps simulé moyen) et
échoue encore 5 fois sur 12. La piste ouverte n'est plus « élargir » mais
**affiner** et **rendre le micro-simulateur représentatif**.

## 5 nonies ter. Les seuils de conduite (v9) - faits, et ce qu'ils ont appris

La suite identifiée était d'exposer les **seuils** de conduite : la v8 livrait
les grandeurs (cap, erreur, vitesse visée) mais **pas les constantes auxquelles
la loi les compare**, si bien que la bande morte d'alignement
existait dans les features sans être lisible (`err/π` y vaut 0,032).

### Ce que la v9 ajoute (157 → 171 entrées)

- **`_ship_drive_features` (+12)** : l'erreur **en unités du seuil**
  (`err / TURN_DEADBAND`, `err / THRUST_DEADBAND` - le seuil tombe à ±1) et les
  comparaisons qui décident : rotation droite/gauche, bande morte de rotation,
  alignement de poussée, « trop lent pour la vitesse visée »,
  « plus rapide que la vitesse visée », arrêt (`desired < 0,4` et
  `v > 0,02`), frein d'esquive (`velocity > 0,5`), branche 4 WAYS
  (`|ex| > |ey|`) et rotation résiduelle (`|rotation| > 0,06`) ;
- **`_ship_decision_features` (+2)** : le **seuil d'alignement de tir**
  (`|aim − orientation| < 0,14`) vers la cible de tir et l'erreur normalisée
  par ce seuil - l'erreur vers la cible de **conduite** existait, pas celle
  vers la cible de **tir**.

Ce sont des **prédicats d'état** (de quel côté d'une constante on est), jamais
la commande combinée. Portés à l'identique dans `src/learned_pilot.rs`
(`FEATURE_COUNT = 171`), portage mesuré **fidèle à 100 %** (3 379 pas, fixture
ré-enregistré). Cette variante a été mesurée avec un numéro de format propre
(`9`) puis **écartée** : le numéro **10** qui est déployé correspond à une autre
correction - le format des **cibles** (§5 nonies quinquies).

### Résultat - l'imitation devient parfaite, la boucle fermée **régresse**

| | v8 | **v9** |
|---|---|---|
| exactitude train / validation | 78,9 % / 69,8 % | **100,0 % / 100,0 %** |
| micro-simulateur : livraisons | 1/6 | **1/6** |
| **dans le jeu**, livraisons (12 graines) | **7/12** | **3/12** |

Autrement dit : rendre la loi **entièrement décidable** ne suffit pas - et
**dégrade** le résultat. C'est le troisième enseignement de cette cible, et il
est important : le facteur limitant n'est pas la **représentation** mais
l'**écart de distribution** entre l'entraînement et le déploiement.

### Diagnostic du mode d'échec (mesuré)

Sonde d'un épisode appris (graine 1) : le réseau accède avec l'expert à
**100 %** sur `up`/`down`/`left`/`right`, mais à **1,9 %** sur `fire` - il
**tire quasiment en permanence**, vide ses munitions, ne détruit presque rien
(2 météores en 1 000 s), ne collecte pas, et finit détruit. Sur des frames où
l'expert ne tire jamais (aucun hostile à portée), le réseau tire à **p = 1,0**,
saturé. Les données d'entraînement couvrent pourtant largement ce cas
(85 % des pas sans cible à portée, `fire = 0`), et le réseau y est juste à
100 % : il est donc **hors distribution dans la partie**, pas mal entraîné.

### Deux causes réelles, trouvées et corrigées en route

1. **Un centre de forme à `NaN`.** `compute_shape_center` (`src/shape.rs`)
   divisait par un compteur de triangles vivants **nul** (`0/0`) quand une
   forme n'a aucun triangle vivant : le centre devenait `NaN`, puis
   contaminait `center`, les sommets monde et l'observation publiée - où
   `serde_json` l'écrit `null`. **Tous** les minerais d'une vraie partie
   étaient dans ce cas (150/150 mesurés). Le portage refuse désormais de
   recentrer une forme vide (il garde le centre précédent) et
   `nn.py::_finite` / `learned_pilot.rs::finite` neutralisent en plus toute
   valeur non finie (le portage doit reproduire **exactement** le même calcul).
2. **Le mode de déplacement par défaut du micro-simulateur ne correspondait
   pas à la partie.** Le simulateur démarrait en **REALISTIC (3)** alors que
   les 90 fenêtres de `fixtures/ship_physics_windows.json` (une vraie partie
   `economy`) sont **toutes** en **DIRECTIONAL (2)**. Tout l'entraînement se
   faisait donc sur un mode que le jeu n'utilise pas - one-hot jamais vu à
   l'inférence. Corrigé (`ShipSim` démarre en DIRECTIONAL) : le simulateur
donne alors **6/6 livraisons** pour l'autopilote, comme le jeu.

### Ce qu'il reste à faire (et ce qui ne marche pas)

- **La régularisation ne suffit pas** : un entraînement avec bruit d'entrée
  (`--noise 0,08`, désormais exposé) redonne 100 % d'exactitude et le même
  résultat - les seuils quasi binaires saturent les sorties, le bruit ne
  change pas la décision.
- **La vraie piste est de supprimer l'écart de distribution** : entraîner
  (ou affiner) sur des états **du jeu réel** plutôt que du micro-simulateur -
  c'est exactement ce que fait `dagger.py` pour l'EVA (collecter des états
  visités, les étiqueter avec l'expert, ré-entraîner), et le simulateur
  vaisseau étant mesuré **non représentatif** (§5 nonies bis), la cible
  `ship` a besoin du même traitement. C'est un chantier, pas un réglage.
- **Conséquence assumée** : avec 3/12 contre 7/12, la politique v9
  **n'est pas un progrès** - l'artefact embarqué doit être choisi sur la
  mesure **dans le jeu**, jamais sur l'exactitude d'imitation ni sur
  l'écart de récompense du simulateur.

## 5 nonies quater. Le micro-simulateur **représentatif** - fait

§5 nonies bis avait laissé un résidu précis : le micro-simulateur **désignait
le mauvais réseau** - l'écart de récompense hors ligne était *anti-corrélé* au
résultat réel. La cause n'était ni la physique ni l'imitation, mais trois
écarts entre le simulateur et la partie, tous **mesurés** :

1. **le monde** : le champ minier était **synthétisé** (mêmes règles, flux
   aléatoire différent) - une même graine donnait **deux mondes**, donc deux
   géométries de cibles ;
2. **le départ** : le jeu retient le vaisseau **à quai, liens attachés**, et
   **1,5 s** après la première commande de mouvement (rétraction des liens), en
   **ignorant toutes les entrées** - le tir compris. Le simulateur démarrait
   immédiatement : le vaisseau partait 1,5 s plus tôt, tournait et **tirait**
   pendant que le jeu ne faisait rien (munitions consommées pour rien, cap et
   position d'entrée en vol différents) ;
3. **l'accostage** : le jeu enchaîne **animation de 3 s** (vaisseau pivoté vers
   la droite et recentré, entrées ignorées), boîte DOCK STATION, déchargement,
   ravitaillement, puis rétraction de 1,5 s - la livraison est **datée à la
   boîte**. Le simulateur livrait à l'instant de l'entrée dans le cercle :
   l'épisode était ~4,5 s plus court, donc la **récompense** fausse.

### Ce qui a été changé

- `ship_env.py` porte la **séquence du jeu** (`game.rs::update`,
  `docking.rs`) : liens attachés, rétraction de 1,5 s (vaisseau figé au centre,
  orientation 0, entrées ignorées), animation d'accostage de 3 s, boîte DOCK
  STATION, déchargement puis ravitaillement au **maximum achetable**
  (`autopilot_handle_dock` / `autopilot_handle_shop`), rétraction avant de
  repartir. `docked` est désormais la définition **publiée par le jeu**
  (`dock_links || dock_anim > 0 || dock_retract > 0`), et `player_at_station`
  décide, comme en jeu, entre **déclencher l'animation** (retour) et
  **décharger** (pilote venant de la station) ;
- le **champ minier est importé du jeu** par graine
  (`fixtures/ship_mining_fields.json`, écrit par `measure_in_game.py
  --fields` depuis une vraie partie) : `ShipSim` rejoue le monde de la graine ;
- `validate_ship_env.py` mesure la **représentativité** hors ligne (sans
  processus de jeu) : jeu contre simulateur, graine par graine, accord des
  dénouements et verdict.

### Reproduction

```bash
cargo build --release && cargo run --release -- --headless       # 1. le jeu
cd tools/trainer
python3 measure_in_game.py --seeds 1 2 3 4 5 6 7 8 9 10 11 12 \
    --fields fixtures/ship_mining_fields.json                    # 2. traces + champs du jeu
python3 validate_ship_env.py                                     # 3. la représentativité
```

### Mesures (graines 1..12, cible `ship`, scénario `economy`)

Mesures du **18 septembre 2026**, artefact **v10** embarqué (§5 nonies
quinquies) - les mêmes chiffres sortent de `validate_ship_env.py` (hors ligne)
et de `measure_in_game.py` (dans le jeu) :

| | jeu | simulateur |
|---|---|---|
| loi scriptée : livraisons | **10/12** (31,4 s de moyenne) | **12/12** (21,8 s) |
| réseau embarqué : livraisons | **7/12** (30,0 s de moyenne) | **11/12** |
| accord des dénouements | - | scripté **10/12** (83 %) · appris **8/12** (67 %) |
| écart moyen de temps (livraisons communes) | - | **11,1 s** |

- **le classement ne s'inverse pas** : le jeu préfère la loi scriptée
  (**10** livraisons contre **7**), et le simulateur la préfère aussi (**12**
  contre **11**) - c'est ce qui manquait pour choisir un artefact sans lancer le
  jeu. Un test le verrouille (`test_ship_env.py::TestRepresentativity` : champ de
  la graine identique au fixture, accord ≥ 80 %, **inversion** de classement
  refusée) ;
- **mais la marge hors ligne ne suffit pas à sélectionner** : l'accord du
  cerveau **appris** est de **67 %**, sous le seuil de 80 %, et le simulateur
  l'**annonce** (« ne pas sélectionner de politique sur cette mesure »). La
  raison est nommée : le simulateur est plus **clément** que la partie - il
  n'**y a pas de destruction** là où le jeu perd le vaisseau (§ ci-dessous) ;
- la fidélité de l'épisode, elle, se mesure à la **piste** : exacte pendant
  toute la rétraction (86 pas), < 1 u pendant ~3 s, < 10 u ensuite
  (§5 septies).

### Ce qui reste approché (et ce que ça coûte)

Deux résidus, mesurés :

- la **géométrie** - un météore reste un **cercle** (rayon de collision effectif
  × 0,6, marge de ramassage des minerais). C'est la source de l'écart de temps
  (11,1 s) et des graines qui divergent ;
- la **destruction** - le jeu perd le vaisseau par **collision** sur les graines
  2 et 5 (11,0 s et 8,4 s) là où le simulateur livre (20,1 s et 22,2 s). C'est
  désormais **le** poste d'écart du cerveau appris : sur les 5 graines qu'il
  échoue dans le jeu, 2 sont des collisions, 3 des délais, **aucune n'est la
  trappe figée** d'avant §5 nonies quinquies.

Le résidu de **généralisation** du réseau, lui, ne se règle plus dans le
simulateur : l'imitation porte sur la géométrie de la partie (champ importé), et
l'artefact a été **ré-entraîné sur ce monde-là** puis **mesuré dans le jeu**
(§5 nonies quinquies) - la seule mesure qui fait foi pour l'artefact embarqué
(§5 octies).

## 5 nonies quinquies. Le format des **cibles** (v10) — le tir n'était pas appris

Le résidu de §5 nonies quater n'était pas un manque de capacité, ni d'abord un
écart de distribution : **la cible d'apprentissage était fausse**. C'est la
cause racine de la trappe du pilote appris, et elle explique d'un coup les trois
observations restées séparées jusqu'ici (le clone qui se fige, la boucle de
minage qui ne se ferme qu'à moitié, le « désaccord de 1,9 % sur `fire` »).

### 1. Le symptôme : un point fixe

Rejouée depuis le quai (graine 1, simulateur **représentatif**), la politique
pousse ~2 s, puis **ne commande plus rien** : le vaisseau s'arrête à 401,7 u de
la station et sa position reste **identique pendant 80 s** (soute vide, 26
munitions). Ses sorties y valent `[0,045, 0,00, 0,00, 0,00, 0,00, 1,00]` : les
trois sigmoïdes sous 0,5 et la rotation « none » à 1,0 - la politique **décide
délibérément de ne rien faire**.

### 2. L'état est franchissable, et l'expert le sait

Lâché **depuis cet état exact**, l'expert (loi scriptée portée) **livre en
17,9 s** : il ouvre le feu, tue un météore dès la 1,7 s et embarque les
minerais. L'action qui ouvre la trappe est donc le **tir** - et c'est bien ce
que l'expert répond sur cet état, de façon **sans état** (trois appels, trois
fois `fire`).

### 3. La cible ne pouvait pas l'exprimer

L'étiquette **enregistrée** sur cette même observation valait pourtant
`[0, 0, 0, 0, 0, 1]` - « ne rien faire ». La raison est dans `nn.py` :

```python
ACTIONS = ("up", "down", "left", "right", "fire")
SIGMOID_OUTPUTS = 3
y = [1.0 if action.get(a) else 0.0 for a in ACTIONS[:SIGMOID_OUTPUTS]]  # ← v8
```

`ACTIONS[:3]` vaut `(up, down, left)` puisque `left`/`right` sont rangés **avant**
`fire` : la cible des trois sigmoïdes était `up`, `down`, **`left`**. Mesuré :

| action de l'expert | cible v8 | cible v10 |
|---|---|---|
| `fire` | `[0,0,0, 0,0,1]` | `[0,0,**1**, 0,0,1]` |
| `left` | `[0,0,**1**, 1,0,0]` | `[0,0,0, 1,0,0]` |

Autrement dit : **`fire` était absent de l'apprentissage** et **`left` comptait
double** (sa propre sigmoïde, en plus de la tête de rotation). La sortie que le
jeu lit comme `fire` (`greedy()`, `policies.nn_policy`) était la sigmoïde
entraînée sur les virages à gauche : le pilote appris **tirait par accident**,
et la boucle de minage ne se fermait que par accident. Le « désaccord de 1,9 %
sur `fire` » relevé plus haut (§5 nonies bis) n'était pas une dérive de
distribution : c'était **l'étiquette**.

Conséquence directe sur DAgger, mesurée : sur la trappe, la cible disait « ne
rien faire » - agréger ces états **apprenait la trappe**. Trois itérations
DAgger (roulages à 600 u, cible v8) : validation 95,9 % → 77,0 %, boucle fermée
**0/6** avec le vaisseau **détruit** sur la graine 4 (§5 nonies quater ne
pouvait donc pas conclure).

### 4. La correction (format **v10**)

- `nn.py` : `SIGMOID_ACTIONS = ("up", "down", "fire")` et `TURN_ACTIONS =
  ("left", "right", "none")` définissent l'**ordre des sorties** (distinct de
  l'ordre des boutons `ACTIONS`) ; `action_target` les suit, `summary` aussi ;
- l'artefact **déclare** `sigmoid_actions` / `turn_actions`, et les deux
  lecteurs **refusent** un fichier qui déclare autre chose (`nn.py::load_nn`,
  `src/learned_pilot.rs::parse`) : des poids v8 sont **refusés** au lieu d'être
  pilotés de travers ;
- `FEATURES_VERSION` passe à **10** (même mécanisme que les versions de
  features) : la garde vaut aussi pour le portage Rust.

Deuxième correction, trouvée en instrumentant la trappe : les roulages DAgger
doivent partir de la **distribution de départ de la tâche** - le vaisseau **à
quai** (`--dagger-starts dock`, désormais le défaut). Partant de départs
**écartés** (`--dagger-dists 600`), les roulages n'atteignaient **jamais** la
trappe d'évaluation (distance L∞ minimale de **0,96** à cet état, la géométrie
de vol n'étant pas la même) : la correction n'existait donc pas dans les données
DAgger. Depuis le quai, la trappe est visitée **exactement** (600 pas,
étiquetés par l'expert). S'y ajoute un **coupe-circuit** (`--dagger-stall`,
600 pas) : un point fixe n'apprend rien de neuf, le rejouer ne coûte que du
temps de simulateur (~10 min par itération avant, ~5 min après).

### 5. Le résultat

```bash
cd tools/trainer
python3 ship_warmstart.py --dagger-iterations 3 --dagger-starts dock \
    --dagger-stride 4 --measure-sim --output ship_warmstart_policy.json
```

| étape | boucle fermée hors ligne (graines 1..6) | récompense moyenne |
|---|---|---|
| amorce (clonage seul) | **1/6** | −118,7 |
| DAgger 1 | **4/6** | 522,2 |
| DAgger 2 | 2/6 | 116,5 |
| **DAgger 3** (artefact déployé) | **6/6** | **958,0** (autopilote 960,5) |

La progression n'est pas monotone (la trappe se déplace d'une itération à
l'autre) : c'est le **dernier** point de reprise qui est déployé, et
`--output` est réécrit après **chaque** itération pour qu'un run interrompu
laisse un artefact que la mesure vient de qualifier.

**Dans le jeu** (`measure_in_game.py`, 12 graines) : **7/12 livrés contre 10/12**
pour la loi scriptée, à **30,0 s** de temps simulé moyen contre 31,4 s - il
livre **aussi vite** que la loi, et plus vite sur certaines graines (graine 7 :
21,4 s contre 71,6 s). Les 5 échecs ne sont **plus** la trappe : 2
**collisions** (graines 2 et 5) et 3 **délais** (6, 8, 9) - c'est la frontière
hybride du simulateur (§5 nonies quater), pas le format des cibles.

**Portage Rust** : **1 423 / 1 423 pas identiques (100 %)** avec les nouveaux
poids, sur trois épisodes de minage (les graines 7 et 21 **livrent**).

### 6. Les verrous

`test_nn_backend.py` (`ActionTargetTest`) : les sigmoïdes sont `up`, `down`,
`fire` ; `left` n'y est plus dupliquée ; **relire une cible avec le décodeur du
jeu rend l'action d'origine** sur les 24 combinaisons ; un fichier de poids qui
déclare l'ancien format est **refusé** ; et - bout en bout - un réseau entraîné
sur ce format exécute bien l'action de l'expert (> 95 %, contre **0 %** en v8,
vérifié). `test_ship_env.py` verrouille côté épisode : les roulages DAgger
partagent la distribution de départ de la mesure, la trappe est visitée, le
coupe-circuit ne tronque pas une politique qui commande.

## 5 octies. Phase 4 — la politique apprise **déployée dans le jeu**

Objectif de la phase : que le réseau entraîné **tourne dans le jeu**, comme
stratégie « autopilote » alternative - mesurable, sélectionnable, et fidèle
au calcul de l'entraîneur.

### Le portage (`src/learned_pilot.rs`)

Deux pièces sont rejouées à l'identique : l'**extraction de features**
(`nn.py::obs_features`, version **10**) sur l'**observation du jeu**
(`driver::Observation`) et le **réseau** (`nn.py::MLP` : couche cachée tanh,
sigmoïdes `up`/`down`/`fire`, softmax de rotation `left`/`right`/`none`). Les
poids sont **embarqués dans le binaire** (`include_str!` de
`assets/ship_pilot_policy.json`, écrit par
`ship_warmstart.py --output ../../assets/ship_pilot_policy.json`) : mettre à
jour le cerveau déployé = ré-entraîner **et** recompiler. Un asset illisible
n'est pas joué (le portage **retombe sur la loi scriptée**) et la case refuse
de s'allumer.

Détail qui compte : l'**ordre des opérations** de la couche est celui de
`nn.py` (somme des termes, **puis** ajout du biais). L'addition flottante
n'étant pas associative, un ordre différent suffit à faire basculer la
rotation sur une probabilité limite - le portage doit être le même calcul, pas
seulement la même formule.

### La sélection dans le jeu

- **Case LEARNED PILOT** de l'écran de paramétrage (touche **Y**, clé
  persistée `learned_pilot`) : elle choisit le **cerveau** de l'autopilote -
  elle n'a d'effet qu'avec le pilote automatique allumé (X). L'indicateur du
  HUD affiche « AUTOPILOT (LEARNED) ».
- La **loi reste celle de l'entraînement** : le réseau ne décide que des
  entrées de pilotage ; les gestes d'accostage (décharger, se ravitailler,
  repartir) restent à la machine à états de l'autopilote - c'est exactement ce
  que faisait l'expert étiqueteur, qui les exécutait **hors** du réseau.
- Côté **entraîneur**, le protocole gagne la bascule `learned_pilot`
  (`POST /cmd {"autopilot":true,"learned_pilot":true}` : le jeu joue le
  réseau lui-même) et l'observation publie `learned_pilot` **et** `learned` -
  l'action du portage, miroir exact de `expert`.
- Le **banc d'essai éteint la stratégie apprise** (`headless::run_bench`) :
  il mesure la ligne de base **scriptée** ; sans ce nettoyage, une case
  LEARNED PILOT restée allumée ferait passer le réseau pour « l'autopilote du
  jeu » dans toutes les comparaisons suivantes (même classe de piège que le
  pilote externe resté engagé, §5 quater).

### Fidélité du portage — mesurée

`learned` permet la mesure symétrique de `validate_ship_port.py` : on laisse
le **jeu** jouer avec le réseau embarqué et on compare, à chaque pas, l'action
du portage Rust à celle de la politique Python **sur la même observation**.

```bash
cargo run --release -- --headless        # dans un autre terminal
cd tools/trainer
python3 validate_learned_port.py --scenario economy --seeds 1 2 3 4 --seconds 30
```

**Mesure : 4 941 / 4 941 pas identiques (100 %)**, sur trois épisodes de
minage (scénario `economy`, artefact v8 **élargi**, 64 cachés). Le portage Rust
lit `hidden` dans le fichier de politique et valide les dimensions :
**agrandir le réseau ne touche pas le Rust**. Le taux est verrouillé hors ligne par le
test unitaire Rust de `learned_pilot.rs`, qui rejoue le fixture
`fixtures/learned_pilot_windows.jsonl` (40 fenêtres d'une **vraie partie** :
observation, features de `nn.py`, action de la politique Python - régénérables
par `validate_learned_port.py --record`), plus la fenêtre-limite de
l'observation vide ; les features y sont reproduites à moins de 1e-9 et les
décisions **exactement**. `test_learned_pilot.py` verrouille le maillon Python
(l'asset embarqué reste rejouable par `nn.py`, le fixture reste cohérent) :
ré-entraîner ou changer les features **casse le test** tant que le fixture n'a
pas été ré-enregistré.

### Ce que la stratégie déployée vaut — mesuré, sans enjolivement

| artefact | dans le jeu, livraisons | temps simulé moyen (livrés) |
|---|---|---|
| **loi scriptée** (graines 1..12) | **10/12** | 31,4 s |
| **appris v10, 64 cachés** (graines 1..12, embarqué) | **7/12** | **30,0 s** |
| appris v8, 64 cachés (graines 1..12, d'alors) | 7/12 | 49,2 s |
| appris v9 (seuils de conduite, repli de capacité) | 1/12 | 17,6 s |
| appris v8 d'avant (24 cachés, 6 000 pas, graines 1..6) | 0/6 | - |

> ⚠ **Ces lignes décrivent les artefacts d'alors**, dont le **tir n'était pas
> appris** (format de cibles v8, §5 nonies quinquies) : la ligne « appris v8,
> 64 cachés » bouclait par accident. L'artefact embarqué aujourd'hui est un
> **v10** (cibles corrigées, DAgger depuis le quai) à **7/12** et **30,0 s** -
> même nombre de livraisons, mais un tir **appris**, sans trappe. Ce qui reste
> vrai et verrouillé : le **portage** Rust du réseau est **fidèle à 100 %**
> (1 423 pas de vraie partie, `validate_learned_port.py`) et la **plomberie**
> d'entraînement ne dépend pas de la valeur de l'artefact.

avec l'artefact élargi (240 000 pas étiquetés, 24 000 en entraînement,
64 cachés, 300 époques, moteur numpy) à **78,9 %** d'exactitude d'entraînement
et **69,8 %** de validation (65,0 % / 57,9 % à 24 cachés et 25 époques).
Sur les graines 1..6, le réseau livre **5/6** contre **6/6** pour la loi
scriptée : il reste **derrière**, mais **il livre** - l'écart n'est plus
« ne boucle pas du tout » mais « boucle plus lentement ». Le micro-simulateur
du **même** artefact donne **0/6** (écart −574,7) : hors ligne, il **désigne
le mauvais réseau** (détail et ablation en §5 nonies bis).

### Le protocole de mesure en boucle fermée **dans le jeu** (rejouable)

La mesure « dans le jeu » a son **script du dépôt** :
`tools/trainer/measure_in_game.py`. Il fait jouer le **jeu lui-même** - la
seule mesure qui ne dépende ni du micro-simulateur ni d'un portage - et rend la
comparaison reproductible :

```bash
cargo build --release && cargo run --release -- --headless   # 1. le jeu
cd tools/trainer                                              # 2. la mesure
python3 measure_in_game.py --seeds 1 2 3 4 5 6                # appris vs scripté
python3 measure_in_game.py --seeds 1 2 --only learned --sim-cap 60 --json /tmp/m.json
```

**Protocole** - une graine = **deux épisodes identiques**, un par cerveau :

1. `POST /reset` sur la graine, puis `POST /cmd {"autopilot": true}` avec
   `learned_pilot` **faux** (loi scriptée) ou **vrai** (réseau embarqué) ;
2. attente de la **nouvelle piste** : l'`episode_id` de l'observation doit
   changer (l'observation d'avant la remise à zéro appartient à l'épisode
   précédent - une livraison y serait comptée deux fois) **et** le cerveau
   demandé doit être appliqué (le champ `learned` est **neutre** tant que la
   bascule n'est pas consommée : comparer ces pas d'amorçage ferait échouer la
   mesure pour rien, le même piège que `validate_learned_port.py`) ;
3. suivi des frames jusqu'au **dénouement explicite** publié par le jeu
   (`episode_done`, cf. `src/driver.rs::advance_episode`) : le script ne
   réinterprète pas la condition de succès, elle est celle du jeu ;
4. deux **garde-fous** pour un épisode qui ne se termine pas (le cas mesuré) :
   `--sim-cap` (150 s de temps **simulé**, dénouement de mesure `délai`) et
   `--wall-cap` (120 s muraux, `mur` - protège d'un jeu qui ne publie plus de
   frame). Le temps rapporté est le temps simulé (`episode_t`), comparable
   d'une machine à l'autre.

La **logique de mesure est testée hors partie** (`test_measure_in_game.py` :
classification des dénouements, garde-fous, attente de la bascule, contre un
faux client) et le **vocabulaire** des dénouements est verrouillé contre
`src/driver.rs` - le rapport ne peut pas inventer un état que le jeu ne publie
pas. Rejouée avec l'artefact **déployé** (v10, §5 nonies quinquies), la mesure
donne **7/12** livraisons pour le cerveau appris contre **10/12** pour la loi
scriptée (graines 1..12) et **3/6** contre **6/6** sur les graines 1..6 : c'est
cette ligne, et non l'écart de récompense du micro-simulateur, qui fait foi.

Entraînement de l'artefact déployé (`ship_warmstart.py --dagger-iterations 3
--dagger-starts dock --dagger-stride 4` : 12 graines × 4 distances, `--stride 4`,
**147 917 pas étiquetés** + 13 291 états DAgger, **64 cachés**, 300 époques,
moteur **numpy**, **features v10**) : exactitude **train 98,4 %**, **validation
91,0 %** (la validation baisse à mesure que le DAgger ajoute des états hors
distribution - c'est sa définition, pas une régression : la **boucle fermée**,
elle, passe de 1/6 à **6/6**). Autrement dit : **le mécanisme est livré et
fidèle, et la politique livre dans la partie** (7/12) **au rythme de la loi**
(30,0 s contre 31,4 s). Le résidu n'est plus ni la trappe ni le tir : c'est la
**frontière hybride du simulateur** (collisions) et l'**affinage** (§5 nonies
quater) - la stratégie apprise reste un objet mesurable **dans le jeu**, pas
seulement hors ligne.

### Grandeurs de décision du vaisseau (v7) - fait

Les trois entrées manquantes identifiées ici sont **livrées** en version 7 :
**balles en vol** (`bullets`, avec les deux indicateurs de **retenue de feu**),
**`supplies_affordable`** et **`moving_mode`** en one-hot (§5 nonies). Mesure
faite : elles font passer l'écart de récompense de −582,5 à **−566,2** mais ne
bouclent toujours pas la boucle (0/6 livraisons) - **nécessaires, pas
suffisantes**.

### Visée de mission de la conduite (v8) - fait

La cause résiduelle identifiée était mesurée : la **conduite** de l'autopilote
vaisseau vise la **cible de sa mission** (minerai, hostile ou station selon
soute, réserves et garde de la station), alors que les features n'exposaient
d'erreur d'alignement que vers la **station** (sur 113 706 pas d'expert, le sens
de rotation n'y était expliqué qu'à **50,3 %**, le hasard).

La version 8 livre la visée complète (§5 nonies) : mission en one-hot, **cap
effectif** (esquive comprise) et son erreur d'alignement, **vitesse visée**,
vitesse projetée, composantes **4 WAYS**, drapeaux d'esquive / survitesse /
arrêt. Résultat : la rotation de l'expert est **exactement** reproduite par la
règle rejouée depuis le cap v8 (**100,0 %** des pas, contre 63,3 % avec la
seule erreur vers la station), l'imitation passe de **31,6 %** à **57,9 %** de
validation, et le micro-simulateur de 0/6 à **1/6** livraisons - sans encore
fermer la boucle **dans le jeu** (0/6 contre 6/6 pour la loi scriptée).

### Piste identifiée pour l'affinage - dont la seconde moitié est faite

Sur l'artefact **d'avant l'élargissement**, le résidu était mesuré et n'était
plus un manque d'information : dans la partie, le réseau n'était juste qu'à
**45,2 %** dans la **bande morte** d'alignement (`|err| ≤ 0,10 rad`), contre
84,6 % au-delà et 100 % au-delà de 0,35 rad - et cette bande couvrait **68 %**
de la trajectoire (le nez reste pointé, l'autopilote ne corrige que par
rafales). Deux suites étaient proposées : exposer les **seuils de conduite**
(drapeaux « aligné pour tourner », « aligné pour pousser », « vitesse visée
atteinte ») - symétrique de l'esprit `hold_fire_*` de la v7 - et/ou élargir la
**capacité d'apprentissage** (cachés, époques, données).

**La seconde a été suivie et elle ferme la boucle** (§5 nonies bis) :
la bande morte était un **symptôme de capacité**, pas un mur. La première
reste ouverte - elle viserait la **précision** (livrer plus vite, réduire les
5 échecs sur 12), non la fermeture. Le vrai chantier devenu prioritaire est le
**micro-simulateur représentatif** : hors ligne, il désigne le **mauvais**
réseau.

## 6. Suite (phases suivantes)

- **Phase 2 — épisodes à objectifs DAG.** ✅ Livrée (§5 quinquies) : les
  épisodes se jouent sur les scénarios à objectifs de l'éditeur (missions
  chaînées, progression exposée dans l'observation, récompense par
  complétion et terminaison `objectives_complete`). Le mode headless
  accélère le protocole existant et exécute les lots d'épisodes en continu
  dans le processus (§5 ter, centaines d'épisodes/s pour la tâche EVA).
- **Phase 3 — vrais apprenants.** L'imitation hors-ligne (§5 ter) donne un
  premier réseau de neurones qui transfère en boucle fermée sur la tâche EVA.
  L'infrastructure **DAgger** est en place (§5 quater : étiquette experte
  dans l'observation, `dagger.py`, tête de rotation softmax) et améliore la
  prédiction de l'expert sur les états visités par la politique (~12 % →
  ~98 % de validation en 3 itérations) ; `dagger.py` accepte maintenant
  **plus de départs et de graines** (`--seeds`, `--spawn-dists` élargi,
  `--spawn-angle-jitter` / `--spawn-dist-jitter` pour les départs nez
  désaligné) et **mesure l'écart en boucle fermée contre l'autopilote porté**
  sans lancer le jeu (`--measure-sim`). L'infrastructure **RL** est livrée
  (§5 sexies : PPO sur l'observation complète, rejeu - le DQN a été essayé
  puis écarté, l'horizon long tuant le bootstrap) et le **gel de la boucle
  fermée est levé** : l'amorce par départs perturbés + features de décision
  EVA fait que les rollouts atteignent enfin la station (16-17/20 contre 0/20
  auparavant, §5 sexies). L'écart restant est l'**affinage** (les mises à jour
  PPO dégradent encore l'amorce - la sauvegarde du meilleur point protège le
  résultat),  puis l'évaluation contre l'autopilote (943,8) et la boucle de
  minage du vaisseau - dont la **référence et le micro-simulateur hybride sont
  livrés** (§5 septies : portage vaisseau validé à 100 % contre la loi du jeu,
  cinématique du simulateur validée à la précision machine contre une vraie
  partie) :  l'**amorce par départs perturbés de la cible `ship`** est en place
  et se mesure hors ligne (`ship_warmstart.py --measure-sim`). Après
  l'**élargissement de la capacité** (64 cachés, 300 époques, 24 000 pas -
  moteur numpy, §5 nonies bis), le réseau ne sous-apprend plus (validation
  **69,8 %**) et la boucle **se ferme dans le jeu** (**7/12** livraisons contre
  **10/12** pour la loi scriptée) : le levier était la capacité, pas la donnée
  (ablation : 1/12 à 24 cachés sur les mêmes données). Cette politique est
  ensuite **déployée dans le jeu** (Phase 4, §5 octies).
- **Phase 4 — politique apprise dans le jeu.** ✅ Livrée (§5 octies) : le
  réseau de la boucle de minage est **embarqué dans le binaire**
  (`assets/ship_pilot_policy.json`) et **rejoué par le jeu**
  (`src/learned_pilot.rs` : features `nn.py` v8 + perceptron, mêmes ordres
  d'opérations), sélectionnable par la case **LEARNED PILOT** (touche Y) et
  exposé à l'entraîneur (`POST /cmd {"learned_pilot":true}`, champs
  `learned_pilot` / `learned` de l'observation). Le portage est mesuré
  **fidèle à 100 %** (4 941 pas comparés avec l'artefact v8 élargi, 64 cachés,
  sur trois épisodes de minage),
  verrouillé par un fixture de vraie partie (`test_learned_pilot.py` +
  test unitaire Rust) et le banc d'essai n'hérite plus du cerveau appris.
  Ce qui reste ouvert n'est donc plus la plomberie mais **la politique
  elle-même** : après élargissement de la capacité (§5 nonies bis), elle livre
  **7/12** dans le jeu contre **10/12** pour la loi scriptée (et **5/6** contre
  **6/6** sur les graines 1..6). La mesure « dans le jeu » n'est plus une
  manipulation ponctuelle : c'est un **protocole rejouable du dépôt**
  (`tools/trainer/measure_in_game.py`, §5 octies) - deux épisodes identiques
  par graine, arrêt au dénouement explicite du jeu, garde-fous simulé et
  mural - testé hors partie par `test_measure_in_game.py`. La **visée de la
  mission** de la conduite est livrée en features **v8** (§5 nonies) : la
  rotation devient décidable (100 % contre 63,3 % avec la seule erreur vers la
  station) et l'imitation passe à 57,9 % de validation.  **Élargir la capacité**
  (64 cachés, 300 époques, 24 000 pas, moteur numpy) porte la validation à
  **69,8 %** et ferme la boucle dans le jeu (7/12) ; la piste restante est
  d'exposer les **seuils de conduite** (bande morte d'alignement) pour gagner
  en précision, et de rendre le **micro-simulateur représentatif** (hors ligne,
  il désignait le mauvais réseau) - **fait** en §5 nonies quater : le champ
  minier de la graine est **importé du jeu**, la séquence de l'épisode
  (rétraction des liens, animation d'accostage) est portée, et le classement
  hors ligne suit celui de la partie (verrouillé par
  `test_ship_env.py::TestRepresentativity`). **Ce qui reste** : l'artefact
  embarqué, ré-entraîné depuis le 16 septembre, ne livre plus aucune graine
  (0/12 contre 10/12 pour la loi scriptée) - il doit être **ré-entraîné sur le
  simulateur représentatif** puis **mesuré dans le jeu** (`measure_in_game.py`),
  sans se fier à l'exactitude d'imitation. Restent enfin le durcissement du
  protocole (version explicite des features) et la reproductibilité des
  artefacts.

## 7. Conventions et reproductibilité

- L'épisode est déterministe à la graine : même `seed` → mêmes formes,
  mêmes positions de départ ; l'orientation du cosmonaute éjecté est
  réinitialisée (0 = est) par `activate_cosmonaut`.
- Le micro-simulateur `eva_env.py` reproduit **les formules du jeu**
  (`thrust_vector`, `moving_shape`, `PLAYER_*`, `STATION_DOCK_DISTANCE`) ;
  l'observation simulée a le même format JSON que `/obs` - les politiques et
  l'entraîneur sont interchangeables entre simulation et partie réelle.
- L'entraînement du réseau a **deux moteurs** (`nn.py::MLP.train(backend=...)`) :
  `python` (**défaut**, aucune dépendance - la convention du répertoire) et
  `numpy` (vectorisé, ~33 × plus rapide, **optionnel**). Ils font le même
  apprentissage ; seul l'entraînement peut passer par numpy, jamais
  l'inférence rejouée par le jeu ni le portage Rust, et les poids sont
toujours sauvegardés en **listes Python** (`save_nn`, JSON).
  `test_nn_backend.py` verrouille les deux (les cas numpy se sautent sans
  numpy, la CI n'installe rien).
- La **vraie partie** fait foi : la simulation est un outil de développement
  (60 Hz, pas de collisions ni d'aliens autour de l'EVA, qui est un
  non-collider). Ce qui doit être mesuré **dans le jeu** a son protocole
  rejouable : `tools/trainer/measure_in_game.py` (cible `ship`, scénario
  `economy` par défaut) fait jouer les deux cerveaux sur les mêmes graines et
  rapporte dénouement, temps simulé et pas - `--json` pour un rapport machine.
  Voir §5 octies pour le protocole et ses deux pièges (piste d'épisode non
  encore ouverte, bascule de cerveau non encore consommée).
