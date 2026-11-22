//! Curriculum automatique par drill : chaque drill de [`scenarios::CURRICULUM`] a un niveau de difficulté 0..1
//! (`scenarios::sample_at`). Sur chaque fenêtre d'au moins [`MIN_EPISODES`] épisodes, le niveau monte de
//! [`STEP`] si le taux de réussite dépasse [`UP_RATE`], descend si il passe sous [`DOWN_RATE`] ; la fenêtre
//! repart de zéro à chaque changement (le taux dépend du niveau). Niveaux persistés dans
//! `<out>/drill_levels.json` (réécrit à chaque changement, donc à côté du checkpoint et sans perte à la reprise).
//! Voir `docs/drills.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use scenarios::{CURRICULUM, DRILL_COUNT, Drill, DrillLevels};

pub const MIN_EPISODES: u64 = 100;
pub const UP_RATE: f64 = 0.5;
pub const DOWN_RATE: f64 = 0.15;
pub const STEP: f32 = 0.05;
/// Niveau de départ sans fichier : les drills commencent à leur version la plus facile.
pub const START_LEVEL: f32 = 0.0;
const FILE: &str = "drill_levels.json";

/// Seuils du curriculum (`DRILL_UP_RATE`, `DRILL_DOWN_RATE`, `DRILL_MIN_EPISODES`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillThresholds {
    pub up_rate: f64,
    pub down_rate: f64,
    pub min_episodes: u64,
}

impl Default for DrillThresholds {
    fn default() -> Self {
        Self {
            up_rate: UP_RATE,
            down_rate: DOWN_RATE,
            min_episodes: MIN_EPISODES,
        }
    }
}

pub struct DrillCurriculum {
    pub thresholds: DrillThresholds,
    levels: Arc<DrillLevels>,
    episodes: [u64; DRILL_COUNT],
    success: [u64; DRILL_COUNT],
    path: PathBuf,
}

impl DrillCurriculum {
    /// Charge `<dir>/drill_levels.json` (absent ou illisible : [`START_LEVEL`]) et règle les niveaux partagés.
    pub fn load(dir: &Path, levels: Arc<DrillLevels>) -> Self {
        let parsed = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        for d in CURRICULUM {
            let v = parsed
                .as_ref()
                .and_then(|p| p[d.label()].as_f64())
                .filter(|v| v.is_finite())
                .map_or(START_LEVEL, |v| v.clamp(0.0, 1.0) as f32);
            levels.set(d, v);
        }
        let c = Self {
            thresholds: DrillThresholds::default(),
            levels,
            episodes: [0; DRILL_COUNT],
            success: [0; DRILL_COUNT],
            path: dir.join(FILE),
        };
        c.save();
        c
    }

    /// Comme [`DrillCurriculum::load`] avec des seuils configurés.
    pub fn load_with(dir: &Path, levels: Arc<DrillLevels>, thresholds: DrillThresholds) -> Self {
        Self {
            thresholds,
            ..Self::load(dir, levels)
        }
    }

    pub fn level(&self, d: Drill) -> f32 {
        self.levels.get(d)
    }

    fn save(&self) {
        let body = CURRICULUM
            .iter()
            .map(|d| format!("\"{}\":{:.3}", d.label(), self.levels.get(*d)))
            .collect::<Vec<_>>()
            .join(",");
        if let Err(e) = std::fs::write(&self.path, format!("{{{body}}}\n")) {
            eprintln!("niveaux des drills non écrits : {e}");
        }
    }

