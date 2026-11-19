# Observation v3 et table d'actions v3 (boost, action précédente, flip, frein à main)

## Observation : 89 valeurs

| Indices | Contenu |
| --- | --- |
| 0–32 | voiture (pos, vit, vit. ang., avant, haut), balle, relatif, `boost/100`, `on_ground`, `has_flip_or_jump` (inchangé) |
| 33–42 | adversaire : pos, vit, avant, présence (nul sans adversaire) |
| 43–76 | **34 pads de boost** : 1 si active, sinon la part du temps de réapparition écoulée (0 = vient d'être prise, → 1 à la réapparition ; 4 s petite, 10 s grosse). Positions fixes, non données. Ordre fixe de l'arène ; vue orange : la pad `j` est celle qui occupe la place symétrique (x, y → −x, −y) |
| 77 | **boost de l'adversaire** / 100 (0 sans adversaire) |
| 78–85 | **action précédente** : accélérateur, direction, tangage, lacet, roulis, saut, boost, frein à main (derniers contrôles appliqués) |
| 86–88 | **flip** : dodge en cours, temps depuis le saut (borné à 1,5 s, ramené à 0–1), roues en contact / 4 |

`has_flip_or_jump` (index 32) couvre déjà « a encore son flip ». `Snapshot` (sim-core) porte désormais `pads` et, par
voiture, `is_flipping`, `air_time_since_jump`, `wheels_contact`, `last_controls` : le client de jeu construit la même
observation que l'entraînement (`rl_env::build_obs`).

## Table d'actions : 90

Les 78 premières sont inchangées ; 78–89 = les 12 actions du sol avec **frein à main** (virages glissés, demi-tours),
dans l'ordre des 12 du sol.

## Compatibilité (chirurgie de poids)

Un réseau de 43 entrées / 78 actions (checkpoint ou `policy.safetensors` de prod) est repris sans perte :
- entrées ajoutées : poids nuls → mêmes logits (acteur et critique) quelles que soient leurs valeurs ;
- sorties ajoutées : poids nuls, biais **−4** (`rl_env::NEW_ACTION_BIAS`) → jamais choisies au départ ;
- `Trainer::load_checkpoint` (formats 89/90, 43/78, 33/78), `Mlp::from_tensors` (`--init-actor`, pool d'anciens, Elo,
  références) agrandissent automatiquement ;
- `rst bot pull`, `rst watch --vs-bot`, `rst play --vs-bot` acceptent encore l'ancien export (43/78 : observation tronquée,
  frein à main jamais choisi) et le nouveau (89/90). Test : `v2_checkpoint_keeps_greedy_actions_and_values` (≥ 99 % d'actions
  gloutonnes identiques sur 1 000 états).
- `policy.json` : `action_table` = `rlgym-90`.

## Récompense de boost (paliers 5 et 6 seulement, hors drills)

`r = c × (√(boost_t/100) − √(boost_{t−1}/100))`, seulement quand le boost **monte** (ramassage), `c = 0,2` (`BOOST_REWARD`).
Une grosse pad à vide vaut 0,2 (un toucher : 1 au palier 5, 0 au palier 6), une petite à vide ≈ 0,07, une petite à
33 de boost ≈ 0,02, une petite à 80 ≈ 0,005. **Aucune pénalité de dépense** : le bot ose booster, et le gain total est
borné par ce qu'il a dépensé. Le but (+10 / −10 au palier 6) domine : on ne délaisse pas une occasion pour une pad.
Somme nulle : conservée pour le but ; la prime de boost ne l'est volontairement pas (l'adversaire est une copie gelée,
non entraînée, et la prime est une aide à l'apprentissage de l'agent, pas un résultat du match).

Boost de départ : tiré dans 0–100 aux épisodes de match des paliers 5/6 (voiture et adversaire) ; coups d'envoi à 33
(comme le vrai jeu) ; drills inchangés (`docs/drills.md`).

Métriques : `docs/metrics.md` (`trainer_boost_*`).

## Essai local (acteur de prod, palier 5, 9 min, 32 envs, drills 30 %)

Acteur `https://rocketsim.celri.dev/policy.safetensors` (palier 6), critique neuf échauffé 60 itérations (politique figée),
même binaire : **avant** = `--boost-reward 0 --fixed-boost` (comportement antérieur), **après** = défauts (prime 0,2, boost
de départ aléatoire). Les deux runs n'ont pas eu la même charge CPU (168 k vs 315 k pas) : comparer des ratios.

| | avant | après |
| --- | --- | --- |
| taux de but (moyenne par itération, hors échauffement : après seulement) | 0,57 (échauffement) | 0,54 (échauffement) / 0,53 (apprentissage) |
| buts marqués / encaissés | 196 / 14 | 330 / 33 |
| ramassages pour 1 000 pas (petits + gros) | 8,8 | 8,4 |
| boost moyen · part du temps à 0 | 5,0 · 83 % | 5,6 · 80 % |
| Elo de départ (palier 5) | 1414 | 1412 |

Lecture : pas de régression du taux de but ; en 9 min les ramassages ne changent pas encore nettement (la politique est
figée un tiers du temps et le bot reste à 0 de boost ~80 % du temps : marge de progrès, à suivre via `trainer_boost_*` en prod).
