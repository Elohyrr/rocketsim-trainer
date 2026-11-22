# Audit des clips de mécaniques

Mesure les faux positifs du détecteur (`crates/mechanics`) sur les clips de prod, jugés par un sous-agent Opus
avec les définitions **communautaires** (`juge.md`). Résultats : `docs/audit-clips.md`.

1. Convertir : `python3 -I tools/audit-clips/convert.py --out DIR [--since 2026-11-20 | <unix>] [--per-mechanic 5] [--max 60] [--exclude touch,bump,clear] [--detector 5]`
   → `DIR/<id>.txt` (chronologie : 1 ligne par décision, z, roues, saut/dodge, boost, entrées du bot, touchers, vitesses)
   + `DIR/manifest.json`. `--since` ne garde que les clips créés après la date ; mécaniques ≤ `--per-mechanic` : tous les clips,
   sinon échantillon reproductible (`--seed`) ; `--max` plafonne le total. Lecture seule de l'API publique
   (`--base`, défaut `https://rocketsim.celri.dev`). Les clips sortis de l'anneau (404) sont ignorés.
2. Juger : lancer des sous-agents `Agent(model: sonnet)` par lots de 10 clips (Opus seulement pour les `douteux`, 15 max : coût) avec la consigne `juge.md` et la liste
   d'ids du lot ; chaque juge écrit `DIR/verdicts/lotN.jsonl` (une ligne JSON par clip).
3. Agréger : `python3 -I tools/audit-clips/aggregate.py DIR/verdicts` → tableau Markdown (taux de faux par mécanique)
   et exemples de faux, à reporter dans `docs/audit-clips.md`.

Les verdicts sont un jugement de modèle sur une chronologie à 15 Hz : à recouper à l'œil dans le lecteur
(`rlclips.home.celri.dev/#<id>`) pour les mécaniques décisives.

Plafonds par défaut (quota) : 5 clips par mécanique, 60 au total, `touch`/`bump`/`clear` exclus. Premier audit (145 clips,
Opus) : `resultats/2026-11-22.jsonl`.
