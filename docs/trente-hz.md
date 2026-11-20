# Mode 30 Hz (`tick_skip` 4)

Le bot décide par défaut toutes les 8 ticks de simulation (120 Hz) : 15 décisions par seconde de jeu. Le mode 30 Hz
(`tick_skip` 4) double ce rythme, pour des mécaniques plus fines (speed flip, flip reset, demi-volée). Il est **activable
par configuration** et le 15 Hz reste le défaut, inchangé.

## Activation

| Où | Réglage |
| --- | --- |
| learner | `--tick-skip 4` ou `TICK_SKIP=4` (8 par défaut ; seules 4 et 8 sont acceptées) ; `--eval-kickoff` le lit aussi |
| `rl_env::EnvConfig` | `.with_tick_skip(4)` (le champ `tick_skip` existait déjà) |
| politique exportée | `policy.json` porte `"tick_skip":4` (absent = 8) ; lu par `rst watch`, `rst play --vs-bot` |
| métriques | `trainer_tick_skip`, `trainer_game_seconds_per_second` |

## Speed flip : le seuil du détecteur était-il trop strict ?

**Oui.** `SPEED_FLIP_MAX_LAND_TICKS = 96` (0,8 s entre le dodge et le retour au sol) est physiquement inatteignable.
Banc `cargo run --release -p rl-env --example speed_flip_bench -- 20000 1,4,8` (coup d'envoi diagonal, 33 de boost,
saut tenu, relâche, dodge diagonal, annulation `p+1`, 20 000 essais aléatoires par résolution de décision) :

| Résolution | dodge → sol, minimum | p10 | médiane | vitesse horizontale max à l'atterrissage |
| --- | --- | --- | --- | --- |
| tick (120 Hz) | 113 ticks (0,94 s) | 132 | 168 | 1 935 |
| 30 Hz (`tick_skip` 4) | 116 | 132 | 168 | 1 886 |
| 15 Hz (`tick_skip` 8) | 116 | 130 | 168 | 1 835 |

