# Exploration : entropie cible, plafond de KL, lr décroissant, garde-fou

PR 1 du plan de `docs/specs/decouverte-mecaniques.md` (§ 3.1), corrigée après l'incident du 17–18/11 (voir plus bas).
Constat de départ : entropie de la prod à 0,07 nat (max ln 78 ≈ 4,36) avec `ent_coef` 0,01 fixe et lr 1e-3 constant.
Code : `crates/learner/src/explore.rs` (régulateur, garde-fou) et `crates/learner/src/best.rs` (points de restauration).

| Réglage | Valeur |
| --- | --- |
| Entropie visée | `0,12 × ln(nb d'actions) × scale` ≈ 0,52 nat pour 78 actions ; gauge `trainer_entropy_target` |
| `ent_coef` | `× exp(g × (cible − H))` par itération, g = 0,03 à la hausse, 0,1 à la baisse, borné à `[1e-4, 0,02 × scale]` ; gauge `trainer_entropy_coef` |
| Garde-fou | taux de but sur 100 itérations < ½ de la référence du palier (moyenne exp. ≈ 1000 it., ≥ 0,2, établie après 300 it.) → `scale` et `ent_coef` ÷ 2 (`scale` ≥ 0,25), au plus toutes les 300 it. ; `scale` × 2 après 5000 it. sans alerte ; gauges `trainer_entropy_guard_scale`, `trainer_entropy_guard_backoffs_total` |
| Plafond de KL | les époques PPO s'arrêtent quand la KL moyenne d'une époque dépasse 0,02 |
| lr | `max(5e-5, 3e-4 / (1 + pas / 5e8))` ; gauge `trainer_learning_rate` |

État persisté dans `<out>/explore.json` (`ent_coef`, pas cumulés, `scale`). À la relecture, `ent_coef` est ramené dans
les bornes courantes : le 0,0498 laissé en prod par la première version devient 0,02 dès le redémarrage. Sans
fichier : `ent_coef` 0,01, pas à 0 (lr 3e-4), `scale` 1.

## Points de restauration (`best.rs`)

Dans le dossier `--out` (`/data/run` en prod) :

- `best-stage-N/` : meilleur état mesuré au palier N (taux de but sur la fenêtre de 100 itérations du garde-fou,
  paliers ≥ 2), remplacé seulement si le taux gagne 0,02, au plus toutes les 100 itérations. Écrit en continu : il
  existe donc toujours avant une promotion, une régression ou une alerte du garde-fou. `best.json` donne le taux, les pas
  et la date ; `goal_rate` -1 = état copié au démarrage, pas encore mesuré.
