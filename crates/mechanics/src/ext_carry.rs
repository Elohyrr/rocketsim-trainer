//! Automates du catalogue étendu : groupe « carry » (variantes de dribble et de flick).
//!
//! « Balle portée » (fenêtre toit) : la même géométrie que le dribble de base — balle à
//! [`DRIBBLE_UP_MIN`]..[`DRIBBLE_UP_MAX`] au-dessus de la voiture (repère voiture), |avant| <
//! [`DRIBBLE_MAX_ABS_FWD`], |côté| < [`DRIBBLE_MAX_ABS_SIDE`], vitesse relative < [`DRIBBLE_MAX_REL_SPEED`] —,
//! avec la même tolérance aux petits trous ([`DRIBBLE_GAP_TICKS`]). Chaque portage est suivi ici, l'automate
//! dribble de base n'est pas modifié. Chaque variante de dribble compte une fois par séquence de portage.
//!
//! - **HoodDribble** : quand le dribble de base est validé (`cx.base.dribble`), la moyenne de la position
//!   avant de la balle pendant ce portage au sol dépasse [`HOOD_MIN_MEAN_FWD`] (balle sur le capot). Compté en
//!   plus de `Dribble` ; sinon c'est un dribble « toit » (seul `Dribble`). Approximation : le capot est
//!   repéré par la seule position longitudinale, sans modèle de hitbox.
//! - **ReverseDribble** : balle portée, voiture au sol à plat qui recule (vel·forward <
//!   [`REVERSE_MAX_FWD_SPEED`]) pendant ≥ [`REVERSE_MIN_TICKS`], ≥ [`REVERSE_MIN_TOUCHES`] touchers distincts
//!   (≥ [`DRIBBLE_TOUCH_SEP_TICKS`] d'écart) et ≥ [`REVERSE_MIN_DIST`] parcourus. Approximation : la balle
//!   doit rester dans la fenêtre toit (une balle poussée devant le pare-chocs arrière n'est pas vue).
//! - **WallDribble** : balle portée, voiture sur un mur (|up.z| < 0,5, z > 200) pendant ≥
//!   [`WALL_DRIBBLE_MIN_TICKS`], ≥ [`WALL_DRIBBLE_MIN_DIST`] parcourus (vitesse 3D intégrée) et ≥
//!   [`SURFACE_DRIBBLE_MIN_TOUCHES`] toucher. Approximation : pas de distinction avec une balle qui suit la
//!   voiture sans être contrôlée si elle est touchée au moins une fois.
//! - **CeilingDribble** : balle portée, voiture au plafond (up.z < −0,9, z > 1 900) pendant ≥
//!   [`CEILING_DRIBBLE_MIN_TICKS`] et ≥ [`SURFACE_DRIBBLE_MIN_TOUCHES`] toucher ; pas de condition de
//!   distance.
//! - **BounceDribble** : ≥ [`BOUNCE_DRIBBLE_MIN_TOUCHES`] touchers successifs espacés de
//!   [`BOUNCE_DRIBBLE_MIN_GAP_TICKS`] à [`BOUNCE_DRIBBLE_MAX_GAP_TICKS`], voiture au sol à plat à plus de
//!   [`BOUNCE_DRIBBLE_MIN_CAR_SPEED`], balle sous [`BOUNCE_DRIBBLE_BALL_MAX_Z`] et à moins de
//!   [`BOUNCE_DRIBBLE_MAX_DIST`] de la voiture à chaque toucher, un rebond de la balle au sol entre deux
//!   touchers (vitesse verticale qui change de signe, centre sous [`BALL_FLOOR_BOUNCE_MAX_Z`]), chaque toucher
//!   doux (balle < [`BOUNCE_DRIBBLE_MAX_BALL_SPEED`] après, jugé 4 ticks plus tard). Émis au jugement du
//!   3ᵉ toucher, une fois par série ; un toucher qui ne remplit pas les conditions casse la série.
//!   Approximation : la distance balle-voiture n'est vérifiée qu'aux touchers.
//!
//! Variantes de flick, classées quand le flick de base se déclenche (`cx.base.flick`) avec le dernier dodge
//! (`cx.dodge`) ; valeur = vitesse de balle à ce tick (record) :
//! - **Flick45** : secteur 1 ou 7 (dodge avant-diagonal). **Flick90** : secteur 2 ou 6 (latéral).
//!   **MustyFlick** : dodge arrière (composante avant < [`MUSTY_MAX_FWD`], secteurs 3/4/5) avec la balle
//!   au-dessus de la voiture au moment du dodge (hauteur repère voiture > [`MUSTY_MIN_BALL_UP`]).
//!   Exclusifs entre eux, en plus de `Flick` ; le dodge droit devant (secteur 0) reste un simple `Flick`.
//!   Approximation : la bascule du nez avant le backflip (signature du musty) n'est pas vérifiée.
//! - **DelayedFlick** (en plus) : le saut a été pressé balle portée (dribble actif) et le dodge vient ≥
//!   [`DELAYED_FLICK_MIN_TICKS`] après, balle encore portée (à moins de [`CARRIED_BALL_DIST`], au-dessus de la
//!   voiture) au moment du dodge. Approximation : portage vérifié aux deux bouts seulement ; le flick de base
//!   impose le dodge ≤ 0,4 s après la fin du dribble, d'où un retard utile de 0,3 à 0,4 s.
//! - **BreeziFlick** (en plus) : la voiture tourne autour de son axe haut à plus de
//!   [`BREEZI_MIN_YAW_RATE`] (|ang_vel·up|) au dernier toucher qui suit le dodge. Approximation : flick avec
//!   rotation continue façon tornado ; l'enchaînement exact air roll + flip du breezi n'est pas vérifié.

