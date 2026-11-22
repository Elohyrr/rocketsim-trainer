//! Exploration : coefficient d'entropie adaptatif (cible en nat, proportionnelle à ln(nb d'actions)), plafond de
//! KL, lr décroissant, garde-fou sur le taux de but.
//!
//! Historique (voir `docs/exploration.md`) : avec `ent_coef` fixe à 0,01 et un lr de 1e-3, l'entropie de la prod
//! s'était effondrée à 0,07 nat. La première version (PR #15) visait 1,25 nat avec un `ent_coef` borné à 1,0 : le
//! coefficient a passé des heures au plafond (100 × la valeur de référence) le temps que l'entropie monte, puis
//! l'a maintenue à 1,25 nat avec un coefficient ≈ 0,05 : politique détruite (palier 5 → 2, taux de but 0). Ici :
//! - cible `ENT_TARGET_FRAC × ln(nb d'actions)` (≈ 0,52 nat pour 78 actions) × `scale` (garde-fou) ;
//! - `ent_coef ← ent_coef × exp(gain × (cible − H))`, gain plus fort à la baisse qu'à la hausse, borné à
//!   `[ENT_MIN, ENT_MAX × scale]` avec `ENT_MAX` = 0,02 : l'entropie n'est jamais achetée plus cher que 2 × la
//!   référence rlgym ; si la récompense la refuse, elle reste sous la cible ;
//! - garde-fou ([`Guard`]) : taux de but récent < la moitié de sa référence du palier → `scale` et `ent_coef`
//!   divisés par 2 ; `scale` remonte après `RECOVER_ITERS` itérations sans alerte ;
//! - les époques PPO s'arrêtent dès que la KL moyenne d'une époque dépasse `KL_MAX` ;
//! - `lr = max(LR_MIN, LR0 / (1 + pas/LR_SCALE))`.
//!
//! L'état (`ent_coef`, pas cumulés, `scale`) est persisté dans `<out>/explore.json`. À la relecture, `ent_coef` est
//! ramené dans les bornes courantes (un 0,049 laissé par la PR #15 devient 0,02).

use std::collections::VecDeque;
use std::path::Path;

/// Entropie visée, en fraction de l'entropie maximale ln(nb d'actions) : 0,12 × ln 78 ≈ 0,52 nat.
pub const ENT_TARGET_FRAC: f64 = 0.12;
/// Ancienne cible absolue (PR #15) : plus utilisée, gardée pour la documentation de l'incident.
pub const ENT_TARGET_PR15: f64 = 1.25;
pub const ENT_MIN: f64 = 1e-4;
/// Plafond bas (2 × le 0,01 fixe de rlgym-ppo/Necto) : pas d'emballement possible du coefficient.
pub const ENT_MAX: f64 = 0.02;
/// Gains de l'ajustement : lent à la hausse, rapide à la baisse (pas de dépassement prolongé de la cible).
pub const ENT_GAIN_UP: f64 = 0.03;
pub const ENT_GAIN_DOWN: f64 = 0.1;
/// KL moyenne par époque au-delà de laquelle les époques suivantes sont sautées.
pub const KL_MAX: f64 = 0.02;
pub const LR0: f64 = 3e-4;
pub const LR_MIN: f64 = 5e-5;
/// Pas cumulés pour diviser le lr par 2 (≈ 1 jour de prod à 420 M pas/jour).
pub const LR_SCALE: f64 = 5e8;
/// Changement de rythme de décision (15 → 30 Hz ou retour) : pendant `ADAPT_ITERS` itérations le lr est plafonné à
/// `ADAPT_LR` (les actions sont tenues 2 fois moins longtemps, la politique reprise doit s'y adapter sans dérive).
pub const ADAPT_ITERS: u64 = 500;
pub const ADAPT_LR: f64 = 1e-4;
/// Coefficient d'entropie initial (celui de la prod avant la PR #15).
pub const ENT_INIT: f64 = 0.01;
/// Plus petite valeur de `scale` (cible et plafond réduits au quart).
pub const SCALE_MIN: f64 = 0.25;

/// Entropie visée pour `n_actions` actions discrètes, avant `scale`.
pub fn ent_target(n_actions: usize) -> f64 {
    ENT_TARGET_FRAC * (n_actions.max(2) as f64).ln()
}

