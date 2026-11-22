# Plateau d'apprentissage (21-22/11) : diagnostic et réglages

Constat prod (11 h → 18 h le 21/11) : Elo 1559 → 1567 (record 1575), KL 0,0004, lr 7,8e-5, entropie 0,48 sous la cible 0,54,
`ent_coef` 0,0012, aucune nouvelle méca en match, buts 0,25-0,3/match. Drills : keeper 37 % à difficulté 0,25, rings 38 % à 0,2
(figés), `aerial_touch` / `aerial_high` 0 % au niveau 0. Aucun changement d'observation ni d'actions : le checkpoint de prod
se reprend tel quel (pas de release `rst`). Pas de démonstration (`--demo-lambda` reste à 0).

## Diagnostic

1. **Mise à jour PPO trop petite.** lr = 3e-4/(1 + 1,423e9/5e8) = 7,8e-5 (plancher 5e-5 vers 2,5e9 pas). Une itération =
   16 envs × 128 = 2 048 décisions, minibatch 512 : 4 minibatchs × 4 époques = 16 pas Adam. La KL mesurée (4e-4, moyenne qui
   inclut le premier minibatch à 0) donne un écart de ratio ≈ √(2·KL) ≈ 3-4 %, loin du clip de 20 % : le clip et `KL_MAX` (0,02)
   ne servent jamais, seul lr × nb de pas limite l'apprentissage. Une PPO saine vise 3e-3 à 1e-2.
2. **Régulateur d'entropie.** À H = 0,48 constant, `ent_coef` ne monterait que de +0,18 %/itération (gain haut 0,03 ×
   écart 0,06), alors que H, mesurée sur une seule itération (~1 400 décisions), est bruitée et que les gains sont asymétriques
   (0,1 à la baisse) : l'équilibre est sous la cible et le coefficient dérive vers le bas (0,0075 → 0,0012 en 24 h). Hypothèse
   (non prouvée) ; le comportement observé en essai court est cohérent (voir plus bas).
3. **Promotion des drills.** `UP_RATE` = 0,5 > 37-38 % : keeper et rings ne peuvent plus monter (écart-type d'une fenêtre de
   100 épisodes ≈ 4,8 %).
4. **Aérien niveau 0 à 0 %.** Le script « saut + boost nez en l'air » réussit déjà 19/40 (test existant) : le drill est faisable,
   mais rien ne guide la politique : la réussite ne paie rien (`on_events`, contrairement au gardien), pas de prime de toucher
   au palier 6, la prime `AerialTouch` exige une balle partant vers le but adverse (jamais payée), l'épisode dure ~27 s même
   une fois la balle au sol, et `aerial_high` ne met pas la vitesse de la voiture à l'échelle du niveau (jusqu'à 1 200 uu/s au
   niveau 0 : 800 uu trop loin à l'arrivée de la balle). Le boost est déjà plein.

## Réglages ajoutés (défauts = comportement d'avant)

Flag `--x` ou variable d'env ; le flag prime.