    /// Ajoute les épisodes et réussites d'une itération ; renvoie vrai si un niveau a changé.
    pub fn update(&mut self, episodes: &[u64; DRILL_COUNT], success: &[u64; DRILL_COUNT]) -> bool {
        let mut changed = false;
        for d in CURRICULUM {
            let i = d.index();
            self.episodes[i] += episodes[i];
            self.success[i] += success[i];
            if self.episodes[i] < self.thresholds.min_episodes {
                continue;
            }
            let rate = self.success[i] as f64 / self.episodes[i] as f64;
            let l = self.levels.get(d);
            let next = if rate > self.thresholds.up_rate {
                (l + STEP).min(1.0)
            } else if rate < self.thresholds.down_rate {
                (l - STEP).max(0.0)
            } else {
                l
            };
            // Fenêtre glissante : on repart de zéro après chaque jugement (changement ou non).
            self.episodes[i] = 0;
            self.success[i] = 0;
            if next != l {
                self.levels.set(d, next);
                changed = true;
            }
        }
        if changed {
            self.save();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(d: Drill, eps: u64, ok: u64) -> ([u64; DRILL_COUNT], [u64; DRILL_COUNT]) {
        let (mut e, mut s) = ([0; DRILL_COUNT], [0; DRILL_COUNT]);
        e[d.index()] = eps;
        s[d.index()] = ok;
        (e, s)
    }

    #[test]
    fn monte_descend_et_persiste() {
        let dir = tempfile::tempdir().unwrap();
        let levels = Arc::new(DrillLevels::default());
        let mut c = DrillCurriculum::load(dir.path(), levels.clone());
        assert_eq!(c.level(Drill::Keeper), 0.0);
        // Moins de 100 épisodes : pas de jugement.
        let (e, s) = counts(Drill::Keeper, 60, 60);
        assert!(!c.update(&e, &s));
        // 60 + 60 ≥ 100, 100 % de réussite : monte.
        assert!(c.update(&e, &s));
        assert!((c.level(Drill::Keeper) - STEP).abs() < 1e-6);
        // Taux intermédiaire : stable.
        let (e, s) = counts(Drill::Keeper, 120, 40);
        assert!(!c.update(&e, &s));
        // Échec : redescend, sans passer sous 0.
        let (e, s) = counts(Drill::Keeper, 120, 2);
        assert!(c.update(&e, &s));
        assert!(!c.update(&e, &s));
        assert_eq!(c.level(Drill::Keeper), 0.0);
        // Les autres drills ne bougent pas.
        assert_eq!(c.level(Drill::Rings), 0.0);
        // Persistance.
        let (e, s) = counts(Drill::Rings, 200, 200);
        c.update(&e, &s);
        let l2 = Arc::new(DrillLevels::default());
        let c2 = DrillCurriculum::load(dir.path(), l2);
        assert!((c2.level(Drill::Rings) - STEP).abs() < 1e-6);
        assert_eq!(c2.level(Drill::Keeper), 0.0);
        // Un drill hors curriculum reste au niveau complet.
        assert_eq!(c2.level(Drill::ShotRoll), 1.0);
    }

    #[test]
    fn seuils_configurables() {
        let dir = tempfile::tempdir().unwrap();
        let th = DrillThresholds {
            up_rate: 0.3,
            down_rate: 0.1,
            min_episodes: 200,
        };
        let mut c = DrillCurriculum::load_with(dir.path(), Arc::new(DrillLevels::default()), th);
        // 150 < 200 épisodes : pas de jugement, même à 100 %.
        let (e, s) = counts(Drill::Keeper, 150, 150);
        assert!(!c.update(&e, &s));
        // 300 cumulés à 60 % : monte (> 0,3 ; aurait été stable avec les défauts 0,5).
        let (e, s) = counts(Drill::Keeper, 150, 30);
        assert!(c.update(&e, &s));
        assert!((c.level(Drill::Keeper) - STEP).abs() < 1e-6);
        // 12 % : stable (≥ 0,1 ; aurait descendu avec le défaut 0,15).
        let (e, s) = counts(Drill::Keeper, 200, 24);
        assert!(!c.update(&e, &s));
        // 5 % : redescend.
        let (e, s) = counts(Drill::Keeper, 200, 10);
        assert!(c.update(&e, &s));
        assert_eq!(c.level(Drill::Keeper), 0.0);
    }
}
