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
flip shot ≈ 60 %, flick ≈ 5 %, **speed flip ≈ 0,01 %** (1 sur 9 000 : 0 sur 12 000 dans un autre tirage).

**Le speed flip n'est pas reproductible à 15 Hz avec ce détecteur.** Le détecteur exige le retour au sol ≤ 96 ticks (0,8 s) après le
dodge ; à 15 Hz le saut dure au moins 8 ticks (touche maintenue pendant la décision), il faut une décision de relâche, puis le dodge
se fait à ≈ 60 uu de hauteur, et la voiture reste en « flip » (contrôle aérien coupé) ≈ 0,65 s : le plus tôt mesuré est à 112 ticks
(14 décisions) dans 3 000 essais. La grille 1.3 b de l'étude (dodge diagonal annulé) n'a de « vrai speed flip » qu'à 30 Hz. Le
générateur le tente quand même (essais plafonnés, ≈ 3 s) et n'ajoute **aucune** séquence si le détecteur ne la reconnaît pas ; pour
l'obtenir il faudrait 30 Hz ou un seuil `SPEED_FLIP_MAX_LAND_TICKS` ≈ 120–140 (hors périmètre de cette PR, détecteur partagé).

## 2. Perte de clonage dans PPO (`learner::ppo`)

Au démarrage (si λ0 > 0), `rl_env::demo::build_pool` génère `--demo-per-kind` séquences par mécanique (un thread chacune, ≈ 1 min
pour 150) et les range dans une **mémoire tampon bornée** (`--demo-buffer`, 50 000 échantillons, anneau : les plus anciens sont
écrasés). À chaque minibatch PPO, `--demo-batch` (128) échantillons de démonstration sont tirés et la perte de l'acteur devient

    L = L_PPO − c_ent·H + λ · CE(π(·|s_démo), a_démo) + λ · A · (−log π(a_jouée | s_collecte))

* `CE` : entropie croisée sur les actions de la table de 90, **seulement sur les états de démonstration** ; le critique n'est pas
  touché ; rien ne change au format de la politique ni du checkpoint (aucune release `rst` nécessaire) ;
* `λ = λ0 · max(0, 1 − pas / decay)` : linéaire, de λ0 à 0 sur `--demo-decay-steps` (30 M) pas de collecte comptés sous clonage
  (`<out>/demo-steps`, repris avec `--resume`) ;
* l'**ancre** (terme en `A`, défaut 5) : sans elle, le clonage dégrade le match même à λ minuscule (voir § 3). La politique de prod
  est saturée (entropie ≈ 0,3, perte de clonage initiale ≈ 785) : le gradient de PPO est quasi nul sur les actions qu'elle joue
  (p ≈ 1) alors que celui de la CE sur des actions de probabilité ≈ 0 est de l'ordre de 1 ; Adam normalise, donc il suit le
  clonage quelle que soit la valeur de λ. `−log π(a_jouée)` sur les décisions de la collecte vaut en espérance
  `H + KL(π_ancienne ‖ π)` : c'est une ancre sur les états de match, pas une perte de clonage sur eux.

Réglages (flag ou variable) : `--demo-lambda L` / `DEMO_LAMBDA` (**0 = désactivé, défaut**), `--demo-decay-steps N` /
`DEMO_DECAY_STEPS`, `--demo-per-kind N` (150), `--demo-buffer N`, `--demo-batch N`, `--demo-anchor A`.
**À chaud** (relu à chaque itération, sans redémarrage) : le fichier `<out>/demo-off` coupe le clonage (λ = 0), `<out>/demo-lambda`
(un nombre) remplace λ0 ; en prod : `kubectl exec … -- touch /data/runs/latest/demo-off`.

Métriques (2 séries) : `trainer_bc_loss` (entropie croisée moyenne sur les démonstrations, 0 si coupé), `trainer_bc_lambda` (poids
courant). Voir `docs/metrics.md`.

## 3. Mesures
