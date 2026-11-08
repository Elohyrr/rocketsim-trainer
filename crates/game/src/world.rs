//! Monde de jeu : simulation à pas fixe 120 Hz, buts, remise en jeu, drill, ralenti.
//! Indépendant du rendu et du périphérique : testable sans fenêtre.

use macroquad::math::{Quat, Vec3};
use sim_core::{PadSnapshot, Sim, Snapshot};

use crate::input::{Actions, RawInput};

/// Durée d'un tick de simulation (s).
pub const TICK_DT: f32 = 1.0 / sim_core::TICK_RATE as f32;
/// Durée pendant laquelle le jeu continue après un but avant la remise en jeu (s).
pub const GOAL_PAUSE: f32 = 2.5;
/// Durée maximale d'un essai de drill (s).
pub const DRILL_TIME: f32 = 10.0;
/// Rapports de ralenti proposés (0,25× à 1×).
pub const TIME_SCALES: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Terrain complet, balle, une voiture.
    Free,
    /// Tir au but : balle posée, il faut marquer avant la fin du temps.
    Drill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Goal { blue: bool },
    Reset,
    DrillSuccess,
    DrillFail,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DrillStats {
    pub successes: u32,
    pub attempts: u32,
    /// Temps écoulé dans l'essai en cours (s).
    pub elapsed: f32,
}

pub struct World {
    sim: Sim,
    car: u32,
    pub mode: Mode,
    /// Buts marqués : [bleu, orange].
    pub score: [u32; 2],
    pub drill: DrillStats,
    pub time_scale: f32,
    pub ball_cam: bool,
    pub prev: Snapshot,
    pub cur: Snapshot,
    pub pads: Vec<PadSnapshot>,
    /// Orientation visuelle de la balle (intégrée depuis sa vitesse angulaire).
    pub ball_rot: Quat,
    /// Temps restant avant remise en jeu après un but (s), si un but vient d'être marqué.
    pub goal_timer: Option<f32>,
    /// Vrai si le boost est actif au dernier tick (touche tenue et réservoir non vide).
    pub boosting: bool,
    acc: f32,
    jump_latch: bool,
    seed: u32,
}

impl World {
    /// `sim_core::init_meshes` doit avoir réussi avant.
    pub fn new() -> Result<Self, sim_core::SimError> {
        let mut sim = Sim::new_soccar()?;
        let car = sim.add_car(true);
        sim.reset_kickoff(1);
        let cur = sim.snapshot();
        let pads = sim.pads();
        Ok(Self {
            sim,
            car,
            mode: Mode::Free,
            score: [0, 0],
            drill: DrillStats::default(),
            time_scale: 1.0,
            ball_cam: true,
            prev: cur.clone(),
            cur,
            pads,
            ball_rot: Quat::IDENTITY,
            goal_timer: None,
            boosting: false,
            acc: 0.0,
            jump_latch: false,
            seed: 1,
        })
    }

    pub fn car(&self) -> &sim_core::CarSnapshot {
        &self.cur.cars[0]
    }

    pub fn ticks(&self) -> u64 {
        self.cur.tick
    }

    /// Part de tick écoulée depuis le dernier tick, dans [0, 1[ : pour interpoler l'affichage.
    pub fn alpha(&self) -> f32 {
        (self.acc / TICK_DT).clamp(0.0, 1.0)
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.seed >> 8
    }

    /// Remise en jeu : coup d'envoi (mode libre) ou nouvel essai (drill).
    pub fn reset(&mut self) {
        let seed = self.next_seed();
        match self.mode {
            Mode::Free => self.sim.reset_kickoff(seed as i32),
            Mode::Drill => {
                self.sim.reset_kickoff(seed as i32);
                let r = |s: u32, lo: f32, hi: f32| lo + (s % 1000) as f32 / 1000.0 * (hi - lo);
                let (bx, by) = (r(seed, -1800., 1800.), r(seed / 1000, -800., 1500.));
                let _ = self.sim.place_car(
                    self.car,
                    [0., -3000., 17.],
                    std::f32::consts::FRAC_PI_2,
                    100.,
                );
                self.sim.set_ball([bx, by, 93.], [0., 0., 0.]);
                self.drill.elapsed = 0.0;
            }
        }
        self.goal_timer = None;
        self.refresh();
        self.prev = self.cur.clone();
    }

    fn refresh(&mut self) {
        self.cur = self.sim.snapshot();
        self.pads = self.sim.pads();
    }

