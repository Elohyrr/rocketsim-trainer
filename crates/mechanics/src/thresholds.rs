//! Seuils du détecteur (spec §2.1, valeurs de départ). Durées en ticks à 120 Hz, distances en uu,
//! vitesses en uu/s.

/// Deux changements de `ball_hit_tick` séparés d'au plus ce nombre de ticks = un seul toucher (contact
/// continu : balle sur le toit, poussée du nez).
pub const TOUCH_MERGE_TICKS: u64 = 4;

/// Toucher aérien : balle au-dessus de cette hauteur.
pub const AERIAL_BALL_MIN_Z: f32 = 300.0;
/// Toucher aérien : en l'air depuis au moins 0,3 s (depuis le dernier contact roues-terrain).
pub const AERIAL_MIN_AIR_TICKS: u32 = 36;

/// Air dribble : deux touchers aériens espacés de moins de 1 s…
pub const AIR_DRIBBLE_MAX_GAP_TICKS: u64 = 120;
/// … balle au-dessus de cette hauteur à chaque toucher (et boost actif entre les deux).
pub const AIR_DRIBBLE_BALL_MIN_Z: f32 = 500.0;

/// Flip reset : voiture au-dessus de cette hauteur…
pub const RESET_MIN_Z: f32 = 250.0;
/// … centre de la balle à moins de cette distance du centre de la voiture, côté roues.
pub const RESET_BALL_DIST: f32 = 200.0;
/// Ticks tolérés entre le passage `on_ground` et la remise à zéro des drapeaux de saut/flip.
pub const RESET_CONFIRM_TICKS: u8 = 3;

/// Plafond : roues au plafond si `up.z` < −0,9 et z > 1 900.
pub const CEILING_UP_Z: f32 = -0.9;
pub const CEILING_MIN_Z: f32 = 1900.0;
/// Ceiling shot : toucher aérien (flip disponible) dans les 3 s après avoir quitté le plafond.
pub const CEILING_SHOT_WINDOW_TICKS: u64 = 360;

/// Mur : roues sur une surface dont la normale est horizontale (|up.z| < 0,5) au-dessus de 200 uu.
pub const WALL_MAX_ABS_UP_Z: f32 = 0.5;
pub const WALL_MIN_Z: f32 = 200.0;
/// Wall shot : toucher sur le mur ou dans la seconde qui suit le départ du mur (sans retour au sol).
pub const WALL_SHOT_WINDOW_TICKS: u64 = 120;

/// Double touch : second toucher en l'air au plus 3 s après le premier, rebond de la balle sur un mur entre
/// les deux, sans retour au sol.
pub const DOUBLE_TOUCH_WINDOW_TICKS: u64 = 360;
/// Arène RocketSim (soccar) : murs latéraux x = ±4 096, murs du fond y = ±5 120, rayon de balle 91,25.
pub const ARENA_SIDE_X: f32 = 4096.0;
pub const ARENA_BACK_Y: f32 = 5120.0;
pub const BALL_RADIUS: f32 = 91.25;
/// Marge (au-delà du rayon) pour considérer la balle au contact d'un mur.
pub const WALL_BOUNCE_MARGIN: f32 = 120.0;
/// Vitesse normale minimale (avant rebond) pour compter un rebond de la balle sur un mur.
pub const WALL_BOUNCE_MIN_SPEED: f32 = 100.0;

/// Dribble : balle dans le repère voiture (hauteur `up`, avant, côté).
pub const DRIBBLE_UP_MIN: f32 = 110.0;
pub const DRIBBLE_UP_MAX: f32 = 190.0;
pub const DRIBBLE_MAX_ABS_FWD: f32 = 110.0;
pub const DRIBBLE_MAX_ABS_SIDE: f32 = 80.0;
/// Dribble : vitesse balle − voiture.
pub const DRIBBLE_MAX_REL_SPEED: f32 = 400.0;
/// Dribble : voiture à plat (exclut voiture coincée contre un mur).
pub const DRIBBLE_MIN_UP_Z: f32 = 0.7;
/// Dribble : maintenu 0,5 s.
pub const DRIBBLE_MIN_TICKS: u32 = 60;
/// Dribble : trou toléré (petit rebond sur le toit) sans remettre la durée à zéro.
pub const DRIBBLE_GAP_TICKS: u32 = 6;

/// Flick : dodge au plus 0,4 s après la fin d'un dribble…
pub const FLICK_DODGE_WINDOW_TICKS: u64 = 48;
/// … puis toucher et balle > 1 500 uu/s au plus 0,3 s après le dodge.
pub const FLICK_HIT_WINDOW_TICKS: u64 = 36;
pub const FLICK_MIN_BALL_SPEED: f32 = 1500.0;

