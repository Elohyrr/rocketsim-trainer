//! Enregistrement de clips pendant le jeu : une mécanique reconnue donne un clip qui se rejoue à ± tolérance.
//! Exige les meshes (ROCKETSIM_MESHES_DIR).

use rl_env::{Env, EnvConfig, Rng, VecEnv, init_meshes_from_env};
use sim_core::Sim;
use sim_core::clip::{Clip, DRIFT_BOUND, Replayer};

fn cfg() -> EnvConfig {
    EnvConfig {
        record_clips: true,
        ..EnvConfig::for_stage(5)
    }
}

fn check(clip: &Clip) {
    assert_ne!(clip.mechanic, "touch");
    assert_eq!(
        clip.blue,
        [true, false],
        "agent bleu puis adversaire orange"
    );
    assert!(clip.steps.len() >= 2 && clip.trigger_step < clip.steps.len() as u32);
    // Détecteur v3 (collisions) par défaut, qualité jointe au clip (vitesse de balle vers le but adverse, but dans les 3 s).
    assert_eq!(clip.detector, 3);
    // Étiquettes multiples : la principale est dans la liste, ordre chronologique, instants dans le clip, combo
    // cohérent avec la liste ; ni `touch` ni `bump`.
    assert!(
        clip.mechanics.iter().any(|l| l.mechanic == clip.mechanic),
        "étiquette principale absente de {:?}",
        clip.mechanics
    );
    assert!(clip.mechanics.windows(2).all(|w| w[0].step <= w[1].step));
    assert!(
        clip.mechanics
            .iter()
            .all(|l| l.step < clip.steps.len() as u32
                && l.mechanic != "touch"
                && l.mechanic != "bump")
    );
    let window = sim_core::clip::COMBO_WINDOW_TICKS / clip.tick_skip;
    assert_eq!(
        clip.combo,
        sim_core::clip::is_combo(&clip.mechanics, window)
    );
    let q = clip.quality.expect("qualité jointe au clip");
    assert!(
        q.ball_speed_to_goal >= 0.0 && (0.0..=1.0).contains(&q.score),
        "{q:?}"
    );
    let clip = Clip::from_json(&clip.to_json()).expect("aller-retour JSON");
    let mut sim = Sim::new_soccar().unwrap();
    let mut rp = Replayer::start(&mut sim, clip).unwrap();
    let d = rp.run_all(&mut sim);
    println!("clip {} : dérive {d} uu", rp.clip().mechanic);
    assert!(d <= DRIFT_BOUND, "dérive {d}");
}

#[test]
fn random_play_records_replayable_clips() {
    init_meshes_from_env().expect("meshes (ROCKETSIM_MESHES_DIR)");
    let mut env = Env::new(cfg(), 1).expect("env");
    let n = env.action_count() as u64;
    let mut rng = Rng::new(5);
    let mut clips = Vec::new();
    for _ in 0..30_000 {
        let r = env.step_vs(
            (rng.next_u64() % n) as usize,
            Some((rng.next_u64() % n) as usize),
        );
        if r.terminated || r.truncated {
            env.reset();
        }
        clips.extend(env.take_clips());
        if clips.len() >= 3 {
            break;
        }
    }
    assert!(!clips.is_empty(), "aucun clip en 30 000 décisions");
    for c in &clips {
        check(c);
    }
}

#[test]
fn clips_are_off_by_default_and_flow_through_vecenv() {
    init_meshes_from_env().expect("meshes");
    let mut off = Env::new(EnvConfig::for_stage(5), 1).unwrap();
    for _ in 0..500 {
        off.step_vs(0, Some(0));
    }
    assert!(off.take_clips().is_empty());

    let mut v = VecEnv::new(4, 2, cfg(), 9).expect("vecenv");
    v.reset();
    let mut rng = Rng::new(2);
    let n = v.action_count() as u64;
    let mut got = Vec::new();
    for _ in 0..3_000 {
        let a: Vec<usize> = (0..4).map(|_| (rng.next_u64() % n) as usize).collect();
        let o: Vec<usize> = (0..4).map(|_| (rng.next_u64() % n) as usize).collect();
        got.extend(v.step_vs(&a, Some(&o)).clips);
        if !got.is_empty() {
            break;
        }
    }
    assert!(!got.is_empty(), "aucun clip remonté par VecEnv");
    check(&got[0]);
}