- `last-start/` : état relu au démarrage, avant la première itération (le point de retour d'un déploiement).
- `restore-from` : fichier texte contenant un dossier (absolu, ou relatif à `--out`). Au démarrage, avant la reprise,
  tous ses fichiers sont recopiés dans `--out` ; `explore.json` et `opponent-obs-v2` sont effacés s'il ne les contient
  pas (pas de mélange avec l'état courant) ; le fichier devient `restore-from.done`. Sans `checkpoint.*` dans la source :
  refus et arrêt (code 2), rien n'est touché.

Chaque état fait ≈ 350 Ko.

## Incident du 17–18/11 : la cible 1,25 nat a détruit la politique

Déployée le 17/11 vers 21 h 55 au palier 5 (taux de but 0,5–1,0, entropie ≈ 0,1). Le 18/11 à 8 h : entropie 1,25,
`ent_coef` 0,0498, curriculum redescendu 5 → 4 → 3 → 2 ; au palier 2, sur les 22 000 dernières itérations des logs, ≈ 9 buts et 44 buts contre son camp pour 100
itérations (≈ 700 épisodes) : taux de but ≈ 0,01, épisodes au temps maximal, KL ≈ 0.

Causes :

1. **Une cible d'entropie n'est pas un coefficient fixe.** Avec `ent_coef` fixe (0,01 dans rlgym-ppo / Necto, et dans
   notre prod d'avant), l'entropie s'établit là où la récompense le permet : elle n'est pas imposée. Un régulateur qui
   *vise* une entropie l'achète à n'importe quel prix : pour tenir 1,25 nat face au gradient de la politique, il lui a
   fallu ≈ 0,05, 5 × la référence. La cible 1,0–1,5 de l'étude venait de valeurs observées sur des bots matures (table
   de 90 actions, beaucoup d'états où plusieurs actions se valent), pas d'une valeur à imposer à un bot qui apprend à
   viser.
2. **Emballement du coefficient (windup).** Plafond à 1,0 = 100 × la référence. À 0,07 nat le gradient de l'entropie
   est minuscule (softmax saturé) : le coefficient atteint le plafond en ≈ 80 itérations (essai local de la PR #15) et y
   reste des heures pendant que l'entropie monte. Avec des avantages normalisés (écart-type 1), le terme de politique a
   une taille fixe : à 1,0 le terme d'entropie l'écrase dans tous les états où l'avantage n'est pas énorme, la politique
   est poussée vers l'uniforme. Quand l'entropie passe la cible, le coefficient ne baisse que de ≈ 3 % par itération et
   par nat d'écart : dépassement.
3. **Récompense rare face à un bonus dense.** Aux paliers 2–5, le seul signal de but est +10 une fois par épisode
   (≈ 300 décisions, mise en forme 0,05/décision) ; le bonus d'entropie agit sur chaque décision. Viser le but demande
   une politique concentrée (≈ 0,1–0,2 nat avant l'incident) : à 1,25 nat (≈ e^1,25 ≈ 3,5 actions « effectives » par
   décision, à 15 décisions/s), les tirs ne sont plus cadrés.
4. **Cascade du curriculum.** Le taux de but s'effondrant, le curriculum a régressé palier par palier jusqu'au 2 (on ne
   descend pas plus bas) ; aucun garde-fou ne reliait l'exploration au taux de but, et aucun état antérieur n'était
   conservé dans `/data/run`.
5. **Essai trop court.** L'essai de la PR #15 (8 min) s'est arrêté à 0,2 nat : le régime proche de la cible n'avait
   jamais été observé.

Correctif (cette version) : cible proportionnelle et basse (0,52 nat), plafond 0,02 (au plus 2 × la référence : si la
récompense refuse l'entropie, elle reste sous la cible au lieu d'être achetée), baisse 3 × plus rapide que la hausse,
garde-fou sur le taux de but, points de restauration par palier.

## Essais locaux (nœud partagé, 2 threads, ≈ 2 500–5 500 pas/s)

Taux de but : fenêtre de 100 itérations du garde-fou (`best.json`) ou estimation par blocs de 200 itérations
(épisodes ≈ 2048 / longueur moyenne, coups d'envoi compris : sous-estime un peu au palier 5).

1. **Checkpoint de prod du 18/11 tel quel** (`/data/run` : palier 2, `explore.json` à 0,0498), nouveau code, 15 min,
   1 680 itérations, 3,4 M pas. `ent_coef` relu à 0,02, puis au plancher 1e-4 en ≈ 100 itérations ; entropie
   1,24 → 0,49–0,6. Taux de but **≈ 0,01 du début à la fin** (contre son camp ≈ 0,075, comme la prod) :
   baisser l'entropie ne suffit pas, la politique a désappris. Ne repart pas en 15 min.
2. **Sauvegarde du 15/11** (`/data/sauvegarde-avant-palier5`, acteur seul, politique gelée), 5 min : palier 5 → taux
   de but **0,40** (entropie 0,18) ; palier 2 → 0,14 (politique spécialisée au palier 5). Son bloc adversaire est donc
   cohérent avec l'ordre des voitures corrigé : la restaurer avec le marqueur `opponent-obs-v2`.
3. **Reprise par `restore-from`** : dossier de prod du 18/11 + `restore-from` → copie de la sauvegarde du 15/11
   (avec marqueur), `--resume` sans palier figé, 15 min, 1 500 itérations, 3,1 M pas. Log « état restauré », palier 5,
   `ent_coef` 0,01 → 0,02 (plafond) en ≈ 100 itérations ; entropie 0,16 → 0,25 ; taux de but **0,45** sur la première
   fenêtre exacte, ≈ 0,35–0,38 par bloc de 200 itérations ensuite (estimation), stable sur toute la durée
   (pas de dérive, pas d'alerte du garde-fou, pas de régression de palier).

## Reprise de la prod après l'incident

Repartir de la sauvegarde du 15/11 (palier 5) plutôt que du checkpoint détruit (essais 1 et 3), sur le PVC du pod :

```sh
# 1. Garder l'état détruit (analyse), puis préparer la restauration (l'image actuelle ignore restore-from).
cp -a /data/run /data/sauvegarde-incident-entropie-20261118
mkdir /data/run/restore-palier5 && cp /data/sauvegarde-avant-palier5/* /data/run/restore-palier5/
echo 1 > /data/run/restore-palier5/opponent-obs-v2   # bloc adversaire cohérent (essai 2) : ne pas le remettre à zéro
echo restore-palier5 > /data/run/restore-from
# 2. Déployer l'image de cette PR (pin flux-gitops) ; au démarrage : « état restauré depuis /data/run/restore-palier5 »,
#    « exploration : ent_coef 0.0100, 0 pas cumulés », palier 5 ; /data/run/restore-from.done présent.
```

Sans restauration, le nouveau code relit `explore.json` (0,0498 → 0,02) et repart du palier 2 détruit : à éviter.

## Historique : premiers essais de la PR #15

- Plafond de `ent_coef` à 0,1 : 15 min, 1,2 M pas, entropie 0,09 → 0,115.
- Plafond 1,0, gain 0,03 : 8 min, 1,3 M pas : `ent_coef` au plafond en ≈ 80 itérations, entropie 0,107 → 0,20, taux de
  but du palier 5 inchangé. C'est cette version qui, en prod, a détruit la politique en une nuit (ci-dessus).
