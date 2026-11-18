//! Entraînement PPO H24 : curriculum, checkpoints périodiques, reprise, `/metrics`, pause Jellyfin.
//!
//! learner --eval-kickoff N --policy policy.safetensors [--idle] : évalue les coups d'envoi (voir `eval.rs`).
//! learner [--minutes M] [--envs N] [--threads T] [--out DIR] [--metrics ADDR] [--resume]
//!         [--checkpoint-minutes N] [--stage S] [--init-actor policy.safetensors [--critic-warmup N]]
//!         [--elo-minutes N] [--pool-every N]
//! `--minutes 0` = sans limite (mode cluster). `--stage S` fige le palier (pas de promotion automatique).
//! Variables : `JELLYFIN_URL`, `JELLYFIN_API_KEY` (pause D6, désactivée sans clé), `JELLYFIN_RESUME_MINUTES`
//! (défaut 10), `JELLYFIN_POLL_SECONDS` (défaut 30). SIGTERM/SIGINT : checkpoint final puis sortie.
//! Détecteur de mécaniques : règles strictes v2 par défaut ; `--detector-v1` (ou `DETECTOR_V1=1`) garde les règles
//! d'origine. Au premier démarrage v2, l'anneau de clips est vidé une fois (marqueur `clips/detecteur-v2`).
//! Clips : une mécanique reconnue en match enregistre un clip rejouable dans `<out>/clips` (anneau borné :
//! `--clips-max N` (200), `--clips-mb M` (200) ; `--no-clips` pour couper), servi par `GET /clips` et `GET /clips/<id>`.
//! Le dossier `--out` reçoit `checkpoint.mpk`, `policy.safetensors`, `stage`, `opponent-obs-v2`, `explore.json`,
//! `elo.json`, `pool/` (anciennes politiques du self-play) et `refs/` (références d'Elo : `docs/self-play.md`). Le serveur
//! `--metrics` sert aussi en lecture seule `GET /policy.safetensors` et `GET /policy.json` (palier, taille, date).
//! Reprise d'un checkpoint sans `opponent-obs-v2` (entraîné avant la correction de l'ordre des voitures) : les poids
//! d'entrée du bloc adversaire sont remis à zéro une fois (voir `docs/curriculum.md`).
//! Points de restauration (`best-stage-N/`, `last-start/`, fichier `restore-from`) : voir `best.rs` et
//! `docs/exploration.md`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use learner::best::{self, Best, OPPONENT_OBS_MARKER};
use learner::clips::{ClipStore, DEFAULT_MAX_CLIPS};
use learner::curriculum::{Curriculum, IterResult, Move};
use learner::elo;
use learner::explore::{Explore, Guard, GuardEvent};
use learner::jellyfin::Jellyfin;
use learner::metrics::{Metrics, serve_with_dir};
use learner::pause::PauseGate;
use learner::pool::Pool;
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

fn fmt_ref(r: Option<(f64, f64)>) -> String {
    r.map_or("—".into(), |(p, e)| {
        format!("{:.0} % → {e:.0}", 100.0 * p)
    })
}

