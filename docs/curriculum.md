# Curriculum

Le learner suit sept paliers (`EnvConfig::for_stage`, `crates/rl-env` ; `crates/learner/src/curriculum.rs`). Le palier
courant est la métrique `trainer_curriculum_stage` et le fichier `<out>/stage` (persisté avec les checkpoints).

| Palier | Objectif | Fin d'épisode |
| --- | --- | --- |
| 0 | aller vers la balle (vitesse vers la balle 0,3 + 3 par toucher) | premier toucher, ou 150 décisions |
| 1 | toucher la balle (vitesse 0,1 + 1 par toucher) | premier toucher, ou 150 décisions |
| 2 | marquer, balle au repos, cage vide | but (l'un ou l'autre), ou 300 décisions |
| 3 | balle en mouvement : vitesse horizontale initiale aléatoire (0,2 à 1500 uu/s), rebonds | idem |
| 4 | positions difficiles (1/3 près des murs ou du fond, 1/3 en l'air basse, 1/3 loin du but adverse), balle jusqu'à 1000 uu/s | but, ou 400 décisions |
| 5 | adversaire statique (voiture orange immobile devant sa cage), départs mélangés (50 % aléatoires, 50 % positions difficiles), balle jusqu'à 1000 uu/s | idem |
| 6 | self-play : l'adversaire est une copie gelée de la politique, remise à jour toutes les 50 itérations | idem |

Récompenses dès le palier 2 : +10 pour un but marqué (+Y), **−5 pour un but contre son camp** (balle dans le but −Y,
y compris un but encaissé en self-play), 0,1 par toucher, 0,05 × vitesse vers la balle.

## Promotion et régression

- Paliers 0 et 1 : taux de toucher moyen ≥ 90 % sur les 10 dernières itérations.
- Paliers 2 à 5 : **taux de but** (buts / épisodes, cumulés sur les 10 dernières itérations) ≥ 80 %.
- Régression d'un palier (3 et plus, jamais sous 2) si le taux de but glissant tombe sous 10 % après une période de
  grâce passée au palier : 30 itérations aux paliers 3 et 4, **300 aux paliers avec adversaire (5 et 6)**. Une
  itération = 2048 pas (16 envs × 128) ≈ 5 épisodes de 400 pas : 30 itérations ne suffisent pas à apprendre à
  contourner un adversaire.
- Après une régression, pas de nouvelle promotion avant **200 itérations** (évite l'oscillation promu/rétrogradé).
- La fenêtre est vidée à chaque changement de palier ; un checkpoint est écrit immédiatement.
- `--stage N` fige le palier (ni promotion ni régression).

Métriques : `trainer_goals_total{side="scored"|"conceded"}` (conceded = but contre son camp / encaissé),
`trainer_goal_rate` (taux de but glissant du curriculum). Le log par itération a les colonnes `goal_rate goals own_goals`.

## Observation et compatibilité des checkpoints

L'observation passe de 33 à 43 valeurs : les 33 premières sont inchangées, 10 valeurs s'ajoutent (position, vitesse,
cap de la voiture adverse, indicateur de présence), **nulles sans adversaire** (paliers 0 à 4). Au chargement,
`Trainer::load_checkpoint` essaie le format courant ; en cas d'échec il lit le format historique (entrée de 33), agrandit
la couche d'entrée de l'acteur et du critique avec des poids **nuls** pour les nouvelles entrées : le réseau se comporte
exactement comme avant (testé : `crates/learner/tests/legacy_checkpoint.rs`), puis apprend à s'en servir aux paliers 5
et 6. Le palier courant est conservé (fichier `stage`) et la reprise est automatique (`--resume`).
Les moments d'Adam ne sont pas sauvegardés (inchangé).
Le `policy.safetensors` exporté a désormais 43 entrées : un consommateur de l'ancien export (33) doit être mis à jour.

Les voitures sont identifiées par leur id (`Sim::snapshot` les trie par id) : jusqu'à la correction de novembre 2026,
`snapshot().cars[0]` était la voiture **orange** dès qu'il y avait deux voitures (ordre d'un `unordered_set` RocketSim),
si bien qu'aux paliers 5 et 6 l'agent observait l'adversaire immobile à la place de sa propre voiture et que la
récompense d'approche était calculée sur l'adversaire (taux de but ≈ 0, oscillation 4 ↔ 5). Les poids d'entrée du bloc
adversaire d'un tel checkpoint ont appris à y lire la voiture de l'agent : à la reprise d'un dossier sans fichier
`opponent-obs-v2`, `Trainer::reset_opponent_inputs` les remet à zéro (une fois ; le fichier est écrit à chaque
checkpoint). Le reste du réseau (paliers 0 à 4) est conservé.

Les moments où l'env est reconfiguré (changement de palier) remettent tous les envs à zéro.

**Non fait** : drills de mécaniques issus de `scenarios` (aérien, flick, wall shot…), gardien scripté mobile,
métrique `trainer_episode_outcome_total` et temps moyen jusqu'au but.
