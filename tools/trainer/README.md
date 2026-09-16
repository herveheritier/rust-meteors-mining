# Auto-entraînement du pilote — entraîneur indépendant (`tools/trainer`)

Système **indépendant de l'application** qui pilote le cosmonaute (le
vaisseau **ou** le cosmonaute EVA) en utilisant le jeu comme environnement, à
travers l'**interface de contrôle** HTTP de `src/driver.rs` (voir
`docs/AUTOENTRAINEMENT.md` pour la démarche complète).

Python **standard uniquement** (aucune dépendance - `urllib`, `random`,
`math`). Seule exception, **optionnelle** : `numpy`, utilisé par le second
moteur d'entraînement du réseau (`MLP.train(backend="numpy")`) pour les
réseaux larges et les gros jeux de données - le défaut reste `backend="python"`,
sans dépendance, et l'inférence n'en dépend jamais.

```
tools/trainer/
├── client.py        ← client du protocole (GET /obs, POST /cmd, POST /reset, POST/GET /bench)
├── eva_env.py       ← géométrie partagée + micro-simulateur de l'EVA (mêmes lois que le jeu)
├── autopilot_ref.py ← **portage Python de l'autopilote EVA du jeu** (référence hors-ligne)
├── ship_autopilot_ref.py ← **portage Python de l'autopilote VAISSEAU** (boucle de minage)
├── ship_env.py      ← **micro-simulateur vaisseau hybride** (boucle de minage, cible ship)
├── ship_warmstart.py ← **amorce par départs perturbés** de la cible ship (+ mesure boucle fermée)
├── policies.py      ← politiques : idle / random / seek / nn / ppo / autopilot_sim
├── nn.py            ← petit MLP + features d'observation (imitation) ; moteurs `python`/`numpy`
├── warmstart.py     ← **amorce experte par perturbation des états de départ** (imitation)
├── imitate.py       ← entraînement hors-ligne par **imitation de l'autopilote** sur les trajectoires
├── dagger.py        ← **DAgger** : déploie la politique, étiquette ses états avec l'expert, ré-entraîne
├── evaluate.py      ← lignes de base : mesure une stratégie sur des épisodes (+ --reference)
├── bench.py         ← banc d'essai **en continu** dans le processus headless (POST /bench)
├── cem.py           ← entraînement par croix-entropie (CEM) de la politique `seek`
├── test_reward_gap.py ← **test de non-régression** de l'écart politique/autopilote (EVA)
├── test_ship_port.py ← tests du portage de l'autopilote **vaisseau** (observations synthétiques)
├── test_ship_env.py ← tests du **micro-simulateur vaisseau** + amorce perturbée (ship)
├── test_learned_pilot.py ← **non-régression** de la politique **embarquée dans le jeu** + fixture
├── test_nn_backend.py ← **non-régression** des deux moteurs d'entraînement du réseau (python/numpy)
├── measure_in_game.py ← **protocole de mesure en boucle fermée dans le jeu** (appris vs scripté)
├── test_measure_in_game.py ← tests du **protocole de mesure** (dénouements, garde-fous, bascule)
├── validate_ship_port.py ← mesure la **fidélité du port vaisseau** contre la vraie partie
├── validate_learned_port.py ← mesure la **fidélité du portage du réseau appris** (champ `learned`) + fixture
├── fixtures/        ← fenêtres d'une **vraie partie** rejouées par les tests (fidélité physique et du réseau)
└── policy.json      ← politique entraînée (sortie de cem.py, rejouable)
```

La politique **déployée dans le jeu** n'est pas ici : elle est **embarquée** dans
le binaire (`assets/ship_pilot_policy.json`, `include_str!` de
`src/learned_pilot.rs`) - `ship_warmstart.py` l'écrit là où on le lui demande :

```bash
cd tools/trainer
python3 ship_warmstart.py --output ../../assets/ship_pilot_policy.json --measure-sim
```

