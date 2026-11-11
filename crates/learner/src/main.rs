//! Entraînement PPO H24 : curriculum, checkpoints périodiques, reprise, `/metrics`, pause Jellyfin.
//!
//! learner [--minutes M] [--envs N] [--threads T] [--out DIR] [--metrics ADDR] [--resume]
//!         [--checkpoint-minutes N] [--stage S]
//! `--minutes 0` = sans limite (mode cluster). `--stage S` fige le palier (pas de promotion automatique).
//! Variables : `JELLYFIN_URL`, `JELLYFIN_API_KEY` (pause D6, désactivée sans clé), `JELLYFIN_RESUME_MINUTES`
//! (défaut 10), `JELLYFIN_POLL_SECONDS` (défaut 30). SIGTERM/SIGINT : checkpoint final puis sortie.
//! Le dossier `--out` reçoit `checkpoint.mpk`, `policy.safetensors`, `policy.json` et `stage`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use learner::curriculum::{Curriculum, IterResult, Move};
use learner::jellyfin::Jellyfin;
use learner::metrics::{Metrics, serve};
use learner::pause::PauseGate;
use learner::ppo::{PpoConfig, Trainer};
use rl_env::EnvConfig;

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|x| x == name)
        .and_then(|i| a.get(i + 1).cloned())
}

fn env_num(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Surveille Jellyfin dans un thread ; `None` si la pause est désactivée (pas de clé).
fn spawn_watcher() -> Option<Arc<AtomicBool>> {
    let jf = match Jellyfin::from_env()? {
        Ok(j) => j,
        Err(e) => {
            eprintln!("pause Jellyfin désactivée : {e}");
            return None;
        }
    };
    let playing = Arc::new(AtomicBool::new(false));
    let flag = playing.clone();
    let every = Duration::from_secs(env_num("JELLYFIN_POLL_SECONDS", 30).max(5));
    std::thread::Builder::new()
        .name("jellyfin".into())
        .spawn(move || {
            let mut last_err = String::new();
            loop {
                match jf.video_playing() {
                    Ok(p) => {
                        flag.store(p, Ordering::Relaxed);
                        last_err.clear();
                    }
                    Err(e) => {
                        // Erreur = pas de lecture connue ; on ne journalise que les changements.
                        flag.store(false, Ordering::Relaxed);
                        if e != last_err {
                            eprintln!("jellyfin injoignable : {e}");
                            last_err = e;
                        }
                    }
                }
                std::thread::sleep(every);
            }
        })
        .ok()?;
    eprintln!(
        "pause Jellyfin active (sondage toutes les {}s)",
        every.as_secs()
    );
    Some(playing)
}

struct Saver<'a> {
    tr: &'a Trainer,
    out: &'a Path,
    curriculum: &'a Mutex<Curriculum>,
    metrics: &'a Mutex<Metrics>,
    last: Instant,
}

impl Saver<'_> {
    fn save(&mut self) {
        let ok = self
            .tr
            .save_checkpoint(&self.out.join("checkpoint"))
            .is_ok()
            && self.tr.export(&self.out.join("policy.safetensors")).is_ok()
            && self
                .curriculum
                .lock()
                .is_ok_and(|c| c.save(self.out).is_ok());
        if let Ok(mut m) = self.metrics.lock() {
            if ok {
                m.checkpoints_ok += 1;
            } else {
                m.checkpoints_error += 1;
            }
        }
        if ok {
            self.last = Instant::now();
        } else {
            eprintln!("checkpoint en échec");
        }
    }
}

