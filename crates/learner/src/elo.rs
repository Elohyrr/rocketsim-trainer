//! Elo de la politique contre 3 références fixes : le fonceur scripté (coups d'envoi, ancre à `CHARGER_ELO`), la
//! politique figée du palier 5 (`refs/stage5.safetensors`) et le meilleur ancien (`refs/best.safetensors`, la
//! politique au dernier record d'Elo). 100 matchs gloutons par référence, mêmes graines à chaque mesure.
//!
//! Elo contre une référence = `note_ref + 400·log10(p / (1 − p))` (`p` = victoires + ½ nuls, bornée à
//! [0,02 ; 0,98]) ; l'Elo publié est leur moyenne pondérée (poids `0,05 + 4·p·(1−p)` : un match équilibré informe
//! plus qu'un 98 %–2 %). La note du palier 5 est mesurée une fois contre le fonceur ; celle du meilleur ancien
//! est l'Elo de la politique au moment du record. Voir `docs/self-play.md`.

use std::path::Path;

use burn::tensor::activation::log_softmax;
use burn::tensor::{Device, Tensor, TensorData};
use rl_env::{EnvConfig, KickoffResult, Outcome, VecEnv};

use crate::ppo::Mlp;

/// Palier du self-play (jugé sur l'Elo).
pub const SELFPLAY_STAGE: u32 = 6;
/// Note du fonceur scripté (ancre de l'échelle).
pub const CHARGER_ELO: f64 = 1000.0;
/// Matchs par référence.
pub const MATCHES: usize = 100;
/// Envs parallèles d'une mesure.
const ENVS: usize = 16;
/// Lissage de l'Elo suivi par le curriculum (poids de la nouvelle mesure).
const EMA: f64 = 0.5;
/// Chute (Elo lissé sous le record) qui compte comme une régression, et nombre de mesures de suite requis.
pub const DROP: f64 = 150.0;
pub const LOW_STREAK: u32 = 2;
/// Amélioration minimale du record pour remplacer le « meilleur ancien ».
pub const BEST_MARGIN: f64 = 25.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Score {
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
}

impl Score {
    pub fn total(&self) -> u32 {
        self.wins + self.losses + self.draws
    }
    pub fn rate(&self) -> f64 {
        if self.total() == 0 {
            return 0.5;
        }
        (f64::from(self.wins) + 0.5 * f64::from(self.draws)) / f64::from(self.total())
    }
}

/// Écart d'Elo correspondant à un taux de victoire `p`.
pub fn elo_gap(p: f64) -> f64 {
    let p = p.clamp(0.02, 0.98);
    400.0 * (p / (1.0 - p)).log10()
}

pub enum Opp<'a> {
    /// Fonceur scripté, sur des coups d'envoi.
    Charger,
    Policy(&'a Mlp),
}

fn argmax(row: &[f32]) -> usize {
    row.iter()
        .enumerate()
        .fold(
            (0, f32::NEG_INFINITY),
            |b, (i, &v)| if v > b.1 { (i, v) } else { b },
        )
        .0
}

fn greedy(net: &Mlp, obs: &[f32], n: usize, od: usize, dev: &Device) -> Vec<usize> {
    let x: Tensor<2> = Tensor::from_data(TensorData::new(obs.to_vec(), [n, od]), dev);
    let lp = log_softmax(net.forward(x), 1)
        .into_data()
        .try_to_vec::<f32>()
        .expect("f32");
    let na = lp.len() / n;
    (0..n).map(|e| argmax(&lp[e * na..(e + 1) * na])).collect()
}

/// Joue `n` matchs gloutons de `agent` (voiture bleue) contre `opp`. `agent` et `opp` sur le même `dev`.
pub fn play(
    agent: &Mlp,
    opp: &Opp,
    n: usize,
    dev: &Device,
    tick_skip: u32,
) -> Result<Score, rl_env::SimError> {
    let cfg = match opp {
        Opp::Charger => EnvConfig::kickoff_eval(false),
        Opp::Policy(_) => EnvConfig::for_stage(6),
    }
    .with_tick_skip(tick_skip);
    let mut env = VecEnv::new(ENVS, 2, cfg, 4242)?;
    let od = env.obs_dim();
    let mut obs = env.reset();
    let mut s = Score::default();
    let mut guard = 0usize;
    while (s.total() as usize) < n
        && guard < 20_000 * rl_env::REF_TICK_SKIP as usize / tick_skip.max(1) as usize
    {
        guard += 1;
        let acts = greedy(agent, &obs, ENVS, od, dev);
        let opp_acts = match opp {
            Opp::Charger => None,
            Opp::Policy(p) => Some(greedy(p, env.opp_obs(), ENVS, od, dev)),
        };
        let step = env.step_vs(&acts, opp_acts.as_deref());
        for ep in &step.finished {
            if s.total() as usize >= n {
                break;
            }
            let res = match &ep.kickoff {
                Some(k) => match k.result {
                    KickoffResult::Won => 1,
                    KickoffResult::Lost => -1,
                    KickoffResult::Neutral => 0,
                },
                None => match ep.outcome {
                    Outcome::Goal => 1,
                    Outcome::OwnGoal => -1,
                    _ => 0,
                },
            };
            match res {
                1 => s.wins += 1,
                -1 => s.losses += 1,
                _ => s.draws += 1,
            }
        }
        obs = step.obs;
    }
    Ok(s)
}

