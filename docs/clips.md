# Clips rejouables

Quand le détecteur de mécaniques (`crates/mechanics`) reconnaît une mécanique en match, l'env enregistre un **clip**
que `rst watch --clip` rejoue sur n'importe quelle machine.

## Contenu d'un clip (`sim_core::clip::Clip`, JSON)

- `mechanic` (libellé du catalogue, tout sauf `touch`), `created_unix`, `train_step` (itération du learner), `id`.
- `tick_skip` (8) et `blue[]` : équipe de chaque voiture (agent bleu, puis adversaire).
- `steps[s][voiture]` : contrôles tenus pendant une décision (gaz, direction, tangage, lacet, roulis, bits boost /
  saut / frein à main). 5 s ≈ 75 décisions.
- `keyframes[]` : état complet (`CarState` de chaque voiture, `BallState`) avant la décision `step`, tous les 5 pas ;
  le premier porte aussi l'état des pads, le dernier est l'état final.
- Durée : jusqu'à 45 décisions avant la reconnaissance (+ jusqu'à 20 pour tomber sur un état avec pads) et 15 après.
  Un clip pèse ≈ 30 Kio (JSON non compressé).

## Rejeu et tolérance

RocketSim est déterministe à état et entrées identiques, **mais** garde des états internes que `CarState` ne contient
pas (suspension, contacts de Bullet). Mesuré (`crates/sim-core/tests/clip_replay.rs`) :

- clip pris au début d'un épisode, simulation d'origine neuve : rejeu **exact** (0 uu) sur 80 décisions ;
- clip pris en plein épisode, ou dans la simulation « sale » d'un entraînement : le rejeu sur une simulation vierge
  dérive de quelques uu par décision. Le rejeu mesure l'écart à chaque état clé (`Replayer::max_drift`, affiché
  dans le HUD) puis s'y recale : écart mesuré 1–12 uu, **tolérance documentée `DRIFT_BOUND` = 15 uu** (balle de
  91 uu de rayon). Les tests de l'env (`rl-env/tests/clips.rs`) l'imposent sur des clips de jeu réels.

## Stockage et service (learner)

- `<--out>/clips/<id>.json` + `index.json`, sur le PVC. Anneau borné : `--clips-max` (200) et `--clips-mb` (200) ;
  plein, on retire le plus ancien clip de la mécanique la plus représentée (les mécaniques rares restent).
  20 s minimum entre deux clips de la même mécanique. `--no-clips` coupe tout.
- Le serveur de `--metrics` sert en lecture seule `GET /clips` (liste JSON, du plus récent au plus ancien) et
  `GET /clips/<id>` (id restreint à `[A-Za-z0-9_-]`).
- Métrique : `trainer_clips_saved_total{kind="known"|"novel"}` (`novel` reste à 0 jusqu'à la PR de nouveauté).

## Client

```
rst watch --clips                       # liste (rocketsim.celri.dev, ou --url / $RST_BOT_URL)
rst watch --clip <id>                   # télécharge et rejoue en boucle (C caméra, [ ] ralenti, R relance)
rst watch --clip https://…/clips/<id>   # ou une URL, ou un fichier local
```

## Coût

Enregistrement : ≈ 0,45 µs par décision (mesuré dans `Env::step_vs`, ≈ 0,4 % d'une décision de 100 µs ; en prod,
avec l'apprentissage, ≈ 0,2 %). Les comparaisons de débit bout en bout sont trop bruitées sur le pod partagé
(±30 %) pour mesurer moins de 5 % ; la mesure directe fait foi.
