# Étude — faire découvrir de vraies mécaniques au bot

Nov 15, 2026 · étude seule, aucune implémentation. Commit de référence : `main` @ `af1cf8e` (+ PR #13, branche
`claude/kickoff`, lue).

But originel du projet pour Elohyrr : voir si le bot **trouve** des mécaniques (speed flip, flip reset, dribble sur le
toit…), y compris des mécaniques nouvelles, et pouvoir aussi en récompenser certaines explicitement (spec d'origine,
objectif O4). Cette étude répond à six questions, avec des recommandations chiffrées et ordonnées, puis propose un plan
en PR successives.

Conventions : « pas » = une décision de l'agent (8 ticks de physique à 120 Hz, soit 15 décisions/s). Les chiffres
marqués **[mesuré]** viennent de ce dépôt ou de la prod ; **[source]** d'une source citée ; **[estimation]** sont des
ordres de grandeur argumentés, à vérifier.

## Résumé (recommandations ordonnées)

1. **Rallumer l'exploration** : l'entropie de la politique en prod est à 0,07 nat (quasi déterministe). Entropie cible
   1,0–1,5 nat, plafond de KL, lr décroissant. Sans ça, rien de nouveau ne peut émerger.
2. **Mesurer avant de récompenser** : détecteur de mécaniques par événements tick par tick (catalogue de 12),
   compteurs Prometheus, clips rejouables dans `rst watch`. La découverte = une mécanique vue **en match** sans y être
   récompensée.
3. **Observation et actions v2** (chirurgie de poids, checkpoint conservé) : action précédente (+8), état du flip et
   roues (+3), 12 actions de frein à main au sol (78 → 90).
4. **Self-play qui tient** (13 allers-retours 5 ↔ 6 en 5 h 30) : récompense à somme nulle, pool de 20 anciens, Elo,
   promotion sur l'Elo.
5. **Drills + bonus faibles, conditionnés, décroissants** pour dribble, aérien, flip reset ; jamais de bonus de méca
   dans la récompense de match.
6. **Pas de 30 Hz ni d'actions continues** pour l'instant : à 30 Hz, le meilleur coup d'envoi trouvé n'est pas plus
   rapide qu'à 15 Hz (mesuré, 1.3 b). Le flip reset existe bien dans RocketSim (mesuré, 1.3 a).
7. Budget : 420 M pas/jour mesurés, soit le volume de Necto en 5 jours ; la limite est l'exploration et la taille du
   réseau, pas le nombre de pas. Dribble et toucher aérien : semaines ; flip reset et air dribble : mois, sans garantie.

## 0. État des lieux (mesuré)

| Élément | Valeur | Où |
| --- | --- | --- |
| Actions | table discrète de **78** entrées (12 au sol + 66 « aériennes ») | `rl-env::ActionTable::rlgym` |
| Fréquence de décision | `tick_skip = 8` → **15 Hz** (action maintenue 66,7 ms) | `EnvConfig::default` |
| Observation | **43** valeurs : voiture (pos, vit, vit. ang., avant, haut), balle, relatif, boost, `on_ground`, `has_flip_or_jump`, adversaire (10) | `build_obs` |
| Réseau | MLP 2 × 128 (acteur et critique séparés), ≈ 32 k paramètres chacun | `learner::ppo` |
| PPO | 16 envs × 128 pas = 2 048 pas/itération, 4 époques, minibatch 512, lr 1e-3, γ 0,99, λ 0,95, `ent_coef` 0,01 | `PpoConfig::default` |
| Self-play | copie gelée de la politique, rafraîchie toutes les 50 itérations ; pas de pool | `FROZEN_EVERY` |
| Débit prod | **≈ 4 900 pas/s** (4,09 M pas entre 17:16 et 17:30 UTC le 15/11) ≈ **420 M pas/jour** | logs `deploy/rocketsim-trainer` [mesuré] |
| Temps par itération | collecte ≈ 0,07–0,17 s, entraînement ≈ 0,3 s : **l'apprentissage coûte 2× la simulation** | colonnes `collect_s train_s` [mesuré] |
| Entropie de la politique | **0,06–0,09 nat** (max ln 78 = 4,36) : politique quasi déterministe | colonne `entropy` [mesuré] |
| Curriculum | prod au palier 5 (adversaire statique), taux de but 0,7–1,0 ; **13 promotions 5→6 suivies de 13 régressions** sur la vie du pod (5 h 30) | logs [mesuré] |

Deux constats pèsent plus lourd que tout le reste pour la découverte :

1. **L'exploration est éteinte** : à 0,07 nat d'entropie, la politique joue presque toujours la même action dans un
   état donné. Aucune mécanique nouvelle ne peut émerger d'une politique qui n'essaie plus rien. C'est le premier
   correctif à faire (section 3), avant toute récompense de mécanique.
2. **Le self-play ne tient pas** : chaque passage au palier 6 s'effondre (taux de but < 10 %) et revient au palier 5.
   Or c'est le self-play qui crée la pression vers des mécaniques utiles (ce qui bat l'adversaire d'hier).

## 1. Expressivité : actions et fréquence de décision

### 1.1 Ce que contient la table

