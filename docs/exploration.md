# Exploration : entropie cible, plafond de KL, lr décroissant

PR 1 du plan de `docs/specs/decouverte-mecaniques.md` (§ 3.1). Constat de la prod : entropie 0,07 nat (max ln 78 ≈ 4,36)
avec `ent_coef` 0,01 fixe et lr 1e-3 constant. Code : `crates/learner/src/explore.rs`.

| Réglage | Valeur |
| --- | --- |
| Entropie visée | 1,25 nat (plage 1,0–1,5) ; gauge `trainer_entropy_target` |
| `ent_coef` | `× exp(0,03 × (cible − H))` à chaque itération, borné à `[1e-4, 1,0]` ; gauge `trainer_entropy_coef` |
| Plafond de KL | les époques PPO s'arrêtent quand la KL moyenne d'une époque dépasse 0,02 |
| lr | `max(5e-5, 3e-4 / (1 + pas / 5e8))` ; gauge `trainer_learning_rate` |

État persisté dans `<out>/explore.json` (`ent_coef`, pas cumulés) avec le checkpoint. Reprise d'un checkpoint de prod
sans ce fichier : `ent_coef` 0,01, pas à 0 (lr 3e-4), poids inchangés, pas de remise à zéro.

## Essai local (acteur de prod, palier 5)

Outil : `learner --init-actor policy.safetensors [--critic-warmup N]` charge l'acteur d'une politique servie
(`GET /policy.safetensors`) ; le critique est neuf et réchauffé N itérations, politique gelée.

- Plafond de `ent_coef` à 0,1 (première version) : 15 min, 1,2 M pas, entropie 0,09 → 0,115 : trop timide, car le
  gradient de l'entropie est minuscule quand le softmax est saturé. D'où le plafond à 1,0 et le gain à 0,03.
- Plafond 1,0, gain 0,03 : 8 min, 1,3 M pas : `ent_coef` atteint 1,0 en ≈ 80 itérations, entropie 0,107 → 0,20
  (en accélération), KL par itération 0,004–0,017 (< 0,02), taux de but du palier 5 en moyenne 0,59 (0,63 sur les
  200 dernières itérations) contre 0,51 pour l'acteur de prod gelé mesuré dans le même environnement
  (80 itérations). Pas de perte de taux de but ; remontée de l'entropie lente : la plage 1,0–1,5 demande des heures de
  prod (≈ 2× plus de pas/s que ce nœud), à surveiller sur `trainer_entropy`.
