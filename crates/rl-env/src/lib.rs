//! Environnement d'apprentissage par renforcement headless, dans le style RLGym.
//!
//! - [`ActionTable`] : table discrète d'actions (contrôles de voiture), comme la `LookupTableAction` de RLGym ;
//! - [`Env`] : un épisode « toucher la balle » (1 voiture, 1 balle), action repeat configurable ;
//! - [`VecEnv`] : N environnements répartis sur des threads, remise à zéro automatique en fin d'épisode.
//!
//! Les unités sont celles de RocketSim (unreal units, 120 Hz).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

pub use sim_core::SimError;

/// Dimension du vecteur d'observation.
pub const OBS_DIM: usize = 33;

const POS_SCALE: [f32; 3] = [4096.0, 5120.0, 2048.0];
const REL_POS_SCALE: [f32; 3] = [8192.0, 10240.0, 2048.0];
const CAR_VEL_SCALE: f32 = 2300.0;
const BALL_VEL_SCALE: f32 = 6000.0;
const ANG_VEL_SCALE: f32 = 5.5;
const BALL_REST_Z: f32 = 93.0;
const FIELD_X: f32 = 3800.0;
const FIELD_Y: f32 = 4800.0;

// ---------------------------------------------------------------- RNG

/// Générateur pseudo-aléatoire minimal (splitmix64) : reproductible, sans dépendance.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniforme dans [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniforme dans [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

// ---------------------------------------------------------------- actions

/// Table discrète d'actions : chaque entrée est un jeu complet de contrôles.
#[derive(Clone, Debug)]
pub struct ActionTable {
    controls: Vec<Controls>,
}

impl Default for ActionTable {
    fn default() -> Self {
        Self::rlgym()
    }
}

impl ActionTable {
    /// Table construite comme la `LookupTableAction` de RLGym (rlgym-ppo) : actions au sol
    /// (accélérateur × direction × boost) puis actions aériennes (tangage × lacet × roulis × saut × boost).
    pub fn rlgym() -> Self {
        let mut controls = Vec::new();
        for throttle in [-1.0f32, 0.0, 1.0] {
            for steer in [-1.0f32, 0.0, 1.0] {
                for boost in [false, true] {
                    if boost && throttle != 1.0 {
                        continue;
                    }
                    controls.push(Controls {
                        throttle,
                        steer,
                        yaw: steer,
                        boost,
                        ..Default::default()
                    });
                }
            }
        }
        for pitch in [-1.0f32, 0.0, 1.0] {
            for yaw in [-1.0f32, 0.0, 1.0] {
                for roll in [-1.0f32, 0.0, 1.0] {
                    for jump in [false, true] {
                        for boost in [false, true] {
                            if jump && yaw != 0.0 {
                                continue;
                            }
                            if pitch == 0.0 && roll == 0.0 && !jump {
                                continue;
                            }
                            let handbrake = jump && (pitch != 0.0 || yaw != 0.0 || roll != 0.0);
                            controls.push(Controls {
                                throttle: if boost { 1.0 } else { 0.0 },
                                steer: yaw,
                                pitch,
                                yaw,
                                roll,
                                jump,
                                boost,
                                handbrake,
                            });
                        }
                    }
                }
            }
        }
        Self { controls }
    }

    pub fn len(&self) -> usize {
        self.controls.len()
    }
    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }
    /// Contrôles de l'action `i` (indice borné au nombre d'actions).
    pub fn get(&self, i: usize) -> Controls {
        self.controls[i.min(self.controls.len() - 1)]
    }
}

// ---------------------------------------------------------------- configuration

#[derive(Clone, Debug)]
pub struct EnvConfig {
    /// Ticks de simulation par décision (action repeat). RLGym : 8 (soit 15 décisions/s).
    pub tick_skip: u32,
    /// Fin d'épisode par temps écoulé (en décisions).
    pub max_steps: u32,
    /// Termine l'épisode au premier toucher (drill « toucher la balle »).
    pub end_on_touch: bool,
    /// Récompense par toucher de la balle.
    pub touch_reward: f32,
    /// Poids de la récompense « aller vers la balle » (composante de vitesse vers la balle, dans [-1, 1]
    /// avant poids, par décision).
    pub approach_weight: f32,
    /// Distance mini voiture/balle au reset.
    pub min_spawn_dist: f32,
    /// Boost initial (0–100).
    pub start_boost: f32,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            tick_skip: 8,
            max_steps: 150,
            end_on_touch: true,
            touch_reward: 1.0,
            approach_weight: 0.01,
            min_spawn_dist: 1000.0,
            start_boost: 33.0,
        }
    }
}

