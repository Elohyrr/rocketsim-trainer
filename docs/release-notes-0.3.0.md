# rst 0.3.0 — notes de release (à reprendre dans la release GitHub du tag v0.3.0)

- `rst bot pull [--url URL | --file FICHIER]` : télécharge (ou installe) la dernière politique du bot, vérifiée
  (format et dimensions) avant d'écraser l'ancienne. URL par défaut `https://rocketsim.celri.dev`, surchargée par `$RST_BOT_URL`.
- `rst watch` : le bot conduit seul (buts, remise en jeu automatique) ; caméra balle / voiture / spectateur (`C`) ;
  palier et date de la politique à l'écran.
- `rst play --vs-bot` : 1v1 contre le bot (vous en bleu, layout manette inchangé), score, coups d'envoi.
- `rst bot info`, `rst doctor` signale la politique installée.
- Corrige l'ordre des voitures du snapshot (la voiture orange sortait en premier avec deux voitures).
- Prérequis serveur : learner avec l'endpoint `/policy.*` et route exposée (flux-gitops).
