//! Mesure du drill `turnaround` : temps pour faire face à la cible (+Y) avec de la vitesse dans cette direction,
//! voiture partie dos à la cible, en demi-tour classique (roulant) ou en half flip scripté. Sert à régler le barème
//! de `rl_env::drill::TurnTrack` (voir `docs/drills.md`). Usage : `turnaround_bench` (meshes via ROCKETSIM_MESHES_DIR).
use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

const MIN_COS: f32 = 0.866;
const MAX_TICKS: u32 = 480;

/// Une exécution : renvoie le temps (s) au bout duquel le critère est tenu, ou None.
fn run(min_speed: f32, v0: f32, yaw_off: f32, boost: f32, mut ctl: impl FnMut(u32, &mut Sim, u32) -> Controls) -> Option<f32> {
    let mut sim = Sim::new_soccar().unwrap();
    let car = sim.add_car(true);
    let yaw = -std::f32::consts::FRAC_PI_2 + yaw_off; // dos à la cible (+Y)
    sim.reset_car(car, [0.0, 0.0, 17.0], yaw, [yaw.cos() * v0, yaw.sin() * v0, 0.0], boost)
        .unwrap();
    sim.reset_ball([0.0, 4000.0, 93.0], [0.0; 3]);
    for t in 0..MAX_TICKS {
        let c = ctl(t, &mut sim, car);
        sim.set_controls(car, c).unwrap();
        sim.step(1);
        let ct = sim.car_tick(car);
        if ct.on_ground {
            let h = ct.forward[0].hypot(ct.forward[1]).max(1e-3);
            let cos = ct.forward[1] / h;
            if cos >= MIN_COS && ct.vel[1] >= min_speed {
                return Some((t + 1) as f32 / 120.0);
            }
        }
    }
    None
}

fn roll_turn(steer_gain: f32, boost: bool, handbrake: bool, reverse_first: bool) -> impl FnMut(u32, &mut Sim, u32) -> Controls {
    // steer > 0 = lacet croissant (sens trigonométrique) ; ang = écart horaire au cap +Y. Près de ±π le sens est
    // imposé (+1) pour ne pas osciller.
    move |t, sim, car| {
        let ct = sim.car_tick(car);
        let ang = ct.forward[0].atan2(ct.forward[1]);
        let steer = if ang.abs() > 2.9 { 1.0 } else { (ang * steer_gain).clamp(-1.0, 1.0) };
        let throttle = if reverse_first && t < 20 && ct.forward[1] < 0.0 { -1.0 } else { 1.0 };
        Controls {
            throttle,
            steer,
            boost,
            handbrake: handbrake && ang.abs() > 0.8,
            ..Default::default()
        }
    }
}

/// Half flip : saut, dodge arrière, annulation (tangage inverse) avec roulis, boost ; ensuite conduite vers +Y.
fn half_flip(dodge_at: u32, cancel_at: u32, cancel_len: u32, roll: f32, yaw: f32) -> impl FnMut(u32, &mut Sim, u32) -> Controls {
    move |t, sim, car| {
        let ct = sim.car_tick(car);
        let mut c = Controls { throttle: 1.0, boost: t > dodge_at, ..Default::default() };
        if t < 2 {
            c.jump = true;
        } else if t == dodge_at {
            c.jump = true;
            c.pitch = 1.0; // dodge arrière
        } else if t > dodge_at && t < dodge_at + cancel_at {
            c.pitch = 1.0;
        } else if t >= dodge_at + cancel_at && t < dodge_at + cancel_at + cancel_len {
            c.pitch = -1.0;
            c.roll = roll;
            c.yaw = yaw;
        } else if !ct.on_ground {
            c.roll = roll;
            c.yaw = yaw;
        }
        if ct.on_ground && t > dodge_at + 20 {
            let ang = ct.forward[0].atan2(ct.forward[1]);
            c.steer = (ang * 2.0).clamp(-1.0, 1.0);
        }
        c
    }
}

fn main() {
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("init");
    if std::env::args().nth(1).as_deref() == Some("debug") {
        let mut f = roll_turn(2.0, true, false, false);
        run(700.0, 0.0, 0.0, 100.0, |t, s, c| {
            let ct = s.car_tick(c);
            if t % 30 == 0 {
                println!("t={t} pos={:?} fwd=({:.2},{:.2}) v=({:.0},{:.0}) ground={}", ct.pos, ct.forward[0], ct.forward[1], ct.vel[0], ct.vel[1], ct.on_ground);
            }
            f(t, s, c)
        });
        return;
    }
    let vmin: f32 = std::env::var("VMIN").ok().and_then(|v| v.parse().ok()).unwrap_or(700.0);
    println!("vitesse mini vers la cible : {vmin} uu/s");
    for (label, v0, boost) in [("à l'arrêt", 0.0, 100.0), ("roulant 700 uu/s (dos à la cible)", 700.0, 100.0), ("roulant 1400 uu/s", 1400.0, 100.0), ("à l'arrêt, boost 0", 0.0, 0.0)] {
        // Meilleur demi-tour en roulant.
        let mut best_roll: Option<(f32, String)> = None;
        for gain in [1.0f32, 2.0, 4.0] {
            for b in [false, true] {
                for hb in [false, true] {
                    for rev in [false, true] {
                        if let Some(t) = run(vmin, v0, 0.0, boost, roll_turn(gain, b && boost > 0.0, hb, rev))
                            && best_roll.as_ref().is_none_or(|(bt, _)| t < *bt)
                        {
                            best_roll = Some((t, format!("gain {gain} boost {b} frein à main {hb} marche arrière d'abord {rev}")));
                        }
                    }
                }
            }
        }
        // Meilleur half flip scripté.
        let mut best_hf: Option<(f32, String)> = None;
        let mut hf_runs = 0;
        for dodge_at in [3u32, 6, 10, 15] {
            for cancel_at in [3u32, 5, 8, 12, 16] {
                for cancel_len in [10u32, 15, 20, 30, 40] {
                    for roll in [-1.0f32, 1.0] {
                        for yaw in [-1.0f32, 0.0, 1.0] {
                            hf_runs += 1;
                            if let Some(t) = run(vmin, v0, 0.0, boost, half_flip(dodge_at, cancel_at, cancel_len, roll, yaw))
                                && best_hf.as_ref().is_none_or(|(bt, _)| t < *bt)
                            {
                                best_hf = Some((t, format!("dodge t={dodge_at} annulation après {cancel_at} durée {cancel_len} roulis {roll} lacet {yaw}")));
                            }
                        }
                    }
                }
            }
        }
        println!("## {label}");
        println!("  demi-tour en roulant : {best_roll:?}");
        println!("  half flip scripté   : {best_hf:?} ({hf_runs} essais)");
    }
}