La table reprend la `LookupTableAction` de RLGym, **moins les 12 actions au sol avec frein à main** : RLGym en a 90
(24 au sol, 66 en l'air) [source : rlgym `LookupTableAction`, Nexto/replay-pretraining parle de 90 actions], nous en
avons 78. Conséquences :

- **Pas de powerslide au sol.** Le frein à main n'existe que couplé au saut + rotation (actions aériennes), où il ne sert
  qu'à l'atterrissage. Pas de virage serré glissé, pas de « powerslide cut », pas de demi-tour freiné.
- Pas de `saut + lacet` (`jump && yaw != 0` exclu, comme RLGym) : sans effet, la direction latérale du flip vient de
  `yaw + roll` dans RocketSim, et `saut + roll ±1` existe.
- Commandes quantifiées à {−1, 0, +1} : contrôle aérien en « tout ou rien » à 15 Hz.
- L'agent ne voit **ni sa dernière action, ni le temps depuis le saut, ni l'état du flip** (`is_flipping`,
  `flip_time`), ni le contact des roues : une séquence de plusieurs décisions (speed flip, wave dash, flip reset) doit
  être reconstruite à partir de la vitesse angulaire et de l'orientation seules. L'observation par défaut de RLGym inclut
  l'action précédente ; la nôtre non.

### 1.2 Mécanique par mécanique

Vérifié dans le code et, quand indiqué, par une expérience jetable qui rejoue des séquences dans RocketSim
(`rocketsim_rs` 0.37.0, même table d'actions ; code non commité, voir 1.3).

| Mécanique | Possible avec la table à 15 Hz ? | Limite principale |
| --- | --- | --- |
| **Flip avant / diagonal** | oui : saut → relâcher → saut + tangage (± roulis), 3 décisions | aucune |
| **Speed flip** | oui : saut, relâche, dodge diagonal `saut + p−1 + r±1`, annulation `p+1`, roulis d'atterrissage existent tous ; la grille (1.3 b) trouve des enchaînements de ce type, et **30 Hz n'apporte aucun gain mesurable** sur cette approche | séquence de 5–6 décisions, sans observation de l'action précédente |
| **Flip cancel / half flip** | oui : dodge arrière (`saut + p+1`), puis `p−1` (annulation), puis roulis ; atterrissage propre sans powerslide moins bon | pas de frein à main au sol pour pivoter en sortie |
| **Wave dash** | non démontré (voir 1.3) : il faut un dodge dans les derniers ticks avant le contact des roues arrière, fenêtre plus courte que 66,7 ms | fréquence + absence d'info « distance au sol » |
| **Air roll** | oui : roulis ±1, seul ou avec tangage/lacet, avec ou sans boost ; « air roll libre » = combinaisons de la table | tout-ou-rien, pas de demi-commande |
| **Toucher aérien** | oui (saut, double saut ou dodge + boost + tangage) | contrôle grossier à 15 Hz |
| **Dribble au sol / flick** | oui en principe (gaz et direction ±1 suffisent à suivre la balle, flick = dodge) ; à 15 Hz l'équilibre est plus difficile qu'avec des commandes continues | pas de commande « mi-gaz » : maintien de la balle en zig-zag |
| **Air dribble** | possible mais difficile : chaque correction dure ≥ 66,7 ms à fond | quantification + fréquence |
| **Flip reset** | **oui, physiquement** : RocketSim rend le flip quand ≥ 3 roues touchent la balle en l'air (vérifié, 1.3) ; la table a ce qu'il faut pour aller chercher la balle roues en avant puis dodger | l'agent ne voit pas le contact de ses roues ; seul `has_flip_or_jump` repasse à 1 |
| **Ceiling shot** | oui : conduite au plafond (gaz), décrochage, le flip reste disponible ; même mécanisme que le flip reset (roues au contact d'une surface) | rarement rentable, longue séquence |
| **Powerslide, virage glissé** | **non** au sol : frein à main absent des 12 actions au sol | table (→ 90 actions) |

### 1.3 Expériences scriptées

Code jetable (non commité, dans le dossier temporaire du travailleur) : un binaire qui dépend directement de
`rocketsim_rs` 0.37.0, reproduit la table de 78 actions et rejoue des séquences scriptées. Build dans le pod :
`CARGO_BUILD_JOBS=1`, sans LTO, zig en CC/CXX/linker (≈ 6 min à froid). Les chiffres sont des résultats de
simulation, pas des garanties sur le vrai jeu.

**a) Flip reset.** Voiture en l'air (z = 800), flip et double saut déjà utilisés, qui monte vers une balle
immobile. Roues vers la balle (voiture retournée) : 2 roues en contact au tick 18, **4 roues au tick 22 →
`is_on_ground` = 1, `has_jumped`, `has_double_jumped` et `has_flipped` repassent à 0** : flip rendu. Toit vers la balle :
toucher enregistré, flip **non** rendu. Détail important pour le détecteur : le contact roues-balle **n'enregistre pas
de toucher** (`ball_hit_info` reste invalide). Le flip reset existe donc bien dans RocketSim, avec la même règle que le
jeu (≥ 3 roues en contact = « au sol »).

**b) Coup d'envoi : quel gain les flips offrent-ils à 15 Hz ?** Position arrière centrale (0, −4608), 33 de boost,
balle au centre ; score = temps du premier toucher. Référence : le **fonceur** (gaz + boost + direction vers la balle,
décision à chaque tick) touche à **2,642 s** (la PR #13 mesure 2,65 s pour cette position : cohérent). Grille de
47 616 séquences construites avec la table : `a` décisions de boost (0–30), saut, relâche (3 variantes), dodge
(avant ou diagonal), annulation (3 variantes × 0–5 décisions), atterrissage (3 variantes × 0–5 décisions), puis
fonceur jusqu'au toucher (85 s de calcul).

| Variante (15 Hz) | Premier toucher | Gain sur le fonceur |
| --- | --- | --- |
| Fonceur (sans flip) | 2,642 s | — |
| Meilleur flip avant, sans annulation | 2,650 s | −8 ms (perte) |
| Meilleur flip diagonal, sans annulation | 2,667 s | −25 ms (perte) |
| **Meilleure séquence** : flip avant à ≈ 1,0 s, annulé (`p+1` + roulis) 5 décisions, roulis à l'atterrissage | **2,600 s** | **+42 ms** |
| Même grille à **30 Hz** (`tick_skip` 4, 357 492 séquences, 634 s) : meilleure séquence | 2,600 s | +42 ms (identique) |
| 30 Hz : vrai « speed flip » (dodge diagonal annulé par `p+1` pendant 100 ms, saut de 2 décisions) | 2,617 s | +25 ms |

Lecture : avec la table actuelle et une seule manœuvre en milieu d'approche, le meilleur « flip annulé » ne fait gagner
que ≈ 40 ms, soit **+0,08** de récompense de rapidité dans la formule de la PR #13 (`(t_ref − t) / 0,5 s`). Un signal
aussi faible, au bout de 5–6 décisions précises, explique pourquoi PPO ne le trouve pas : ce n'est pas seulement une
question de fréquence (30 Hz donne exactement le même meilleur temps), **le gain lui-même est petit** dans notre configuration (33 de boost épuisés en ≈ 1 s, pas de
second flip dans la balle). Le gain d'un speed flip de joueur vient surtout de ce qu'il garde le boost actif sans
quitter l'axe et enchaîne un second flip au contact ; la grille ne teste pas ce second flip.

**c) Wave dash.** Recherche naïve (saut, bascule arrière 1–4 décisions, attente, dodge avant, à 15, 30 et 60 Hz) :
**aucune** séquence ne retombe sur les roues dans les 12 ticks suivant le dodge. Test **non concluant** (la recherche
ne règle pas finement l'angle d'arrivée) : on ne peut ni affirmer ni exclure le wave dash avec la table. À refaire avec
un état initial « voiture en chute, cabrée de 20–30° » plutôt qu'à partir du sol.

**d) Débit de l'outil.** Cloner une arène RocketSim coûte ≈ 57 ms et en créer une ≈ 43 ms : pour des recherches de
séquences, réutiliser une arène (`reset_to_random_kickoff` + `set_car`) est ≈ 40× plus rapide. Utile pour les tests des
détecteurs (PR 2) et pour générer des démonstrations.

