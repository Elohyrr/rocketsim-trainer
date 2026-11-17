# Détecteur de mécaniques (`crates/mechanics`)

Détecteur **par tick, sans récompense** (étude du 15/11/2026, §2.1 et §4.1–4.2). Un `Detector` par voiture,
appelé à chaque tick de physique (120 Hz, dans la boucle des 8 ticks d'une décision) avec un `CarTick` et un
`BallTick` ; `reset()` à chaque nouvel épisode ou téléportation (le tick suivant sert seulement à initialiser).
Chaque occurrence reconnue incrémente **une fois** `Counts[mechanic]` (→ `trainer_mechanic_total{mechanic}`).
Pas d'allocation ; ≈ 60 ns/tick mesurés en release (test `bench_ns_par_tick`, `-- --nocapture`).

## Événements (un tick = un masque `Events`)

`GROUND`/`AIR` (changement de `on_ground`), `JUMP`, `DOUBLE_JUMP`, `DODGE` (+ secteur 0–7 via `dodge_sector()`,
0 = avant, 2 = droite, 4 = arrière), `CANCEL` (tangage opposé au dodge ≤ 0,15 s), `TOUCH_GROUND`/`TOUCH_AIR`
(+ `TOUCH_ROOF` si la balle est au-dessus du toit), `TOUCH_WHEELS` (roues qui se posent sur la balle), `RESET`,
`WALL`, `CEILING`, `SUPERSONIC`, `BOOST_ON`/`BOOST_OFF`, `WHEELS` (nombre de roues changé), `BALL_WALL_BOUNCE`.

Notions communes :

- **Toucher** : `ball_hit_tick` change. Deux changements à ≤ 4 ticks d'écart = un seul toucher (contact continu,
  balle posée sur le toit).
- **Balle porteuse** : pas de `world_contact`, voiture à z > 250, centre de la balle à < 200 uu, **côté roues**
  (`(balle − voiture)·up < 0`). `on_ground` porté par la balle n'est pas un retour au sol.
- **Temps en l'air** : compté par le détecteur depuis le dernier contact roues-terrain (un flip reset ne le remet
  pas à zéro) ; `CarTick::air_time` ne sert qu'à l'initialiser après un `reset`.

## Catalogue (seuils : `crates/mechanics/src/thresholds.rs`)

| Mécanique | Signature retenue |
| --- | --- |
| `touch` | chaque nouveau toucher |
| `aerial_touch` | toucher, `!on_ground`, balle z > 300, en l'air ≥ 0,3 s |
| `air_dribble` | 2 touchers aériens, balle z > 500, < 1 s d'écart, boost actif entre les deux, sans retour au sol ; une fois par enchaînement |
| `flip_reset` | `on_ground` passe à vrai avec la balle porteuse, et `has_jumped/has_double_jumped/has_flipped` (dont un au moins était vrai) retombent dans les 3 ticks ; aucun toucher exigé |
| `ceiling_shot` | roues au plafond (`up.z < −0,9`, z > 1 900), puis toucher en l'air (balle z > 300) avec `has_flip_or_jump` dans les 3 s, sans retour au sol ailleurs |
| `wall_shot` | toucher sur le mur (roues au sol, `|up.z| < 0,5`, z > 200) ou ≤ 1 s après l'avoir quitté sans retour au sol |
| `double_touch` | toucher en l'air (balle z > 300), rebond de la balle sur un mur latéral ou du fond, second toucher en l'air ≤ 3 s, sans retour au sol |
| `dribble` | balle dans le repère voiture : `up` ∈ [110, 190], \|avant\| < 110, \|côté\| < 80, vitesse relative < 400, `on_ground`, `up.z > 0,7`, maintenu 60 ticks (trous ≤ 6 ticks tolérés) ; une fois par dribble |
| `flick` | dodge ≤ 0,4 s après un dribble validé, puis toucher et balle > 1 500 uu/s ≤ 0,3 s après le dodge |
| `speed_flip` | dodge diagonal avant (composantes normalisées > 0,3), annulation (tangage opposé ≥ 0,5, ≤ 18 ticks), retour au sol ≤ 0,8 s, vitesse horizontale > 1 900 |
| `wave_dash` | dodge puis retour au sol ≤ 6 ticks, gain de vitesse horizontale > 250 (vs tick avant le dodge) |
| `half_flip` | dodge arrière (avant < −0,7), annulation, roulis ≥ 0,5 après l'annulation, retour au sol ≤ 1,5 s, cap tourné (cos < −0,8) |

