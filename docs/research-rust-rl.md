# Recherche — RL en Rust pour rocketsim-trainer (état au 2026-11-07)

Sources : API crates.io, API GitHub (`gh api`), PyPI, lues le 2026-11-07. « Dernier push » = `pushed_at` GitHub.
Ce que je n'ai **pas** pu vérifier : aucun benchmark exécuté (pas de `cargo`, pas de `/dev/dri` ni de Vulkan dans
l'environnement de recherche). Les chiffres de performance de la section 3 sont des estimations.

## 0. Constat principal : RocketSim est passé en Rust natif

- **RocketSim v3 = réécriture Rust** (branche `v3-rust` de ZealanL/RocketSim), publiée sous le crate
  [`rocketsim`](https://crates.io/crates/rocketsim) **0.3.5** (crate créé le 2026-09-25, release 2026-11-03, MIT,
  Rust ≥ 1.95, édition 2024). README : « The core simulation does not need a C++ compiler ».
  Crates associés : `rocketsim_vis` / `rocketsim_viewer` 0.3.5 (visualiseur 3D optionnel).
  https://github.com/ZealanL/RocketSim/tree/v3-rust
- L'écosystème de VirxEC (auteur de rocketsim-rs) a **déjà migré** dessus : `rlgym_rs` et `rlgymppo_rs` (voir §1, §2).
- Jeune : correctifs physiques encore en cours (2026-11-03 : « Fix GJK penetration witnesses », « Fix box-triangle
  SAT »). Parité avec RocketSim C++ 2.2.x **non vérifiée** par moi.
- Conséquence pour la spec (qui vise `rocketsim_rs`) : à re-trancher en M0 (voir recommandation).

## 1. PPO / RL en Rust

