//! Curriculum : aller vers la balle (0) → toucher (1) → marquer balle au repos (2) → balle en mouvement (3)
//! → positions difficiles (4) → adversaire statique (5) → self-play (6). Voir `docs/curriculum.md`.
//!
//! Paliers 0 et 1 : promotion sur le taux de toucher (≥ 90 %). Paliers 2 à 5 : promotion sur le taux de but
//! (≥ 80 %) sur les `WINDOW` dernières itérations ; régression d'un palier si le taux de but s'effondre
//! (< 10 %) après `GRACE` itérations passées au palier. Le palier est persisté dans `<out>/stage`.

use std::collections::VecDeque;
use std::path::Path;

pub const LAST_STAGE: u32 = 6;
const WINDOW: usize = 10;
const PROMOTE_TOUCH: f64 = 0.9;
const PROMOTE_GOAL: f64 = 0.8;
const REGRESS_GOAL: f64 = 0.1;
/// Itérations minimales passées à un palier avant d'envisager une régression.
const GRACE: usize = 30;
/// Premier palier jugé sur le taux de but.
const FIRST_GOAL_STAGE: u32 = 2;
/// Premier palier où une régression est possible (on ne redescend jamais sous le palier 2).
const FIRST_REGRESS_STAGE: u32 = 3;

/// Résultat d'une itération d'entraînement.
#[derive(Clone, Copy, Debug, Default)]
pub struct IterResult {
    pub touch_rate: f64,
    pub goals: u64,
    pub episodes: u64,
}

/// Changement de palier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Up(u32),
    Down(u32),
}

impl Move {
    pub fn stage(self) -> u32 {
        match self {
            Move::Up(s) | Move::Down(s) => s,
        }
    }
}

#[derive(Debug)]
pub struct Curriculum {
    stage: u32,
    recent: VecDeque<IterResult>,
    iters_at_stage: usize,
}

impl Curriculum {
    pub fn new(stage: u32) -> Self {
        Self {
            stage: stage.min(LAST_STAGE),
            recent: VecDeque::new(),
            iters_at_stage: 0,
        }
    }

    pub fn stage(&self) -> u32 {
        self.stage
    }

    /// Taux de but glissant (buts / épisodes) sur la fenêtre courante.
    pub fn goal_rate(&self) -> f64 {
        let (g, e) = self
            .recent
            .iter()
            .fold((0u64, 0u64), |a, r| (a.0 + r.goals, a.1 + r.episodes));
        if e == 0 { 0.0 } else { g as f64 / e as f64 }
    }

    fn touch_rate(&self) -> f64 {
        self.recent.iter().map(|r| r.touch_rate).sum::<f64>() / self.recent.len().max(1) as f64
    }

    fn switch(&mut self, stage: u32) {
        self.stage = stage;
        self.recent.clear();
        self.iters_at_stage = 0;
    }

    /// Enregistre le résultat d'une itération ; renvoie le changement de palier éventuel.
    pub fn observe(&mut self, r: IterResult) -> Option<Move> {
        self.recent.push_back(r);
        if self.recent.len() > WINDOW {
            self.recent.pop_front();
        }
        self.iters_at_stage += 1;
        let full = self.recent.len() == WINDOW;
        if self.stage < FIRST_GOAL_STAGE {
            if full && self.touch_rate() >= PROMOTE_TOUCH {
                self.switch(self.stage + 1);
                return Some(Move::Up(self.stage));
            }
            return None;
        }
        if full && self.stage < LAST_STAGE && self.goal_rate() >= PROMOTE_GOAL {
            self.switch(self.stage + 1);
            return Some(Move::Up(self.stage));
        }
        if full
            && self.stage >= FIRST_REGRESS_STAGE
            && self.iters_at_stage >= GRACE
            && self.goal_rate() < REGRESS_GOAL
        {
            self.switch(self.stage - 1);
            return Some(Move::Down(self.stage));
        }
        None
    }

    pub fn load(dir: &Path) -> Self {
        let stage = std::fs::read_to_string(dir.join("stage"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        Self::new(stage)
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::write(dir.join("stage"), format!("{}\n", self.stage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(rate: f64) -> IterResult {
        IterResult {
            touch_rate: rate,
            goals: 0,
            episodes: 100,
        }
    }

    fn goals(per_100: u64) -> IterResult {
        IterResult {
            touch_rate: 1.0,
            goals: per_100,
            episodes: 100,
        }
    }

    #[test]
    fn touch_stages_promote_after_a_full_good_window() {
        let mut c = Curriculum::new(0);
        for _ in 0..WINDOW - 1 {
            assert_eq!(c.observe(touch(1.0)), None);
        }
        assert_eq!(c.observe(touch(1.0)), Some(Move::Up(1)));
        assert_eq!(c.stage(), 1);
    }

    #[test]
    fn goal_stages_promote_on_goal_rate_not_touch_rate() {
        let mut c = Curriculum::new(2);
        for _ in 0..50 {
            assert_eq!(c.observe(goals(70)), None, "70 % < 80 %");
        }
        // Fenêtre glissante : 70 % → 90 % ; la moyenne passe 80 % à la 5e itération à 90 %.
        for _ in 0..4 {
            assert_eq!(c.observe(goals(90)), None);
        }
        assert_eq!(c.observe(goals(90)), Some(Move::Up(3)));
        assert_eq!(c.stage(), 3);
        // Fenêtre vidée : il faut une nouvelle fenêtre complète.
        assert_eq!(c.observe(goals(100)), None);
    }

    #[test]
    fn regresses_only_after_grace_and_a_collapse() {
        let mut c = Curriculum::new(4);
        for _ in 0..GRACE - 1 {
            assert_eq!(c.observe(goals(2)), None, "période de grâce");
        }
        assert_eq!(c.observe(goals(2)), Some(Move::Down(3)));
        assert_eq!(c.stage(), 3);
        // Jamais sous le palier 2.
        let mut low = Curriculum::new(2);
        for _ in 0..200 {
            assert_eq!(low.observe(goals(0)), None);
        }
        assert_eq!(low.stage(), 2);
    }

    #[test]
    fn does_not_promote_past_the_last_stage() {
        let mut last = Curriculum::new(LAST_STAGE);
        for _ in 0..50 {
            assert_eq!(last.observe(goals(100)), None);
        }
        assert_eq!(Curriculum::new(99).stage(), LAST_STAGE);
    }

    #[test]
    fn goal_rate_is_pooled_over_the_window() {
        let mut c = Curriculum::new(2);
        c.observe(goals(50));
        c.observe(goals(100));
        assert!((c.goal_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn stage_survives_a_restart() {
        let d = std::env::temp_dir().join(format!("curr-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Curriculum::new(1).save(&d).unwrap();
        assert_eq!(Curriculum::load(&d).stage(), 1);
        std::fs::remove_dir_all(&d).ok();
    }
}