/// Dodge diagonal : les deux composantes (normalisées) au moins à ce niveau.
pub const DIAGONAL_MIN_COMPONENT: f32 = 0.3;
/// Annulation : tangage opposé au dodge, au moins à cette amplitude, au plus 0,15 s après le dodge.
pub const CANCEL_MIN_PITCH: f32 = 0.5;
pub const CANCEL_WINDOW_TICKS: u64 = 18;

/// Speed flip : retour au sol au plus 1,25 s après le dodge, vitesse horizontale > 1 750 uu/s.
/// Mesuré (`rl-env/examples/speed_flip_bench`, 20 000 essais par résolution de décision, coup d'envoi diagonal à
/// 33 de boost) : le dodge est suivi d'au moins 113 ticks (0,94 s) de vol même au tick près, car le saut seul dure
/// ≈ 0,9 s (292 uu/s vers le haut, gravité 650) ; l'ancien seuil de 96 ticks (0,8 s) est donc physiquement
/// inatteignable, à 15 comme à 30 Hz. Meilleurs vols : 113–150 ticks ; vitesse horizontale max à l'atterrissage
/// 1 935 (tick), 1 886 (30 Hz), 1 835 (15 Hz) : 33 de boost ne donnent pas plus. Un dodge diagonal non annulé
/// (rotation complète) reste exclu par la condition d'annulation. Voir `docs/trente-hz.md`.
pub const SPEED_FLIP_MAX_LAND_TICKS: u64 = 150;
pub const SPEED_FLIP_MIN_LAND_SPEED: f32 = 1750.0;
/// … avec au moins 200 uu/s de gain depuis le dodge et un cap tourné de moins de ≈ 26° (cos > 0,9) : sans ces deux
/// conditions, le seuil élargi laissait passer des dodges aléatoires (132 sur 10 min de politique aléatoire).
pub const SPEED_FLIP_MIN_GAIN: f32 = 200.0;
pub const SPEED_FLIP_MIN_HEADING_COS: f32 = 0.9;

/// Wave dash : retour au sol au plus 6 ticks après le dodge, gain de vitesse horizontale > 250 uu/s.
pub const WAVE_DASH_MAX_LAND_TICKS: u64 = 6;
pub const WAVE_DASH_MIN_GAIN: f32 = 250.0;
/// Wave dash : voiture quasi à plat (`up.z` au-dessus de ce seuil) au tick du dodge, avec au plus ce nombre de
/// roues au contact (typiquement 2 roues arrière posées, l'avant en l'air ; jamais toutes au sol).
pub const WAVE_DASH_MIN_UP_Z: f32 = 0.9;
pub const WAVE_DASH_MAX_WHEELS: u8 = 2;

/// Half flip : dodge essentiellement arrière (composante avant normalisée < −0,7), annulation, roulis
/// (|roll| ≥ 0,5 après l'annulation), retour au sol en 1,5 s, cap tourné de plus de ≈ 143° (cos < −0,8).
pub const HALF_FLIP_MAX_FWD: f32 = -0.7;
pub const HALF_FLIP_MIN_ROLL: f32 = 0.5;
pub const HALF_FLIP_MAX_LAND_TICKS: u64 = 180;
pub const HALF_FLIP_MAX_HEADING_COS: f32 = -0.8;

/// Durée de vie maximale de l'état « dodge en cours » (au-delà, oublié).
pub const DODGE_MEMORY_TICKS: u64 = 180;

// ---------------------------------------------------------------- v2 (détecteur strict)

/// Saut volontaire : le saut est pressé depuis le sol/mur, ou au plus 6 ticks après avoir quitté le terrain.
pub const VOLUNTARY_JUMP_TICKS: u32 = 6;

/// Toucher aérien v2 : voiture z > 300, balle z > 500, en l'air depuis ≥ 0,5 s après un saut volontaire.
pub const AERIAL_CAR_MIN_Z: f32 = 300.0;
pub const AERIAL_V2_BALL_MIN_Z: f32 = 500.0;
pub const AERIAL_V2_MIN_AIR_TICKS: u32 = 60;

/// Wall shot v2 : balle envoyée vers le but adverse à plus de 1 000 uu/s (jugé dans les 6 ticks du toucher),
/// ou saut depuis le mur.
pub const WALL_SHOT_MIN_GOAL_SPEED: f32 = 1000.0;
pub const WALL_SHOT_JUDGE_TICKS: u64 = 6;
/// … et le toucher doit y ajouter au moins 300 uu/s (une balle déjà lancée vers le but ne compte pas).
pub const WALL_SHOT_MIN_GAIN: f32 = 300.0;

