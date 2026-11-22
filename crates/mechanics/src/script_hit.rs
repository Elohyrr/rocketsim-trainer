//! Séquences scriptées du catalogue étendu : groupe « hit ».

use crate::script::{Case, Frame, Script, car_on_ground, case, dodge, hit};
use crate::{BallTick, CarTick, Mechanic};

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Voiture en l'air (depuis 1 s, sans saut vu par le détecteur : pas de toucher aérien v2), à plat, face à +x.
fn air_car(pos: [f32; 3]) -> CarTick {
    CarTick {
        pos,
        forward: [1.0, 0.0, 0.0],
        right: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
        has_jumped: true,
        air_time: 1.0,
        ..Default::default()
    }
}

// ---------------------------------------------------------------- power shot

/// Voiture au sol à 1 500 uu/s qui frappe une balle au repos ; la balle repart à `v_after`.
fn ground_shot(v_after: [f32; 3]) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1500.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [150.0, 0.0, 93.0],
            vel: [0.0; 3],
        },
    );
    s.hold(10).once(|c, b| {
        hit(c);
        b.vel = v_after;
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- pinch, kuxir pinch

/// Voiture en l'air en `car` qui frappe la balle en `ball` (vitesse avant `v_before`, après `v_after`) ;
/// `dodged` : dodge avant 10 ticks avant le toucher.
fn pinch_seq(
    ball: [f32; 3],
    car: [f32; 3],
    v_before: [f32; 3],
    v_after: [f32; 3],
    dodged: bool,
) -> Vec<Frame> {
    let mut s = Script::new(
        air_car(car),
        BallTick {
            pos: ball,
            vel: v_before,
        },
    );
    s.hold(10);
    if dodged {
        s.once(|c, _| dodge(c, 1.0, 0.0)).hold(9);
    } else {
        s.hold(10);
    }
    s.once(|c, b| {
        hit(c);
        c.pos = car;
        b.pos = ball;
        b.vel = v_after;
    });
    s.hold(10);
    s.frames()
}

/// Balle contre le mur latéral (x = 3 990), voiture côté terrain à 190 uu.
const WALL_BALL: [f32; 3] = [3990.0, 0.0, 400.0];
const WALL_CAR: [f32; 3] = [3800.0, 0.0, 400.0];

// ---------------------------------------------------------------- flip shot

/// Voiture au sol qui saute puis dodge (`fwd`, `lat`) ; `wait` ticks plus tard elle touche une balle posée à
/// `ball_off` (repère monde, relatif à la voiture) et l'envoie à `v_after`.
fn flip_shot_seq(
    fwd: f32,
    lat: f32,
    wait: usize,
    ball_off: [f32; 3],
    v_after: [f32; 3],
) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1200.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [400.0, 0.0, 93.0],
            vel: [0.0; 3],
        },
    );
    s.hold(5).once(|c, _| {
        crate::script::jump(c);
        c.pos[2] = 60.0;
    });
    s.hold(5).once(|c, _| dodge(c, fwd, lat));
    s.hold(wait).once(move |c, b| {
        hit(c);
        c.pos[2] = 80.0;
        b.pos = add(c.pos, ball_off);
        b.vel = v_after;
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- cushion

/// Voiture au sol à 500 uu/s, balle qui arrive à `vx_before` (vers la voiture) et repart à `v_after`.
/// Même amorti, mais la voiture est en vrille (haut presque horizontal, `up.z` < 0,5) : pas un amorti voulu.
fn cushion_tumbling() -> Vec<Frame> {
    let mut f = cushion_seq(-1500.0, [600.0, 0.0, 0.0]);
    for (c, _) in f.iter_mut() {
        c.up = [0.95, 0.0, 0.3];
    }
    f
}

fn cushion_seq(vx_before: f32, v_after: [f32; 3]) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [500.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [300.0, 0.0, 93.0],
            vel: [vx_before, 0.0, 0.0],
        },
    );
    s.hold(10).once(|c, b| {
        hit(c);
        b.vel = v_after;
    });
    s.hold(45);
    s.frames()
}

