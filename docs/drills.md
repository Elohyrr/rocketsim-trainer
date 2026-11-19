# Drills de mécaniques

Spec : `docs/specs/decouverte-mecaniques.md` §2.2–2.3 (branche `claude/etude-mecas`), PR 7 du plan (§6).
Idée : **guider par l'état de départ avant de guider par la récompense**. Une part des épisodes commence dans une
situation qui montre une mécanique au bot (balle sur le toit, balle haute…) ; le reste reste du match. Le match et le
self-play ne contiennent **aucun** terme de mécanique : le détecteur y compte, sans récompenser.

## Activation

| Réglage | Défaut | Effet |
|---|---|---|
| `--drill-prob P` du learner, ou variable `DRILL_PROB` | `0` (coupé) | part des épisodes en drill, bornée à [0 ; 0,9]. Conseillé : `0.3` |
| `--drill-weights label=poids,…` ou `DRILL_WEIGHTS` | voir « Pondération » | poids de tirage par drill (labels de la table ci-dessous) ; label inconnu ou poids négatif = le learner refuse de démarrer |
| `--drill-clips` ou `DRILL_CLIPS=1` | coupé | garde aussi les clips des drills de dribble (`roof_still`, `roof_moving`, `dribble_turn`) ; jamais les autres |

Sans effet aux paliers 0 et 1 (`end_on_touch`) : un drill y finirait au premier toucher. Chaque épisode tire d'abord le
drill (probabilité `P`), sinon le coup d'envoi (paliers 5/6), sinon l'apparition aléatoire. Observations et actions
sont inchangées : un checkpoint de prod se reprend tel quel (aucune remise à zéro).

## Les 16 drills (`crates/scenarios`)

Fonctions pures `scenarios::sample(drill, unit)` ; `rl-env` applique l'état (`Env::reset_drill`). Tirage pondéré (voir « Pondération »).

