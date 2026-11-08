//! Curriculum de base : aller vers la balle (0) → toucher la balle (1) → marquer (2).
//!
//! Les paliers 0 et 1 se terminent au premier toucher ; le palier 2 laisse l'épisode courir jusqu'au but.
//! Promotion : taux de toucher moyen ≥ `PROMOTE_AT` sur les `WINDOW` dernières itérations.
//! Le palier est persisté dans `<out>/stage` pour survivre à un redémarrage.

use std::collections::VecDeque;
use std::path::Path;

pub const LAST_STAGE: u32 = 2;
const WINDOW: usize = 10;
const PROMOTE_AT: f64 = 0.9;

#[derive(Debug)]
pub struct Curriculum {
    stage: u32,
    recent: VecDeque<f64>,
}

impl Curriculum {
    pub fn new(stage: u32) -> Self {
        Self {
            stage: stage.min(LAST_STAGE),
            recent: VecDeque::new(),
        }
    }

    pub fn stage(&self) -> u32 {
        self.stage
    }

    /// Enregistre le taux de toucher d'une itération ; renvoie le nouveau palier en cas de promotion.
    pub fn observe(&mut self, touch_rate: f64) -> Option<u32> {
        if self.stage >= LAST_STAGE {
            return None;
        }
        self.recent.push_back(touch_rate);
        if self.recent.len() > WINDOW {
            self.recent.pop_front();
        }
        let mean = self.recent.iter().sum::<f64>() / self.recent.len() as f64;
        if self.recent.len() == WINDOW && mean >= PROMOTE_AT {
            self.stage += 1;
            self.recent.clear();
            return Some(self.stage);
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

    #[test]
    fn promotes_after_a_full_good_window() {
        let mut c = Curriculum::new(0);
        for _ in 0..WINDOW - 1 {
            assert_eq!(c.observe(1.0), None);
        }
        assert_eq!(c.observe(1.0), Some(1));
        assert_eq!(c.stage(), 1);
    }

    #[test]
    fn does_not_promote_on_a_weak_window_nor_past_the_last_stage() {
        let mut c = Curriculum::new(0);
        for _ in 0..50 {
            assert_eq!(c.observe(0.5), None);
        }
        let mut last = Curriculum::new(LAST_STAGE);
        assert_eq!(last.observe(1.0), None);
        assert_eq!(Curriculum::new(99).stage(), LAST_STAGE);
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