// ---------------------------------------------------------------- redirect, air roll shot

/// Même séquence sans aucun saut pressé : la voiture tombe (mur, plafond).
fn no_jump(mut f: Vec<Frame>) -> Vec<Frame> {
    for (c, _) in f.iter_mut() {
        c.has_jumped = false;
    }
    f
}

/// Voiture en l'air à z = 600 (vitesse angulaire `ang_vel`, roulis commandé `roll`) qui touche une balle de
/// vitesse `v_before` et l'envoie à `v_after` ; `side_dodge` : dodge latéral 10 ticks avant.
fn air_hit(
    ang_vel: [f32; 3],
    roll: f32,
    side_dodge: bool,
    v_before: [f32; 3],
    v_after: [f32; 3],
) -> Vec<Frame> {
    let mut car = air_car([0.0, 0.0, 600.0]);
    // Le saut est pressé en vol (tick 1) : le détecteur le voit comme un décollage voulu.
    car.has_jumped = false;
    car.ang_vel = ang_vel;
    car.roll_input = roll;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [120.0, 0.0, 620.0],
            vel: v_before,
        },
    );
    s.hold(1).once(|c, _| c.has_jumped = true);
    s.hold(30);
    if side_dodge {
        s.once(|c, _| dodge(c, 0.0, 1.0)).hold(9);
    } else {
        s.hold(10);
    }
    s.once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [120.0, 0.0, 20.0]);
        b.vel = v_after;
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- backboard read / double

/// Balle lancée vers un mur (fond si `back`, sinon latéral) à 1 500 uu/s, rebond à −1 200, toucher en l'air
/// `wait` ticks après le départ.
fn wall_read(back: bool, wait: usize) -> Vec<Frame> {
    let (car, ball, vel) = if back {
        (
            [0.0, 4300.0, 700.0],
            [0.0, 4600.0, 800.0],
            [0.0, 1500.0, 0.0],
        )
    } else {
        (
            [3300.0, 0.0, 700.0],
            [3600.0, 0.0, 800.0],
            [1500.0, 0.0, 0.0],
        )
    };
    let mut s = Script::new(air_car(car), BallTick { pos: ball, vel });
    s.run(wait, |_, _, b| bounce_off_walls(b));
    s.once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [0.0, 120.0, 50.0]);
        b.vel = [500.0, -1400.0, 200.0];
    });
    s.hold(10);
    s.frames()
}

/// Rebond scripté de la balle sur le mur latéral +x ou du fond +y (vitesse normale ramenée à −1 200).
fn bounce_off_walls(b: &mut BallTick) {
    let r = crate::thresholds::BALL_RADIUS;
    if b.pos[1] > crate::thresholds::ARENA_BACK_Y - r && b.vel[1] > 0.0 {
        b.vel[1] = -1200.0;
    }
    if b.pos[0] > crate::thresholds::ARENA_SIDE_X - r && b.vel[0] > 0.0 {
        b.vel[0] = -1200.0;
    }
}

/// Double touch : toucher en l'air vers un mur (fond si `back`), rebond, second toucher en l'air 1,2 s après.
fn wall_double(back: bool) -> Vec<Frame> {
    let (car, ball, v1, v2) = if back {
        (
            [0.0, 4300.0, 600.0],
            [0.0, 4400.0, 650.0],
            [0.0, 2000.0, 0.0],
            [0.0, 1200.0, 400.0],
        )
    } else {
        (
            [3300.0, 0.0, 600.0],
            [3400.0, 0.0, 650.0],
            [2000.0, 0.0, 0.0],
            [1200.0, 0.0, 400.0],
        )
    };
    let mut s = Script::new(
        air_car(car),
        BallTick {
            pos: ball,
            vel: [0.0; 3],
        },
    );
    s.hold(5).once(|c, b| {
        hit(c);
        b.vel = v1;
    });
    s.run(140, |_, _, b| bounce_off_walls(b));
    s.once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [0.0, 120.0, 50.0]);
        b.vel = v2;
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- half volley

