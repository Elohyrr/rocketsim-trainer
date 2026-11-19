//! Séquences scriptées : collisions voiture-voiture, touchers simultanés, car pinch, bump, demo.

use crate::script::{Case, Frame, Script, ball_far, car_on_ground, case, hit};
use crate::{BallTick, CarTick, Mechanic, OtherCar};

/// Voiture en l'air depuis 1 s à 500 uu/s, à plat.
fn air_car() -> CarTick {
    CarTick {
        pos: [0.0, 0.0, 800.0],
        vel: [500.0, 0.0, 0.0],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
        has_jumped: true,
        air_time: 1.0,
        ..Default::default()
    }
}

fn other(pos: [f32; 3]) -> OtherCar {
    OtherCar {
        pos,
        ..Default::default()
    }
}

/// Choc subi en l'air à côté d'une autre voiture (`near`) ou loin d'elle, suivi d'une rotation rapide de 0,4 s :
/// un vrai tornado spin si `contact` est faux, une rotation subie sinon.
fn spin_after(contact: bool) -> Vec<Frame> {
    let mut s = Script::new(air_car(), ball_far());
    s.car.other = Some(other([3000.0, 0.0, 800.0]));
    s.hold(45);
    s.once(|c, _| {
        if contact {
            c.contact_other = true;
            c.contact_cooldown = 0.25;
            c.other = Some(other([c.pos[0] + 120.0, 0.0, c.pos[2]]));
            c.vel = [200.0, 0.0, 0.0];
        }
        c.ang_vel = [0.0, 0.0, 6.0];
    });
    s.run(50, |_, c, _| {
        c.ang_vel = [0.0, 0.0, 6.0];
        if contact {
            c.other = Some(other([c.pos[0] + 120.0, 0.0, c.pos[2]]));
        }
    });
    s.run(20, |_, c, _| c.ang_vel = [0.0; 3]);
    s.frames()
}

/// Les deux voitures frappent la balle (au repos) au même tick, face à face ; la balle part à `v_after`.
/// `gap` : distance de l'autre voiture à la balle ; `other_touches` : si elle la touche aussi.
fn two_hit(v_after: [f32; 3], gap: f32, other_touches: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1500.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [150.0, 0.0, 93.0],
            vel: [0.0; 3],
        },
    );
    s.car.other = Some(other([150.0 + gap, 0.0, 17.0]));
    s.hold(10).once(|c, b| {
        hit(c);
        b.vel = v_after;
        let mut o = other([150.0 + gap, 0.0, 17.0]);
        o.vel = [-1500.0, 0.0, 0.0];
        if other_touches {
            o.ball_hit_tick = Some(c.tick);
        }
        c.other = Some(o);
        c.contact_other = true;
        c.contact_cooldown = 0.25;
        c.vel = [-300.0, 0.0, 0.0];
    });
    s.hold(20);
    s.frames()
}

/// Choc au sol : l'autre voiture reçoit `odv` uu/s, nous `dv` ; `demo` : elle est démolie ensuite.
fn bump_seq(odv: f32, dv: f32, near: bool, demo: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1400.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    let x = if near { 130.0 } else { 3000.0 };
    s.car.other = Some(other([x, 0.0, 17.0]));
    s.hold(10).once(|c, _| {
        c.contact_other = near;
        c.contact_cooldown = 0.25;
        c.vel[0] -= dv;
        let mut o = other([x, 0.0, 17.0]);
        o.vel = [odv, 0.0, 0.0];
        c.other = Some(o);
    });
    s.hold(5).once(|c, _| {
        if let Some(o) = c.other.as_mut() {
            o.demoed = demo;
        }
    });
    s.hold(10);
    s.frames()
}

/// Arrêt : la balle fonce vers le but bleu (y < 0) à 2 000 uu/s ; la voiture (au sol, ou en l'air à `z` si
/// `aerial`) la touche en `ball_z` et elle repart à `v_after`. `car_z` = hauteur de la voiture.
fn save_seq(
    ball_z: f32,
    car_z: f32,
    aerial: bool,
    v_before_y: f32,
    v_after: [f32; 3],
) -> Vec<Frame> {
    let mut car = if aerial { air_car() } else { car_on_ground() };
    car.vel = [0.0; 3];
    car.pos = [0.0, -3000.0, car_z];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, -3000.0, ball_z],
            vel: [0.0, v_before_y, 0.0],
        },
    );
    s.hold(1).once(|c, b| {
        hit(c);
        b.vel = v_after;
    });
    s.hold(20);
    s.frames()
}

/// Reconstitution du clip de prod 1795074459-redirect-311 : balle à (−767, −4114, 214) à (1578, −733, 267),
/// voiture en l'air à 368 de haut qui l'écrase : elle repart à (903, 597, −1588).
fn squish_311() -> Vec<Frame> {
    let mut car = air_car();
    car.vel = [0.0; 3];
    car.pos = [-748.0, -4172.0, 368.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [-767.0, -4114.0, 214.0],
            vel: [1578.0, -733.0, 267.0],
        },
    );
    s.car.pos = [-748.0, -4172.0, 368.0];
    s.ball.pos = [-767.0, -4114.0, 214.0];
    s.hold(1).once(|c, b| {
        hit(c);
        b.vel = [903.0, 597.0, -1588.0];
    });
    s.hold(20);
    s.frames()
}

