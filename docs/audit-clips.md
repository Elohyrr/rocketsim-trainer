# Audit critique des clips de mécaniques (22/11/2026)

**But** : mesurer les faux positifs du détecteur (`crates/mechanics`, définitions de `docs/mechanics.md`) sur les clips de
prod, en jugeant chaque clip avec la définition **communautaire** de la mécanique. Détecteur **non modifié** par cette étude.

**Méthode** : 145 clips de `GET /clips` (27 mécaniques, tous `detector` 4 ; ≤ 7 par mécanique, tous si moins ; échantillon
aléatoire sinon), convertis en chronologie à 15 Hz (`tools/audit-clips/convert.py` : z, roues/sol, saut/dodge, boost, entrées
du bot, touchers, vitesses), jugés un par un par un sous-agent Opus (`tools/audit-clips/juge.md`). Verdicts bruts :
`tools/audit-clips/resultats/2026-11-22.jsonl`.

**Réserves** : jugement de modèle sur une chronologie (pas l'œil d'un joueur) : à recouper dans le lecteur pour les cas
décisifs. Échantillons petits (≤ 7) : un taux de 100 % veut dire « aucun vrai sur 6-7 », pas « 0 % en général ».
**stall** et **wave dash** : anciens clips (detector 4), avant le resserrement de la PR #39 (detector 5) ; leurs verdicts
ne mesurent pas le détecteur actuel. Les 5 clips 404 (sortis de l'anneau) ont été ignorés.

## Résultats par mécanique (trié par taux de faux)

| Mécanique | jugés | vrais | faux | douteux | taux de faux |
|---|---|---|---|---|---|
| fast_aerial | 7 | 0 | 7 | 0 | 100 % |
| half_volley | 7 | 0 | 7 | 0 | 100 % |
| pinch | 7 | 0 | 7 | 0 | 100 % |
| redirect | 7 | 0 | 7 | 0 | 100 % |
| squishy_save | 7 | 0 | 7 | 0 | 100 % |
| aerial_save | 6 | 0 | 6 | 0 | 100 % |
| air_roll_shot | 6 | 0 | 6 | 0 | 100 % |
| backboard_read | 6 | 0 | 6 | 0 | 100 % |
| bounce_dribble | 6 | 0 | 6 | 0 | 100 % |
| ceiling_shot | 6 | 0 | 6 | 0 | 100 % |
| cushion | 6 | 0 | 6 | 0 | 100 % |
| power_shot | 6 | 0 | 6 | 0 | 100 % |
| recovery | 6 | 0 | 6 | 0 | 100 % |
| tornado_spin | 6 | 0 | 6 | 0 | 100 % |
| wall_shot | 5 | 0 | 5 | 0 | 100 % |
| half_flip | 2 | 0 | 2 | 0 | 100 % |
| backboard_double | 1 | 0 | 1 | 0 | 100 % |
| double_touch | 1 | 0 | 1 | 0 | 100 % |
| dribble_turn | 7 | 0 | 6 | 1 | 86 % |
| clear | 6 | 1 | 5 | 0 | 83 % |
| save | 7 | 2 | 3 | 2 | 43 % |
| demo | 7 | 6 | 0 | 1 | 0 % |
| dribble | 7 | 7 | 0 | 0 | 0 % |
| hood_dribble | 7 | 7 | 0 | 0 | 0 % |
| stall (anciens clips) | 3 | 1 | 0 | 2 | 0 % (2 douteux) |
| wall_dribble | 2 | 1 | 0 | 1 | 0 % |
| wave_dash (ancien clip) | 1 | 0 | 0 | 1 | 0 % (1 douteux) |
| **total** | 145 | 25 | 112 | 8 | 77 % |

Seuls `dribble`, `hood_dribble` et `demo` (collisions réelles, mais fortuites : le bot ne cherche pas à démolir) tiennent.
Le jugement sur stall et wave dash est plus nuancé que « tous faux » : 1 stall vrai (`1795216791-stall-1903`, vol
stationnaire de 0,5 s) et 3 douteux ; ces clips sont de toute façon d'avant le detector 5.

## Les 10 pires (≥ 5 clips jugés)

fast_aerial, half_volley, pinch, redirect, squishy_save, aerial_save, air_roll_shot, backboard_read, bounce_dribble,
ceiling_shot : 100 % de faux (suivis de cushion, power_shot, recovery, tornado_spin, wall_shot : 100 % aussi).

## Causes transversales

1. **Chute ou sortie d'un mur / du plafond prise pour une action** : tornado_spin, recovery, air_roll_shot, ceiling_shot,
   aerial_save, cushion. Le bot tombe du mur du fond ou du plafond en tournant sans entrée cohérente ; roulis et up.z
   extrêmes déclenchent les règles.
2. **Vitesse de la voiture, pas geste** : power_shot (balle > 3 000 uu/s juste parce que la voiture est supersonique, sans
   dodge), half_volley (poussée d'une balle qui roule avec micro-rebonds z≈95), redirect (contact fortuit sur un tir qui entre
   dans son propre but).
3. **Pas de notion d'intention ni de direction** : redirect, clear, save, wall_shot (toucher en roulant sur un mur, sans saut,
   balle partie loin du but adverse), cushion (ralentissement dû au mur).
