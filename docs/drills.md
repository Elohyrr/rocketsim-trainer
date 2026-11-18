# Drills de mécaniques

Spec : `docs/specs/decouverte-mecaniques.md` §2.2–2.3 (branche `claude/etude-mecas`), PR 7 du plan (§6).
Idée : **guider par l'état de départ avant de guider par la récompense**. Une part des épisodes commence dans une
situation qui montre une mécanique au bot (balle sur le toit, balle haute…) ; le reste reste du match. Le match et le
self-play ne contiennent **aucun** terme de mécanique : le détecteur y compte, sans récompenser.

## Activation

| Réglage | Défaut | Effet |
|---|---|---|
| `--drill-prob P` du learner, ou variable `DRILL_PROB` | `0` (coupé) | part des épisodes en drill, bornée à [0 ; 0,9]. Conseillé : `0.3` |

Sans effet aux paliers 0 et 1 (`end_on_touch`) : un drill y finirait au premier toucher. Chaque épisode tire d'abord le
drill (probabilité `P`), sinon le coup d'envoi (paliers 5/6), sinon l'apparition aléatoire. Observations et actions
sont inchangées : un checkpoint de prod se reprend tel quel (aucune remise à zéro).

## Les 7 drills (`crates/scenarios`)

Fonctions pures `scenarios::sample(drill, unit)` ; `rl-env` applique l'état (`Env::reset_drill`). Tirage uniforme.

**Boost de départ aléatoire** (tous les drills, `scenarios::random_boost`) : 25 % de 0 à 20, 25 % à 100, 50 % uniforme
sur 0–100, pour apprendre les mécaniques avec ET sans boost. Hors drill, `start_boost` reste à 33. Aucune
récompense de boost dans cette PR (la gestion du boost en match, avec les pads dans l'observation, est un autre
chantier) : les bonus de méca ne dépendent pas du boost restant, donc ne poussent pas à en gaspiller, et rien ne
pénalise de tenter une occasion avec peu de boost.

| Drill (`drill=`) | Départ | Mécaniques visées |
|---|---|---|
| `roof_still` | balle sur le toit d'une voiture à l'arrêt, cap vers le but ±0,6 rad | dribble, flick |
| `roof_moving` | idem, voiture à 600–1400 uu/s, balle à la même vitesse | dribble, flick |
| `aerial_touch` | balle à 800–1600 uu de haut, 300–900 devant la voiture au sol, boost 100 | toucher aérien |
| `air_dribble` | balle qui monte (+500–900) et avance, voiture dessous lancée à 600–1100 uu/s, boost 100 | air dribble |
| `wall_ball` | balle qui descend le long d'un mur latéral (x ≈ ±3500–3950), voiture au pied du mur, boost 100 | wall shot, air dribble depuis le mur |
| `flip_reset` | voiture en l'air sur le dos sous la balle ; flip disponible une fois sur deux | flip reset (plus tard) |
| `recovery` | voiture à 300–1500 uu de haut, orientation, vitesse (≤ 1500 uu/s) et vitesse angulaire (≤ 4 rad/s) aléatoires, flip une fois sur deux, balle au sol à > 800 uu | retomber sur les roues, vite, bien orientée |

Pas de drill « plafond » ni de speed flip/wave dash/half flip (pas d'état de départ utile : ils se jouent depuis le
kickoff ou le sol).

## Opposition : aucune adversaire actif

En drill, l'adversaire (paliers 5/6) est **garé immobile** près de sa cage (x ∈ ±1500, y = 4600) et la politique
adverse est ignorée. Raisons : (1) on montre la mécanique, pas la défense : un adversaire actif transforme 30 % des
épisodes en match bruité, avec des conflits de ballon qui masquent le signal ; (2) c'est déterministe pour le
détecteur et le bonus d'utilité (balle vers le but) ; (3) la voiture adverse reste dans la simulation, donc la forme
des observations et le self-play vectorisé ne changent pas. L'adversaire garé est inoffensif ; une balle bien frappée
passe à côté. La confrontation reste dans les 70 % de matchs.

## Bonus de méca (drills uniquement) — `crates/rl-env/src/drill.rs`

Versés par `EpisodeBonus` à partir des `Counts` du détecteur (`mechanics::Detector`, API inchangée : seule la
différence de compteurs sur un pas compte, donc valable avec les règles actuelles comme avec les règles durcies).

- **Faibles** : `b0` par mécanique, plafonné à 10 % du but (`0,1 × goal_reward` = 1,0 pour +10). Flip reset 1,0 ;
  air dribble, ceiling shot 0,8 ; flick 0,6 ; wall shot 0,5 ; aérien, double touch 0,4 ; dribble 0,3 ; toucher simple
  et speed flip / wave dash / half flip : 0.
