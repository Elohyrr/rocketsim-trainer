//! Entraînement PPO « toucher la balle ».
//!
//! learner [--minutes M] [--envs N] [--threads T] [--out DIR] [--metrics ADDR] [--resume]
//! Le dossier `--out` reçoit `checkpoint.mpk`, `policy.safetensors` et `policy.json`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use learner::metrics::{Metrics, serve};
use learner::ppo::{PpoConfig, Trainer};
use rl_env::EnvConfig;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
}

fn main() {
    let minutes: f64 = arg("--minutes").and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let mut cfg = PpoConfig::default();
    if let Some(n) = arg("--envs").and_then(|s| s.parse().ok()) {
        cfg.n_envs = n;
    }
    if let Some(n) = arg("--threads").and_then(|s| s.parse().ok()) {
        cfg.threads = n;
    }
    let out = PathBuf::from(arg("--out").unwrap_or_else(|| "runs/latest".into()));
    std::fs::create_dir_all(&out).expect("dossier de sortie");
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    if let Some(addr) = arg("--metrics") {
        match serve(&addr, metrics.clone()) {
            Ok(a) => eprintln!("metrics sur http://{a}/metrics"),
            Err(e) => eprintln!("metrics indisponibles : {e}"),
        }
    }
    let mut tr = Trainer::new(cfg, EnvConfig::default()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let ckpt = out.join("checkpoint");
    if std::env::args().any(|a| a == "--resume") {
        match tr.load_checkpoint(&ckpt) {
            Ok(()) => eprintln!("reprise depuis {}", ckpt.display()),
            Err(e) => eprintln!("pas de reprise ({e}) : départ à neuf"),
        }
    }
    let t0 = Instant::now();
    let mut last_ckpt = Instant::now();
    let (mut steps, mut eps, mut touches) = (0u64, 0u64, 0u64);
    let mut it = 0;
    println!("iter steps_total steps/s ep_reward ep_len touch_rate entropy kl collect_s train_s");
    while t0.elapsed() < Duration::from_secs_f64(minutes * 60.0) {
        let ti = Instant::now();
        let s = tr.iteration();
        it += 1;
        steps += s.steps;
        eps += s.episodes;
        touches += s.touches;
        let dt = ti.elapsed().as_secs_f64();
        println!(
            "{it} {steps} {:.0} {:.3} {:.1} {:.2} {:.3} {:.4} {:.2} {:.2}",
            s.steps as f64 / dt,
            s.reward_mean,
            s.length_mean,
            s.touch_rate,
            s.entropy,
            s.kl,
            s.collect_seconds,
            s.train_seconds
        );
        if let Ok(mut m) = metrics.lock() {
            m.steps_total = steps;
            m.steps_per_second = s.steps as f64 / dt;
            m.episodes_total = eps;
            m.reward_mean = s.reward_mean;
            m.episode_length_mean = s.length_mean;
            m.policy_loss = s.policy_loss;
            m.value_loss = s.value_loss;
            m.entropy = s.entropy;
            m.kl = s.kl;
            m.touch_total = touches;
            m.observe_iteration(dt);
            m.checkpoint_age_seconds = last_ckpt.elapsed().as_secs_f64();
        }
        if last_ckpt.elapsed() > Duration::from_secs(60) {
            let ok = tr.save_checkpoint(&ckpt).is_ok();
            if let Ok(mut m) = metrics.lock() {
                if ok {
                    m.checkpoints_ok += 1;
                } else {
                    m.checkpoints_error += 1;
                }
            }
            last_ckpt = Instant::now();
        }
    }
    tr.save_checkpoint(&ckpt).expect("checkpoint final");
    tr.export(&out.join("policy.safetensors")).expect("export");
    eprintln!("terminé : {steps} pas, export dans {}", out.display());
}
