# Spec — Simulateur de mécas + bot IA (RocketSim / Rust)

Nov 7, 2026 · @BambOula

## Contexte et objectifs

On construit en Rust un simulateur d'entraînement aux mécas basé sur [RocketSim](https://github.com/ZealanL/RocketSim), plus un pipeline d'entraînement d'IA qui tourne H24 sur le homelab. Aucun reverse engineering du jeu : RocketSim fournit déjà la physique, via les bindings [rocketsim-rs](https://github.com/VirxEC/rocketsim-rs).

- **O1 — Jouer** : s'entraîner à la manette sur le laptop LMDE, hors ligne (dans le train), avec peu de RAM et de CPU.
- **O2 — Entraîner** : faire tourner un bot par apprentissage par renforcement en continu sur le cluster Kubernetes, avec 1–2 Go de RAM et 1–2 vCPU.
- **O3 — Expérimenter** : mesurer jusqu'où le bot monte en mécanique, dans la simu puis dans le vrai jeu, en hors ligne uniquement.
- **O4 — Mécaniques** : détecter un catalogue de mécas connues par heuristiques, avec compteurs ; enregistrer les séquences réussies non reconnues pour rejeu/ralenti ; option future de récompense de curiosité (voir D5).

## Non-objectifs et contraintes

Le projet n'utilise et ne redistribue aucun contenu du jeu, et ne touche jamais au jeu en ligne.

