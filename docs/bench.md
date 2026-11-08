# Benchmarks

Aucun chiffre n'est consigné tant qu'il n'est pas mesuré. Chaque mesure indique : commit, machine / limites
du pod, commande exacte, date.

## Environnement de mesure

| Champ | Valeur |
| --- | --- |
| Commit | |
| Machine / pod (vCPU, RAM) | |
| Compilateur, version Rust | |
| Backend sim (`rocketsim_rs` / `rocketsim` v3) | |

## M0 — simulation brute (ticks/s, 1 vCPU)

Commande : `ROCKETSIM_MESHES_DIR=… cargo run --release -p sim-core --example m0-spike`

| Backend | 1 voiture | 2 voitures | Date |
| --- | --- | --- | --- |
| | | | |

## M0 — temps de build et taille

| Backend | Build à froid | Taille du binaire release |
| --- | --- | --- |
| | | |

## M2 — env RL (steps/s sur 2 vCPU)

Commande : `taskset -c 0,1 target/release/examples/bench <threads> 16 15` (16 envs, politique aléatoire, 1 step = 8 ticks).
Pod 8 cœurs limités à 2 par `taskset`, Rust 1.99, `-C target-cpu=x86-64-v2`, C++ via zig 0.17 (`-fno-sanitize=undefined`).
Commit : branche `claude/m2-env`.

| Threads | Steps/s | Ticks/s | RAM max | Date |
| --- | --- | --- | --- | --- |
| 1 | 43 274 | 346 191 | non mesurée | 2026-11-08 |
| 2 | 50 761 | 406 092 | non mesurée | 2026-11-08 |

Le 2e thread apporte peu (+17 %) : les 2 cœurs du `taskset` sont probablement 2 threads d'un même cœur physique.

## M2 — spike learner PPO (burn-flex, CPU, 2 cœurs)

Commande : `taskset -c 0,1 target/release/learner --minutes 5 --threads 2 --out runs/spike` (MLP, 16 envs, « toucher la balle »,
reward = touche + 0,1 × progression vers la balle). Une itération = 2048 pas : ~0,06 s de collecte + ~0,22 s d'entraînement.

| Pas cumulés | Reward moyen | Longueur d'épisode | Taux de touche | Entropie |
| --- | --- | --- | --- | --- |
| 4 096 | 0,9 | 150 (max) | 0 % | 4,34 |
| 400 000 | 1,9 | 150 | 8 % | 2,03 |
| 612 000 | 3,6 | 128 | 38 % | 1,67 |
| 817 000 | 3,2 | 77 | 89 % | 1,35 |
| 2 023 000 | 3,6–4,4 | 61–73 | ~94 % | 1,2 |

Débit global ~7 000 pas/s (entraînement compris), RAM non mesurée. Résultat : le reward monte et la balle est touchée
dans ~94 % des épisodes après ~2 M pas (5 min).

## M2 — burn CPU vs wgpu (iGPU), phase d'apprentissage

| Backend | Temps / minibatch | Temps / itération complète | RAM |
| --- | --- | --- | --- |
| burn-flex (CPU) | | | |
| burn-wgpu (Vulkan) | | | |
