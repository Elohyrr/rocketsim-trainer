//! Séquences scriptées du catalogue étendu : groupe « move ».

use crate::script::{Case, Frame, Script, ball_far, car_on_ground, case, dodge, hit, jump, land};
use crate::{BallTick, CarTick, Mechanic};

// ---------------------------------------------------------------- états de départ

/// Voiture qui monte un mur latéral (x = +4 096), roues sur le mur, nez vers le haut.
fn wall_car() -> CarTick {
    CarTick {
        pos: [4079.0, 0.0, 600.0],
        vel: [0.0, 0.0, 500.0],
        forward: [0.0, 0.0, 1.0],
        right: [0.0, 1.0, 0.0],
        up: [-1.0, 0.0, 0.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    }
}

/// Voiture en l'air (z = 800) depuis 1 s, à plat, après un saut.
fn car_in_air(vel: [f32; 3]) -> CarTick {
    CarTick {
        pos: [0.0, 0.0, 800.0],
        vel,
        forward: [1.0, 0.0, 0.0],
        right: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
        has_jumped: true,
        has_flip_or_jump: true,
        air_time: 1.0,
        ..Default::default()
    }
}

/// Voiture retournée à `z`, saut/double saut/flip utilisés, qui monte vers une balle immobile 170 uu au-dessus
/// (côté roues), puis s'en approche (10 ticks).
fn under_ball(z: f32) -> Script {
    let car = CarTick {
        pos: [0.0, 0.0, z],
        vel: [0.0, 0.0, 300.0],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, -1.0, 0.0],
        up: [0.0, 0.0, -1.0],
        has_jumped: true,
        has_double_jumped: true,
        has_flipped: true,
        air_time: 1.5,
        ..Default::default()
    };
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, z + 170.0],
            vel: [0.0; 3],
        },
    );
    s.hold(10);
    s
}

/// Roues sur la balle (2 puis 4 roues), drapeaux de saut/flip remis à zéro : flip reset.
fn reset_on_ball(s: &mut Script) {
    s.run(4, |_, c, _| {
        c.vel = [0.0; 3];
        c.wheels_contact = 2;
    });
    s.once(|c, _| {
        c.on_ground = true;
        c.wheels_contact = 4;
        c.has_jumped = false;
        c.has_double_jumped = false;
        c.has_flipped = false;
        c.is_flipping = false;
        c.has_flip_or_jump = true;
    });
}

/// Quitte la balle, dodge avant (sans toucher), puis revient sous la balle au même endroit (44 ticks).
fn leave_and_dodge(s: &mut Script) {
    s.once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [0.0, 0.0, -200.0];
    });
    s.hold(10).once(|c, _| dodge(c, 1.0, 0.0));
    s.run(10, |_, c, _| c.pitch_input = 0.0);
    s.run(22, |_, c, _| c.vel = [0.0, 0.0, 200.0]);
}

// ---------------------------------------------------------------- wall to air dribble

/// Montée du mur au boost avec la balle sur le capot, décollage, premier toucher `delay` ticks après le départ
/// du mur, puis deux autres à 0,25 s d'écart (chaîne d'air dribble).
fn wall_to_air(delay: usize) -> Vec<Frame> {
    let mut car = wall_car();
    car.boosting = true;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [car.pos[0] - 140.0, 0.0, car.pos[2]],
            vel: car.vel,
        },
    );
    s.hold(20).once(|c, b| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [-100.0, 0.0, 600.0];
        b.vel = c.vel;
        b.pos = [c.pos[0] - 140.0, c.pos[1], c.pos[2]];
    });
    s.hold(delay).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- resets

/// Deux flip resets (et `extra` de plus) dans la même phase aérienne, un dodge entre chaque.
fn resets_in_air(extra: usize) -> Vec<Frame> {
    let mut s = under_ball(800.0);
    reset_on_ball(&mut s);
    for _ in 0..1 + extra {
        leave_and_dodge(&mut s);
        reset_on_ball(&mut s);
    }
    s.hold(20);
    s.frames()
}

/// Un flip reset, retour au sol, nouveau saut sous la balle et second flip reset.
fn resets_with_landing() -> Vec<Frame> {
    let mut s = under_ball(800.0);
    reset_on_ball(&mut s);
    s.once(|c, _| {
        land(c);
        c.pos = [0.0, 0.0, 17.0];
        c.vel = [0.0; 3];
        c.up = [0.0, 0.0, 1.0];
        c.right = [0.0, 1.0, 0.0];
    });
    s.hold(10).once(|c, _| {
        jump(c);
        c.pos = [0.0, 0.0, 825.0];
        c.up = [0.0, 0.0, -1.0];
        c.right = [0.0, -1.0, 0.0];
    });
    reset_on_ball(&mut s);
    s.hold(20);
    s.frames()
}