- **Pas de reverse engineering** du binaire Rocket League.
- **Assets** : les 16 maillages de collision soccar (`.cmf`) sont téléchargés par `scripts/fetch-meshes.sh` depuis le paquet PyPI public `rlgym-rocket-league` 2.0.1 (empreintes sha256 vérifiées). Ils ne sont jamais commités, ni intégrés à une image : ils vivent dans `collision_meshes/` (ignoré par le VCS) ou sur un volume monté. Leur géométrie reste la propriété de leur ayant droit : usage local et personnel uniquement.
- **Rendu original** : des primitives simples (boîtes, sphères, lignes). Pas de modèles, textures, logos ni noms de marque du jeu.
- **Hors ligne uniquement** : le bot ne joue dans le vrai jeu que via RLBot, en match privé ou hors ligne. Jamais en ligne ni en ranked.
- **Pas de GPU requis** (l'iGPU du nœud est une option à mesurer en M2, voir D3) et pas de multijoueur réseau en v1.
- **Ressources limitées** : le pod d'entraînement ne doit jamais gêner les autres applis du cluster.

## Architecture

Un seul workspace Cargo partage la simu et les scénarios entre le client de jeu et l'entraînement. Flux : `learner` écrit des checkpoints sur un PVC → export de la politique → `game` la charge comme adversaire.

| Crate | Rôle | Dépendances clés |
| --- | --- | --- |
| `sim-core` | Wrapper autour de rocketsim\_rs : chargement des meshes, création d'arène, état sérialisable, reset d'état | `rocketsim_rs`, `serde` |
| `scenarios` | Drills décrits en fichiers (TOML/RON) : position et vitesse de la balle et de la voiture, conditions de réussite | `serde` |
| `game` | Client jouable : rendu, manette, HUD | `macroquad` ou `bevy` (à trancher), `gilrs` |
| `rl-env` | Env RL style RLGym : observations, actions, rewards, conditions de fin | `sim-core`, éventuellement `rlgym_sim_rs` |
| `learner` | Boucle PPO, checkpoints, métriques | à trancher (voir questions ouvertes) |
| `policy-runtime` | Inférence légère de la politique, partagée par `game` et `learner` | à trancher |

## Client de jeu (laptop)

Le client fait tourner la physique à 120 Hz fixe et un rendu découplé, sur LMDE, sans réseau. Cible à valider : moins de 500 Mo de RAM et 60 FPS sur GPU intégré.

- **Manette** via `gilrs`, avec un mapping configurable dans un fichier.
- **Caméras** : ball-cam et caméra voiture, avec bascule par bouton.
- **Mode libre** : terrain complet, balle, une voiture.
- **Drills** : chargement d'un scénario depuis `scenarios`, reset instantané par bouton et compteur de réussites.
- **Outils d'entraînement** : ralenti (0,25× à 1×) et rejeu des 10 dernières secondes.
- **HUD minimal** : boost, vitesse, temps du drill.
- **Adversaire IA** (jalon M4) : une politique exportée est chargée via `policy-runtime`.
- **Visuels** : uniquement des primitives originales, sans aucun asset du jeu.

## Entraînement H24 (cluster)

Un seul pod lent mais continu, qui reprend toujours depuis son dernier checkpoint. La métrique clé est le nombre de steps par seconde sur 2 vCPU : elle est à mesurer en M2, sans chiffre supposé à l'avance.

- **Algorithme** : PPO, avec une petite politique MLP (CPU only).
- **Curriculum** : aller vers la balle, toucher la balle, marquer, puis des drills de mécas issus de `scenarios`.
- **Reprise** : checkpoint toutes les N minutes sur le PVC. Un kill ou un reboot du nœud ne perd jamais plus de N minutes.
- **Observabilité** : logs structurés (steps/s, reward moyen, longueur d'épisode) et endpoint `/metrics` Prometheus en option.
- **Export** : chaque checkpoint « meilleur » est exporté dans le format lu par `policy-runtime`.

## Ressources et déploiement Kubernetes

Le learner tient dans 2 Gi et 2 vCPU au maximum, avec une priorité basse pour céder la place aux autres applis.

| Ressource | Requests | Limits |
| --- | --- | --- |
| CPU | 500m | 2 |
| RAM | 1 Gi | 2 Gi |
| PVC checkpoints | 5 Gi (à ajuster) | — |

- **Workload** : un `Deployment` à 1 réplique, stratégie `Recreate`, avec une `PriorityClass` basse dédiée.
- **Image** : build multi-stage avec un binaire Rust release, sur une base `debian-slim` ou distroless. Utilisateur non-root et système de fichiers en lecture seule, hors PVC.
- **Meshes** : montés depuis un PVC ou un `hostPath`, jamais dans l'image.
- **Manifests** : Kustomize dans `deploy/`, plus un `Makefile` ou `justfile` (build, push, deploy, logs).

## Jalons, risques et questions ouvertes

Six jalons. Chacun se termine par une validation manuelle avant de passer au suivant.

1. **M0 — Spike** : rocketsim\_rs compile sur LMDE, charge les meshes et simule 1 800 ticks sans erreur.
2. **M1 — Jouable** : le client affiche le terrain, la manette pilote la voiture, et le reset fonctionne.
3. **M2 — Env RL** : `rl-env` tourne en headless, avec un benchmark de steps/s sur 2 vCPU.
4. **M3 — H24** : PPO, checkpoints et reprise, déployé sur le cluster. Le reward moyen monte sur le drill « toucher la balle ».
5. **M4 — Drills et bot** : scénarios jouables dans le client, avec la politique entraînée comme adversaire.
6. **M5 — Vrai jeu** : le bot est exporté vers RLBot et testé hors ligne.

**Risques**

- Les meshes viennent d'un paquet tiers (PyPI) : si la version est retirée, repli sur [Ripple-RLBot](https://github.com/Rolv-Arild/Ripple-RLBot) (`src/collision_meshes/soccar/`). Statut de licence de ces fichiers non garanti.
- `rlgym_sim_rs` est en alpha : à évaluer, sans en dépendre.
- RocketSim dérive du vrai jeu au-delà d'environ une seconde, ce qui pèse sur les mécas longues.
- Le débit peut être trop faible sur 2 vCPU pour des milliards de steps.

**Questions ouvertes**

- [x] Learner : Rust (burn) de préférence, repli Python — voir Décisions, D1. Spike M2 pour confirmer.
- [x] Client : macroquad (D2).
- [x] Format d'export : MLP manuelle + safetensors + JSON de métadonnées (recommandé par la recherche, à confirmer en M2).
- [x] Registry ghcr.io, StorageClass local-path (D7).
- [x] Pas de Prometheus local : agent + Grafana Cloud (D4).

## Décisions (2026-11-07)

Détail et justifications : [`../decisions.md`](../decisions.md). Recherche : [`../research-rust-rl.md`](../research-rust-rl.md).

- **Learner** : Rust pur de préférence (burn, CPU) ; repli Python (rlgym-ppo) si le spike M2 échoue (critères D1).
- **Client** : macroquad, sauf blocage avéré.
- **GPU** : iGPU Intel du nœud (déjà partagé avec un serveur média) ; burn wgpu vs CPU à mesurer en M2, jamais supposé.
- **Métriques Prometheus** obligatoires (`/metrics`), liste fermée ([`../metrics.md`](../metrics.md)), < 500 séries
  (idéalement < 100), test de plafond. Pas de Prometheus local : un agent scrape et envoie vers Grafana Cloud avec allowlist.
- **O4 Mécaniques** : catalogue + compteurs, replays de séquences non reconnues, curiosité en option. Attention : une
  « méca » peut être une faille de RocketSim.
- **Céder la place** à une lecture vidéo (API des sessions du serveur média) : checkpoint puis pause, reprise après N min ;
  `trainer_paused`. PriorityClass basse, CPU request 500m, limits 2 vCPU / 2 Gi (remplace le tableau ci-dessus).
- **Stockage** : `local-path` (PV supprimé avec le PVC) → copie du meilleur checkpoint hors cluster (M3). Registry
  `ghcr.io/<propriétaire>`. Déploiement via dépôt GitOps, Kustomize de référence dans `deploy/`.
- **Meshes** : jamais commités, chemin par `ROCKETSIM_MESHES_DIR`.
- **Backend sim** : `rocketsim_rs` 0.37.0 (clang requis) ; crate Rust pur `rocketsim` 0.3.5 à comparer en M0 (D9).

## Prompt pour Claude Code

Exporte d'abord cette spec en Markdown, puis dépose-la dans le repo sous `docs/specs/rocketsim-trainer.md`. Colle ensuite ce prompt à Claude Code.

```markdown
Tu vas construire le projet décrit dans docs/specs/rocketsim-trainer.md (lis-la en entier avant toute action).

Méthode (skills superpowers si installés : brainstorming, writing-plans, executing-plans, test-driven-development) :
1. Relis la spec. Pour chaque question ouverte qui bloque le jalon en cours, pose-moi UNE question à la fois.
2. Écris le plan d'implémentation dans docs/plans/, découpé par jalon (M0 à M5), en tâches de 2 à 5 minutes.
3. Exécute UNIQUEMENT le jalon M0, en TDD, avec un commit par tâche. Arrête-toi ensuite et présente-moi le résultat et les critères d'acceptation cochés.

Règles non négociables :
- Aucun asset du jeu dans le repo ni dans une image : les meshes de collision sont montés depuis un chemin configurable (variable d'environnement), et collision_meshes/ est ajouté au .gitignore.
- Rendu en primitives originales uniquement.
- Ne suppose jamais l'API d'une crate : lis sa doc sur docs.rs (page complète, version exacte) avant de l'utiliser, et épingle les versions dans Cargo.toml.
- N'invente aucun chiffre de performance : mesure-le avec un benchmark et note-le dans docs/bench.md.
- Workload K8s : limits de 2 vCPU et 2 Gi max, PriorityClass basse, non-root, reprise depuis checkpoint.
- Si une hypothèse de la spec s'avère fausse, arrête-toi et dis-le-moi au lieu de la contourner.
```
