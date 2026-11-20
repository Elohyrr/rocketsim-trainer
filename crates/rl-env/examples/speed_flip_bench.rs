//! Durée réelle d'un speed flip scripté, selon la résolution de décision (`Q` ticks : 1 = 120 Hz, 4 = 30 Hz, 8 = 15 Hz).
//! Coup d'envoi en diagonale, boost permanent. Pour chaque essai (saut tenu H décisions, relâche R, dodge diagonal,
//! annulation `p+1` après C décisions pendant L décisions) : ticks entre le dodge et le retour au sol (`dt`), vitesse
//! horizontale à l'atterrissage, temps pour parcourir 1 500 uu depuis le départ.
//! Usage : `speed_flip_bench [N=20000] [Q=4,8]` (meshes via ROCKETSIM_MESHES_DIR).
use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % u64::from(hi - lo + 1)) as u32
    }
}

#[derive(Clone, Copy, Debug)]
struct P {
    q: u32,
    hold: u32,
    rel: u32,
    cancel_after: u32,
    cancel_len: u32,
    roll: f32,
}

#[derive(Clone, Copy, Debug)]
struct R {
    land_dt: u32,
    hspeed: f32,
    t1500: f32,
}

type Best = Option<(R, P)>;

fn run(sim: &mut Sim, car: u32, p: P) -> Option<R> {
    let yaw = std::f32::consts::FRAC_PI_4;
    sim.reset_car(car, [-2048.0, -2560.0, 17.0], yaw, [0.0; 3], 33.0)
        .unwrap();
    sim.reset_ball([0.0, 0.0, 93.0], [0.0; 3]);
    let start = [-2048.0f32, -2560.0];
    let dodge_step = p.hold + p.rel;
    let mut dodge_tick = None;
    let mut t1500 = f32::NAN;
    for t in 0..400u32 {
        let step = t / p.q;
        let mut c = Controls {
            throttle: 1.0,
            boost: true,
            ..Default::default()
        };
        if step < p.hold {
            c.jump = true;
        } else if step == dodge_step {
            c.jump = true;
            c.pitch = -1.0;
            c.roll = p.roll;
        } else if step > dodge_step {
            let d = step - dodge_step;
            if d > p.cancel_after && d <= p.cancel_after + p.cancel_len {
                c.pitch = 1.0;
            }
        }
        sim.set_controls(car, c).unwrap();
        sim.step(1);
        let ct = sim.car_tick(car);
        if dodge_tick.is_none() && ct.has_double_jumped | ct.is_flipping {
            dodge_tick = Some(t);
        }
        let d = ((ct.pos[0] - start[0]).powi(2) + (ct.pos[1] - start[1]).powi(2)).sqrt();
        if t1500.is_nan() && d >= 1500.0 {
            t1500 = (t + 1) as f32 / 120.0;
        }
        if let Some(dt) = dodge_tick {
            if ct.on_ground && t > dt + 1 {
                let hs = ct.vel[0].hypot(ct.vel[1]);
                return Some(R {
                    land_dt: t - dt,
                    hspeed: hs,
                    t1500,
                });
            }
        }
    }
    None
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(20000);
    let qs: Vec<u32> = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "4,8".into())
        .split(',')
        .filter_map(|s| s.parse().ok())
        .collect();
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("meshes");
    let mut sim = Sim::new_soccar().unwrap();
    let car = sim.add_car(true);
    let mut rng = Rng(7);
    // Référence : pas de saut, boost seul.
    let base = {
        let yaw = std::f32::consts::FRAC_PI_4;
        sim.reset_car(car, [-2048.0, -2560.0, 17.0], yaw, [0.0; 3], 33.0)
            .unwrap();
        let mut t1500 = f32::NAN;
        for t in 0..400u32 {
            sim.set_controls(
                car,
                Controls {
                    throttle: 1.0,
                    boost: true,
                    ..Default::default()
                },
            )
            .unwrap();
            sim.step(1);
            let ct = sim.car_tick(car);
            let d = (ct.pos[0] + 2048.0).hypot(ct.pos[1] + 2560.0);
            if t1500.is_nan() && d >= 1500.0 {
                t1500 = (t + 1) as f32 / 120.0;
            }
        }
        t1500
    };
    println!("sans saut (boost seul) : 1 500 uu en {base:.3} s");
    for q in qs {
        let (mut best_dt, mut best_t, mut best_v): (Best, Best, Best) = (None, None, None);
        let mut ok = 0;
        let (mut max_v, mut best_150) = (0.0f32, 0.0f32);
        let mut dts: Vec<u32> = vec![];
        for _ in 0..n {
            let m = 120 / q.max(1) / 4; // jusqu'à 0,25 s
            let p = P {
                q,
                hold: rng.range(1, m.max(1)),
                rel: rng.range(1, m.max(1)),
                cancel_after: rng.range(0, m.max(1)),
                cancel_len: rng.range(1, 3 * m.max(1)),
                roll: if rng.next() & 1 == 0 { 1.0 } else { -1.0 },
            };
            let Some(r) = run(&mut sim, car, p) else {
                continue;
            };
            ok += 1;
            dts.push(r.land_dt);
            max_v = max_v.max(r.hspeed);
            if r.land_dt <= 150 {
                best_150 = best_150.max(r.hspeed);
            }
            if r.hspeed > 1900.0 {
                if best_dt.is_none_or(|(b, _)| r.land_dt < b.land_dt) {
                    best_dt = Some((r, p));
                }
                if best_v.is_none_or(|(b, _)| r.hspeed > b.hspeed) {
                    best_v = Some((r, p));
                }
            }
            if best_t.is_none_or(|(b, _)| r.t1500 < b.t1500) {
                best_t = Some((r, p));
            }
        }
        println!("  vitesse max à l'atterrissage {max_v:.0} (dt ≤ 150 : {best_150:.0})");
        dts.sort_unstable();
        println!(
            "Q={q} : {ok} atterrissages, dt min {:?}, p10 {:?}, médiane {:?}",
            dts.first(),
            dts.get(dts.len() / 10),
            dts.get(dts.len() / 2)
        );
        println!("  plus court (hspeed>1900) : {best_dt:?}");
        println!("  plus rapide à 1 500 uu : {best_t:?}");
        println!("  plus rapide à l'atterrissage : {best_v:?}");
    }
}
