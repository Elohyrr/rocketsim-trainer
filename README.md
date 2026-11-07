# rocketsim-trainer

Simulateur d'entraînement aux mécaniques de jeu de voiture-football et pipeline d'entraînement d'un bot par
apprentissage par renforcement, en Rust, basés sur [RocketSim](https://github.com/ZealanL/RocketSim).

> **Projet non officiel.** Ce dépôt n'est affilié ni à Psyonix ni à Epic Games, et n'utilise aucun asset, modèle,
> texture, logo ou nom de marque du jeu. Le rendu n'emploie que des primitives (boîtes, sphères, lignes).
> Utilisation **hors ligne uniquement** ; le bot ne joue jamais en ligne ni en classé.

## État

Jalon **M0** (socle) : workspace Cargo, wrapper `sim-core`, spike de simulation. Voir [`docs/plans/`](docs/plans/)
et la [spécification](docs/specs/rocketsim-trainer.md). Décisions : [`docs/decisions.md`](docs/decisions.md).

## Crates

`sim-core` (simulation) · `scenarios` (drills) · `game` (client jouable) · `rl-env` (env RL) · `learner` (PPO)
· `policy-runtime` (inférence légère).

## Meshes de collision (à fournir vous-même)

RocketSim a besoin des maillages de collision de l'arène, **qui ne sont pas fournis** ici. Ils s'extraient de votre
propre copie du jeu avec [RLArenaCollisionDumper](https://github.com/ZealanL/RLArenaCollisionDumper) (outil Windows ;
vérifiez ses conditions d'usage et celles du jeu). Ne les commitez jamais : `collision_meshes/` est ignoré par git.

Attendu : `<dossier>/soccar/*.cco`. Indiquez le dossier par variable d'environnement :

```sh
export ROCKETSIM_MESHES_DIR=/chemin/vers/collision_meshes
```

## Compiler

Rust stable (voir `rust-toolchain.toml`) et un compilateur **clang** (C++20) : RocketSim C++ ne compile pas avec
GCC 12. Pas de CMake requis.

```sh
# Debian / LMDE
sudo apt install build-essential clang pkg-config
export CC=clang CXX=clang++
cargo build --workspace
cargo test --workspace
```

## Spike M0 (1 800 ticks = 15 s à 120 Hz)

```sh
ROCKETSIM_MESHES_DIR=/chemin/vers/collision_meshes \
  cargo run --release -p sim-core --example m0-spike
# test d'intégration équivalent
ROCKETSIM_MESHES_DIR=… cargo test -p sim-core -- --ignored
```

Sans la variable, l'exemple affiche un message et sort avec le code 2 ; les tests qui exigent les meshes sont ignorés.

## Licence

MIT, voir [`LICENSE`](LICENSE). RocketSim est sous licence MIT (ZealanL).
