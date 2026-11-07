# Contribuer

- Branche + PR vers `main` ; jamais de push direct sur `main`.
- Avant de pousser : `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Commits atomiques, un sujet par commit.
- **Ne jamais commiter de contenu du jeu** : meshes de collision, modèles, textures, logos, noms de marque.
  `collision_meshes/` est ignoré par git ; le chemin se donne par `ROCKETSIM_MESHES_DIR`.
- Rendu : primitives originales uniquement.
- Aucun chiffre de performance sans mesure reproductible consignée dans `docs/bench.md`.
- Les tests qui exigent les meshes sont `#[ignore]` (`cargo test -- --ignored` avec la variable définie).
- Pas d'informations personnelles ni d'infrastructure privée (domaines, IP, secrets) dans le dépôt.