// ---------------------------------------------------------------- Env

/// Résultat d'une décision.
#[derive(Clone, Debug, Default)]
pub struct StepInfo {
    pub reward: f32,
    /// Fin naturelle (toucher en mode `end_on_touch`, but).
    pub terminated: bool,
    /// Fin par temps (`max_steps`).
    pub truncated: bool,
    pub touched: bool,
}

/// Un environnement : une voiture bleue, une balle, un seul agent.
pub struct Env {
    sim: Sim,
    car: u32,
    cfg: EnvConfig,
    actions: ActionTable,
    rng: Rng,
    steps: u32,
    last_touch: Option<u64>,
}

impl Env {
    /// `init_meshes` doit avoir réussi avant (une fois par processus).
    pub fn new(cfg: EnvConfig, seed: u64) -> Result<Self, SimError> {
        let mut sim = Sim::new_soccar()?;
        let car = sim.add_car(true);
        let mut e = Self {
            sim,
            car,
            cfg,
            actions: ActionTable::rlgym(),
            rng: Rng::new(seed),
            steps: 0,
            last_touch: None,
        };
        e.reset();
        Ok(e)
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Reset aléatoire : voiture et balle tirées dans le terrain, distance minimale entre elles.
    pub fn reset(&mut self) -> [f32; OBS_DIM] {
        let (car_pos, ball_pos) = loop {
            let c = [
                self.rng.range(-FIELD_X, FIELD_X),
                self.rng.range(-FIELD_Y, FIELD_Y),
                17.0,
            ];
            let b = [
                self.rng.range(-FIELD_X, FIELD_X),
                self.rng.range(-FIELD_Y, FIELD_Y),
                BALL_REST_Z,
            ];
            let d = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2)).sqrt();
            if d >= self.cfg.min_spawn_dist {
                break (c, b);
            }
        };
        let yaw = self.rng.range(-std::f32::consts::PI, std::f32::consts::PI);
        self.sim
            .reset_car(self.car, car_pos, yaw, [0.0; 3], self.cfg.start_boost)
            .expect("voiture connue");
        self.sim.reset_ball(ball_pos, [0.0; 3]);
        self.sim.set_controls(self.car, Controls::default()).ok();
        self.steps = 0;
        self.last_touch = self.sim.last_ball_touch_tick(self.car);
        self.observe()
    }

    /// Avance d'une décision avec l'action `action` (indice de la table).
    pub fn step(&mut self, action: usize) -> StepInfo {
        self.sim
            .set_controls(self.car, self.actions.get(action))
            .expect("voiture connue");
        self.sim.step(self.cfg.tick_skip);
        self.steps += 1;

        let touch_now = self.sim.last_ball_touch_tick(self.car);
        let touched = touch_now.is_some() && touch_now != self.last_touch;
        self.last_touch = touch_now;

        let snap = self.sim.snapshot();
        let car = &snap.cars[0];
        let mut dir = [
            snap.ball.pos[0] - car.pos[0],
            snap.ball.pos[1] - car.pos[1],
            snap.ball.pos[2] - car.pos[2],
        ];
        let n = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
            .sqrt()
            .max(1.0);
        for d in &mut dir {
            *d /= n;
        }
        let toward =
            (car.vel[0] * dir[0] + car.vel[1] * dir[1] + car.vel[2] * dir[2]) / CAR_VEL_SCALE;

        let mut reward = self.cfg.approach_weight * toward.clamp(-1.0, 1.0);
        if touched {
            reward += self.cfg.touch_reward;
        }
        let scored = self.sim.is_ball_scored();
        StepInfo {
            reward,
            terminated: scored || (touched && self.cfg.end_on_touch),
            truncated: self.steps >= self.cfg.max_steps,
            touched,
        }
    }

    /// Action de la table qui roule vers l'avant avec boost et direction `steer` (-1 ou 1) ; sert aux tests
    /// et au benchmark (politique de référence).
    pub fn expert_action(&self, steer: f32) -> usize {
        (0..self.actions.len())
            .find(|&i| {
                let c = self.actions.get(i);
                c.throttle == 1.0 && c.steer == steer && c.boost && !c.jump && c.pitch == 0.0
            })
            .unwrap_or(0)
    }

    /// Observation normalisée (≈ [-1, 1]) de l'état courant.
    pub fn observe(&mut self) -> [f32; OBS_DIM] {
        let s = self.sim.snapshot();
        let c = &s.cars[0];
        let mut o = [0.0f32; OBS_DIM];
        let mut i = 0;
        let mut push3 = |v: [f32; 3], sc: [f32; 3], o: &mut [f32; OBS_DIM]| {
            for k in 0..3 {
                o[i] = v[k] / sc[k];
                i += 1;
            }
        };
        let cs = [CAR_VEL_SCALE; 3];
        push3(c.pos, POS_SCALE, &mut o);
        push3(c.vel, cs, &mut o);
        push3(c.ang_vel, [ANG_VEL_SCALE; 3], &mut o);
        push3(c.forward, [1.0; 3], &mut o);
        push3(c.up, [1.0; 3], &mut o);
        push3(s.ball.pos, POS_SCALE, &mut o);
        push3(s.ball.vel, [BALL_VEL_SCALE; 3], &mut o);
        push3(s.ball.ang_vel, [6.0; 3], &mut o);
        let rel_pos = [
            s.ball.pos[0] - c.pos[0],
            s.ball.pos[1] - c.pos[1],
            s.ball.pos[2] - c.pos[2],
        ];
        let rel_vel = [
            s.ball.vel[0] - c.vel[0],
            s.ball.vel[1] - c.vel[1],
            s.ball.vel[2] - c.vel[2],
        ];
        push3(rel_pos, REL_POS_SCALE, &mut o);
        push3(rel_vel, [BALL_VEL_SCALE; 3], &mut o);
        o[i] = c.boost / 100.0;
        o[i + 1] = f32::from(u8::from(c.on_ground));
        o[i + 2] = f32::from(u8::from(c.has_flip_or_jump));
        debug_assert_eq!(i + 3, OBS_DIM);
        o
    }
}