### 1.4 Que changer, et à quel coût

Ordonné du meilleur rapport gain/coût au pire :

1. **Ajouter l'action précédente à l'observation** (8 valeurs de contrôle, ou 78 en one-hot ; recommandé : les 8
   contrôles, +8 entrées). Coût : chirurgie d'entrée déjà maîtrisée (`Mlp::pad_inputs`, poids nuls = comportement
   identique au chargement, comme le passage 33 → 43). Gain : rend les séquences de plusieurs décisions
   (saut → relâcher → flip → annulation) markoviennes. **À faire en premier.**
2. **Ajouter 3 indicateurs d'état** : `is_flipping` (ou temps depuis le dodge, borné à 1), temps depuis le saut
   (borné), nombre de roues en contact / 4. Même chirurgie (+3 entrées). Gain : indispensable pour le flip reset
   (savoir qu'on a récupéré son flip en l'air est déjà couvert par `has_flip_or_jump`, mais savoir *pourquoi* ne l'est
   pas) et pour le wave dash (approche du sol).
3. **Table 78 → 90 : rajouter les 12 actions au sol avec frein à main**, **à la fin** de la table (indices 78–89) pour
   que les indices existants ne bougent pas. Chirurgie de sortie : ajouter 12 lignes à la dernière couche de l'acteur,
   poids nuls et biais ≈ −4 (probabilité initiale ≈ 1/(78·e⁴) ≈ 0,02 % chacune) : la politique reste quasi identique,
   puis apprend à s'en servir. Le critique ne change pas. `policy-runtime`, `rst watch` et le client lisent la taille
   dans `policy.json` : à vérifier dans la même PR.
4. **Répétition d'action choisie par l'agent** (« action repeat » appris) plutôt qu'une fréquence fixe plus haute :
   une seconde tête discrète {1, 2, 4, 8} ticks. Permet 120 Hz là où il faut (dodge + annulation) et 15 Hz ailleurs, à
   coût moyen presque inchangé. Coût : format de sortie changé (nouvelle tête, initialisée pour choisir 8 ticks à
   ~97 %), GAE avec pas de durée variable (γ^durée), refonte de `VecEnv` (les envs ne sont plus synchrones en ticks).
   Gros chantier : **seulement si** la démonstration scriptée (point 6) ne suffit pas.
5. **Passer à 30 Hz (`tick_skip` 4)**. Coût : 2× plus de décisions par seconde de jeu → l'inférence et
   l'apprentissage coûtent 2× plus par seconde simulée (la physique, elle, ne change pas) ; débit en temps de jeu
   ≈ 2× plus faible ; γ à remonter de 0,99 à 0,995 pour garder un horizon de ≈ 6,7 s ; le checkpoint se charge
   tel quel mais la politique apprise à 15 Hz joue « deux fois trop vite » ses séquences : réapprentissage partiel.
   Gain mesuré nul sur le coup d'envoi (1.3 b : même meilleur temps à 15 et 30 Hz) : **non recommandé** à ce stade. 60 Hz encore moins.