| Réglage | Défaut | Essai |
|---|---|---|
| `LR_MIN` (`--lr-min`) | 5e-5 | 2e-4 |
| `LR0`, `PPO_EPOCHS`, `PPO_CLIP`, `KL_MAX` | 3e-4, 4, 0,2, 0,02 | inchangés |
| `PPO_MINIBATCH` (base 15 Hz) | 512 | 256 |
| `ENT_MIN` | 1e-4 | 0,004 |
| `ENT_GAIN_UP` / `ENT_GAIN_DOWN` | 0,03 / 0,1 | 0,05 / 0,05 |
| `ENT_EMA_ITERS` (0 = H brut) | 0 | 50 |
| `DRILL_UP_RATE` / `DRILL_DOWN_RATE` / `DRILL_MIN_EPISODES` | 0,5 / 0,15 / 100 | 0,30 / 0,10 / 200 |
| `AERIAL_SUCCESS_BONUS` | 0 | 1,0 (payé une fois, `aerial_touch`/`aerial_high`) |
| `AERIAL_END_ON_SUCCESS` | 0 | 1 (fin d'épisode à la réussite, 5 s max) |
| `AERIAL_SHAPING` | 0 | 0,5 (potentiel −‖voiture−balle‖/3000, en l'air ou balle > 200 uu) |
| `AERIAL_EASY` | 0 | 1 (niveau 0 : balle plus basse, plus proche, montée 100 uu/s ; `aerial_high` : vitesse voiture mise à l'échelle) |

Nouvelles métriques : `trainer_clip_fraction`, `trainer_epochs_run`, `trainer_kl_last_epoch`, `trainer_entropy_smoothed`.
Reprise : `explore.json` reste lisible (champs absents = défauts) ; un `ent_coef` de 0,0012 remonte à `ENT_MIN` au chargement.
Tests : le script scripté réussit 40/40 (`aerial_touch` et `aerial_high` niveau 0, tick_skip 8 et 4) contre 0/40 pour une action
nulle ; la montée initiale de balle à 300 uu/s de la spec ne donnait que 14/40, d'où 100 uu/s.

## Essais courts (témoin vs réglages)

Deux learners en parallèle depuis une copie du checkpoint de prod (21/11 18 h, palier 6), 20 min, 2 threads chacun,
~2 000 pas/s (≈ 2,4 M pas, soit 0,6 % d'une journée de prod). Même binaire, mêmes réglages de prod hors réglages testés.

| Mesure (relevés 5 / 10 / 15 / 19 min) | Témoin | Réglages |
|---|---|---|
| KL | 1,1e-3 / 1,2e-4 / 2,5e-3 / 1,0e-3 | 1,5e-2 / 1,1e-2 / 1,2e-2 / 1,2e-3 |
| clip_fraction | 0,01 / 0,003 / 0,009 / 0,003 | 0,024 / 0,073 / 0,031 / 0,021 |
| époques faites (sur 4) | 4 / 4 / 4 / 4 | 3 / 4 / 3 / 4 |
| entropie (lissée pour réglages) | 0,48 / 0,54 / 0,49 / 0,59 | 0,50 / 0,56 / 0,52 / 0,51 |
| ent_coef | 0,006 / 0,008 / 0,005 / 0,009 | 0,014 / 0,017 / 0,020 / 0,020 |
| taux de but | 0,29 / 0,28 / 0,39 / 0,32 | 0,23 / 0,40 / 0,28 / 0,13 |
| garde-fou (back-offs) | 0 | 0 |

- Drills (cumul 19 min) : keeper 74/408 (18 %) vs 65/388 (17 %), rings 36/230 vs 37/207, `aerial_touch` 0/132 vs 0/123,
  `aerial_high` 0/156 vs 0/148. Niveaux inchangés (keeper 0,30, rings 0,25, aériens 0) dans les deux : 20 min ne suffisent pas
  (< 4 fenêtres de 200 épisodes).
- Elo des évaluations (1 par run à 7-8 s et 13-14 s) : témoin 1576 puis 1557, réglages 1594 puis 1480 : bruit, rien à conclure.

**Lecture honnête.** Le seul effet net est sur l'optimisation : la KL passe de ~1e-3 (moyenne, avec des creux à 1e-4) à
~1e-2 (époques parfois coupées par `KL_MAX`), le clip devient actif, l'entropie lissée reste dans la cible, sans back-off.
Le témoin montre aussi que `ent_coef` ne reste pas à 0,0012 dans cet essai (0,005-0,009) : le coefficient oscille, donc
l'hypothèse « dérive vers le bas » reste à confirmer sur la durée. Le coefficient des réglages colle au plafond 0,02 : à
surveiller. Les aériens restent à 0 % dans les deux essais : trop court pour que la politique découvre le saut, même si le
niveau 0 est atteignable (script 40/40). Le taux de but (petits effectifs, 13 % au dernier relevé) n'est pas concluant.

## Args à mettre dans le déploiement (sans le modifier)

Le CMD de l'image n'a pas d'args : ajouter des variables d'env au conteneur `learner` de
`apps/rocketsim/deployment.yaml` (flux-gitops), après avoir déployé une image contenant cette PR :

```
LR_MIN=2e-4
PPO_MINIBATCH=256
ENT_MIN=0.004
ENT_GAIN_UP=0.05
ENT_GAIN_DOWN=0.05
ENT_EMA_ITERS=50
DRILL_UP_RATE=0.30
DRILL_DOWN_RATE=0.10
DRILL_MIN_EPISODES=200
AERIAL_SUCCESS_BONUS=1
AERIAL_END_ON_SUCCESS=1
AERIAL_SHAPING=0.5
AERIAL_EASY=1
```

Variante prudente (si la KL vers 1e-2 inquiète) : `LR_MIN=1e-4 PPO_MINIBATCH=512` (non essayée). Retour arrière : retirer les
variables ; `explore.json` et `drill_levels.json` restent compatibles. À surveiller après déploiement : `trainer_kl` (3e-3 à
1e-2), `trainer_clip_fraction` (< 0,2), `trainer_entropy` (≈ 0,5), `trainer_entropy_coef` (pas collé à 0,02),
`trainer_entropy_guard_backoffs_total`, `trainer_drill_difficulty` (keeper/rings montent, aériens quittent 0), goal rate.
