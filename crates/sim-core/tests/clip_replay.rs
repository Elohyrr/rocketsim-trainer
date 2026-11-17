//! Déterminisme du rejeu : un clip enregistré pendant une simulation se rejoue à l'identique (positions de la
//! balle et des voitures, à chaque décision) sur une simulation vierge : exactement pour un clip pris au début
//! d'un épisode, à DRIFT_BOUND près pour un clip pris en plein épisode. Exige les meshes (ROCKETSIM_MESHES_DIR).

use sim_core::clip::{Clip, ClipRecorder, DRIFT_BOUND, DRIFT_TOLERANCE, MAX_STEPS, Replayer};
use sim_core::{Controls, Sim, Snapshot, init_meshes, meshes_dir_from_env};

const TICK_SKIP: u32 = 8;

fn lcg(s: &mut u64) -> f32 {
    *s = s
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*s >> 33) as f32) / (u32::MAX >> 1) as f32
}

fn rand_controls(s: &mut u64) -> Controls {
    let ax = |s: &mut u64| (lcg(s) * 3.0).floor() - 1.0;
    Controls {
        throttle: 1.0,
        steer: ax(s),
        pitch: ax(s),
        yaw: ax(s),
        roll: ax(s),
        boost: lcg(s) < 0.8,
        jump: lcg(s) < 0.15,
        handbrake: lcg(s) < 0.05,
    }
}

/// Joue `n` décisions avec des contrôles pseudo-aléatoires ; déclenche à la décision `trigger`.
/// Retourne le clip et la trace des snapshots (un par décision, avant les contrôles).
fn record(n: u32, trigger: u32, seed: u64) -> (Clip, Vec<Snapshot>) {
    record_with(n, trigger, seed, false)
}

/// Comme `record`, dans une simulation « sale » (un épisode d'abord joué au hasard contre les murs, comme dans
/// l'entraînement où la simulation vit longtemps), puis remise en état comme `Env::reset`.
fn record_with(n: u32, trigger: u32, seed: u64, dirty: bool) -> (Clip, Vec<Snapshot>) {
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("init meshes");
    let mut sim = Sim::new_soccar().unwrap();
    let (a, b) = (sim.add_car(true), sim.add_car(false));
    sim.reset_kickoff(7);
    let mut rng = seed ^ 0xABCDEF;
    for _ in 0..if dirty { 150 } else { 0 } {
        sim.set_controls(a, rand_controls(&mut rng)).unwrap();
        sim.set_controls(b, rand_controls(&mut rng)).unwrap();
        sim.step(TICK_SKIP);
    }
    sim.reset_kickoff(7);
    sim.reset_ball([0.0, 0.0, 93.15], [0.0; 3]);
    sim.reset_car(a, [-1500.0, -2000.0, 17.0], 1.0, [300.0, 500.0, 0.0], 33.0)
        .unwrap();
    sim.reset_car(b, [1800.0, 2300.0, 17.0], -2.0, [0.0; 3], 33.0)
        .unwrap();
    sim.set_controls(a, Controls::default()).unwrap();
    sim.set_controls(b, Controls::default()).unwrap();
    let mut rec = ClipRecorder::new(TICK_SKIP);
    let mut rng = seed;
    let mut trace = Vec::new();
    let mut clip = None;
    for step in 0..n {
        trace.push(sim.snapshot());
        let ctl = [rand_controls(&mut rng), rand_controls(&mut rng)];
        rec.begin_step(&mut sim, &ctl);
        sim.set_controls(a, ctl[0]).unwrap();
        sim.set_controls(b, ctl[1]).unwrap();
        sim.step(TICK_SKIP);
        if step == trigger {
            rec.trigger("flip_reset");
        }
        if let Some(c) = rec.end_step(&mut sim) {
            clip = Some(c);
        }
    }
    trace.push(sim.snapshot());
    (clip.expect("un clip"), trace)
}

