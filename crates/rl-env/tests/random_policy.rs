//! Politique aléatoire pendant 10 min de jeu : aucun faux positif sur les mécaniques rares (spec § 6, PR 2),
//! et les comptages restent cohérents (touchers détectés = touchers de l'env). Exige les meshes.

use mechanics::{Counts, Mechanic};
use rl_env::{Env, EnvConfig, Rng, init_meshes_from_env};

#[test]
fn ten_minutes_of_random_play_has_no_rare_mechanic() {
    init_meshes_from_env().expect("meshes (ROCKETSIM_MESHES_DIR)");
    let cfg = EnvConfig::for_stage(5);
    // 10 min = 72 000 ticks = 9 000 décisions à 8 ticks.
    let mut total = Counts::default();
    let (mut rng, mut touches) = (Rng::new(3), 0u64);
    for seed in 0..3 {
        let mut env = Env::new(cfg.clone(), seed).expect("env");
        let n = env.action_count() as u64;
        for _ in 0..3_000 {
            let r = env.step_vs(
                (rng.next_u64() % n) as usize,
                Some((rng.next_u64() % n) as usize),
            );
            touches += u64::from(r.touched);
            if r.terminated || r.truncated {
                env.reset();
            }
        }
        total.add(&env.take_mechanics());
    }
    println!("{total:?}");
    for m in [
        Mechanic::FlipReset,
        Mechanic::SpeedFlip,
        Mechanic::CeilingShot,
        Mechanic::AirDribble,
        Mechanic::Flick,
    ] {
        assert_eq!(
            total.get(m),
            0,
            "{} sur politique aléatoire : {total:?}",
            m.label()
        );
    }
    // Un toucher par décision au plus dans l'env ; le détecteur voit chaque toucher au tick près.
    assert!(
        total.get(Mechanic::Touch) >= touches,
        "{} < {touches}",
        total.get(Mechanic::Touch)
    );
}