/// Dribble v2 : touchers distincts ≥ 10 ticks d'écart ; 5 touchers distincts, ou (contrôle ≥ 2 s et ≥ 3
/// touchers) ; dans les deux cas la voiture avance d'au moins `DRIBBLE_V2_MIN_DIST` uu pendant le contrôle.
pub const DRIBBLE_TOUCH_SEP_TICKS: u64 = 10;
pub const DRIBBLE_V2_MIN_TOUCHES: u32 = 5;
pub const DRIBBLE_V2_LONG_TICKS: u32 = 240;
pub const DRIBBLE_V2_LONG_MIN_TOUCHES: u32 = 3;
pub const DRIBBLE_V2_MIN_DIST: f32 = 600.0;

/// Qualité : vitesse max de la balle vers le but adverse dans la seconde qui suit, but dans les 3 s.
pub const QUALITY_SPEED_TICKS: u32 = 120;
pub const QUALITY_GOAL_TICKS: u32 = 360;
pub const QUALITY_SPEED_REF: f32 = 3000.0;
/// |y| du centre de la balle au-delà duquel le but est marqué (RocketSim).
pub const GOAL_LINE_Y: f32 = 5124.25;

/// Décollage au boost sans saut : intentionnel si la balle est portée (côté toit, à moins de 250 uu du centre).
pub const CARRIED_BALL_DIST: f32 = 250.0;

// ---------------------------------------------------------------- catalogue étendu

/// Jugement d'un toucher : vitesse de balle maximale dans les 4 ticks qui suivent (inclus le tick du toucher).
pub const HIT_JUDGE_TICKS: u64 = 4;

// ---- groupe hit

/// Power shot : vitesse de balle après le toucher (jugée, uu/s) au-delà de ce seuil…
pub const POWER_SHOT_MIN_SPEED: f32 = 3000.0;
/// … avec un gain d'au moins ce montant (uu/s) : une balle déjà rapide à peine déviée n'est pas un tir.
pub const POWER_SHOT_MIN_GAIN: f32 = 500.0;

/// Flip shot : dodge avant ou diagonal (secteur 0, 1 ou 7) au plus 0,25 s avant le toucher, balle touchée par
/// l'avant de la voiture (au moins `FLIP_SHOT_MIN_FWD` uu devant le centre), gain de vitesse ≥ 300 uu/s.
pub const FLIP_SHOT_MAX_DODGE_TICKS: u64 = 30;
pub const FLIP_SHOT_MIN_FWD: f32 = 30.0;
pub const FLIP_SHOT_MIN_GAIN: f32 = 300.0;
/// … et balle basse (centre sous 300 uu : jeu au sol ; plus haut, c'est un aérien, un pinch ou un reset).
pub const FLIP_SHOT_MAX_BALL_Z: f32 = 300.0;

/// Pinch : gain de vitesse de balle (après − avant, uu/s) et vitesse après (uu/s).
pub const PINCH_MIN_GAIN: f32 = 2500.0;
pub const PINCH_MIN_SPEED: f32 = 3500.0;
/// Pinch : centre de la balle à moins de `BALL_RADIUS` + cette marge (uu) d'une surface au toucher.
pub const PINCH_SURFACE_MARGIN: f32 = 60.0;
/// Pinch : balle entre la voiture et la surface : (balle − voiture)·normale sortante > ce ratio × |balle − voiture|.
pub const PINCH_INSIDE_RATIO: f32 = 0.2;
/// Hauteur du plafond (uu).
pub const ARENA_CEILING_Z: f32 = 2044.0;
/// Pan de coin diagonal (approximation) : plan |x| + |y| = cette somme (uu).
pub const ARENA_CORNER_SUM: f32 = 8000.0;
/// Ouverture du but (demi-largeur et hauteur, uu) : pas de mur du fond devant le but.
pub const GOAL_HALF_WIDTH: f32 = 892.755;
pub const GOAL_HEIGHT: f32 = 642.775;

/// Kuxir pinch : dodge au plus 0,35 s avant le toucher (ticks), balle envoyée vers le but adverse à plus de
/// cette vitesse (uu/s).
pub const KUXIR_MAX_DODGE_TICKS: u64 = 42;
pub const KUXIR_MIN_GOAL_SPEED: f32 = 1500.0;

