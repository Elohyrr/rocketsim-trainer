//! Audit des clips : rejoue chaque clip JSON d'un dossier dans RocketSim et compare le détecteur v1 et v2.
//!
//! `ROCKETSIM_MESHES_DIR=… cargo run --release -p sim-core --example audit_clips -- <dossier de clips> [--verbose]`
//!
//! Le détecteur démarre au début du clip (état initial approximatif : seul le contenu du clip est jugé).
//! Sortie : une ligne par clip (mécanique d'origine, comptes v1 / v2 des 4 mécaniques durcies, plus longue
//! séquence de dribble avec nombre de touchers) puis un bilan : combien de clips v1 sont validés en v2.

use mechanics::{Counts, Detector, Mechanic, Mode};
use sim_core::clip::{Clip, Replayer};
use sim_core::{Sim, init_meshes};
use std::path::Path;

#[derive(Default)]
struct Run {
    len: u32,
    touches: u32,
    dist: f32,
    last_touch: Option<u64>,
    raw_changes: u32,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args.first().expect("dossier de clips");
    let verbose = args.iter().any(|a| a == "--verbose");
    init_meshes(Path::new(
        &std::env::var("ROCKETSIM_MESHES_DIR").expect("ROCKETSIM_MESHES_DIR"),
    ))
    .expect("meshes");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && !p.ends_with("index.json"))
        .collect();
    files.sort();
    let targets = [
        Mechanic::AerialTouch,
        Mechanic::AirDribble,
        Mechanic::WallShot,
        Mechanic::Dribble,
    ];
    let mut per_mech: std::collections::BTreeMap<String, (u32, u32, u32)> = Default::default();
    for f in files {
        let Ok(clip) = Clip::from_json(&std::fs::read(&f).unwrap()) else {
            eprintln!("illisible : {}", f.display());
            continue;
        };
        let label = clip.mechanic.clone();
        let blue0 = clip.blue[0];
        let mut sim = Sim::new_soccar().unwrap();
        let mut rep = Replayer::start(&mut sim, clip).unwrap();
        let id = rep.car_ids()[0];
        let (mut d1, mut d2) = (Detector::with_mode(Mode::V1), Detector::with_mode(Mode::V2));
        d1.set_blue(blue0);
        d2.set_blue(blue0);
        let (mut c1, mut c2) = (Counts::default(), Counts::default());
        let mut run = Run::default();
        let mut best = Run::default();
        let mut prev_hit = None;
        loop {
            let done = rep.tick(&mut sim);
            if done {
                break;
            }
            let (c, b) = (sim.car_tick(id), sim.ball_tick());
            d1.tick(&c, &b, &mut c1);
            d2.tick(&c, &b, &mut c2);
            // Géométrie du dribble v1, pour mesurer les touchers dans un contrôle continu.
            let rel = [
                b.pos[0] - c.pos[0],
                b.pos[1] - c.pos[1],
                b.pos[2] - c.pos[2],
            ];
            let dot = |a: [f32; 3], u: [f32; 3]| a[0] * u[0] + a[1] * u[1] + a[2] * u[2];
            let rv = [
                b.vel[0] - c.vel[0],
                b.vel[1] - c.vel[1],
                b.vel[2] - c.vel[2],
            ];
            let ok = c.on_ground
                && c.up[2] > 0.7
                && (110.0..=190.0).contains(&dot(rel, c.up))
                && dot(rel, c.forward).abs() < 110.0
                && dot(rel, c.right).abs() < 80.0
                && dot(rv, rv) < 400.0 * 400.0;
            let changed = c.ball_hit_tick.is_some() && c.ball_hit_tick != prev_hit;
            prev_hit = c.ball_hit_tick;
            if ok {
                run.len += 1;
                run.dist += (c.vel[0].powi(2) + c.vel[1].powi(2)).sqrt() / 120.0;
                if changed {
                    run.raw_changes += 1;
                    if run.last_touch.is_none_or(|t| c.tick - t >= 10) {
                        run.touches += 1;
                        run.last_touch = Some(c.tick);
                    }
                }
                if run.len > best.len {
                    best = Run { ..run_copy(&run) };
                }
            } else if run.len > 0 {
                run = Run::default();
            }
        }
        let g = |c: &Counts| targets.map(|m| c.get(m));
        if verbose {
            println!(
                "{:<14} v1 aer/ad/wall/dr {:?}  v2 {:?}  dribble-run {} ticks, {} touchers distincts ({} changements bruts), {:.0} uu",
                label,
                g(&c1),
                g(&c2),
                best.len,
                best.touches,
                best.raw_changes,
                best.dist
            );
        }
        let m: Mechanic = Mechanic::ALL
            .into_iter()
            .find(|m| m.label() == label)
            .unwrap_or(Mechanic::Touch);
        let e = per_mech.entry(label).or_default();
        e.0 += 1;
        e.1 += (c1.get(m) > 0) as u32;
        e.2 += (c2.get(m) > 0) as u32;
    }
    println!("mécanique  clips  v1(rejoué)  v2");
    let (mut n, mut a, mut b) = (0, 0, 0);
    for (k, (t, v1, v2)) in &per_mech {
        println!("{k:<14} {t:>4} {v1:>4} {v2:>4}");
        n += t;
        a += v1;
        b += v2;
    }
    println!("TOTAL          {n:>4} {a:>4} {b:>4}");
}

fn run_copy(r: &Run) -> Run {
    Run {
        len: r.len,
        touches: r.touches,
        dist: r.dist,
        last_touch: r.last_touch,
        raw_changes: r.raw_changes,
    }
}
