# Améliorations fonctionnelles proposées

> **CONSIGNE DE MAINTENANCE** : chaque amélioration ci-dessous doit être
> **retirée de cette liste** dès qu'elle est intégrée dans le jeu (code
> mainline, pas juste un prototype). Un item ne doit pas coexister avec sa
> réalisation. Si une amélioration est abandonnée, la marquer explicitement
> (~~barré~~) avec la raison plutôt que de la supprimer, pour garder une
> trace des choix.

---

## Gameplay & nouvelles mécaniques

- [ ] **Dégats de la base** — Les collisions des météores sur la base provoquent des dégâts qui peuvent aller jusqu'à la destruction de triangles qui la compose ; en mode d'affichage meshes afficher sur chaque triangle le niveau de dégâts  
- [ ] **Difficulté adaptative / vagues progressives** — Nombre de météores, vitesse et densité qui augmentent progressivement pendant la session, particulièrement utile en Survival.
- [ ] **Boss / météore spécial** — Gros astéroïde minable apparaissant périodiquement, avec plus de triangles, plus de résistance et un minerai rare (platinum ?). Renforce le système de réputation.
- [ ] **Système de craft simple** — Utiliser les minerais (GOLD, IRON, WATER) pour fabriquer des consommables (bouclier temporaire, boost de vitesse, mines) à la station ou en vol. Extension de l'atelier actuel (fuel/ammo/cargo).
- [ ] **Warp gates / portails** — Portails aléatoires dans le monde torique permettant des sauts courts (20-30 % de la distance monde), mécanique de fuite ou de raccourci stratégique utile en Progression.

## Interface & UX

- [ ] **Minimap interactive dans le HUD** — Zones colorées (station = vert, météores = rouge, minerais = jaune), clustering visuel, highlight de la zone d'accostage quand `docking_guide` est actif.
- [ ] **Tooltips au survol dans le magasin** — Afficher prix, effets et descriptions au survol des boutons du magasin (onglets RAVITAILLEMENT, ÉQUIPEMENT, ATELIER, MODES). Le schéma du vaisseau dans l'onglet ÉQUIPEMENT pourrait montrer les stats de l'arme survolée.
- [ ] **Journal de bord (log scrollable)** — Les 20 derniers événements (tirs, minerais récupérés, accostages, achats) consultables via une touche ou un bouton, en plus du message HUD actuel. Utile en Progression/Survival.
- [ ] **Écran de briefing pré-partie** — Avant le lancement d'un scénario custom, résumer les objectifs DAG, les contraintes (fuel/ammo) et un conseil. Les objectifs sont aujourd'hui visibles uniquement sur l'écran titre et dans le HUD.

## Multijoueur & Social

- [ ] **Mode coopératif local (2 joueurs)** — Deux vaisseaux sur un même clavier (J1 : flèches + shift, J2 : WASD + Q). Le mode télécommande HTTP permet déjà un deuxième joueur via téléphone. Météores partagés.
- [ ] **Défi asynchrone (replay / ghost)** — Enregistrer la trajectoire du vaisseau (position + orientation par frame) dans un fichier. Afficher le « ghost » du meilleur run au lancement. Le bouton T sur GAME OVER pourrait proposer « Watch ghost ».

## Architecture & Extensibilité

- [ ] **Scénario custom : édition en vol** — Modifier les objectifs DAG et les règles en cours de partie via un écran d'éditeur intégré (touche +). L'architecture DAG (`scenario_objectives.rs`) s'y prête naturellement.
- [ ] **Modding par assets externes** — Remplacement de textures/musiques/sons par des fichiers dans un dossier `user_assets/` (détection au démarrage, fallback sur `include_bytes!`). Le binaire actuel est autonome mais empêche la personnalisation.
- [ ] **Statistiques de session détaillées** — Tracker en mémoire : temps de vol, distance parcourue, précision de tir, minerais/triangles détruits, accostages, valeur totale de cargaison déchargée. Afficher un récapitulatif de fin de partie avant l'écran GAME OVER.
