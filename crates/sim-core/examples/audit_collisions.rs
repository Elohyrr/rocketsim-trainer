//! Audit des collisions : rejoue chaque clip JSON d'un dossier dans RocketSim avec deux détecteurs sur la voiture
//! de l'agent — « avant » (sans voir l'autre voiture : ni contact, ni toucher simultané) et « après » (version
//! avec collisions) — et dit, mécanique par mécanique, combien de clips sont conservés, rejetés ou réétiquetés.
//!
//! `ROCKETSIM_MESHES_DIR=… cargo run --release -p sim-core --example audit_collisions -- <dossier de clips> [--verbose]`
//!
//! Le détecteur démarre au début du clip (état initial approximatif). Un clip est « conservé » si la mécanique
//! d'origine est encore reconnue ; « rejeté » sinon ; « réétiqueté » s'il est rejeté mais qu'une autre
//! mécanique (hors toucher) est reconnue à la place (ex. pinch car, bump).

use mechanics::{Counts, Detector, Mechanic, Mode};
use sim_core::clip::{Clip, Replayer};
use sim_core::{Sim, init_meshes};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default)]
struct Row {
    clips: u32,
    before: u32,
    after: u32,
    rejected: u32,
    relabeled: u32,
    with_collision: u32,
    with_shared: u32,
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
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut gained: BTreeMap<&'static str, u32> = BTreeMap::new();
    for f in files {
        let Ok(clip) = Clip::from_json(&std::fs::read(&f).unwrap()) else {
            eprintln!("illisible : {}", f.display());
            continue;
        };
        let label = clip.mechanic.clone();
        let id = clip.id.clone();
        let blue0 = clip.blue[0];
        let mut sim = Sim::new_soccar().unwrap();
        let mut rep = Replayer::start(&mut sim, clip).unwrap();
        let car_id = rep.car_ids()[0];
        let (mut d0, mut d1) = (Detector::with_mode(Mode::V2), Detector::with_mode(Mode::V2));
        d0.set_blue(blue0);
        d1.set_blue(blue0);
        let (mut c0, mut c1) = (Counts::default(), Counts::default());
        let (mut coll, mut shared) = (false, false);
        let mut prev_cd = 0.0f32;
        let mut ticks = 0u32;
        while !rep.tick(&mut sim) {
            ticks += 1;
            let (c, b) = (sim.car_tick(car_id), sim.ball_tick());
            let mut blind = c;
            blind.other = None;
            blind.contact_other = false;
            blind.demoed = false;
            d0.tick(&blind, &b, &mut c0);
            d1.tick(&c, &b, &mut c1);
            if c.contact_other && c.contact_cooldown > prev_cd {
                coll = true;
            }
            prev_cd = c.contact_cooldown;
            shared |= d1.tainted() && c.other.is_some_and(|o| o.ball_hit_tick.is_some());
        }
        let m = Mechanic::ALL
            .into_iter()
            .find(|m| m.label() == label)
            .unwrap_or(Mechanic::Touch);
        let r = rows.entry(label.clone()).or_default();
        r.clips += 1;
        let (b, a) = (c0.get(m) > 0, c1.get(m) > 0);
        r.before += b as u32;
        r.after += a as u32;
        r.with_collision += coll as u32;
        r.with_shared += shared as u32;
        let mut others = vec![];
        for x in Mechanic::ALL {
            if x != Mechanic::Touch && x != m && c1.get(x) > c0.get(x) {
                others.push(x.label());
                *gained.entry(x.label()).or_default() += 1;
            }
        }
        if b && !a {
            r.rejected += 1;
            if !others.is_empty() {
                r.relabeled += 1;
            }
        }
        if verbose {
            println!(
                "{id:<34} {label:<14} avant {} après {} choc {} touchers-simultanés {} +{:?} ({ticks} ticks)",
                b as u8, a as u8, coll as u8, shared as u8, others
            );
        }
    }
    println!(
        "mécanique        clips  rejoué-avant  conservé-après  rejeté  réétiqueté  avec-choc  tainted+autre-touche"
    );
    let mut t = Row::default();
    for (k, r) in &rows {
        println!(
            "{k:<16} {:>5} {:>12} {:>15} {:>7} {:>11} {:>10} {:>10}",
            r.clips, r.before, r.after, r.rejected, r.relabeled, r.with_collision, r.with_shared
        );
        t.clips += r.clips;
        t.before += r.before;
        t.after += r.after;
        t.rejected += r.rejected;
        t.relabeled += r.relabeled;
        t.with_collision += r.with_collision;
        t.with_shared += r.with_shared;
    }
    println!(
        "TOTAL            {:>5} {:>12} {:>15} {:>7} {:>11} {:>10} {:>10}",
        t.clips, t.before, t.after, t.rejected, t.relabeled, t.with_collision, t.with_shared
    );
    println!("mécaniques apparues (clips) : {gained:?}");
}
