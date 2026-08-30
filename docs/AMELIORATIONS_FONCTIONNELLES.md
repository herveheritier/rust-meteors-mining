# Améliorations fonctionnelles proposées

> **CONSIGNE DE MAINTENANCE** : chaque amélioration ci-dessous doit être
> **retirée de cette liste** dès qu'elle est intégrée dans le jeu (code
> mainline, pas juste un prototype). Un item ne doit pas coexister avec sa
> réalisation. Si une amélioration est abandonnée, la marquer explicitement
> (~~barré~~) avec la raison plutôt que de la supprimer, pour garder une
> trace des choix.

---

## Multijoueur & Social

- [ ] **Mode coopératif local (2 joueurs)** — Deux vaisseaux sur un même clavier (J1 : flèches + shift, J2 : WASD + Q). Le mode télécommande HTTP permet déjà un deuxième joueur via téléphone. Météores partagés.
- [ ] **Défi asynchrone (replay / ghost)** — Enregistrer la trajectoire du vaisseau (position + orientation par frame) dans un fichier. Afficher le « ghost » du meilleur run au lancement. Le bouton T sur GAME OVER pourrait proposer « Watch ghost ».

## Architecture & Extensibilité

- [ ] **Scénario custom : édition en vol** — Modifier les objectifs DAG et les règles en cours de partie via un écran d'éditeur intégré (touche +). L'architecture DAG (`scenario_objectives.rs`) s'y prête naturellement.
