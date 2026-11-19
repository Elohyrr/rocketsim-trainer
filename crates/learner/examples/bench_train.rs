//! Banc de débit du learner : itérations PPO complètes, temps collecte / apprentissage et CPU consommé.
//!
//! taskset -c 0-3 cargo run --release -p learner --example bench_train -- [iters] [threads] [envs] [hidden] [stage]
//! (DRILL_PROB=0.3 pour le profil de la prod ; le CPU est lu dans /proc/self/stat : utime + stime du processus)

use std::time::Instant;

use learner::ppo::{PpoConfig, Trainer};
use rl_env::EnvConfig;

fn cpu_seconds() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let f: Vec<&str> = s
        .rsplit(')')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect();
    // après le nom : état = f[0] ; utime = champ 14 → f[11], stime → f[12] (en ticks de 10 ms).
    let t = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (t(11) + t(12)) / 100.0
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize, d: usize| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (iters, threads, envs, hidden, stage) =
        (num(1, 30), num(2, 4), num(3, 16), num(4, 128), num(5, 5));
    let drill: f32 = std::env::var("DRILL_PROB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let c = EnvConfig::for_stage(stage as u32);
    let env_cfg = EnvConfig {
        drill_prob: if c.end_on_touch { 0.0 } else { drill },
        ..c
    };
    let cfg = PpoConfig {
        n_envs: envs,
        threads,
        hidden,
        ..PpoConfig::default()
    };
    let mut tr = Trainer::new(cfg, env_cfg).expect("trainer");
    for _ in 0..3 {
        tr.iteration(); // échauffement
    }
    let (c0, t0) = (cpu_seconds(), Instant::now());
    let mut inf = 0.0;
    let (mut col, mut tra, mut steps, mut ent, mut pl) = (0.0, 0.0, 0u64, 0.0, 0.0);
    for _ in 0..iters {
        let s = tr.iteration();
        col += s.collect_seconds;
        inf += s.infer_seconds;
        tra += s.train_seconds;
        steps += s.steps;
        ent = s.entropy;
        pl += s.policy_loss;
    }
    let (wall, cpu) = (t0.elapsed().as_secs_f64(), cpu_seconds() - c0);
    println!(
        "pas/s {:.0} | cpu {:.2} | collect {:.3}s (inférence {:.3}s) train {:.3}s /iter | entropie {:.4} pol_loss_moy {:.5}",
        steps as f64 / wall,
        cpu / wall,
        col / iters as f64,
        inf / iters as f64,
        tra / iters as f64,
        ent,
        pl / iters as f64
    );
}
