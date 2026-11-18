//! Automates du catalogue étendu : groupe « hit » (qualité et contexte d'un toucher).
//!
//! Tout est jugé sur le toucher jugé `cx.hit` (4 ticks après le toucher : état voiture/balle au toucher,
//! vitesse de balle avant / après), sauf le backboard double (`cx.base`) et le suivi des rebonds au sol.
//! Seuils : `thresholds.rs`, section « groupe hit ». Signatures retenues et approximations :
//!
//! - `power_shot` : vitesse après > 3 000 uu/s avec un gain ≥ 500 (ajout : une balle déjà rapide à peine
//!   déviée ne compte pas), et pas un pinch (exclusif avec `pinch`/`kuxir_pinch`). Valeur = vitesse après.
//! - `pinch` : gain > 2 500 et vitesse après > 3 500, balle COINCÉE au toucher : centre de la balle à moins de
//!   rayon + 60 uu d'une surface, et voiture du côté intérieur ((balle − voiture)·normale sortante >
//!   0,2·|balle − voiture|). Support : `Corner` (mur latéral ET mur du fond, ou pan de coin diagonal approché
//!   par le plan |x| + |y| = 8 000), sinon `Wall` (mur latéral), `BackWall` (mur du fond, hors ouverture du but
//!   jugée sur le centre de la balle), `Ceiling`, `Ground` (sol seul). Balle en plein air : pas de pinch (c'est
//!   un power shot). Pinch contre une autre voiture : non détectable (pas d'adversaire dans `CarTick`).
//!   Arène soccar standard seulement. Valeur = vitesse après.
//! - `kuxir_pinch` : un pinch (compté aussi comme `pinch`) contre un mur latéral, du fond ou un coin, la voiture
//!   ayant dodgé ≤ 0,35 s avant le toucher, balle envoyée vers le but adverse à > 1 500 uu/s (`attack_neg`).
//!   La position de départ (mur défensif) n'est pas exigée. Valeur = vitesse après.
//! - `cushion` : balle arrivant à > 1 000 uu/s, |v_après − v_voiture| < 400 et vitesse après < 0,5 × avant
//!   (la balle repart avec la voiture). Support = où est la voiture au toucher (sol, mur, coin, plafond ; en
//!   l'air : `None`). « Après » = tick de vitesse maximale du jugement : un amorti qui ralentit encore ensuite
//!   est jugé sur sa vitesse la plus haute.
//! - `redirect` : voiture en l'air (pas `on_ground`, z > 300), balle > 1 500 avant et après, déviée de plus de
//!   45°. Valeur = vitesse après.
//! - `air_roll_shot` : toucher aérien (en l'air ≥ 0,3 s, balle z > 300) avec rotation autour de l'axe avant
//!   |ω·avant| > 3 rad/s ou |roll| ≥ 0,5, balle > 1 500, et sans dodge dans les 0,65 s (un dodge latéral tourne
//!   aussi autour de l'axe avant). Valeur = vitesse après.
//! - `backboard_read` : toucher ≤ 2 s après un rebond de la balle sur un mur du fond (l'un ou l'autre), balle
//!   z > 300 au toucher ; une fois par rebond.
//! - `backboard_double` : double touch de base dont le rebond était sur le mur du fond (compté en plus de
//!   `double_touch`). Valeur = vitesse de balle au second toucher.
//! - `half_volley` : toucher 1 à 8 ticks après un rebond de la balle au sol (centre < 170 uu, vitesse verticale
//!   < −100 puis > 0, rebond antérieur au tick du toucher), balle z < 300 au toucher, vitesse après > 1 800.

use crate::Mechanic;
use crate::ctx::{Ctx, Hit};
use crate::detail::{Out, Support};
use crate::thresholds::*;

#[inline]
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Surface contre laquelle la balle est coincée par la voiture (balle en `ball`, voiture en `car`), ou `None`.
pub(crate) fn pinch_support(ball: [f32; 3], car: [f32; 3]) -> Option<Support> {
    let near = BALL_RADIUS + PINCH_SURFACE_MARGIN;
    let rel = sub(ball, car);
    let min_in = PINCH_INSIDE_RATIO * dot(rel, rel).sqrt();
    let (sx, sy) = (ball[0].signum(), ball[1].signum());
    // Balle près de la surface et voiture côté terrain (balle entre la voiture et la surface).
    let pinned = |dist: f32, n: [f32; 3]| dist < near && dot(rel, n) > min_in;

    let side = pinned(ARENA_SIDE_X - ball[0].abs(), [sx, 0.0, 0.0]);
    let in_goal = ball[0].abs() < GOAL_HALF_WIDTH && ball[2] < GOAL_HEIGHT;
    let back = !in_goal && pinned(ARENA_BACK_Y - ball[1].abs(), [0.0, sy, 0.0]);
    let h = core::f32::consts::FRAC_1_SQRT_2;
    let diag = pinned(
        (ARENA_CORNER_SUM - ball[0].abs() - ball[1].abs()) * h,
        [sx * h, sy * h, 0.0],
    );
    if diag || (side && back) {
        Some(Support::Corner)
    } else if side {
        Some(Support::Wall)
    } else if back {
        Some(Support::BackWall)
    } else if pinned(ARENA_CEILING_Z - ball[2], [0.0, 0.0, 1.0]) {
        Some(Support::Ceiling)
    } else if pinned(ball[2], [0.0, 0.0, -1.0]) {
        Some(Support::Ground)
    } else {
        None
    }
}

