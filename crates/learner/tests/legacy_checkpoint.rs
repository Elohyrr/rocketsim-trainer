//! Un checkpoint d'avant l'observation adversaire (entrée de 33 valeurs) se recharge : couche d'entrée agrandie,
//! nouveaux poids nuls, mêmes logits tant que le bloc adversaire est nul. Exige les meshes.

use burn::module::Module;
use burn::tensor::{Device, Tensor, TensorData};
use learner::ppo::{ActorCritic, Mlp, PpoConfig, Trainer};
use rl_env::{EnvConfig, OBS_DIM, OBS_DIM_V1};

#[test]
fn old_format_checkpoint_loads_with_zero_new_weights() {
    let cfg = PpoConfig {
        n_envs: 2,
        threads: 1,
        horizon: 16,
        hidden: 32,
        minibatch: 16,
        epochs: 1,
        ..Default::default()
    };
    let dir = std::env::temp_dir().join(format!("learner-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let device = Device::flex().autodiff();
    device.seed(5);
    let legacy = ActorCritic {
        policy: Mlp::new(OBS_DIM_V1, cfg.hidden, 78, &device),
        value: Mlp::new(OBS_DIM_V1, cfg.hidden, 1, &device),
    };
    let ck = dir.join("checkpoint");
    legacy.clone().save_file(&ck).unwrap();

    let mut tr = Trainer::new(cfg, EnvConfig::for_stage(2)).expect("meshes (ROCKETSIM_MESHES_DIR)");
    tr.load_checkpoint(&ck).unwrap();

    let base: Vec<f32> = (0..OBS_DIM_V1).map(|i| (i as f32 * 0.37).sin()).collect();
    let x = Tensor::<2>::from_data(TensorData::new(base.clone(), [1, OBS_DIM_V1]), &device);
    let expected: Vec<f32> = legacy.policy.forward(x).into_data().try_to_vec().unwrap();
    let mut obs = base.clone();
    obs.resize(OBS_DIM, 0.0);
    let got = tr.policy_logits(&obs);
    for (a, b) in got.iter().zip(&expected) {
        assert!((a - b).abs() < 1e-5, "{a} vs {b}");
    }
    // Un adversaire présent n'a aucun effet tant que les nouveaux poids sont nuls.
    let mut with_opp = obs.clone();
    for v in &mut with_opp[OBS_DIM_V1..] {
        *v = 0.7;
    }
    assert_eq!(tr.policy_logits(&with_opp), got);
    // Et l'entraînement continue (les nouveaux poids deviennent apprenables).
    tr.iteration();
    std::fs::remove_dir_all(&dir).ok();
}
