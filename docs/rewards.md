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
