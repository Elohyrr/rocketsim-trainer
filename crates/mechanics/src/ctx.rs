//! Contexte partagé par les automates du catalogue étendu (`ext_*`) : tout ce que le détecteur de base a déjà
//! calculé à ce tick, plus le jugement d'un toucher (vitesse de balle après coup).

use crate::{BallTick, CarTick, Events};

/// Dernier dodge connu (jamais effacé au retour au sol : on compare l'âge).
#[derive(Clone, Copy, Debug, Default)]
pub struct DodgeInfo {
    /// Tick interne du détecteur.
    pub tick: u64,
    /// Direction normalisée : avant (+) / arrière (−), latéral (+ droite).
    pub fwd: f32,
    pub lat: f32,
    /// Secteur 0–7 (0 avant, 2 droite, 4 arrière, 6 gauche).
    pub sector: u8,
}

/// Un toucher jugé : état de la voiture au toucher et vitesse de balle avant/après (maximum sur les
/// [`crate::thresholds::HIT_JUDGE_TICKS`] ticks qui suivent le toucher).
#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    /// Tick interne du toucher.
    pub tick: u64,
    /// Voiture et balle au tick du toucher.
    pub car: CarTick,
    pub ball: BallTick,
    /// Vitesse de balle (vecteur) juste avant le toucher et après (au tick de vitesse maximale du jugement).
    pub v_before: [f32; 3],
    pub v_after: [f32; 3],
    pub speed_before: f32,
    pub speed_after: f32,
    /// Ticks en l'air au toucher (0 si au sol).
    pub air_ticks: u32,
    /// Dernier dodge avant le toucher et son âge en ticks.
    pub dodge: Option<DodgeInfo>,
    pub dodge_age: Option<u64>,
    /// Ticks depuis que la voiture roulait sur un mur / le plafond (None si pas depuis le dernier sol).
    pub wall_age: Option<u64>,
    pub ceiling_age: Option<u64>,
    /// Dernier rebond de la balle sur un mur latéral (`false`) ou du fond (`true`) et son âge.
    pub ball_bounce_back: bool,
    pub ball_bounce_age: Option<u64>,
}

/// Résultats des automates de base (v1/v2) à ce tick, pour s'y appuyer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Base {
    pub flip_reset: bool,
    pub flick: bool,
    pub wave_dash: bool,
    pub air_dribble: bool,
    pub aerial_touch: bool,
    pub ceiling_shot: bool,
    pub wall_shot: bool,
    pub double_touch: bool,
    /// Le rebond de balle entre les deux touchers était sur le mur du fond (backboard).
    pub double_touch_back: bool,
    pub dribble: bool,
    /// Un dribble validé est en cours (ou fini il y a ≤ [`crate::thresholds::FLICK_DODGE_WINDOW_TICKS`]).
    pub dribble_active: bool,
    /// Dernier tick où le dribble était valide.
    pub dribble_last_ok: Option<u64>,
}

/// Tout ce qu'un automate étendu peut lire à un tick.
pub struct Ctx<'a> {
    /// Tick interne (1 = premier tick après l'initialisation).
    pub n: u64,
    pub car: &'a CarTick,
    pub ball: &'a BallTick,
    pub prev_car: &'a CarTick,
    pub prev_ball: &'a BallTick,
    pub ev: Events,
    /// Nouveau toucher à ce tick (contacts continus fusionnés).
    pub touch: bool,
    /// Le toucher jugé à ce tick (4 ticks après le toucher), s'il y en a un.
    pub hit: Option<&'a Hit>,
    /// Voiture au sol hors balle porteuse ; sur un mur ; au plafond ; vient d'atterrir.
    pub on_terrain: bool,
    pub on_wall: bool,
    pub on_ceiling: bool,
    pub landed: bool,
    /// Ticks en l'air depuis le dernier contact roues-terrain.
    pub air_ticks: u32,
    /// Balle − voiture, et ses composantes (haut, avant, côté) dans le repère voiture.
    pub rel: [f32; 3],
    pub rel_up: f32,
    pub rel_fwd: f32,
    pub rel_side: f32,
    pub dist: f32,
    pub hspeed: f32,
    pub ball_speed: f32,
    /// Dernier dodge connu et son âge en ticks.
    pub dodge: Option<DodgeInfo>,
    pub dodge_age: Option<u64>,
    /// Dodge à ce tick.
    pub dodge_now: bool,
    pub base: Base,
}
