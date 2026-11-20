//! Taux de séquences vérifiées par le détecteur, par mécanique et par jeu de paramètres.
//! Usage : `demo_bench [N=200] [kind]` (meshes via ROCKETSIM_MESHES_DIR).
use rl_env::demo::{DemoGen, DemoKind};
use sim_core::{init_meshes, meshes_dir_from_env};
use std::collections::BTreeMap;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200);
    let only = std::env::args().nth(2);
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("meshes");
    let mut g = DemoGen::new(1).expect("env");
    g.diag = std::env::var("DIAG").is_ok();
    g.trace = std::env::var("TRACE").is_ok();
    for kind in DemoKind::ALL {
        if only.as_deref().is_some_and(|o| o != kind.label()) {
            continue;
        }
        let mut by: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let mut len = 0usize;
        for _ in 0..n {
            let d = g.sample(kind);
            let e = by.entry(g.last.clone()).or_default();
            e.0 += 1;
            if let Some(d) = d {
                e.1 += 1;
                len += d.len();
            }
        }
        let (t, k) = (g.tried[kind.index()], g.kept[kind.index()]);
        println!(
            "{} : {k}/{t} ({:.0} %), {:.1} décisions en moyenne",
            kind.label(),
            100.0 * k as f64 / t as f64,
            len as f64 / k.max(1) as f64
        );
        if std::env::var("VERBOSE").is_ok() {
            for (p, (t, k)) in by {
                if k > 0 || std::env::var("DIAG").is_ok() {
                    println!("   {k}/{t}  {p}");
                }
            }
        }
    }
}