6. **Actions multi-discrètes** (8 têtes : 3×3×3×3×3×2×2×2 = 1 944 combinaisons) ou **continues** (gaussiennes
   bornées). Plus expressif (frein à main + rotation libre en l'air, demi-commandes), mais : exploration plus difficile
   (beaucoup de combinaisons inutiles), checkpoint **incompatible** (aucune correspondance exacte entre une catégorielle
   à 78 issues et 8 têtes indépendantes ; il faudrait distiller l'ancienne politique dans la nouvelle par clonage, ≈ 1
   à 2 jours de calcul). Les meilleurs bots connus (Necto/Nexto) restent sur une table de 90 actions à 15 Hz
   [source/incertain] : **non recommandé**.

**Démonstrations scriptées** (complément des points 1–3, déjà proposé par la PR #13) : clonage de comportement sur des
séquences scriptées (perte de log-vraisemblance mélangée à PPO, poids décroissant), sans changer le format du
checkpoint. Mais l'expérience 1.3 b montre qu'une manœuvre unique en milieu d'approche ne rapporte que ≈ 40 ms :
avant d'imiter un speed flip, **mesurer** avec le même outil le gain d'un coup d'envoi complet (flip annulé + second
flip dans la balle, positions diagonales) ; ne lancer la démonstration que si le gain dépasse ≈ 0,15 s.

## 2. Récompenser des mécaniques sans exploit ni enfermement

### 2.1 Détection fiable dans RocketSim

RocketSim expose par voiture (`CarState`, `rocketsim_rs` 0.37) tout ce qu'il faut, au tick près : `wheels_with_contact[4]`,
`is_on_ground`, `has_jumped`, `has_double_jumped`, `has_flipped`, `is_flipping`, `flip_time`, `flip_rel_torque`
(direction du dodge), `jump_time`, `air_time`, `world_contact { has_contact, contact_normal }`, `car_contact`,
`ball_hit_info` (tick, point de contact relatif `relative_pos_on_ball`), `is_supersonic`, `last_controls`. La
détection se fait **dans l'env, à chaque tick** (dans la boucle des 8 ticks, pas seulement à la décision), sans coût
notable (quelques comparaisons par tick face à la physique).

Les seuils ci-dessous sont des valeurs de départ ; chacun doit être validé par un test scripté (une séquence qui doit
déclencher, une qui ne doit pas).

| Mécanique | Signature (repère voiture : `forward`, `right`, `up`) | Faux positifs à exclure |
| --- | --- | --- |
| **Balle sur le toit (dribble)** | balle dans le repère voiture : hauteur `up` entre 110 et 190 uu, \|avant\| < 110, \|côté\| < 80 ; vitesse relative < 400 uu/s ; voiture `on_ground` ; maintenu ≥ 0,5 s (60 ticks) | balle qui rebondit une fois sur le toit (durée), voiture coincée contre un mur avec la balle (exiger `up.z > 0,7`) |
| **Flick** | dribble ≥ 0,5 s, puis dodge (`is_flipping` passe à 1) dans les 0,4 s, puis vitesse de balle > 1 500 uu/s dans les 0,3 s | dodge sans toucher (exiger un `ball_hit_info` après le dodge) |
| **Toucher aérien** | toucher avec `!on_ground`, `z_balle > 300`, `air_time > 0,3 s` | saut simple sur balle basse (seuils de hauteur) |
| **Air dribble** | ≥ 2 touchers aériens sans retour au sol, intervalle < 1 s, `z_balle > 500` | rebond voiture-balle passif : exiger boost actif entre les touchers |
| **Flip reset** | `is_on_ground` passe à 1 (≥ 3 roues en contact) alors que la voiture est loin de toute surface du terrain (`z > 250`, pas de `world_contact`) et que la balle est à < 200 uu : c'est la balle qui porte les roues. RocketSim remet alors `has_jumped`, `has_double_jumped`, `has_flipped` à 0 ; ensuite `CarState::has_flip_reset()` (exposé par `rocketsim_rs`) vaut vrai tant que le flip n'est pas utilisé. « Reset utilisé » = dodge ou saut avant le prochain contact avec le terrain | **le contact roues-balle ne met pas à jour `ball_hit_info`** (vérifié, 1.3) : ne pas exiger de « toucher » ; contact du toit = toucher sans reset ; reset au mur ou au plafond exclu par `world_contact` / hauteur |
| **Ceiling shot** | roues au plafond (`is_on_ground` avec `up.z < −0,9` et `z > 1 900`), puis toucher aérien dans les 3 s avec flip disponible | voiture qui roule au plafond sans toucher |
| **Speed flip** | dodge diagonal (`flip_rel_torque` avec composantes avant et latérale), puis commande de tangage opposée dans les 0,15 s (annulation), retour au sol en < 0,8 s, vitesse > 1 900 uu/s à l'atterrissage | flip diagonal non annulé (rotation complète : pas d'annulation) |
| **Wave dash** | dodge puis ≥ 3 roues au sol dans les 6 ticks (le sol coupe le flip), gain de vitesse horizontale > 250 uu/s | atterrissage normal après un flip complet (délai > 0,5 s) |
| **Half flip** | dodge arrière (`flip_rel_torque` arrière), annulation, roulis, retour au sol, cap tourné de ≈ 180° | demi-tour au sol |
| **Wall shot / double touch / bump / démo** | contacts mur, 2 touchers en l'air dont un après rebond mur, `car_contact`, `is_demoed` adverse | — |