/// Flip reset sur une balle dont le centre est à `z + 170`, puis la voiture s'en écarte.
fn reset_at(z: f32) -> Vec<Frame> {
    let mut s = under_ball(z);
    reset_on_ball(&mut s);
    s.once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [0.0, 0.0, -200.0];
    });
    s.hold(20);
    s.frames()
}

/// Flip reset, la voiture quitte la balle, (dodge avant), toucher `delay` ticks après le dodge qui envoie la
/// balle à `ball_speed` (+ 300 uu/s vers le haut).
fn reset_shot(with_dodge: bool, delay: usize, ball_speed: f32) -> Vec<Frame> {
    let mut s = under_ball(800.0);
    reset_on_ball(&mut s);
    s.once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [0.0, 0.0, -200.0];
    });
    s.hold(5).once(|c, _| {
        if with_dodge {
            dodge(c, 1.0, 0.0);
        }
        c.vel = [500.0, 0.0, -200.0];
    });
    s.run(delay, |_, c, _| c.pitch_input = 0.0);
    s.once(|c, b| {
        hit(c);
        b.pos = [c.pos[0] + 120.0, c.pos[1], c.pos[2]];
        b.vel = [ball_speed, 0.0, 300.0];
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- dashes

/// Saut, chute, dodge avant au ras du sol, roues au sol 3 ticks après à `land_speed` (wave dash).
fn wave_dash_once(s: &mut Script, land_speed: f32) {
    s.once(|c, _| {
        jump(c);
        c.vel[2] = 300.0;
    });
    s.hold(20).run(20, |_, c, _| c.vel[2] = -300.0);
    s.once(|c, _| {
        dodge(c, 1.0, 0.0);
        c.vel = [land_speed, 0.0, -300.0];
    });
    s.run(2, |_, c, _| c.pitch_input = 0.0);
    s.once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [land_speed, 0.0, 0.0];
    });
}

/// Deux wave dashes (1 000 → 1 500 → 2 000 uu/s), le second `gap` ticks après le premier atterrissage.
fn chain_dash(gap: usize) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1000.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(5);
    wave_dash_once(&mut s, 1500.0);
    s.hold(gap);
    wave_dash_once(&mut s, 2000.0);
    s.hold(10);
    s.frames()
}

/// Saut depuis le mur, dodge avant `delay` ticks après, retour sur le mur 15 ticks après le dodge à `land_vz`
/// (vitesse avant le dodge : 539 uu/s).
fn wall_dash(delay: usize, land_vz: f32) -> Vec<Frame> {
    let mut s = Script::new(wall_car(), ball_far());
    s.hold(10).once(|c, _| {
        jump(c);
        c.vel = [-200.0, 0.0, 500.0];
    });
    s.hold(delay).once(|c, _| {
        dodge(c, 1.0, 0.0);
        c.vel = [-200.0, 0.0, 900.0];
    });
    s.run(14, |_, c, _| c.pitch_input = 0.0);
    s.once(|c, _| {
        land(c);
        c.pos[0] = 4079.0;
        c.vel = [0.0, 0.0, land_vz];
    });
    s.hold(10);
    s.frames()
}

// ---------------------------------------------------------------- contrôle aérien

/// Saut avec tangage `pitch` (+1 = nez vers le haut), second saut `gap` ticks après, boost ou non.
fn fast_aerial(boost: bool, gap: usize, pitch: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.boosting = boost;
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.pitch_input = pitch;
        c.vel = [0.0, 0.0, 300.0];
    });
    s.hold(gap - 1).once(|c, _| {
        c.has_double_jumped = true;
        c.vel = [0.0, 0.0, 600.0];
    });
    s.hold(60);
    s.frames()
}

/// Entrées et couple d'un dodge en l'air au tick du dodge d'un stall : `inputs` = (tangage, lacet, roulis),
/// `torque` = couple de dodge (avant, latéral), `after` = vitesse juste après (la vitesse horizontale ne doit
/// pas bouger pour un vrai stall).
struct StallSpec {
    inputs: (f32, f32, f32),
    torque: (f32, f32),
    before: [f32; 3],
    after: [f32; 3],
    up: [f32; 3],
}

