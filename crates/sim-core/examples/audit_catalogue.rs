//! Audit du catalogue complet : rejoue chaque clip JSON d'un dossier dans RocketSim avec le détecteur v2 (toutes
//! les mécaniques) et résume ce qui y est reconnu.
//!
//! `ROCKETSIM_MESHES_DIR=… cargo run --release -p sim-core --example audit_catalogue -- <dossier de clips> [--verbose]`
//!
//! Le détecteur démarre au début du clip (état initial approximatif : seul le contenu du clip est jugé).
//! Sortie : (verbose) une ligne par clip avec les mécaniques vues ; puis par mécanique le nombre de clips où elle
//! apparaît, son total et son record (vitesse de balle, uu/s).

use mechanics::{Counts, Detector, Mechanic, Mode};
use sim_core::clip::{Clip, Replayer};
use sim_core::{Sim, init_meshes};
use std::path::Path;

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
    let mut clips_with = [0u32; mechanics::MECH_COUNT];
    let mut total = Counts::default();
    let mut n = 0;
    for f in files {
        let Ok(clip) = Clip::from_json(&std::fs::read(&f).unwrap()) else {
            eprintln!("illisible : {}", f.display());
            continue;
        };
        n += 1;
        let label = clip.mechanic.clone();
        let blue0 = clip.blue[0];
        let mut sim = Sim::new_soccar().unwrap();
        let mut rep = Replayer::start(&mut sim, clip).unwrap();
        let id = rep.car_ids()[0];
        let mut det = Detector::with_mode(Mode::V2);
        det.set_blue(blue0);
        let mut counts = Counts::default();
        while !rep.tick(&mut sim) {
            det.tick(&sim.car_tick(id), &sim.ball_tick(), &mut counts);
        }
        for m in Mechanic::ALL {
            if counts.get(m) > 0 {
                clips_with[m.index()] += 1;
            }
        }
        if verbose {
            let seen: Vec<String> = counts
                .nonzero()
                .into_iter()
                .filter(|(m, _)| *m != Mechanic::Touch)
                .map(|(m, k)| {
                    let d = det.detail(m);
                    if d.value > 0.0 || d.support != mechanics::Support::None {
                        format!(
                            "{}×{k} ({:.0} uu/s {})",
                            m.label(),
                            d.value,
                            d.support.label()
                        )
                    } else {
                        format!("{}×{k}", m.label())
                    }
                })
                .collect();
            println!("{label:<14} {}", seen.join(", "));
        }
        total.add(&counts);
    }
    println!("{n} clips rejoués\nmécanique            clips  total  record");
    for m in Mechanic::ALL {
        if total.get(m) > 0 {
            println!(
                "{:<20} {:>5} {:>6}  {:.0}",
                m.label(),
                clips_with[m.index()],
                total.get(m),
                total.records().get(m)
            );
        }
    }
}
