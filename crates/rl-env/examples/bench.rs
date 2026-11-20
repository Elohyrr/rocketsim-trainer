//! Benchmark de débit de l'env headless.
//!
//! taskset -c 0,1 cargo run --release -p rl-env --example bench -- <threads> <n_envs> <secondes>
//! NO_DETECT=1 coupe le détecteur de mécaniques (mesure de son coût) ; CLIPS=1 enregistre des clips (coût de
//! l'enregistreur) ; STAGE=5 prend le palier 5 (adversaire, coups d'envoi : le profil de la prod).
//! (politique aléatoire uniforme sur la table d'actions ; steps = décisions de `tick_skip` ticks : 8 par défaut,
//! TICK_SKIP=4 pour 30 Hz)

use std::time::{Duration, Instant};

use rl_env::{EnvConfig, Rng, VecEnv};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let threads: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let n_envs: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
    let stage = std::env::var("STAGE")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or_else(EnvConfig::default, EnvConfig::for_stage);
    // TICK_SKIP=4 : 30 Hz (2 fois plus de décisions par seconde de jeu).
    let tick_skip: u32 = std::env::var("TICK_SKIP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(stage.tick_skip);
    let stage = stage.with_tick_skip(tick_skip);
    let opp = stage.opponent != rl_env::Opponent::None;
    let mut v = VecEnv::new(
        n_envs,
        threads,
        EnvConfig {
            detect_mechanics: std::env::var_os("NO_DETECT").is_none(),
            record_clips: std::env::var_os("CLIPS").is_some(),
            ..stage
        },
        42,
    )
    .unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    v.reset();
    let mut rng = Rng::new(1);
    let n_act = v.action_count();
    let t = Instant::now();
    let mut steps = 0u64;
    let mut clips = 0u64;
    while t.elapsed() < Duration::from_secs(secs) {
        let acts: Vec<usize> = (0..n_envs)
            .map(|_| (rng.next_u64() % n_act as u64) as usize)
            .collect();
        if opp {
            let o: Vec<usize> = (0..n_envs)
                .map(|_| (rng.next_u64() % n_act as u64) as usize)
                .collect();
            clips += v.step_vs(&acts, Some(&o)).clips.len() as u64;
        } else {
            clips += v.step(&acts).clips.len() as u64;
        }
        steps += n_envs as u64;
    }
    let dt = t.elapsed().as_secs_f64();
    println!(
        "threads={threads} envs={n_envs} tick_skip={tick_skip} steps={steps} en {dt:.1}s -> {:.0} steps/s ({:.0} ticks/s, {:.1} s de jeu/s), {clips} clips",
        steps as f64 / dt,
        steps as f64 * f64::from(tick_skip) / dt,
        steps as f64 * f64::from(tick_skip) / 120.0 / dt
    );
}
