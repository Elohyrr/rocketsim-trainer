//! Rejeu des clips avec le détecteur courant (detector 6) : pour chaque clip, la mécanique d'origine est-elle encore
//! reconnue ? Sert au tableau avant/après de `docs/audit-clips.md`.
//!
//! `ROCKETSIM_MESHES_DIR=… cargo run --release -p sim-core --example audit_detector -- <dossier de clips>`
//!
//! Sortie TSV : `id  mécanique  reconnue(0/1)  autres mécaniques reconnues (hors touch)`. Le détecteur démarre au
//! début du clip (état initial approximatif : une mécanique qui exige un historique long peut manquer).

use mechanics::{Counts, Detector, Mechanic};
use sim_core::clip::{Clip, Replayer};
use sim_core::{Sim, init_meshes};
use std::path::Path;

fn main() {
    let dir = std::env::args().nth(1).expect("dossier de clips");
    init_meshes(Path::new(
        &std::env::var("ROCKETSIM_MESHES_DIR").expect("ROCKETSIM_MESHES_DIR"),
    ))
    .expect("meshes");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json") && !p.ends_with("index.json"))
        .collect();
    files.sort();
    for f in files {
        let Ok(clip) = Clip::from_json(&std::fs::read(&f).unwrap()) else {
            eprintln!("illisible : {}", f.display());
            continue;
        };
        let (id, label, blue0) = (clip.id.clone(), clip.mechanic.clone(), clip.blue[0]);
        let mut sim = Sim::new_soccar().unwrap();
        let mut rep = Replayer::start(&mut sim, clip).unwrap();
        let car = rep.car_ids()[0];
        let mut det = Detector::new();
        det.set_blue(blue0);
        let mut counts = Counts::default();
        while !rep.tick(&mut sim) {
            det.tick(&sim.car_tick(car), &sim.ball_tick(), &mut counts);
        }
        let seen = Mechanic::ALL
            .into_iter()
            .filter(|m| !matches!(m, Mechanic::Touch) && counts.get(*m) > 0)
            .map(|m| m.label())
            .collect::<Vec<_>>();
        println!(
            "{id}\t{label}\t{}\t{}",
            seen.contains(&label.as_str()) as u8,
            seen.join(",")
        );
    }
}