/// Cushion : balle arrivant à plus de cette vitesse (uu/s)…
pub const CUSHION_MIN_SPEED_BEFORE: f32 = 1000.0;
/// … qui repart avec la voiture : |v_balle − v_voiture| après le toucher sous ce seuil (uu/s)…
pub const CUSHION_MAX_REL_SPEED: f32 = 400.0;
/// … et vitesse après < ce ratio × vitesse avant.
pub const CUSHION_MAX_RATIO: f32 = 0.5;

/// Redirect : voiture en l'air au-dessus de cette hauteur (uu)…
pub const REDIRECT_MIN_CAR_Z: f32 = 300.0;
/// … balle à plus de cette vitesse avant et après (uu/s)…
pub const REDIRECT_MIN_SPEED: f32 = 1500.0;
/// … et déviée de plus de 45° (cosinus de l'angle avant/après sous ce seuil).
pub const REDIRECT_MAX_COS: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// Air roll shot : rotation autour de l'axe avant (rad/s) ou commande de roulis (|roll| ≥) au toucher…
pub const AIR_ROLL_MIN_RATE: f32 = 3.0;
pub const AIR_ROLL_MIN_INPUT: f32 = 0.5;
/// … balle envoyée à plus de cette vitesse (uu/s)…
pub const AIR_ROLL_MIN_SPEED: f32 = 1500.0;
/// … sans dodge dans les 0,65 s (durée du couple d'un flip, ticks) : un dodge latéral tourne aussi autour de l'axe avant.
pub const AIR_ROLL_NO_DODGE_TICKS: u64 = 78;

/// Backboard read : toucher au plus 2 s (ticks) après un rebond de la balle sur le mur du fond, balle z > (uu).
pub const BACKBOARD_READ_MAX_TICKS: u64 = 240;
pub const BACKBOARD_READ_MIN_Z: f32 = 300.0;

/// Half volley : rebond de la balle au sol (centre sous cette hauteur, uu ; vitesse verticale < −ce seuil puis
/// > 0, uu/s)…
pub const HALF_VOLLEY_BOUNCE_MAX_Z: f32 = 170.0;
pub const HALF_VOLLEY_BOUNCE_MIN_VZ: f32 = 100.0;
/// … toucher au plus ce nombre de ticks après, balle sous cette hauteur (uu), envoyée à plus de cette vitesse (uu/s).
pub const HALF_VOLLEY_MAX_TICKS: u64 = 8;
pub const HALF_VOLLEY_MAX_BALL_Z: f32 = 300.0;
pub const HALF_VOLLEY_MIN_SPEED: f32 = 1800.0;
// ---- groupe carry

/// Hood dribble : moyenne de la position avant de la balle (repère voiture, uu) pendant le contrôle au-delà de
/// laquelle la balle est sur le capot plutôt que sur le toit.
pub const HOOD_MIN_MEAN_FWD: f32 = 35.0;

/// Reverse dribble : vitesse longitudinale (vel·forward, uu/s) en dessous de laquelle la voiture recule.
pub const REVERSE_MAX_FWD_SPEED: f32 = -300.0;
/// Reverse dribble : portage en marche arrière d'au moins 0,5 s (ticks)…
pub const REVERSE_MIN_TICKS: u32 = 60;
/// … avec au moins ce nombre de touchers distincts…
pub const REVERSE_MIN_TOUCHES: u32 = 3;
/// … et au moins cette distance parcourue (uu).
pub const REVERSE_MIN_DIST: f32 = 400.0;

/// Wall dribble : portage sur un mur d'au moins 0,5 s (ticks), au moins 400 uu parcourus.
pub const WALL_DRIBBLE_MIN_TICKS: u32 = 60;
pub const WALL_DRIBBLE_MIN_DIST: f32 = 400.0;
/// Ceiling dribble : portage au plafond d'au moins 0,4 s (ticks).
pub const CEILING_DRIBBLE_MIN_TICKS: u32 = 48;
/// Wall / ceiling dribble : touchers distincts minimum pendant le portage (la balle ne tient pas seule).
pub const SURFACE_DRIBBLE_MIN_TOUCHES: u32 = 1;

