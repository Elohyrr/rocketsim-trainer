# Métriques Prometheus (`/metrics`)

Liste **fermée** : toute nouvelle métrique passe par une PR qui met à jour ce fichier **et** le test de plafond.
Règles : labels à valeurs bornées (énumérations fixes), jamais d'identifiant d'épisode / de run, histogrammes
à ≤ 8 buckets. Objectif : ≲ 100 séries (110 avec les drills, le boost et la vitesse de frappe ; le test exige < 115), plafond dur 500 (un test du crate `learner` échoue au-delà).

| Métrique | Type | Labels (valeurs) | Séries |
| --- | --- | --- | --- |
| `trainer_steps_total` | counter | — | 1 |
| `trainer_steps_per_second` | gauge | — | 1 |
| `trainer_episodes_total` | counter | — | 1 |
| `trainer_reward_mean` | gauge | — | 1 |
| `trainer_episode_length_mean` | gauge | — | 1 |
| `trainer_policy_loss`, `trainer_value_loss`, `trainer_entropy`, `trainer_kl` | gauge | — | 4 |
| `trainer_entropy_target`, `trainer_entropy_coef`, `trainer_learning_rate`, `trainer_entropy_guard_scale` (régulateur d'exploration et garde-fou, `explore.rs`) | gauge | — | 4 |
| `trainer_entropy_guard_backoffs_total` (déclenchements du garde-fou depuis le démarrage) | counter | — | 1 |
| `trainer_iteration_seconds` | histogram (≤ 8 buckets) | — | ≈ 10 |
| `trainer_checkpoint_age_seconds` | gauge | — | 1 |
| `trainer_checkpoints_total` | counter | `result` = ok\|error | 2 |
| `trainer_paused` | gauge (0/1) | — | 1 |
| `trainer_pause_reason` | gauge | `reason` = none\|jellyfin\|manual | 3 |
| `trainer_curriculum_stage` | gauge | — | 1 |
| `trainer_goals_total` | counter | `side` = scored\|conceded (conceded = contre son camp / encaissé) | 2 |
| `trainer_goal_rate` | gauge (0–1, fenêtre glissante du curriculum) | — | 1 |
| `trainer_kickoffs_total` | counter | `outcome` = won\|lost\|neutral (coups d'envoi des paliers 5 et 6) | 3 |
| `trainer_kickoff_first_touch_seconds` | gauge (temps moyen jusqu'au premier toucher de l'agent, dernière itération avec toucher) | — | 1 |
| `trainer_kickoff_touch_speed` | gauge (vitesse moyenne au contact, uu/s) | — | 1 |
| `trainer_mechanic_total` | counter | `mechanic` ∈ catalogue fermé (12), `context` = match\|drill | 24 |
| `trainer_drill_episodes_total` | counter | `drill` ∈ {roof_still, roof_moving, aerial_touch, air_dribble, wall_ball, flip_reset, recovery} | 7 |
| `trainer_drill_recovery_total` | counter | `outcome` ∈ {good, wheels, bad, none} | 4 |
| `trainer_drill_bonus_total` | counter | `mechanic` (les 8 qui peuvent recevoir un bonus) | 8 |
| `trainer_boost_pickups_total` (boosts ramassés par l'agent en match : le ramassage a fait monter son boost) | counter | `size` = small\|big | 2 |
| `trainer_boost_mean` (niveau moyen 0–100) , `trainer_boost_empty_fraction` (part des décisions à 0 de boost) ; dernière itération avec des décisions de match | gauge | — | 2 |
| `trainer_hit_speed_to_goal_mean` (vitesse de balle vers le but adverse juste après un toucher de l'agent en match, dernière itération avec des touchers), `trainer_hit_speed_to_goal_record` (record depuis le démarrage, uu/s) | gauge | — | 2 |
| `trainer_unrecognized_sequences_total` | counter | — | 1 |
| `trainer_elo`, `trainer_elo_best`, `trainer_elo_age_seconds` (self-play, `docs/self-play.md`) | gauge | — | 3 |
| `trainer_elo_matches_total` | counter | — | 1 |
| `trainer_elo_vs`, `trainer_elo_winrate` | gauge | `ref` = charger\|stage5\|best | 6 |
| `trainer_pool_size` | gauge | — | 1 |
| `trainer_pool_snapshots_total` | counter | — | 1 |
| `trainer_selfplay_goals_total` | counter | `opponent` = current\|pool, `side` = scored\|conceded | 4 |
| `trainer_stage_changes_total` | counter | `direction` = up\|down | 2 |
| `process_*` (RSS, CPU) | gauge/counter | — | ≈ 6 |

Total estimé : ≈ 111 séries (+4 : boost). Taux de kickoffs gagnés : `rate(trainer_kickoffs_total{outcome="won"}[15m]) / sum(rate(trainer_kickoffs_total[15m]))`. Catalogue `mechanic` (détecteur par tick, `docs/mechanics.md`) : `touch`, `aerial_touch`, `air_dribble`, `flip_reset`,
`ceiling_shot`, `wall_shot`, `double_touch`, `flick`, `dribble`, `speed_flip`, `wave_dash`, `half_flip`. La série `drill` compte les mécaniques vues
pendant les épisodes de drill (`docs/drills.md`) ; elle reste à 0 tant que `--drill-prob` vaut 0.

## Envoi vers Grafana Cloud

Pas de Prometheus local : un agent (Alloy / k8s-monitoring) scrape `/metrics` et envoie à Grafana Cloud. Pour rester
dans le quota de séries gratuites, l'agent doit filtrer avec une allowlist (`includeMetrics`) limitée aux préfixes
`trainer_` et `process_`. Configuré dans le dépôt GitOps de l'infrastructure : PodMonitor `rocketsim` (`metricRelabelings` `keep` sur `trainer_.*`,
histogramme réduit à `_sum`/`_count`). `process_*` n'est pas exposé : seul `process_uptime_seconds` l'est (CPU/RAM du pod :
cAdvisor).