/// Nouvelle valeur du coefficient d'entropie après une itération d'entropie mesurée `h`, plafond `max`.
pub fn next_ent_coef(coef: f64, h: f64, target: f64, max: f64) -> f64 {
    next_ent_coef_with(&ExploreConfig::default(), coef, h, target, max)
}

/// Comme [`next_ent_coef`] avec les gains et le plancher de `cfg`.
pub fn next_ent_coef_with(cfg: &ExploreConfig, coef: f64, h: f64, target: f64, max: f64) -> f64 {
    let gain = if h > target {
        cfg.ent_gain_down
    } else {
        cfg.ent_gain_up
    };
    (coef * (gain * (target - h)).exp()).clamp(cfg.ent_min, max.max(cfg.ent_min))
}

/// lr après `steps` pas cumulés (réglages par défaut).
pub fn lr_at(steps: u64) -> f64 {
    lr_at_with(&ExploreConfig::default(), steps)
}

/// lr après `steps` pas cumulés avec `lr0` et `lr_min` de `cfg`.
pub fn lr_at_with(cfg: &ExploreConfig, steps: u64) -> f64 {
    (cfg.lr0 / (1.0 + steps as f64 / LR_SCALE)).max(cfg.lr_min)
}

/// Réglages de l'exploration et de l'optimisation PPO (flags `--lr0`, `--lr-min`, `--kl-max`, `--ent-min`,
/// `--ent-gain-up`, `--ent-gain-down`, `--ent-ema` du learner) ; les défauts reprennent les constantes ci-dessus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExploreConfig {
    pub lr0: f64,
    pub lr_min: f64,
    pub kl_max: f64,
    pub ent_min: f64,
    pub ent_gain_up: f64,
    pub ent_gain_down: f64,
    /// 0 = le régulateur reçoit l'entropie brute ; N > 0 = moyenne exponentielle sur N itérations.
    pub ent_ema_iters: u32,
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self {
            lr0: LR0,
            lr_min: LR_MIN,
            kl_max: KL_MAX,
            ent_min: ENT_MIN,
            ent_gain_up: ENT_GAIN_UP,
            ent_gain_down: ENT_GAIN_DOWN,
            ent_ema_iters: 0,
        }
    }
}

/// État persistant du régulateur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Explore {
    pub ent_coef: f64,
    pub steps: u64,
    /// Réduction de la cible et du plafond par le garde-fou, dans `[SCALE_MIN, 1]`.
    pub scale: f64,
    /// Itérations restantes d'adaptation après un changement de rythme (lr plafonné) ; 0 = hors adaptation.
    pub adapt_iters: u64,
    /// Moyenne exponentielle de l'entropie (`None` tant qu'aucune itération ; inutilisée si `ent_ema_iters` = 0).
    pub h_ema: Option<f64>,
    /// Réglages (non persistés : donnés par les flags à chaque lancement).
    pub cfg: ExploreConfig,
}

impl Default for Explore {
    fn default() -> Self {
        Self {
            ent_coef: ENT_INIT,
            steps: 0,
            scale: 1.0,
            adapt_iters: 0,
            h_ema: None,
            cfg: ExploreConfig::default(),
        }
    }
}

impl Explore {
    pub fn with_config(cfg: ExploreConfig) -> Self {
        Self {
            cfg,
            ..Self::default()
        }
    }

    pub fn lr(&self) -> f64 {
        let lr = lr_at_with(&self.cfg, self.steps);
        if self.adapt_iters > 0 {
            lr.min(ADAPT_LR)
        } else {
            lr
        }
    }

    /// Bascule de rythme de décision : coefficient d'entropie remis à sa valeur de départ (l'entropie par décision
    /// change : une action tenue 2 fois moins longtemps), garde-fou à pleine échelle, lr plafonné `ADAPT_ITERS`
    /// itérations. `steps` (en pas de 1/15 s, voir [`Trainer::iteration`](crate::ppo::Trainer)) reste valable.
    pub fn begin_adaptation(&mut self) {
        self.ent_coef = ENT_INIT.clamp(self.cfg.ent_min, ENT_MAX);
        self.scale = 1.0;
        self.h_ema = None;
        self.adapt_iters = ADAPT_ITERS;
    }

    /// Plafond courant du coefficient.
    pub fn coef_max(&self) -> f64 {
        ENT_MAX * self.scale
    }

    /// Entropie visée courante pour `n_actions` actions.
    pub fn target(&self, n_actions: usize) -> f64 {
        ent_target(n_actions) * self.scale
    }