/// Palier courant du curriculum (peut avoir changé après la mesure d'Elo).
fn tr_stage_now(c: &Mutex<Curriculum>, fallback: u32) -> u32 {
    c.lock().map_or(fallback, |c| c.stage())
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
                .is_ok_and(|c| c.save(self.out).is_ok())
            && self.tr.explore.save(self.out).is_ok()
            && std::fs::write(self.out.join(OPPONENT_OBS_MARKER), "1\n").is_ok();
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
    if let Some(n) = arg("--eval-kickoff") {
        let policy = arg("--policy").unwrap_or_else(|| "runs/latest/policy.safetensors".into());
        let idle = std::env::args().any(|a| a == "--idle");
        if let Err(e) = learner::eval::run(Path::new(&policy), n.parse().unwrap_or(40), idle) {
            eprintln!("{e}");
            std::process::exit(2);
        }
        return;
    }
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
    match best::apply_restore(&out) {
        Ok(Some(src)) => eprintln!("état restauré depuis {}", src.display()),
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
    let pinned: Option<u32> = arg("--stage").and_then(|s| s.parse().ok());
    let curriculum = Mutex::new(match pinned {
        Some(s) => {
            let mut c = Curriculum::new(s);
            if resume {
                c.elo = learner::elo::EloState::load(&out);
            }
            c
        }
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
        match serve_with_dir(&addr, metrics.clone(), Some(out.clone())) {
            Ok(a) => eprintln!("metrics sur http://{a}/metrics"),
            Err(e) => eprintln!("metrics indisponibles : {e}"),
        }
    }
    let watcher = spawn_watcher();
    let mut gate = PauseGate::new(Duration::from_secs(
        env_num("JELLYFIN_RESUME_MINUTES", 10) * 60,
    ));

    // `--kickoff-prob P` : surcharge la part de coups d'envoi aux paliers 5/6 (0 = témoin sans kickoff).
    let kp: Option<f32> = arg("--kickoff-prob").and_then(|s| s.parse().ok());
    let want_clips = !std::env::args().any(|a| a == "--no-clips");
    // `--detector-v1` ou `DETECTOR_V1=1` : règles d'origine du détecteur de mécaniques (sinon v2, strictes).
    let detector_v1 = std::env::args().any(|a| a == "--detector-v1")
        || std::env::var("DETECTOR_V1").is_ok_and(|v| v == "1");
    let mut clip_store = want_clips
        .then(|| {
            let max = arg("--clips-max")
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MAX_CLIPS);
            let mb: u64 = arg("--clips-mb")
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);
            ClipStore::open_for(&out, max, mb * 1024 * 1024, !detector_v1)
                .inspect_err(|e| eprintln!("clips désactivés : {e}"))
                .ok()
        })
        .flatten();
    let record_clips = clip_store.is_some();
    let env_cfg = move |st: u32| {
        let c = EnvConfig::for_stage(st);
        EnvConfig {
            record_clips,
            detector_v1,
            kickoff_prob: if c.kickoff_prob > 0.0 {
                kp.unwrap_or(c.kickoff_prob)
            } else {
                0.0
            },
            ..c
        }
    };
    let mut tr = Trainer::new(cfg, env_cfg(stage)).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let mut loaded = false;
    if resume {
        let ckpt = out.join("checkpoint");
        match tr.load_checkpoint(&ckpt) {
            Ok(()) => {
                eprintln!("reprise depuis {} (palier {stage})", ckpt.display());
                loaded = true;
                tr.explore = Explore::load(&out);
                eprintln!(
                    "exploration : ent_coef {:.4}, {} pas cumulés, lr {:.2e}",
                    tr.explore.ent_coef,
                    tr.explore.steps,
                    tr.explore.lr()
                );
                if !out.join(OPPONENT_OBS_MARKER).exists() {
                    tr.reset_opponent_inputs();
                    eprintln!(
                        "checkpoint d'avant la correction de l'ordre des voitures : bloc adversaire remis à zéro"
                    );
                }
            }
            Err(e) => eprintln!("pas de reprise ({e}) : départ à neuf"),
        }
    }
    // Outil d'essai local : repartir de l'acteur d'une politique servie (critique neuf, échauffé d'abord).
    let mut warmup: usize = 0;
    if let Some(f) = arg("--init-actor") {
        match tr.load_actor(Path::new(&f)) {
            Ok(()) => {
                warmup = arg("--critic-warmup")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(300);
                tr.freeze_policy = warmup > 0;
                eprintln!(
                    "acteur chargé depuis {f} ; échauffement du critique : {warmup} itérations"
                );
            }
            Err(e) => {
                eprintln!("--init-actor : {e}");
                std::process::exit(2);
            }
        }
    }
    // Points de restauration : l'état relu (avant toute itération) et, s'il n'y en a pas, le « meilleur » du palier.
    let mut best_by_stage = Best::load(&out);
    if loaded {
        let ok = best::snapshot(&tr, &out, best::LAST_START, stage, -1.0).is_ok()
            && (best::best_dir(&out, stage).exists()
                || best::snapshot(&tr, &out, &format!("best-stage-{stage}"), stage, -1.0).is_ok());
        if !ok {
            eprintln!("point de restauration au démarrage en échec");
        } else if best_by_stage.get(stage).is_none() {
            best_by_stage.record(stage, -1.0, 0);
        }
    }
    let mut guard = Guard::default();
    let mut backoffs = 0u64;
    // Pool d'anciennes politiques : rechargé du disque (il survit aux redémarrages, comme le checkpoint).
    tr.pool = Pool::load(&out.join("pool"), &tr.policy_snapshot(), &tr.plain_device());
    if let Some(n) = arg("--pool-every").and_then(|s| s.parse::<usize>().ok()) {
        tr.pool.every = n.max(1);
    }
    eprintln!(
        "palier de curriculum : {stage}, pool : {} anciens",
        tr.pool.len()
    );
    // Elo : première mesure dès qu'on est au palier 5 ou 6, puis toutes les `--elo-minutes` (30 par défaut).
    let elo_every = Duration::from_secs_f64(
        arg("--elo-minutes")
            .and_then(|s| s.parse().ok())
            .unwrap_or(30.0f64)
            * 60.0,
    );
    let mut elo_last: Option<Instant> = None;
    let (mut stage_up, mut stage_down) = (0u64, 0u64);
    let mut sp_goals = [[0u64; 2]; 2];

    let t0 = Instant::now();
    let limit = (minutes > 0.0).then(|| Duration::from_secs_f64(minutes * 60.0));
    let (mut steps, mut eps, mut touches) = (0u64, 0u64, 0u64);
    let mut mech_total = mechanics::Counts::default();
    let (mut goals, mut own_goals) = (0u64, 0u64);
    let (mut kick_won, mut kick_lost, mut kick_neutral) = (0u64, 0u64, 0u64);
    let mut it = 0;
    let mut clips_saved = 0u64;
    let mut saver_last = Instant::now();
    println!(
        "iter stage steps_total steps/s ep_reward ep_len touch_rate goal_rate goals own_goals entropy kl collect_s train_s kick_won kick_lost kick_neutral kick_t_s kick_speed ent_coef lr"
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
        if tr.freeze_policy && it >= warmup {
            tr.freeze_policy = false;
            eprintln!("échauffement du critique terminé");
        }
        let mut clips_new = 0;
        if let Some(store) = clip_store.as_mut() {
            for c in s.clips.iter().cloned() {
                match store.add(c, it as u64) {
                    Ok(Some(e)) => {
                        clips_new += 1;
                        eprintln!("clip {} ({} décisions, {:.1} s)", e.id, e.steps, e.seconds);
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("clip non écrit : {e}"),
                }
            }
        }
        clips_saved += clips_new;
        steps += s.steps;
        eps += s.episodes;
        touches += s.touches;
        goals += s.goals;
        own_goals += s.own_goals;
        kick_won += s.kick_won;
        kick_lost += s.kick_lost;
        kick_neutral += s.kick_neutral;
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
        let mut promoted = promoted;
        if let Some(mv) = promoted {
            match mv {
                Move::Up(n) => {
                    stage_up += 1;
                    eprintln!("promotion : palier {n}");
                    if n == 6 {
                        // Référence figée « palier 5 » : la politique qui vient de le réussir.
                        let dir = elo::refs_dir(&out);
                        let ok = std::fs::create_dir_all(&dir).is_ok()
                            && Pool::write_net(
                                &tr.policy_snapshot(),
                                &dir.join("stage5.safetensors"),
                            )
                            .is_ok();
                        if ok && let Ok(mut c) = curriculum.lock() {
                            c.elo.rating_stage5 = None;
                        }
                    }
                }
                Move::Down(n) => {
                    stage_down += 1;
                    eprintln!("régression : palier {n} (taux de but effondré)");
                }
            }
            tr.set_env_config(&env_cfg(mv.stage()));
            guard.reset();
        }
        // Garde-fou d'exploration et meilleur état du palier (paliers 2 à 5, jugés sur le taux de but ; le palier 6
        // est jugé sur l'Elo, voir plus bas : son taux de but est symétrique).
        if (2..elo::SELFPLAY_STAGE).contains(&cur_stage) && promoted.is_none() {
            match guard.observe(s.goals, s.episodes, tr.explore.scale < 1.0) {
                Some(GuardEvent::BackOff { recent, reference }) => {
                    tr.explore.back_off();
                    backoffs += 1;
                    eprintln!(
                        "garde-fou : taux de but {recent:.2} < moitié de la référence {reference:.2} ; cible d'entropie × {:.2}, ent_coef {:.4}",
                        tr.explore.scale, tr.explore.ent_coef
                    );
                }
                Some(GuardEvent::Recover) => {
                    tr.explore.recover();
                    eprintln!(
                        "garde-fou : longue période saine, cible d'entropie × {:.2}",
                        tr.explore.scale
                    );
                }
                None => {}
            }
            if let Some(r) = guard.recent_rate()
                && best_by_stage.wants(cur_stage, r, it)
            {
                let name = format!("best-stage-{cur_stage}");
                match best::snapshot(&tr, &out, &name, cur_stage, r) {
                    Ok(_) => {
                        best_by_stage.record(cur_stage, r, it);
                        eprintln!(
                            "meilleur état du palier {cur_stage} : taux de but {r:.2} → {name}"
                        );
                    }
                    Err(e) => eprintln!("{name} : {e}"),
                }
            }
        }
        if cur_stage >= 5 && elo_last.is_none_or(|t| t.elapsed() >= elo_every) {
            let mut st = curriculum.lock().expect("curriculum").elo.clone();
            let best_before = st.best;
            let te = Instant::now();
            match elo::measure(&tr.policy_snapshot(), &mut st, &out, &tr.plain_device()) {
                Ok(rep) => {
                    let mv = {
                        let mut c = curriculum.lock().expect("curriculum");
                        c.elo = st.clone();
                        if pinned.is_none() {
                            c.after_elo()
                        } else {
                            None
                        }
                    };
                    eprintln!(
                        "elo {:.0} (lissé {:.0}, record {:.0}) : fonceur {}, palier 5 {}, ancien {} ; {:.0} s",
                        rep.elo,
                        st.ema.unwrap_or(rep.elo),
                        st.best,
                        fmt_ref(rep.per_ref[0]),
                        fmt_ref(rep.per_ref[1]),
                        fmt_ref(rep.per_ref[2]),
                        te.elapsed().as_secs_f64()
                    );
                    if let Ok(mut m) = metrics.lock() {
                        m.elo = st.ema.unwrap_or(rep.elo);
                        m.elo_best = st.best;
                        for (i, r) in rep.per_ref.iter().enumerate() {
                            (m.elo_winrate[i], m.elo_vs[i]) = r.unwrap_or((0.0, 0.0));
                        }
                        m.elo_matches_total += u64::from(rep.matches);
                    }
                    // Palier 6 : le « meilleur état » est celui du record d'Elo (taux de but inconnu : −1).
                    if cur_stage == elo::SELFPLAY_STAGE && st.best > best_before {
                        let name = format!("best-stage-{cur_stage}");
                        match best::snapshot(&tr, &out, &name, cur_stage, -1.0) {
                            Ok(_) => eprintln!(
                                "meilleur état du palier {cur_stage} : Elo {:.0} → {name}",
                                st.best
                            ),
                            Err(e) => eprintln!("{name} : {e}"),
                        }
                    }
                    if let Some(mv) = mv {
                        stage_down += 1;
                        guard.reset();
                        eprintln!(
                            "régression : palier {} (Elo en baisse sur {} mesures)",
                            mv.stage(),
                            elo::LOW_STREAK
                        );
                        tr.set_env_config(&env_cfg(mv.stage()));
                        promoted = Some(mv);
                    }
                }
                Err(e) => eprintln!("mesure d'Elo en échec : {e}"),
            }
            elo_last = Some(Instant::now());
        }
        println!(
            "{it} {cur_stage} {steps} {:.0} {:.3} {:.1} {:.2} {:.2} {} {} {:.3} {:.4} {:.2} {:.2} {} {} {} {:.2} {:.0} {:.4} {:.2e}",
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
            s.train_seconds,
            s.kick_won,
            s.kick_lost,
            s.kick_neutral,
            s.kick_touch_s_sum / s.kick_touched.max(1) as f64,
            s.kick_speed_sum / s.kick_touched.max(1) as f64,
            s.ent_coef,
            s.lr
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
            m.entropy_target = tr.explore.target(tr.n_actions());
            m.entropy_scale = tr.explore.scale;
            m.entropy_backoffs = backoffs;
            m.ent_coef = s.ent_coef;
            m.learning_rate = s.lr;
            m.touch_total = touches;
            mech_total.add(&s.mechanics);
            m.mechanics = mech_total.clone();
            m.clips_saved_known = clips_saved;
            m.goals_scored_total = goals;
            m.goals_conceded_total = own_goals;
            m.goal_rate = goal_rate;
            m.kickoffs_won = kick_won;
            m.kickoffs_lost = kick_lost;
            m.kickoffs_neutral = kick_neutral;
            if s.kick_touched > 0 {
                m.kickoff_first_touch_seconds = s.kick_touch_s_sum / s.kick_touched as f64;
                m.kickoff_touch_speed = s.kick_speed_sum / s.kick_touched as f64;
            }
            m.curriculum_stage = tr_stage_now(&curriculum, cur_stage);
            m.pool_size = tr.pool.len() as u64;
            m.pool_snapshots_total = tr.pool.snapshots_taken;
            sp_goals[0][0] += s.cur_goals;
            sp_goals[0][1] += s.cur_conceded;
            sp_goals[1][0] += s.pool_goals;
            sp_goals[1][1] += s.pool_conceded;
            m.selfplay_goals = sp_goals;
            m.stage_up_total = stage_up;
            m.stage_down_total = stage_down;
            m.elo_age_seconds = elo_last.map_or(0.0, |t| t.elapsed().as_secs_f64());
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
