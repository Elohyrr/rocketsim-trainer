# Curriculum

Le learner suit trois paliers (`EnvConfig::for_stage`, `crates/rl-env`). Le palier courant est la métrique
`trainer_curriculum_stage` et le fichier `<out>/stage` (persisté avec les checkpoints).

| Palier | Objectif | Récompense | Fin d'épisode |
| --- | --- | --- | --- |
| 0 | aller vers la balle | vitesse vers la balle (poids 1,0) + 0,2 par toucher | premier toucher, ou 150 décisions |
| 1 | toucher la balle | vitesse vers la balle (poids 0,1) + 1,0 par toucher | premier toucher, ou 150 décisions |
| 2 | marquer | + 10 pour un but dans le but adverse (+Y), 0,1 par toucher, vitesse vers la balle (0,05) | but (l'un ou l'autre), ou 300 décisions |

**Promotion** : taux de toucher moyen ≥ 90 % sur les 10 dernières itérations → palier suivant (checkpoint immédiat).
`--stage N` fige le palier (pas de promotion). Au palier 2 la balle part au repos, voiture et balle tirées au hasard.

**Non fait / à documenter plus tard** : drills de mécaniques issus de `scenarios` (aérien, flick, wall shot…), adversaire,
tirs à vitesse initiale non nulle, promotion sur le taux de but au palier 2, régression de palier si le taux chute.
Le critère de promotion du palier 1→2 reste le toucher (il n'existe pas de taux de but avant le palier 2) ; si le
palier 2 stagne, repasser à 1 avec `--stage 1` et envisager D1 (repli Python).
