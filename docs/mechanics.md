# Détecteur de mécaniques (`crates/mechanics`)

Détecteur **par tick, sans récompense** (étude du 15/11/2026, §2.1 et §4.1–4.2). Un `Detector` par voiture,
appelé à chaque tick de physique (120 Hz, dans la boucle des 8 ticks d'une décision) avec un `CarTick` et un
`BallTick` ; `reset()` à chaque nouvel épisode ou téléportation (le tick suivant sert seulement à initialiser).
Chaque occurrence reconnue incrémente **une fois** `Counts[mechanic]` (→ `trainer_mechanic_total{mechanic,context}`).
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

## Détecteur v2 (strict) et v1

Les clips de prod montraient surtout du bruit (aerial_touch = rouler sur le mur et suivre une balle qui monte, wall
shot et dribble sans intention, dribble de 0,5 s). La **v2** (défaut) ne reconnaît que des actions intentionnelles ;
la **v1** (règles d'origine, tableau ci-dessous) reste disponible : `Detector::with_mode(Mode::V1)`,
`EnvConfig::detector_v1`, flag du learner `--detector-v1` ou `DETECTOR_V1=1`. Les compteurs
`trainer_mechanic_total{mechanic}` gardent les mêmes noms (mêmes 12 séries) ; en v2 ils comptent les règles strictes.

Règles v2 (seuils dans `thresholds.rs`, section « v2 ») :

| Mécanique | Règle v2 |
| --- | --- |
| `aerial_touch` | toucher en l'air, voiture z > 300, balle z > 500, en l'air ≥ 0,5 s (60 ticks), **et** intention : saut pressé depuis le sol OU le mur (≤ 6 ticks du décollage), ou décollage au boost avec la balle portée (côté toit, < 250 uu), ou chaîne de touchers contrôlés au boost (air dribble). Exclu : rouler du mur puis suivre une balle qui monte, sans saut, sans boost, sans contrôle |
| `air_dribble` | ≥ 2 touchers aériens (mêmes conditions de hauteur), < 1 s d'écart, boost actif entre les deux, sans retour au sol ; le décollage peut venir du sol, du mur (wall to air dribble) en saut ou en poussée au boost |
| `wall_shot` | toucher sur le mur (ou ≤ 1 s après) qui envoie la balle vers le but adverse à > 1 000 uu/s avec un gain ≥ 300 uu/s imputable au toucher (jugé dans les 6 ticks), ou saut depuis le mur |
| `dribble` | mêmes conditions géométriques qu'en v1 puis ≥ 5 touchers distincts (≥ 10 ticks d'écart ; le contact continu est fusionné à 4 ticks) ou (contrôle ≥ 2 s et ≥ 3 touchers), et ≥ 600 uu parcourus par la voiture pendant le contrôle |

`flick` s'appuie sur le dribble validé (v2 : dribble strict). Flip reset, ceiling shot, double touch, speed flip,
wave dash, half flip : inchangés (le ceiling shot et le double touch utilisent le toucher aérien « de base »,
sans l'intention exigée).

**Mesure sur de vraies trajectoires** : 200 clips v1 de prod (14 aerial_touch, 93 wall_shot, 93 dribble ; `GET
https://rocketsim.celri.dev/clips/<id>`), rejoués tick par tick dans RocketSim
(`cargo run --release -p sim-core --example audit_clips -- <dossier> [--verbose]`, meshes requis). Le rejeu
re-détecte 200/200 en v1 (le rejeu est fidèle). Passent en v2 : **aerial_touch 0/14, dribble 18/93, wall_shot 23/93,
soit 41/200**. Constats : `ball_hit_tick` change bien à chaque re-contact pendant un contrôle (2 à 53 changements
bruts par séquence), donc on compte les touchers sans équivalent temporel ; aucun clip n'a plus de 183 ticks (1,5 s) de
contrôle continu, la règle « ≥ 2 s » n'est donc jamais le critère décisif sur ces données. Le seuil de 1 000 uu/s du
wall shot est le plus sensible : 23/93 à 1 000, 3/93 à 1 500, 0/93 à 2 000 (`WALL_SHOT_MIN_GOAL_SPEED`). Le gain ≥ 300
uu/s est un ajout (sans lui une balle déjà lancée vers le but compterait).

**Qualité** : chaque clip porte `quality` = `{ball_speed_to_goal, goal_3s, score}` (vitesse maximale de la balle vers le
but adverse dans la seconde qui suit la mécanique ; but dans les 3 s ; score = 0,5·min(vitesse/3 000, 1) + 0,5·but),
joint quand la fenêtre de 3 s est écoulée ou à la fin de l'épisode (valeur partielle). Un clip n'est enregistré que si
la mécanique est validée par le détecteur actif (v2 par défaut). `GET /clips` expose `detector` (1 ou 2) et `quality` ;
les anciennes entrées sans champ se lisent v1. Au premier démarrage v2, l'anneau est vidé une fois (marqueur
`clips/detecteur-v2`, pas de vidage avec `--detector-v1`) ; la page web affiche la qualité et filtre « v2 seulement » si
des clips v1 subsistent.

## Catalogue v1 (seuils : `crates/mechanics/src/thresholds.rs`)

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

## Catalogue étendu (41 mécaniques)

Recherche : définitions communautaires (guides ginx, dignitas, gamersrdy, ggrecon ; Liquipedia et le wiki Fandom
n'étaient pas lisibles) et constantes RLGym (vitesse max de balle 6 000, voiture 2 300, supersonique 2 200).
Chaque mécanique est classée **(a)** fiable avec ce qu'on lit (voiture et balle de l'agent par tick), **(b)**
approximation (dite), **(c)** non détectable. Une occurrence compte une fois ; les sous-types comptent **en plus** de
leur famille (un `kuxir_pinch` est aussi un `pinch`, un `flick_45` est aussi un `flick`). Valeur = vitesse de balle après
le toucher (uu/s, jugée sur les 4 ticks qui suivent) ; la colonne « record » indique celles qui alimentent
`trainer_mechanic_record{mechanic}` (meilleure valeur depuis le démarrage du learner, matchs et drills).

| Méca | Cat. | Signature (seuils : `thresholds.rs`) | Record |
| --- | --- | --- | --- |
| `touch` | a | `ball_hit_tick` change (contacts ≤ 4 ticks fusionnés) | |
| `power_shot` | a | balle > 3 000 uu/s après toucher, gain ≥ 500, pas un pinch | oui |
| `pinch` | a | gain > 2 500, balle > 3 500 après, **coincée** (< rayon + 60 uu d'une surface, voiture côté intérieur). Support en métadonnée : `ground`, `wall`, `backwall`, `corner`, `ceiling` | oui |
| `kuxir_pinch` | b | pinch contre mur/fond/coin, dodge ≤ 0,35 s avant, balle > 1 500 uu/s vers le but adverse (le départ du mur défensif n'est pas exigé) | oui |
| `cushion` (catch) | b | balle > 1 000 avant, vitesse relative balle−voiture < 400 après, vitesse après < 0,5 × avant | |
| `redirect` | b | voiture en l'air (z > 300), balle > 1 500 avant et après, déviée > 45° | oui |
| `air_roll_shot` | b | toucher aérien (≥ 0,3 s en l'air, balle z > 300), \|ω·avant\| > 3 rad/s ou \|roll\| ≥ 0,5, sans dodge ≤ 0,65 s, balle > 1 500 | oui |
| `backboard_read` | b | toucher ≤ 2 s après un rebond de la balle sur le mur du fond, balle z > 300 | |
| `backboard_double` | a | `double_touch` dont le rebond est sur le mur du fond | oui |
| `half_volley` | b | toucher 1 à 8 ticks après un rebond de balle au sol, balle z < 300, > 1 800 après | |
| `aerial_touch`, `air_dribble`, `wall_shot`, `double_touch`, `ceiling_shot`, `flip_reset` | a/b | voir ci-dessus (v2) | wall_shot, ceiling_shot, double_touch |
| `dribble` | a | v2 ci-dessus | |
| `hood_dribble` | b | dribble dont la balle est sur le capot (avant moyen > 35 uu) | |
| `reverse_dribble` | b | balle sur le toit, voiture en marche arrière (< −300 uu/s) ≥ 0,5 s, 3 touchers, 400 uu | |
| `wall_dribble` | b | portage sur un mur ≥ 0,5 s, 400 uu, ≥ 1 toucher | |
| `ceiling_dribble` | b | portage au plafond ≥ 0,4 s | |
| `bounce_dribble` | b | ≥ 3 touchers doux (10–60 ticks d'écart) voiture au sol > 300 uu/s, balle qui rebondit au sol entre deux | |
| `flick` | a | dodge ≤ 0,4 s après un dribble, balle > 1 500 | oui |
| `flick_45` / `flick_90` | b | flick avec dodge diagonal avant (secteurs 1, 7) / latéral (2, 6) | oui |
| `musty_flick` | b | flick avec dodge arrière (avant < −0,5), balle au-dessus du capot ; la bascule du nez n'est pas vérifiée | oui |
| `delayed_flick` | b | dodge ≥ 0,3 s après le saut, balle encore portée | oui |
| `breezi_flick` | b | flick avec rotation \|ω·haut\| > 3 rad/s au toucher (approximation « tornade » : l'air roll n'est pas vu) | oui |
| `wall_to_air_dribble` | b | `air_dribble` dont la voiture roulait sur un mur ≤ 1,5 s avant le premier toucher | |
| `double_reset` | a | 2e flip reset sans toucher le terrain (le 3e n'est pas recompté) | |
| `ceiling_reset` | b | flip reset avec la balle à z > 1 800 (sens « roues au plafond » non compté : c'est `ceiling_shot`) | |
| `reset_shot` | b | après un reset, nouveau dodge puis toucher ≤ 0,3 s, balle > 1 500 | oui |
| `speed_flip`, `wave_dash`, `half_flip` | a/b | voir catalogue v1 | |
| `chain_dash` (zap dash) | b | 2 wave dashes à ≤ 1,5 s | |
| `wall_dash` | b | dodge ≤ 0,3 s après le mur, retour au terrain ≤ 1 s, gain > 200 uu/s | |
| `fast_aerial` | b | saut puis second saut sans dodge ≤ 0,35 s, boost et nez vers le haut entre les deux | |
| `stall` | b | dodge en l'air (z > 300) latéral/arrière avec roulis, vitesse < 60 % de l'avant dans les 0,3 s (le lacet n'est pas lu) | |
| `tornado_spin` | b | \|ω·haut\| > 4 rad/s ≥ 0,3 s en l'air | |
| `recovery` | b | atterrissage à plat après ≥ 0,6 s en l'air dont un moment renversé/de côté, en gardant ≥ 60 % de la vitesse | |

Séries Prometheus : `trainer_mechanic_total{mechanic,context}` = 41 × 2 ; `trainer_mechanic_record{mechanic}` = 16 ; le
total reste très sous les 500 séries (test `series_stay_far_below_the_cap`). Les nouvelles mécaniques ne rapportent
**aucun bonus** en drill (`base_bonus` → 0) : à décider. Clips : toutes sauf `touch` ; la mécanique la plus précieuse
du pas gagne (`Mechanic::clip_priority`), le clip porte `detail = {value, support}` (vitesse de balle, support du pinch)
et la page web liste les mécaniques présentes dans les clips (filtre automatique) avec la valeur et le support.

### (c) Non détectable avec CarTick/BallTick de l'agent

| Méca | Raison |
| --- | --- |
| `bump`, `demo`, `pinch` contre une voiture | aucune donnée de l'adversaire dans `CarTick` (demo = état « démolie » de l'autre voiture) |
| `50/50` | deux voitures adverses : l'adversaire n'est pas observé |
| `boost steal`, gestion de boost | ni pads ni quantité de boost dans `CarTick` |
| `fake`, `shadow defense`, `kill`/`clear`/`pass`/`rotation`, `bang it` | dépendent de l'intention et de la position des autres joueurs (une fake = absence de toucher) |
| `psycho` | définition floue selon les sources (backboard, retourné), pas de signature stable |
| `ceiling musty` | combinaison de reset, plafond et musty non définie de façon univoque ; couvert partiellement par `ceiling_shot` + `musty_flick` |
| `powerslide`, `bunny hop`, `curb dash`, `pogo` | frein à main et lacet absents des entrées ; signature trop proche d'un virage ou d'un saut ordinaire |
| `squishy save`, `turtle dive`, `tomahawk`, `popping`, `ground pound` | trop floues ou dépendantes du contexte (rumble, intention) |
| `ball cam`, caméra | état de caméra absent de la simulation |
| `pre-flip`, `drop kick` | le « vers où la balle sera » est une intention ; `drop kick` ≈ `half_volley` côté avant le rebond, non mesuré |
| `flip cancel` | non listée à part : le détecteur de base lit déjà l'annulation (`CANCEL`) pour speed flip et half flip |
| `aztral pinch` | = pinch (coin/mur offensif) : couvert par `pinch` + support `corner`/`wall` |

Faux positifs connus : un tir au sol très puissant (balle à l'arrêt près d'un mur) peut dépasser 3 500 après gain > 2 500
sans vrai pincement ; le critère « voiture côté intérieur » les limite. Les seuils sont des valeurs de départ à ajuster sur
de vraies parties.

## Conventions d'entrée (câblage)

- `flip_rel_torque` est attendu comme **direction du dodge** : `[0]` avant (+) / arrière (−), `[1]` latéral
  (+ droite). Dans RocketSim, `flipRelTorque` est un couple (`(−dodgeDir.y, dodgeDir.x, 0) / tickTimeScale` avec
  `dodgeDir = (−pitch, yaw + roll)`, vérifié dans `RocketSim/src/Sim/Car/Car.cpp` de `rocketsim_rs` 0.37.0) : convertir avec avant = `torque.y`, latéral = `−torque.x`. S'il est nul au tick du dodge, la
  direction est tirée de `(−pitch_input, roll_input)`.
- Tangage : −1 = nez vers le bas (dodge avant). L'annulation d'un dodge avant est donc `pitch_input ≥ +0,5`.
- Arène soccar standard (murs x = ±4 096, y = ±5 120).

## Tests scriptés

`mechanics::script` : générateur `Script` (intègre les positions, avance `tick`/`air_time`) et `cases()` = 46
séquences (≥ 1 positive + ≥ 1 négative par mécanique, compteurs exacts attendus ; v2 : positifs wall to air dribble, saut
depuis le mur, dribble long ; négatifs roulé du mur, sans saut, trop court/bas, wall shot lent/vers son but/sans gain,
dribble continu/à l'arrêt/peu de touchers) ; `script::replay_mode(frames, Mode::V1)` rejoue en v1, rejouables depuis un autre crate
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
