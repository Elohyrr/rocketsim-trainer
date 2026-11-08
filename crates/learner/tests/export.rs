//! Équivalence learner (burn) ↔ policy-runtime (MLP manuelle) à 1e-5 près, et reprise depuis un checkpoint.
//! Exige les meshes (`scripts/fetch-meshes.sh`).

use learner::ppo::{PpoConfig, Trainer};
use policy_runtime::Policy;
use rl_env::EnvConfig;

fn small() -> PpoConfig {
    PpoConfig {
        n_envs: 2,
        threads: 1,
        horizon: 16,
        hidden: 32,
        minibatch: 16,
        epochs: 1,
        ..Default::default()
    }
}

#[test]
fn exported_policy_matches_burn_and_checkpoint_resumes() {
    let dir = std::env::temp_dir().join(format!("learner-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut tr =
        Trainer::new(small(), EnvConfig::default()).expect("meshes (ROCKETSIM_MESHES_DIR)");
    tr.iteration();
    let st = dir.join("policy.safetensors");
    tr.export(&st).unwrap();
    let pol = Policy::load(&st).unwrap();
    assert_eq!((pol.obs_dim(), pol.action_count()), (rl_env::OBS_DIM, 78));
    let obs: Vec<f32> = (0..rl_env::OBS_DIM)
        .map(|i| (i as f32 * 0.37).sin())
        .collect();
    let (a, b) = (tr.policy_logits(&obs), pol.logits(&obs));
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() < 1e-5, "{x} vs {y}");
    }

    // Checkpoint puis reprise dans un nouveau trainer : mêmes logits.
    let ck = dir.join("checkpoint");
    tr.save_checkpoint(&ck).unwrap();
    let mut tr2 = Trainer::new(
        PpoConfig {
            seed: 99,
            ..small()
        },
        EnvConfig::default(),
    )
    .unwrap();
    assert_ne!(tr2.policy_logits(&obs), a);
    tr2.load_checkpoint(&ck).unwrap();
    for (x, y) in tr2.policy_logits(&obs).iter().zip(&a) {
        assert!((x - y).abs() < 1e-6);
    }
    std::fs::remove_dir_all(&dir).ok();
}