// ---------------------------------------------------------------- VecEnv

/// Épisode terminé, remonté par [`VecEnv::step`].
#[derive(Clone, Debug)]
pub struct EpisodeStats {
    pub ret: f32,
    pub length: u32,
    pub touched: bool,
}

/// Résultat groupé d'une décision sur tous les envs. Les observations sont celles *après* remise à zéro
/// automatique : si `done[i]`, `obs[i]` est le début de l'épisode suivant.
#[derive(Clone, Debug, Default)]
pub struct VecStep {
    pub obs: Vec<f32>,
    pub reward: Vec<f32>,
    pub terminated: Vec<bool>,
    pub truncated: Vec<bool>,
    pub finished: Vec<EpisodeStats>,
}

struct Reply {
    first: usize,
    obs: Vec<f32>,
    reward: Vec<f32>,
    terminated: Vec<bool>,
    truncated: Vec<bool>,
    finished: Vec<EpisodeStats>,
}

enum Cmd {
    Step(Vec<usize>),
    Reset,
    Stop,
}

struct Worker {
    tx: Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
    first: usize,
}

/// N environnements répartis sur `threads` threads. Chaque thread possède ses envs (la simulation C++
/// n'est jamais partagée entre threads).
pub struct VecEnv {
    workers: Vec<Worker>,
    rx: Receiver<Reply>,
    n_envs: usize,
    action_count: usize,
}

