# Déploiement (cluster)

Image : `ghcr.io/elohyrr/rocketsim-trainer` (workflow `.github/workflows/image.yml`). Tags : `sha-<court>` (main et tags `v*`),
`X.Y.Z` et `X.Y` (tag `v*`), `latest` (main). Sur une PR l'image est construite sans être publiée.

- Build : Debian 12, clang-16, `-C target-cpu=x86-64-v2` et `-march=x86-64-v2` (sinon SIGILL, `rocketsim_rs` compile en
  `-march=native`). Seul le binaire `learner` est construit (pas `game`).
- Exécution : `debian:12-slim` + `libstdc++6`, `curl`, `ca-certificates`, utilisateur 10001 non-root. Écrit uniquement dans
  `/data` (PVC) et `/tmp` (emptyDir) : le système de fichiers peut être en lecture seule.
- **Meshes** (jamais dans l'image) : un initContainer de la même image lance
  `/opt/trainer/scripts/fetch-meshes.sh /data/collision_meshes` (même source PyPI et mêmes sha256 que `scripts/fetch-meshes.sh`) ;
  sans effet si les 16 fichiers sont déjà présents et valides.
- Commande par défaut : `learner --minutes 0 --resume --out /data/run --metrics 0.0.0.0:9090` (sans limite, reprise du dernier
  checkpoint, `/metrics` et `/healthz` sur 9090). Checkpoint toutes les 5 min (`--checkpoint-minutes`), à chaque
  promotion de palier, avant une pause et sur SIGTERM.
- **Pause Jellyfin (D6)** : `JELLYFIN_URL` (http, Service interne, ex. `http://jellyfin.media.svc:8096`) et `JELLYFIN_API_KEY`.
  Sans clé (ou clé vide) la pause est désactivée. Sondage de `GET /Sessions` toutes les `JELLYFIN_POLL_SECONDS` (30) avec
  `Authorization: MediaBrowser Token="…"` ; une session qui lit une vidéo (non en pause) déclenche checkpoint + pause
  (CPU ≈ 0), reprise `JELLYFIN_RESUME_MINUTES` (10) après la dernière lecture. API injoignable = pas de pause.
  Métriques `trainer_paused`, `trainer_pause_reason{reason="jellyfin"}`.
- Suivi : `kubectl -n rocketsim logs deploy/rocketsim-trainer -f` (une ligne par itération : palier, steps/s, reward,
  taux de toucher) et le dashboard Grafana `homelab-rocketsim` (dépôt GitOps).
- Politique exportée : `/data/run/policy.safetensors` (+ `policy.json`) à chaque checkpoint. Le PV `local-path` disparaît avec le
  PVC : la copie hors cluster (GitHub Releases, D7) reste à faire.
