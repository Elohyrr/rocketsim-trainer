//! Spike M0 : simule 1800 ticks (15 s à 120 Hz) avec une voiture qui accélère.
//!
//! ROCKETSIM_MESHES_DIR=/chemin/vers/collision_meshes cargo run --release -p sim-core --example m0-spike

use std::time::Instant;

use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

fn main() {
    let dir = match meshes_dir_from_env() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "m0-spike : {e}\nDéfinir {} (dossier contenant soccar/*.cco).",
                sim_core::MESHES_ENV
            );
            std::process::exit(2);
        }
    };
    init_meshes(&dir).expect("init meshes");
    let mut sim = Sim::new_soccar().expect("arène");
    let car = sim.add_car(true);
    sim.set_controls(
        car,
        Controls {
            throttle: 1.0,
            boost: true,
            ..Default::default()
        },
    )
    .unwrap();
    let t = Instant::now();
    sim.step(1800);
    let dt = t.elapsed();
    println!("{}", serde_json::to_string_pretty(&sim.snapshot()).unwrap());
    println!("1800 ticks en {dt:?}");
}