pub const REFS: [&str; 3] = ["charger", "stage5", "best"];

/// Mesure d'un Elo : par référence (taux de victoire, Elo estimé ; `None` si la référence manque) et moyenne.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub elo: f64,
    pub per_ref: [Option<(f64, f64)>; 3],
    pub matches: u32,
}

/// Combine des estimations `(p, elo_estimé)` en un Elo.
pub fn combine(parts: &[(f64, f64)]) -> f64 {
    let (mut num, mut den) = (0.0, 0.0);
    for &(p, e) in parts {
        let w = 0.05 + 4.0 * p * (1.0 - p);
        num += w * e;
        den += w;
    }
    if den == 0.0 { CHARGER_ELO } else { num / den }
}

/// État d'Elo persisté (`<out>/elo.json`) et décisions de maintien du palier 6.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EloState {
    /// Elo lissé (`None` tant qu'aucune mesure), record du Elo lissé, mesures consécutives sous le record − `DROP`.
    pub ema: Option<f64>,
    pub best: f64,
    pub low: u32,
    /// Notes des références figées (`None` : pas encore capturée).
    pub rating_stage5: Option<f64>,
    pub rating_best: Option<f64>,
}

impl EloState {
    /// Enregistre une mesure ; renvoie vrai si c'est un nouveau record (au-delà de `BEST_MARGIN`).
    pub fn observe(&mut self, elo: f64) -> bool {
        let ema = self.ema.map_or(elo, |p| (1.0 - EMA) * p + EMA * elo);
        let first = self.ema.is_none();
        self.ema = Some(ema);
        if first || ema > self.best + BEST_MARGIN {
            self.best = if first { ema } else { ema.max(self.best) };
            self.low = 0;
            return true;
        }
        if ema < self.best - DROP {
            self.low += 1;
        } else {
            self.low = 0;
        }
        false
    }

    /// Le niveau a chuté de façon répétée (hystérésis : `LOW_STREAK` mesures de suite).
    pub fn regressed(&self) -> bool {
        self.low >= LOW_STREAK
    }

    /// Pas d'effondrement connu (autorise la promotion au palier 6).
    pub fn healthy(&self) -> bool {
        self.ema.is_none_or(|e| e >= self.best - DROP)
    }

    /// Après une régression : on repart d'un compteur neuf (le record reste).
    pub fn reset_streak(&mut self) {
        self.low = 0;
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let v = serde_json::json!({
            "ema": self.ema, "best": self.best, "low": self.low,
            "rating_stage5": self.rating_stage5, "rating_best": self.rating_best,
        });
        std::fs::write(dir.join("elo.json"), format!("{v}\n"))
    }

