//! Événements par tick et automates du catalogue.

use crate::ctx::{Base, Ctx, DodgeInfo, Hit};
use crate::detail::{Details, Out};
use crate::thresholds::*;
use crate::{BallTick, CarTick, Counts, Detail, MECH_COUNT, Mechanic, Records, Support};

/// Événements discrets d'un tick (masque de bits, spec §4.1). La direction d'un dodge est donnée à part par
/// [`Detector::dodge_sector`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Events(pub u32);

impl Events {
    /// `on_ground` passe à vrai (SOL).
    pub const GROUND: u32 = 1 << 0;
    /// `on_ground` passe à faux (AIR).
    pub const AIR: u32 = 1 << 1;
    pub const JUMP: u32 = 1 << 2;
    pub const DOUBLE_JUMP: u32 = 1 << 3;
    pub const DODGE: u32 = 1 << 4;
    /// Tangage opposé au dodge dans les 0,15 s (ANNUL).
    pub const CANCEL: u32 = 1 << 5;
    pub const TOUCH_GROUND: u32 = 1 << 6;
    pub const TOUCH_AIR: u32 = 1 << 7;
    /// Toucher par le toit (balle au-dessus du toit) ; en plus de TOUCH_GROUND ou TOUCH_AIR.
    pub const TOUCH_ROOF: u32 = 1 << 8;
    /// Roues qui se posent sur la balle (en l'air, sans toucher enregistré par RocketSim).
    pub const TOUCH_WHEELS: u32 = 1 << 9;
    /// Flip rendu en l'air par la balle.
    pub const RESET: u32 = 1 << 10;
    /// Roues qui se posent sur un mur.
    pub const WALL: u32 = 1 << 11;
    /// Roues qui se posent au plafond.
    pub const CEILING: u32 = 1 << 12;
    pub const SUPERSONIC: u32 = 1 << 13;
    pub const BOOST_ON: u32 = 1 << 14;
    pub const BOOST_OFF: u32 = 1 << 15;
    /// Le nombre de roues en contact a changé (ROUES_N, valeur dans `CarTick::wheels_contact`).
    pub const WHEELS: u32 = 1 << 16;
    /// Rebond de la balle sur un mur (sert au double touch).
    pub const BALL_WALL_BOUNCE: u32 = 1 << 17;

    #[inline]
    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Ce qu'on garde du tick précédent.
#[derive(Clone, Copy, Debug, Default)]
struct Prev {
    on_ground: bool,
    on_terrain: bool,
    on_wall: bool,
    on_ceiling: bool,
    has_jumped: bool,
    has_double_jumped: bool,
    has_flipped: bool,
    is_flipping: bool,
    boosting: bool,
    supersonic: bool,
    wheels: u8,
    ball_hit_tick: Option<u64>,
    hspeed: f32,
    heading: [f32; 2],
    ball_vel: [f32; 3],
}

/// Dodge en cours d'analyse (speed flip, wave dash, half flip).
#[derive(Clone, Copy, Debug, Default)]
struct Dodge {
    tick: u64,
    fwd: f32,
    lat: f32,
    cancelled: bool,
    rolled: bool,
    heading: [f32; 2],
    speed_before: f32,
}

/// Détecteur pour **une** voiture. Appeler [`Detector::tick`] à chaque tick de physique, et
/// [`Detector::reset`] à chaque nouvel épisode ou téléportation.
#[derive(Clone, Debug, Default)]
pub struct Detector {
    /// Horloge interne (nombre d'appels à `tick`), indépendante de `CarTick::tick`.
    n: u64,
    init: bool,
    prev: Prev,
    events: Events,
    dodge_sector: Option<u8>,
    air_ticks: u32,
    last_raw_hit: Option<u64>,
    reset_pending: u8,
    // Air dribble.
    ad_last: Option<u64>,
    ad_boost: bool,
    ad_counted: bool,
    ad_last_counted: bool,
    // Plafond, mur.
    ceil_last: Option<u64>,
    wall_last: Option<u64>,
    // Double touch.
    dt_first: Option<u64>,
    dt_bounced: bool,
    dt_back: bool,
    // Dribble, flick.
    dr_run: u32,
    dr_gap: u32,
    dr_counted: bool,
    dr_last_ok: Option<u64>,
    fl_dodge: Option<u64>,
    fl_hit: bool,
    // Dodge.
    dodge: Option<Dodge>,
    // v2 : mode strict, sens d'attaque, décollage volontaire, saut depuis le mur.
    mode: Mode,
    attack_neg: bool,
    vol_takeoff: bool,
    wall_jumped: bool,
    /// Wall shot v2 en attente : tick du toucher et vitesse vers le but adverse juste avant.
    ws_pending: Option<(u64, f32)>,
    dr_touches: u32,
    dr_last_touch: Option<u64>,
    dr_dist: f32,
    dr_valid: bool,
    // Catalogue étendu : contexte commun (dernier dodge, âges mur/plafond, rebond de balle), jugement des
    // touchers, détails, records et groupes d'automates.
    prev_car: CarTick,
    prev_ball: BallTick,
    last_dodge: Option<DodgeInfo>,
    wall_seen: Option<u64>,
    ceil_seen: Option<u64>,
    bounce: Option<(u64, bool)>,
    hit_pending: Option<Hit>,
    details: Details,
    records: Records,
    g_hit: crate::ext_hit::Group,
    g_carry: crate::ext_carry::Group,
    g_move: crate::ext_move::Group,
}

/// Version des règles de détection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Règles de la v2 (strictes : actions intentionnelles seulement).
    #[default]
    V2,
    /// Règles d'origine (PR #16), gardées pour comparer ou revenir en arrière.
    V1,
}