/// Bounce dribble : au moins 3 touchers, espacés de 10 à 60 ticks…
pub const BOUNCE_DRIBBLE_MIN_TOUCHES: u32 = 4;
pub const BOUNCE_DRIBBLE_MIN_GAP_TICKS: u64 = 10;
pub const BOUNCE_DRIBBLE_MAX_GAP_TICKS: u64 = 60;
/// … voiture au sol à plus de 300 uu/s (vitesse horizontale)…
pub const BOUNCE_DRIBBLE_MIN_CAR_SPEED: f32 = 300.0;
/// … balle sous 300 uu de hauteur et à moins de 500 uu de la voiture à chaque toucher…
pub const BOUNCE_DRIBBLE_BALL_MAX_Z: f32 = 300.0;
pub const BOUNCE_DRIBBLE_MAX_DIST: f32 = 500.0;
/// … toucher doux : vitesse de balle après le toucher sous 1 500 uu/s.
pub const BOUNCE_DRIBBLE_MAX_BALL_SPEED: f32 = 1500.0;
/// Rebond de la balle au sol : centre sous 140 uu (rayon 91,25 + marge) et vitesse verticale qui passe de
/// moins de −50 uu/s à positive.
pub const BALL_FLOOR_BOUNCE_MAX_Z: f32 = 140.0;
pub const BALL_FLOOR_BOUNCE_MIN_VZ: f32 = 50.0;

/// Musty flick : dodge arrière (composante avant normalisée < −0,5)…
pub const MUSTY_MAX_FWD: f32 = -0.5;
/// … avec la balle au-dessus de la voiture au moment du dodge (hauteur dans le repère voiture, uu).
pub const MUSTY_MIN_BALL_UP: f32 = 60.0;
/// Delayed flick : dodge au moins 0,3 s (ticks) après le saut, balle toujours portée.
pub const DELAYED_FLICK_MIN_TICKS: u64 = 36;
/// Breezi flick : vitesse de rotation autour de l'axe haut de la voiture (|ang_vel·up|, rad/s) au toucher.
pub const BREEZI_MIN_YAW_RATE: f32 = 3.0;
// ---- groupe move

/// Wall to air dribble : la voiture roulait sur un mur au plus 1,5 s (ticks) avant le premier toucher de la chaîne.
pub const W2AD_MAX_WALL_TICKS: u64 = 180;
/// Ceiling reset : hauteur minimale du centre de la balle au flip reset (uu ; plafond à 2 044).
pub const CEILING_RESET_BALL_MIN_Z: f32 = 1800.0;
/// Reset shot : toucher au plus 0,3 s (ticks) après le dodge qui suit le reset…
pub const RESET_SHOT_HIT_TICKS: u64 = 36;
/// … et balle envoyée à plus de cette vitesse (uu/s, jugée après le toucher).
pub const RESET_SHOT_MIN_SPEED: f32 = 1500.0;
/// Chain dash : deux wave dashes à au plus 1,5 s (ticks) d'écart.
pub const CHAIN_DASH_MAX_GAP_TICKS: u64 = 180;
/// Wall dash : dodge au plus 0,3 s (ticks) après avoir quitté le mur…
pub const WALL_DASH_DODGE_TICKS: u64 = 36;
/// … retour au terrain au plus 1 s (ticks) après le dodge…
pub const WALL_DASH_LAND_TICKS: u64 = 120;
/// … avec un gain de vitesse (uu/s) par rapport au tick d'avant le dodge.
pub const WALL_DASH_MIN_GAIN: f32 = 200.0;
/// Fast aerial : second saut au plus 0,35 s (ticks) après le premier…
pub const FAST_AERIAL_MAX_GAP_TICKS: u64 = 42;
/// … avec un tangage vers le haut (−1 = nez vers le bas) au-delà de ce niveau entre les deux.
pub const FAST_AERIAL_MIN_PITCH: f32 = 0.3;
/// Stall : voiture au-dessus de cette hauteur (uu) au dodge…
pub const STALL_MIN_Z: f32 = 300.0;
/// … lacet et roulis commandés d'au moins cette amplitude, de signes opposés…
pub const STALL_MIN_YAW_ROLL: f32 = 0.5;
/// … qui s'annulent (|lacet + roulis| sous ce seuil, comme RocketSim : direction de dodge nulle)…
pub const STALL_MAX_YAW_ROLL_SUM: f32 = 0.1;
/// … sans tangage (|tangage| sous ce seuil)…
pub const STALL_MAX_PITCH: f32 = 0.1;
/// … couple de dodge (norme avant/latéral) sous ce seuil : aucune rotation de dodge…
pub const STALL_MAX_FLIP_TORQUE: f32 = 0.1;
/// … vitesse horizontale presque inchangée au tick du dodge (pas d'impulsion), écart max (uu/s)…
pub const STALL_MAX_HSPEED_JUMP: f32 = 100.0;
/// … puis, à ce nombre de ticks après le dodge (0,2 s), toujours en l'air sans toucher ni choc…
pub const STALL_CHECK_TICKS: u64 = 24;
/// … avec une vitesse verticale annulée (|vz| sous ce seuil, uu/s).
pub const STALL_MAX_VZ: f32 = 120.0;
/// Tornado spin : en l'air depuis au moins 0,25 s (ticks)…
pub const TORNADO_MIN_AIR_TICKS: u32 = 30;
/// … rotation autour de l'axe `up` de la voiture au-delà de cette vitesse (rad/s)…
pub const TORNADO_MIN_SPIN: f32 = 4.0;
/// … maintenue au moins 0,3 s (ticks consécutifs).
pub const TORNADO_MIN_TICKS: u32 = 36;
/// Recovery : au moins 0,6 s (ticks) en l'air…
pub const RECOVERY_MIN_AIR_TICKS: u32 = 72;
/// … voiture renversée ou de côté à un moment (`up.z` sous ce seuil)…
pub const RECOVERY_FLIPPED_UP_Z: f32 = 0.2;
/// … atterrissage à plat (`up.z` au-dessus de ce seuil)…
pub const RECOVERY_LAND_UP_Z: f32 = 0.8;
/// … en gardant cette fraction de la vitesse horizontale du début de la phase aérienne…
pub const RECOVERY_MIN_KEEP: f32 = 0.6;
/// … et au moins cette vitesse horizontale (uu/s) à l'atterrissage.
pub const RECOVERY_MIN_SPEED: f32 = 400.0;

