# Démonstration : séquences expertes et clonage de comportement

Constat de prod (14 h, palier 6) : malgré les drills (`flip_shot` 49 %, `turnaround` 71 %, `flick` 15 % de réussite en drill), le
bot ne fait **jamais** en match `flip_shot`, `half_flip`, `flick`, `speed_flip` (compteurs à 0). Les drills récompensent la
mécanique mais PPO ne la trouve pas assez souvent pour l'apprendre ; on la **montre** : séquences expertes scriptées, vérifiées
par le détecteur, et une perte de clonage de comportement mélangée à PPO (spec `decouverte-mecaniques.md` §6, PR 6 et 10).

## 1. Séquences expertes (`rl_env::demo`)

Chaque séquence est jouée dans un `Env` privé (sans adversaire, détecteur v2 actif, `tick_skip` 8 = 15 Hz) **avec les actions de la
table de 90** : ce qui est vérifié est exactement ce qui est cloné (observation v3 du moment, indice d'action). Les scripts
choisissent les contrôles voulus, `nearest_action` prend l'action de la table la plus proche. Les états de départ sont ceux des
drills (`scenarios::sample`, positions, caps et vitesses aléatoires) ou d'un des 5 kickoffs. À la fin, la séquence est **jetée si
le détecteur n'a pas compté la mécanique** (et, pour le flip shot, si la balle ne part pas à ≥ 600 uu/s vers le but) ; sinon on garde
les décisions du début jusqu'à 2 décisions après la détection.

| Mécanique | État de départ | Script (au rythme des décisions) |
| --- | --- | --- |
| `half_flip` | drill `turnaround` (dos à la balle, arrêt / avant / marche arrière) | saut, tangage arrière, dodge arrière (`saut + p+1`), annulation (`p−1` + roulis ± lacet, 4–6 décisions), remise à plat en boucle fermée (tangage, roulis, lacet vers la balle), sortie boostée |
| `flip_shot` | drill `flip_shot` (balle au sol ou basse, voiture lancée derrière) | approche boostée vers la balle, saut quand le temps avant contact tombe à 0,28–0,5 s, relâche, dodge avant (± roulis = diagonal) dans la balle, boost |
| `flick` | drill `flick` (balle sur le toit, voiture lancée) | dribble en boucle fermée 10–30 décisions (accélérateur selon le décalage avant/arrière de la balle, braquage selon le côté), saut, dodge avant (± roulis) |
| `speed_flip` | un des 5 kickoffs | boost, saut, relâche, dodge diagonal `saut + p−1 + r±1`, annulation `p+1`, suite aérienne tirée au hasard |

**Taux de séquences vérifiées** (`cargo run --release -p rl-env --example demo_bench -- N [mécanique]`) : half flip ≈ 23 %,
flip shot ≈ 60 %, flick ≈ 5 %, speed flip ≈ 0,01 % avec l'ancien seuil du détecteur (**≈ 17 % depuis la PR 30 Hz**).

**Speed flip (mis à jour avec la PR 30 Hz)** : le détecteur exigeait un retour au sol ≤ 96 ticks (0,8 s) après le dodge, ce qui est
physiquement impossible (vol de 113 ticks au minimum, même au tick près : `docs/trente-hz.md`). Le seuil est passé à 150 ticks /
1 750 uu/s (+ gain de vitesse et cap conservé) : les scripts atteignent maintenant **≈ 17 % de séquences vérifiées à 15 Hz comme à
30 Hz**, et ne sont plus plafonnés. Les scripts suivent le rythme de décision (`DemoGen::with_tick_skip`, `--tick-skip` du learner) :
les durées en décisions sont multipliées par `8 / tick_skip`.

## 2. Perte de clonage dans PPO (`learner::ppo`)

Au démarrage (si λ0 > 0), `rl_env::demo::build_pool` génère `--demo-per-kind` séquences par mécanique (un thread chacune, ≈ 1 min
pour 150) et les range dans une **mémoire tampon bornée** (`--demo-buffer`, 50 000 échantillons, anneau : les plus anciens sont
écrasés). À chaque minibatch PPO, `--demo-batch` (128) échantillons de démonstration sont tirés et la perte de l'acteur devient

    L = L_PPO − c_ent·H + λ · CE(π(·|s_démo), a_démo) + λ · A · KL(π_réf(·|s) ‖ π(·|s))   (s = états de MATCH du lot PPO)

* `CE` : entropie croisée sur les actions de la table de 90, **seulement sur les états de démonstration** ; le critique n'est pas
  touché ; rien ne change au format de la politique ni du checkpoint (aucune release `rst` nécessaire) ;
* `λ = λ0 · max(0, 1 − pas / decay)` : linéaire, de λ0 à 0 sur `--demo-decay-steps` (30 M) pas de collecte comptés sous clonage
  (`<out>/demo-steps`, repris avec `--resume`) ;
* l'**ancre** (terme en `A`, défaut 50) : `π_réf` est la politique au démarrage du clonage (gelée, `Trainer::set_reference`), la KL
  est calculée sur les états de **match** du lot PPO (drills exclus : le clonage doit pouvoir y déplacer la politique). Sans
  elle le clonage détruit le match, voir § 3. Cause : la politique de prod est saturée (entropie ≈ 0,3, logits très grands,
  perte de clonage initiale ≈ 785) ; son gradient PPO est quasi nul sur les actions qu'elle joue (p ≈ 1) alors que celui de la CE sur
  des actions de probabilité ≈ 0 est de l'ordre de 1, et Adam normalise : il suit le clonage quelle que soit la valeur de λ.

Réglages (flag ou variable) : `--demo-lambda L` / `DEMO_LAMBDA` (**0 = désactivé, défaut**), `--demo-decay-steps N` /
`DEMO_DECAY_STEPS`, `--demo-per-kind N` (150), `--demo-buffer N`, `--demo-batch N`, `--demo-anchor A` (50).
**À chaud** (relu à chaque itération, sans redémarrage) : le fichier `<out>/demo-off` coupe le clonage (λ = 0), `<out>/demo-lambda`
(un nombre) remplace λ0 ; en prod : `kubectl exec … -- touch /data/runs/latest/demo-off`.

Métriques (2 séries) : `trainer_bc_loss` (entropie croisée moyenne sur les démonstrations, 0 si coupé), `trainer_bc_lambda` (poids
courant). Voir `docs/metrics.md`.

## 3. Mesures

Protocole : acteur de prod (`policy.safetensors`, palier 6, obs v3 / 90 actions), `--stage 6 --critic-warmup 20 --drill-prob 0.3
--no-clips --hit-speed-reward 0.5 --ball-goal-reward 0.02`, sur le nœud partagé (3 essais en parallèle, ≈ 1,4–1,8 M pas en 12–20 min), un
témoin sans clonage (λ = 0, 12 min, 1,65 M pas). Mesures : compteurs `trainer_mechanic_total` (`match` / `drill`), buts, coups d'envoi, et
`learner --eval-kickoff 100` sur la politique finale (500 coups d'envoi : prod 84,4 %, témoin 83,0 % de gagnés).

**1. Sans ancre, le clonage dégrade le match** (12 min, λ0 = 0,1 / 0,01 / 0,003) : coups d'envoi gagnés en entraînement 10–26 % (témoin
62 %), rapport buts marqués / encaissés 0,43–0,57 (témoin 0,87), entropie 0,15–0,17 (témoin 0,34). Les mécaniques apparaissent, mais
par dérive : `half_flip` 14 en match et 1 en drill (λ0 = 0,1), 1 / 1 (λ0 = 0,003), `flip_shot` 1 ; `flick` et `speed_flip` 0. Avec une
ancre plus naïve (−log π(a_jouée) sur les décisions de la collecte, 5 × λ) l'éval des coups d'envoi tombe à 0,6–1,6 % (500 coups) : la
politique ne touche plus la balle au coup d'envoi (le « saut + tangage » du half flip, appris depuis l'arrêt, déborde sur le kickoff).

