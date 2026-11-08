//! Learner PPO (burn, backend CPU `Flex`) pour l'env `rl-env` : politique et critique MLP séparées,
//! actions discrètes, GAE, objectif clippé. Spike M2 : voir `docs/decisions.md` (D1) et `docs/bench.md`.

pub mod metrics;
pub mod ppo;