// ---- dribble_turn (groupe carry)
/// Changement de cap cumulé (rad, ≈ 45°) pendant un portage au sol pour compter un virage avec la balle.
pub const DRIBBLE_TURN_MIN_ANGLE: f32 = 0.785;
/// Vitesse horizontale mini (uu/s) : un tour sur place balle sur le toit n'est pas un virage de dribble.
pub const DRIBBLE_TURN_MIN_SPEED: f32 = 250.0;

// --- Collisions voiture-voiture et touchers simultanés.

/// Fenêtre d'invalidation (0,5 s) après une collision ou un toucher simultané : les mécaniques de
/// [`crate::Mechanic::collision_sensitive`] ne comptent pas.
pub const COLLISION_WINDOW_TICKS: u64 = 60;
/// Deux touchers de la balle par deux voitures à au plus ce nombre de ticks d'écart (= `HIT_JUDGE_TICKS` : le jugement du toucher voit le partage) = toucher simultané.
pub const SHARED_TOUCH_TICKS: u64 = 4;
/// Collision détectée sans contact RocketSim : l'autre voiture à moins de cette distance (centres) et un saut de
/// vitesse d'un tick supérieur à `COLLISION_MIN_DV` (uu/s ; `COLLISION_MIN_DV_TOUCH` si la voiture touche aussi la balle).
pub const COLLISION_NEAR_DIST: f32 = 260.0;
pub const COLLISION_MIN_DV: f32 = 250.0;
pub const COLLISION_MIN_DV_TOUCH: f32 = 500.0;
/// Car pinch : toucher simultané (ou collision) avec gain de vitesse de balle ≥ 1 500 et vitesse après ≥ 2 000,
/// l'autre voiture à moins de 350 uu de la balle au toucher.
pub const CAR_PINCH_MIN_GAIN: f32 = 1500.0;
pub const CAR_PINCH_MIN_SPEED: f32 = 2000.0;
pub const CAR_PINCH_MAX_OTHER_DIST: f32 = 350.0;
/// Bump : saut de vitesse de l'autre voiture d'au moins 350 uu/s en un tick, plus grand que celui de la nôtre.
pub const BUMP_MIN_OTHER_DV: f32 = 350.0;
/// Demo : l'autre voiture devient démolie dans les 12 ticks qui suivent un contact.
pub const DEMO_CONTACT_TICKS: u64 = 12;
/// Un choc compte comme « subi » si la vitesse change d'au moins 150 uu/s ou la rotation d'au moins 3 rad/s en un tick.
pub const COLLISION_TAINT_DV: f32 = 150.0;
pub const COLLISION_TAINT_DW: f32 = 3.0;

// --- Arrêts (save, aerial_save, squishy_save).