    /// Fin d'itération : `steps` pas de plus, entropie mesurée `h` sur `n_actions` actions.
    pub fn update(&mut self, steps: u64, h: f64, n_actions: usize) {
        self.steps += steps;
        self.adapt_iters = self.adapt_iters.saturating_sub(1);
        if h.is_finite() {
            let h = self.smooth(h);
            self.ent_coef = next_ent_coef_with(
                &self.cfg,
                self.ent_coef,
                h,
                self.target(n_actions),
                self.coef_max(),
            );
        }
    }

    /// Entropie vue par le régulateur après l'itération de mesure brute `h` (met à jour la moyenne exponentielle).
    fn smooth(&mut self, h: f64) -> f64 {
        if self.cfg.ent_ema_iters == 0 {
            return h;
        }
        let a = 1.0 / f64::from(self.cfg.ent_ema_iters);
        let m = match self.h_ema.filter(|m| m.is_finite()) {
            Some(prev) => prev + a * (h - prev),
            None => h,
        };
        self.h_ema = Some(m);
        m
    }

    /// Entropie lissée courante, `None` si le lissage est coupé ou sans mesure.
    pub fn entropy_smoothed(&self) -> Option<f64> {
        (self.cfg.ent_ema_iters > 0).then_some(self.h_ema).flatten()
    }

    /// Garde-fou déclenché : cible, plafond et coefficient divisés par 2 (`scale` ≥ `SCALE_MIN`).
    pub fn back_off(&mut self) {
        self.scale = (self.scale * 0.5).max(SCALE_MIN);
        self.ent_coef = (self.ent_coef * 0.5).clamp(self.cfg.ent_min, self.coef_max());
    }

    /// Longue période sans alerte : `scale` remonte d'un cran (× 2, ≤ 1).
    pub fn recover(&mut self) {
        self.scale = (self.scale * 2.0).min(1.0);
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let ema = self
            .h_ema
            .filter(|m| m.is_finite())
            .map_or(String::new(), |m| format!(",\"ent_ema\":{m}"));
        let s = format!(
            "{{\"ent_coef\":{},\"steps\":{},\"scale\":{},\"adapt_iters\":{}{ema}}}\n",
            self.ent_coef, self.steps, self.scale, self.adapt_iters
        );
        std::fs::write(dir.join("explore.json"), s)
    }

    /// Lit `<dir>/explore.json` ; valeurs initiales si absent ou illisible. `ent_coef` est ramené dans les bornes
    /// courantes (fichier écrit par une version au plafond plus haut).
    pub fn load(dir: &Path) -> Self {
        Self::load_with(dir, ExploreConfig::default())
    }

    /// Comme [`Explore::load`] avec les réglages `cfg` (le plancher `ent_min` configuré borne `ent_coef`). Champs
    /// absents (fichier d'une version antérieure, dont `ent_ema`) = valeurs par défaut.
    pub fn load_with(dir: &Path, cfg: ExploreConfig) -> Self {
        let parsed = std::fs::read_to_string(dir.join("explore.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let Some(v) = parsed else {
            return Self::with_config(cfg);
        };
        let scale = v["scale"]
            .as_f64()
            .filter(|c| c.is_finite())
            .map_or(1.0, |c| c.clamp(SCALE_MIN, 1.0));
        let mut e = Self {
            ent_coef: ENT_INIT,
            steps: v["steps"].as_u64().unwrap_or(0),
            scale,
            adapt_iters: v["adapt_iters"].as_u64().unwrap_or(0),
            h_ema: v["ent_ema"].as_f64().filter(|m| m.is_finite()),
            cfg,
        };
        e.ent_coef = v["ent_coef"]
            .as_f64()
            .filter(|c| c.is_finite())
            .unwrap_or(ENT_INIT)
            .clamp(cfg.ent_min, e.coef_max().max(cfg.ent_min));
        e
    }
}

/// Itérations dans la fenêtre courte du garde-fou.
pub const GUARD_WINDOW: usize = 100;
/// Épisodes minimum dans la fenêtre courte pour juger.
pub const GUARD_MIN_EPISODES: u64 = 100;
/// Itérations au palier avant que la référence soit jugée fiable.
pub const GUARD_WARMUP: usize = 300;
/// Horizon (itérations) de la moyenne exponentielle de référence.
pub const GUARD_REF_ITERS: f64 = 1000.0;
/// Alerte quand le taux récent tombe sous cette fraction de la référence…
pub const GUARD_DROP: f64 = 0.5;
/// … et seulement si la référence est significative.
pub const GUARD_MIN_REF: f64 = 0.2;
/// Itérations entre deux alertes.
pub const GUARD_COOLDOWN: usize = 300;
/// Itérations sans alerte avant de remonter `scale`.
pub const RECOVER_ITERS: usize = 5000;

/// Action demandée par le garde-fou.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuardEvent {
    /// Taux de but récent effondré : `Explore::back_off`.
    BackOff { recent: f64, reference: f64 },
    /// Longue période saine : `Explore::recover`.
    Recover,
}

