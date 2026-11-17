# Self-play solide : somme nulle, pool, Elo, hystérésis

PR 5 du plan de `docs/specs/decouverte-mecaniques.md` (§ 3.2). Constat de la prod : 13 allers-retours 5 ↔ 6 en 5 h 30
(promu au palier 6 sur le taux de but, puis taux de but < 10 % et retour au palier 5). En self-play, le taux de but
est symétrique : il s'effondre dès que l'adversaire défend, sans que le niveau baisse. Code : `crates/learner/src/`
`pool.rs`, `elo.rs`, `curriculum.rs`, `ppo.rs`.

## Ce qui change

| Élément | Avant | Après |
| --- | --- | --- |
| Récompense palier 6 | but +10, encaissé −5, toucher +0,1, approche 0,05 | but +10, encaissé **−10** (somme nulle), pas de prime au toucher, approche 0,05 |
| Adversaire | copie gelée, rafraîchie toutes les 50 itérations | 70 % des envs : copie gelée (inchangée) ; 30 % : un ancien tiré du pool (un par itération) |
| Pool | — | 20 instantanés, un toutes les 200 itérations (≈ 400 k pas), `<out>/pool/NNNNNN.safetensors` (≈ 130 ko) ; tirage pondéré `0,2 + taux de buts encaissés lissé` |
| Évaluation | taux de but | Elo toutes les 30 min (`--elo-minutes`), 100 matchs gloutons contre chacune des 3 références |
| Promotion 5 → 6 | taux de but ≥ 0,8 | idem **et** Elo sans effondrement (`EloState::healthy`) |
| Régression 6 → 5 | taux de but < 0,1 | Elo lissé < record − 150 sur 2 mesures de suite ; délai de 1000 itérations avant de retenter |

Le taux de but n'est plus un critère de régression au palier 6 (il reste celui des paliers 2 à 5). Le terme dense de
vitesse de la balle vers le but adverse de la spec n'est pas ajouté (hors périmètre : touche `rl-env`) ; l'approche
existante (0,05) reste.

## Elo

Références (`elo.rs`) : le **fonceur scripté** (coups d'envoi ; ancre : 1000), la **politique figée du palier 5**
(`refs/stage5.safetensors`, copiée à la promotion 5 → 6, sinon à la première mesure ; sa note = son score contre le
fonceur) et le **meilleur ancien** (`refs/best.safetensors`, copiée à chaque record d'Elo ; sa note = l'Elo du
moment). Elo contre une référence = `note + 400·log10(p/(1−p))` (`p` borné à [0,02 ; 0,98], nul = ½) ; l'Elo publié est
la moyenne pondérée (`0,05 + 4p(1−p)`), puis lissé (EMA 0,5) pour décider. Matchs : 16 envs, mêmes graines, politique
gloutonne des deux côtés, kickoffs 40 % ; ≈ 20–60 s par mesure sur le nœud. État dans `<out>/elo.json`.

## Métriques (+ 20 séries, total ≈ 105 < 500)

`trainer_elo`, `trainer_elo_best`, `trainer_elo_vs{ref=charger|stage5|best}`, `trainer_elo_winrate{ref}`,
`trainer_elo_matches_total`, `trainer_elo_age_seconds`, `trainer_pool_size`, `trainer_pool_snapshots_total`,
`trainer_selfplay_goals_total{opponent=current|pool,side=scored|conceded}`,
`trainer_stage_changes_total{direction=up|down}`.

## Reprise et déploiement

Aucune remise à zéro : le checkpoint de prod est repris tel quel ; sans `elo.json`, `pool/` ni `refs/`, tout démarre
vide (premier instantané du pool dès la première itération du palier 6, première mesure d'Elo dès le palier 5).
Fichiers sur le PVC dans `--out` (aucun changement de volume). Réglages : `--elo-minutes N`, `--pool-every N`.

## Critères

- Local : essai court (voir la PR) : Elo mesuré, pool qui se remplit et se recharge, palier 5 non dégradé.
- Prod : **aucune régression 6 → 5 sur 48 h** (`trainer_stage_changes_total{direction="down"}` constant) et
  `trainer_elo` croissant sur la semaine ; surveiller `trainer_elo_vs{ref="best"}` (≈ 1000 + écart au record).