#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
fn norm2(a: [f32; 3]) -> f32 {
    dot(a, a)
}

#[inline]
fn heading(f: [f32; 3]) -> [f32; 2] {
    let n = (f[0] * f[0] + f[1] * f[1]).sqrt();
    if n < 1e-3 {
        [0.0, 0.0]
    } else {
        [f[0] / n, f[1] / n]
    }
}

#[inline]
fn within(since: Option<u64>, now: u64, window: u64) -> bool {
    since.is_some_and(|t| now - t <= window)
}

/// Rebond de la balle sur un mur latéral ou du fond : vitesse normale qui change de signe près du mur.
#[inline]
fn ball_wall_bounce(prev_vel: [f32; 3], ball: &BallTick) -> Option<bool> {
    let near_x = ARENA_SIDE_X - BALL_RADIUS - WALL_BOUNCE_MARGIN;
    let near_y = ARENA_BACK_Y - BALL_RADIUS - WALL_BOUNCE_MARGIN;
    let bx = ball.pos[0].abs() > near_x
        && prev_vel[0].abs() > WALL_BOUNCE_MIN_SPEED
        && prev_vel[0].signum() == ball.pos[0].signum()
        && prev_vel[0] * ball.vel[0] < 0.0;
    let by = ball.pos[1].abs() > near_y
        && prev_vel[1].abs() > WALL_BOUNCE_MIN_SPEED
        && prev_vel[1].signum() == ball.pos[1].signum()
        && prev_vel[1] * ball.vel[1] < 0.0;
    if by {
        Some(true)
    } else if bx {
        Some(false)
    } else {
        None
    }
}