/// Voiture au sol à 1 000 uu/s ; la balle tombe à −600, rebondit au sol à +500, frappe `wait` ticks après le
/// rebond (balle envoyée à `v_after`).
fn half_volley_seq(wait: usize, v_after: [f32; 3]) -> Vec<Frame> {
    half_volley_v6(wait, v_after, 400.0, true)
}

/// `start_z` : hauteur de départ de la balle (apogée avant le rebond) ; `jump` : saut pressé au rebond.
fn half_volley_v6(wait: usize, v_after: [f32; 3], start_z: f32, jump: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1000.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [300.0, 0.0, start_z],
            vel: [0.0, 0.0, -600.0],
        },
    );
    s.hold(2).once(move |c, b| {
        if jump {
            crate::script::jump(c);
        }
        b.pos[2] = 93.0;
        b.vel[2] = 500.0;
    });
    s.hold(wait - 1).once(|c, b| {
        hit(c);
        b.vel = v_after;
    });
    s.hold(10);
    s.frames()
}

pub fn cases() -> Vec<Case> {
    use Mechanic::*;
    vec![
        // --- Power shot.
        case(
            "power_shot",
            PowerShot,
            true,
            ground_shot([300.0, 3300.0, 200.0]),
            &[(Touch, 1), (PowerShot, 1)],
        ),
        // Detector 6 : vitesse de balle sans dodge ni visée du but adverse (voiture supersonique) = pas un tir voulu.
        case(
            "power_shot_negative_unaimed_no_dodge",
            PowerShot,
            false,
            ground_shot([3300.0, 0.0, 200.0]),
            &[(Touch, 1)],
        ),
        case(
            "power_shot_negative_2900",
            PowerShot,
            false,
            ground_shot([2900.0, 0.0, 0.0]),
            &[(Touch, 1)],
        ),
        // --- Flip shot.
        case(
            "flip_shot",
            FlipShot,
            true,
            flip_shot_seq(1.0, 0.0, 8, [110.0, 0.0, 0.0], [2200.0, 0.0, 300.0]),
            &[(Touch, 1), (FlipShot, 1)],
        ),
        case(
            "flip_shot_diagonal",
            FlipShot,
            true,
            flip_shot_seq(0.7, 0.7, 8, [110.0, 0.0, 0.0], [2000.0, 400.0, 300.0]),
            &[(Touch, 1), (FlipShot, 1)],
        ),
        case(
            "flip_shot_negative_side_dodge",
            FlipShot,
            false,
            flip_shot_seq(0.0, 1.0, 8, [110.0, 0.0, 0.0], [2200.0, 0.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "flip_shot_negative_late",
            FlipShot,
            false,
            flip_shot_seq(1.0, 0.0, 40, [110.0, 0.0, 0.0], [2200.0, 0.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "flip_shot_negative_behind",
            FlipShot,
            false,
            flip_shot_seq(1.0, 0.0, 8, [-110.0, 0.0, 0.0], [-2200.0, 0.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "flip_shot_negative_no_gain",
            FlipShot,
            false,
            flip_shot_seq(1.0, 0.0, 8, [110.0, 0.0, 0.0], [150.0, 0.0, 0.0]),
            &[(Touch, 1)],
        ),
        // --- Pinch.
        case(
            "pinch_wall",
            Pinch,
            true,
            pinch_seq(WALL_BALL, WALL_CAR, [0.0; 3], [-3800.0, 0.0, 300.0], false),
            &[(Touch, 1), (Pinch, 1)],
        ),
        case(
            "pinch_negative_open_air",
            Pinch,
            false,
            pinch_seq(
                [0.0, 0.0, 800.0],
                [-190.0, 0.0, 800.0],
                [0.0; 3],
                [3800.0, 0.0, 300.0],
                false,
            ),
            &[(Touch, 1)],
        ),
        case(
            "pinch_negative_fast_ball_low_gain",
            Pinch,
            false,
            pinch_seq(
                WALL_BALL,
                WALL_CAR,
                [0.0, 2000.0, 0.0],
                [-1500.0, 3300.0, 300.0],
                false,
            ),
            &[(Touch, 1), (PowerShot, 1)],
        ),
        case(
            "pinch_negative_along_wall",
            Pinch,
            false,
            pinch_seq(
                WALL_BALL,
                [3990.0, -200.0, 400.0],
                [0.0; 3],
                [-500.0, 3700.0, 300.0],
                false,
            ),
            &[(Touch, 1)],
        ),
        case(
            "pinch_negative_too_slow",
            Pinch,
            false,
            pinch_seq(WALL_BALL, WALL_CAR, [0.0; 3], [-3300.0, 0.0, 300.0], false),
            &[(Touch, 1)],
        ),
        // --- Kuxir pinch.
        case(
            "kuxir_pinch",
            KuxirPinch,
            true,
            pinch_seq(
                WALL_BALL,
                WALL_CAR,
                [0.0; 3],
                [-2000.0, 3200.0, 300.0],
                true,
            ),
            &[(Touch, 1), (Pinch, 1), (KuxirPinch, 1)],
        ),
        case(
            "kuxir_pinch_negative_own_goal",
            KuxirPinch,
            false,
            pinch_seq(
                WALL_BALL,
                WALL_CAR,
                [0.0; 3],
                [-2000.0, -3200.0, 300.0],
                true,
            ),
            &[(Touch, 1), (Pinch, 1)],
        ),
        case(
            "kuxir_pinch_negative_no_dodge",
            KuxirPinch,
            false,
            pinch_seq(
                WALL_BALL,
                WALL_CAR,
                [0.0; 3],
                [-2000.0, 3200.0, 300.0],
                false,
            ),
            &[(Touch, 1), (Pinch, 1)],
        ),
        // --- Cushion.
        case(
            "cushion",
            Cushion,
            true,
            cushion_seq(-1500.0, [600.0, 0.0, 0.0]),
            &[(Touch, 1), (Cushion, 1)],
        ),
        case(
            "cushion_negative_car_tumbling",
            Cushion,
            false,
            cushion_tumbling(),
            &[(Touch, 1)],
        ),
        case(
            "cushion_negative_slow_ball",
            Cushion,
            false,
            cushion_seq(-900.0, [400.0, 0.0, 0.0]),
            &[(Touch, 1)],
        ),
        case(
            "cushion_negative_dead_bounce",
            Cushion,
            false,
            cushion_seq(-1500.0, [-200.0, 0.0, 0.0]),
            &[(Touch, 1)],
        ),
        // --- Redirect.
        case(
            "redirect",
            Redirect,
            true,
            air_hit(
                [0.0; 3],
                0.0,
                false,
                [0.0, -1800.0, 0.0],
                [1800.0, 0.0, 300.0],
            ),
            &[(Touch, 1), (Redirect, 1)],
        ),
        // Detector 6 : balle déviée vers son propre but, sans gain de vitesse, ou sans saut/dodge = contact fortuit.
        case(
            "redirect_negative_toward_own_goal",
            Redirect,
            false,
            air_hit([0.0; 3], 0.0, false, [1800.0, 0.0, 0.0], [0.0, -1900.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "redirect_negative_no_gain",
            Redirect,
            false,
            air_hit([0.0; 3], 0.0, false, [1800.0, 0.0, 0.0], [0.0, 1700.0, 0.0]),
            &[(Touch, 1)],
        ),
        case(
            "redirect_negative_no_jump",
            Redirect,
            false,
            no_jump(air_hit(
                [0.0; 3],
                0.0,
                false,
                [0.0, -1800.0, 0.0],
                [1800.0, 0.0, 300.0],
            )),
            &[(Touch, 1)],
        ),
        case(
            "redirect_negative_30deg",
            Redirect,
            false,
            air_hit(
                [0.0; 3],
                0.0,
                false,
                [0.0, -1800.0, 0.0],
                [900.0, -1560.0, 0.0],
            ),
            &[(Touch, 1)],
        ),
        // --- Air roll shot.
        case(
            "air_roll_shot",
            AirRollShot,
            true,
            air_hit([0.0; 3], 1.0, false, [0.0; 3], [1800.0, 0.0, 300.0]),
            &[(Touch, 1), (AirRollShot, 1)],
        ),
        case(
            "air_roll_shot_negative_no_jump_fall",
            AirRollShot,
            false,
            no_jump(air_hit([0.0; 3], 1.0, false, [0.0; 3], [1800.0, 0.0, 300.0])),
            &[(Touch, 1)],
        ),
        case(
            "air_roll_shot_negative_toward_own_goal",
            AirRollShot,
            false,
            air_hit([0.0; 3], 1.0, false, [0.0; 3], [0.0, -1800.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "air_roll_shot_negative_yaw_spin",
            AirRollShot,
            false,
            air_hit([0.0, 0.0, 4.0], 0.0, false, [0.0; 3], [1800.0, 0.0, 300.0]),
            &[(Touch, 1)],
        ),
        case(
            "air_roll_shot_negative_side_dodge",
            AirRollShot,
            false,
            air_hit([0.0; 3], 1.0, true, [0.0; 3], [1800.0, 0.0, 300.0]),
            &[(Touch, 1)],
        ),
        // --- Backboard read.
        case(
            "backboard_read",
            BackboardRead,
            true,
            wall_read(true, 60),
            &[(Touch, 1), (BackboardRead, 1)],
        ),
        case(
            "backboard_read_negative_side_wall",
            BackboardRead,
            false,
            wall_read(false, 60),
            &[(Touch, 1)],
        ),
        case(
            "backboard_read_negative_late",
            BackboardRead,
            false,
            wall_read(true, 330),
            &[(Touch, 1)],
        ),
        // --- Backboard double.
        case(
            "backboard_double",
            BackboardDouble,
            true,
            wall_double(true),
            &[
                (Touch, 2),
                (DoubleTouch, 1),
                (BackboardDouble, 1),
                (BackboardRead, 1),
            ],
        ),
        case(
            "backboard_double_negative_side_wall",
            BackboardDouble,
            false,
            wall_double(false),
            &[(Touch, 2), (DoubleTouch, 1)],
        ),
        // --- Half volley.
        case(
            "half_volley",
            HalfVolley,
            true,
            half_volley_seq(4, [2000.0, 0.0, 400.0]),
            &[(Touch, 1), (HalfVolley, 1)],
        ),
        case(
            "half_volley_negative_no_jump",
            HalfVolley,
            false,
            half_volley_v6(4, [2000.0, 0.0, 400.0], 400.0, false),
            &[(Touch, 1)],
        ),
        case(
            "half_volley_negative_rolling_ball",
            HalfVolley,
            false,
            half_volley_v6(4, [2000.0, 0.0, 400.0], 103.0, true),
            &[(Touch, 1)],
        ),
        case(
            "half_volley_negative_late",
            HalfVolley,
            false,
            half_volley_seq(20, [2000.0, 0.0, 400.0]),
            &[(Touch, 1)],
        ),
        case(
            "half_volley_negative_slow",
            HalfVolley,
            false,
            half_volley_seq(4, [1500.0, 0.0, 400.0]),
            &[(Touch, 1)],
        ),
    ]
}
