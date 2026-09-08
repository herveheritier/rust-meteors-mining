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
├── policies.py   ← politiques : idle / random / seek (contrôleur paramétré entraîné)
├── evaluate.py   ← lignes de base : mesure une stratégie sur des épisodes
├── bench.py      ← banc d'essai **en continu** dans le processus headless (POST /bench)
├── cem.py        ← entraînement par croix-entropie (CEM) de la politique `seek`
└── policy.json   ← politique entraînée (sortie de cem.py, rejouable)
```

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

### 3. Entraîner une politique (CEM)

```bash
python3 cem.py                       # simulateur : départ naïf → découvre une politique
python3 cem.py --init expert         # départ près du réglage robuste de `policies.py`
python3 cem.py --backend live        # contre la vraie partie (épisodes en temps réel - lents)
```

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
- La **Phase 2** (voir `docs/AUTOENTRAINEMENT.md` §5 bis, §5 ter et §6)
  enrichira encore les épisodes côté jeu (missions des objectifs DAG comme
  langage de tâche/récompense), puis viendront des apprenants plus puissants
  (réseau de neurones, RL) qui remplaceront la politique `seek` paramétrée.
