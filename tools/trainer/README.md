# Auto-entraînement du pilote — entraîneur indépendant (`tools/trainer`)

Système **indépendant de l'application** qui pilote le cosmonaute (le
vaisseau **ou** le cosmonaute EVA) en utilisant le jeu comme environnement, à
travers l'**interface de contrôle** HTTP de `src/driver.rs` (voir
`docs/AUTOENTRAINEMENT.md` pour la démarche complète).

Python **standard uniquement** (aucune dépendance - `urllib`, `random`,
`math`).

```
tools/trainer/
├── client.py     ← client du protocole (GET /obs, POST /cmd, POST /reset, POST/GET /bench)
├── eva_env.py    ← géométrie partagée + micro-simulateur de l'EVA (mêmes lois que le jeu)
├── policies.py   ← politiques : idle / random / seek (contrôleur paramétré) / nn (réseau)
├── nn.py         ← petit MLP en Python standard + features d'observation (imitation)
├── imitate.py    ← entraînement hors-ligne par **imitation de l'autopilote** sur les trajectoires
├── dagger.py     ← **DAgger** : déploie la politique, étiquette ses états avec l'expert, ré-entraîne
├── evaluate.py   ← lignes de base : mesure une stratégie sur des épisodes
├── bench.py      ← banc d'essai **en continu** dans le processus headless (POST /bench)
├── cem.py        ← entraînement par croix-entropie (CEM) de la politique `seek`
└── policy.json   ← politique entraînée (sortie de cem.py, rejouable)
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
n'est plus qu'une étape de la boucle. Le réseau (`nn.py` v5) reçoit la
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

### 3. Entraîner une politique (CEM)

```bash
python3 cem.py                       # simulateur : départ naïf → découvre une politique
python3 cem.py --init expert         # départ près du réglage robuste de `policies.py`
python3 cem.py --backend live        # contre la vraie partie (épisodes en temps réel - lents)
```

### 3 quater. Apprentissage par renforcement (PPO)

```bash
python3 ppo.py                       # PPO sur l'observation complète, amorce experte `seek`
python3 ppo.py --iters 200           # budget plus long
```

PPO (`ppo.py`) apprend de la **seule observation** (112 features, politique
factorisée poussée × rotation comme l'autopilote, tête de valeur, γ =
0,9995, avantages bornés, objectif clipé). L'amorce est une imitation
supervisée du contrôleur `seek` (équilibrée : ~80 % de ses pas sont « ne
rien faire » - sans duplication des pas rares, l'entraînement finit
immobile). Sortie : `ppo_policy.json`, rejouable par `evaluate.py
--strategy ppo` (backend sim/live/hybride). Mesure actuelle (simulateur) :
l'amorce atteint ~99 % hors-ligne mais la boucle fermée gèle au premier état
« aligné loin de la station » (sous-représenté dans les données de `seek`) -
le +1000 n'atteint jamais les rollouts et PPO reste au plateau ; l'écart
ouvert, identique à celui du clone d'imitation, est documenté dans
`docs/AUTOENTRAINEMENT.md` §5 sexies.

Note : un DQN a d'abord été essayé (`dqn.py`, retiré) - l'**horizon long**
de la tâche (~1100 pas, γ 0,99 ⇒ le +1000 n'atteint pas les états de
départ) empêche le Q-learning bootstrapé de démarrer ; PPO (on-policy) est
la famille retenue.

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
  (PPO, §5 sexies) mais l'écart en boucle fermée reste ouvert : la politique
  apprise gèle à la première bifurcation hors distribution de son amorce -
  la piste la plus courte est une amorce dont la boucle fermée ne gèle pas
  (DAgger, imitation avec états de départ perturbés), puis le remplacement
  de la politique `seek`
  paramétrée. Le champ **`expert`** de l'observation (§3 ter) sert aussi de
  guide de récompense pour ces apprenants.