Même au tick près, jamais moins de 113 ticks : le saut seul dure ≈ 0,9 s en l'air (impulsion de 292 uu/s, gravité 650).
Les speed flips de Nexto/Necto (`tick_skip` 8) existent donc bien à 15 Hz : c'est le seuil du détecteur qui était faux
(la fiche de l'étude, § 1.3 b, disait « retour au sol en < 0,8 s »). **Nouveau seuil : 150 ticks (1,25 s), vitesse > 1 750
uu/s** (33 de boost ne donnent pas plus de 1 935 ; 1 900 exclurait presque tout). La condition d'annulation reste : un
dodge diagonal non annulé (rotation complète) n'est pas un speed flip.

**Gain mesuré** : 1 500 uu depuis l'arrêt en 1,317 s au boost seul, 1,317 s pour le meilleur flip au tick, 1,325 s à 30 Hz,
1,333 s à 15 Hz. Un speed flip ne bat pas la ligne droite avec 33 de boost sur 1 500 uu ; l'étude le disait déjà
(§ 1.3 b : « même meilleur temps à 15 et 30 Hz »). Le 30 Hz n'est donc pas justifié par le speed flip seul : il l'est par la
finesse de contrôle (décisions tous les 33 ms au lieu de 67 ms) pour les mécaniques d'enchaînement courtes.

## Ce qui passe « par seconde de jeu »

Convention : les constantes et champs de config « par décision » restent exprimés **pour 15 Hz** (1/15 s) et sont convertis
à l'usage par `EnvConfig::decisions(n)` (durées) et `EnvConfig::step_scale()` (récompenses denses : 0,5 à 30 Hz). Aucun
changement de valeur à 15 Hz.

| Quantité | 15 Hz | 30 Hz | Où |
| --- | --- | --- | --- |
| `max_steps` (150 / 300 / 400) | tel quel | ×2 | `Env::max_steps` |
| horizon des drills courts | secondes × 120 / `tick_skip` | idem (déjà) | `Env::max_steps` |
| coup d'envoi : durée max, après toucher (90 / 30) | tel quel | ×2 | `KICKOFF_MAX_STEPS`, `KICKOFF_AFTER_TOUCH` |
| délai du fonceur (0–4 décisions) | tel quel | 0–9 | reset de coup d'envoi |
| `approach_weight`, `ball_goal_reward` | par décision | × 0,5 | récompense dense |
| pénalité de boost vide (0,003) | 0,045/s | 0,0015 par décision (0,045/s) | `boost_step` |
| touchers, but, boost ramassé, vitesse de frappe | événements / télescopiques | inchangé | — |
| γ | 0,99 | 0,99^½ ≈ 0,99499 | `PpoConfig::for_tick_skip` |
| λ (GAE) | 0,95 | 0,95^½ ≈ 0,9747 | idem |
| horizon PPO | 128 décisions (8,5 s) | 256 (8,5 s) | idem |
| minibatch | 512 | 1 024 (même nombre de minibatchs) | idem |
| cadences en itérations (gel de la copie, pool, curriculum, garde-fou) | tel quel | tel quel : une itération couvre la même durée de jeu par env | — |
| lr (`LR_SCALE` 5·10⁸ pas) | pas cumulés | pas de 1/15 s (2 décisions = 1) | `Trainer::iteration` |
| Elo : matchs, garde de boucle (20 000) | tel quel | ×2 pour la garde | `elo::play` |
| clips : état clé (5), avant (45), après (15), silence (150) | décisions | ×2 | `ClipRecorder` |
| démonstrations | voir PR #37 (`rl_env::demo`) | — | — |

Le total de récompense dense par seconde de jeu est ainsi identique aux deux rythmes (tests
`recompenses_denses_par_seconde_de_jeu`, `penalite_de_boost_vide_par_seconde_de_jeu`).

**Coût** : 2× plus de décisions par seconde de jeu, donc à débit de pas constant, deux fois moins de secondes de jeu par
seconde réelle. Voir les mesures plus bas.

## Reprise d'un checkpoint 15 Hz en 30 Hz

Mêmes poids : l'observation ne change pas (89 entrées ; l'« action précédente » reste valable, elle date de 33 ms au lieu de
67 ms). Comportement attendu au départ : chaque action est tenue **2 fois moins longtemps** (4 ticks au lieu de 8), la
politique se comporte donc comme si elle « bégayait » : plus d'oscillations de direction, les saute-boost et dodges commencent
plus tôt, l'entropie par décision monte. Elle redevient correcte en quelques centaines d'itérations.

