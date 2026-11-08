//! D6 — porte de pause : pause dès qu'une lecture vidéo est détectée, reprise quand il n'y en a plus
//! depuis `resume_after`.

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct PauseGate {
    resume_after: Duration,
    paused: bool,
    idle_since: Option<Instant>,
}

impl PauseGate {
    pub fn new(resume_after: Duration) -> Self {
        Self {
            resume_after,
            paused: false,
            idle_since: None,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Met à jour l'état avec la dernière observation ; renvoie `true` si le learner doit être en pause.
    pub fn update(&mut self, playing: bool, now: Instant) -> bool {
        if playing {
            self.paused = true;
            self.idle_since = None;
        } else if self.paused {
            let since = *self.idle_since.get_or_insert(now);
            if now.duration_since(since) >= self.resume_after {
                self.paused = false;
                self.idle_since = None;
            }
        }
        self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pauses_on_playback_and_resumes_after_the_delay() {
        let t0 = Instant::now();
        let min = Duration::from_secs(60);
        let mut g = PauseGate::new(10 * min);
        assert!(!g.update(false, t0));
        assert!(g.update(true, t0 + min));
        assert!(g.update(false, t0 + 2 * min));
        assert!(g.update(false, t0 + 11 * min));
        assert!(!g.update(false, t0 + 12 * min));
    }

    #[test]
    fn a_new_playback_restarts_the_countdown() {
        let t0 = Instant::now();
        let min = Duration::from_secs(60);
        let mut g = PauseGate::new(10 * min);
        g.update(true, t0);
        g.update(false, t0 + min);
        g.update(true, t0 + 5 * min);
        assert!(g.update(false, t0 + 6 * min));
        assert!(g.update(false, t0 + 15 * min));
        assert!(!g.update(false, t0 + 16 * min));
    }
}
