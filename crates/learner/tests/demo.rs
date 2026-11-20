//! Clonage de comportement mélangé à PPO : avec des démonstrations en mémoire, la perte de clonage baisse ; sans
//! démonstration ou avec λ = 0 elle reste nulle. Exige les meshes.

use learner::ppo::{PpoConfig, Trainer};
use rl_env::{EnvConfig, OBS_DIM};

fn trainer() -> Trainer {
    let cfg = PpoConfig {
        n_envs: 2,
        threads: 1,
        horizon: 32,
        hidden: 32,
        minibatch: 32,
        epochs: 2,
        ..Default::default()
    };
    Trainer::new(cfg, EnvConfig::for_stage(2)).expect("meshes (ROCKETSIM_MESHES_DIR)")
}

#[test]
fn la_perte_de_clonage_baisse_et_lambda_decroit() {
    let mut tr = trainer();
    tr.bc.lambda0 = 1.0;
    tr.bc.decay_steps = 300;
    tr.set_reference();
    // Deux « états » distincts, deux actions de la table.
    for i in 0..64 {
        let mut o = vec![0.0f32; OBS_DIM];
        let k = i % 2;
        o[0] = if k == 0 { 0.5 } else { -0.5 };
        o[1] = 0.1 * (i as f32);
        tr.demo.push(&o, if k == 0 { 3 } else { 40 });
    }
    let first = tr.iteration();
    assert!(first.bc_loss > 0.0 && first.bc_lambda > 0.9, "{first:?}");
    let mut last = first.clone();
    for _ in 0..8 {
        last = tr.iteration();
    }
    assert!(
        last.bc_loss < first.bc_loss,
        "{} -> {}",
        first.bc_loss,
        last.bc_loss
    );
    // 9 itérations × 64 pas > 300 : λ est retombé à 0 et le clonage est coupé.
    assert_eq!(last.bc_lambda, 0.0, "{last:?}");
    assert_eq!(last.bc_loss, 0.0);
}

#[test]
fn sans_demonstration_ou_lambda_nul_rien_ne_change() {
    let mut tr = trainer();
    assert_eq!(tr.iteration().bc_lambda, 0.0);
    // Mémoire garnie mais λ0 = 0 (défaut) : désactivé.
    tr.demo.push(&vec![0.0; OBS_DIM], 3);
    let s = tr.iteration();
    assert_eq!((s.bc_lambda, s.bc_loss), (0.0, 0.0));
    // λ0 > 0 mais mémoire vide : désactivé.
    let mut tr = trainer();
    tr.bc.lambda0 = 1.0;
    assert_eq!(tr.iteration().bc_lambda, 0.0);
}
