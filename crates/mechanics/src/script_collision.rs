//! Séquences scriptées : collisions voiture-voiture, touchers simultanés, car pinch, bump, demo.

use crate::script::{Case, Frame, Script, ball_far, car_on_ground, case, dodge, hit};
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
fn spin_after(contact: bool, roll: f32) -> Vec<Frame> {
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
        c.roll_input = roll;
    });
    s.run(80, |_, c, _| {
        c.ang_vel = [0.0, 0.0, 6.0];
        c.roll_input = roll;
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
    car.pos = [0.0, -4000.0, car_z];
    // La balle file déjà depuis 12 ticks (tir stable) et arrive en y = −4000 (< 1 800 uu de la ligne) au toucher.
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, -4000.0 - v_before_y * 0.1, ball_z],
            vel: [0.0, v_before_y, 0.0],
        },
    );
    s.hold(11).once(|c, b| {
        hit(c);
        b.vel = v_after;
    });
    s.hold(20);
    s.frames()
}

/// Aérien sans saut : la voiture tombe d'un mur à z = 150 (aucun saut vu), la balle est arrêtée : un save, pas un aerial save.
fn fall_save() -> Vec<Frame> {
    let mut f = save_seq(300.0, 150.0, true, -2000.0, [0.0, 1200.0, 200.0]);
    for (c, _) in f.iter_mut() {
        c.has_jumped = false;
    }
    f
}

/// Coup d'envoi simultané (clip de prod 1795095498-save-2038) : balle posée au centre, l'autre voiture la touche
/// au tick 12 (elle part vers le but bleu, y < 0), la nôtre 1 tick plus tard (elle repart vers le camp orange).
/// `rest` faux : la balle roulait déjà vers notre but avant (vrai tir) et le toucher adverse l'accélère.
pub(crate) fn kickoff_two_touch(rest: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [0.0; 3];
    car.pos = [0.0, -300.0, 17.0];
    let v0: f32 = if rest { 0.0 } else { -1500.0 };
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, -(v0.abs()) * 0.1, 93.0],
            vel: [0.0, v0, 0.0],
        },
    );
    s.car.other = Some(other([0.0, 300.0, 17.0]));
    s.hold(11).once(|c, b| {
        let mut o = other([0.0, 300.0, 17.0]);
        o.ball_hit_tick = Some(c.tick);
        c.other = Some(o);
        b.vel = [0.0, -1800.0, 0.0];
    });
    s.once(|c, b| {
        hit(c);
        let mut o = other([0.0, 300.0, 17.0]);
        o.ball_hit_tick = Some(c.tick - 1);
        c.other = Some(o);
        b.vel = [0.0, 2500.0, 300.0];
    });
    s.hold(20);
    s.frames()
}

/// Reconstitution du clip de prod 1795074459-redirect-311 (squishy save validé) : la voiture monte la paroi
/// intérieure de son but (x = −878, y = −5 404, haut de la voiture vers +x), en sort, dodge, et touche à
/// (−767, −4 114, 214) la balle qui file à (1 578, −733, 267) (elle coupait la ligne à x ≈ +1 410, hors des poteaux) ;
/// la balle repart à (903, 597, −1 588). Vol de ≈ 1,8 s.
fn squish_311() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [0.0; 3];
    car.pos = [-878.0, -5404.0, 124.0];
    car.up = [1.0, 0.0, 0.0];
    let air = 210;
    let total = (10 + 1 + air + 1 + 10) as f32 / 120.0;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [
                -767.0 - 1578.0 * total,
                -4114.0 + 733.0 * total,
                214.0 - 267.0 * total,
            ],
            vel: [1578.0, -733.0, 267.0],
        },
    );
    s.hold(10).once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.up = [0.0, 0.0, 1.0];
    });
    s.hold(air).once(|c, _| dodge(c, 1.0, 0.0));
    s.hold(10).once(|c, b| {
        c.pos = [-748.0, -4172.0, 368.0];
        hit(c);
        b.vel = [903.0, 597.0, -1588.0];
    });
    s.hold(20);
    s.frames()
}

/// Reconstitution du clip de prod 1795107331-squishy_save-2800 (faux squishy save) : l'agent monte le mur du fond
/// à côté de son but (x ≈ −1 600, hors des poteaux), la balle roule vers le coin à (1 165, −852, −146), touche la
/// voiture posée sur le mur et rebondit à (236, 747, −54). Elle n'allait pas dans le but.
fn squish_2800() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [0.0; 3];
    car.pos = [-1583.0, -5104.0, 158.0];
    car.forward = [-0.31, 0.0, 0.95];
    car.right = [0.95, 0.0, 0.31];
    car.up = [0.02, 1.0, 0.01];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [-1634.0 - 116.5, -4976.0 + 85.2, 109.0 + 14.6],
            vel: [1165.0, -852.0, -146.0],
        },
    );
    s.hold(11).once(|c, b| {
        hit(c);
        b.vel = [236.0, 747.0, -54.0];
    });
    s.hold(20);
    s.frames()
}

/// Point de départ de la voiture dans `squish_net` : position, orientation (haut de la voiture) et contact roues.
struct NetStart {
    pos: [f32; 3],
    up: [f32; 3],
}