impl VecEnv {
    /// Charge les meshes depuis l'environnement, puis lance les threads.
    pub fn new(n_envs: usize, threads: usize, cfg: EnvConfig, seed: u64) -> Result<Self, SimError> {
        let dir = meshes_dir_from_env()?;
        init_meshes(&dir)?;
        let threads = threads.clamp(1, n_envs.max(1));
        let (rtx, rx) = channel::<Reply>();
        let mut workers = Vec::new();
        let (base, extra) = (n_envs / threads, n_envs % threads);
        let mut first = 0;
        let (ready_tx, ready_rx) = channel::<Result<(), SimError>>();
        for t in 0..threads {
            let count = base + usize::from(t < extra);
            let (tx, crx) = channel::<Cmd>();
            let (rtx, cfg, ready_tx) = (rtx.clone(), cfg.clone(), ready_tx.clone());
            let f = first;
            let handle = std::thread::Builder::new()
                .name(format!("env-{t}"))
                .spawn(move || worker_loop(f, count, cfg, seed, crx, rtx, ready_tx))
                .expect("thread");
            workers.push(Worker {
                tx,
                handle: Some(handle),
                first,
            });
            first += count;
        }
        drop((ready_tx, rtx));
        for _ in 0..threads {
            ready_rx.recv().expect("un worker s'est arrêté")?;
        }
        let action_count = ActionTable::rlgym().len();
        Ok(Self {
            workers,
            rx,
            n_envs,
            action_count,
        })
    }

    pub fn n_envs(&self) -> usize {
        self.n_envs
    }
    pub fn action_count(&self) -> usize {
        self.action_count
    }
    pub fn obs_dim(&self) -> usize {
        OBS_DIM
    }

    fn gather(&self) -> VecStep {
        let mut replies: Vec<Reply> = (0..self.workers.len())
            .map(|_| self.rx.recv().expect("worker"))
            .collect();
        replies.sort_by_key(|r| r.first);
        let mut out = VecStep::default();
        for r in replies {
            out.obs.extend(r.obs);
            out.reward.extend(r.reward);
            out.terminated.extend(r.terminated);
            out.truncated.extend(r.truncated);
            out.finished.extend(r.finished);
        }
        out
    }

    /// Remet tous les envs à zéro et renvoie les observations initiales (`n_envs * OBS_DIM`).
    pub fn reset(&mut self) -> Vec<f32> {
        for w in &self.workers {
            w.tx.send(Cmd::Reset).expect("worker");
        }
        self.gather().obs
    }

    /// Une décision par env (`actions.len() == n_envs`).
    pub fn step(&mut self, actions: &[usize]) -> VecStep {
        assert_eq!(actions.len(), self.n_envs, "une action par env");
        let next = self
            .workers
            .iter()
            .map(|w| w.first)
            .skip(1)
            .chain([self.n_envs]);
        for (w, end) in self.workers.iter().zip(next) {
            w.tx.send(Cmd::Step(actions[w.first..end].to_vec()))
                .expect("worker");
        }
        self.gather()
    }
}

impl Drop for VecEnv {
    fn drop(&mut self) {
        for w in &self.workers {
            w.tx.send(Cmd::Stop).ok();
        }
        for w in &mut self.workers {
            if let Some(h) = w.handle.take() {
                h.join().ok();
            }
        }
    }
}