fn max_diff(a: &Snapshot, b: &Snapshot) -> f32 {
    let d = |x: [f32; 3], y: [f32; 3]| (0..3).map(|i| (x[i] - y[i]).abs()).fold(0.0, f32::max);
    let mut m = d(a.ball.pos, b.ball.pos);
    for (c, e) in a.cars.iter().zip(&b.cars) {
        m = m.max(d(c.pos, e.pos));
    }
    m
}

#[test]
fn replay_is_deterministic() {
    let (clip, trace) = record(200, 120, 42);
    // Le clip passe par le JSON (comme sur le réseau) avant d'être rejoué.
    let clip = Clip::from_json(&clip.to_json()).unwrap();
    assert!(clip.steps.len() as u32 <= MAX_STEPS, "{}", clip.steps.len());
    assert!(clip.trigger_step < clip.steps.len() as u32);

    let first = clip.keyframes[0].tick;
    let start = trace.iter().position(|s| s.tick == first).unwrap();

    // Clip pris en plein épisode (état interne de RocketSim non capturable) : avec recalage sur les états clés,
    // la trajectoire reste à DRIFT_BOUND de l'originale à chaque décision.
    let mut sim = Sim::new_soccar().unwrap();
    let mut rp = Replayer::start(&mut sim, clip.clone()).unwrap();
    let mut worst = 0.0f32;
    let mut s = 0;
    loop {
        let (step, total) = rp.progress();
        if step > s || step == total {
            let (got, want) = (sim.snapshot(), &trace[start + step]);
            let dd = max_diff(&got, want);

            worst = worst.max(dd);
            s = step;
        }
        if rp.tick(&mut sim) {
            break;
        }
    }
    assert!(worst <= DRIFT_BOUND, "écart {worst} uu");
    assert!(
        rp.max_drift <= DRIFT_BOUND,
        "dérive aux états clés {}",
        rp.max_drift
    );
    println!(
        "écart de trajectoire {worst} uu, aux états clés {}",
        rp.max_drift
    );

    // Rejeu en boucle (le client relance le clip) : deux passes identiques, bit à bit.
    let mut sim2 = Sim::new_soccar().unwrap();
    let mut rp2 = Replayer::start(&mut sim2, clip).unwrap();
    rp2.run_all(&mut sim2);
    let first = sim2.snapshot();
    rp2.restart(&mut sim2).unwrap();
    rp2.run_all(&mut sim2);
    assert!(max_diff(&first, &sim2.snapshot()) <= DRIFT_BOUND);
}

#[test]
fn clip_replays_the_mechanic_trajectory_on_a_busy_scene() {
    // Autre graine et déclenchement précoce (moins de LEAD_STEPS d'historique) : le clip commence à l'épisode.
    let (clip, trace) = record(80, 10, 3);
    assert_eq!(clip.keyframes[0].step, 0);
    let start = trace
        .iter()
        .position(|s| s.tick == clip.keyframes[0].tick)
        .unwrap();
    assert_eq!(start, 0);
    let mut sim = Sim::new_soccar().unwrap();
    let mut rp = Replayer::start(&mut sim, clip.clone()).unwrap();
    rp.resync = false;
    assert!(rp.run_all(&mut sim) <= DRIFT_TOLERANCE);
    let last = sim.snapshot();
    assert!(max_diff(&last, &trace[start + clip.steps.len()]) <= DRIFT_TOLERANCE);
}

#[test]
fn long_episode_keeps_a_bounded_clip_and_stays_close() {
    // Simulation « sale » (comme en entraînement) : le rejeu sur une simulation vierge dérive un peu, mais
    // l'écart aux états clés (avant recalage) reste sous DRIFT_BOUND.
    let (clip, _) = record_with(260, 200, 9, true);
    assert!(clip.steps.len() as u32 <= MAX_STEPS, "{}", clip.steps.len());
    let mut sim = Sim::new_soccar().unwrap();
    let mut rp = Replayer::start(&mut sim, clip).unwrap();
    let d = rp.run_all(&mut sim);
    println!("clip {} décisions", rp.progress().1);
    println!("dérive max aux états clés (recalage 1 s) : {d} uu");
    assert!(d <= DRIFT_BOUND, "{d}");
}
