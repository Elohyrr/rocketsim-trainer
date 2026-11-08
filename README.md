# rocketsim-trainer

Simulateur d'entraînement aux mécaniques de jeu de voiture-football et pipeline d'entraînement d'un bot par
apprentissage par renforcement, en Rust, basés sur [RocketSim](https://github.com/ZealanL/RocketSim).

> **Projet non officiel.** Ce dépôt n'est affilié ni à Psyonix ni à Epic Games, et n'utilise aucun asset, modèle,
> texture, logo ou nom de marque du jeu. Le rendu n'emploie que des primitives (boîtes, sphères, lignes).
> Utilisation **hors ligne uniquement** ; le bot ne joue jamais en ligne ni en classé.

## État

Jalon **M1** (jouable) : client `game` (macroquad), clavier/souris + manette. Avant : M0 (socle, `sim-core`). Voir [`docs/plans/`](docs/plans/)
et la [spécification](docs/specs/rocketsim-trainer.md). Décisions : [`docs/decisions.md`](docs/decisions.md).

## Crates

`sim-core` (simulation) · `scenarios` (drills) · `game` (client jouable) · `rl-env` (env RL) · `learner` (PPO)
· `policy-runtime` (inférence légère).

## Jouer (Linux x86_64)

```sh
curl -LO https://github.com/Elohyrr/rocketsim-trainer/releases/latest/download/rocketsim-trainer-linux-x86_64
chmod +x rocketsim-trainer-linux-x86_64
./rocketsim-trainer-linux-x86_64
```

Au premier lancement, le jeu télécharge et vérifie lui-même les meshes de collision (voir plus bas) dans
`~/.local/share/rocketsim-trainer/` (besoin de `bash`, `curl`, `tar`, Internet une seule fois), puis reste hors ligne.
Prérequis : un GPU/driver OpenGL 3.3 (iGPU suffisant) et X11 ou XWayland. Le jeu est jouable au **clavier + souris**
ou à la **manette** (branchée avant ou pendant la partie).

| Action | Clavier / souris |
| --- | --- |
| Accélérer / freiner | `W` `S` (ou flèches) |
| Direction (sol), lacet (air) | `A` `D` |
| Saut, double saut / flip | `Espace` (2e appui + direction) |
| Boost | clic gauche, `Ctrl` |
| Drift (powerslide) | `Maj`, clic droit |
| Roulis aérien | `Maj`/clic droit + `A` `D`, ou `Q` `E` |
| Remise en jeu instantanée | `R` |
| Caméra balle / voiture | `C` ou `Tab` |
| Ralenti 0,25× – 1× | `[` `]` |
| Drill « tir au but » (on/off) | `F2` |

### Manette PlayStation (DualShock 4 / DualSense, USB ou Bluetooth)

Aucune configuration : les défauts sont le layout d’Elohyrr. Le stick gauche dirige (sol) et pilote tangage et
lacet (air). Le nom de la manette détectée s'affiche en haut à gauche du HUD (« aucune manette » sinon).

| Action | Bouton | Nom gilrs |
| --- | --- | --- |
| Accélérer | `R2` | `RightTrigger2` |
| Freiner / reculer | `L2` | `LeftTrigger2` |
| Saut, double saut / flip | `Croix` (`Cross`) | `South` |
| Boost | `R1` | `RightTrigger` |
| Drift (powerslide) | `L1` | `LeftTrigger` |
| Roulis libre (en l'air) | `L1` + stick gauche | `LeftTrigger` |
| Roulis gauche | `Carré` (`Square`) | `West` |
| Roulis droit | `L3` (clic du stick gauche) | `LeftThumb` |
| Caméra balle / voiture | `Triangle` | `North` |
| Remise en jeu | `Share` (DS4) / `Create` (DualSense) | `Select` |

`L1` sert à la fois au drift et au roulis libre : un même bouton peut être donné à plusieurs actions.
Pour changer une touche, éditer la section `[gamepad]` du fichier de config avec les noms PlayStation
(`Cross`, `Circle`, `Square`, `Triangle`, `L1`, `R1`, `L2`, `R2`, `L3`, `R3`, `Share`/`Create`, `Options`, `PS`)
ou les noms gilrs. Exemple : `jump = ["Circle"]`.

#### Dépannage manette (Linux)

- DualShock 4 et DualSense filaires sont reconnus par gilrs (mappings SDL intégrés) grâce aux pilotes noyau
  `hid-sony` / `hid-playstation`, sans rien installer. Le pavé tactile et les capteurs de mouvement, que le noyau
  expose comme périphériques séparés, sont ignorés.
- Manette non détectée : brancher avec un **câble de données** (certains câbles ne font que charger), puis
  appuyer sur la touche **PS** : la manette qui envoie un bouton devient la manette active.
- Vérifier que le noyau la voit : `ls -l /dev/input/event*`. Ces fichiers doivent être lisibles par l'utilisateur :
  sous LMDE/Debian ils sont du groupe `input` (et l'accès de la session locale est normalement donné par udev).
  Sinon : `sudo usermod -aG input $USER`, puis se déconnecter/reconnecter.
- Mise à jour des défauts : un `config.toml` généré automatiquement et jamais modifié côté manette est régénéré
  tout seul (ancienne copie dans `config.toml.bak`) ; une manette personnalisée est conservée et le terminal
  affiche un avertissement : supprimer la section `[gamepad]` pour obtenir les nouveaux défauts.

Un but fait gagner un point et remet en jeu après 2,5 s. Les touches sont dans
`~/.config/rocketsim-trainer/config.toml` (créé au premier lancement, noms de touches documentés dedans et dans
`crates/game/src/config.rs`) ; l'option `mouse_air_control = true` fait piloter tangage et lacet en l'air à la souris.
![Rendu M1 (primitives originales)](docs/m1-jouable.png)

Capture d'écran automatique : `rocketsim-trainer --screenshot out.png --frames 120 --demo`.

## Meshes de collision

RocketSim a besoin des 16 maillages de collision soccar (`mesh_0.cmf` … `mesh_15.cmf`). Ils **ne sont pas dans ce
dépôt** et ne doivent jamais y être commités (`collision_meshes/` est ignoré). Un script les télécharge depuis
le sdist PyPI public `rlgym-rocket-league` 2.0.1, n'en extrait que ces 16 fichiers et vérifie les sha256 (archive,
puis chaque fichier, empreintes dans `scripts/meshes.sha256`) ; il est idempotent et échoue si une empreinte diffère :

```sh
scripts/fetch-meshes.sh                             # -> ./collision_meshes/soccar/mesh_*.cmf
export ROCKETSIM_MESHES_DIR=$PWD/collision_meshes   # défaut si absent : ./collision_meshes
```

**Réserve de licence** : la géométrie des meshes provient du jeu (Psyonix / Epic) ; ce dépôt ne la redistribue pas
et le script ne fait que la récupérer sur un paquet tiers public, dont le statut de licence n'est pas garanti.
Usage local et personnel uniquement ; vérifiez les conditions du jeu. Repli si PyPI change :
[Ripple-RLBot](https://github.com/Rolv-Arild/Ripple-RLBot) (`src/collision_meshes/soccar/`).

## Compiler

Rust stable (voir `rust-toolchain.toml`) et un compilateur **clang** (C++20) : RocketSim C++ ne compile pas avec
GCC 12. Pas de CMake requis.

```sh
# Debian / LMDE (libudev-dev : manette)
sudo apt install build-essential clang pkg-config libudev-dev
export CC=clang CXX=clang++
cargo build --workspace
cargo test --workspace
cargo run --release -p game      # le jeu (télécharge les meshes si absents)
```

Release : pousser un tag `v*` déclenche `.github/workflows/release.yml` (binaire Linux x86_64, `-march=x86-64-v2`,
sans meshes, construit sous Debian 12 pour une glibc ancienne).

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