Depuis la **Phase 2**, les épisodes peuvent aussi se jouer sur un **scénario
à objectifs DAG** de l'éditeur (`scenarios/*.scenario.json`) : `--scenario
campaign_prospector` (dans `bench.py`, `evaluate.py`, `dagger.py`, ou
`"scenario":"<id>"` du protocole `/reset` / `/bench`). Les **missions** du
scénario (chaîne de prérequis) deviennent la tâche de l'épisode :
`objectives` dans l'observation (mission courante + progression `current` /
`required`), chaque complétion rapporte `OBJECTIVE_BONUS` (200,
`eva_env.py`), et l'épisode se termine quand **tous les objectifs** sont
complétés (dénouement `objectives_complete`, +1000) - la livraison de soute
n'est plus qu'une étape de la boucle. Le réseau (`nn.py` v8) reçoit la
progression des objectifs en features.

## La tâche d'entraînement (milestone 1)

Chaque **épisode** remet le monde à zéro (`POST /reset` : graine
déterministe), fait « exploser » le vaisseau à une distance donnée de la
station et donne le contrôle au **cosmonaute EVA** éjecté. Le pilote
entraîné ne reçoit que l'**observation JSON** (`GET /obs` : cinématique du
pilote, distance/deltas toriques vers la station, objets proches…) et
renvoie des **actions** (`POST /cmd` : `up/right/left` - mêmes primitives
que les touches). L'épisode réussit quand le cosmonaute entre dans le cercle
d'accostage au centre de la station (récupération).

Le **micro-simulateur** (`eva_env.py`) reproduit les formules exactes du jeu
(poussée vectorielle le long de l'orientation, rotation ←/→, monde torique,
récupération sous 15 unités) à 60 Hz - la cadence de l'interface réelle -
pour itérer en quelques millisecondes par épisode. La **vraie partie**
(`--backend live`, `cargo run` lancé) reste la référence : la politique
entraînée en simulation se rejoue à l'identique contre le jeu.

## Usage

### 0. Mode headless (optionnel, accéléré - sans fenêtre)

Le jeu peut aussi servir l'interface **sans fenêtre ni rendu** : la vraie
physique tourne à **pas fixe** dans un mode headless
(`cargo run --release -- --headless`, port 8643 par défaut), qui accélère
l'entraînement - chaque
`POST /cmd` fait avancer d'un pas quand le pilote externe est engagé
(pas-à-pas à pleine vitesse), et l'autopilote de référence file à cadence
bornée (480 pas/s, lisible par `GET /obs`). Le protocole est inchangé : les
commandes `--backend live` ci-dessous fonctionnent telles quelles, mais les
épisodes s'exécutent en quelques dixièmes de seconde au lieu du temps réel
(voir `docs/AUTOENTRAINEMENT.md` §5 bis).

```bash
cargo run --release -- --headless --port 8643
# puis, dans un autre terminal :
python3 evaluate.py --backend live --strategy autopilot --episodes 3
```

### 0 bis. Banc d'essai en continu (au-delà du pas-à-pas)

Pour **mesurer la ligne de base à pleine vitesse**, le processus headless
peut aussi exécuter des lots d'épisodes **en continu dans le processus** -
aucun aller-retour HTTP par pas : `POST /bench` pose le lot, `GET /bench`
sert le rapport (déroulé par épisode avec **récompense** - les mêmes règles
que l'entraîneur, calculées côté jeu - + cadence en épisodes/s).

```bash
python3 bench.py --episodes 200 --target eva        # tâche EVA
python3 bench.py --episodes 20 --target ship --scenario economy  # boucle de minage
python3 bench.py --episodes 20 --target eva --trajectories  # + trajectoires RL
```

Avec `--trajectories`, le processus écrit un fichier **JSONL** (une entrée
par ligne : bornes d'épisode, pas avec observation + action de l'autopilote,
dénouement avec récompense) pour un entraînement RL **hors-ligne** sur les
décisions de la ligne de base - chemin exposé dans le rapport, chargeable par
`client.load_trajectories(path)`.

### 0 ter. Mode hybride : politique externe vs autopilote (mêmes épisodes)

`evaluate.py --backend hybrid` compare une **politique externe** (seek, random,
idle) à l'**autopilote du jeu** sur des épisodes **identiques** : l'autopilote
joue le lot en continu dans le processus (bench, centaines d'épisodes/s),
puis la politique externe rejoue les mêmes épisodes pas à pas (HTTP) - même
graine, même cible, même position, même scénario. Rapport épisode par épisode
(dénouement + récompense des deux côtés) et moyennes.

```bash
python3 evaluate.py --backend hybrid --strategy seek --episodes 10 --target eva
```

Mesure réelle (5 épisodes, graines 1..5, départ 300 u à l'est) : autopilote
**5/5** (récompense moyenne 943,8, ~136 épisodes/s au bench) contre `seek`
**4/5** (moyenne 705,0) - la politique paramétrée perd sur la maîtrise de
l'arrivée, l'écart que la CEM vise à combler.

Mesures réelles (release) : **~140-230 épisodes/s** pour la tâche EVA
(~5,6 s simulées par épisode, secours systématique), **~4-11 épisodes/s**
pour la boucle complète de minage du vaisseau en économie (~30-50 s simulées
par épisode, livraison systématique sur les graines testées). Le même lot est
lancé au démarrage du processus par `cargo run --release -- --headless --bench N`
(voir `docs/AUTOENTRAINEMENT.md` §5 ter).

### 1. Lancer le jeu (interface sur `http://127.0.0.1:8643/`)

```bash
cargo run
```

(Un message en jeu annonce l'URL de l'interface au lancement.)

### 2. Mesurer les lignes de base (simulateur, instantané)

```bash
cd tools/trainer
python3 evaluate.py --strategy idle    --episodes 3
python3 evaluate.py --strategy random  --episodes 3
python3 evaluate.py --strategy seek    --episodes 3     # réglage robuste de départ
python3 evaluate.py --strategy seek --policy policy.json --episodes 3   # politique entraînée
```

Résultat typique (départ à 300 unités, délai 60 s) :

| stratégie                | succès | temps moyen | récompense |
|--------------------------|--------|-------------|------------|
| `idle`                   | 0/3    | –           | −200       |
| `random`                 | 0/3    | –           | −247       |
| `seek` (défauts)         | 3/3    | 7,4 s       | 900        |
| `seek` entraîné (CEM)    | 3/3    | 11,6 s      | 977        |