    pub fn toggle_drill(&mut self) {
        self.mode = match self.mode {
            Mode::Free => Mode::Drill,
            Mode::Drill => Mode::Free,
        };
        self.reset();
    }

    fn change_scale(&mut self, up: bool) {
        let i = TIME_SCALES
            .iter()
            .position(|&s| (s - self.time_scale).abs() < 1e-3)
            .unwrap_or(TIME_SCALES.len() - 1);
        let j = if up {
            (i + 1).min(TIME_SCALES.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        self.time_scale = TIME_SCALES[j];
    }

    /// Avance d'une frame de `dt` secondes réelles : applique les actions, puis autant de ticks fixes
    /// que nécessaire (`dt` × ralenti, borné à 0,1 s pour éviter la spirale). Retourne les événements.
    pub fn advance(&mut self, dt: f32, input: &RawInput, actions: &Actions) -> Vec<Event> {
        let mut events = Vec::new();
        if actions.toggle_ball_cam {
            self.ball_cam = !self.ball_cam;
        }
        if actions.slower {
            self.change_scale(false);
        }
        if actions.faster {
            self.change_scale(true);
        }
        if actions.toggle_drill {
            self.toggle_drill();
            events.push(Event::Reset);
        }
        if actions.reset {
            self.reset();
            events.push(Event::Reset);
        }

        self.jump_latch |= input.jump;
        let scaled = dt.clamp(0.0, 0.1) * self.time_scale;
        self.acc += scaled;
        let mut first = true;
        while self.acc >= TICK_DT {
            let mut inp = *input;
            if first {
                inp.jump |= self.jump_latch;
                self.jump_latch = false;
                first = false;
            }
            self.acc -= TICK_DT;
            self.tick(inp, &mut events);
        }
        let w = self.cur.ball.ang_vel;
        self.ball_rot = (Quat::from_scaled_axis(Vec3::new(w[0], w[1], w[2]) * scaled)
            * self.ball_rot)
            .normalize();
        events
    }

    fn tick(&mut self, input: RawInput, events: &mut Vec<Event>) {
        let _ = self.sim.set_controls(self.car, input.to_controls());
        self.sim.step(1);
        self.boosting = input.boost && self.cur.cars[0].boost > 0.0;
        self.prev = std::mem::replace(&mut self.cur, self.sim.snapshot());
        if self.cur.tick % 4 == 0 {
            self.pads = self.sim.pads();
        }

        if let Some(t) = &mut self.goal_timer {
            *t -= TICK_DT;
            if *t <= 0.0 {
                self.reset();
                events.push(Event::Reset);
            }
            return;
        }
        if self.mode == Mode::Drill {
            self.drill.elapsed += TICK_DT;
        }
        if self.sim.ball_scored() {
            // La balle est au-delà de y = +5120 : but dans le camp orange, marqué par les bleus.
            let blue = self.cur.ball.pos[1] > 0.0;
            self.score[usize::from(!blue)] += 1;
            self.goal_timer = Some(GOAL_PAUSE);
            events.push(Event::Goal { blue });
            if self.mode == Mode::Drill {
                self.drill.attempts += 1;
                if blue {
                    self.drill.successes += 1;
                    events.push(Event::DrillSuccess);
                } else {
                    events.push(Event::DrillFail);
                }
            }
        } else if self.mode == Mode::Drill && self.drill.elapsed >= DRILL_TIME {
            self.drill.attempts += 1;
            events.push(Event::DrillFail);
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let dir = sim_core::meshes_dir_from_env().unwrap_or_else(|e| panic!("{e}"));
        sim_core::init_meshes(&dir).unwrap();
        World::new().unwrap()
    }

    /// Boucle de jeu headless : entrées simulées, frames de 1/60 s, vérifie 120 ticks/s.
    #[test]
    fn game_loop_runs_fixed_120hz_with_simulated_input() {
        let mut w = world();
        let t0 = w.ticks();
        let p0 = w.car().pos;
        let input = RawInput {
            throttle: 1.0,
            stick_y: 1.0,
            boost: true,
            ..Default::default()
        };
        for _ in 0..60 {
            w.advance(1.0 / 60.0, &input, &Actions::default());
        }
        assert_eq!(w.ticks() - t0, 120, "1 s de jeu = 120 ticks");
        let d = (0..3)
            .map(|i| (w.car().pos[i] - p0[i]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(d > 300.0, "la voiture doit avancer en 1 s : {d}");
    }

    #[test]
    fn slow_motion_quarter_speed_runs_quarter_ticks() {
        let mut w = world();
        w.time_scale = 0.25;
        let t0 = w.ticks();
        for _ in 0..60 {
            w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default());
        }
        assert_eq!(w.ticks() - t0, 30);
    }

    #[test]
    fn slower_faster_cycle_within_bounds() {
        let mut w = world();
        for _ in 0..10 {
            w.advance(
                0.0,
                &RawInput::default(),
                &Actions {
                    slower: true,
                    ..Default::default()
                },
            );
        }
        assert_eq!(w.time_scale, 0.25);
        for _ in 0..10 {
            w.advance(
                0.0,
                &RawInput::default(),
                &Actions {
                    faster: true,
                    ..Default::default()
                },
            );
        }
        assert_eq!(w.time_scale, 1.0);
    }

    #[test]
    fn goal_is_detected_scored_and_play_restarts() {
        let mut w = world();
        // Tir fort vers le but orange (y > 0), au centre.
        w.sim.set_ball([0., 4500., 300.], [0., 3000., 0.]);
        let mut events = vec![];
        for _ in 0..60 {
            events.extend(w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default()));
        }
        assert!(events.contains(&Event::Goal { blue: true }), "{events:?}");
        assert_eq!(w.score, [1, 0]);
        assert!(w.goal_timer.is_some());
        // Après la pause, remise en jeu : balle au centre.
        for _ in 0..200 {
            events.extend(w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default()));
        }
        assert!(events.contains(&Event::Reset));
        assert!(w.goal_timer.is_none());
        assert!(w.cur.ball.pos[1].abs() < 500.0, "{:?}", w.cur.ball.pos);
    }

    #[test]
    fn reset_key_restores_kickoff_instantly() {
        let mut w = world();
        let start = w.car().pos;
        let input = RawInput {
            throttle: 1.0,
            boost: true,
            ..Default::default()
        };
        for _ in 0..120 {
            w.advance(1.0 / 60.0, &input, &Actions::default());
        }
        assert_ne!(w.car().pos, start);
        w.advance(
            0.0,
            &RawInput::default(),
            &Actions {
                reset: true,
                ..Default::default()
            },
        );
        assert!(w.car().vel.iter().all(|v| v.abs() < 1.0));
        assert!((w.cur.ball.pos[2] - 93.0).abs() < 5.0);
    }

    #[test]
    fn drill_places_car_and_ball_then_times_out() {
        let mut w = world();
        w.toggle_drill();
        assert_eq!(w.mode, Mode::Drill);
        assert!(w.cur.ball.vel.iter().all(|v| v.abs() < 1e-3));
        let mut events = vec![];
        for _ in 0..(12 * 60) {
            events.extend(w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default()));
        }
        assert!(events.contains(&Event::DrillFail));
        assert_eq!(w.drill.attempts, 1);
    }
}

#[cfg(test)]
mod control_signs {
    use sim_core::{Controls, Sim};