fn main() {
    let minutes: f64 = arg("--minutes").and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let ckpt_every = Duration::from_secs_f64(
        arg("--checkpoint-minutes")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0f64)
            * 60.0,
    );
    let mut cfg = PpoConfig::default();
    if let Some(n) = arg("--envs").and_then(|s| s.parse().ok()) {
        cfg.n_envs = n;
    }
    if let Some(n) = arg("--threads").and_then(|s| s.parse().ok()) {
        cfg.threads = n;
    }
    let out = PathBuf::from(arg("--out").unwrap_or_else(|| "runs/latest".into()));
    std::fs::create_dir_all(&out).expect("dossier de sortie");
    let resume = std::env::args().any(|a| a == "--resume");
    let pinned: Option<u32> = arg("--stage").and_then(|s| s.parse().ok());
    let curriculum = Mutex::new(match pinned {
        Some(s) => Curriculum::new(s),
        None if resume => Curriculum::load(&out),
        None => Curriculum::new(0),
    });
    let stage = curriculum.lock().map(|c| c.stage()).unwrap_or(0);

    let stop = Arc::new(AtomicBool::new(false));
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(sig, stop.clone()).expect("signal");
    }
    let metrics = Arc::new(Mutex::new(Metrics {
        curriculum_stage: stage,
        ..Metrics::default()
    }));
    if let Some(addr) = arg("--metrics") {
        match serve(&addr, metrics.clone()) {
            Ok(a) => eprintln!("metrics sur http://{a}/metrics"),
            Err(e) => eprintln!("metrics indisponibles : {e}"),
        }
    }
    let watcher = spawn_watcher();
    let mut gate = PauseGate::new(Duration::from_secs(
        env_num("JELLYFIN_RESUME_MINUTES", 10) * 60,
    ));

    let mut tr = Trainer::new(cfg, EnvConfig::for_stage(stage)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    if resume {
        let ckpt = out.join("checkpoint");
        match tr.load_checkpoint(&ckpt) {
            Ok(()) => eprintln!("reprise depuis {} (palier {stage})", ckpt.display()),
            Err(e) => eprintln!("pas de reprise ({e}) : départ à neuf"),
        }
    }
    eprintln!("palier de curriculum : {stage}");

    let t0 = Instant::now();
    let limit = (minutes > 0.0).then(|| Duration::from_secs_f64(minutes * 60.0));
    let (mut steps, mut eps, mut touches) = (0u64, 0u64, 0u64);
    let (mut goals, mut own_goals) = (0u64, 0u64);
    let mut it = 0;
    let mut saver_last = Instant::now();
    println!(
        "iter stage steps_total steps/s ep_reward ep_len touch_rate goal_rate goals own_goals entropy kl collect_s train_s"
    );
    while !stop.load(Ordering::Relaxed) && limit.is_none_or(|l| t0.elapsed() < l) {
        // D6 : lecture vidéo en cours → checkpoint puis pause (CPU ≈ 0) jusqu'à N min après la fin.
        if let Some(playing) = &watcher {
            let was_paused = gate.paused();
            if gate.update(playing.load(Ordering::Relaxed), Instant::now()) {
                if !was_paused {
                    eprintln!("lecture vidéo détectée : checkpoint puis pause");
                    let mut s = Saver {
                        tr: &tr,
                        out: &out,
                        curriculum: &curriculum,
                        metrics: &metrics,
                        last: saver_last,
                    };
                    s.save();
                    saver_last = s.last;
                }
                if let Ok(mut m) = metrics.lock() {
                    m.paused = true;
                    m.pause_jellyfin = true;
                    m.checkpoint_age_seconds = saver_last.elapsed().as_secs_f64();
                }
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            if was_paused {
                eprintln!("plus de lecture depuis un moment : reprise");
                if let Ok(mut m) = metrics.lock() {
                    m.paused = false;
                    m.pause_jellyfin = false;
                }
            }
        }

        let ti = Instant::now();
        let s = tr.iteration();
        it += 1;
        steps += s.steps;
        eps += s.episodes;
        touches += s.touches;
        goals += s.goals;
        own_goals += s.own_goals;
        let dt = ti.elapsed().as_secs_f64();
        let (cur_stage, promoted, goal_rate) = {
            let mut c = curriculum.lock().expect("curriculum");
            let p = if pinned.is_none() {
                c.observe(IterResult {
                    touch_rate: s.touch_rate,
                    goals: s.goals,
                    episodes: s.episodes,
                })
            } else {
                None
            };
            (c.stage(), p, c.goal_rate())
        };
        if let Some(mv) = promoted {
            match mv {
                Move::Up(n) => eprintln!("promotion : palier {n}"),
                Move::Down(n) => eprintln!("régression : palier {n} (taux de but effondré)"),
            }
            tr.set_env_config(&EnvConfig::for_stage(mv.stage()));
        }
        println!(
            "{it} {cur_stage} {steps} {:.0} {:.3} {:.1} {:.2} {:.2} {} {} {:.3} {:.4} {:.2} {:.2}",
            s.steps as f64 / dt,
            s.reward_mean,
            s.length_mean,
            s.touch_rate,
            s.goal_rate,
            s.goals,
            s.own_goals,
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
            m.goals_scored_total = goals;
            m.goals_conceded_total = own_goals;
            m.goal_rate = goal_rate;
            m.curriculum_stage = cur_stage;
            m.observe_iteration(dt);
            m.checkpoint_age_seconds = saver_last.elapsed().as_secs_f64();
        }
        if saver_last.elapsed() > ckpt_every || promoted.is_some() {
            let mut s = Saver {
                tr: &tr,
                out: &out,
                curriculum: &curriculum,
                metrics: &metrics,
                last: saver_last,
            };
            s.save();
            saver_last = s.last;
        }
    }
    let mut s = Saver {
        tr: &tr,
        out: &out,
        curriculum: &curriculum,
        metrics: &metrics,
        last: saver_last,
    };
    s.save();
    eprintln!("terminé : {steps} pas, export dans {}", out.display());
}