Récompense : +1000 récupéré − 2 s/épisode − pénalité d'arrivée trop rapide
(le retour doit rester maîtrisé, comme le fait l'autopilote du jeu).

Contre la **vraie partie** (le jeu fournit aussi la stratégie `autopilot`,
la référence absolue - l'ordinateur du jeu pilote lui-même) :

```bash
python3 evaluate.py --backend live --strategy autopilot --episodes 3
python3 evaluate.py --backend live --strategy seek --policy policy.json --episodes 3
```

**Validé en conditions réelles** (jeu lancé sur X11, `cargo run`) : la
politique entraînée en simulateur **se transfère telle quelle dans le jeu** -
à 300 unités du centre, elle ramène le cosmonaute EVA à la station en
~11,6 s (vitesse d'entrée ~26 u/s). Les essais réels ont aussi révélé que
l'**autopilote du jeu échouait** depuis cette distance (sans amortissement,
il se mettait en orbite autour de la base et s'éloignait en accélérant) -
comportement que le micro-simulateur avait prédit exactement. La loi EVA de
l'autopilote a depuis été **corrigée** (bande d'alignement adaptative +
frein tangentiel à hystérésis, `src/autopilot.rs`) et re-validée en live :
retour réussi depuis 300 / 800 / 1500 unités, vitesse de pointe ≤ 25 u/s,
plus aucune orbite. La politique `seek` reste utile comme ligne de base
paramétrée et comme point de départ de l'entraînement par renforcement.

### 3 bis. Imiter l'autopilote hors-ligne sur les trajectoires du bench (réseau)

Le banc d'essai peut **enregistrer ses déroulés** (`--trajectories` : une
ligne JSONL par pas - observation + action de l'autopilote) ; `imitate.py`
entraîne **hors-ligne** un petit **réseau de neurones** (Python standard
uniquement, `nn.py`) à **reproduire ces décisions** (clonage comportemental),
puis écrit une politique rejouable.

```bash
python3 bench.py --episodes 30 --target eva --trajectories          # 1. trajectoires
python3 imitate.py --trajectories /tmp/meteors_mining_headless_*/trajectories_*.jsonl  # 2. entraîner
python3 evaluate.py --backend hybrid --strategy nn --policy nn_policy.json --target eva --episodes 8  # 3. comparer
```

Le réseau reçoit en entrée les **grandeurs que l'autopilote calcule pour
décider** (visée vers la station, erreur d'alignement, vitesses radiale et
tangentielle, objets proches, économie) - fournir ces features dérivées est
ce qui rend le clonage **stable en boucle fermée** (sans elles, l'imitation
parfaite hors-ligne dérive et échoue en conditions réelles).

Mesures réelles (tâche EVA, départ 300 u à l'est, graines 1..8) : imitation
hors-ligne à **~98 % d'exactitude**, et en **boucle fermée contre le jeu** la
politique `nn` ramène le cosmonaute **7-8/8** (récompense moyenne ~820-840,
mais plus lente que l'autopilote : ~27 s contre 5,6 s - le réseau hésite
plus). Limites connues du clonage pur, à documenter avant la suite : la
politique ne généralise pas aux **départs hors distribution** (le simulateur
qui tire un angle aléatoire échoue), et la **boucle de minage complète** du
vaisseau (décisions discrètes séquentielles, états internes) ne se clone pas
(~0/6) - la suite (DAgger / RL) doit combler ces écarts.

### 3 ter. DAgger — déployer la politique et l'étiqueter avec l'expert

Le clonage pur (§3 bis) apprend sur les seules trajectoires de l'autopilote :
en boucle fermée, chaque erreur de la politique la déplace hors de la
distribution apprise (mesuré : ~98 % d'exactitude hors-ligne mais **0/8** sur
les départs **hors distribution** - angle d'éjection aléatoire). **DAgger**
fait jouer la politique elle-même et étiquette chaque état qu'elle visite
avec l'action que prendrait l'autopilote sur cet état - le champ **`expert`**
de l'observation (`/obs`, calculé côté jeu par `src/driver.rs`), avec le
drapeau d'hystérésis `eva_tang_braking` pour ne pas porter d'étiquettes
contradictoires. Les (état, action experte) sont agrégés, le MLP est
ré-entraîné, et on itère.

```bash
python3 bench.py --episodes 12 --target eva --trajectories          # 1. amorce (autopilote)
python3 imitate.py --trajectories /tmp/.../trajectories_*.jsonl     # 2. clone de départ
python3 dagger.py --init-trajectories /tmp/.../trajectories_*.jsonl \
    --init-policy nn_policy.json --iterations 3 --episodes 3 \
    --spawn-dists 300,500 --target eva                              # 3. DAgger
python3 evaluate.py --backend hybrid --strategy nn --policy dagger_policy.json  # 4. vs autopilote
```

Le réseau `nn.py` a depuis une **tête de rotation softmax** ({gauche,
droite, rien} mutuellement exclusifs - l'expert n'appuie jamais les deux
ensemble ; deux sigmoïdes indépendantes laissaient la politique coincée à
les enfoncer toutes les deux) ; `up`/`down`/`fire` restent des sigmoïdes
indépendantes.

Mesures réelles (release, headless, graines 101..109, départs 300/500 u) :
la politique apprend à prédire l'expert sur **ses propres états**
(exactitude de validation par graines de déploiement ~12 % → ~98 % en 3
itérations) et corrige l'action initiale sur les départs hors distribution ;
le **bouclage complet reste ouvert** (en boucle fermée, elle tourne vers la
station mais ne tient pas encore le rythme poussée/freinage de l'expert) -
à poursuivre par davantage d'itérations DAgger puis RL (§6 d'`AUTOENTRAINEMENT.md`).

Sur **épisodes à objectifs** (cible vaisseau, `--scenario campaign_prospector`),
le premier lancement a révélé et corrigé deux bugs : (1) `dagger.py` attendait
`station_dist ≥ 15` avant de piloter - impossible pour un vaisseau qui démarre
**à quai** (la garde ne s'applique plus qu'aux épisodes EVA, sinon le
lancement tourne sur ~2 000 connexions HTTP/s sans progresser) ; (2) un banc
d'essai lancé après un épisode rejoué pas à pas était piloté par le **pilote
externe resté engagé** (boutons encore enfoncés) au lieu de l'autopilote -
`run_bench` dégage désormais le pilote externe au départ (`clear_driver`).
Mesures (mêmes épisodes, graines 1..3) : autopilote 68,1 (2/5 objectifs) ;
clone −170,0 (0/3) ; politique DAgger −30,2 (0/3 mais +249,4 sur l'épisode 1 :
elle décolle, mine, rapporte et accoste - 2 objectifs - avant d'être détruite) :
progression nette sur le clone, boucle complète encore ouverte.

**Plus de départs, plus de graines, mesure hors-ligne.** DAgger gagne des
leviers pour viser le régime hors distribution, et une mesure qui ne dépend
pas du jeu :

```bash
python3 dagger.py --iterations 6 --episodes 10 --target eva \
    --seeds 1 2 3 4 5 6 7 8 101 102 103 104 \
    --spawn-dists 200,300,500,800,1200 \
    --spawn-angle-jitter 60 --spawn-dist-jitter 120 \
    --measure-sim --init-policy nn_policy.json
```

- `--seeds` : liste explicite de graines (cyclées) - plus de départs
  hors distribution vus par la politique ;
- `--spawn-dists` : cycle de distances élargi (200 → 1200 u) ;
- `--spawn-angle-jitter` / `--spawn-dist-jitter` : **départs perturbés**
  (l'épisode démarre orientation 0 : décaler le cap revient à démarrer nez
  désaligné) ;
- `--measure-sim` : après l'entraînement, mesure l'écart de récompense en
  **boucle fermée contre l'autopilote porté** sur des graines hors
  entraînement (`--measure-seeds`), sans processus headless.

### 3. Entraîner une politique (CEM)

```bash
python3 cem.py                       # simulateur : départ naïf → découvre une politique
python3 cem.py --init expert         # départ près du réglage robuste de `policies.py`
python3 cem.py --backend live        # contre la vraie partie (épisodes en temps réel - lents)
```

### 3 quater. Apprentissage par renforcement (PPO)

```bash
python3 ppo.py                       # PPO, amorce experte `autopilot` sur départs perturbés
python3 ppo.py --iters 0             # produire l'amorce seule (policy de rentrée)
python3 ppo.py --expert seek --warmup-perturb 0   # ancienne amorce (seek, départs nominaux)
python3 ppo.py --iters 200           # budget plus long
```

PPO (`ppo.py`) apprend de la **seule observation** (157 features, politique
factorisée poussée × rotation comme l'autopilote, tête de valeur, γ =
0,9995, avantages bornés, objectif clipé). Sortie : `ppo_policy.json`,
rejouable par `evaluate.py --strategy ppo` (backend sim/live/hybride) ; le
**meilleur point** mesuré est conservé, donc un affinage PPO qui dégrade ne
fait pas perdre l'amorce.

Depuis la Phase 3 (suite), l'amorce est une **imitation de l'autopilote du
jeu depuis des états de départ perturbés** (`warmstart.py`, `--expert
autopilot`) : départs au repos nez aligné, cap et distance décalés,
approches trop rapides à freiner, orbites à casser. C'est le correctif du
gel en boucle fermée (§5 sexies d'`AUTOENTRAINEMENT.md`). Deux pièces sont
nécessaires :

- les **features de décision EVA** (`nn.py` v6 : freinage anticipé, cassure
d'orbite, erreur de visée **résultante**) - sans elles, deux états de
cinématique identique portent des actions opposées selon que l'expert freine
ou non, et l'imitation est structurellement impossible (mesuré : la
politique poussait nez désaligné et fuyait) ;
- les **départs perturbés** (l'état qui gelait - « aligné au repos loin de
la station » - n'apparaît qu'une frame dans les trajectoires nominales).

Mesure (simulateur, graines d'évaluation 11..15, 300 u) : l'ancienne amorce
(`seek`, départs nominaux) ne produit **aucun** rollout complet (meilleure
somme de récompenses ≈ 793, boucle fermée gelée) ; l'amorce perturbée produit
**16-17 rollouts sur 20** qui atteignent la station (meilleure ≈ 1015, une
réussite complète) et une politique gloutonne qui réussit selon le tirage
(0-4/5). Le **signal d'apprentissage existe** désormais ; l'affinage PPO
reste ouvert (les mises à jour dégradent encore la politique - la sauvegarde
du meilleur conserve l'amorce), comme documenté §5 sexies.

Note : un DQN a d'abord été essayé (`dqn.py`, retiré) - l'**horizon long**
de la tâche (~1100 pas, γ 0,99 ⇒ le +1000 n'atteint pas les états de
départ) empêche le Q-learning bootstrapé de démarrer ; PPO (on-policy) est
la famille retenue.

### 3 quinquies. Mesurer l'écart à l'autopilote **sans lancer le jeu**

L'autopilote du jeu est **porté en Python** (`autopilot_ref.py`, mêmes
formules et constantes que `src/autopilot.rs`) : on peut donc mesurer une
politique contre la référence dans le micro-simulateur, en quelques
millisecondes par épisode, sans processus headless.

```bash
python3 evaluate.py --strategy seek --policy policy.json --episodes 5 --reference
python3 evaluate.py --strategy autopilot_sim --episodes 5   # la référence elle-même
```

Le portage est **vérifié par test** : il reproduit la mesure documentée
(~941 en simulateur contre 943,8 documentés, 6/6 récupérations).

#### 3 quinquies bis. Référence **vaisseau** (boucle de minage)

`ship_autopilot_ref.py` porte la **loi vaisseau** de l'autopilote du jeu
(`src/autopilot.rs::autopilot_inputs`) : mission de la frame (soute pleine →
accoster, hostile menaçant la station, ravitaillement, minerai à collecter,
hostile à détruire, stationnement), tir avec **retenue de feu**, et conduite
(cap sur la cible, vitesse visée selon la distance, esquive d'hostiles).

Le portage vaisseau lit plus d'état que l'EVA ; l'observation du jeu a donc
été **étendue** (`src/driver.rs`) avec exactement ce qui manquait :

- **`moving_mode`** (la conduite 4 WAYS diffère des modes « nez ») ;
- le **centre du corps** de chaque objet proche (`center_x`/`center_y`) - la
  visée réelle (`body_center`), un météore asymétrique a son corps décalé de
  `position` ;
- la liste des **balles en vol** (`bullets`, séparée de `nearby` pour ne pas
  changer les slots des features du réseau) - la retenue de feu ne se lit pas
  dans la cinématique ;
- **`supplies_affordable`** (les prix du magasin ne sont pas dans
  l'observation) - l'autopilote rentre se ravitailler quand les réserves sont
  basses **et** payables.

La fidélité se mesure contre la **vraie partie** (le portage doit rendre la
même commande que le champ `expert` du jeu à chaque frame) :

```bash
cargo run --release -- --headless        # dans un autre terminal
python3 validate_ship_port.py --seeds 7 21 --scenario economy --seconds 20
```

Mesure de référence : **100 % d'accord** sur un épisode complet de boucle de
minage (graine 7, 4 377 pas comparés, dénouement `delivered`). `test_ship_port.py`
verrouille hors-ligne les décisions clés (cap, 4 WAYS, soute pleine, garde de
la station, retenue de feu, ravitaillement) sur des observations synthétiques.

#### 3 quinquies ter. Micro-simulateur vaisseau et amorce perturbée (cible `ship`)

`ship_env.py` est le **micro-simulateur de la boucle de minage** : tir,
minage (un tir = un triangle ; minerais libérés à la destruction), collecte,
économie, accostage et livraison. C'est un simulateur **hybride** :

- **fidèle** : cinématique du vaisseau (quatre modes), tir, minage, économie,
  accostage - la cinématique est reproduite à la **précision machine** contre
  une **vraie partie** enregistrée (`fixtures/ship_physics_windows.json`,
  90 fenêtres état/action/état-suivant) ; `test_ship_env.py` le verrouille ;
- **approché** : géométrie des météores en **cercles** (collisions
  cercle/cercle au lieu du SAT triangle à triangle), champ minier
  **synthétisé** (mêmes règles, flux aléatoire différent), rayon de collision
  effectif et tolérance de ramassage documentés dans le module.

`ship_warmstart.py` est le pendant vaisseau de `warmstart.py` : **départs
perturbés** (nez désaligné, vitesse initiale, soute entamée, réserves basses -
le jeu, lui, démarre toujours **à quai**), déroulé de l'**expert autopilote**
(`ship_autopilot_ref.py`), étiquetage de chaque pas et rééquilibrage.

```bash
cd tools/trainer
python3 ship_warmstart.py --output ship_warmstart_policy.json --measure-sim
```

`--measure-sim` compare la politique à l'autopilote **sur les mêmes épisodes**
dans le simulateur (écart de récompense, sans processus headless).

**Deux moteurs d'entraînement** (`--backend`) : `python` (défaut, **aucune
dépendance** - la convention du répertoire), `numpy` (vectorisé, exige numpy)
ou `auto` (numpy s'il est importable, repli Python pur sinon). Les deux font
**le même** apprentissage (même perte, même élan, même arrêt précoce) ; le
chemin numpy est **~33 × plus rapide** (mesuré) et c'est lui qui rend
alteignables les réseaux larges et les gros jeux de données que le Python pur
ne tient pas (des heures). Il est **optionnel** : `backend="python"` par défaut,
la CI n'installe rien, et seul l'entraînement - jamais l'inférence rejouée par
le jeu ni le portage Rust - peut passer par numpy. `test_nn_backend.py`
verrouille les deux chemins (les cas numpy se **sautent** sans numpy). Les
réglages par défaut étant dimensionnés pour le moteur vectorisé,
`ship_warmstart.py` **avertit** si le moteur effectif est le Python pur sur un
gros travail (au-delà de ~5 M pas×époques×cachés) au lieu de partir pour des
heures sans le dire.

**Résultat actuel, honnête** - élargissement de la capacité (artefact par
défaut : 12 graines × 4 distances, `--stride 4`, **240 000 pas étiquetés**
(puis 24 000 en entraînement, 60 000 en validation), **64 cachés**, 300 époques,
moteur numpy, **features v8**) : l'imitation relève encore (exactitude train
**78,9 %**, validation **69,8 %**, contre 65,0 % / 57,9 % à 24 cachés et 25
époques) et - c'est le fait marquant - la boucle de minage **se ferme enfin
dans le jeu** : **7/12 livraisons** (10/12 pour la loi scriptée) contre **0/6**
avant, avec **5/6** sur les graines 1..6. Le micro-simulateur, lui, se révèle
**trompeur** : il **pénalise** le réseau élargi (écart −574,7 contre −447,8)
alors que la partie le récompense - c'est exactement pourquoi la mesure qui
fait foi est celle **dans le jeu** (§3 quinquies quater et quinquies).

**La capacité est bien le levier (ablation).** Mêmes données élargies, capacité
d'**avant** (24 cachés, 25 époques) : exactitude validation 60,6 %, micro-
simulateur **−369,1** (1/6) - mais **1/12 livraisons dans le jeu** seulement.
Autrement dit le **gros réseau passe de 1/12 à 7/12** tandis que le petit
« gagne » hors ligne : sur cette cible, le champ minier synthétique du
micro-simulateur n'est **pas** un critère de sélection valide (l'écart de
récompense hors ligne est même **anti-corréléé** au résultat dans la partie).
Le chantier ouvert n'est donc plus « élargir la capacité » mais **rendre le
micro-simulateur représentatif** (ou s'en passer pour sélectionner).

Les **grandeurs de décision du vaisseau** sont exposées aux features : balles
en vol (`bullets`, avec les deux indicateurs de **retenue de feu**),
`supplies_affordable` et le **mode de déplacement** en one-hot (**v7**), puis
la **visée de mission de la conduite** - mission de la frame, cap effectif
(esquive comprise), erreur d'alignement, vitesse visée, composantes du mode
4 WAYS et drapeaux d'arrêt (**v8**, §3 quinquies quater).

#### 3 quinquies quater. La politique **déployée dans le jeu** (Phase 4)

La politique produite ci-dessus n'est plus seulement un fichier de mesure :
elle est **embarquée dans le binaire du jeu** (`assets/ship_pilot_policy.json`,
`include_str!` de `src/learned_pilot.rs`) et **rejouée par le jeu**, comme
**stratégie « autopilote » alternative** :

- **dans le jeu** : case **LEARNED PILOT** de l'écran de paramétrage (touche
  **Y**, clé persistée `learned_pilot`) - elle choisit le **cerveau** de
  l'autopilote (X allume le pilote), l'indicateur du HUD affiche
  « AUTOPILOT (LEARNED) », et les gestes d'accostage restent à la machine à
  états de l'autopilote (c'est ce que faisait l'expert étiqueteur) ;
- **côté entraîneur** : `client.cmd(autopilot=True, learned_pilot=True)` laisse
  le **jeu** jouer le réseau lui-même, et l'observation publie le champ
  **`learned`** (l'action du portage Rust sur cet état) - miroir exact du champ
  `expert` qui a servi à valider le portage de la loi vaisseau.

```bash
cd tools/trainer
# 1. entraîner et déployer le cerveau (écrit dans les assets du jeu)
python3 ship_warmstart.py --output ../../assets/ship_pilot_policy.json --measure-sim
# 2. (re)compiler le jeu : les poids sont embarqués à la compilation
cargo build --release
# 3. mesurer la fidélité du portage contre la politique Python, à chaque pas
cargo run --release -- --headless        # dans un autre terminal
python3 validate_learned_port.py --scenario economy --seeds 1 2 3 4 --seconds 30
# 4. (re)enregistrer le fixture rejoué par le test unitaire Rust
python3 validate_learned_port.py --record fixtures/learned_pilot_windows.jsonl
```

**Fidélité mesurée : 3 379 / 3 379 pas identiques (100 %)** avec l'artefact
**v9** (171 entrées, **64 cachés**), sur trois épisodes de minage - le
portage Rust lit `hidden` dans le fichier de politique et valide les
dimensions, donc **agrandir le réseau ne touche pas le Rust**. Le test unitaire Rust de `src/learned_pilot.rs`
rejoue le même fixture hors ligne (les features à moins de 1e-9, les décisions
**exactement**), et `test_learned_pilot.py` verrouille le maillon Python :
ré-entraîner la politique ou changer les features **casse ce test** tant que le
fixture n'a pas été ré-enregistré. Un **désaccord n'est pas muet** :
`validate_learned_port.py` imprime le pas fautif et les probabilités de la
couche de sortie (c'est ainsi qu'un écart à la toute première frame a été
identifié comme un artefact de mesure - le champ `learned` est neutre tant que
le jeu n'a pas consommé la bascule du cerveau - et non comme une divergence).

**Ce que la stratégie déployée vaut** (mêmes graines, boucle de minage
complète, features v8) :

| artefact | dans le jeu, livraisons | temps simulé moyen (livrés) |
|---|---|---|
| **loi scriptée** (graines 1..12) | **10/12** | 31,4 s |
| **appris v8, 64 cachés** (graines 1..12) | **7/12** | 49,2 s |
| appris **v9** (seuils de conduite, 100 % d'imitation) | **3/12** | 28,8 s |
| appris, 24 cachés + données élargies (ablation) | **1/12** | 17,6 s |
| appris, v8 d'avant (24 cachés, 6 000 pas, graines 1..6) | 0/6 | - |

La ligne **v9** est le résultat le plus instructif du chantier : les **seuils de
conduite** rendent la loi **entièrement décidable** (exactitude d'imitation
**100 %**, contre 78,9 % en v8) et la boucle fermée **régresse** (3/12 contre
7/12). Le facteur limitant n'est donc pas la **représentation** mais l'**écart
de distribution** entraînement ↔ jeu : le réseau, parfait sur le
micro-simulateur, **tire en permanence** dans la partie (1,9 % d'accord sur
`fire`, 100 % sur les autres touches) et vide ses munitions sans miner. Voir
`docs/AUTOENTRAINEMENT.md` §5 nonies ter, qui documente aussi les **deux bugs
réels** trouvés et corrigés en route (centre de forme à `NaN` - `0/0` dans
`compute_shape_center` - et mode de déplacement du simulateur qui ne
correspondait pas à celui de la partie).

Sur les graines 1..6, le réseau élargi livre **5/6** (contre 6/6 pour la loi
scriptée, 14,9 - 50,0 s). Il reste donc **derrière** l'autopilote scripté,
mais il **livre** - l'écart n'est plus « ne boucle pas du tout » mais « boucle
plus lentement ». Le micro-simulateur du même artefact donne **0/6** (écart
−574,7) : hors ligne, il **désigne le mauvais réseau**.

La ligne « dans le jeu » est produite par le **protocole rejouable** du dépôt
(`measure_in_game.py`, §3 quinquies quinquies) : une graine, deux épisodes
identiques, un par cerveau, arrêt au dénouement explicite du jeu.

**Diagnostic de l'écart résiduel (mesuré sur l'artefact d'avant
l'élargissement, 24 cachés / 25 époques).** La visée de mission est bien
dans les features (**v8**, §3 quinquies ter) et elle **rend la rotation
décidable** : sur une vraie partie (graine 1, autopilote scripté, 999 pas),
la règle de rotation **rejouée depuis le cap effectif v8** reproduit
exactement la rotation de l'expert (**100,0 %** des pas, 3 classes) là où la
même règle appliquée à l'erreur vers la **station** n'en explique que
**63,3 %** - l'information manquante est livrée. Le réseau, lui, reste à
**42,9 %** d'exactitude toutes touches dans la partie : son échec se
**concentre dans la bande morte** d'alignement (`|err| ≤ 0,10 rad`) - **45,2 %**
des pas y sont justes, contre **84,6 %** au-delà et **100 %** au-delà de
0,35 rad. Or cette bande couvre **68 %** de la trajectoire (le nez reste
pointé, l'autopilote ne corrige que par rafales). Le réseau **ne sait pas
apprendre le seuil** dans cette zone (0,10 rad ≈ 0,032 en feature normalisée
par π) : l'information est là, la **représentation du seuil** ne l'est pas.
C'est l'analogue vaisseau du diagnostic EVA : **exposer les seuils de conduite**
(drapeaux « aligné pour tourner / pousser », vitesse visée atteinte) et/ou
**élargir la capacité d'apprentissage**, puis ré-entraîner.

**Ce diagnostic est désormais fermé pour l'essentiel.** L'élargissement de la
capacité (64 cachés, 300 époques, 24 000 pas sur 240 000 - moteur numpy) a
fait ce qu'il annonçait : l'imitation monte à 78,9 % / 69,8 % et la boucle
**se ferme dans le jeu** (7/12). La bande morte n'est donc pas un mur mais un
symptôme de **capacité insuffisante** ; le résidu devient « livrer plus
lentement que la loi » (49,2 s contre 31,4 s) et le **micro-simulateur
non représentatif**, qui est le vrai chantier ouvert.

Le **banc d'essai n'hérite pas** du cerveau appris : `headless::run_bench` le
remet sur la loi scriptée (sinon la « ligne de base de l'autopilote » serait en
réalité le réseau).

#### 3 quinquies quinquies. Le protocole de mesure en boucle fermée **dans le jeu**

Les sections précédentes mesurent soit dans le micro-simulateur (`ship_env.py`),
soit la **fidélité** d'un portage (`validate_*.py`). La question « que vaut la
stratégie déployée **dans la vraie partie** ? » a son propre protocole,
rejouable : `measure_in_game.py`.

```bash
cargo build --release && cargo run --release -- --headless   # 1. le jeu
cd tools/trainer                                              # 2. la mesure
python3 measure_in_game.py --seeds 1 2 3 4 5 6                # appris vs scripté
python3 measure_in_game.py --seeds 1 2 --only learned --sim-cap 60 --json /tmp/m.json
```

**Protocole** (une graine = deux épisodes identiques, un par cerveau) :

1. `POST /reset` sur la graine, puis `POST /cmd {"autopilot": true}` avec
   `learned_pilot` **faux** (loi scriptée) ou **vrai** (réseau embarqué) ;
2. on attend que la remise à zéro ouvre une **nouvelle piste** (l'`episode_id`
   doit changer) **et** que le cerveau demandé soit appliqué. Deux pièges sont
   ainsi évités : l'observation d'avant la remise à zéro appartient encore à
   l'épisode précédent (une livraison y serait comptée deux fois), et la
   bascule du cerveau n'est pas instantanée - le champ `learned` est **neutre**
   tant qu'elle n'a pas été consommée (un pas d'amorçage serait comparé pour
   rien, le même piège que `validate_learned_port.py`) ;
3. on suit les frames publiées jusqu'au **dénouement explicite** de l'épisode
   (`episode_done` : `delivered` / `destroyed` / `eva_recovered` /
   `objectives_complete`, cf. `src/driver.rs::advance_episode`). Le script ne
   devine donc rien : la condition de livraison est celle du jeu ;
4. un épisode qui ne se termine pas de lui-même - le cas du réseau mesuré -
   est classé `délai` au **garde-fou de temps simulé** (`--sim-cap`, 150 s), et
   un jeu qui ne publie plus de frame est classé `mur` (`--wall-cap`, 120 s).

Le temps rapporté est le **temps simulé** (`episode_t`), comparable d'une
machine à l'autre ; `--json` écrit le même rapport en machine. La logique de
mesure est testée **hors partie** (`test_measure_in_game.py`, faux client) et le
vocabulaire des dénouements est verrouillé contre `src/driver.rs`.

Mesure rejouée avec l'artefact v8 (cible `ship`, scénario `economy`) :

| graine | scripté | appris |
|---|---|---|
| 1 | `delivered` 22,1 s (1 601 pas) | `délai` 150,0 s (9 091 pas) |
| 2 | `delivered` 39,9 s (2 665 pas) | `délai` 150,0 s (9 092 pas) |

soit la même conclusion que la mesure initiale, désormais **rejouable par le
dépôt** : `scripté 2/2 livrés` (moyenne 31,0 s), `appris 0/2`.

### 3 sexies. Test de non-régression de l'écart de récompense

`test_reward_gap.py` (stdlib `unittest`, aucune dépendance) verrouille
l'écart `politique apprise − autopilote` sur des graines hors entraînement :
il vérifie que la politique ne décroche pas de la référence, que le portage
de l'autopilote reste fidèle, et que l'autopilote domine l'immobilité.

```bash
cd tools/trainer
python3 -m unittest -v test_reward_gap
TRAINER_POLICY=ppo_policy.json python3 -m unittest -v test_reward_gap
```

La politique testée est `policy.json` par défaut (la `seek` CEM committée,
qui fait mieux que l'autopilote : 976,7 contre 940,7, écart **+35,9**) ;
`TRAINER_POLICY` permet de tester un `nn_policy.json` / `ppo_policy.json`.

À chaque génération, des candidats (les 5 paramètres de `seek` : bandes
d'alignement `turn_db`/`thrust_db`, croisière `cruise`, ralentissement
`slow_zone`, hystérésis `band`) sont évalués sur des épisodes déterministes,
et les meilleurs deviennent la moyenne suivante (première génération en
exploration uniforme, élitisme du meilleur candidat). Sortie : `policy.json`
+ courbe d'apprentissage :

```
gén  4   meilleur   -233.5   moyenne élite   -244.6   cumulé   -233.5
gén  5   meilleur    974.1   moyenne élite    247.8   cumulé    974.1
gén  8   meilleur    975.8   moyenne élite    975.3   cumulé    975.8
```

## Ce que l'entraînement apprend

Le réglage critique découvert par l'entraînement est la **bande
d'alignement** (`turn_db`) : le cosmonaute n'a qu'une poussée vectorielle
(↑) et pas de frein ; si le nez n'est pas visé assez juste, la trajectoire
d'approche dérive et l'EVA se met en **orbite** autour de la base au lieu
d'entrer dans le petit cercle d'accostage (15 unités) - c'est l'échec du
départ naïf que la CEM doit surmonter. La politique entraînée (bande très
serrée, croisière plus élevée) dépasse le réglage manuel sur la récompense.

## Limites connues et suite

- Le **simulateur** est une aide au développement : seule la **vraie partie**
  fait foi. Le **mode headless** (cf. §0, `cargo run -- --headless`) lance
  cette même physique sans fenêtre : valider une politique contre le jeu prend
  maintenant des dixièmes de seconde par épisode au lieu de secondes réelles.
  Le pas-à-pas HTTP (`POST /cmd` par pas) reste le canal des politiques
  **externes** (Python) ; l'exécution **en continu dans le processus** (cf.
  §0 bis, `POST /bench` / `bench.py`) mesure la ligne de base de l'autopilote
  du jeu à des centaines d'épisodes par seconde.
- La **Phase 2** (voir `docs/AUTOENTRAINEMENT.md` §5 bis, §5 ter, §5 quater,
  §5 sexies et §6) enrichit les épisodes côté jeu (missions des objectifs DAG
  comme langage de tâche/récompense) ; l'infrastructure **RL** est livrée
  (PPO, §5 sexies) et le **gel de la boucle fermée est levé** (§3 quater :
  amorce par départs perturbés + features de décision EVA - les rollouts
  atteignent la station là où ils ne la voyaient jamais). L'écart restant est
  l'**affinage** : les mises à jour PPO dégradent encore l'amorce (la
  sauvegarde du meilleur point protège le résultat), et la boucle de minage
  du vaisseau reste à apprendre. Le champ **`expert`** de l'observation
  (§3 ter) sert aussi de guide pour ces apprenants, et le portage de
  l'autopilote (`autopilot_ref.py`) permet de mesurer l'écart sans lancer le
  jeu (`evaluate.py --reference`, `test_reward_gap.py`).
- Les **deux lignes de base sont portées** (EVA et vaisseau) : la référence
  vaisseau (`ship_autopilot_ref.py`) est validée à 100 % contre la loi du jeu.
  La boucle de minage a désormais son **micro-simulateur** (`ship_env.py`,
  **hybride** : cinématique/tir/minage/économie/accostage fidèles - la
  cinématique est reproduite à la **précision machine** contre une vraie partie
  (`fixtures/ship_physics_windows.json`, testé par `test_ship_env.py`) - mais
  **géométrie des météores en cercles** et champ synthétisé). Dessus tourne
  l'**amorce par départs perturbés de la cible `ship`** (`ship_warmstart.py`) :
  l'infrastructure et la mesure hors ligne (`--measure-sim`) sont en place et,
  après l'**élargissement de capacité** (64 cachés, 300 époques, 24 000 pas,
  moteur numpy - validation **69,8 %**), la boucle **se ferme dans le jeu**
  (7/12 livraisons). Attention : sur cette cible le micro-simulateur
  **désigne le mauvais réseau** (il préfère le petit modèle, cf. §3 quinquies
  ter) - sa géométrie de météores en cercles et son champ synthétisé sont le
  **chantier ouvert**, plus que l'affinage du réseau.
- La **Phase 4 est livrée** (§3 quinquies quater) : la politique de la boucle
  de minage est **embarquée dans le jeu** (`assets/ship_pilot_policy.json`,
  `src/learned_pilot.rs`, case LEARNED PILOT / touche Y) et son portage est
  mesuré **fidèle à 100 %** contre la politique Python (4 941 pas comparés,
  `validate_learned_port.py` + fixture rejoué par le test unitaire Rust).
  Après élargissement de capacité, le cerveau déployé **livre 7/12** dans le
  jeu (10/12 pour la loi scriptée) - il reste **derrière** mais n'est plus hors
  jeu. La mesure qui fait foi est celle **dans la partie**, par le protocole
  rejouable `measure_in_game.py` (§3 quinquies quinquies), pas le
  micro-simulateur. Les features **v8** exposent la **visée de mission de la
  conduite** (mission, cap effectif, `desired_speed`, 4 WAYS, esquive) : la
  rotation experte devient décidable (100 % contre 63,3 % avec la seule
  erreur vers la station). L'élargissement de capacité (64 cachés, 300 époques)
  porte la validation à **69,8 %** et ferme la boucle **dans le jeu** (7/12) :
  la **bande morte** d'alignement diagnostiquée sur l'ancien artefact était un
  symptôme de capacité, pas un mur (voir §3 quinquies quater).