    pub fn load(dir: &Path) -> Self {
        let Some(v) = std::fs::read_to_string(dir.join("elo.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        else {
            return Self::default();
        };
        let f = |k: &str| v[k].as_f64().filter(|x| x.is_finite());
        Self {
            ema: f("ema"),
            best: f("best").unwrap_or(0.0),
            low: v["low"].as_u64().unwrap_or(0) as u32,
            rating_stage5: f("rating_stage5"),
            rating_best: f("rating_best"),
        }
    }
}

/// Dossier des références figées.
pub fn refs_dir(out: &Path) -> std::path::PathBuf {
    out.join("refs")
}

/// Mesure l'Elo de `agent`. Capture au passage la référence du palier 5 si elle manque (politique courante :
/// à défaut d'avoir été figée à la promotion, on fige ce qu'on a), puis met à jour le record et le « meilleur
/// ancien ». `template` : réseau de mêmes dimensions (lecture des références).
pub fn measure(
    agent: &Mlp,
    st: &mut EloState,
    out: &Path,
    dev: &Device,
    tick_skip: u32,
) -> Result<Report, String> {
    let dir = refs_dir(out);
    let sim = |e: rl_env::SimError| e.to_string();
    let mut rep = Report::default();
    let mut parts = Vec::new();

    let c = play(agent, &Opp::Charger, MATCHES, dev, tick_skip).map_err(sim)?;
    let p = c.rate();
    let e = CHARGER_ELO + elo_gap(p);
    rep.per_ref[0] = Some((p, e));
    parts.push((p, e));
    rep.matches += c.total();

    let p5 = dir.join("stage5.safetensors");
    if !p5.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::pool::Pool::write_net(agent, &p5).map_err(|e| e.to_string())?;
        st.rating_stage5 = None;
    }
    let pb = dir.join("best.safetensors");
    for (i, (path, rating)) in [(&p5, st.rating_stage5), (&pb, st.rating_best)]
        .into_iter()
        .enumerate()
    {
        if !path.exists() {
            continue;
        }
        let net = crate::pool::Pool::read_net(path, agent, dev)?;
        let rating = match (rating, i) {
            (Some(r), _) => r,
            // Note du palier 5 : son score contre le fonceur, mesuré une seule fois.
            (None, 0) => {
                let s = play(&net, &Opp::Charger, MATCHES, dev, tick_skip).map_err(sim)?;
                let r = CHARGER_ELO + elo_gap(s.rate());
                st.rating_stage5 = Some(r);
                r
            }
            (None, _) => continue,
        };
        let s = play(agent, &Opp::Policy(&net), MATCHES, dev, tick_skip).map_err(sim)?;
        let (p, e) = (s.rate(), rating + elo_gap(s.rate()));
        rep.per_ref[i + 1] = Some((p, e));
        parts.push((p, e));
        rep.matches += s.total();
    }
    rep.elo = combine(&parts);
    if st.observe(rep.elo) {
        // Nouveau record : la politique courante devient le « meilleur ancien » (note = son Elo).
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::pool::Pool::write_net(agent, &pb).map_err(|e| e.to_string())?;
        st.rating_best = st.ema;
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_are_played_and_counted_against_both_kinds_of_opponent() {
        let dev = Device::flex();
        let od = rl_env::OBS_DIM;
        let net = Mlp::new(od, 16, rl_env::ActionTable::rlgym().len(), &dev);
        let c = play(&net, &Opp::Charger, 5, &dev, 8).expect("meshes (ROCKETSIM_MESHES_DIR)");
        assert_eq!(c.total(), 5);
        let p = play(&net, &Opp::Policy(&net), 3, &dev, 8).expect("meshes");
        assert_eq!(p.total(), 3);
    }

    #[test]
    fn gap_is_antisymmetric_and_clamped() {
        assert!(elo_gap(0.5).abs() < 1e-9);
        assert!((elo_gap(0.7) + elo_gap(0.3)).abs() < 1e-9);
        assert!((elo_gap(0.76) - 200.0).abs() < 5.0, "76 % ≈ +200 Elo");
        assert_eq!(elo_gap(1.0), elo_gap(0.98));
        assert!(elo_gap(0.0).is_finite());
    }

    #[test]
    fn score_counts_draws_as_half() {
        let s = Score {
            wins: 2,
            losses: 1,
            draws: 2,
        };
        assert!((s.rate() - 0.6).abs() < 1e-12);
        assert_eq!(Score::default().rate(), 0.5);
    }

    #[test]
    fn combine_weights_balanced_matches_more() {
        let e = combine(&[(0.5, 1000.0), (0.98, 1600.0)]);
        assert!(e > 1000.0 && e < 1300.0, "{e}");
        assert_eq!(combine(&[]), CHARGER_ELO);
    }

    #[test]
    fn hysteresis_needs_repeated_drops_and_noise_does_not_trigger() {
        let mut s = EloState::default();
        assert!(s.observe(1000.0), "première mesure = record");
        assert!(s.healthy() && !s.regressed());
        // Bruit : ± 60 autour du record, jamais de régression.
        for e in [940.0, 1050.0, 960.0, 1020.0, 950.0] {
            s.observe(e);
            assert!(!s.regressed(), "{e}");
        }
        // Chute durable : le lissé passe sous record − 150 puis y reste.
        let mut flagged = None;
        for k in 0..6 {
            s.observe(700.0);
            if s.regressed() && flagged.is_none() {
                flagged = Some(k);
            }
        }
        assert!(
            flagged.is_some_and(|k| k >= 1),
            "une seule mesure ne suffit pas : {flagged:?}"
        );
        assert!(!s.healthy());
        s.reset_streak();
        assert!(!s.regressed());
    }

    #[test]
    fn new_record_resets_the_streak_and_state_round_trips() {
        let mut s = EloState::default();
        s.observe(1000.0);
        s.observe(600.0);
        s.observe(600.0);
        assert!(s.low >= 1);
        s.rating_stage5 = Some(950.0);
        let d = std::env::temp_dir().join(format!("elo-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        s.save(&d).unwrap();
        assert_eq!(EloState::load(&d), s);
        assert_eq!(EloState::load(&d.join("rien")), EloState::default());
        std::fs::remove_dir_all(&d).ok();
        let mut r = EloState::default();
        r.observe(1000.0);
        r.observe(500.0);
        assert!(r.observe(5000.0), "record battu");
        assert_eq!(r.low, 0);
    }
}