impl Detector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Détecteur d'une version donnée des règles.
    pub fn with_mode(mode: Mode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Sens d'attaque : l'équipe bleue (défaut) marque en y > 0, l'orange en y < 0. Sert au wall shot v2.
    pub fn set_blue(&mut self, blue: bool) {
        self.attack_neg = !blue;
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Oublie tout l'état (nouvel épisode, téléportation) ; garde la version et le sens d'attaque.
    /// Le tick suivant sert seulement à initialiser.
    pub fn reset(&mut self) {
        *self = Self {
            mode: self.mode,
            attack_neg: self.attack_neg,
            details: self.details,
            records: std::mem::take(&mut self.records),
            ..Self::default()
        };
    }

    /// Dernier détail (valeur, support) de la mécanique `m` ; nul tant qu'elle n'a pas été vue.
    #[inline]
    pub fn detail(&self, m: Mechanic) -> Detail {
        self.details.0[m.index()]
    }

    /// Records (meilleure valeur par mécanique) depuis le dernier [`Detector::take_records`].
    pub fn records(&self) -> &Records {
        &self.records
    }

    /// Records depuis le dernier appel (remis à zéro).
    pub fn take_records(&mut self) -> Records {
        std::mem::take(&mut self.records)
    }

    /// Vitesse de la balle vers le but adverse (+ = vers le but).
    #[inline]
    fn toward_goal(&self, vel: [f32; 3]) -> f32 {
        if self.attack_neg { -vel[1] } else { vel[1] }
    }

    /// Événements du dernier tick traité.
    #[inline]
    pub fn events(&self) -> Events {
        self.events
    }

    /// Secteur du dodge du dernier tick (si `Events::DODGE`) : 0 = avant, 1 = avant-droite, 2 = droite,
    /// 3 = arrière-droite, 4 = arrière, 5 = arrière-gauche, 6 = gauche, 7 = avant-gauche.
    #[inline]
    pub fn dodge_sector(&self) -> Option<u8> {
        self.dodge_sector
    }

    /// Traite un tick de physique pour cette voiture et incrémente `counts` pour chaque mécanique reconnue.
    #[inline]
    pub fn tick(&mut self, car: &CarTick, ball: &BallTick, counts: &mut Counts) {
        self.n += 1;
        let n = self.n;

        // Géométrie voiture-balle.
        let rel = sub(ball.pos, car.pos);
        let dist2 = norm2(rel);
        let rel_up = dot(rel, car.up);
        let hspeed = (car.vel[0] * car.vel[0] + car.vel[1] * car.vel[1]).sqrt();
        let head = heading(car.forward);

        // La balle porte les roues : loin du terrain, balle proche côté roues.
        let ball_supported = !car.world_contact
            && car.pos[2] > RESET_MIN_Z
            && dist2 < RESET_BALL_DIST * RESET_BALL_DIST
            && rel_up < 0.0;
        let on_terrain = car.on_ground && !ball_supported;
        let on_ceiling = on_terrain && car.up[2] < CEILING_UP_Z && car.pos[2] > CEILING_MIN_Z;
        let on_wall = on_terrain && car.up[2].abs() < WALL_MAX_ABS_UP_Z && car.pos[2] > WALL_MIN_Z;

        if !self.init {
            self.init = true;
            self.events = Events::default();
            self.dodge_sector = None;
            self.air_ticks = if on_terrain {
                0
            } else {
                (car.air_time.max(0.0) * crate::TICK_HZ) as u32
            };
            if on_ceiling {
                self.ceil_last = Some(n);
            }
            if on_wall {
                self.wall_last = Some(n);
            }
            self.save_prev(car, ball, on_terrain, on_wall, on_ceiling, hspeed, head);
            return;
        }

        let p = self.prev;
        let mut ev = 0u32;
        self.dodge_sector = None;

        // --- Événements simples.
        if car.on_ground != p.on_ground {
            ev |= if car.on_ground {
                Events::GROUND
            } else {
                Events::AIR
            };
        }
        if car.has_jumped && !p.has_jumped {
            ev |= Events::JUMP;
        }
        if car.has_double_jumped && !p.has_double_jumped {
            ev |= Events::DOUBLE_JUMP;
        }
        if car.supersonic && !p.supersonic {
            ev |= Events::SUPERSONIC;
        }
        if car.boosting != p.boosting {
            ev |= if car.boosting {
                Events::BOOST_ON
            } else {
                Events::BOOST_OFF
            };
        }
        if car.wheels_contact != p.wheels {
            ev |= Events::WHEELS;
        }
        if on_wall && !p.on_wall {
            ev |= Events::WALL;
        }
        if on_ceiling && !p.on_ceiling {
            ev |= Events::CEILING;
        }
        if let Some(back) = ball_wall_bounce(p.ball_vel, ball) {
            ev |= Events::BALL_WALL_BOUNCE;
            self.bounce = Some((n, back));
        }
        let mut base = Base::default();

        // Temps en l'air depuis le dernier contact roues-terrain (un flip reset ne le remet pas à zéro).
        let landed = on_terrain && !p.on_terrain;
        if on_terrain {
            self.air_ticks = 0;
        } else {
            self.air_ticks = self.air_ticks.saturating_add(1);
        }

        if on_wall {
            self.wall_seen = Some(n);
        } else if on_terrain {
            self.wall_seen = None;
        }
        if on_ceiling {
            self.ceil_seen = Some(n);
        } else if on_terrain {
            self.ceil_seen = None;
        }

        // Décollage volontaire (v2) : un saut pressé depuis le sol ou le mur (ou ≤ 6 ticks après l'avoir quitté),
        // pas un simple départ en roulant. Le mur sert au wall shot : saut depuis le mur.
        if on_terrain && !car.has_jumped {
            self.vol_takeoff = false;
            self.wall_jumped = false;
        }
        if ev & Events::JUMP != 0 && (p.on_terrain || self.air_ticks <= VOLUNTARY_JUMP_TICKS) {
            self.vol_takeoff = true;
            if p.on_wall || on_wall || within(self.wall_last, n, VOLUNTARY_JUMP_TICKS as u64) {
                self.wall_jumped = true;
            }
        }

        // Décollage poussé au boost en gardant la balle sur le capot (wall to air dribble) : intentionnel aussi.
        if !on_terrain
            && self.air_ticks <= VOLUNTARY_JUMP_TICKS
            && car.boosting
            && rel_up > 0.0
            && dist2 < CARRIED_BALL_DIST * CARRIED_BALL_DIST
        {
            self.vol_takeoff = true;
        }

        // --- Dodge et annulation.
        let dodge = (car.has_flipped && !p.has_flipped)
            || (car.is_flipping && !p.is_flipping && !car.has_flipped);
        if dodge {
            ev |= Events::DODGE;
            let (mut f, mut l) = (car.flip_rel_torque[0], car.flip_rel_torque[1]);
            let mut m = (f * f + l * l).sqrt();
            if m < 1e-3 {
                // Repli : direction tirée des commandes (tangage −1 = avant).
                f = -car.pitch_input;
                l = car.roll_input;
                m = (f * f + l * l).sqrt();
            }
            let (f, l) = if m < 1e-3 { (0.0, 0.0) } else { (f / m, l / m) };
            let sector = if m < 1e-3 {
                0
            } else {
                let a = l.atan2(f); // 0 = avant, +π/2 = droite
                (((a / core::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8)) as u8
            };
            self.dodge_sector = Some(sector);
            self.last_dodge = Some(DodgeInfo {
                tick: n,
                fwd: f,
                lat: l,
                sector,
            });
            self.dodge = Some(Dodge {
                tick: n,
                fwd: f,
                lat: l,
                cancelled: false,
                rolled: false,
                heading: p.heading,
                speed_before: p.hspeed,
            });
        } else if let Some(d) = self.dodge.as_mut() {
            let dt = n - d.tick;
            if !d.cancelled
                && dt <= CANCEL_WINDOW_TICKS
                && d.fwd.abs() > DIAGONAL_MIN_COMPONENT
                && car.pitch_input * d.fwd.signum() >= CANCEL_MIN_PITCH
            {
                d.cancelled = true;
                ev |= Events::CANCEL;
            } else if d.cancelled && car.roll_input.abs() >= HALF_FLIP_MIN_ROLL {
                d.rolled = true;
            }
        }

        // --- Touchers.
        let raw_hit = car.ball_hit_tick.is_some() && car.ball_hit_tick != p.ball_hit_tick;
        let touch = raw_hit && !within(self.last_raw_hit, n, TOUCH_MERGE_TICKS);
        if raw_hit {
            self.last_raw_hit = Some(n);
        }
        if touch {
            counts.inc(Mechanic::Touch);
            ev |= if car.on_ground {
                Events::TOUCH_GROUND
            } else {
                Events::TOUCH_AIR
            };
            if rel_up > 0.7 * dist2.sqrt() {
                ev |= Events::TOUCH_ROOF;
            }
        }
        if car.wheels_contact > 0 && p.wheels == 0 && !on_terrain && ball_supported {
            ev |= Events::TOUCH_WHEELS;
        }

        // --- Flip reset : on_ground passe à vrai porté par la balle, drapeaux qui retombent.
        if car.on_ground
            && !p.on_ground
            && ball_supported
            && (p.has_jumped || p.has_double_jumped || p.has_flipped)
        {
            self.reset_pending = RESET_CONFIRM_TICKS;
        }
        if self.reset_pending > 0 {
            if car.world_contact || on_terrain {
                self.reset_pending = 0;
            } else if !car.has_jumped && !car.has_double_jumped && !car.has_flipped {
                ev |= Events::RESET;
                counts.inc(Mechanic::FlipReset);
                base.flip_reset = true;
                self.reset_pending = 0;
            } else {
                self.reset_pending -= 1;
            }
        }

        // --- Toucher aérien.
        let air_touch = touch && !car.on_ground && ball.pos[2] > AERIAL_BALL_MIN_Z;
        // Toucher aérien « de base » ; en v2 il faut en plus une intention (décollage volontaire, ou chaîne de
        // touchers contrôlés au boost : air dribble).
        let (core, intent) = match self.mode {
            Mode::V1 => (air_touch && self.air_ticks >= AERIAL_MIN_AIR_TICKS, true),
            Mode::V2 => (
                air_touch
                    && car.pos[2] > AERIAL_CAR_MIN_Z
                    && ball.pos[2] > AERIAL_V2_BALL_MIN_Z
                    && self.air_ticks >= AERIAL_V2_MIN_AIR_TICKS,
                self.vol_takeoff,
            ),
        };

        // --- Air dribble : ≥ 2 touchers aériens hauts, < 1 s d'écart, boost entre les deux, sans sol.
        if on_terrain {
            self.ad_last = None;
            self.ad_counted = false;
            self.ad_boost = false;
            self.ad_last_counted = false;
        }
        if car.boosting {
            self.ad_boost = true;
        }
        let mut aerial = core && intent;
        if core && ball.pos[2] > AIR_DRIBBLE_BALL_MIN_Z {
            if within(self.ad_last, n, AIR_DRIBBLE_MAX_GAP_TICKS) {
                if self.ad_boost {
                    // Chaîne contrôlée : ce toucher compte, et le précédent aussi s'il ne l'était pas encore.
                    aerial = true;
                    if !self.ad_last_counted {
                        counts.inc(Mechanic::AerialTouch);
                    }
                    if !self.ad_counted {
                        counts.inc(Mechanic::AirDribble);
                        base.air_dribble = true;
                        self.ad_counted = true;
                    }
                }
            } else {
                self.ad_counted = false;
            }
            self.ad_last = Some(n);
            self.ad_last_counted = aerial;
            self.ad_boost = false;
        }
        if aerial {
            base.aerial_touch = true;
            counts.inc(Mechanic::AerialTouch);
        }

        // --- Ceiling shot : roues au plafond puis toucher en l'air avec le flip disponible dans les 3 s.
        if on_ceiling {
            self.ceil_last = Some(n);
        } else if on_terrain {
            self.ceil_last = None;
        }
        if air_touch && car.has_flip_or_jump && within(self.ceil_last, n, CEILING_SHOT_WINDOW_TICKS)
        {
            counts.inc(Mechanic::CeilingShot);
            base.ceiling_shot = true;
            self.note(Mechanic::CeilingShot, norm2(ball.vel).sqrt());
            self.ceil_last = None;
        }

        // --- Wall shot : toucher sur le mur ou peu après l'avoir quitté (sans retour au sol).
        if on_wall {
            self.wall_last = Some(n);
        } else if on_terrain {
            self.wall_last = None;
        }
        if touch && within(self.wall_last, n, WALL_SHOT_WINDOW_TICKS) {
            if self.mode == Mode::V1 || self.wall_jumped {
                counts.inc(Mechanic::WallShot);
                base.wall_shot = true;
                self.note(Mechanic::WallShot, norm2(ball.vel).sqrt());
            } else {
                self.ws_pending = Some((n, self.toward_goal(p.ball_vel)));
            }
            self.wall_last = None;
        }
        // v2 : le toucher doit envoyer la balle vers le but adverse à plus de 1 000 uu/s (gain imputable au toucher).
        if let Some((t, before)) = self.ws_pending {
            let after = self.toward_goal(ball.vel);
            if after > WALL_SHOT_MIN_GOAL_SPEED && after - before >= WALL_SHOT_MIN_GAIN {
                counts.inc(Mechanic::WallShot);
                base.wall_shot = true;
                self.note(Mechanic::WallShot, norm2(ball.vel).sqrt());
                self.ws_pending = None;
            } else if n - t >= WALL_SHOT_JUDGE_TICKS {
                self.ws_pending = None;
            }
        }

        // --- Double touch : toucher en l'air, rebond mur de la balle, second toucher en l'air.
        if on_terrain {
            self.dt_first = None;
            self.dt_bounced = false;
        }
        if ev & Events::BALL_WALL_BOUNCE != 0 && self.dt_first.is_some() {
            self.dt_bounced = true;
            self.dt_back = self.bounce.is_some_and(|(_, b)| b);
        }
        if air_touch {
            if self.dt_bounced && within(self.dt_first, n, DOUBLE_TOUCH_WINDOW_TICKS) {
                counts.inc(Mechanic::DoubleTouch);
                base.double_touch = true;
                base.double_touch_back = self.dt_back;
                self.note(Mechanic::DoubleTouch, norm2(ball.vel).sqrt());
                self.dt_first = None;
            } else {
                self.dt_first = Some(n);
            }
            self.dt_bounced = false;
        }

        // --- Dribble : balle posée sur le toit, voiture au sol et à plat, maintenu 0,5 s.
        let fwd = dot(rel, car.forward);
        let side = dot(rel, car.right);
        let rv = norm2(sub(ball.vel, car.vel));
        let dribble_ok = car.on_ground
            && car.up[2] > DRIBBLE_MIN_UP_Z
            && (DRIBBLE_UP_MIN..=DRIBBLE_UP_MAX).contains(&rel_up)
            && fwd.abs() < DRIBBLE_MAX_ABS_FWD
            && side.abs() < DRIBBLE_MAX_ABS_SIDE
            && rv < DRIBBLE_MAX_REL_SPEED * DRIBBLE_MAX_REL_SPEED;
        if dribble_ok {
            self.dr_run += 1;
            self.dr_gap = 0;
            if self.mode == Mode::V1 {
                if self.dr_run >= DRIBBLE_MIN_TICKS {
                    self.dr_last_ok = Some(n);
                    if !self.dr_counted {
                        counts.inc(Mechanic::Dribble);
                        base.dribble = true;
                        self.dr_counted = true;
                    }
                }
            } else {
                // v2 : touchers distincts (≥ 10 ticks d'écart) et distance parcourue pendant le contrôle.
                self.dr_dist += hspeed / crate::TICK_HZ;
                if touch && !within(self.dr_last_touch, n, DRIBBLE_TOUCH_SEP_TICKS) {
                    self.dr_touches += 1;
                    self.dr_last_touch = Some(n);
                }
                let by_touches = self.dr_touches >= DRIBBLE_V2_MIN_TOUCHES;
                let by_time = self.dr_run >= DRIBBLE_V2_LONG_TICKS
                    && self.dr_touches >= DRIBBLE_V2_LONG_MIN_TOUCHES;
                if (by_touches || by_time) && self.dr_dist >= DRIBBLE_V2_MIN_DIST {
                    self.dr_valid = true;
                }
                if self.dr_valid {
                    self.dr_last_ok = Some(n);
                    if !self.dr_counted {
                        counts.inc(Mechanic::Dribble);
                        base.dribble = true;
                        self.dr_counted = true;
                    }
                }
            }
        } else {
            self.dr_gap += 1;
            if self.dr_gap > DRIBBLE_GAP_TICKS {
                self.dr_run = 0;
                self.dr_counted = false;
                self.dr_touches = 0;
                self.dr_last_touch = None;
                self.dr_dist = 0.0;
                self.dr_valid = false;
            }
        }

        // --- Flick : dribble, dodge dans les 0,4 s, toucher et balle rapide dans les 0,3 s.
        if dodge && within(self.dr_last_ok, n, FLICK_DODGE_WINDOW_TICKS) {
            self.fl_dodge = Some(n);
            self.fl_hit = false;
        }
        if let Some(d) = self.fl_dodge {
            if n - d > FLICK_HIT_WINDOW_TICKS {
                self.fl_dodge = None;
            } else {
                if raw_hit {
                    self.fl_hit = true;
                }
                if self.fl_hit && norm2(ball.vel) > FLICK_MIN_BALL_SPEED * FLICK_MIN_BALL_SPEED {
                    counts.inc(Mechanic::Flick);
                    base.flick = true;
                    self.note(Mechanic::Flick, norm2(ball.vel).sqrt());
                    self.fl_dodge = None;
                }
            }
        }

        // --- Fin d'un dodge : speed flip, wave dash, half flip (jugés au retour au sol).
        if let Some(d) = self.dodge {
            let dt = n - d.tick;
            if landed && dt >= 1 {
                if dt <= WAVE_DASH_MAX_LAND_TICKS && hspeed - d.speed_before > WAVE_DASH_MIN_GAIN {
                    counts.inc(Mechanic::WaveDash);
                    base.wave_dash = true;
                }
                let diagonal =
                    d.fwd > DIAGONAL_MIN_COMPONENT && d.lat.abs() > DIAGONAL_MIN_COMPONENT;
                if diagonal
                    && d.cancelled
                    && dt <= SPEED_FLIP_MAX_LAND_TICKS
                    && hspeed > SPEED_FLIP_MIN_LAND_SPEED
                {
                    counts.inc(Mechanic::SpeedFlip);
                }
                let turned = d.heading[0] * head[0] + d.heading[1] * head[1];
                if d.fwd < HALF_FLIP_MAX_FWD
                    && d.cancelled
                    && d.rolled
                    && dt <= HALF_FLIP_MAX_LAND_TICKS
                    && turned < HALF_FLIP_MAX_HEADING_COS
                {
                    counts.inc(Mechanic::HalfFlip);
                }
                self.dodge = None;
            } else if dt > DODGE_MEMORY_TICKS {
                self.dodge = None;
            }
        }

        base.dribble_active = within(self.dr_last_ok, n, FLICK_DODGE_WINDOW_TICKS);
        base.dribble_last_ok = self.dr_last_ok;

        // --- Jugement des touchers : vitesse de balle maximale dans les ticks qui suivent.
        if touch {
            self.hit_pending = Some(Hit {
                tick: n,
                car: *car,
                ball: *ball,
                v_before: p.ball_vel,
                v_after: ball.vel,
                speed_before: norm2(p.ball_vel).sqrt(),
                speed_after: norm2(ball.vel).sqrt(),
                air_ticks: self.air_ticks,
                dodge: self.last_dodge,
                dodge_age: self.last_dodge.map(|d| n - d.tick),
                wall_age: self.wall_seen.map(|t| n - t),
                ceiling_age: self.ceil_seen.map(|t| n - t),
                ball_bounce_back: self.bounce.is_some_and(|(_, b)| b),
                ball_bounce_age: self.bounce.map(|(t, _)| n - t),
            });
        } else if let Some(h) = self.hit_pending.as_mut() {
            let sp = norm2(ball.vel).sqrt();
            if sp > h.speed_after {
                h.speed_after = sp;
                h.v_after = ball.vel;
            }
        }
        let judged = match self.hit_pending {
            Some(h) if n - h.tick >= HIT_JUDGE_TICKS => {
                self.hit_pending = None;
                Some(h)
            }
            _ => None,
        };

        // --- Catalogue étendu.
        let cx = Ctx {
            n,
            car,
            ball,
            prev_car: &self.prev_car,
            prev_ball: &self.prev_ball,
            ev: Events(ev),
            touch,
            hit: judged.as_ref(),
            on_terrain,
            on_wall,
            on_ceiling,
            landed,
            air_ticks: self.air_ticks,
            rel,
            rel_up,
            rel_fwd: dot(rel, car.forward),
            rel_side: dot(rel, car.right),
            dist: dist2.sqrt(),
            hspeed,
            ball_speed: norm2(ball.vel).sqrt(),
            dodge: self.last_dodge,
            dodge_age: self.last_dodge.map(|d| n - d.tick),
            dodge_now: dodge,
            base,
            attack_neg: self.attack_neg,
        };
        let mut out = Out {
            counts,
            details: &mut self.details.0,
            records: &mut self.records,
        };
        self.g_hit.tick(&cx, &mut out);
        self.g_carry.tick(&cx, &mut out);
        self.g_move.tick(&cx, &mut out);

        self.events = Events(ev);
        self.save_prev(car, ball, on_terrain, on_wall, on_ceiling, hspeed, head);
    }

    /// Enregistre valeur et record d'une mécanique de base (compteur déjà incrémenté).
    #[inline]
    fn note(&mut self, m: Mechanic, value: f32) {
        self.details.0[m.index()] = Detail {
            value,
            support: Support::None,
        };
        self.records.observe(m, value);
    }

    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn save_prev(
        &mut self,
        car: &CarTick,
        ball: &BallTick,
        on_terrain: bool,
        on_wall: bool,
        on_ceiling: bool,
        hspeed: f32,
        head: [f32; 2],
    ) {
        self.prev_car = *car;
        self.prev_ball = *ball;
        self.prev = Prev {
            on_ground: car.on_ground,
            on_terrain,
            on_wall,
            on_ceiling,
            has_jumped: car.has_jumped,
            has_double_jumped: car.has_double_jumped,
            has_flipped: car.has_flipped,
            is_flipping: car.is_flipping,
            boosting: car.boosting,
            supersonic: car.supersonic,
            wheels: car.wheels_contact,
            ball_hit_tick: car.ball_hit_tick,
            hspeed,
            heading: head,
            ball_vel: ball.vel,
        };
    }
}