/// Garde-fou du taux de but, par palier (à remettre à zéro à chaque changement de palier). Référence : moyenne
/// exponentielle des buts et des épisodes (≈ `GUARD_REF_ITERS` itérations), gelée pendant une alerte.
#[derive(Clone, Debug, Default)]
pub struct Guard {
    recent: VecDeque<(u64, u64)>,
    ref_goals: f64,
    ref_eps: f64,
    iters: usize,
    cooldown: usize,
    calm: usize,
}

impl Guard {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Taux de but sur la fenêtre courte, `None` si trop peu d'épisodes.
    pub fn recent_rate(&self) -> Option<f64> {
        let (g, e) = self
            .recent
            .iter()
            .fold((0, 0), |a, r| (a.0 + r.0, a.1 + r.1));
        (self.recent.len() == GUARD_WINDOW && e >= GUARD_MIN_EPISODES).then(|| g as f64 / e as f64)
    }

    /// Taux de but de référence du palier (`None` tant qu'il n'est pas établi).
    pub fn reference(&self) -> Option<f64> {
        (self.iters >= GUARD_WARMUP && self.ref_eps > 0.0).then(|| self.ref_goals / self.ref_eps)
    }

    /// Itération terminée : `goals` buts sur `episodes` épisodes ; `scaled` = `scale` < 1 (remontée possible).
    pub fn observe(&mut self, goals: u64, episodes: u64, scaled: bool) -> Option<GuardEvent> {
        self.recent.push_back((goals, episodes));
        if self.recent.len() > GUARD_WINDOW {
            self.recent.pop_front();
        }
        self.iters += 1;
        self.cooldown = self.cooldown.saturating_sub(1);
        let recent = self.recent_rate();
        let alarm = match (recent, self.reference()) {
            (Some(r), Some(rf)) => rf >= GUARD_MIN_REF && r < GUARD_DROP * rf,
            _ => false,
        };
        if !alarm {
            let a = 1.0 / GUARD_REF_ITERS;
            if self.iters == 1 {
                (self.ref_goals, self.ref_eps) = (goals as f64, episodes as f64);
            } else {
                self.ref_goals += a * (goals as f64 - self.ref_goals);
                self.ref_eps += a * (episodes as f64 - self.ref_eps);
            }
            self.calm += 1;
            if scaled && self.calm >= RECOVER_ITERS {
                self.calm = 0;
                return Some(GuardEvent::Recover);
            }
            return None;
        }
        self.calm = 0;
        if self.cooldown > 0 {
            return None;
        }
        self.cooldown = GUARD_COOLDOWN;
        Some(GuardEvent::BackOff {
            recent: recent.unwrap_or(0.0),
            reference: self.reference().unwrap_or(0.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 0.5;

    #[test]
    fn target_is_a_fraction_of_the_maximum_entropy() {
        assert!((ent_target(78) - 0.12 * 78f64.ln()).abs() < 1e-12);
        assert!(ent_target(78) > 0.5 && ent_target(78) < 0.55);
        assert!(ent_target(90) > ent_target(78));
        let e = Explore {
            scale: 0.5,
            ..Explore::default()
        };
        assert!((e.target(78) - ent_target(78) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn coef_rises_when_entropy_is_too_low_and_falls_faster_when_too_high() {
        assert!(next_ent_coef(0.01, 0.07, T, ENT_MAX) > 0.01);
        assert!(next_ent_coef(0.01, 1.25, T, ENT_MAX) < 0.01);
        assert!((next_ent_coef(0.01, T, T, ENT_MAX) - 0.01).abs() < 1e-12);
        let up = next_ent_coef(0.01, T - 0.3, T, ENT_MAX) / 0.01;
        let down = 0.01 / next_ent_coef(0.01, T + 0.3, T, ENT_MAX);
        assert!(down > up, "baisse plus rapide que la hausse");
    }

    #[test]
    fn coef_stays_in_bounds_and_cannot_wind_up() {
        let mut c = ENT_INIT;
        for _ in 0..100_000 {
            c = next_ent_coef(c, 0.0, T, ENT_MAX);
        }
        assert!((c - ENT_MAX).abs() < 1e-12, "plafond {ENT_MAX}, pas 1,0");
        // Entropie remontée au-dessus de la cible (+0,3 nat) : retour sous 0,01 en moins de 30 itérations.
        let mut n = 0;
        while c > 0.01 {
            c = next_ent_coef(c, T + 0.3, T, ENT_MAX);
            n += 1;
        }
        assert!(n < 30, "{n} itérations");
        for _ in 0..100_000 {
            c = next_ent_coef(c, 4.0, T, ENT_MAX);
        }
        assert!((c - ENT_MIN).abs() < 1e-12);
    }

    #[test]
    fn lr_decays_with_a_floor() {
        assert!((lr_at(0) - LR0).abs() < 1e-15);
        assert!((lr_at(LR_SCALE as u64) - LR0 / 2.0).abs() < 1e-9);
        assert!(lr_at(1_000_000) > lr_at(100_000_000));
        assert!((lr_at(u64::MAX / 2) - LR_MIN).abs() < 1e-15);
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("explore-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn state_round_trips_and_missing_file_gives_defaults() {
        let dir = tmp("rt");
        assert_eq!(Explore::load(&dir), Explore::default());
        let e = Explore {
            ent_coef: 0.0037,
            steps: 123_456_789,
            scale: 0.5,
            adapt_iters: 17,
            ..Explore::default()
        };
        e.save(&dir).unwrap();
        assert_eq!(Explore::load(&dir), e);
        std::fs::write(dir.join("explore.json"), "pas du json").unwrap();
        assert_eq!(Explore::load(&dir), Explore::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_left_by_pr15_is_brought_under_the_new_cap() {
        let dir = tmp("pr15");
        // Contenu exact de /data/run/explore.json en prod le 18/11 (pas de champ `scale`).
        std::fs::write(
            dir.join("explore.json"),
            "{\"ent_coef\":0.04977765133842551,\"steps\":199659520}\n",
        )
        .unwrap();
        let e = Explore::load(&dir);
        assert!((e.ent_coef - ENT_MAX).abs() < 1e-12);
        assert_eq!(e.steps, 199_659_520);
        assert!((e.scale - 1.0).abs() < 1e-12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ent_ema_smooths_the_entropy_seen_by_the_regulator() {
        let cfg = ExploreConfig {
            ent_ema_iters: 10,
            ..ExploreConfig::default()
        };
        let mut e = Explore::with_config(cfg);
        assert_eq!(e.entropy_smoothed(), None);
        e.update(1, 1.0, 78);
        assert_eq!(e.entropy_smoothed(), Some(1.0));
        // Un pic isolé d'entropie est amorti à 1/10.
        e.update(1, 2.0, 78);
        assert!((e.entropy_smoothed().unwrap() - 1.1).abs() < 1e-12);
        // Sans lissage : H brut, aucune moyenne conservée.
        let mut raw = Explore::default();
        raw.update(1, 1.0, 78);
        raw.update(1, 2.0, 78);
        assert_eq!(raw.entropy_smoothed(), None);
        assert_eq!(raw.h_ema, None);
        // Le coefficient réagit moins à un pic avec le lissage qu'en brut.
        let (mut a, mut b) = (Explore::with_config(cfg), Explore::default());
        for h in [0.5, 0.5, 3.0] {
            a.update(1, h, 78);
            b.update(1, h, 78);
        }
        assert!(a.ent_coef > b.ent_coef, "{} {}", a.ent_coef, b.ent_coef);
        // La moyenne est persistée et relue.
        let dir = tmp("ema");
        a.save(&dir).unwrap();
        assert_eq!(Explore::load_with(&dir, cfg).h_ema, a.h_ema);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_explore_json_loads_with_defaults_and_configured_floor() {
        let dir = tmp("old");
        std::fs::write(
            dir.join("explore.json"),
            "{\"ent_coef\":0.0012,\"steps\":5,\"scale\":1.0,\"adapt_iters\":0}\n",
        )
        .unwrap();
        let e = Explore::load(&dir);
        assert!((e.ent_coef - 0.0012).abs() < 1e-12);
        assert_eq!((e.h_ema, e.cfg), (None, ExploreConfig::default()));
        let cfg = ExploreConfig {
            ent_min: 0.004,
            ..ExploreConfig::default()
        };
        let e = Explore::load_with(&dir, cfg);
        assert!((e.ent_coef - 0.004).abs() < 1e-12, "{}", e.ent_coef);
        assert_eq!(e.steps, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn configured_lr_and_gains_apply() {
        let cfg = ExploreConfig {
            lr0: 1e-3,
            lr_min: 2e-4,
            ent_gain_up: 0.3,
            ..ExploreConfig::default()
        };
        assert!((lr_at_with(&cfg, 0) - 1e-3).abs() < 1e-15);
        assert!((lr_at_with(&cfg, u64::MAX / 2) - 2e-4).abs() < 1e-15);
        assert!(
            next_ent_coef_with(&cfg, 0.01, 0.0, T, ENT_MAX) > next_ent_coef(0.01, 0.0, T, ENT_MAX)
        );
    }

    #[test]
    fn back_off_halves_target_and_cap_down_to_a_floor_then_recovers() {
        let mut e = Explore {
            ent_coef: ENT_MAX,
            ..Explore::default()
        };
        e.back_off();
        assert!((e.scale - 0.5).abs() < 1e-12);
        assert!((e.ent_coef - ENT_MAX / 2.0).abs() < 1e-12);
        for _ in 0..10 {
            e.back_off();
        }
        assert!((e.scale - SCALE_MIN).abs() < 1e-12);
        assert!(e.ent_coef <= ENT_MAX * SCALE_MIN);
        e.update(2048, 0.0, 78);
        assert!(e.ent_coef <= e.coef_max() + 1e-15);
        e.recover();
        e.recover();
        e.recover();
        assert!((e.scale - 1.0).abs() < 1e-12);
    }

    /// Alimente le garde-fou avec `n` itérations de 10 épisodes à `rate` buts par épisode.
    fn feed(g: &mut Guard, n: usize, rate: f64, scaled: bool) -> Vec<GuardEvent> {
        (0..n)
            .filter_map(|_| g.observe((rate * 10.0).round() as u64, 10, scaled))
            .collect()
    }

    #[test]
    fn guard_fires_on_a_collapse_but_not_on_noise_or_low_reference() {
        let mut g = Guard::default();
        assert!(feed(&mut g, GUARD_WARMUP, 0.7, false).is_empty());
        assert!((g.reference().unwrap() - 0.7).abs() < 1e-9);
        // Baisse modérée (0,7 → 0,5) : pas d'alerte.
        assert!(feed(&mut g, 300, 0.5, false).is_empty());
        // Effondrement : une alerte, puis délai entre deux alertes.
        let ev = feed(&mut g, GUARD_COOLDOWN, 0.0, false);
        assert_eq!(ev.len(), 1, "{ev:?}");
        let GuardEvent::BackOff { reference, .. } = ev[0] else {
            panic!("{ev:?}")
        };
        assert!(
            reference > 0.5,
            "référence gelée pendant l'alerte : {reference}"
        );
        assert_eq!(feed(&mut g, GUARD_COOLDOWN, 0.0, false).len(), 1);
        // Palier où l'on marque peu (référence < 0,2) : jamais d'alerte.
        let mut low = Guard::default();
        assert!(feed(&mut low, 2000, 0.1, false).is_empty());
        assert!(feed(&mut low, 2000, 0.0, false).is_empty());
        // Pas d'alerte avant que la référence soit établie.
        let mut fresh = Guard::default();
        assert!(feed(&mut fresh, GUARD_WARMUP - 1, 0.8, false).is_empty());
        fresh.reset();
        assert!(feed(&mut fresh, GUARD_WARMUP - 1, 0.0, false).is_empty());
    }

    #[test]
    fn guard_asks_to_recover_after_a_long_calm_only_when_scaled() {
        let mut g = Guard::default();
        assert!(feed(&mut g, 2 * RECOVER_ITERS, 0.6, false).is_empty());
        let ev = feed(&mut g, RECOVER_ITERS, 0.6, true);
        assert_eq!(ev, vec![GuardEvent::Recover]);
    }
}
