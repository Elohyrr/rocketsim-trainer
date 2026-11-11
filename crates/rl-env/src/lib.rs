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

/// Dimension de l'observation d'avant l'adversaire (checkpoints historiques, voir `docs/curriculum.md`).
pub const OBS_DIM_V1: usize = 33;
/// Dimension du vecteur d'observation : [`OBS_DIM_V1`] + bloc adversaire (position, vitesse, cap, présence).
/// Le bloc adversaire est nul sans adversaire : un réseau dont les poids d'entrée de ce bloc sont nuls
/// se comporte exactement comme avant.
pub const OBS_DIM: usize = OBS_DIM_V1 + 10;
/// Fréquence de simulation (ticks par seconde).
pub const TICKS_PER_SECOND: f32 = 120.0;

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
    /// Récompense d'un but marqué dans le but adverse (la balle franchit le fond du terrain côté +Y).
    pub goal_reward: f32,
    /// Pénalité (valeur positive, soustraite) quand la balle entre dans le but de la voiture (+Y est le but visé).
    pub own_goal_penalty: f32,
    /// Vitesse horizontale initiale maximale de la balle (uu/s) ; 0 = balle au repos.
    pub ball_speed_max: f32,
    /// Répartition des positions de départ.
    pub spawn: SpawnMode,
    /// Adversaire (voiture orange).
    pub opponent: Opponent,
}

/// Tirage des positions de départ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnMode {
    /// Voiture et balle uniformes dans le terrain.
    Random,
    /// Positions difficiles : balle près des murs, en l'air basse, ou loin du but adverse.
    Hard,
    /// Une moitié `Random`, une moitié `Hard`.
    Mixed,
}

/// Adversaire de l'agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opponent {
    /// Pas d'adversaire (pas de seconde voiture).
    None,
    /// Voiture adverse immobile devant sa cage.
    Static,
    /// Voiture adverse pilotée de l'extérieur (copie gelée de la politique) via [`Env::step_vs`].
    SelfPlay,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            tick_skip: 8,
            max_steps: 150,
            end_on_touch: true,
            touch_reward: 1.0,
            approach_weight: 0.1,
            min_spawn_dist: 1000.0,
            start_boost: 33.0,
            goal_reward: 0.0,
            own_goal_penalty: 0.0,
            ball_speed_max: 0.0,
            spawn: SpawnMode::Random,
            opponent: Opponent::None,
        }
    }
}

impl EnvConfig {
    /// Config d'un palier de curriculum (voir `docs/curriculum.md`) : 0 = aller vers la balle, 1 = toucher,
    /// 2 = marquer (balle au repos), 3 = balle en mouvement, 4 = positions difficiles, 5 = adversaire statique,
    /// 6 = self-play. Au-delà : palier 6.
    pub fn for_stage(stage: u32) -> Self {
        let base = Self::default();
        let scoring = Self {
            end_on_touch: false,
            max_steps: 300,
            touch_reward: 0.1,
            approach_weight: 0.05,
            goal_reward: 10.0,
            own_goal_penalty: 5.0,
            ..base.clone()
        };
        match stage {
            0 => Self {
                approach_weight: 0.3,
                touch_reward: 3.0,
                ..base
            },
            1 => base,
            2 => scoring,
            3 => Self {
                ball_speed_max: 1500.0,
                ..scoring
            },
            4 => Self {
                ball_speed_max: 1000.0,
                spawn: SpawnMode::Hard,
                max_steps: 400,
                ..scoring
            },
            5 => Self {
                ball_speed_max: 1000.0,
                spawn: SpawnMode::Mixed,
                max_steps: 400,
                opponent: Opponent::Static,
                ..scoring
            },
            _ => Self {
                ball_speed_max: 1000.0,
                spawn: SpawnMode::Mixed,
                max_steps: 400,
                opponent: Opponent::SelfPlay,
                ..scoring
            },
        }
    }
}

// ---------------------------------------------------------------- Env