/// Même écrasement mais au milieu du terrain (la balle ne menace pas le but).
fn squish_far() -> Vec<Frame> {
    let mut f = squish_311();
    for (c, b) in f.iter_mut() {
        c.pos[1] += 3500.0;
        b.pos[1] += 3500.0;
    }
    f
}

/// Dégagement au sol depuis le tiers défensif : balle à y = −3500, `vy_before` avant, 2 400 uu/s après.
fn clear_seq(vy_before: f32, after: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [0.0; 3];
    car.pos = [0.0, -3600.0, 17.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, -3500.0, 120.0],
            vel: [0.0, vy_before, 0.0],
        },
    );
    s.hold(1).once(|c, b| {
        hit(c);
        b.vel = [0.0, after, 0.0];
    });
    s.hold(20);
    s.frames()
}

pub(crate) fn cases() -> Vec<Case> {
    use Mechanic::*;
    vec![
        case(
            "tornado_spin_sans_collision",
            TornadoSpin,
            true,
            spin_after(false),
            &[(TornadoSpin, 1)],
        ),
        case(
            "tornado_spin_apres_collision",
            TornadoSpin,
            false,
            spin_after(true),
            &[],
        ),
        case(
            "car_pinch_kickoff",
            Pinch,
            true,
            two_hit([0.0, 0.0, 2400.0], 200.0, true),
            &[(Touch, 1), (Pinch, 1)],
        ),
        case(
            "car_pinch_negatif_un_seul_toucheur",
            Pinch,
            false,
            two_hit([0.0, 0.0, 2400.0], 200.0, false),
            &[(Touch, 1)],
        ),
        case(
            "power_shot_invalide_par_toucher_simultane",
            PowerShot,
            false,
            two_hit([3500.0, 0.0, 0.0], 800.0, true),
            &[(Touch, 1)],
        ),
        case(
            "save",
            Save,
            true,
            save_seq(300.0, 17.0, false, -2000.0, [0.0, 1200.0, 200.0]),
            &[(Touch, 1), (Save, 1)],
        ),
        case(
            "save_negatif_balle_deja_hors_but",
            Save,
            false,
            save_seq(300.0, 17.0, false, 800.0, [0.0, 1200.0, 200.0]),
            &[(Touch, 1)],
        ),
        case(
            "save_negatif_balle_toujours_au_but",
            Save,
            false,
            save_seq(300.0, 17.0, false, -2000.0, [0.0, -1800.0, 0.0]),
            &[(Touch, 1)],
        ),
        case(
            "aerial_save",
            AerialSave,
            true,
            save_seq(500.0, 400.0, true, -2000.0, [0.0, 1200.0, 200.0]),
            &[(Touch, 1), (Save, 1), (AerialSave, 1)],
        ),
        case(
            "aerial_save_negatif_au_sol",
            AerialSave,
            false,
            save_seq(300.0, 17.0, false, -2000.0, [0.0, 1200.0, 200.0]),
            &[(Touch, 1), (Save, 1)],
        ),
        case(
            "squishy_save",
            SquishySave,
            true,
            save_seq(100.0, 300.0, true, -1500.0, [0.0, -100.0, 0.0]),
            &[
                (Touch, 1),
                (Cushion, 1),
                (Save, 1),
                (AerialSave, 1),
                (SquishySave, 1),
            ],
        ),
        case(
            "squishy_save_negatif_balle_libre",
            SquishySave,
            false,
            save_seq(300.0, 17.0, false, -1500.0, [0.0, -100.0, 0.0]),
            &[(Touch, 1), (Cushion, 1), (Save, 1)],
        ),
        case(
            "squishy_save_clip_redirect_311",
            SquishySave,
            true,
            squish_311(),
            &[(Touch, 1), (Redirect, 1), (SquishySave, 1)],
        ),
        case(
            "squishy_save_negatif_balle_loin_du_but",
            SquishySave,
            false,
            squish_far(),
            &[(Touch, 1), (Redirect, 1)],
        ),
        case(
            "clear",
            Clear,
            true,
            clear_seq(-200.0, 2400.0),
            &[(Touch, 1), (Clear, 1)],
        ),
        case(
            "clear_negatif_balle_deja_partie",
            Clear,
            false,
            clear_seq(1800.0, 2400.0),
            &[(Touch, 1)],
        ),
        case(
            "bump",
            Bump,
            true,
            bump_seq(900.0, 100.0, true, false),
            &[(Bump, 1)],
        ),
        case(
            "bump_negatif_loin",
            Bump,
            false,
            bump_seq(900.0, 100.0, false, false),
            &[],
        ),
        case(
            "demo",
            Demo,
            true,
            bump_seq(900.0, 100.0, true, true),
            &[(Bump, 1), (Demo, 1)],
        ),
        case(
            "demo_negatif_sans_demolition",
            Demo,
            false,
            bump_seq(900.0, 100.0, true, false),
            &[(Bump, 1)],
        ),
    ]
}