| Projet | Version / date | Stars / licence | Verdict |
|---|---|---|---|
| [burn](https://github.com/tracel-ai/burn) | 0.22.1 (2026-10-20), 0.23.0-pre.1 (2026-10-29) | 16,2k / Apache-2.0+MIT | Très actif (push 2026-11-07) |
| [burn-rl](https://crates.io/crates/burn-rl) | 0.22.1, dans le dépôt burn | idem | Briques seulement (`Environment`, `Policy`, `TransitionBuffer`), **pas de PPO** ; seul exemple : DQN (`examples/dqn-agent`). Hors features par défaut depuis 0.22 (`features = ["train","rl"]`) |
| [rlgymppo_rs](https://github.com/VirxEC/rlgymppo_rs) (VirxEC) | pas sur crates.io, push 2026-10-24 | 5 / **GPL-2.0** | **PPO Rust complet pour Rocket League** sur burn 0.22 + RocketSim v3 : learner multi-thread, self-play, Elo, TUI, export burnpack, agent RLBot v5, inférence Nexto |
| candle [examples/reinforcement-learning](https://github.com/huggingface/candle/tree/main/candle-examples/examples/reinforcement-learning) | dernier changement 2025-03-26 | 21,3k / Apache-2.0 | DQN, DDPG, policy-gradient via gym Python (pyo3) ; **pas de PPO**, exemple pédagogique |
| [tch-rs](https://github.com/LaurentMazare/tch-rs) | 0.26.0 (2026-08-23) | 5,5k / Apache-2.0 | Exige libtorch 2.13 (C++, plusieurs centaines de Mo). Backend `burn-tch` **déprécié** en 0.22 et retiré de `main` |
| [border](https://github.com/laboroai/border) | 0.0.8 (2025-05) | 67 / Apache-2.0 | Peu actif, basé tch/candle : non |
| `rl_rs` | **absent de crates.io** | — | n'existe pas sous ce nom |

Lecture : pas de crate PPO générique mature ; la seule implémentation PPO sérieuse et vivante est rlgymppo_rs,
spécifique Rocket League. Hyperparamètres par défaut (README) : MLP partagée 256×2, policy et critic 256×3,
80k pas/itération, batch 40k, minibatch 10k, lr 1,5e-4, entropie 0,036.
**Attention licence** : GPL-2.0 → en dépendre rend le projet dérivé GPL ; s'en inspirer pour réécrire un PPO
(~500–800 lignes) est le chemin propre.

## 2. Environnements / bots RocketSim en Rust

- **[rocketsim_rs](https://crates.io/crates/rocketsim_rs) 0.37.0** (2026-06-11, MIT, 15 stars, 105k téléchargements).
  Bindings `cxx` vers RocketSim C++ en sous-module, commit `f80f9d9` = **RocketSim v2.2.1** (2026-04-19).
  Build : `cc` + `cxx-build`, **compilateur C++20 requis (g++ ou clang), pas de CMake**. Compile bullet3 entier
  (build long). En release : `-O3`, `-flto=thin`, **`-march=native`** (piège : binaire non portable si l'image est
  construite sur une autre machine que le nœud). Feature par défaut `bin` (flatbuffers, RLViser) qui **appelle
  `rustfmt` au build** → `default-features = false` si inutile. Autres features : `glam`, `serde_utils`,
  `debug_logging`. Meshes : `rocketsim_rs::init(None, true)` lit `./collision_meshes`, `init(Some(chemin), …)`
  sinon. Les meshes sont **exclus du crate** (à fournir).
- **[rocketsim](https://crates.io/crates/rocketsim) 0.3.5** (Rust pur, §0) : `init_from_default` lit `collision_meshes/`
  (soccar 16 .cmf, hoops 12, dropshot 2), `init(dir)` ou `init_from_mem(buffers)`. Mode `TheVoid` sans mesh
  pour les tests.
- **Meshes** : récupérés depuis le sdist PyPI `rlgym-rocket-league` 2.0.1 (`scripts/fetch-meshes.sh`) ; sinon dump via
  [RLArenaCollisionDumper](https://github.com/ZealanL/RLArenaCollisionDumper) (outil Windows). Assets du jeu : ne pas les committer (dépôt public), les monter depuis un PVC.
- **[rlgym_rs](https://github.com/VirxEC/rlgym_rs)** (crate `rlgym` 0.2.0, **non publié**, push 2026-10-21) :
  env gym sur `rocketsim` 0.3.1 (StateSetter, Obs, Action, Reward, Terminal, Truncate, SharedInfo).
  **Aucune licence déclarée** → réutilisation non permise en l'état ; s'en inspirer seulement.
- **[rlgym_sim_rs](https://github.com/JeffA233/rlgym-sim-rs)** 0.8.0-alpha (2024-07, push 2026-03, 2 stars) et
  JeffA233/RLGym-Rust (2023) : quasi abandonnés. `rlgym-rs` : absent de crates.io.
- **RLBot** : crate [`rlbot`](https://crates.io/crates/rlbot) 0.7.1 (2026-11-04, MIT, RLBot v5,
  https://github.com/RLBot/rust-interface) — pour jouer contre son bot dans le vrai jeu, hors M0–M3.
- **Necto / Nexto** : https://github.com/Rolv-Arild/Necto (306 stars, push 2024-09, projet arrêté ; Nexto ≈ GC1),
  Python/PyTorch. `rlgymppo-nexto` (dans rlgymppo_rs) fournit une **inférence Nexto en Rust** : adversaire de
  référence intéressant, mais GPL-2.0.

## 3. burn : CPU vs iGPU (Vulkan) pour une MLP ~256×3

Backends burn 0.22 : **`ndarray` déprécié** (retiré de `main`), remplacé par **`burn-flex`** (CPU pur Rust, eager,
autodiff) ; `burn-cpu` = CubeCL via LLVM (fusion, dépendance LLVM lourde, non testé) ; `burn-wgpu` = Vulkan via
CubeCL, fusion par défaut.

Estimation (non mesurée), obs ~100, actions ~90, ~200k paramètres :
- 1 minibatch de 10k ≈ 4 GFLOP forward, ~12 GFLOP forward+backward. 2 vCPU AVX2 en eager : ~20–50 GFLOP/s
  effectifs → ~0,3–0,6 s. Iris Xe (~1,5–2 TFLOP/s crête fp32 ; via wgpu plutôt 0,2–0,6 effectifs) → ~20–60 ms.
  Gain plausible **×5 à ×15 sur la phase d'apprentissage seule**.
- Sur 2 vCPU, **le goulot sera la collecte** (simulation + inférence par pas). Gain global réaliste : **×1,2 à ×2**.
- L'inférence de collecte (petits batchs) reste meilleure sur CPU (latence de lancement GPU).
- Coûts wgpu : build plus long et binaire plus gros (wgpu, naga, cubecl), compilation/autotune des kernels au
  démarrage, quelques centaines de Mo de RAM en plus (estimation) ; la mémoire iGPU est **prise sur la RAM du nœud**.
  Côté Kubernetes : exposer `/dev/dri` (Intel GPU device plugin ou hostPath + groupe `render`) et embarquer
  Mesa/ANV + loader Vulkan → **incompatible avec une image distroless**, il faut debian-slim.
- candle CPU : bien pour l'inférence, moins outillé (optimiseurs, entraînement) pour un learner ; pas d'avantage ici.

Conclusion §3 : démarrer en **burn-flex (CPU)**, code générique sur le backend, mesurer wgpu plus tard.

## 4. Format d'export pour `policy-runtime`

- **burnpack** (natif burn 0.22, `burn-store`, mmap) : zéro effort côté learner, mais impose burn dans le jeu.
- **safetensors** (crate `safetensors` 0.8.0 ; `burn-store` a `SafetensorsStore`) : standard, lisible aussi depuis
  Python/PyTorch (repli), aucune dépendance burn côté runtime.
- **ONNX + tract** (`tract-onnx` 0.23.10, 2026-11-05 ; export via `burn-onnx` 0.22) : surdimensionné pour une MLP.
- **MLP codée à la main** : ~0,4 MFLOP par inférence → dizaines de µs, très en dessous du budget 120 Hz.
- **Recommandation** : `policy-runtime` = MLP à la main (matmul + ReLU + argmax, ~150 lignes, sans dépendance
  lourde) chargeant un **safetensors** + un JSON de métadonnées (couches, normalisation des obs, table d'actions,
  version de l'obs builder). Test d'équivalence learner ↔ runtime à 1e-5 près.

## 5. Recommandation : Rust ou Python (rlgym-ppo) ?

**Rust, avec repli défini.**
- 2 vCPU / 2 Gi : rlgym-ppo lance un process Python par worker (chacun avec RocketSim + meshes) + PyTorch CPU
  → la RAM devient limitante. En Rust : un process, des threads, meshes partagés.
- Faisabilité prouvée : rlgymppo_rs fait un PPO Rocket League complet sur burn 0.22 (référence de design et
  d'hyperparamètres, à ne pas copier pour la licence).
- Python vieillit : rlgym-ppo absent de PyPI, AechPro/rlgym-ppo inactif depuis 2025-01 ; `rlgym` 2.0.1 (2025-04) ;
  seul `rocketsim` Python 2.2.1 (2026-04) est récent. La communauté avancée est passée au C++ (GigaLearnCPP,
  RLGymPPO_CPP) ou au Rust (VirxEC).

Sim à re-trancher en M0 : `rocketsim` 0.3.x (Rust pur, pas de C++, image simple, actif) **vs** `rocketsim_rs` 0.37
(C++ 2.2.1 éprouvé). Proposition : spike M0 sur les deux (1 800 ticks, temps de build, ticks/s sur 1 vCPU) ;
garder `rocketsim_rs` si v3 montre écarts physiques ou plantages.

Risques : PPO maison (bugs silencieux GAE / normalisation / masques → valider sur CartPole porté, comparer à
rlgymppo_rs) ; burn bouge vite (ndarray et tch dépréciés en 0.22 → épingler la version) ; RocketSim v3 jeune ;
`-march=native` de rocketsim_rs ; meshes non redistribuables ; Vulkan en pod non garanti.

**Critère de repli vers Python (rlgym-ppo + rocketsim Python)**, fin du spike M2 :
1. le PPO Rust ne résout pas CartPole (retour ≥ 475) en ~2 jours de travail, **ou**
2. scénario simple (toucher la balle) : pas de hausse nette de la récompense après ~50 M pas alors qu'une config
   rlgym-ppo équivalente progresse, **ou**
3. débit Rust < débit rlgym-ppo sur le même pod.
Avec safetensors + JSON, `policy-runtime` reste valable quel que soit le learner.

## URLs principales

- https://github.com/ZealanL/RocketSim/tree/v3-rust — https://crates.io/crates/rocketsim
- https://github.com/VirxEC/rocketsim-rs — https://crates.io/crates/rocketsim_rs
- https://github.com/VirxEC/rlgym_rs — https://github.com/VirxEC/rlgymppo_rs
- https://github.com/tracel-ai/burn — https://crates.io/crates/burn-flex — https://crates.io/crates/burn-rl
- https://github.com/huggingface/candle/tree/main/candle-examples/examples/reinforcement-learning
- https://github.com/LaurentMazare/tch-rs — https://github.com/sonos/tract — https://crates.io/crates/safetensors
- https://github.com/AechPro/rlgym-ppo — https://github.com/RLGym/rlgym — https://pypi.org/project/rocketsim/
- https://github.com/ZealanL/RLArenaCollisionDumper — https://github.com/RLBot/rust-interface — https://github.com/Rolv-Arild/Necto