/// Horizon de la trajectoire balistique de la balle (2 s) et pas d'intégration (1/60 s).
pub const SAVE_HORIZON_TICKS: u32 = 240;
pub const SAVE_STEP_TICKS: u32 = 2;
/// Gravité (uu/s²), restitution du rebond au sol, centre de la balle au sol.
pub const BALL_GRAVITY: f32 = 650.0;
pub const BALL_GROUND_RESTITUTION: f32 = 0.6;
pub const BALL_GROUND_Z: f32 = 93.0;
/// Squishy save : marge sur les poteaux (|x| < 1 600 à la ligne, poteaux à 893) pour la menace : le clip 311 validé
/// avait une balle coupant la ligne à x ≈ +1 410.
pub const SQUISHY_GOAL_MARGIN_X: f32 = 1600.0;
/// Squishy save : voiture partie de l'intérieur de son but (roues sur le fond du filet ou le plafond : up.z < 0,7),
/// en l'air au plus 2 s (240 ticks ; le clip 311 en compte ≈ 1,8 s, une chute depuis le haut des cages dure longtemps ; 3 ticks de tolérance) avant le toucher, dodge au plus 0,5 s avant le toucher.
pub const SQUISHY_NET_MAX_UP_Z: f32 = 0.7;
pub const SQUISHY_MAX_LEAVE_TICKS: u64 = 240;
pub const SQUISHY_AIR_SLACK_TICKS: u64 = 3;
pub const SQUISHY_DODGE_MAX_AGE_TICKS: u64 = 60;
/// Clear : balle à plus de 2 500 uu du centre du terrain côté son but, vitesse après ≥ 1 500, éloignement ≥ 1 000.
pub const CLEAR_MIN_OWN_Y: f32 = 2500.0;
pub const CLEAR_MIN_SPEED: f32 = 1500.0;
pub const CLEAR_MIN_AWAY: f32 = 1000.0;

// --- Save : vitesse de balle d'avant la séquence de contacts ; coup d'envoi.
/// Un toucher de l'autre voiture à au plus ce nombre de ticks du toucher jugé (avant ou après) fait partie de la
/// même séquence de contacts : la vitesse « avant » se lit avant le tout premier contact.
pub const SAVE_SEQUENCE_TICKS: u64 = 20;
/// Écart (ticks) entre les deux lectures qui doivent toutes deux montrer la balle filant vers son but.
pub const SAVE_STABLE_TICKS: u64 = 8;
/// Vitesse minimale de la balle vers son but avant la séquence (uu/s) : une balle posée au centre n'est pas un tir.
pub const SAVE_MIN_TOWARD: f32 = 500.0;
/// Taille de l'historique de balle (ticks, puissance de 2 > fenêtre + marge).
pub const BALL_HISTORY: usize = 64;
/// Phase de coup d'envoi : jusqu'à ce nombre de ticks (1,5 s) après le premier toucher de la balle.
pub const KICKOFF_PHASE_TICKS: u64 = 180;
/// `kickoff_won` : vitesse de balle vers le but adverse après le premier toucher (uu/s).
pub const KICKOFF_WON_MIN_TOWARD: f32 = 1200.0;

// --- Detector 6 (audit des clips) : une mécanique est une action VOULUE, lisible dans les entrées du bot.