fn worker_loop(
    first: usize,
    count: usize,
    cfg: EnvConfig,
    seed: u64,
    rx: Receiver<Cmd>,
    tx: Sender<Reply>,
    ready: Sender<Result<(), SimError>>,
) {
    let mut envs = Vec::with_capacity(count);
    for i in 0..count {
        match Env::new(
            cfg.clone(),
            seed.wrapping_add(((first + i) as u64).wrapping_mul(0x1234_5678_9ABC_DEF1)),
        ) {
            Ok(e) => envs.push(e),
            Err(e) => {
                ready.send(Err(e)).ok();
                return;
            }
        }
    }
    ready.send(Ok(())).ok();
    let mut ret = vec![0.0f32; count];
    let mut len = vec![0u32; count];
    let mut touched = vec![false; count];
    while let Ok(cmd) = rx.recv() {
        let mut reply = Reply {
            first,
            obs: Vec::with_capacity(count * OBS_DIM),
            reward: Vec::new(),
            terminated: Vec::new(),
            truncated: Vec::new(),
            finished: Vec::new(),
        };
        match cmd {
            Cmd::Stop => return,
            Cmd::Reset => {
                for (i, e) in envs.iter_mut().enumerate() {
                    reply.obs.extend_from_slice(&e.reset());
                    (ret[i], len[i], touched[i]) = (0.0, 0, false);
                }
            }
            Cmd::Step(actions) => {
                for (i, e) in envs.iter_mut().enumerate() {
                    let r = e.step(actions[i]);
                    ret[i] += r.reward;
                    len[i] += 1;
                    touched[i] |= r.touched;
                    reply.reward.push(r.reward);
                    reply.terminated.push(r.terminated);
                    reply.truncated.push(r.truncated);
                    if r.terminated || r.truncated {
                        reply.finished.push(EpisodeStats {
                            ret: ret[i],
                            length: len[i],
                            touched: touched[i],
                        });
                        (ret[i], len[i], touched[i]) = (0.0, 0, false);
                        reply.obs.extend_from_slice(&e.reset());
                    } else {
                        reply.obs.extend_from_slice(&e.observe());
                    }
                }
            }
        }
        if tx.send(reply).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_table_has_expected_size_and_valid_ranges() {
        let t = ActionTable::rlgym();
        assert!(!t.is_empty());
        println!("actions: {}", t.len());
        for i in 0..t.len() {
            let c = t.get(i);
            for v in [c.throttle, c.steer, c.pitch, c.yaw, c.roll] {
                assert!((-1.0..=1.0).contains(&v));
            }
        }
        // 12 actions au sol + 66 aériennes
        assert_eq!(t.len(), 78);
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..100 {
            let x = a.unit();
            assert_eq!(x, b.unit());
            assert!((0.0..1.0).contains(&x));
        }
    }

    fn setup() -> bool {
        match meshes_dir_from_env() {
            Ok(d) => init_meshes(&d).is_ok(),
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn observation_is_normalized_and_reset_respects_min_distance() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::default(), 1).unwrap();
        for _ in 0..20 {
            let o = e.reset();
            assert!(o.iter().all(|x| x.is_finite() && x.abs() <= 1.5), "{o:?}");
            let d = (o[24] * REL_POS_SCALE[0]).hypot(o[25] * REL_POS_SCALE[1]);
            assert!(d >= 990.0, "distance {d}");
        }
    }

    #[test]
    fn times_out_and_stays_finite() {
        assert!(setup());
        let cfg = EnvConfig {
            max_steps: 30,
            end_on_touch: false,
            ..Default::default()
        };
        let mut e = Env::new(cfg, 2).unwrap();
        let mut last = StepInfo::default();
        for _ in 0..30 {
            last = e.step(0);
            assert!(last.reward.is_finite());
        }
        assert!(last.truncated);
    }

    #[test]
    fn driving_at_ball_gets_touch_and_approach_reward() {
        assert!(setup());
        // Voiture alignée sur la balle : on lui impose le cap par un reset dédié via tirage du seed.
        let cfg = EnvConfig {
            max_steps: 400,
            ..Default::default()
        };
        let mut hits = 0;
        for seed in 0..40 {
            let mut e = Env::new(cfg.clone(), seed).unwrap();
            // Politique experte : tourner vers la balle puis accélérer et utiliser le boost.
            for _ in 0..400 {
                let o = e.observe();
                let (rx, ry) = (o[24], o[25]);
                let (fx, fy) = (o[9], o[10]);
                let cross = fx * ry - fy * rx;
                let steer = if cross > 0.0 { 1.0 } else { -1.0 };
                let a = e.expert_action(steer);
                let r = e.step(a);
                if r.touched {
                    hits += 1;
                    assert!(r.reward >= cfg.touch_reward - 0.02);
                    break;
                }
                if r.truncated {
                    break;
                }
            }
        }
        assert!(hits >= 20, "l'expert devrait toucher souvent : {hits}/40");
    }

    #[test]
    fn vec_env_steps_and_autoresets() {
        assert!(setup());
        let cfg = EnvConfig {
            max_steps: 5,
            ..Default::default()
        };
        let mut v = VecEnv::new(5, 2, cfg, 3).unwrap();
        let o = v.reset();
        assert_eq!(o.len(), 5 * OBS_DIM);
        let mut finished = 0;
        for _ in 0..12 {
            let s = v.step(&[0, 1, 2, 3, 4]);
            assert_eq!(s.obs.len(), 5 * OBS_DIM);
            assert_eq!(s.reward.len(), 5);
            finished += s.finished.len();
        }
        assert!(finished >= 10, "{finished}");
    }
}