use crate::Mechanic;
use crate::ctx::Ctx;
use crate::detail::{Out, Support};
use crate::thresholds::*;

#[inline]
fn within(since: Option<u64>, now: u64, window: u64) -> bool {
    since.is_some_and(|t| now - t <= window)
}

/// Une séquence de portage (balle dans la fenêtre toit sous une condition donnée).
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    ticks: u32,
    gap: u32,
    touches: u32,
    last_touch: Option<u64>,
    dist: f32,
    /// Somme de la position avant de la balle (hood dribble).
    sum_fwd: f32,
    counted: bool,
}

impl Run {
    #[inline]
    fn update(&mut self, ok: bool, cx: &Ctx, speed: f32) {
        if ok {
            self.ticks += 1;
            self.gap = 0;
            self.dist += speed / crate::TICK_HZ;
            self.sum_fwd += cx.rel_fwd;
            if cx.touch && !within(self.last_touch, cx.n, DRIBBLE_TOUCH_SEP_TICKS) {
                self.touches += 1;
                self.last_touch = Some(cx.n);
            }
        } else if self.ticks > 0 {
            self.gap += 1;
            if self.gap > DRIBBLE_GAP_TICKS {
                *self = Run::default();
            }
        }
    }

    /// Vrai une seule fois par portage, quand les seuils sont atteints.
    #[inline]
    fn fire(&mut self, min_ticks: u32, min_touches: u32, min_dist: f32) -> bool {
        if !self.counted
            && self.ticks >= min_ticks
            && self.touches >= min_touches
            && self.dist >= min_dist
        {
            self.counted = true;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Group {
    /// Portages : au sol à plat (hood), en marche arrière, sur un mur, au plafond.
    ground: Run,
    reverse: Run,
    wall: Run,
    ceiling: Run,
    // Bounce dribble : série de touchers, rebond au sol depuis le dernier toucher.
    bd_last: Option<u64>,
    bd_touches: u32,
    bd_bounced: bool,
    bd_counted: bool,
    // Flicks : saut (tick, balle portée), état au dernier dodge, rotation au dernier toucher qui suit.
    jump: Option<(u64, bool)>,
    fl_delayed: bool,
    fl_ball_above: bool,
    fl_yaw: f32,
}

impl Group {
    #[inline]
    pub fn tick(&mut self, cx: &Ctx, out: &mut Out) {
        self.carry(cx, out);
        self.bounce(cx, out);
        self.flick(cx, out);
    }

    /// Hood, reverse, wall et ceiling dribble.
    #[inline]
    fn carry(&mut self, cx: &Ctx, out: &mut Out) {
        let car = cx.car;
        // Sortie rapide : balle loin et aucun portage en cours.
        let idle = self.ground.ticks == 0
            && self.reverse.ticks == 0
            && self.wall.ticks == 0
            && self.ceiling.ticks == 0;
        if idle && cx.dist > CARRIED_BALL_DIST {
            return;
        }
        let carried = cx.dist <= CARRIED_BALL_DIST
            && (DRIBBLE_UP_MIN..=DRIBBLE_UP_MAX).contains(&cx.rel_up)
            && cx.rel_fwd.abs() < DRIBBLE_MAX_ABS_FWD
            && cx.rel_side.abs() < DRIBBLE_MAX_ABS_SIDE
            && {
                let rv = [
                    cx.ball.vel[0] - car.vel[0],
                    cx.ball.vel[1] - car.vel[1],
                    cx.ball.vel[2] - car.vel[2],
                ];
                rv[0] * rv[0] + rv[1] * rv[1] + rv[2] * rv[2]
                    < DRIBBLE_MAX_REL_SPEED * DRIBBLE_MAX_REL_SPEED
            };
        // Même condition « au sol à plat » que le dribble de base.
        let flat = car.on_ground && car.up[2] > DRIBBLE_MIN_UP_Z;
        let fwd_speed =
            car.vel[0] * car.forward[0] + car.vel[1] * car.forward[1] + car.vel[2] * car.forward[2];

        self.ground.update(carried && flat, cx, cx.hspeed);
        self.reverse.update(
            carried && flat && fwd_speed < REVERSE_MAX_FWD_SPEED,
            cx,
            cx.hspeed,
        );
        let speed3 =
            (car.vel[0] * car.vel[0] + car.vel[1] * car.vel[1] + car.vel[2] * car.vel[2]).sqrt();
        self.wall.update(carried && cx.on_wall, cx, speed3);
        self.ceiling.update(carried && cx.on_ceiling, cx, speed3);

        if cx.base.dribble
            && self.ground.ticks > 0
            && self.ground.sum_fwd / self.ground.ticks as f32 > HOOD_MIN_MEAN_FWD
        {
            out.emit(Mechanic::HoodDribble, 0.0, Support::Ground);
        }
        if self
            .reverse
            .fire(REVERSE_MIN_TICKS, REVERSE_MIN_TOUCHES, REVERSE_MIN_DIST)
        {
            out.emit(Mechanic::ReverseDribble, 0.0, Support::Ground);
        }
        if self.wall.fire(
            WALL_DRIBBLE_MIN_TICKS,
            SURFACE_DRIBBLE_MIN_TOUCHES,
            WALL_DRIBBLE_MIN_DIST,
        ) {
            out.emit(Mechanic::WallDribble, 0.0, Support::Wall);
        }
        if self
            .ceiling
            .fire(CEILING_DRIBBLE_MIN_TICKS, SURFACE_DRIBBLE_MIN_TOUCHES, 0.0)
        {
            out.emit(Mechanic::CeilingDribble, 0.0, Support::Ceiling);
        }
    }

    /// Bounce dribble : touchers doux espacés, la balle rebondit au sol entre deux.
    #[inline]
    fn bounce(&mut self, cx: &Ctx, out: &mut Out) {
        let b = cx.ball;
        if cx.prev_ball.vel[2] < -BALL_FLOOR_BOUNCE_MIN_VZ
            && b.vel[2] > 0.0
            && b.pos[2] < BALL_FLOOR_BOUNCE_MAX_Z
        {
            self.bd_bounced = true;
        }
        if cx.touch {
            let ok = cx.on_terrain
                && cx.car.up[2] > DRIBBLE_MIN_UP_Z
                && cx.hspeed > BOUNCE_DRIBBLE_MIN_CAR_SPEED
                && b.pos[2] < BOUNCE_DRIBBLE_BALL_MAX_Z
                && cx.dist < BOUNCE_DRIBBLE_MAX_DIST;
            if ok {
                let gap_ok = self.bd_last.is_some_and(|t| {
                    (BOUNCE_DRIBBLE_MIN_GAP_TICKS..=BOUNCE_DRIBBLE_MAX_GAP_TICKS)
                        .contains(&(cx.n - t))
                });
                if gap_ok && self.bd_bounced {
                    self.bd_touches += 1;
                } else {
                    self.bd_touches = 1;
                    self.bd_counted = false;
                }
                self.bd_last = Some(cx.n);
            } else {
                self.bd_reset();
            }
            self.bd_bounced = false;
        }
        // Jugement du dernier toucher de la série : doux, sinon la série est cassée.
        if let Some(h) = cx.hit
            && Some(h.tick) == self.bd_last
        {
            if h.speed_after >= BOUNCE_DRIBBLE_MAX_BALL_SPEED {
                self.bd_reset();
            } else if self.bd_touches >= BOUNCE_DRIBBLE_MIN_TOUCHES && !self.bd_counted {
                self.bd_counted = true;
                out.emit(Mechanic::BounceDribble, 0.0, Support::Ground);
            }
        }
    }

    #[inline]
    fn bd_reset(&mut self) {
        self.bd_last = None;
        self.bd_touches = 0;
        self.bd_counted = false;
    }

    /// Variantes de flick : direction du dodge, retard depuis le saut, rotation au toucher.
    #[inline]
    fn flick(&mut self, cx: &Ctx, out: &mut Out) {
        let car = cx.car;
        if cx.landed {
            self.jump = None;
        }
        if cx.ev.has(crate::Events::JUMP) {
            self.jump = Some((cx.n, cx.base.dribble_active));
        }
        if cx.dodge_now {
            let carried_air = cx.rel_up > 0.0 && cx.dist < CARRIED_BALL_DIST;
            self.fl_delayed = carried_air
                && self
                    .jump
                    .is_some_and(|(t, carried)| carried && cx.n - t >= DELAYED_FLICK_MIN_TICKS);
            self.fl_ball_above = cx.rel_up > MUSTY_MIN_BALL_UP;
            self.fl_yaw = 0.0;
        }
        // Toucher brut (comme le flick de base) dans la fenêtre du flick : rotation autour de l'axe haut.
        let raw_hit = car.ball_hit_tick.is_some() && car.ball_hit_tick != cx.prev_car.ball_hit_tick;
        if raw_hit && cx.dodge_age.is_some_and(|a| a <= FLICK_HIT_WINDOW_TICKS) {
            let w = car.ang_vel;
            self.fl_yaw = (w[0] * car.up[0] + w[1] * car.up[1] + w[2] * car.up[2]).abs();
        }
        if !cx.base.flick {
            return;
        }
        let Some(d) = cx.dodge else { return };
        let v = cx.ball_speed;
        match d.sector {
            1 | 7 => out.emit(Mechanic::Flick45, v, Support::None),
            2 | 6 => out.emit(Mechanic::Flick90, v, Support::None),
            3..=5 if d.fwd < MUSTY_MAX_FWD && self.fl_ball_above => {
                out.emit(Mechanic::MustyFlick, v, Support::None)
            }
            _ => {}
        }
        if self.fl_delayed {
            out.emit(Mechanic::DelayedFlick, v, Support::None);
        }
        if self.fl_yaw > BREEZI_MIN_YAW_RATE {
            out.emit(Mechanic::BreeziFlick, v, Support::None);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Counts, Detector, Mechanic, Support};

    /// Les variantes de flick gardent la vitesse de balle comme valeur et record.
    #[test]
    fn flick_45_record_et_detail() {
        let mut det = Detector::new();
        let mut counts = Counts::default();
        for (c, b) in crate::script_carry::flick_variant(0.707, 0.707, 8, 0.0, true) {
            det.tick(&c, &b, &mut counts);
        }
        assert_eq!(counts.get(Mechanic::Flick45), 1);
        let d = det.detail(Mechanic::Flick45);
        assert!(d.value > 1500.0 && d.value < 2900.0, "{d:?}");
        assert_eq!(d.support, Support::None);
        assert_eq!(det.records().get(Mechanic::Flick45), d.value);
    }

    /// Les variantes de dribble ne gardent pas de record (valeur 0) mais leur support.
    #[test]
    fn wall_dribble_support() {
        let mut det = Detector::new();
        let mut counts = Counts::default();
        for (c, b) in crate::script_carry::wall_dribble(700.0, 80) {
            det.tick(&c, &b, &mut counts);
        }
        assert_eq!(counts.get(Mechanic::WallDribble), 1);
        assert_eq!(det.detail(Mechanic::WallDribble).support, Support::Wall);
        assert_eq!(det.records().get(Mechanic::WallDribble), 0.0);
    }
}