/// Surface où se trouve la voiture (roues au contact) au toucher, `None` en l'air.
fn car_support(h: &Hit) -> Support {
    let c = &h.car;
    if !c.on_ground {
        return Support::None;
    }
    let up_z = c.up[2];
    if up_z < CEILING_UP_Z && c.pos[2] > CEILING_MIN_Z {
        Support::Ceiling
    } else if up_z.abs() < WALL_MAX_ABS_UP_Z && c.pos[2] > WALL_MIN_Z {
        let dx = ARENA_SIDE_X - c.pos[0].abs();
        let dy = ARENA_BACK_Y - c.pos[1].abs();
        let diag =
            (ARENA_CORNER_SUM - c.pos[0].abs() - c.pos[1].abs()) * core::f32::consts::FRAC_1_SQRT_2;
        if diag < dx.min(dy) {
            Support::Corner
        } else if dy < dx {
            Support::BackWall
        } else {
            Support::Wall
        }
    } else if up_z > DRIBBLE_MIN_UP_Z {
        Support::Ground
    } else {
        Support::None
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Group {
    /// Dernier rebond de la balle au sol (tick).
    ground_bounce: Option<u64>,
    /// Dernier toucher et le dernier rebond au sol connu juste avant lui (pour le half volley).
    touch_tick: u64,
    bounce_before_touch: Option<u64>,
    /// Tick du rebond sur le mur du fond déjà exploité par un backboard read.
    bb_read_bounce: Option<u64>,
}

impl Group {
    #[inline]
    pub fn tick(&mut self, cx: &Ctx, out: &mut Out) {
        // Rebond au sol suivi en continu ; on retient celui qui précède le toucher (avant la mise à jour de ce
        // tick : un « rebond » au tick même du toucher est dû à la voiture).
        if cx.touch {
            self.touch_tick = cx.n;
            self.bounce_before_touch = self.ground_bounce;
        }
        if cx.prev_ball.vel[2] < -HALF_VOLLEY_BOUNCE_MIN_VZ
            && cx.ball.vel[2] > 0.0
            && cx.ball.pos[2] < HALF_VOLLEY_BOUNCE_MAX_Z
        {
            self.ground_bounce = Some(cx.n);
        }

        // Backboard double : le double touch de base, rebond sur le mur du fond.
        if cx.base.double_touch && cx.base.double_touch_back {
            out.emit(Mechanic::BackboardDouble, cx.ball_speed, Support::BackWall);
        }

        let Some(h) = cx.hit else { return };
        let after = h.speed_after;
        let gain = after - h.speed_before;

        // --- Pinch, kuxir pinch, power shot (exclusifs : un pinch n'est pas un power shot).
        let pinch = if gain > PINCH_MIN_GAIN && after > PINCH_MIN_SPEED {
            pinch_support(h.ball.pos, h.car.pos)
        } else {
            None
        };
        if let Some(sup) = pinch {
            out.emit(Mechanic::Pinch, after, sup);
            let toward_goal = if cx.attack_neg {
                -h.v_after[1]
            } else {
                h.v_after[1]
            };
            if matches!(sup, Support::Wall | Support::BackWall | Support::Corner)
                && h.dodge_age.is_some_and(|a| a <= KUXIR_MAX_DODGE_TICKS)
                && toward_goal > KUXIR_MIN_GOAL_SPEED
            {
                out.emit(Mechanic::KuxirPinch, after, sup);
            }
        } else if after > POWER_SHOT_MIN_SPEED && gain >= POWER_SHOT_MIN_GAIN {
            out.emit(Mechanic::PowerShot, after, Support::None);
        }

        // --- Cushion : la balle rapide repart avec la voiture.
        if h.speed_before > CUSHION_MIN_SPEED_BEFORE && after < CUSHION_MAX_RATIO * h.speed_before {
            let r = sub(h.v_after, h.car.vel);
            if dot(r, r) < CUSHION_MAX_REL_SPEED * CUSHION_MAX_REL_SPEED {
                out.emit(Mechanic::Cushion, 0.0, car_support(h));
            }
        }

        // --- Redirect : balle rapide déviée de plus de 45° par une voiture en l'air.
        if !h.car.on_ground
            && h.car.pos[2] > REDIRECT_MIN_CAR_Z
            && h.speed_before > REDIRECT_MIN_SPEED
            && after > REDIRECT_MIN_SPEED
            && dot(h.v_before, h.v_after) < REDIRECT_MAX_COS * h.speed_before * after
        {
            out.emit(Mechanic::Redirect, after, Support::None);
        }

        // --- Air roll shot : toucher aérien en roulis, sans dodge récent.
        if !h.car.on_ground
            && h.air_ticks >= AERIAL_MIN_AIR_TICKS
            && h.ball.pos[2] > AERIAL_BALL_MIN_Z
            && after > AIR_ROLL_MIN_SPEED
            && !h.dodge_age.is_some_and(|a| a <= AIR_ROLL_NO_DODGE_TICKS)
            && (dot(h.car.ang_vel, h.car.forward).abs() > AIR_ROLL_MIN_RATE
                || h.car.roll_input.abs() >= AIR_ROLL_MIN_INPUT)
        {
            out.emit(Mechanic::AirRollShot, after, Support::None);
        }

        // --- Backboard read : toucher haut peu après un rebond sur le mur du fond (une fois par rebond).
        if h.ball_bounce_back
            && h.ball.pos[2] > BACKBOARD_READ_MIN_Z
            && let Some(age) = h.ball_bounce_age.filter(|a| *a <= BACKBOARD_READ_MAX_TICKS)
        {
            let bounce = h.tick - age;
            if self.bb_read_bounce != Some(bounce) {
                self.bb_read_bounce = Some(bounce);
                out.emit(Mechanic::BackboardRead, 0.0, Support::BackWall);
            }
        }

        // --- Half volley : frappe juste après un rebond au sol.
        if h.tick == self.touch_tick
            && h.ball.pos[2] < HALF_VOLLEY_MAX_BALL_Z
            && after > HALF_VOLLEY_MIN_SPEED
            && self
                .bounce_before_touch
                .is_some_and(|b| h.tick - b <= HALF_VOLLEY_MAX_TICKS)
        {
            out.emit(Mechanic::HalfVolley, 0.0, Support::Ground);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_de_pinch() {
        // Mur latéral : voiture côté terrain.
        assert_eq!(
            pinch_support([4000.0, 0.0, 400.0], [3800.0, 0.0, 400.0]),
            Some(Support::Wall)
        );
        // Même balle, voiture qui longe le mur : pas coincée.
        assert_eq!(
            pinch_support([4000.0, 0.0, 400.0], [4000.0, -200.0, 400.0]),
            None
        );
        // Mur du fond, hors de l'ouverture du but.
        assert_eq!(
            pinch_support([2000.0, -5030.0, 300.0], [2000.0, -4830.0, 300.0]),
            Some(Support::BackWall)
        );
        // Devant le but : pas de mur.
        assert_eq!(
            pinch_support([0.0, 5030.0, 300.0], [0.0, 4830.0, 300.0]),
            None
        );
        // Coin : deux murs, ou pan diagonal.
        assert_eq!(
            pinch_support([4000.0, 5030.0, 300.0], [3800.0, 4830.0, 300.0]),
            Some(Support::Corner)
        );
        assert_eq!(
            pinch_support([3600.0, 4300.0, 300.0], [3450.0, 4150.0, 300.0]),
            Some(Support::Corner)
        );
        // Plafond, sol (voiture au-dessus).
        assert_eq!(
            pinch_support([0.0, 0.0, 1950.0], [0.0, -100.0, 1800.0]),
            Some(Support::Ceiling)
        );
        assert_eq!(
            pinch_support([0.0, 0.0, 93.0], [-100.0, 0.0, 180.0]),
            Some(Support::Ground)
        );
        // Balle au sol frappée de côté par une voiture au sol : pas coincée.
        assert_eq!(pinch_support([0.0, 0.0, 93.0], [-150.0, 0.0, 17.0]), None);
        // Plein air.
        assert_eq!(pinch_support([0.0, 0.0, 800.0], [-190.0, 0.0, 800.0]), None);
    }

    /// Rejoue un cas du groupe et rend le détecteur.
    fn replay_case(name: &str) -> crate::Detector {
        let case = crate::script_hit::cases()
            .into_iter()
            .find(|c| c.name == name)
            .expect("cas inconnu");
        let mut det = crate::Detector::new();
        let mut counts = crate::Counts::default();
        for (c, b) in &case.frames {
            det.tick(c, b, &mut counts);
        }
        det
    }

    #[test]
    fn details_et_records() {
        let det = replay_case("kuxir_pinch");
        for m in [Mechanic::Pinch, Mechanic::KuxirPinch] {
            let d = det.detail(m);
            assert_eq!(d.support, Support::Wall);
            assert!(d.value > 3700.0 && d.value < 3800.0, "{m:?} : {}", d.value);
            assert_eq!(det.records().get(m), d.value);
        }
        let det = replay_case("cushion");
        assert_eq!(det.detail(Mechanic::Cushion).support, Support::Ground);
        assert_eq!(det.records().get(Mechanic::Cushion), 0.0);
        let det = replay_case("backboard_double");
        assert!(det.records().get(Mechanic::BackboardDouble) > 1000.0);
    }
}
