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

/// Speed flip : retour au sol au plus 0,8 s après le dodge, vitesse horizontale > 1 900 uu/s.
pub const SPEED_FLIP_MAX_LAND_TICKS: u64 = 96;
pub const SPEED_FLIP_MIN_LAND_SPEED: f32 = 1900.0;

/// Wave dash : retour au sol au plus 6 ticks après le dodge, gain de vitesse horizontale > 250 uu/s.
pub const WAVE_DASH_MAX_LAND_TICKS: u64 = 6;
pub const WAVE_DASH_MIN_GAIN: f32 = 250.0;

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
/// … roulis commandé d'au moins cette amplitude (sauf voiture inversée)…
pub const STALL_MIN_ROLL: f32 = 0.5;
/// … vitesse avant le dodge d'au moins (uu/s)…
pub const STALL_MIN_SPEED_BEFORE: f32 = 300.0;
/// … puis vitesse tombée sous cette fraction de la vitesse avant le dodge (chute > 40 %)…
pub const STALL_MAX_SPEED_RATIO: f32 = 0.6;
/// … au plus 0,3 s (ticks) après le dodge.
pub const STALL_WINDOW_TICKS: u64 = 36;
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