- **Une fois par occurrence**, **≤ 2 par épisode**.
- **Utiles** : payé si la balle va vers le but adverse à plus de 500 uu/s (au pas de détection) ; sinon l'occurrence
  attend et est payée si un but arrive dans les 3 s (360 ticks). Un dribble sur place ne paie rien.
- **Décroissants** : `b = b0 / √(1 + n/N)`, `N = 1000`, `n` = bonus de cette mécanique payés sur la dernière heure
  (fenêtre glissante, registre `BonusLedger` partagé par tous les envs et les paliers ; remis à zéro au redémarrage).
- Jamais en match ni en self-play : `bonus` n'est calculé que si l'épisode est un drill.

### Bonus de récupération (drill `recovery`)

Suivi dans `drill::RecoveryTrack` (pas dans le détecteur : l'ajouter à `mechanics` créerait 2 séries `mechanic` de
plus ; une métrique dédiée `trainer_drill_recovery_total{outcome}` suffit). Premier contact du châssis avec le terrain
(`world_contact` ou `on_ground`) ; verdict 24 ticks (0,2 s) plus tard ; fin de l'épisode 120 ticks (1 s) après le
contact. Verdicts :

- `good` : au moins 3 roues au sol, **vite** (temps en l'air ≤ 1,3 × la chute libre prévue + 0,25 s), nez à moins de
  60° de la direction utile (horizontale), et ≥ 400 uu/s de vitesse dans cette direction : bonus 0,3 (plafonné à 10 %
  du but, une fois, décroissant `b0/√(1+n/N)` avec son propre compteur horaire) ;
- `wheels` : sur les roues mais une condition manque ; `bad` : toit ou côté ; `none` : jamais retombée.
- Direction utile : vers la balle ; vers son propre but (−Y) si la balle est à plus de 500 uu derrière la voiture
  (plus proche de son but).

## Exclusions

- Les épisodes de drill sont **hors** des stats de curriculum (taux de but, promotion), de l'Elo / du pool du
  self-play, des compteurs d'épisodes et de récompense moyenne (comme les kickoffs).
- Le **garde-fou d'entropie** et l'entropie loguée sont mesurés sur les seules décisions de match ; les drills
  entraînent quand même la politique (perte PPO normale). Si un rollout est entièrement en drill, repli sur toutes
  les décisions.
- Pas de clips pendant un drill (une méca provoquée par l'état de départ n'est pas une « découverte »).

## Métriques

- `trainer_drill_episodes_total{drill}` (7 séries) ; `trainer_drill_recovery_total{outcome}` (good, wheels, bad, none) ; `trainer_drill_bonus_total{mechanic}` (8 séries : celles qui ont
  un `b0 > 0`) ; `trainer_mechanic_total{context="drill"}` (le label existait déjà).
- Les compteurs de match (`context="match"`) ne contiennent pas les mécaniques vues en drill : c'est ce qui permet
  de dire si une méca apprise en drill est **réutilisée spontanément en match**.

## Essai local et mesures

Acteur de prod (`https://rocketsim.celri.dev/policy.safetensors`, palier 5), `--stage 5 --init-actor … --critic-warmup 20`,
sur le nœud partagé (2 vCPU), sans clips.

**Détecteur v2 (strict, défaut) et drills.** Les bonus ne lisent que `Counts` : ils marchent avec v1 et v2 sans
changement (`--detector-v1` garde les règles d'origine). Les règles v2 (dribble ≥ 0,5 s et 600 uu avec balle sur le
toit, aérien avec décollage volontaire et balle > 500 uu) tiennent dans un épisode de drill (400 décisions = 26 s au
palier 5 ; un dribble n'a besoin que de 0,5 s) : la durée n'a pas été changée. Le détecteur est remis à zéro à chaque
drill en gardant sa version.

Essai v2 de 9 min, `--drill-prob 0.3` (≈ 2 490 épisodes de drill, ≈ 355 par drill) : taux de but en match 0,920
(0,928 sans drills avant la v2 : pas d'effondrement ; les drills n'entrent pas dans ce taux).

- mécaniques vues **en drill** (v2) : touch 5 724, flip_reset 8, wall_shot 8, dribble 5 ; en match : touch 11 644,
  wall_shot 35, dribble 4 ;
- bonus payés : flip_reset 4, wall_shot 8, dribble 5 (aerial_touch, air_dribble, flick, double_touch : 0 : l'acteur de
  prod ne décolle pas volontairement vers la balle, ce que la v2 exige ; avec la v1 du même acteur : aerial_touch 200 en
  7,5 min) ;
- récupération : good 27, wheels 159, bad 159 : l'acteur de prod retombe sur le toit ou le côté dans près de la moitié
  des cas, c'est ce que le drill doit lui apprendre.