**Boost de départ aléatoire** (tous les drills, `scenarios::random_boost`) : 25 % de 0 à 20, 25 % à 100, 50 % uniforme
sur 0–100, pour apprendre les mécaniques avec ET sans boost. Hors drill, le boost de départ est 33 (ou aléatoire aux paliers 5/6, `docs/observation.md`). Aucune
récompense de boost en drill (la prime de ramassage ne vaut qu'en match, `docs/observation.md`) : les bonus de méca ne dépendent pas du boost restant, donc ne poussent pas à en gaspiller, et rien ne
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

**Drills v2** (« packs d'entraînement » maison : tout est tiré dans des plages continues, jamais deux fois le même
état, donc rien à apprendre par cœur). Boost de départ aléatoire comme ci-dessus (≥ 40 pour `aerial_high`,
`wall_run`, `shot_lob`). Poids par défaut entre parenthèses.

| Drill | Famille | Départ | Réussite |
|---|---|---|---|
| `dribble_turn` (3) | dribble | balle sur le toit, voiture à l'arrêt (40 %) ou à 300–1100 uu/s, **cap à ±0,8–π du but dans 75 % des cas** (de côté ou dos au but), adversaire **actif** entre 1500 et 4300 en Y, face à la voiture | but |
| `shot_roll` (1,5) | attaquant | balle qui roule vers le but (cap ±0,9 rad, 400–1500 uu/s), voiture 900–2500 uu derrière, cap ±0,7 rad vers la balle | but |
| `shot_bounce` (1,5) | attaquant | balle à 400–1000 uu qui descend (−300 à +200) et avance vers le but, voiture 700–2200 derrière | but |
| `shot_cross` (1,5) | attaquant | centre : balle à 2600–3600 du centre, lancée vers le devant du but (balistique), voiture dans l'axe | but |
| `shot_lob` (1) | attaquant | balle à 500–1300 uu qui avance vers le but, voiture au sol derrière | but |
| `keeper` (2,5) | gardien | balle tirée vers SA cage (largeur du but, 1300–2600 uu/s, balistique) depuis (±3000, −3500…0), voiture en y −4600…−2200 (devant sa cage), cap vers la balle ; **6 s** | arrêt sans but encaissé |
| `aerial_high` (1) | aérien | balle à 900–1700 uu en mouvement, voiture au sol à 800–2200 | toucher en l'air (balle > 300) |
| `wall_run` (1) | mur | balle qui longe et monte le mur latéral (x 3700–3980, +600–1600 en Y), voiture au pied du mur | but |
| `rebound` (1,5) | récupération | balle repartie du mur du fond (y 3800–4900, −500 à −1800 uu/s), voiture à mi-terrain | but |

Les épisodes courts (horizon `Env::max_steps`) : gardien 6 s ; `shot_roll/bounce/cross`, `rebound` 10 s ;
`shot_lob`, `aerial_high`, `wall_run` 8 s ; `dribble_turn` 20 s (autres : `max_steps` du palier).

Pas de drill « plafond » ni de speed flip/wave dash/half flip (pas d'état de départ utile : ils se jouent depuis le
kickoff ou le sol).

## Pondération

`Drill::DEFAULT_WEIGHTS` (ordre de `Drill::ALL`) : les 7 drills de la v1 à 1, `dribble_turn` 3, `keeper` 2,5,
`shot_roll`/`shot_bounce`/`shot_cross`/`rebound` 1,5, `shot_lob`/`aerial_high`/`wall_run` 1 (≈ 21 au total : un drill
sur 7 est un virage de dribble). `--drill-weights dribble_turn=5,keeper=0` surcharge ces valeurs ; un poids à 0
retire le drill. Poids tous nuls : tirage uniforme.

## Opposition : adversaire garé, sauf `dribble_turn`

En drill, l'adversaire (paliers 5/6) est **garé immobile** près de sa cage (x ∈ ±1500, y = 4600) et la politique
adverse est ignorée. Raisons : (1) on montre la mécanique, pas la défense : un adversaire actif transforme 30 % des
épisodes en match bruité, avec des conflits de ballon qui masquent le signal ; (2) c'est déterministe pour le
détecteur et le bonus d'utilité (balle vers le but) ; (3) la voiture adverse reste dans la simulation, donc la forme
des observations et le self-play vectorisé ne changent pas. L'adversaire garé est inoffensif ; une balle bien frappée
passe à côté. La confrontation reste dans les 70 % de matchs.

**Exception v2 : `dribble_turn`.** Le constat d'Elohyrr (bon joueur) sur les clips : balle sur le toit, le bot va
toujours tout droit ; il sait redresser un départ en virage mais pas tourner une fois la balle posée, et il craint un
bot qui dribble en ligne droite sans flick. Ce drill garde donc l'adversaire **actif** : placé entre 1500 et 4300 en Y,
face à la voiture, il joue avec la politique adverse (self-play comme en match ; sans self-play il reste inerte, comme
avant). Raison : sans défenseur, tourner n'est pas nécessaire, juste plus lent ; avec un défenseur le virage devient la
condition pour marquer. Conflits de ballon inclus : c'est voulu pour cette famille seulement.

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

### Bonus de dribble de virage (v2)

Détecteur : `dribble_turn` (42ᵉ mécanique, `mechanics::ext_carry`). Pendant un portage au sol à plat (fenêtre toit du
dribble), le cap du nez change d'au moins 45° (cumul signé, `DRIBBLE_TURN_MIN_ANGLE` = 0,785 rad) en roulant à plus
de 250 uu/s ; une fois par portage. Le détecteur ne récompense rien ; il compte en match comme en drill, ce qui
permet de voir si le virage appris en drill est réutilisé en match. Cas scriptés : virage de 80° positif ; ligne droite
et 29° négatifs.

Bonus (drills seulement, plafonnés à 10 % du but, décroissants `b0/√(1+n/N)` comme les autres) :

- **virage** : `b0 = 0,2`, payé tout de suite (pas besoin d'une balle qui file vers le but : une balle portée n'y va
  pas), 3 par épisode au plus, sans consommer le plafond de 2 bonus classiques ;
- **but après virage ou flick** : un but dans les 6 s (720 ticks) qui suivent un `dribble_turn` ou un `flick` rapporte
  0,5 (une fois, registre horaire propre). Un dribble en ligne droite, même suivi d'un but, ne rapporte pas ce bonus :
  c'est la correction visée.

### Gardien (drill `keeper`)

Un toucher de l'agent qui arrête une balle qui menaçait sa cage (vitesse vers −Y ≥ 500 uu/s avant, < 200 après) est un
**arrêt** : +1,0 (plafonné, décroissant, registre propre), l'épisode finit 1,5 s plus tard (dégagement). But encaissé :
la pénalité de but contre son camp du palier (5) **plus** −1,0 propre au drill ; l'épisode finit sur le but.

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

- `trainer_drill_episodes_total{drill}` (16 séries) ; **`trainer_drill_success_total{drill}`** (16 séries, réussite
  par famille : but pour les tirs, dribbles, `wall_*`, `rebound` ; arrêt sans but encaissé pour `keeper` ; toucher en l'air
  balle > 300 uu pour `aerial_touch`, `air_dribble`, `aerial_high` ; flip reset vu pour `flip_reset` ; verdict `good` pour
  `recovery`) ; `trainer_drill_dribble_goal_total` (buts payés après virage/flick) ; `trainer_drill_saves_total` ; `trainer_drill_recovery_total{outcome}` (good, wheels, bad, none) ; `trainer_drill_bonus_total{mechanic}` (9 séries : celles qui ont
  un `b0 > 0`, dont `dribble_turn`) ; `trainer_mechanic_total{context="drill"}` (le label existait déjà).
- Les compteurs de match (`context="match"`) ne contiennent pas les mécaniques vues en drill : c'est ce qui permet
  de dire si une méca apprise en drill est **réutilisée spontanément en match**.

Séries ajoutées par la v2 : 9 (épisodes) + 16 (succès) + 1 (bonus) + 2 (compteurs) = 28 (≤ 30) + 2 pour le label
`dribble_turn` de `trainer_mechanic_total` (match/drill). Le garde-fou de séries du test passe de 200 à 250 ; la
limite dure reste 500.

## Packs d'entraînement de la communauté : pas d'import

Question : importer de vrais packs (Custom Training de Rocket League) pour en tirer des états initiaux ? Recherche
faite le 2026-11-19 (sources publiques, non vérifiées sur fichiers réels) :

- **Fichiers locaux `.Tem`** (dossier `TAGame/Training` du jeu) : sérialisation Unreal binaire propriétaire, non
  documentée. Un décodeur open source existe (RocketRP, C#, licence MIT : `RocketRP.TrainingCLI -f pack.tem -p`
  sort du JSON), mais le schéma n'est pas publié et je n'ai pas pu le vérifier sur un vrai fichier ici.
- **BakkesMod** : pas de format d'échange standard ; le plugin VersatileTraining exporte du JSON sans schéma public.
- **Sites et codes de packs** (Prejump, codes `XXXX-XXXX-XXXX-XXXX`) : pas d'API publique, conditions d'utilisation et
  droits des auteurs de packs non lisibles ; ce sont des créations de joueurs, hébergées sur le service de Psyonix.
  Aucune source dont la redistribution soit clairement permise.

Décision : **pas d'importeur ni de packs dans ce dépôt public** (pas de source légale vérifiable, schéma `.Tem` non
vérifié ; un téléchargeur serait du scraping de contenu tiers). À la place, le générateur de la v2 reprend le principe
des packs (balle + voiture + vitesse + boost, par famille de situation) avec des plages continues écrites ici. Pour
utiliser ses propres packs plus tard : les convertir en JSON avec RocketRP sur ses fichiers `.Tem` locaux, puis
ajouter une famille de `scenarios` qui lit ce JSON (position, vitesse, rotation de la balle ; position, rotation,
vitesse, boost de la voiture) — à faire hors dépôt public.

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