/// En l'air à z = 800, dodge selon `spec`, puis 0,25 s sans rien d'autre.
fn stall(spec: StallSpec) -> Vec<Frame> {
    let mut car = car_in_air(spec.before);
    car.up = spec.up;
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        c.has_flipped = true;
        c.is_flipping = true;
        c.has_flip_or_jump = false;
        c.flip_rel_torque = [spec.torque.0, spec.torque.1, 0.0];
        (c.pitch_input, c.yaw_input, c.roll_input) = spec.inputs;
        c.vel = spec.after;
    });
    s.hold(30);
    s.frames()
}

/// Le vrai geste : lacet droit + roulis gauche, flip annulé (couple nul), vitesse verticale tuée.
fn real_stall() -> StallSpec {
    StallSpec {
        inputs: (0.0, 1.0, -1.0),
        torque: (0.0, 0.0),
        before: [300.0, 0.0, -600.0],
        after: [300.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
    }
}

/// En l'air, rotation autour de l'axe `up` à `spin` rad/s pendant `ticks`, puis plus de rotation.
fn tornado(ticks: usize, spin: f32) -> Vec<Frame> {
    let mut s = Script::new(car_in_air([500.0, 0.0, 0.0]), ball_far());
    s.run(ticks, |_, c, _| c.ang_vel = [0.0, 0.0, spin]);
    s.run(20, |_, c, _| c.ang_vel = [0.0; 3]);
    s.frames()
}

/// Saut à 1 200 uu/s, `ticks` en l'air (tonneau complet si `flip`), atterrissage à plat à `land_speed`.
fn recovery(ticks: usize, flip: bool, land_speed: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1200.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.vel = [1200.0, 0.0, 400.0];
    });
    s.run(ticks, |i, c, _| {
        let a = if flip {
            core::f32::consts::TAU * (i as f32 + 1.0) / ticks as f32
        } else {
            0.0
        };
        c.up = [0.0, -a.sin(), a.cos()];
        c.right = [0.0, a.cos(), a.sin()];
        c.vel = [1100.0, 0.0, 400.0];
    });
    s.once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.up = [0.0, 0.0, 1.0];
        c.right = [0.0, 1.0, 0.0];
        c.vel = [land_speed, 0.0, 0.0];
    });
    s.hold(10);
    s.frames()
}

