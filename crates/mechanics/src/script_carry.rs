//! Séquences scriptées du catalogue étendu : groupe « carry » (variantes de dribble et de flick).

use crate::script::{Case, Frame, Script, ball_far, car_on_ground, case, dodge, hit, jump, land};
use crate::{BallTick, CarTick, Mechanic};

/// Voiture au sol qui roule à `vx` (face à +x : `vx` < 0 = marche arrière), balle sur le toit décalée de
/// `ball_fwd` vers l'avant ; un toucher tous les `every` ticks pendant `ticks` (le premier tombe sur le tick
/// d'initialisation du détecteur et ne compte pas).
fn roof_carry(ball_fwd: f32, vx: f32, ticks: usize, every: usize) -> Script {
    let mut car = car_on_ground();
    car.vel = [vx, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [ball_fwd, 0.0, 157.0],
            vel: [vx, 0.0, 0.0],
        },
    );
    s.run(ticks, |i, c, _| {
        if i % every == 0 {
            hit(c);
        }
    });
    s
}

/// Portage au sol à `vx` pendant 100 ticks, le cap tourne de `total` radians (balle sur le toit, touchers tous les 15).
fn turning_carry(vx: f32, total: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [vx, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 157.0],
            vel: [vx, 0.0, 0.0],
        },
    );
    let n = 130;
    s.run(n, |i, c, b| {
        let yaw = total * (i + 1) as f32 / n as f32;
        let (sn, cs) = yaw.sin_cos();
        c.forward = [cs, sn, 0.0];
        c.right = [-sn, cs, 0.0];
        c.vel = [vx * cs, vx * sn, 0.0];
        b.pos = [c.pos[0], c.pos[1], 157.0];
        b.vel = c.vel;
        if i % 22 == 0 {
            hit(c);
        }
    });
    s.hold(5);
    s.frames()
}

fn carry_frames(ball_fwd: f32, vx: f32, ticks: usize, every: usize) -> Vec<Frame> {
    let mut s = roof_carry(ball_fwd, vx, ticks, every);
    s.hold(5);
    s.frames()
}

/// Voiture qui monte un mur à `vz` avec la balle contre le toit (côté terrain), un toucher tous les 30 ticks.
pub(crate) fn wall_dribble(vz: f32, ticks: usize) -> Vec<Frame> {
    let car = CarTick {
        pos: [4079.0, 0.0, 600.0],
        vel: [0.0, 0.0, vz],
        forward: [0.0, 0.0, 1.0],
        right: [0.0, 1.0, 0.0],
        up: [-1.0, 0.0, 0.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    };
    let mut s = Script::new(
        car,
        BallTick {
            pos: [car.pos[0] - 140.0, 0.0, car.pos[2]],
            vel: car.vel,
        },
    );
    s.run(ticks, |i, c, _| {
        if i % 30 == 0 {
            hit(c);
        }
    });
    // La balle se décolle du mur et retombe, la voiture continue seule.
    s.once(|_, b| b.vel = [-600.0, 0.0, -300.0]);
    s.hold(20);
    s.frames()
}

/// Voiture qui roule au plafond à 600 uu/s, balle portée sous le toit, un toucher tous les 25 ticks.
fn ceiling_dribble(ticks: usize) -> Vec<Frame> {
    let car = CarTick {
        pos: [0.0, 0.0, 2010.0],
        vel: [600.0, 0.0, 0.0],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, -1.0, 0.0],
        up: [0.0, 0.0, -1.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    };
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 1870.0],
            vel: car.vel,
        },
    );
    s.run(ticks, |i, c, _| {
        if i % 25 == 0 {
            hit(c);
        }
    });
    // La balle tombe, la voiture reste au plafond.
    s.once(|_, b| b.vel = [600.0, 0.0, -800.0]);
    s.hold(20);
    s.frames()
}

/// Voiture au sol à `vx`, balle 160 uu devant qui rebondit au sol : chaque toucher (tous les `period` ticks)
/// la renvoie vers le sol, elle rebondit puis remonte. `hard` : chaque toucher l'envoie à ≈ 1 750 uu/s.
fn bounce_dribble(vx: f32, period: usize, touches: usize, hard: bool, vz: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [vx, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [160.0, 0.0, 93.0],
            vel: [vx, 0.0, vz],
        },
    );
    let half = period / 2;
    s.run(period * touches, |i, c, b| {
        if i % period == half {
            hit(c);
            b.vel = [if hard { 1700.0 } else { vx }, 0.0, -vz];
        } else if b.vel[0] != vx {
            b.vel[0] = vx;
        }
        if b.pos[2] <= 93.5 && b.vel[2] < 0.0 {
            b.pos[2] = 93.0;
            b.vel[2] = vz;
        }
    });
    s.hold(10);
    s.frames()
}

