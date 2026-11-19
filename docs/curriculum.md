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
| 6 | self-play : adversaire = copie gelée (70 % des envs) ou ancien du pool (30 %), récompense à somme nulle ; maintien jugé sur l'Elo (`docs/self-play.md`) | idem |

Récompenses dès le palier 2 : +10 pour un but marqué (+Y), **−5 pour un but contre son camp** (balle dans le but −Y,
y compris un but encaissé en self-play), 0,1 par toucher, 0,05 × vitesse vers la balle. Termes optionnels de vitesse de balle (désactivés par défaut) : `docs/rewards.md`.

## Coups d'envoi (paliers 5 et 6)

Un épisode sur 40 % (`KICKOFF_PROB`, tirée à chaque reset) est un **coup d'envoi** : une des 5 positions officielles
tirée au hasard (±50 uu, ±0,05 rad), l'adversaire en miroir, balle au centre, 33 de boost. L'adversaire est le
**fonceur scripté** (cap vers la balle, gaz et boost, départ retardé de 0 à 4 décisions) au palier 5 ; au palier 6 il
est la copie gelée de la politique, sauf 30 % des kickoffs contre le fonceur (sinon « personne ne va à la balle »
devient une stratégie). 10 % des kickoffs partent avec de l'élan (1200–1600 uu/s) : coup de pouce pour découvrir le flip
avant l'impact. Fin : 2 s (30 décisions) après le premier toucher de l'une des voitures, 6 s (90 décisions) au plus, ou
but. Ces épisodes sont **exclus** de `episodes`, `touches`, `goals`, `own_goals` : ils ne pèsent pas sur la promotion.

Récompenses (en plus de 0,05 × approche et 0,1 par toucher), versées **une seule fois** et seulement si l'agent
touche le premier (égalité au tick : personne) :
- rapidité `clamp(1 + (t_ref − t) / 0,5 s, 0, 2)`, `t_ref` = temps du fonceur seul (2,15 s diagonales, 2,65 s ailleurs,
  mesuré par `measure_charger_times`) ;
- vitesse au contact `0,5 × clamp((v − 1200) / 1000, 0, 1)` (vitesse à la décision d'avant, balle partie vers +Y) ;
- direction `clamp(vy_balle / 2000, −1, 1)` ; supersonique (≥ 2200 uu/s) avant le toucher : +0,5 ;
- issue : +3 gagné (but, ou balle à y > 300 après un toucher de l'agent), −3 perdu (but encaissé, y < −300, ou pas
  de toucher en 6 s), 0 sinon ; −1 si l'adversaire touche le premier.
Total maximal ≈ 7 hors but, sous les +10 d'un but. Pas de récompense dense de vitesse (tourner en rond ne paie pas).

Évaluation reproductible : `learner --eval-kickoff 40 --policy policy.safetensors [--idle]` (politique gloutonne, 40
kickoffs par position, mêmes graines ; `--idle` = adversaire immobile). `--kickoff-prob P` surcharge la part de kickoffs
à l'entraînement (0 = témoin).

Mesures locales (copie du checkpoint de prod, `--stage 5`, 2 threads, 10 min ≈ 2,2 M pas, `--eval-kickoff 40`, adversaire
fonceur) : avant 34 % de kickoffs gagnés / 44,5 % de premiers touchers / 2,58 s / 1278 uu/s au contact. Après 10 min avec
kickoffs : 45 % / 57,5 % / 2,53 s / 1687 uu/s ; témoin 10 min sans kickoff (`--kickoff-prob 0`) : 41,5 % / 52,5 % / 2,56 s
/ 1536 uu/s. L'écart avec le témoin (+3,5 points) est dans le bruit (±7 points) : 10 minutes ne prouvent rien, l'effet se
juge sur plusieurs heures en prod (`trainer_kickoffs_total`). Contre un adversaire immobile : 100 % dans tous les cas.

### Réalisme : flip avant et speed flip

La table de 78 actions contient déjà les flips (diagonale pitch −1 + roll ±1 + saut : indices 20/21 et 28/29 ; flip avant
24/25 ; saut seul 44/45 ; annulation 60/61, 68/69). Un flip avant = 3 décisions de suite (saut, relâcher, flip) : à la
portée du PPO. Un **speed flip** exige ≈ 5 décisions précises (saut, relâcher, diagonale, annulation vers 67 ms, roll
pour retomber) à 15 Hz : en PPO pur sans démonstration, c'est très improbable en dix minutes et peu probable même en
plusieurs jours. Ce qui est réaliste : un flip avant ou diagonal pour finir l'approche (les bonus de rapidité, de vitesse
au contact et de supersonique le récompensent directement, avec 33 de boost le flip est le moyen évident de dépasser
2200 uu/s). Coups de pouce déjà en place : élan initial sur 10 % des kickoffs, bonus supersonique. Piste suivante, non
faite : clonage de comportement sur un speed flip scripté, puis PPO (ne change pas le format du checkpoint). Essayé et
écarté : un coût du temps de 0,02 par décision avant le premier toucher (taux de kickoffs gagnés plus bas que sans, sur
un seul essai de 10 min : bruit probable, mais aucun gain mesuré).

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

Depuis l'observation v3 (89 valeurs, 90 actions), voir `docs/observation.md` : l'export 43/78 reste lisible par les clients.

Les voitures sont identifiées par leur id (`Sim::snapshot` les trie par id) : jusqu'à la correction de novembre 2026,
`snapshot().cars[0]` était la voiture **orange** dès qu'il y avait deux voitures (ordre d'un `unordered_set` RocketSim),
si bien qu'aux paliers 5 et 6 l'agent observait l'adversaire immobile à la place de sa propre voiture et que la
récompense d'approche était calculée sur l'adversaire (taux de but ≈ 0, oscillation 4 ↔ 5). Les poids d'entrée du bloc
adversaire d'un tel checkpoint ont appris à y lire la voiture de l'agent : à la reprise d'un dossier sans fichier
`opponent-obs-v2`, `Trainer::reset_opponent_inputs` les remet à zéro (une fois ; le fichier est écrit à chaque
checkpoint). Le reste du réseau (paliers 0 à 4) est conservé.

Les moments où l'env est reconfiguré (changement de palier) remettent tous les envs à zéro.

**Non fait** (drills de mécaniques : fait, voir `docs/drills.md`) : gardien scripté mobile,
métrique `trainer_episode_outcome_total` et temps moyen jusqu'au but.