Seuils de la spec conservés tels quels ; ajouts du détecteur (non chiffrés dans la spec) : fusion des touchers
≤ 4 ticks, confirmation du reset en 3 ticks, tolérance de 6 ticks dans le dribble, fenêtre wall shot 1 s,
double touch 3 s, half flip 1,5 s et cos < −0,8.

## Conventions d'entrée (câblage)

- `flip_rel_torque` est attendu comme **direction du dodge** : `[0]` avant (+) / arrière (−), `[1]` latéral
  (+ droite). Dans RocketSim, `flipRelTorque` est un couple (`(−dodgeDir.y, dodgeDir.x, 0) / tickTimeScale` avec
  `dodgeDir = (−pitch, yaw + roll)`, vérifié dans `RocketSim/src/Sim/Car/Car.cpp` de `rocketsim_rs` 0.37.0) : convertir avec avant = `torque.y`, latéral = `−torque.x`. S'il est nul au tick du dodge, la
  direction est tirée de `(−pitch_input, roll_input)`.
- Tangage : −1 = nez vers le bas (dodge avant). L'annulation d'un dodge avant est donc `pitch_input ≥ +0,5`.
- Arène soccar standard (murs x = ±4 096, y = ±5 120).

## Tests scriptés

`mechanics::script` : générateur `Script` (intègre les positions, avance `tick`/`air_time`) et `cases()` = 31
séquences (≥ 1 positive + ≥ 1 négative par mécanique, compteurs exacts attendus), rejouables depuis un autre crate
avec `script::replay`. Inclut les cas de §1.3 : flip reset roues sur la balle sans toucher enregistré, toit vers la
balle (toucher sans reset), atterrissage au mur, speed flip annulé / non annulé / flip avant annulé.

Conséquences naturelles attendues : un toucher aérien compte aussi `touch` ; air dribble, double touch et
ceiling shot comptent aussi leurs `touch` et `aerial_touch` ; flick compte aussi `dribble`.

## Limites connues

- Séquences synthétiques, pas de trajectoires RocketSim : les seuils restent à valider sur de vraies parties
  (objectif PR 2 : 0 faux positif flip reset / speed flip sur 10 min de politique aléatoire).
- Hypothèse non vérifiée : `ball_hit_tick` change à chaque tick de contact continu (sinon la fusion à 4 ticks est
  sans effet, ce qui reste correct).
- `world_contact` de RocketSim ne concerne que le châssis : rouler sur le sol ne le met pas à vrai. Le terrain est
  donc reconnu par « `on_ground` et balle non porteuse ».
- Mur et plafond reconnus par l'orientation (`up`) et la hauteur, pas par la normale de contact ; les arènes
  non standard (hoops, dropshot) ne sont pas gérées pour le rebond mur.
- Wave dash : un dodge très près du sol compte même sans intention (c'est la définition retenue). Half flip :
  exige un roulis commandé après l'annulation.
- Pas de `bump`/`demo`, ni de « reset utilisé », ni de détection de nouveauté (§4.2 point 2) : à faire au-dessus des
  `Events`.

## Câblage et coût

- `sim-core` : `Sim::car_tick` / `ball_tick` lisent l'état au tick ; `flip_rel_torque` est converti du couple RocketSim
  en direction du dodge (avant, latéral). `Sim::place_car_airborne` sert aux scénarios scriptés.
- `rl-env` : quand `EnvConfig::detect_mechanics` est vrai (défaut), `Env::step_vs` avance tick par tick (même physique
  que `step(tick_skip)`) et nourrit le `Detector` de la voiture de l'agent ; l'adversaire n'est pas observé. Les
  compteurs remontent par `VecStep::mechanics` jusqu'à `trainer_mechanic_total{mechanic,context}` (contexte `match`).
- Tests réels (RocketSim) : flip reset roues vers la balle reconnu (4 roues, drapeaux remis à zéro), toit vers la balle =
  toucher sans reset (`crates/sim-core/tests/mechanics_sim.rs`) ; 10 min de jeu aléatoire sans flip reset, speed flip,
  ceiling shot, air dribble ni flick (`crates/rl-env/tests/random_policy.rs`).
- Coût : détecteur ≈ 63 ns/tick. Banc `rl-env --example bench` (2 threads, 16 envs, politique aléatoire,
  `NO_DETECT=1` pour couper) : 25,9 k pas/s sans, 25,2–25,3 k avec (≈ −3 %, dans le bruit de la machine), pour l'env
  seul ; l'apprentissage occupant ≈ 2/3 d'une itération, l'effet sur le débit de bout en bout est inférieur à 1 %.