/// Dribble toit (6 touchers, 0,83 s), saut, `dodge_hold` ticks plus tard dodge (`fwd`, `lat`), toucher 3 ticks
/// après : balle à ≈ 2 280 uu/s. `yaw` : rotation autour de l'axe haut (rad/s) du dodge au retour au sol.
/// `ball_above` faux : au dodge la balle est devant le nez, à hauteur du châssis.
pub(crate) fn flick_variant(
    fwd: f32,
    lat: f32,
    dodge_hold: usize,
    yaw: f32,
    ball_above: bool,
) -> Vec<Frame> {
    let mut s = roof_carry(0.0, 800.0, 100, 15);
    s.once(|c, b| {
        jump(c);
        c.vel = [800.0, 0.0, 300.0];
        b.vel = c.vel;
    });
    s.hold(dodge_hold).once(|c, b| {
        dodge(c, fwd, lat);
        c.ang_vel = [0.0, 0.0, yaw];
        if !ball_above {
            b.pos = [c.pos[0] + 150.0, c.pos[1], c.pos[2] - 10.0];
        }
    });
    s.hold(3).once(|c, b| {
        c.pitch_input = 0.0;
        hit(c);
        b.vel = [2200.0, 0.0, 600.0];
    });
    s.hold(40).once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [900.0, 0.0, 0.0];
        c.ang_vel = [0.0; 3];
    });
    s.hold(5);
    s.frames()
}

/// Sans dribble : voiture qui roule, saut sous une balle, backflip et toucher qui l'envoie à 2 280 uu/s.
fn backflip_hit_no_dribble() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [800.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(20).once(|c, b| {
        jump(c);
        c.vel = [800.0, 0.0, 300.0];
        b.pos = [c.pos[0], c.pos[1], c.pos[2] + 140.0];
        b.vel = c.vel;
    });
    s.hold(8).once(|c, _| dodge(c, -1.0, 0.0));
    s.hold(3).once(|c, b| {
        c.pitch_input = 0.0;
        hit(c);
        b.vel = [-2200.0, 0.0, 600.0];
    });
    s.hold(40).once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
    });
    s.hold(5);
    s.frames()
}

