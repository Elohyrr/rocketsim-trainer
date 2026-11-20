//! Évaluation reproductible des coups d'envoi : `learner --eval-kickoff N --policy policy.safetensors [--idle]`.
//!
//! Politique gloutonne (celle du jeu), `N` kickoffs par position officielle, mêmes graines à chaque lancement
//! (bruit de position et départ de l'adversaire identiques) : deux politiques se comparent sur les mêmes essais.
//! L'adversaire est le fonceur scripté (cap vers la balle, gaz et boost) ; `--idle` le laisse immobile.

use std::path::Path;

use policy_runtime::Policy;
use rl_env::{Env, EnvConfig, KickoffResult, KickoffStats, OBS_DIM};

/// Résultat agrégé d'une évaluation.
#[derive(Clone, Debug, Default)]
pub struct KickoffEval {
    pub won: u32,
    pub lost: u32,
    pub neutral: u32,
    /// Kickoffs où l'agent a touché la balle, et parmi eux ceux où il a touché le premier.
    pub touched: u32,
    pub first: u32,
    pub touch_s_sum: f64,
    pub speed_sum: f64,
}

impl KickoffEval {
    pub fn total(&self) -> u32 {
        self.won + self.lost + self.neutral
    }
    pub fn win_rate(&self) -> f64 {
        f64::from(self.won) / f64::from(self.total().max(1))
    }
    pub fn mean_touch_s(&self) -> f64 {
        self.touch_s_sum / f64::from(self.touched.max(1))
    }
    pub fn mean_speed(&self) -> f64 {
        self.speed_sum / f64::from(self.touched.max(1))
    }
    fn add(&mut self, k: &KickoffStats) {
        match k.result {
            KickoffResult::Won => self.won += 1,
            KickoffResult::Lost => self.lost += 1,
            KickoffResult::Neutral => self.neutral += 1,
        }
        if let (Some(t), Some(v)) = (k.first_touch_s, k.touch_speed) {
            self.touched += 1;
            self.touch_s_sum += f64::from(t);
            self.speed_sum += f64::from(v);
        }
        self.first += u32::from(k.first);
    }
}

/// Joue `n` kickoffs par position (5 positions) avec la politique `act` (observation → action).
/// Renvoie le total et le détail par position.
pub fn eval_kickoffs(
    act: &dyn Fn(&[f32]) -> usize,
    n: u32,
    idle: bool,
    tick_skip: u32,
) -> Result<(KickoffEval, [KickoffEval; 5]), rl_env::SimError> {
    rl_env::init_meshes_from_env()?;
    let mut total = KickoffEval::default();
    let mut per: [KickoffEval; 5] = Default::default();
    let cfg = EnvConfig::kickoff_eval(idle).with_tick_skip(tick_skip);
    for (spawn, slot) in per.iter_mut().enumerate() {
        // Une seule graine par position : les n essais enchaînent le même flux aléatoire d'un lancement à l'autre.
        let mut env = Env::new(cfg.clone(), 1000 + spawn as u64)?;
        for _ in 0..n {
            let mut obs: [f32; OBS_DIM] = env.reset_kickoff(Some(spawn));
            loop {
                let r = env.step_vs(act(&obs), None);
                if let Some(k) = r.kickoff {
                    slot.add(&k);
                    total.add(&k);
                    break;
                }
                if r.terminated || r.truncated {
                    break;
                }
                obs = env.observe();
            }
        }
    }
    Ok((total, per))
}

fn line(name: &str, e: &KickoffEval) -> String {
    format!(
        "{name:<8} n={:<4} gagnes={:>5.1}% perdus={:>5.1}% neutres={:>5.1}% premier_toucher={:>5.1}% t_toucher={:.2}s vitesse_contact={:.0}",
        e.total(),
        100.0 * e.win_rate(),
        100.0 * f64::from(e.lost) / f64::from(e.total().max(1)),
        100.0 * f64::from(e.neutral) / f64::from(e.total().max(1)),
        100.0 * f64::from(e.first) / f64::from(e.total().max(1)),
        e.mean_touch_s(),
        e.mean_speed()
    )
}

/// Évalue le fichier de politique `path` et affiche le résultat.
pub fn run(path: &Path, n: u32, idle: bool, tick_skip: u32) -> Result<(), String> {
    let pol = Policy::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dim = pol.obs_dim();
    let act = move |o: &[f32]| pol.act(&o[..dim.min(o.len())]);
    let (total, per) = eval_kickoffs(&act, n, idle, tick_skip).map_err(|e| e.to_string())?;
    for (i, e) in per.iter().enumerate() {
        println!("{}", line(&format!("pos {i}"), e));
    }
    println!("{}", line("TOTAL", &total));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_is_reproducible_and_a_passive_policy_loses_every_kickoff() {
        // Action 4 : gaz nul, sans boost (table `ActionTable::rlgym`).
        let idle = |_: &[f32]| 4usize;
        let (a, per) = eval_kickoffs(&idle, 2, false, 8).expect("meshes (ROCKETSIM_MESHES_DIR)");
        let (b, _) = eval_kickoffs(&idle, 2, false, 8).expect("meshes");
        assert_eq!(a.total(), 10);
        assert_eq!(per.iter().map(KickoffEval::total).sum::<u32>(), 10);
        assert_eq!((a.won, a.lost, a.neutral), (b.won, b.lost, b.neutral));
        assert_eq!(a.won, 0, "ne rien faire ne gagne jamais un kickoff");
        assert_eq!(a.touched, 0);
    }
}
