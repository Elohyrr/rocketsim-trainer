# Métriques Prometheus (`/metrics`)

Liste **fermée** : toute nouvelle métrique passe par une PR qui met à jour ce fichier **et** le test de plafond.
Règles : labels à valeurs bornées (énumérations fixes), jamais d'identifiant d'épisode / de run, histogrammes
à ≤ 8 buckets. Objectif : < 100 séries, plafond dur 500 (un test du crate `learner` échoue au-delà).

| Métrique | Type | Labels (valeurs) | Séries |
| --- | --- | --- | --- |
| `trainer_steps_total` | counter | — | 1 |
| `trainer_steps_per_second` | gauge | — | 1 |
| `trainer_episodes_total` | counter | — | 1 |
| `trainer_reward_mean` | gauge | — | 1 |
| `trainer_episode_length_mean` | gauge | — | 1 |
| `trainer_policy_loss`, `trainer_value_loss`, `trainer_entropy`, `trainer_kl` | gauge | — | 4 |
| `trainer_iteration_seconds` | histogram (≤ 8 buckets) | — | ≈ 10 |
| `trainer_checkpoint_age_seconds` | gauge | — | 1 |
| `trainer_checkpoints_total` | counter | `result` = ok\|error | 2 |
| `trainer_paused` | gauge (0/1) | — | 1 |
| `trainer_pause_reason` | gauge | `reason` = none\|jellyfin\|manual | 3 |
| `trainer_curriculum_stage` | gauge | — | 1 |
| `trainer_mechanic_total` | counter | `mechanic` ∈ catalogue fermé (≈ 12) | ≈ 12 |
| `trainer_unrecognized_sequences_total` | counter | — | 1 |
| `process_*` (RSS, CPU) | gauge/counter | — | ≈ 6 |

Total estimé : ≈ 50 séries. Catalogue `mechanic` (heuristiques, voir `docs/decisions.md` D9) : `touch`, `aerial`,
`air_dribble`, `flip_reset`, `ceiling_shot`, `wall_shot`, `double_touch`, `flick`, `bump`, `demo`, `half_flip`,
`other`.

## Envoi vers Grafana Cloud

Pas de Prometheus local : un agent (Alloy / k8s-monitoring) scrape `/metrics` et envoie à Grafana Cloud. Pour rester
dans le quota de séries gratuites, l'agent doit filtrer avec une allowlist (`includeMetrics`) limitée aux préfixes
`trainer_` et `process_`. Configuré dans le dépôt GitOps de l'infrastructure : PodMonitor `rocketsim` (`metricRelabelings` `keep` sur `trainer_.*`,
histogramme réduit à `_sum`/`_count`). `process_*` n'est pas exposé : seul `process_uptime_seconds` l'est (CPU/RAM du pod :
cAdvisor).