**2. Avec l'ancre KL sur la politique de référence** (le réglage retenu) :

| Essai (λ0 / A / lot) | durée | perte de clonage | coups d'envoi gagnés (éval) | buts marqués / encaissés | half_flip / flip_shot / flick / speed_flip |
| --- | --- | --- | --- | --- | --- |
| prod, sans clonage | — | — | 84,4 % | — | 0 / 0 / 0 / 0 |
| témoin λ = 0 | 12 min | — | 83,0 % | 974 / 1 117 (0,87) | 0 / 0 / 0 / 0 |
| 0,1 / 10 | 12 min | 785 → 52 | 61,8 % | 349 / 495 (0,71) | 0 |
| 0,1 / 50 | 12 min | 302 | 81,6 % | 555 / 698 (0,80) | 0 |
| 0,1 / 50 | 20 min | 89 | 70,4 % | 1 096 / 1 205 (0,91) | 0 |
| 0,3 / 100 | 20 min | 180 | 82,8 % | 1 137 / 1 259 (0,90) | 0 |
| **0,3 / 30** | 20 min | **66** | **81,2 %** | 842 / 1 004 (0,84) | 0 |
| 1,0 / 60 | 20 min | 103 | 82,4 % | 982 / 1 209 (0,81) | 0 |
| 1,0 / 200 | 20 min | 321 | 83,2 % | 1 066 / 1 256 (0,85) | 0 |
| 0,3 / 100, lot 512 | 20 min | 258 | 79,0 % | 877 / 998 (0,88) | 0 |

