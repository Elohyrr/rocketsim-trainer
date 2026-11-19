# Récompenses de vitesse de balle (gros tirs, power shots, pinchs)

Demande d'Elohyrr : pousser le bot vers les frappes puissantes sans l'inciter à pousser la balle en fond de terrain
ni à l'envoyer n'importe où. En match (palier ≥ 2) la récompense était : +10 but, −5 contre son camp (−10 au palier 6),
0,1 par toucher (0 au palier 6), 0,05 × vitesse vers la balle, prime de boost. Aucun terme de vitesse de balle.

## Les deux termes (`EnvConfig`, hors drills, désactivés à 0)

`u` = direction unitaire balle → centre du but adverse (0, 5120, 321) ; `V = 6000 uu/s` (vitesse max de la balle).

| Terme | Formule | Flag / env |
|---|---|---|
| dense (`ball_goal_reward`, façon RLGym `VelocityBallToGoal`) | par décision : `w × clamp(v_balle·u / V, −1, 1)` | `--ball-goal-reward W` / `BALL_GOAL_REWARD` |
| frappe (`hit_speed_reward`) | au toucher : `w × clamp((v_après − v_avant)·u / V, −1, 1)` | `--hit-speed-reward W` / `HIT_SPEED_REWARD` |

Coefficient 0 = désactivé (défaut du code : rien ne change tant qu'on ne les active pas). Les flags ne s'appliquent
qu'aux paliers qui marquent (but > 0).

## Pourquoi ces choix

- **Frappe (terme principal)** : récompense l'acte (la balle gagne de la vitesse vers le but à cause du toucher), pas
  le roulis passif. Il est signé et se télescope : des touchers successifs qui s'annulent ne rapportent rien, donc
  pas de farm par petits coups ; un dégagement vers notre but coûte, un arrêt d'une balle qui arrivait vers nous
  rapporte. Un tir à 3000 uu/s depuis l'arrêt vaut `w × 0,5`.
- **Dense (terme d'appoint, petit)** : guide la phase « la balle part vers le but » après le toucher (passes,
  rebonds, pinch qui ricoche). Seul, il paierait la balle qui roule ; on le garde ≥ 10× plus petit.
- Pas de pousse-fond-de-terrain : la composante n'est que la vitesse *vers le centre du but* (une balle collée au
  mur latéral ou lente ne paie presque rien), et un boulet en direction de notre but est négatif.

## Somme nulle en self-play

Les deux termes dépendent de la même balle et du même but visé, avec signe : ce que gagne l'agent quand la balle file
vers le but adverse, l'adversaire (miroir, but en −Y) le perd. Cohérent avec but +10 / −10 du palier 6. (L'adversaire
est une copie gelée : il n'apprend pas pendant l'itération, donc la symétrie sert de garde-fou de conception, pas de
mécanique.) Le terme n'a donc pas de seuil positif seulement, contrairement à un « bonus de tir ».

## Valeurs conseillées

`hit_speed_reward = 0,5`, `ball_goal_reward = 0,02`. Ordres de grandeur : un power shot à 4000 uu/s ≈ +0,33 ; une
possession de 3 s à 3000 uu/s vers le but ≈ +0,45 au plus ; un but vaut +10 : les termes restent sous 10 % d'un but
sur un épisode normal. Un `ball_goal_reward` plus fort risque de payer la poussée sans tir.

## Métriques

`trainer_hit_speed_to_goal_mean` (vitesse de balle vers le but adverse juste après un toucher de l'agent, dernière
itération) et `trainer_hit_speed_to_goal_record` (record). Mesurées en match même coefficients à 0 (base de comparaison).

## Essai local (5 min par bras, acteur de prod, `--stage 6 --init-actor --critic-warmup 20`, 2 vCPU)

| Bras | pas | buts / contre son camp (self-play) | part de buts | vitesse vers le but au toucher (moy. / record) |
|---|---|---|---|---|
| témoin (coefficients 0) | 800 k | 306 / 362 | 45,8 % | 1234 / 2962 uu/s |
| `--hit-speed-reward 0.5 --ball-goal-reward 0.02` | 548 k | 183 / 199 | 47,9 % | 916 / 2926 uu/s |

Lecture : pas de régression du taux de but (la part de buts reste ≈ 46–48 %, bruit large avec ~380–670 événements ;
en self-play la part visée est ~50 %). **Aucune hausse de la vitesse de balle au toucher en 5 min** (la moyenne
est même plus basse, dans le bruit : 548 k pas contre 800 k, un seul essai par bras). Un effet sur les gros tirs
demande des heures d'entraînement ; ce test ne prouve que l'absence de casse. À surveiller en prod :
`trainer_hit_speed_to_goal_mean` / `_record`, et le taux de but.