    /// Vérifie les conventions de `RawInput::to_controls` sur la vraie simulation : en l'air,
    /// pitch+ lève le nez, yaw+ tourne à droite, roll+ penche à droite.
    #[test]
    fn rocketsim_axis_conventions() {
        let dir = sim_core::meshes_dir_from_env().unwrap_or_else(|e| panic!("{e}"));
        sim_core::init_meshes(&dir).unwrap();
        let run = |c: Controls| {
            let mut s = Sim::new_soccar().unwrap();
            let car = s.add_car(true);
            s.place_car(car, [0., 0., 1000.], 0.0, 0.).unwrap();
            s.set_controls(car, c).unwrap();
            s.step(30);
            s.snapshot().cars.remove(0)
        };
        let p = run(Controls {
            pitch: 1.0,
            ..Default::default()
        });
        assert!(
            p.forward[2] > 0.2,
            "pitch+ doit lever le nez : {:?}",
            p.forward
        );
        let y = run(Controls {
            yaw: 1.0,
            ..Default::default()
        });
        assert!(
            y.forward[1] > 0.1,
            "yaw+ doit tourner vers la droite (+y) : {:?}",
            y.forward
        );
        let r = run(Controls {
            roll: 1.0,
            ..Default::default()
        });
        assert!(
            r.right[2] < -0.2,
            "roll+ doit abaisser le côté droit : {:?}",
            r.right
        );
    }
}