4. **Collisions voiture-voiture au coup d'envoi** : pinch (support « car », 50/50, balle < 3 500).
5. **Proximité spatiale lue comme contrôle** : bounce_dribble (balle poussée au sol), dribble_turn (courbe du mur ou
   contact bref), wall_dribble.

## Exemples de clips faux et propositions de règle

Format : id — raison. Les propositions sont **à implémenter dans une autre PR** (rien n'est changé ici).

- **fast_aerial** — `1795233483-fast_aerial-8511` (double saut à l'arrêt, balle au sol), `1795222208-fast_aerial-3994` (apogée
  265 uu vers une balle au sol), `1795222494-fast_aerial-4118` (saut de mur). Règle : second saut ≤ 0,35 s **et** nez ≥ 30°
  au 2ᵉ saut, boost tenu ≥ 0,5 s après, et gain d'altitude ≥ 400 uu (ou balle cible z > 400 avec toucher ≤ 2 s) ; exclure
  le saut depuis un mur.
- **half_volley** — `1795281080-half_volley-28211`, `…-28283`, `…-28228` : voiture au sol, aucun saut/dodge, balle quasi
  roulante (z 93–107). Règle : exiger un dodge ou un saut ≤ 0,3 s avant le toucher et un vrai rebond (apogée de la balle
  avant > 150 uu, vz descendante > 300 au rebond) ; balle z < 300 au toucher.
- **pinch** — `1795281234-pinch-28276`, `…-28218`, `…-28256` : 50/50 avec l'adversaire (support « car »), kickoff. Règle :
  support `car` exclu (garder sol/mur/coin), balle ≥ 3 500 réelle après, et toucher voiture-balle sans contact voiture
  adverse à < 250 uu dans les 8 ticks.
- **redirect** — `1795248469-redirect-14647`, `…-17887`, `…-16007` : contact fortuit, la balle file dans son propre but.
  Règle : balle après le toucher dirigée vers le but adverse ou une zone utile (pas vers son but), voiture en l'air avec saut
  ou dodge ≤ 0,5 s, angle > 45° avec gain de vitesse positif.
- **squishy_save** — `1795109128-squishy_save-3319` (balle à 540 uu/s hors cadre), `1795111170-…-4020`, `1795109182-…-3336` :
  aucune menace. Règle : (5) exigée sur la balle réellement cadrée (≥ 800 uu/s vers le but, projection dans les poteaux) et
  voiture sortant du filet avec dodge, ≤ 0,5 s avant.
- **aerial_save** — `1795279611-aerial_save-27599`, `…-27672`, `…-26999` : voiture qui tombe du mur du fond, z ≈ 110–170,
  sans saut. Règle : z voiture > 300 (ou saut/dodge réel), pas de contact avec le mur du fond dans les 0,5 s avant.
- **air_roll_shot** — `1795279929-air_roll_shot-27735`, `…-28103`, `…-23630` : chute de mur/coin, roulis de bruit. Règle :
  décollage voulu (saut), roulis en entrée tenu ≥ 3 décisions de même signe, balle > 1 500 sortant **vers l'avant**, pas dans
  son filet.
- **backboard_read** — `1795281272-backboard_read-28290`, `…-28204`, `…-28268` : voiture sur le mur du fond, aucun rebond.
  Règle : voiture en l'air (pas de contact roues) et rebond de balle avec vitesse > 800 avant le toucher, mur du fond adverse.
- **bounce_dribble** — `1795280725-bounce_dribble-28063`, `…-28082`, `…-28142` : balle poussée au sol (z 93–120). Règle :
  apogée de balle > 200 uu entre deux touchers consécutifs ; ≥ 3 rebonds contrôlés.
- **ceiling_shot** — `1795249358-ceiling_shot-15024`, `…-25260`, `…-26093` : chute libre 2 s après le plafond, contact fortuit.
  Règle : saut/dodge **après** le décollage du plafond ≤ 1 s avant le toucher, toucher vers le but adverse (vitesse > 1 000
  vers y+).
- **cushion** — `1795267084-cushion-22361`, `…-9733`, `…-15889` : balle freinée par un mur ou voiture en vrille. Règle :
  distance voiture-balle < 220 pendant ≥ 0,3 s après, voiture à plat (up.z > 0,5), aucun rebond mur dans les 6 ticks.
- **power_shot** — `1795281183-power_shot-28254`, `…-28196`, `…-28241` : voiture supersonique sans dodge, balle non cadrée.
  Règle : gain dû à la voiture (≥ 1 000) avec dodge ≤ 0,3 s avant ou balle > 3 000 dirigée à ≤ 20° du but adverse.
- **recovery** — `1795281180-recovery-28252`, `…-28263`(`1795281263-recovery-28287`), `1795281201-recovery-28260` : chute
  passive du mur, sans saut, vitesse perdue. Règle : entrée d'air roll/wave dash visible (roulis ou tangage ≠ 0) pendant la phase
  renversée et vitesse au sol ≥ 60 % maintenue, roues d'abord au sol (pas de toit).
- **tornado_spin** — `1795281270-tornado_spin-28288`, `…-28257`, `…-28239` : chute du plafond/mur ou choc. Règle : entrée de
  roulis tenue (|roll| ≥ 0,8, même signe ≥ 4 décisions), pas de collision (cadre du but) dans la phase, ≥ 0,5 s.
- **wall_shot** — `1795281096-wall_shot-28216`, `…-28245`(`1795281245-wall_shot-28281`), `1795281053-wall_shot-28197` : toucher
  en roulant sur un mur de son camp. Règle : exiger le saut depuis le mur avant le toucher (drapeau saut) **ou** balle
  partie vers le but adverse à > 1 000 uu/s ; exclure son tiers défensif sans saut.
- **half_flip** — `1795206830-half_flip-1435`, `1795199910-half_flip-946` : vitesse effondrée (1282 → 380 / 77), pas de relance.
  Règle : vitesse horizontale 1,5 s après ≥ 80 % de celle avant le dodge, et cap inversé **atteint** < 1 s.
- **backboard_double / double_touch** — `1795144862-backboard_double-9953`, `1795137443-double_touch-6998` : un seul contact
  collé (voiture et balle montent ensemble le mur). Règle : deux touchers séparés ≥ 0,25 s et un vrai rebond de balle
  entre les deux (vitesse inversée > 500), voiture en l'air entre les deux.
- **dribble_turn** — `1795266095-dribble_turn-21953`, `…-27132`, `…-22446` : balle au sol ou posée < 0,8 s. Règle : portage
  ≥ 1 s (balle côté toit, dist < 200), voiture à plat (up.z > 0,9), variation de cap ≥ 45° pendant ce portage.
- **clear** — `1795281115-clear-28224`, `…-28198`(`1795281198-clear-28258`), `1795281218-clear-28271` : poussée au sol sans menace. Règle :
  balle partie du tiers défensif vers une zone hors du tiers (y > −1 700) ou balle précédemment cadrée vers son but.
- **save** (43 %) — `1795281138-save-28235`, `1795280999-save-28177`, `1795281210-save-28265` : balle loin du but (> 2 500 uu)
  ou dribble de son côté. Règle : toucher à < 1 800 uu de la ligne, balle cadrée stable ≥ 500 uu/s avant, voiture sans
  contrôle de la balle avant.

## Rejouer l'audit

Voir `tools/audit-clips/README.md` : `convert.py --since AAAA-MM-JJ` (plafond `--max`, `--per-mechanic`), consigne du juge
`juge.md`, agrégation `aggregate.py`. Après resserrement du détecteur (ex. detector 5), relancer avec `--detector 5` pour
mesurer le gain.
