//! Qualité d'une mécanique reconnue : vitesse de la balle vers le but adverse juste après, et but dans les 3 s.

use crate::BallTick;
use crate::thresholds::*;

/// Résultat mesuré après une mécanique.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quality {
    /// Vitesse maximale de la balle vers le but adverse (uu/s) dans la seconde après la mécanique (≥ 0).
    pub speed: f32,
    /// But de l'équipe dans les 3 s.
    pub goal: bool,
}

impl Quality {
    /// Score simple 0..1 : moitié vitesse (3 000 uu/s = 1), moitié but.
    pub fn score(&self) -> f32 {
        0.5 * (self.speed / QUALITY_SPEED_REF).clamp(0.0, 1.0) + if self.goal { 0.5 } else { 0.0 }
    }
}

/// Suit la balle pendant 3 s à partir d'une mécanique. Sans allocation.
#[derive(Clone, Copy, Debug, Default)]
pub struct QualityTracker {
    ticks: u32,
    attack_neg: bool,
    q: Quality,
}

impl QualityTracker {
    /// `blue` : équipe de la voiture (le bleu marque en y > 0).
    pub fn start(blue: bool) -> Self {
        Self {
            ticks: 0,
            attack_neg: !blue,
            q: Quality::default(),
        }
    }

    /// À appeler à chaque tick après le déclenchement. Vrai quand la fenêtre de 3 s est écoulée.
    pub fn observe(&mut self, ball: &BallTick) -> bool {
        self.observe_with(ball, false)
    }

    /// Comme [`QualityTracker::observe`] ; `tainted` (collision ou toucher simultané récent, voir
    /// [`crate::Detector::tainted`]) : la vitesse de balle de ce tick n'est pas imputable à la mécanique.
    pub fn observe_with(&mut self, ball: &BallTick, tainted: bool) -> bool {
        self.ticks += 1;
        let s = if self.attack_neg { -1.0 } else { 1.0 };
        if self.ticks <= QUALITY_SPEED_TICKS && !tainted {
            self.q.speed = self.q.speed.max(s * ball.vel[1]);
        }
        if s * ball.pos[1] > GOAL_LINE_Y {
            self.q.goal = true;
        }
        self.done()
    }

    pub fn done(&self) -> bool {
        self.ticks >= QUALITY_GOAL_TICKS || self.q.goal
    }

    /// Mesure actuelle (partielle si la fenêtre n'est pas écoulée).
    pub fn quality(&self) -> Quality {
        self.q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(y: f32, vy: f32) -> BallTick {
        BallTick {
            pos: [0.0, y, 100.0],
            vel: [0.0, vy, 0.0],
        }
    }

    #[test]
    fn vitesse_et_but() {
        let mut t = QualityTracker::start(true);
        t.observe(&ball(0.0, 1500.0));
        t.observe(&ball(100.0, 2000.0));
        assert!(!t.done());
        assert_eq!(t.quality().speed, 2000.0);
        assert!(!t.quality().goal);
        assert!(t.observe(&ball(5200.0, 0.0)));
        assert!(t.quality().goal);
        assert!((t.quality().score() - (0.5 * 2000.0 / 3000.0 + 0.5)).abs() < 1e-5);
    }

    #[test]
    fn vitesse_ignoree_pendant_un_choc() {
        let mut t = QualityTracker::start(true);
        t.observe_with(&ball(0.0, 2400.0), true);
        t.observe_with(&ball(0.0, 900.0), false);
        assert_eq!(t.quality().speed, 900.0);
    }

    #[test]
    fn orange_attaque_vers_y_negatif() {
        let mut t = QualityTracker::start(false);
        t.observe(&ball(0.0, -1200.0));
        t.observe(&ball(0.0, 3000.0));
        assert_eq!(t.quality().speed, 1200.0);
        assert!(!t.quality().goal);
    }
}
