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

| Threads | Steps/s | RAM max | Date |
| --- | --- | --- | --- |
| | | | |

## M2 — burn CPU vs wgpu (iGPU), phase d'apprentissage

| Backend | Temps / minibatch | Temps / itération complète | RAM |
| --- | --- | --- | --- |
| burn-flex (CPU) | | | |
| burn-wgpu (Vulkan) | | | |