`learner --resume --tick-skip 4` sur un état écrit à 15 Hz (le rythme de l'état est dans `<out>/tick-skip`, absent = 8) :

1. **sauvegarde automatique** de l'état d'avant dans `<out>/pre-tickskip-8/` (checkpoint, politique 15 Hz avec son
   `policy.json`, `explore.json`…) ; si elle échoue, le learner s'arrête sans basculer ;
2. `ent_coef` remis à 0,01, garde-fou à pleine échelle, **lr plafonné à 1e-4 pendant 500 itérations**
   (`Explore::begin_adaptation`, `adapt_iters` dans `explore.json`) ;
3. **échauffement du critique** 100 itérations (`--critic-warmup N`, 0 = aucun) : γ et le rythme changent, le critique 15 Hz
   n'est plus calibré, la politique est gelée le temps qu'il se recale ;
4. `pool/` et `refs/` (politiques 15 Hz jouées à 30 Hz seraient faussées) sont renommés `pool-15hz/`, `refs-15hz/`,
   `elo.json` → `elo-15hz.json`, l'Elo repart de zéro ;
5. retour possible : `restore-from` pointant vers `pre-tickskip-8` puis `--tick-skip 8`.

Le curriculum, les paliers, le registre de bonus de méca et les niveaux de drills sont conservés.

## Format de la politique et client

`policy.json` gagne un champ `tick_skip` (absent = 8). Le format des poids est inchangé.

- **Client 0.4.0** : il ignore le champ et jouerait une politique 30 Hz à 15 Hz. Pour l'en empêcher, le learner sert la
  politique sous `/v2/policy.{safetensors,json}` ; si `tick_skip ≠ 8`, les anciens chemins `/policy.*` répondent
  **410 Gone** (« mettre à jour le client ») et le client 0.4.0 garde sa politique installée.
- **Client suivant (release rst à publier)** : `rst bot pull` essaie `/v2/` puis retombe sur `/policy.*` (serveur 15 Hz plus
  ancien) ; lit `tick_skip` ; `Brain` décide tous les `tick_skip` ticks ; une valeur hors {4, 8} est refusée proprement.
  `rst watch` et `rst play --vs-bot` jouent ainsi la politique à son rythme.
- **Il faut donc une release rst** avant de déployer un learner en 30 Hz sur l'URL publique.

## Mesures

Nœud partagé (autres travailleurs en parallèle) : les débits sont indicatifs, ±30 %.

**Débit** (`STAGE=6 TICK_SKIP=… cargo run --release -p rl-env --example bench -- 2 16 20`, 2 threads, 16 envs, politique aléatoire) :

| tick_skip | décisions/s | ticks/s | secondes de jeu/s |
| --- | --- | --- | --- |
| 8 (15 Hz) | 10 383 et 8 505 | 83 064 et 68 038 | 692 et 567 |
| 4 (30 Hz) | 14 552 et 27 771 | 58 208 et 111 085 | 485 et 926 |

La simulation (ticks/s) borne le débit : à 30 Hz on décide 1,4 à 3 fois plus vite mais on simule à peu près le même nombre de
secondes de jeu ; l'observation et le détecteur coûtent par décision. En entraînement (itérations complètes, deux essais en
parallèle sur 2 threads chacun, 20 min) : 324 k décisions à 30 Hz (10 800 s de jeu) contre 209 k à 15 Hz (13 900 s de jeu), soit
**≈ 77 % de la durée de jeu par seconde réelle** à 30 Hz ; chaque itération PPO traite deux fois plus de décisions (horizon 256).

**Scripts experts** (`demo_bench 1500`, séquences vérifiées par le détecteur révisé) :

| Mécanique | 15 Hz | 30 Hz |
| --- | --- | --- |
| half flip | 28 % (23 décisions) | 45 % (41 décisions) |
| flip shot | 62 % | 64 % |
| speed flip | 17 % (ancien seuil : 0,01 %) | 17 % (56 décisions) |
| flick | 6 % | 5 % |

Le speed flip n'a rien de spécifique au 30 Hz : c'est le seuil du détecteur qui l'empêchait. Le half flip est plus souvent
réussi à 30 Hz (commandes plus fines).

**Coups d'envoi** (`learner --eval-kickoff 100 --tick-skip …`, 500 coups d'envoi) : la politique de prod, **sans aucun
entraînement**, gagne 84,4 % à 15 Hz et 81,8 % à 30 Hz (perdus 12,6 % / 15,4 %, premier toucher 95,0 / 92,8 %, même temps de
toucher 2,33 s). Elle reste donc jouable telle quelle en 30 Hz. Après 20 min d'entraînement : témoin 15 Hz 85,8 %, 30 Hz 83,0 %.

**Essai d'apprentissage 30 Hz depuis l'acteur de prod** (20 min, palier 6, `--init-actor prod --critic-warmup 20 --drill-prob 0.3
--no-clips --hit-speed-reward 0.5 --ball-goal-reward 0.02`, 16 envs ; témoin identique en 15 Hz) :

| | 15 Hz (témoin) | 30 Hz |
| --- | --- | --- |
| itérations / décisions | 102 / 209 k | 79 / 324 k |
| buts marqués / encaissés (self-play) | 148 / 152 (0,97) | 113 / 108 (1,05) |
| entropie moyenne | 0,518 | 0,523 |
| coups d'envoi gagnés / perdus (entraînement) | 188 / 97 | 135 / 85 |

**Pas de régression** : les buts, l'entropie et les coups d'envoi sont dans le bruit du témoin. L'essai est trop court (≈ 3 h de
jeu) pour juger l'apprentissage de mécaniques ; il montre que la politique de prod reprise en 30 Hz joue et progresse normalement.
La reprise `--resume` d'un état 15 Hz (sauvegarde, adaptation, pool mis de côté) est couverte par les tests, pas rejouée ici.