(les 20 min comptent ≈ 1,4–1,8 M pas, un à deux ordres de grandeur sous les « quelques dizaines de M » de la décroissance ; le rapport
de buts est celui du self-play contre la copie gelée et le pool, bruité à ± 0,05.) Politique gelée, 3 min de drills `flip_shot` /
`turnaround` / `flick` seuls (≈ 1 000 épisodes chacun) : réussite de drill (bonus de base) identique à la prod (`flip_shot` 50–57 %,
`turnaround` 70–73 %, `flick` 14–16 %) et **aucun** `half_flip`, `flip_shot`, `flick` ou `speed_flip` détecté, avant comme après.

**Lecture.** Avec l'ancre, le clonage est inoffensif (coups d'envoi et buts dans le bruit du témoin pour A ≥ 30–100 selon λ ; à
A = 10 ou à λ0 = 0,1 / A = 50 sur 20 min, le match dérive) et la perte de clonage baisse nettement (785 → 66–180), **mais en 20 min
les mécaniques n'apparaissent pas encore** ni en drill ni en match : la perte reste trop haute (l'ancre retient la politique là même où le
clonage voudrait la déplacer) et l'horizon est trop court. Le test de la mécanique elle-même demande l'essai long (24 h+) ; il n'a pas
été fait ici. Le seul signe d'effet sur le comportement est celui de l'essai sans ancre, qui prouve que les séquences sont apprenables
mais pas qu'on peut les apprendre sans casser le reste.

**λ conseillé : λ0 = 0,3, ancre A = 30** (`--demo-lambda 0.3 --demo-anchor 30 --demo-decay-steps 30000000`), à surveiller au démarrage
en prod : `trainer_bc_loss` doit baisser (785 → < 100 en ≈ 2 M pas), coups d'envoi gagnés et buts inchangés ; sinon
`touch <out>/demo-off` (coupe tout de suite) ou monter `A` à 100 (`<out>/demo-lambda` règle λ0 seulement). Ne pas dépasser λ0 = 0,3 avec
A < 30. Défaut du code : **λ = 0 (désactivé)**, A = 50. Si les mécaniques n'apparaissent pas après ≈ 10 M pas sous clonage, la
cause est la politique saturée : envisager de distiller d'abord la prod à température plus haute (entropie ≈ 1) avant de cloner.

## 4. Limites et suites

* **Speed flip** : reproductible (≈ 17 %) depuis la révision du seuil du détecteur ; reste à voir si la politique l'apprend.
* **Flick** : 5 % des essais sont vérifiés (le dribble v2 exige 5 touchers et 600 uu) ; échantillons peu nombreux (≈ 27 décisions chacun).
* **Formats** : aucun changement du format de la politique ni du checkpoint (donc pas de release `rst`) ; deux séries de métriques de plus.
* Les démonstrations sont régénérées à chaque démarrage (≈ 1 min) ; elles ne sont pas sauvegardées.