/// Issue d'un épisode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outcome {
    /// But marqué dans le but adverse (+Y).
    Goal,
    /// Balle dans son propre but (-Y) : but contre son camp, ou but encaissé en self-play.
    OwnGoal,
    /// Fin par temps écoulé.
    #[default]
    Timeout,
    /// Fin au premier toucher (paliers 0 et 1).
    Touch,
}

/// Résultat d'une décision.
#[derive(Clone, Debug, Default)]
pub struct StepInfo {
    pub reward: f32,
    /// Fin naturelle (toucher en mode `end_on_touch`, but).
    pub terminated: bool,
    /// Fin par temps (`max_steps`).
    pub truncated: bool,
    pub touched: bool,
    /// Issue de l'épisode, significative quand `terminated || truncated`.
    pub outcome: Outcome,
}

/// Un environnement : une voiture bleue, une balle, un seul agent (et un adversaire orange selon le palier).
pub struct Env {
    sim: Sim,
    car: u32,
    opp: Option<u32>,
    cfg: EnvConfig,
    actions: ActionTable,
    rng: Rng,
    steps: u32,
    last_touch: Option<u64>,
}

impl Env {
    /// `init_meshes` doit avoir réussi avant (une fois par processus).
    pub fn new(cfg: EnvConfig, seed: u64) -> Result<Self, SimError> {
        let (sim, car, opp) = Self::build_sim(&cfg)?;
        let mut e = Self {
            sim,
            car,
            opp,
            cfg,
            actions: ActionTable::rlgym(),
            rng: Rng::new(seed),
            steps: 0,
            last_touch: None,
        };
        e.reset();
        Ok(e)
    }

    fn build_sim(cfg: &EnvConfig) -> Result<(Sim, u32, Option<u32>), SimError> {
        let mut sim = Sim::new_soccar()?;
        let car = sim.add_car(true);
        let opp = (cfg.opponent != Opponent::None).then(|| sim.add_car(false));
        Ok((sim, car, opp))
    }

    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// Change la configuration (palier de curriculum). Si la présence d'un adversaire change, la simulation est
    /// reconstruite ; dans tous les cas l'appelant doit faire un [`Env::reset`] ensuite.
    pub fn set_config(&mut self, cfg: EnvConfig) {
        let had = self.opp.is_some();
        let wants = cfg.opponent != Opponent::None;
        self.cfg = cfg;
        if had != wants
            && let Ok((sim, car, opp)) = Self::build_sim(&self.cfg)
        {
            (self.sim, self.car, self.opp) = (sim, car, opp);
        }
    }

    /// Position (x, y) uniforme dans le terrain.
    fn random_xy(&mut self) -> [f32; 2] {
        [
            self.rng.range(-FIELD_X, FIELD_X),
            self.rng.range(-FIELD_Y, FIELD_Y),
        ]
    }

    /// Tire la balle (position, vitesse) selon le mode de départ.
    fn spawn_ball(&mut self) -> ([f32; 3], [f32; 3]) {
        let hard = match self.cfg.spawn {
            SpawnMode::Random => false,
            SpawnMode::Hard => true,
            SpawnMode::Mixed => self.rng.unit() < 0.5,
        };
        let [mut x, mut y] = self.random_xy();
        let mut z = BALL_REST_Z;
        if hard {
            match (self.rng.unit() * 3.0) as u32 {
                // Près d'un mur latéral ou d'un fond de terrain.
                0 => {
                    if self.rng.unit() < 0.7 {
                        x = FIELD_X
                            * self.rng.range(0.85, 0.97)
                            * self.rng.range(-1.0, 1.0).signum();
                    } else {
                        y = (FIELD_Y - self.rng.range(150.0, 900.0))
                            * self.rng.range(-1.0, 1.0).signum();
                    }
                }
                // En l'air, basse.
                1 => z = self.rng.range(250.0, 800.0),
                // Loin du but adverse : dans notre moitié de terrain.
                _ => y = self.rng.range(-FIELD_Y, -1500.0),
            }
        }
        let mut vel = [0.0f32; 3];
        let max = self.cfg.ball_speed_max;
        if max > 0.0 {
            let speed = self.rng.range(0.2 * max, max);
            let ang = self.rng.range(-std::f32::consts::PI, std::f32::consts::PI);
            vel = [speed * ang.cos(), speed * ang.sin(), 0.0];
        }
        ([x, y, z], vel)
    }