/// Saut/dodge « récent » : au plus 0,5 s (ticks) avant le toucher (redirect, aerial save, ceiling shot…).
pub const INTENT_JUMP_TICKS: u64 = 60;
/// Power shot : gain dû à la voiture au moins ce montant (uu/s), avec un dodge ≤ 0,3 s avant (ticks) ou une balle
/// > `POWER_SHOT_MIN_SPEED` dirigée à ≤ 20° du but adverse (cosinus).
pub const POWER_SHOT_MIN_GAIN_V6: f32 = 1000.0;
pub const POWER_SHOT_DODGE_TICKS: u64 = 36;
pub const POWER_SHOT_AIM_COS: f32 = 0.9397;
/// Pinch : aucune voiture adverse à moins de cette distance (uu) de la balle au toucher ; pas au coup d'envoi.
pub const PINCH_MIN_OTHER_DIST: f32 = 250.0;
/// Squishy save : vitesse de la balle vers son but avant le toucher (uu/s). 600 et non 800 : le clip validé 311
/// avait 733 uu/s vers le but, le faux clip 3319 540.
pub const SQUISHY_MIN_TOWARD: f32 = 600.0;
/// Aerial save : voiture plus haute que ceci (uu) hors saut/dodge voulu.
pub const AERIAL_SAVE_MIN_Z: f32 = 300.0;
/// Air roll shot : roulis tenu de même signe au moins ce nombre de ticks (≈ 3 décisions du bot à 15 Hz).
pub const AIR_ROLL_HOLD_TICKS: u32 = 24;
/// Backboard read : vitesse de la balle avant le toucher (uu/s) et voiture en l'air.
pub const BACKBOARD_READ_MIN_SPEED: f32 = 800.0;
/// Half volley : apogée de la balle avant le rebond au moins (uu), vitesse verticale au rebond (uu/s), toucher voulu
/// (saut/dodge ≤ 0,3 s).
pub const HALF_VOLLEY_MIN_APEX: f32 = 150.0;
pub const HALF_VOLLEY_MIN_BOUNCE_VZ: f32 = 300.0;
pub const HALF_VOLLEY_JUMP_TICKS: u64 = 36;
/// Cushion : la balle reste contre la voiture (distance < `CUSHION_STAY_DIST` uu) pendant au moins 0,3 s (ticks)
/// après le toucher, voiture à plat (`up.z` > `CUSHION_MIN_UP_Z`), sans rebond sur un mur dans les 6 ticks avant.
pub const CUSHION_STAY_DIST: f32 = 220.0;
pub const CUSHION_STAY_TICKS: u32 = 36;
pub const CUSHION_MIN_UP_Z: f32 = 0.5;
pub const CUSHION_WALL_BOUNCE_TICKS: u64 = 6;
/// Wall shot : sans saut depuis le mur, la balle doit partir à plus de `WALL_SHOT_MIN_GOAL_SPEED` vers le but adverse
/// (déjà exigé) et le toucher ne se fait pas dans son tiers défensif (|y| côté but au-delà de ce seuil, uu).
pub const WALL_SHOT_OWN_THIRD_Y: f32 = 1700.0;
/// Clear : balle sortie du tiers défensif (y côté son but sous −1 700 uu du centre, soit au-delà de ce seuil
/// vers le centre) ou balle précédemment cadrée vers son but.
pub const CLEAR_EXIT_Y: f32 = 1700.0;
/// Fast aerial : nez ≥ 30° au 2ᵉ saut (`forward.z` ≥ sin 30°), boost tenu ≥ 0,5 s après (ticks), gain d'altitude
/// ≥ 400 uu (ou balle cible z > 400 touchée ≤ 2 s) ; pas depuis un mur.
pub const FAST_AERIAL_MIN_NOSE_Z: f32 = 0.5;
pub const FAST_AERIAL_BOOST_TICKS: u32 = 60;
pub const FAST_AERIAL_MIN_GAIN_Z: f32 = 400.0;
pub const FAST_AERIAL_BALL_Z: f32 = 400.0;
pub const FAST_AERIAL_TOUCH_TICKS: u64 = 240;
/// Tornado spin : roulis tenu (|roll| ≥ 0,8, même signe) ≥ ce nombre de ticks (≈ 4 décisions), vol d'au moins 0,5 s.
pub const TORNADO_MIN_ROLL: f32 = 0.8;
pub const TORNADO_HOLD_ROLL_TICKS: u32 = 32;
pub const TORNADO_V6_MIN_TICKS: u32 = 60;
/// Recovery : entrée d'air roll/tangage ≠ 0 pendant la phase renversée ; atterrissage roues d'abord, vitesse au sol
/// ≥ 60 % déjà exigée ; au moins ce nombre de ticks d'entrée.
pub const RECOVERY_MIN_INPUT_TICKS: u32 = 6;
/// Half flip : vitesse horizontale 1,5 s après le dodge ≥ 80 % de celle avant (ticks) ; le cap inversé est atteint
/// dans la seconde.
pub const HALF_FLIP_KEEP: f32 = 0.8;
pub const HALF_FLIP_HEADING_TICKS: u64 = 120;
/// Double touch / backboard double : deux touchers séparés d'au moins 0,25 s (ticks), voiture en l'air entre les deux.
pub const DOUBLE_TOUCH_MIN_GAP_TICKS: u64 = 30;
/// Bounce dribble : apogée de la balle entre deux touchers > ce seuil (uu), au moins 3 rebonds contrôlés.
pub const BOUNCE_DRIBBLE_MIN_APEX: f32 = 200.0;
/// Dribble turn : portage d'au moins 1 s (ticks), voiture à plat (`up.z` > ce seuil), 45° de virage pendant le portage.
pub const DRIBBLE_TURN_MIN_TICKS: u32 = 120;
pub const DRIBBLE_TURN_MIN_UP_Z: f32 = 0.9;
/// Save : toucher à moins de cette distance de la ligne de but (uu).
pub const SAVE_MAX_LINE_DIST: f32 = 1800.0;
/// Ceiling shot : saut/dodge voulu au plus 1 s avant le toucher, balle envoyée vers le but adverse à plus de ce seuil.
pub const CEILING_SHOT_JUMP_TICKS: u64 = 120;
pub const CEILING_SHOT_MIN_GOAL_SPEED: f32 = 1000.0;
pub const REDIRECT_JUMP_TICKS: u64 = 60;
