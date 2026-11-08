//! Benchmark de débit de l'env headless.
//!
//! taskset -c 0,1 cargo run --release -p rl-env --example bench -- <threads> <n_envs> <secondes>
//! (politique aléatoire uniforme sur la table d'actions ; steps = décisions, 8 ticks chacune)

use std::time::{Duration, Instant};

use rl_env::{EnvConfig, Rng, VecEnv};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let threads: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let n_envs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut v = VecEnv::new(n_envs, threads, EnvConfig::default(), 42).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    v.reset();
    let mut rng = Rng::new(1);
    let n_act = v.action_count();
    let t = Instant::now();
    let mut steps = 0u64;
    while t.elapsed() < Duration::from_secs(secs) {
        let acts: Vec<usize> = (0..n_envs)
            .map(|_| (rng.next_u64() % n_act as u64) as usize)
            .collect();
        v.step(&acts);
        steps += n_envs as u64;
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "threads={threads} envs={n_envs} steps={steps} en {dt:.1}s -> {:.0} steps/s ({:.0} ticks/s)",
        steps as f64 / dt,
        steps as f64 * 8.0 / dt
    );
}