    /// Reset aléatoire : voiture et balle tirées dans le terrain, distance minimale entre elles.
    pub fn reset(&mut self) -> [f32; OBS_DIM] {
        let (car_pos, ball_pos, ball_vel) = loop {
            let [cx, cy] = self.random_xy();
            let (b, v) = self.spawn_ball();
            let c = [cx, cy, 17.0];
            let d = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2)).sqrt();
            if d >= self.cfg.min_spawn_dist {
                break (c, b, v);
            }
        };
        let yaw = self.rng.range(-std::f32::consts::PI, std::f32::consts::PI);
        self.sim
            .reset_car(self.car, car_pos, yaw, [0.0; 3], self.cfg.start_boost)
            .expect("voiture connue");
        self.sim.reset_ball(ball_pos, ball_vel);
        self.sim.set_controls(self.car, Controls::default()).ok();
        if let Some(opp) = self.opp {
            let (pos, yaw) = match self.cfg.opponent {
                // Devant la cage adverse (+Y), face au terrain.
                Opponent::Static => (
                    [self.rng.range(-700.0, 700.0), 4300.0, 17.0],
                    -std::f32::consts::FRAC_PI_2,
                ),
                _ => (
                    [
                        self.rng.range(-FIELD_X, FIELD_X),
                        self.rng.range(0.0, FIELD_Y),
                        17.0,
                    ],
                    self.rng.range(-std::f32::consts::PI, std::f32::consts::PI),
                ),
            };
            self.sim
                .reset_car(opp, pos, yaw, [0.0; 3], self.cfg.start_boost)
                .expect("voiture adverse connue");
            self.sim.set_controls(opp, Controls::default()).ok();
        }
        self.steps = 0;
        self.last_touch = self.sim.last_ball_touch_tick(self.car);
        self.observe()
    }

    /// Avance d'une décision avec l'action `action` (indice de la table) ; l'adversaire, s'il existe, ne bouge pas.
    pub fn step(&mut self, action: usize) -> StepInfo {
        self.step_vs(action, None)
    }

    /// Comme [`Env::step`], avec l'action de l'adversaire (self-play).
    pub fn step_vs(&mut self, action: usize, opp_action: Option<usize>) -> StepInfo {
        self.sim
            .set_controls(self.car, self.actions.get(action))
            .expect("voiture connue");
        if let (Some(opp), Some(a)) = (self.opp, opp_action) {
            self.sim
                .set_controls(opp, self.actions.get(a))
                .expect("voiture adverse connue");
        }
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
        // But pour nous : la balle est au fond du terrain côté +Y (but orange) ; côté -Y, c'est contre son camp.
        let our_goal = scored && snap.ball.pos[1] > 0.0;
        let own_goal = scored && !our_goal;
        if our_goal {
            reward += self.cfg.goal_reward;
        }
        if own_goal {
            reward -= self.cfg.own_goal_penalty;
        }
        let touch_end = touched && self.cfg.end_on_touch;
        let outcome = if our_goal {
            Outcome::Goal
        } else if own_goal {
            Outcome::OwnGoal
        } else if touch_end {
            Outcome::Touch
        } else {
            Outcome::Timeout
        };
        StepInfo {
            reward,
            terminated: scored || touch_end,
            truncated: self.steps >= self.cfg.max_steps,
            touched,
            outcome,
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

    /// Observation normalisée (≈ [-1, 1]) de l'état courant, du point de vue de la voiture bleue.
    pub fn observe(&mut self) -> [f32; OBS_DIM] {
        let s = self.sim.snapshot();
        build_obs(&s, 0, s.cars.get(1).map(|_| 1), false)
    }

    /// Observation du point de vue de l'adversaire (monde retourné de 180° autour de Z : il vise aussi +Y).
    /// Zéros s'il n'y a pas d'adversaire.
    pub fn observe_opponent(&mut self) -> [f32; OBS_DIM] {
        if self.opp.is_none() {
            return [0.0; OBS_DIM];
        }
        let s = self.sim.snapshot();
        build_obs(&s, 1, Some(0), true)
    }

    pub fn has_opponent(&self) -> bool {
        self.opp.is_some()
    }
}

fn push3(v: &mut Vec<f32>, x: [f32; 3], sc: [f32; 3]) {
    v.extend((0..3).map(|k| x[k] / sc[k]));
}

/// Construit l'observation de la voiture `me` ; `other` = voiture adverse éventuelle ; `mirror` retourne le monde
/// de 180° autour de Z (x → -x, y → -y).
fn build_obs(
    s: &sim_core::Snapshot,
    me: usize,
    other: Option<usize>,
    mirror: bool,
) -> [f32; OBS_DIM] {
    let m = |v: [f32; 3]| {
        if mirror { [-v[0], -v[1], v[2]] } else { v }
    };
    let c = &s.cars[me];
    let mut v: Vec<f32> = Vec::with_capacity(OBS_DIM);
    let (cpos, cvel, cang) = (m(c.pos), m(c.vel), m(c.ang_vel));
    let (bpos, bvel, bang) = (m(s.ball.pos), m(s.ball.vel), m(s.ball.ang_vel));
    push3(&mut v, cpos, POS_SCALE);
    push3(&mut v, cvel, [CAR_VEL_SCALE; 3]);
    push3(&mut v, cang, [ANG_VEL_SCALE; 3]);
    push3(&mut v, m(c.forward), [1.0; 3]);
    push3(&mut v, m(c.up), [1.0; 3]);
    push3(&mut v, bpos, POS_SCALE);
    push3(&mut v, bvel, [BALL_VEL_SCALE; 3]);
    push3(&mut v, bang, [6.0; 3]);
    push3(
        &mut v,
        [bpos[0] - cpos[0], bpos[1] - cpos[1], bpos[2] - cpos[2]],
        REL_POS_SCALE,
    );
    push3(
        &mut v,
        [bvel[0] - cvel[0], bvel[1] - cvel[1], bvel[2] - cvel[2]],
        [BALL_VEL_SCALE; 3],
    );
    v.extend([
        c.boost / 100.0,
        f32::from(u8::from(c.on_ground)),
        f32::from(u8::from(c.has_flip_or_jump)),
    ]);
    debug_assert_eq!(v.len(), OBS_DIM_V1);
    if let Some(j) = other {
        let d = &s.cars[j];
        push3(&mut v, m(d.pos), POS_SCALE);
        push3(&mut v, m(d.vel), [CAR_VEL_SCALE; 3]);
        push3(&mut v, m(d.forward), [1.0; 3]);
        v.push(1.0);
    }
    let mut o = [0.0f32; OBS_DIM];
    o[..v.len()].copy_from_slice(&v);
    o
}

// ---------------------------------------------------------------- VecEnv

/// Épisode terminé, remonté par [`VecEnv::step`].
#[derive(Clone, Debug)]
pub struct EpisodeStats {
    pub ret: f32,
    pub length: u32,
    pub touched: bool,
    pub outcome: Outcome,
}

/// Résultat groupé d'une décision sur tous les envs. Les observations sont celles *après* remise à zéro
/// automatique : si `done[i]`, `obs[i]` est le début de l'épisode suivant.
#[derive(Clone, Debug, Default)]
pub struct VecStep {
    pub obs: Vec<f32>,
    /// Observations de l'adversaire (point de vue retourné), vide sans adversaire.
    pub opp_obs: Vec<f32>,
    pub reward: Vec<f32>,
    pub terminated: Vec<bool>,
    pub truncated: Vec<bool>,
    pub finished: Vec<EpisodeStats>,
}

struct Reply {
    first: usize,
    obs: Vec<f32>,
    opp_obs: Vec<f32>,
    reward: Vec<f32>,
    terminated: Vec<bool>,
    truncated: Vec<bool>,
    finished: Vec<EpisodeStats>,
}

enum Cmd {
    Step(Vec<usize>, Option<Vec<usize>>),
    Reset,
    Config(EnvConfig),
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
    opp_obs: Vec<f32>,
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
            opp_obs: Vec::new(),
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
            out.opp_obs.extend(r.opp_obs);
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
        let g = self.gather();
        self.opp_obs = g.opp_obs;
        g.obs
    }

    /// Observations de l'adversaire après le dernier reset / pas (vide sans adversaire).
    pub fn opp_obs(&self) -> &[f32] {
        &self.opp_obs
    }

    /// Applique une nouvelle configuration à tous les envs et les remet à zéro (la présence d'un adversaire
    /// peut changer) ; renvoie les nouvelles observations initiales.
    pub fn set_config(&mut self, cfg: &EnvConfig) -> Vec<f32> {
        for w in &self.workers {
            w.tx.send(Cmd::Config(cfg.clone())).expect("worker");
        }
        let g = self.gather();
        self.opp_obs = g.opp_obs;
        g.obs
    }

    /// Une décision par env (`actions.len() == n_envs`).
    pub fn step(&mut self, actions: &[usize]) -> VecStep {
        self.step_vs(actions, None)
    }

    /// Comme [`VecEnv::step`], avec une action d'adversaire par env (self-play).
    pub fn step_vs(&mut self, actions: &[usize], opp: Option<&[usize]>) -> VecStep {
        assert_eq!(actions.len(), self.n_envs, "une action par env");
        let next = self
            .workers
            .iter()
            .map(|w| w.first)
            .skip(1)
            .chain([self.n_envs]);
        for (w, end) in self.workers.iter().zip(next) {
            w.tx.send(Cmd::Step(
                actions[w.first..end].to_vec(),
                opp.map(|o| o[w.first..end].to_vec()),
            ))
            .expect("worker");
        }
        let g = self.gather();
        self.opp_obs.clone_from(&g.opp_obs);
        g
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
            opp_obs: Vec::new(),
            reward: Vec::new(),
            terminated: Vec::new(),
            truncated: Vec::new(),
            finished: Vec::new(),
        };
        match cmd {
            Cmd::Stop => return,
            Cmd::Config(c) => {
                for (i, e) in envs.iter_mut().enumerate() {
                    e.set_config(c.clone());
                    reply.obs.extend_from_slice(&e.reset());
                    if e.has_opponent() {
                        reply.opp_obs.extend_from_slice(&e.observe_opponent());
                    }
                    (ret[i], len[i], touched[i]) = (0.0, 0, false);
                }
            }
            Cmd::Reset => {
                for (i, e) in envs.iter_mut().enumerate() {
                    reply.obs.extend_from_slice(&e.reset());
                    if e.has_opponent() {
                        reply.opp_obs.extend_from_slice(&e.observe_opponent());
                    }
                    (ret[i], len[i], touched[i]) = (0.0, 0, false);
                }
            }
            Cmd::Step(actions, opp) => {
                for (i, e) in envs.iter_mut().enumerate() {
                    let r = e.step_vs(actions[i], opp.as_ref().map(|o| o[i]));
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
                            outcome: r.outcome,
                        });
                        (ret[i], len[i], touched[i]) = (0.0, 0, false);
                        reply.obs.extend_from_slice(&e.reset());
                    } else {
                        reply.obs.extend_from_slice(&e.observe());
                    }
                    if e.has_opponent() {
                        reply.opp_obs.extend_from_slice(&e.observe_opponent());
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
                    assert!(r.reward >= cfg.touch_reward - cfg.approach_weight - 0.01);
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
