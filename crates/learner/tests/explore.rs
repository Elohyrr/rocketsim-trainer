//! Régulateur d'exploration branché sur l'entraînement : l'entropie pilote `ent_coef`, le plafond de KL coupe
//! les époques, `load_actor` relit une politique exportée. Exige les meshes (ROCKETSIM_MESHES_DIR).

use learner::explore;
use learner::ppo::{PpoConfig, Trainer};
use rl_env::{EnvConfig, OBS_DIM};

fn tiny() -> PpoConfig {
    PpoConfig {
        n_envs: 2,
        threads: 1,
        horizon: 32,
        hidden: 32,
        minibatch: 16,
        epochs: 4,
        ..Default::default()
    }
}

#[test]
fn iteration_reports_and_adapts_the_entropy_coefficient() {
    let mut tr = Trainer::new(tiny(), EnvConfig::for_stage(2)).expect("meshes");
    let before = tr.explore.ent_coef;
    let s = tr.iteration();
    assert!(
        (s.ent_coef - before).abs() < 1e-6,
        "l'itération utilise le coefficient courant"
    );
    assert!((s.lr - explore::LR0).abs() < 1e-12);
    assert!((1..=4).contains(&s.epochs_run));
    assert_eq!(tr.explore.steps, s.steps);
    // Politique initiale ≈ uniforme (H ≈ ln 78 ≈ 4,36 > cible) : le coefficient baisse.
    assert!(
        s.entropy > tr.explore.target(tr.n_actions()),
        "entropie initiale {}",
        s.entropy
    );
    assert!(tr.explore.ent_coef < before);
}

#[test]
fn actor_exported_then_loaded_gives_same_logits_and_frozen_policy_does_not_move() {
    let dir = std::env::temp_dir().join(format!("learner-explore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut a =
        Trainer::new(PpoConfig { seed: 7, ..tiny() }, EnvConfig::for_stage(2)).expect("meshes");
    a.iteration();
    a.export(&dir.join("policy.safetensors")).unwrap();
    let mut b = Trainer::new(PpoConfig { seed: 9, ..tiny() }, EnvConfig::for_stage(2)).unwrap();
    let obs: Vec<f32> = (0..OBS_DIM).map(|i| (i as f32 * 0.3).cos()).collect();
    assert_ne!(a.policy_logits(&obs), b.policy_logits(&obs));
    b.load_actor(&dir.join("policy.safetensors")).unwrap();
    let (la, lb) = (a.policy_logits(&obs), b.policy_logits(&obs));
    for (x, y) in la.iter().zip(&lb) {
        assert!((x - y).abs() < 1e-5);
    }
    b.freeze_policy = true;
    b.iteration();
    assert_eq!(
        b.policy_logits(&obs),
        lb,
        "politique gelée : logits inchangés"
    );
    b.freeze_policy = false;
    b.iteration();
    assert_ne!(b.policy_logits(&obs), lb);
    let _ = std::fs::remove_dir_all(&dir);
}