pub fn cases() -> Vec<Case> {
    use Mechanic::*;
    vec![
        // Wall to air dribble.
        case(
            "wall_to_air_dribble_ext",
            WallToAirDribble,
            true,
            wall_to_air(60),
            &[
                (Touch, 3),
                (AerialTouch, 3),
                (AirDribble, 1),
                (WallToAirDribble, 1),
            ],
        ),
        case(
            "wall_to_air_dribble_negative_late",
            WallToAirDribble,
            false,
            wall_to_air(200),
            &[(Touch, 3), (AerialTouch, 3), (AirDribble, 1)],
        ),
        case(
            "wall_to_air_dribble_negative_ground",
            WallToAirDribble,
            false,
            crate::script::air_dribble_sequence(),
            &[(Touch, 3), (AerialTouch, 3), (AirDribble, 1)],
        ),
        // Double reset.
        case(
            "double_reset",
            DoubleReset,
            true,
            resets_in_air(0),
            &[(FlipReset, 2), (DoubleReset, 1)],
        ),
        case(
            "double_reset_triple",
            DoubleReset,
            true,
            resets_in_air(1),
            &[(FlipReset, 3), (DoubleReset, 1)],
        ),
        case(
            "double_reset_negative_landing",
            DoubleReset,
            false,
            resets_with_landing(),
            &[(FlipReset, 2)],
        ),
        // Ceiling reset.
        case(
            "ceiling_reset",
            CeilingReset,
            true,
            reset_at(1650.0),
            &[(FlipReset, 1), (CeilingReset, 1)],
        ),
        case(
            "ceiling_reset_negative_low",
            CeilingReset,
            false,
            reset_at(1500.0),
            &[(FlipReset, 1)],
        ),
        // Reset shot.
        case(
            "reset_shot",
            ResetShot,
            true,
            reset_shot(true, 10, 2000.0),
            &[(Touch, 1), (FlipReset, 1), (ResetShot, 1)],
        ),
        case(
            "reset_shot_negative_slow",
            ResetShot,
            false,
            reset_shot(true, 10, 1200.0),
            &[(Touch, 1), (FlipReset, 1)],
        ),
        case(
            "reset_shot_negative_late",
            ResetShot,
            false,
            reset_shot(true, 50, 2000.0),
            &[(Touch, 1), (FlipReset, 1)],
        ),
        case(
            "reset_shot_negative_no_dodge",
            ResetShot,
            false,
            reset_shot(false, 10, 2000.0),
            &[(Touch, 1), (FlipReset, 1)],
        ),
        // Chain dash.
        case(
            "chain_dash",
            ChainDash,
            true,
            chain_dash(20),
            &[(WaveDash, 2), (ChainDash, 1)],
        ),
        case(
            "chain_dash_negative_spaced",
            ChainDash,
            false,
            chain_dash(150),
            &[(WaveDash, 2)],
        ),
        // Wall dash.
        case(
            "wall_dash",
            WallDash,
            true,
            wall_dash(10, 950.0),
            &[(WallDash, 1)],
        ),
        case(
            "wall_dash_negative_late_dodge",
            WallDash,
            false,
            wall_dash(50, 950.0),
            &[],
        ),
        case(
            "wall_dash_negative_no_gain",
            WallDash,
            false,
            wall_dash(10, 600.0),
            &[],
        ),
        // Fast aerial.
        case(
            "fast_aerial",
            FastAerial,
            true,
            fast_aerial(true, 10, 1.0),
            &[(FastAerial, 1)],
        ),
        case(
            "fast_aerial_negative_no_boost",
            FastAerial,
            false,
            fast_aerial(false, 10, 1.0),
            &[],
        ),
        case(
            "fast_aerial_negative_late",
            FastAerial,
            false,
            fast_aerial(true, 60, 1.0),
            &[],
        ),
        case(
            "fast_aerial_negative_nose_down",
            FastAerial,
            false,
            fast_aerial(true, 10, -1.0),
            &[],
        ),
        // Stall.
        case("stall", Stall, true, stall(real_stall()), &[(Stall, 1)]),
        // Même geste avec lacet à gauche et roulis à droite.
        case(
            "stall_mirrored",
            Stall,
            true,
            stall(StallSpec {
                inputs: (0.0, -1.0, 1.0),
                ..real_stall()
            }),
            &[(Stall, 1)],
        ),
        // Faux positif d'origine : backflip (tangage +1) retourné dont l'impulsion freine la voiture.
        case(
            "stall_negative_inverted_backflip",
            Stall,
            false,
            stall(StallSpec {
                inputs: (1.0, 0.0, 0.0),
                torque: (-1.0, 0.0),
                before: [900.0, 0.0, -300.0],
                after: [300.0, 0.0, -300.0],
                up: [0.0, 0.0, -1.0],
            }),
            &[],
        ),
        // Flip latéral avec roulis : couple non nul, rotation de dodge.
        case(
            "stall_negative_side_flip",
            Stall,
            false,
            stall(StallSpec {
                inputs: (0.0, 0.0, 1.0),
                torque: (0.0, 1.0),
                before: [300.0, 0.0, -600.0],
                after: [300.0, 500.0, -100.0],
                up: [0.0, 0.0, 1.0],
            }),
            &[],
        ),
        case(
            "stall_negative_front_flip",
            Stall,
            false,
            stall(StallSpec {
                inputs: (-1.0, 0.0, 0.0),
                torque: (1.0, 0.0),
                before: [300.0, 0.0, -600.0],
                after: [300.0, 0.0, 0.0],
                up: [0.0, 0.0, 1.0],
            }),
            &[],
        ),
        // Entrées opposées mais la voiture continue de tomber : pas de vitesse verticale annulée.
        case(
            "stall_negative_still_falling",
            Stall,
            false,
            stall(StallSpec {
                after: [300.0, 0.0, -600.0],
                ..real_stall()
            }),
            &[],
        ),
        // Lacet et roulis du même signe : ils ne s'annulent pas.
        case(
            "stall_negative_same_sign",
            Stall,
            false,
            stall(StallSpec {
                inputs: (0.0, 1.0, 1.0),
                ..real_stall()
            }),
            &[],
        ),
        // Tornado spin.
        case(
            "tornado_spin",
            TornadoSpin,
            true,
            tornado(50, 6.0),
            &[(TornadoSpin, 1)],
        ),
        case(
            "tornado_spin_negative_short",
            TornadoSpin,
            false,
            tornado(30, 6.0),
            &[],
        ),
        case(
            "tornado_spin_negative_slow",
            TornadoSpin,
            false,
            tornado(50, 3.0),
            &[],
        ),
        // Recovery.
        case(
            "recovery",
            Recovery,
            true,
            recovery(90, true, 1000.0),
            &[(Recovery, 1)],
        ),
        case(
            "recovery_negative_slow",
            Recovery,
            false,
            recovery(90, true, 600.0),
            &[],
        ),
        case(
            "recovery_negative_short",
            Recovery,
            false,
            recovery(50, true, 1000.0),
            &[],
        ),
        case(
            "recovery_negative_upright",
            Recovery,
            false,
            recovery(90, false, 1000.0),
            &[],
        ),
    ]
}