Ce catalogue reprend celui déjà prévu dans `docs/metrics.md` (12 valeurs du label `mechanic`) et y ajoute
`dribble`, `speed_flip`, `wave_dash` (à échanger contre `bump`/`demo`, peu utiles en 1v1 sans démolition, pour rester à
12 séries).

### 2.2 Comment récompenser

Trois risques connus : **exploit** (le bot répète une mécanique inutile pour le bonus : dribbler en rond sur place),
**enfermement** (il optimise le bonus au lieu du but et n'explore plus rien d'autre), **faille de simulation** (une
« méca » qui n'existe que dans RocketSim, risque déjà noté dans la spec).

Règles recommandées, dans l'ordre d'importance :

1. **Guider par les états de départ avant de guider par la récompense.** Un épisode qui commence balle sur le toit,
   ou voiture en l'air sous une balle haute avec son flip, ou balle au plafond, montre la situation au bot sans changer
   ce qu'il optimise. C'est l'usage prévu de la crate `scenarios` (jamais faite) et la pratique courante des bots RLGym
   (« state setters ») [source/incertain]. Part recommandée : 30 % des épisodes au total, répartis entre 4–6 drills,
   le reste en match normal.
2. **Bonus faibles** : chaque bonus ≤ 10 % d'un but (≤ 1,0 pour un but à +10), versé **une fois** par occurrence
   détectée et plafonné par épisode (≤ 2 occurrences).
3. **Conditionnés à l'utilité** : le bonus n'est versé que si l'action fait avancer le jeu — vitesse de balle vers le
   but adverse après le toucher > 500 uu/s, ou but dans les 3 s (bonus différé : ajouté au pas du but, ce que la GAE
   propage). Un dribble sur place ne paie rien ; un dribble qui avance paie.
4. **Décroissants** : par mécanique, `bonus = b₀ / √(1 + n/N)` avec `n` occurrences sur la dernière heure et `N`
   ≈ 1 000 ; et extinction globale progressive (×0,5 par semaine de prod). Une mécanique *apprise* doit tenir par sa
   seule utilité (buts, victoires) : si elle disparaît quand le bonus s'éteint, elle n'était pas utile.
5. **Jamais en self-play** pour les bonus à forte valeur (flip reset, ceiling shot) : seulement dans les drills. En
   self-play, la récompense reste le but et ses dérivés.

### 2.3 Guidage ou découverte libre : on tranche

**Les deux, séparés dans le temps et dans l'espace, avec une priorité à la découverte mesurée :**

- La **récompense de match** (self-play, section 3) reste éparse et ne contient **aucun** terme de mécanique.
- Les **drills** (30 % des épisodes) portent le guidage : états de départ + bonus conditionnés et décroissants.
- Le **détecteur** (section 4) compte tout, y compris dans les matchs : c'est lui qui dit si une mécanique, apprise en
  drill, est **réutilisée spontanément en match** (le vrai critère de réussite), et s'il apparaît des séquences qui ne
  ressemblent à rien du catalogue.

Pourquoi : un guidage pur ne « découvre » rien par définition (on récompense ce qu'on connaît déjà) ; une découverte
pure sur 2 vCPU est hors budget pour les mécaniques longues (section 5). La séparation garde la question d'Elohyrr
honnête : une mécanique qui apparaît en match sans y être récompensée est une découverte, même si un drill l'a montrée.

## 3. Découverte libre : self-play, adversaires, curiosité

### 3.1 Réparer l'exploration (priorité 1)

- **Entropie cible** au lieu d'un `ent_coef` fixe : ajuster `ent_coef` à chaque itération pour viser 1,0–1,5 nat
  (`ent_coef ← ent_coef × exp(0,01 × (cible − H))`, borné [1e-4, 0,1]). Aujourd'hui 0,07 nat avec `ent_coef` = 0,01.
- Vérifier pourquoi elle s'est effondrée : lr 1e-3 constant sur 90 M pas est élevé pour PPO (RLGym recommande des lr
  de l'ordre de 1e-4–3e-4 en phase longue [source/incertain]) ; ajouter une décroissance ou un plafond de KL
  (arrêt anticipé des époques si KL > 0,02).
- Critère : entropie stable entre 0,8 et 1,5 nat pendant 24 h sans perte de taux de but au palier 5.

### 3.2 Self-play qui tient

- **Récompense de match** : but +10 / but encaissé −10 (somme nulle), plus un seul terme dense faible : vitesse de la
  balle vers le but adverse (≤ 0,1 par pas). Supprimer à terme l'approche et le bonus de toucher (ils récompensent des
  comportements, pas des résultats).
- **Pool d'adversaires passés** : un instantané de la politique toutes les 200 itérations (≈ 400 k pas), 20 gardés sur
  le PVC (≈ 130 ko chacun, négligeable). Tirage : 70 % politique courante gelée, 30 % pool (pondéré vers les
  adversaires qui battent encore l'agent). Empêche l'oubli (gagner contre hier mais perdre contre avant-hier) et les
  cycles pierre-feuille-ciseaux.
- **Elo** : toutes les 30 min, 100 matchs courts gloutons contre 3 références fixes (fonceur scripté, checkpoint
  palier 5 figé, meilleur ancien) ; Elo publié dans une gauge `trainer_elo` (1 série). Le palier 6 doit être promu sur
  un critère d'Elo ou de taux de victoire contre le pool, pas sur le taux de but brut (qui s'effondre mécaniquement
  contre un adversaire qui défend : d'où les 13 régressions).
- Réseau : 2 × 128 est petit pour du self-play. Passer à 2 × 256 coûte ≈ 3,5× en apprentissage (couche cachée 4×) :
  débit global estimé ≈ 2 000 pas/s [estimation]. À faire **après** les deux points précédents.

### 3.3 Curiosité et diversité : faisable ?

| Méthode | Principe | Coût sur 2 vCPU | Risque | Verdict |
| --- | --- | --- | --- | --- |
| **Nouveauté par comptage** sur une signature discrète | bonus `β / √N(sig)` ; `sig` = hachage de (contacts roues, sol/air, flip dispo, toucher balle, zone de hauteur, vitesse supersonique, n-gramme des 3 derniers événements) | quasi nul (table de hachage, 10⁵ entrées) | bonus pour des états rares mais inutiles (balle coincée) | **recommandé en premier**, β faible (0,01–0,05), décroissant |
| **RND** (Random Network Distillation) | erreur de prédiction d'un petit réseau sur un réseau aléatoire fixe | ≈ +30–50 % du temps d'apprentissage (2 MLP de plus, mais petits) | « télé bruitée » : rebonds chaotiques de la balle ; à calculer sur une vue voiture-balle, pas l'observation brute | essai A/B après le comptage |
| **ICM** (modèle de dynamique) | erreur de prédiction de l'état suivant | +50–100 % | même problème, plus cher | non |
| **Diversité de compétences** (DIAYN, DADS) | politique conditionnée par un code latent, récompensée si le code est devinable depuis l'état | réseau plus gros, plusieurs fois le budget | apprend des « compétences » de déplacement, rarement des mécaniques utiles ; aucun résultat publié connu sur Rocket League | non, hors budget |

Aucun résultat publié de curiosité appliquée à Rocket League n'a été trouvé (recherche faite le 15/11/2026) : c'est
une expérience, pas une recette. Elle doit être mesurée par le détecteur (section 4) : nombre de signatures
distinctes vues par jour, mécaniques du catalogue en match, et Elo (la curiosité ne doit pas le faire baisser).

## 4. Détecteur de nouvelles mécaniques

### 4.1 Événements et signature

Dans l'env, à chaque tick, transformer l'état en **événements** discrets (un octet chacun) :
`SOL`/`AIR` (changement), `SAUT`, `DOUBLE`, `DODGE(dir ∈ 8 secteurs)`, `ANNUL` (tangage opposé ≤ 0,15 s après un
dodge), `TOUCHE(sol|air|toit|roues)`, `RESET` (flip rendu en l'air), `MUR`, `PLAFOND`, `SUPERSONIQUE`, `BOOST_ON/OFF`,
`ROUES_N` (nombre de roues en contact), `BUT`. Une **séquence** = les événements d'une voiture entre deux retours
au sol « calmes » (≥ 0,5 s au sol), ou fenêtre glissante de 3 s autour d'un **moment remarquable** : but, toucher qui
envoie la balle à > 2 500 uu/s, toucher à > 1 000 uu de haut, Elo-gagnant en self-play.

### 4.2 Reconnaître et repérer le nouveau

1. **Catalogue** : chaque mécanique de 2.1 = un petit automate sur les événements (+ seuils continus). Compteur
   `trainer_mechanic_total{mechanic}`.
2. **Nouveauté** : une séquence remarquable qu'aucun automate ne reconnaît est hachée en n-grammes (n = 3 à 6) dans un
   count-min sketch (64 ko) ; si ≥ 1 n-gramme a une fréquence < 1e-5 et la séquence est remarquable, elle est
   **candidate** (`trainer_unrecognized_sequences_total`).
3. **Regroupement hors ligne** : les candidates (vecteur fixe : histogramme d'événements, hauteur max, vitesse balle,
   durée) sont regroupées hors du pod (k-means ou HDBSCAN, script lancé à la main) : une grappe qui grossit de jour en
   jour = une mécanique émergente à nommer et à ajouter au catalogue. Pas de clustering dans le pod (CPU réservé à
   l'apprentissage).

### 4.3 Clips rejouables

- Un clip = **état complet au début** (`CarState`, `BallState` sérialisés, déjà disponibles via la feature
  `serde_utils` de `rocketsim_rs`) + **suite des contrôles** des deux voitures tick par tick + métadonnées (mécanique,
  itération, Elo). RocketSim est déterministe à état et entrées identiques : le rejeu redonne la même trajectoire (à
  vérifier par un test qui rejoue et compare l'état final, sinon stocker aussi les états clés).
- Taille : 3 s × 120 ticks × 2 voitures × 8 commandes ≈ 6 ko compressé. Anneau de 500 clips par catégorie sur le PVC
  (≈ 3 Mo par catégorie), politique « garder les 50 premiers et les 450 plus récents ».
- Lecture : `rst watch --clip <fichier>` (ralenti et pause déjà dans le client) ; le learner sert la liste et les
  fichiers en lecture seule, comme `/policy.safetensors` (`GET /clips/index.json`, `GET /clips/<id>.json`).

### 4.4 Métriques Prometheus (liste fermée, < 100 séries au total)

| Métrique | Séries |
| --- | --- |
| `trainer_mechanic_total{mechanic, context=match\|drill}` (12 × 2) | 24 |
| `trainer_unrecognized_sequences_total` | 1 |
| `trainer_clips_saved_total{kind=known\|novel}` | 2 |
| `trainer_signatures_distinct` (gauge, signatures vues sur 24 h) | 1 |
| `trainer_elo` | 1 |
| `trainer_entropy_target` | 1 |

+30 séries : on passe de ≈ 58 à ≈ 88, sous l'objectif de 100 et loin du plafond de 500 (test de plafond à mettre à
jour dans la même PR). Pas d'identifiant de clip en label.

## 5. Budget de calcul

### 5.1 Ce qu'ont demandé d'autres bots

Les sources publiques sont maigres ; chiffres à lire comme des ordres de grandeur.

| Bot | Ce qu'on sait | Fiabilité |
| --- | --- | --- |
| Necto (RLGym) | ≈ 2 milliards de pas pour le niveau de référence de l'article Lucy-SKG ; réseau type Perceiver (attention) ; niveau ≈ Diamant (v1) | [source : arXiv 2305.15801 ; README Necto] |
| Nexto | Necto affiné, plus de paramètres, table de 90 actions, ≈ Grand Champion 1 ; « 250 000 h de jeu » selon la presse | [source : README Necto, replay-pretraining] ; [incertain : presse] |
| Lucy-SKG | 1 milliard de pas, 2 000–4 000 pas/s, 10–20 instances, 15 Hz ; bat Nexto 300-4 en 2v2 ; égale Necto avec 5× moins de pas | [source : arXiv 2305.15801] |
| Ripple | imitation sur replays puis RL | [incertain] |
| Seer, Element, Opti | aucun chiffre public trouvé | — |
| RocketSim | ≈ 114 000 ticks/s sur 1 cœur (i5-11400, 2v2, v2.1) ≈ 14 000 pas/s à 15 Hz | [source : README RocketSim] |
| RLGymPPO_CPP / GigaLearnCPP | ≈ 70 000 pas/s sur un PC de bureau avec GPU (C++) ; GigaLearn ≈ 2× plus | [incertain : page miroir, dépôt archivé] |

Ce qu'on ne sait pas (non sourcé) : à quel nombre de pas ces bots ont fait leur premier flip reset ou air dribble, et si
c'était récompensé. Retours de la communauté RLGym (non vérifiés) : les mécaniques aériennes avancées n'apparaissent
qu'avec des drills dédiés et des milliards de pas.

### 5.2 Notre débit et ce qu'on peut gagner

- **Mesuré** : 4 900 pas/s en prod = 420 M pas/jour = **≈ 3 milliards par semaine**, **≈ 12 milliards par mois**
  (moins les pauses Jellyfin). En volume de pas, on a le budget de Necto (2 G) en **5 jours**. Ce qui manque n'est pas
  tant le nombre de pas que la **capacité du réseau** (32 k paramètres contre des centaines de milliers ou plus) et
  l'**exploration** (entropie éteinte).
- L'apprentissage coûte 2× la collecte. Gains possibles, à mesurer un par un :

| Levier | Gain estimé | Remarque |
| --- | --- | --- |
| Itérations plus grandes (16 → 32 envs, 128 → 256 pas) | +10–20 % | moins de surcoût fixe par itération |
| Compiler pour `x86-64-v3` (AVX2/FMA) | +20–60 % sur l'apprentissage [estimation] | le nœud est un i5-1135G7 (Tiger Lake : AVX2, FMA et même AVX-512, vérifié dans `/proc/cpuinfo`) ; image actuelle en v2 |
| Moins d'époques (4 → 2) ou minibatch 512 → 2 048 | +30–50 % | à surveiller : efficacité par pas |
| iGPU Iris Xe (burn-wgpu) | inconnu ; utile seulement si le réseau grossit (≥ 256) et les minibatchs aussi | mesure jamais faite (D3), iGPU partagé avec le transcodage Jellyfin |
| Réseau 2 × 256 | −60–70 % de débit | nécessaire pour l'aérien, à compenser par les lignes ci-dessus |

### 5.3 Horizon réaliste par mécanique

Estimation pour notre pod (2 vCPU, ≈ 2–5 k pas/s selon le réseau), **avec** les PR 1 à 6 du plan, en temps de prod
(pauses comprises). Grande incertitude : × 0,5 à × 3.

| Mécanique | Sans aide (PPO + self-play seuls) | Avec drills / bonus / démonstration | Commentaire |
| --- | --- | --- | --- |
| Flip avant/diagonal pour finir l'approche | jours | — | déjà en vue (PR #13) |
| Speed flip | improbable | **1–2 semaines** (démonstration scriptée + kickoffs), si le gain mesuré le justifie | 15 Hz suffit ; gain mesuré d'une seule manœuvre ≈ 40 ms (1.3 b) |
| Half flip, wave dash | semaines, si utile | 1–3 semaines (+ powerslide pour le half flip propre) | peu de pression en 1v1 avec boost |
| Toucher aérien simple | 2–4 semaines | **1–2 semaines** (drills balle haute) | |
| Dribble (balle sur le toit) | rare | **2–4 semaines** (drill balle sur le toit) | utile seulement s'il bat l'adversaire |
| Flick | rare | 1–2 mois (après le dribble) | |
| Air dribble | très improbable | 1–3 mois, réseau 2 × 256 | contrôle aérien à 15 Hz tout-ou-rien : limite |
| Flip reset | très improbable | **2–6 mois**, drill dédié + obs roues | le flip revient bien au contact des roues (1.3) |
| Ceiling shot | très improbable | 2–6 mois, peu rentable | rarement utile |
| Mécanique « nouvelle » | possible à tout moment, mais d'abord des **failles de RocketSim** | — | chaque candidate est à vérifier dans le vrai jeu (M5) |

## 6. Plan de mise en œuvre (PR successives)

Chaque PR est petite, mesurable, et n'enlève rien à la précédente. Ordre = priorité.

| # | PR | Critère de réussite | Risques |
| --- | --- | --- | --- |
| 1 | **Exploration réparée** : entropie cible (1,0–1,5 nat), plafond de KL, lr décroissant ; métrique `trainer_entropy_target` | 24 h de prod : entropie ∈ [0,8 ; 1,5], taux de but palier 5 ≥ 0,7 | le bot rejoue plus mal quelques heures (attendu) |
| 2 | **Détecteur v1** (sans récompense) : événements par tick, automates du catalogue (12), `trainer_mechanic_total{mechanic,context}`, tests scriptés (une séquence positive + une négative par mécanique, dont celles de 1.3) | 100 % des séquences scriptées reconnues, 0 faux positif sur 10 min de politique aléatoire pour flip reset / speed flip ; débit prod −5 % au plus | coût par tick si mal écrit (mesurer) |
| 3 | **Clips** : état initial + contrôles, anneau sur le PVC, `GET /clips/…`, `rst watch --clip` ; test de déterminisme du rejeu | un clip enregistré en prod se rejoue à l'identique sur le laptop | non-déterminisme RocketSim entre machines (stocker des états clés) |
| 4 | **Observation + actions v2** : +8 (action précédente) +3 (état du flip, temps depuis saut, roues), +12 actions frein à main en fin de table ; chirurgie de poids (entrées nulles, sorties biais −4) | rechargement : mêmes actions gloutonnes sur 1 000 états tirés (≥ 99 %) ; `--eval-kickoff` inchangé à ± 5 points ; powerslide > 0 au bout de 24 h | consommateurs de `policy.json` (client, `rst`) à mettre à jour ensemble |
| 5 | **Self-play solide** : récompense somme nulle, pool de 20 anciens, Elo vs 3 références, promotion du palier 6 sur l'Elo | plus aucune régression 6→5 sur 48 h ; Elo croissant sur 1 semaine | pool trop faible = progrès lent ; garder 70 % courant |
| 6 | **Speed flip par démonstration** : d'abord mesurer (outil de 1.3 b) le gain d'un coup d'envoi complet aux 5 positions ; si > 0,15 s, clonage de comportement mélangé à PPO sur les kickoffs | `speed_flip` détecté dans ≥ 30 % des kickoffs de l'agent après 1 semaine, conservé quand le clonage est coupé, temps au toucher −0,1 s | gain trop faible pour valoir le coût (alors abandonner) ; le clonage écrase le reste : poids faible, seulement sur kickoffs |
| 7 | **Drills de mécaniques** (`scenarios`) : balle sur le toit, balle haute, sous la balle avec flip, mur ; 30 % des épisodes ; bonus conditionnés et décroissants (2.2) | `dribble` et `aerial_touch` en **match** (pas en drill) > 0 et croissants sur 2 semaines | enfermement si les bonus sont trop forts : ≤ 10 % d'un but, décroissance |
| 8 | **Nouveauté par comptage** (A/B une semaine avec / sans) | `trainer_signatures_distinct` + 30 % sans perte d'Elo | bonus pour des états inutiles ; β faible |
| 9 | **Débit** : x86-64-v3 si dispo, itérations plus grandes, mesure iGPU (D3) ; puis réseau 2 × 256 | débit ≥ 4 000 pas/s avec 2 × 256 | iGPU partagé avec Jellyfin |
| 10 | (option) **Répétition d'action apprise** si la PR 6 échoue | speed flip sans démonstration | gros chantier (VecEnv asynchrone) |

Ordre de grandeur : PR 1–5 en ≈ 2 semaines de travail + 1 semaine de mesure ; c'est le socle sans lequel aucune
« découverte » n'est mesurable. Les PR 6–8 donnent les premières mécaniques visibles (speed flip, toucher aérien,
dribble) en ≈ 1 mois. Flip reset et air dribble : horizon de plusieurs mois, sans garantie.

## Sources

- Code de ce dépôt (`crates/rl-env`, `crates/learner`, `crates/sim-core`), PR #13 (`claude/kickoff`,
  `docs/curriculum.md`), logs de prod du 15/11/2026.
- Lucy-SKG, arXiv 2305.15801 : <https://arxiv.org/abs/2305.15801>
- Necto / Nexto : <https://github.com/Rolv-Arild/Necto>, <https://github.com/Rolv-Arild/replay-pretraining>
- RocketSim : <https://github.com/ZealanL/RocketSim>
- RLGym (entraîner un agent, `action_repeat` 8) : <https://rlgym.org/Rocket%20League/training_an_agent/>
- rlgym-ppo : <https://github.com/AechPro/rlgym-ppo> ; GigaLearnCPP (archivé) : <https://github.com/ZealanL/GigaLearnCPP-Leak>
- RND : Burda et al., « Exploration by Random Network Distillation », arXiv 1810.12894 ; DIAYN : Eysenbach et al.,
  arXiv 1802.06070.
