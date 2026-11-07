# Décisions

Format : numéro, date, décision, pourquoi.

**D1 — 2026-11-07 — Learner en Rust (burn, backend CPU), repli Python défini.** Préférence du propriétaire pour le
Rust pur. L'état de l'art ([`research-rust-rl.md`](research-rust-rl.md)) montre : pas de crate PPO générique, mais un
PPO Rocket League complet sur burn existe (`rlgymppo_rs`, GPL-2.0 : on s'en inspire, on n'en dépend pas) ;
rlgym-ppo (Python) lance un process par worker, coûteux en RAM sur 2 Gi. Repli vers Python si, à la fin du spike M2 :
CartPole non résolu en ~2 jours, ou pas de progression sur « toucher la balle » après ~50 M pas, ou débit inférieur.

**D2 — 2026-11-07 — Client : macroquad** (léger, GPU intégré), sauf blocage avéré en M1.

**D3 — 2026-11-07 — GPU : option mesurée en M2, jamais supposée.** burn `wgpu` (Vulkan) sur l'iGPU Intel du nœud
vs `burn-flex` CPU. Estimation non mesurée : gain surtout sur la phase d'apprentissage, collecte limitante sur 2 vCPU.
Coût : image non distroless (Mesa/Vulkan), accès `/dev/dri`. Résultat dans `docs/bench.md`.

**D4 — 2026-11-07 — Métriques Prometheus obligatoires, liste fermée** : [`metrics.md`](metrics.md). Test de plafond
des séries (< 500). Envoi par agent + allowlist, pas de Prometheus local.

**D5 — 2026-11-07 — Mécaniques (objectif O4).** Catalogue de mécaniques détectées par heuristiques sur l'état de
simulation (air dribble, flip reset, ceiling shot, wall shot, double touch…) avec compteurs ; séquences réussies non
reconnues enregistrées dans un fichier de replay léger (états par tick + entrées) pour rejeu/ralenti dans le client ;
récompense de curiosité/nouveauté : option future. **Avertissement** : une « méca » trouvée par le bot peut être une
faille propre à RocketSim (non reproductible dans le vrai jeu) ; à vérifier avant de la présenter comme telle.

**D6 — 2026-11-07 — Céder la place à une charge interactive (ex. serveur média).** Le learner interroge périodiquement
l'API des sessions du serveur (URL et clé d'API par variables d'environnement / Secret, jamais dans le dépôt), écrit
un checkpoint puis se met en pause (CPU ≈ 0) tant qu'une lecture est en cours, et reprend après N minutes sans lecture.
Métrique `trainer_paused`. En plus : PriorityClass basse, CPU request 500m, limits 2 vCPU / 2 Gi.

**D7 — 2026-11-07 — Stockage.** StorageClass `local-path` : le PV disparaît avec le PVC → copie régulière du meilleur
checkpoint hors cluster (ex. GitHub Releases), à prévoir en M3. Registry `ghcr.io/<propriétaire>`. Déploiement final
par dépôt GitOps (`apps/`), Kustomize de référence dans `deploy/` (sans domaine ni IP).

**D8 — 2026-11-07 — Meshes jamais commités** (`collision_meshes/`, `collision-meshes/`, `*.cco` dans `.gitignore`),
chemin par `ROCKETSIM_MESHES_DIR`.

**D9 — 2026-11-07 — Backend de simulation (M0).** `sim-core` s'appuie sur `rocketsim_rs =0.37.0` (RocketSim C++ 2.2.1),
comme la spec. Constats :
- Compile dans le pod Debian 12 **uniquement avec clang** (testé clang 19) : GCC 12 échoue sur
  `reinterpret_cast` en `constexpr`. Pas de CMake. Sous LMDE : `sudo apt install build-essential clang`.
- `default-features = false` (évite flatbuffers et l'appel à `rustfmt` au build).
- `-march=native` est codé en dur dans le `build.rs` du crate : construire l'image sur la machine cible, ou accepter la
  non-portabilité (à traiter en M3).
- `rocketsim::init` lève une exception C++ → abort Rust si le dossier est absent : `sim-core` valide avant d'appeler.
- Le dossier doit contenir `soccar/*.cco`.
- **Alternative trouvée** : crate `rocketsim` 0.3.5 (RocketSim v3 réécrit en **Rust pur**, 2026-11-03, MIT, Rust ≥ 1.95).
  Compile dans le pod en ~6 s sans compilateur C++ (vérifié). Jeune, parité physique non vérifiée, format de meshes à
  confirmer (le README cite des `.cmf`). L'API de `sim-core` (`Sim`, `Snapshot`) est volontairement indépendante du
  backend. À faire pour clore M0 : spike comparatif des deux backends avec les vrais meshes ; si v3 est équivalent,
  le préférer (alignement avec la préférence Rust pur, image simple).

**D10 — 2026-11-07 — Hygiène du dépôt public.** README avec avertissement « non officiel » et dumper, CI (fmt,
clippy `-D warnings`, test, avec clang), CONTRIBUTING court, aucune info personnelle ou d'infrastructure.
Versions : `rocketsim_rs` et `cxx` épinglés (`=`), autres dépendances résolues par `Cargo.lock` commité.

**D11 — 2026-11-07 — Outils de build sans root.** Environnement de travail sans `sudo` : rustup en espace utilisateur,
clang récupéré en archive LLVM. Non requis pour les contributeurs ordinaires.