pub fn cases() -> Vec<Case> {
    use Mechanic::*;
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let flick = [(Touch, 7), (Dribble, 1), (Flick, 1)];
    let with = |extra: &[(Mechanic, u64)]| -> Vec<(Mechanic, u64)> {
        flick.iter().chain(extra.iter()).copied().collect()
    };
    let dribble = [(Touch, 5), (Dribble, 1)];
    vec![
        // --- Dribble turn.
        case(
            "dribble_turn",
            DribbleTurn,
            true,
            turning_carry(800.0, 1.4),
            &[(Touch, 5), (Dribble, 1), (DribbleTurn, 1)],
        ),
        case(
            "dribble_turn_negative_straight",
            DribbleTurn,
            false,
            turning_carry(800.0, 0.0),
            &dribble,
        ),
        case(
            "dribble_turn_negative_small_angle",
            DribbleTurn,
            false,
            turning_carry(800.0, 0.5),
            &dribble,
        ),
        // --- Hood dribble.
        case(
            "hood_dribble",
            HoodDribble,
            true,
            carry_frames(60.0, 800.0, 100, 15),
            &[(Touch, 6), (Dribble, 1), (HoodDribble, 1)],
        ),
        case(
            "hood_dribble_negative_roof",
            HoodDribble,
            false,
            carry_frames(0.0, 800.0, 100, 15),
            &[(Touch, 6), (Dribble, 1)],
        ),
        case(
            "hood_dribble_negative_just_behind_threshold",
            HoodDribble,
            false,
            carry_frames(30.0, 800.0, 100, 15),
            &[(Touch, 6), (Dribble, 1)],
        ),
        case(
            "hood_dribble_negative_stationary",
            HoodDribble,
            false,
            carry_frames(60.0, 0.0, 100, 15),
            &[(Touch, 6)],
        ),
        // --- Reverse dribble.
        case(
            "reverse_dribble",
            ReverseDribble,
            true,
            carry_frames(0.0, -700.0, 84, 25),
            &[(Touch, 3), (ReverseDribble, 1)],
        ),
        case(
            "reverse_dribble_negative_slow",
            ReverseDribble,
            false,
            carry_frames(0.0, -280.0, 180, 40),
            &[(Touch, 4)],
        ),
        case(
            "reverse_dribble_negative_short",
            ReverseDribble,
            false,
            carry_frames(0.0, -700.0, 50, 15),
            &[(Touch, 3)],
        ),
        case(
            "reverse_dribble_negative_forward",
            ReverseDribble,
            false,
            carry_frames(0.0, 700.0, 84, 25),
            &[(Touch, 3)],
        ),
        // --- Wall dribble.
        case(
            "wall_dribble",
            WallDribble,
            true,
            wall_dribble(700.0, 80),
            &[(Touch, 2), (WallDribble, 1)],
        ),
        case(
            "wall_dribble_negative_short",
            WallDribble,
            false,
            wall_dribble(700.0, 50),
            &[(Touch, 1)],
        ),
        case(
            "wall_dribble_negative_stationary",
            WallDribble,
            false,
            wall_dribble(0.0, 100),
            &[(Touch, 3)],
        ),
        // --- Ceiling dribble.
        case(
            "ceiling_dribble",
            CeilingDribble,
            true,
            ceiling_dribble(60),
            &[(Touch, 2), (CeilingDribble, 1)],
        ),
        case(
            "ceiling_dribble_negative_short",
            CeilingDribble,
            false,
            ceiling_dribble(40),
            &[(Touch, 1)],
        ),
        // --- Bounce dribble.
        case(
            "bounce_dribble",
            BounceDribble,
            true,
            bounce_dribble(800.0, 40, 4, false, 800.0),
            &[(Touch, 4), (BounceDribble, 1)],
        ),
        case(
            "bounce_dribble_negative_push_on_ground",
            BounceDribble,
            false,
            bounce_dribble(800.0, 40, 4, false, 400.0),
            &[(Touch, 4)],
        ),
        case(
            "bounce_dribble_negative_slow_car",
            BounceDribble,
            false,
            bounce_dribble(250.0, 40, 4, false, 800.0),
            &[(Touch, 4)],
        ),
        case(
            "bounce_dribble_negative_hard_touches",
            BounceDribble,
            false,
            bounce_dribble(800.0, 40, 4, true, 800.0),
            &[(Touch, 4)],
        ),
        case(
            "bounce_dribble_negative_spaced",
            BounceDribble,
            false,
            bounce_dribble(800.0, 64, 4, false, 800.0),
            &[(Touch, 4)],
        ),
        // --- Flicks : direction du dodge.
        case(
            "flick_45",
            Flick45,
            true,
            flick_variant(D, D, 8, 0.0, true),
            &with(&[(Flick45, 1)]),
        ),
        case(
            "flick_45_negative_straight",
            Flick45,
            false,
            flick_variant(1.0, 0.0, 8, 0.0, true),
            &flick,
        ),
        case(
            "flick_90",
            Flick90,
            true,
            flick_variant(0.0, -1.0, 8, 0.0, true),
            &with(&[(Flick90, 1)]),
        ),
        case(
            "flick_90_negative_diagonal",
            Flick90,
            false,
            flick_variant(D, -D, 8, 0.0, true),
            &with(&[(Flick45, 1)]),
        ),
        case(
            "musty_flick",
            MustyFlick,
            true,
            flick_variant(-1.0, 0.0, 8, 0.0, true),
            &with(&[(MustyFlick, 1)]),
        ),
        case(
            "musty_flick_negative_ball_in_front",
            MustyFlick,
            false,
            flick_variant(-1.0, 0.0, 8, 0.0, false),
            &flick,
        ),
        case(
            "musty_flick_negative_no_dribble",
            MustyFlick,
            false,
            backflip_hit_no_dribble(),
            &[(Touch, 1)],
        ),
        // --- Flicks : retard, rotation.
        case(
            "delayed_flick",
            DelayedFlick,
            true,
            flick_variant(1.0, 0.0, 39, 0.0, true),
            &with(&[(DelayedFlick, 1)]),
        ),
        case(
            "delayed_flick_negative_quick",
            DelayedFlick,
            false,
            flick_variant(1.0, 0.0, 25, 0.0, true),
            &flick,
        ),
        case(
            "delayed_flick_negative_out_of_window",
            DelayedFlick,
            false,
            flick_variant(1.0, 0.0, 55, 0.0, true),
            &[(Touch, 7), (Dribble, 1)],
        ),
        case(
            "breezi_flick",
            BreeziFlick,
            true,
            flick_variant(1.0, 0.0, 8, 5.0, true),
            &with(&[(BreeziFlick, 1)]),
        ),
        case(
            "breezi_flick_negative_slow_spin",
            BreeziFlick,
            false,
            flick_variant(1.0, 0.0, 8, 2.0, true),
            &flick,
        ),
    ]
}
