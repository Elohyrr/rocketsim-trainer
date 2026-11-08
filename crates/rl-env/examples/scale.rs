//! Diagnostic de passage à l'échelle : N threads qui ne font que `Sim::step` (sans snapshot ni reset).
use std::time::Instant;

use sim_core::{Sim, init_meshes, meshes_dir_from_env};

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let snap = std::env::args().nth(2).as_deref() == Some("snap");
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("init");
    let t = Instant::now();
    let hs: Vec<_> = (0..n)
        .map(|_| {
            std::thread::spawn(move || {
                let mut s = Sim::new_soccar().unwrap();
                let c = s.add_car(true);
                for _ in 0..40_000 {
                    s.step(8);
                    if snap {
                        std::hint::black_box(s.snapshot());
                    }
                }
                let _ = c;
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "threads={n} snap={snap} {:.0} steps/s total",
        40_000.0 * n as f64 / dt
    );
}
