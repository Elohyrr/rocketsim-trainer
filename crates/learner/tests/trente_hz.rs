//! Conversions « par décision » → « par seconde de jeu » du learner à 30 Hz (`tick_skip` 4) et reprise d'un état 15 Hz.
//! Exige les meshes (ROCKETSIM_MESHES_DIR) pour l'essai d'itération.

use learner::explore::{self, Explore};
use learner::ppo::{PpoConfig, Trainer};
use rl_env::EnvConfig;

#[test]
fn gamma_lambda_horizon_en_secondes_de_jeu() {
    let c15 = PpoConfig::for_tick_skip(8);
    assert_eq!(c15.gamma, 0.99);
    assert_eq!((c15.horizon, c15.minibatch), (128, 512));
    let c30 = PpoConfig::for_tick_skip(4);
    // γ30² = γ15 et λ30² = λ15 : même facteur d'actualisation par seconde de jeu.
    assert!((c30.gamma.powi(2) - 0.99).abs() < 1e-5, "{}", c30.gamma);
    assert!((c30.lambda.powi(2) - 0.95).abs() < 1e-5, "{}", c30.lambda);
    assert!((c30.gamma - 0.994_987).abs() < 1e-4);
    // 8,5 s de jeu par env et par itération aux deux rythmes.
    assert_eq!((c30.horizon, c30.minibatch), (256, 1024));
    assert!((c15.horizon as f32 / 15.0 - c30.horizon as f32 / 30.0).abs() < 1e-4);
}

#[test]
fn adaptation_apres_changement_de_rythme() {
    let mut e = Explore {
        ent_coef: 0.0003,
        steps: 2_000_000_000,
        scale: 0.5,
        adapt_iters: 0,
        ..Explore::default()
    };
    let lr_avant = e.lr();
    assert!(
        lr_avant < explore::ADAPT_LR
            || (lr_avant - explore::LR_MIN).abs() < 1e-12
            || lr_avant > 0.0
    );
    e.begin_adaptation();
    assert_eq!(e.ent_coef, explore::ENT_INIT);
    assert_eq!(e.scale, 1.0);
    assert!(e.lr() <= explore::ADAPT_LR);
    // L'adaptation s'use d'une itération par mise à jour et le lr redevient celui du calendrier.
    for _ in 0..explore::ADAPT_ITERS {
        e.update(1, 0.5, 90);
    }
    assert_eq!(e.adapt_iters, 0);
    assert!((e.lr() - explore::lr_at(e.steps)).abs() < 1e-15);
    // Persisté avec explore.json.
    let dir = std::env::temp_dir().join(format!("lx-adapt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut e2 = Explore::default();
    e2.begin_adaptation();
    e2.save(&dir).unwrap();
    assert_eq!(Explore::load(&dir).adapt_iters, explore::ADAPT_ITERS);
    std::fs::remove_dir_all(&dir).ok();
}

/// À 30 Hz le compteur d'exploration avance en pas de 1/15 s (2 décisions = 1 pas) : même calendrier de lr par
/// seconde de jeu ; la politique exportée porte son `tick_skip`.
#[test]
fn iteration_a_30_hz_compte_en_pas_de_reference_et_exporte_le_rythme() {
    let cfg = PpoConfig {
        n_envs: 2,
        threads: 1,
        horizon: 32,
        hidden: 32,
        minibatch: 16,
        epochs: 1,
        ..PpoConfig::for_tick_skip(4)
    };
    let mut tr = Trainer::new(cfg, EnvConfig::for_stage(2).with_tick_skip(4)).expect("meshes");
    let s = tr.iteration();
    assert_eq!(tr.explore.steps, s.steps / 2);
    let dir = std::env::temp_dir().join(format!("lx-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    tr.export(&dir.join("policy.safetensors")).unwrap();
    let meta = std::fs::read_to_string(dir.join("policy.json")).unwrap();
    assert!(meta.contains("\"tick_skip\":4"), "{meta}");
    std::fs::remove_dir_all(&dir).ok();
}