impl NetStart {
    /// Roues sur le fond du filet de son but (bleu, y < 0).
    const BACK_NET: Self = Self {
        pos: [0.0, -5700.0, 400.0],
        up: [0.0, 1.0, 0.0],
    };
    /// Roues sur le plafond du but.
    const GOAL_CEILING: Self = Self {
        pos: [0.0, -5500.0, 620.0],
        up: [0.0, 0.0, -1.0],
    };
    /// Mur du fond à côté du but (clip 2800) : hors de la largeur du but.
    const BACK_WALL_OUTSIDE: Self = Self {
        pos: [-1600.0, -5100.0, 400.0],
        up: [0.0, 1.0, 0.0],
    };
    /// Sol du but : pas un fond de filet.
    const GOAL_FLOOR: Self = Self {
        pos: [0.0, -5500.0, 17.0],
        up: [0.0, 0.0, 1.0],
    };
}

/// Squishy save : la voiture, roues sur `start`, décolle (flip disponible), reste `air` ticks en l'air, fait un
/// dodge puis touche une balle qui fonçait vers le but bleu (arrêt : elle repart vers le terrain).
fn squish_net(start: &NetStart, air: usize) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [0.0; 3];
    car.pos = start.pos;
    car.up = start.up;
    // 10 + 1 + air + 1 + 10 ticks avant le toucher ; la balle arrive en (0, −3000, 300) à 2 000 uu/s.
    let total = (10 + 1 + air + 1 + 10) as f32;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, -4000.0 - 2000.0 * total / 120.0, 300.0],
            vel: [0.0, -2000.0, 0.0],
        },
    );
    s.hold(10).once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.up = [0.0, 0.0, 1.0];
    });
    s.hold(air).once(|c, _| dodge(c, 1.0, 0.0));
    s.hold(10).once(|c, b| {
        c.pos = [0.0, -4000.0, 400.0];
        hit(c);
        b.vel = [0.0, 1200.0, 200.0];
    });
    s.hold(20);
    s.frames()
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
    s.hold(130);
    s.frames()
}

pub(crate) fn cases() -> Vec<Case> {
    use Mechanic::*;
    vec![
        case(
            "tornado_spin_sans_collision",
            TornadoSpin,
            true,
            spin_after(false, 1.0),
            &[(TornadoSpin, 1)],
        ),
        case(
            "tornado_spin_negatif_sans_roulis_en_entree",
            TornadoSpin,
            false,
            spin_after(false, 0.0),
            &[],
        ),
        case(
            "tornado_spin_apres_collision",
            TornadoSpin,
            false,
            spin_after(true, 1.0),
            &[],
        ),
        case(
            "pinch_negatif_50_50_voiture_adverse",
            Pinch,
            false,
            two_hit([0.0, 0.0, 2400.0], 200.0, true),
            &[(Touch, 1)],
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
            "aerial_save_negatif_chute_du_mur",
            AerialSave,
            false,
            fall_save(),
            &[(Touch, 1), (Save, 1)],
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
            squish_net(&NetStart::BACK_NET, 30),
            &[(Touch, 1), (Save, 1), (AerialSave, 1), (SquishySave, 1)],
        ),
        case(
            "squishy_save_plafond_du_but",
            SquishySave,
            true,
            squish_net(&NetStart::GOAL_CEILING, 30),
            &[(Touch, 1), (Save, 1), (AerialSave, 1), (SquishySave, 1)],
        ),
        case(
            "squishy_save_negatif_montee_du_fond_hors_du_but",
            SquishySave,
            false,
            squish_net(&NetStart::BACK_WALL_OUTSIDE, 30),
            &[(Touch, 1), (Save, 1), (AerialSave, 1)],
        ),
        case(
            "squishy_save_negatif_depart_du_sol_du_but",
            SquishySave,
            false,
            squish_net(&NetStart::GOAL_FLOOR, 30),
            &[(Touch, 1), (Save, 1), (AerialSave, 1)],
        ),
        case(
            "squishy_save_negatif_en_l_air_depuis_trop_longtemps",
            SquishySave,
            false,
            squish_net(&NetStart::BACK_NET, 300),
            &[(Touch, 1), (Save, 1), (AerialSave, 1)],
        ),
        case(
            "squishy_save_negatif_save_au_sol",
            SquishySave,
            false,
            save_seq(300.0, 17.0, false, -2000.0, [0.0, 1200.0, 200.0]),
            &[(Touch, 1), (Save, 1)],
        ),
        case(
            "squishy_save_clip_redirect_311",
            SquishySave,
            true,
            squish_311(),
            &[(Touch, 1), (Redirect, 1), (SquishySave, 1)],
        ),
        case(
            "squishy_save_negatif_clip_2800_mur_du_fond_hors_des_poteaux",
            SquishySave,
            false,
            squish_2800(),
            &[(Touch, 1)],
        ),
        case(
            "clear",
            Clear,
            true,
            clear_seq(-200.0, 2400.0),
            &[(Touch, 1), (Clear, 1)],
        ),
        case(
            "clear_negatif_reste_dans_le_tiers_defensif",
            Clear,
            false,
            clear_seq(-200.0, 1500.0),
            &[(Touch, 1)],
        ),
        case(
            "clear_negatif_balle_deja_partie",
            Clear,
            false,
            clear_seq(1800.0, 2400.0),
            &[(Touch, 1)],
        ),
        case(
            "save_negatif_coup_d_envoi_simultane",
            Save,
            false,
            kickoff_two_touch(true),
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
