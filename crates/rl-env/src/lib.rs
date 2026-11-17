//! Environnement d'apprentissage par renforcement headless, dans le style RLGym.
//!
//! - [`ActionTable`] : table discrète d'actions (contrôles de voiture), comme la `LookupTableAction` de RLGym ;
//! - [`Env`] : un épisode selon le palier de curriculum (voiture bleue, balle, voiture orange aux paliers 5 et 6),
//!   action repeat configurable ;
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
/// Le bloc adversaire est nul sans adversaire : un réseau dont les poids d'entrée de ce bloc sont nuls (checkpoint
/// historique agrandi, voir `docs/curriculum.md`) se comporte exactement comme avant.
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

/// Charge les meshes (`$ROCKETSIM_MESHES_DIR`) : à appeler une fois par processus avant [`Env::new`] hors `VecEnv`.
pub fn init_meshes_from_env() -> Result<(), SimError> {
    init_meshes(&meshes_dir_from_env()?)
}

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
    /// Part des épisodes qui sont des coups d'envoi (0 = aucun ; voir [`KickoffState`]). Tirée à chaque reset.
    pub kickoff_prob: f32,
    /// Coups d'envoi : l'adversaire reste immobile (contrôle d'évaluation) au lieu de foncer sur la balle.
    pub kickoff_idle_opponent: bool,
    /// Détecteur de mécaniques (tick par tick, sans effet sur la récompense) ; coupé pour mesurer son coût.
    pub detect_mechanics: bool,
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
            kickoff_prob: 0.0,
            kickoff_idle_opponent: false,
            detect_mechanics: true,
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
                kickoff_prob: KICKOFF_PROB,
                ..scoring
            },
            _ => Self {
                ball_speed_max: 1000.0,
                spawn: SpawnMode::Mixed,
                max_steps: 400,
                opponent: Opponent::SelfPlay,
                kickoff_prob: KICKOFF_PROB,
                ..scoring
            },
        }
    }

    /// Config d'évaluation de coups d'envoi : uniquement des kickoffs, contre le fonceur scripté (ou immobile
    /// si `idle`). Utilisée par `learner --eval-kickoff`.
    pub fn kickoff_eval(idle: bool) -> Self {
        Self {
            kickoff_prob: 1.0,
            kickoff_idle_opponent: idle,
            opponent: Opponent::Static,
            end_on_touch: false,
            max_steps: KICKOFF_MAX_STEPS + 10,
            touch_reward: 0.1,
            approach_weight: 0.05,
            goal_reward: 10.0,
            own_goal_penalty: 5.0,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------- coups d'envoi

/// Part des épisodes de coup d'envoi aux paliers 5 et 6 (un kickoff dure ~55 décisions contre ~200 pour un épisode
/// normal : 40 % des épisodes font ~15 % des pas).
pub const KICKOFF_PROB: f32 = 0.4;
/// Durée maximale d'un kickoff (décisions, 6 s).
pub const KICKOFF_MAX_STEPS: u32 = 90;
/// Décisions jouées après le premier toucher (2 s) avant de juger le kickoff.
const KICKOFF_AFTER_TOUCH: u32 = 30;
/// Positions de départ officielles côté bleu (x, y, lacet) ; l'orange est le miroir (-x, -y, lacet + π).
const KICKOFF_SPAWNS: [[f32; 3]; 5] = [
    [-2048.0, -2560.0, std::f32::consts::FRAC_PI_4],
    [2048.0, -2560.0, 3.0 * std::f32::consts::FRAC_PI_4],
    [-256.0, -3840.0, std::f32::consts::FRAC_PI_2],
    [256.0, -3840.0, std::f32::consts::FRAC_PI_2],
    [0.0, -4608.0, std::f32::consts::FRAC_PI_2],
];
/// Temps de référence (s) du fonceur scripté seul, par position (mesurés : test `measure_charger_times`) :
/// barème de la récompense de rapidité.
const KICKOFF_T_REF: [f32; 5] = [2.15, 2.15, 2.65, 2.65, 2.65];
/// Vitesse supersonique (uu/s).
const SUPERSONIC: f32 = 2200.0;
/// Une fraction des kickoffs part avec de l'élan (coup de pouce : raccourcit l'horizon de crédit du flip).
const KICKOFF_HEADSTART_PROB: f32 = 0.1;

/// Issue d'un coup d'envoi vue par l'agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KickoffResult {
    /// But marqué, ou balle repartie dans le camp adverse (y > 300) 2 s après le premier toucher.
    Won,
    /// But encaissé, balle dans notre camp, ou pas de toucher en 6 s.
    Lost,
    Neutral,
}

/// Statistiques d'un coup d'envoi terminé.
#[derive(Clone, Copy, Debug)]
pub struct KickoffStats {
    pub result: KickoffResult,
    /// Temps (s) jusqu'au premier toucher de l'agent, s'il a touché.
    pub first_touch_s: Option<f32>,
    /// Vitesse (uu/s) de l'agent juste avant son premier toucher.
    pub touch_speed: Option<f32>,
    /// L'agent a touché en premier (l'adversaire n'avait pas touché avant lui).
    pub first: bool,
    pub spawn: usize,
}

/// État d'un coup d'envoi en cours.
#[derive(Clone, Debug)]
pub struct KickoffState {
    spawn: usize,
    /// Décisions pendant lesquelles le fonceur adverse attend (varie les 50/50).
    delay: u32,
    /// L'adversaire est le fonceur scripté (sinon il suit l'action donnée à `step_vs`).
    charger: bool,
    first_me: Option<u32>,
    opp_first: bool,
    touch_speed: Option<f32>,
    prev_speed: f32,
    max_speed: f32,
    /// Décision du premier toucher (quelconque) : le kickoff se juge `KICKOFF_AFTER_TOUCH` décisions plus tard.
    first_any: Option<u32>,
    last_opp_touch: Option<u64>,
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
    /// Statistiques du coup d'envoi, renseignées à la fin d'un épisode de kickoff.
    pub kickoff: Option<KickoffStats>,
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
    kickoff: Option<KickoffState>,
    /// Tick de départ du kickoff en cours ; temps jusqu'au premier toucher de l'agent (s) une fois touché.
    kickoff_t0: u64,
    kickoff_first_s: Option<f32>,
    /// Détecteur de mécaniques de la voiture de l'agent (sans effet sur la récompense) et ses compteurs.
    detector: mechanics::Detector,
    mech: mechanics::Counts,
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
            kickoff: None,
            kickoff_t0: 0,
            kickoff_first_s: None,
            detector: mechanics::Detector::new(),
            mech: mechanics::Counts::default(),
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

    /// Compteurs de mécaniques depuis le dernier appel (remis à zéro).
    pub fn take_mechanics(&mut self) -> mechanics::Counts {
        std::mem::take(&mut self.mech)
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
        self.kickoff = None;
        self.detector.reset();
        if self.opp.is_some()
            && self.cfg.kickoff_prob > 0.0
            && self.rng.unit() < self.cfg.kickoff_prob
        {
            return self.reset_kickoff(None);
        }
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

    /// Reset en coup d'envoi : une des 5 positions officielles (tirée si `spawn` est `None`), l'adversaire en
    /// miroir, balle au centre, pads remis. Léger bruit de position/cap (±50 uu, ±0,05 rad) pour que les
    /// épisodes ne soient pas identiques. Sans adversaire, seule la voiture de l'agent est placée.
    pub fn reset_kickoff(&mut self, spawn: Option<usize>) -> [f32; OBS_DIM] {
        let idx = spawn.unwrap_or_else(|| (self.rng.next_u64() % 5) as usize) % 5;
        let [x, y, yaw] = KICKOFF_SPAWNS[idx];
        let seed = (self.rng.next_u64() & 0x7FFF_FFFF) as i32;
        // Pads, balle, invalidation des touchers ; les voitures sont replacées ensuite (par id).
        self.sim.reset_kickoff(seed);
        self.sim.reset_ball([0.0, 0.0, BALL_REST_Z], [0.0; 3]);
        let jitter = |rng: &mut Rng| {
            (
                rng.range(-50.0, 50.0),
                rng.range(-50.0, 50.0),
                rng.range(-0.05, 0.05),
            )
        };
        let (jx, jy, jyaw) = jitter(&mut self.rng);
        let headstart = self.rng.unit() < KICKOFF_HEADSTART_PROB;
        let speed = if headstart {
            self.rng.range(1200.0, 1600.0)
        } else {
            0.0
        };
        let yaw_me = yaw + jyaw;
        let vel = [speed * yaw_me.cos(), speed * yaw_me.sin(), 0.0];
        self.sim
            .reset_car(self.car, [x + jx, y + jy, 17.0], yaw_me, vel, 33.0)
            .expect("voiture connue");
        self.sim.set_controls(self.car, Controls::default()).ok();
        let mut delay = 0;
        let mut charger = false;
        if let Some(opp) = self.opp {
            let (jx, jy, jyaw) = jitter(&mut self.rng);
            self.sim
                .reset_car(
                    opp,
                    [-x + jx, -y + jy, 17.0],
                    yaw + std::f32::consts::PI + jyaw,
                    [0.0; 3],
                    33.0,
                )
                .expect("voiture adverse connue");
            self.sim.set_controls(opp, Controls::default()).ok();
            // Le fonceur scripté tient l'orange sauf si l'évaluation le veut immobile ; au self-play, 30 % des
            // kickoffs restent contre lui (sinon « personne ne va à la balle » devient une stratégie).
            charger = !self.cfg.kickoff_idle_opponent
                && (self.cfg.opponent != Opponent::SelfPlay || self.rng.unit() < 0.3);
            delay = (self.rng.next_u64() % 5) as u32;
        }
        self.steps = 0;
        self.last_touch = self.sim.last_ball_touch_tick(self.car);
        self.kickoff_t0 = self.sim.tick_count();
        self.kickoff_first_s = None;
        self.kickoff = Some(KickoffState {
            spawn: idx,
            delay,
            charger,
            first_me: None,
            opp_first: false,
            touch_speed: None,
            prev_speed: speed,
            max_speed: speed,
            first_any: None,
            last_opp_touch: self.opp.and_then(|o| self.sim.last_ball_touch_tick(o)),
        });
        self.observe()
    }

    /// Action du fonceur scripté orange : cap vers la balle, gaz et boost à fond (les flips et le speed flip lui
    /// sont inconnus : c'est le niveau à battre, pas un modèle de joueur).
    fn charger_action(&mut self, opp: u32) -> usize {
        let s = self.sim.snapshot();
        self.expert_action(charger_steer(&s, opp))
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
        if let Some(opp) = self.opp {
            let charging = self
                .kickoff
                .as_ref()
                .filter(|k| k.charger && self.steps >= k.delay)
                .is_some();
            let a = if charging {
                Some(self.charger_action(opp))
            } else {
                opp_action
            };
            if let Some(a) = a {
                self.sim
                    .set_controls(opp, self.actions.get(a))
                    .expect("voiture adverse connue");
            }
        }
        if self.cfg.detect_mechanics {
            // Détection au tick près : on avance tick par tick (même physique que `step(tick_skip)`).
            for _ in 0..self.cfg.tick_skip {
                self.sim.step(1);
                let (c, b) = (self.sim.car_tick(self.car), self.sim.ball_tick());
                self.detector.tick(&c, &b, &mut self.mech);
            }
        } else {
            self.sim.step(self.cfg.tick_skip);
        }
        self.steps += 1;

        let touch_now = self.sim.last_ball_touch_tick(self.car);
        let touched = touch_now.is_some() && touch_now != self.last_touch;
        self.last_touch = touch_now;

        let snap = self.sim.snapshot();
        let car = &snap.cars[car_index(&snap, self.car)];
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
        let touch_end = touched && self.cfg.end_on_touch && self.kickoff.is_none();
        let mut kickoff_done = None;
        let opp_touch = if self.kickoff.is_some() {
            self.opp.and_then(|o| self.sim.last_ball_touch_tick(o))
        } else {
            None
        };
        if let Some(k) = self.kickoff.as_mut() {
            let speed = (car.vel[0].powi(2) + car.vel[1].powi(2) + car.vel[2].powi(2)).sqrt();
            let step_no = self.steps;
            let skip = self.cfg.tick_skip as f32;
            let t0 = self.kickoff_t0;
            let ball = snap.ball;
            let opp_touched = opp_touch.is_some() && opp_touch != k.last_opp_touch;
            k.last_opp_touch = opp_touch;
            let mut first_touch_s = None;
            if touched && k.first_me.is_none() {
                k.first_me = Some(step_no);
                k.touch_speed = Some(k.prev_speed);
                // Temps exact au tick près ; repli sur la décision si l'horloge de la simulation diffère.
                let upper = step_no as f32 * skip / TICKS_PER_SECOND;
                let t = touch_now
                    .and_then(|tk| tk.checked_sub(t0))
                    .map_or(upper, |d| (d as f32 / TICKS_PER_SECOND).min(upper));
                first_touch_s = Some(t);
                // Récompenses seulement si l'agent gagne la course à la balle (ex æquo : personne).
                if k.first_any.is_none() && !opp_touched {
                    let t_ref = KICKOFF_T_REF[k.spawn];
                    reward += (1.0 + (t_ref - t) / 0.5).clamp(0.0, 2.0);
                    if ball.vel[1] > 0.0 {
                        reward += 0.5 * ((k.prev_speed - 1200.0) / 1000.0).clamp(0.0, 1.0);
                    }
                    reward += (ball.vel[1] / 2000.0).clamp(-1.0, 1.0);
                    if k.max_speed >= SUPERSONIC {
                        reward += 0.5;
                    }
                }
            }
            if opp_touched && k.first_any.is_none() && !touched {
                k.opp_first = true;
                reward -= 1.0;
            }
            if (touched || opp_touched) && k.first_any.is_none() {
                k.first_any = Some(step_no);
            }
            k.prev_speed = speed;
            k.max_speed = k.max_speed.max(speed);
            if first_touch_s.is_some() {
                self.kickoff_first_s = first_touch_s;
            }
            let over = scored
                || step_no >= KICKOFF_MAX_STEPS
                || k.first_any
                    .is_some_and(|f| step_no >= f + KICKOFF_AFTER_TOUCH);
            if over {
                let result = if our_goal || (k.first_me.is_some() && ball.pos[1] > 300.0) {
                    KickoffResult::Won
                } else if own_goal || ball.pos[1] < -300.0 || k.first_me.is_none() {
                    KickoffResult::Lost
                } else {
                    KickoffResult::Neutral
                };
                reward += match result {
                    KickoffResult::Won => 3.0,
                    KickoffResult::Lost => -3.0,
                    KickoffResult::Neutral => 0.0,
                };
                kickoff_done = Some(KickoffStats {
                    result,
                    first_touch_s: self.kickoff_first_s,
                    touch_speed: k.touch_speed,
                    first: k.first_me.is_some() && !k.opp_first && k.first_any == k.first_me,
                    spawn: k.spawn,
                });
            }
        }
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
            terminated: scored || touch_end || kickoff_done.is_some(),
            truncated: self.steps >= self.cfg.max_steps,
            touched,
            outcome,
            kickoff: kickoff_done,
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
        let other = self.opp.map(|o| car_index(&s, o));
        build_obs(&s, car_index(&s, self.car), other, false)
    }

    /// Observation du point de vue de l'adversaire (monde retourné de 180° autour de Z : il vise aussi +Y).
    /// Zéros s'il n'y a pas d'adversaire.
    pub fn observe_opponent(&mut self) -> [f32; OBS_DIM] {
        let Some(opp) = self.opp else {
            return [0.0; OBS_DIM];
        };
        let s = self.sim.snapshot();
        build_obs(&s, car_index(&s, opp), Some(car_index(&s, self.car)), true)
    }

    pub fn has_opponent(&self) -> bool {
        self.opp.is_some()
    }
}

/// Direction (-1, 0 ou 1) du fonceur scripté de la voiture `id` : cap vers la balle, zone morte de 0,1 rad.
pub fn charger_steer(s: &sim_core::Snapshot, id: u32) -> f32 {
    let me = &s.cars[car_index(s, id)];
    let (dx, dy) = (s.ball.pos[0] - me.pos[0], s.ball.pos[1] - me.pos[1]);
    let cross = me.forward[0] * dy - me.forward[1] * dx;
    let dot = me.forward[0] * dx + me.forward[1] * dy;
    let ang = cross.atan2(dot);
    if ang.abs() > 0.1 { ang.signum() } else { 0.0 }
}

/// Contrôles du fonceur scripté (gaz et boost à fond, cap vers la balle) : identique à l'action experte de la table.
pub fn charger_controls(s: &sim_core::Snapshot, id: u32) -> Controls {
    let steer = charger_steer(s, id);
    Controls {
        throttle: 1.0,
        steer,
        yaw: steer,
        boost: true,
        ..Default::default()
    }
}

/// Indice de la voiture `id` dans le snapshot : ne jamais supposer un ordre (voir `Sim::snapshot`).
fn car_index(s: &sim_core::Snapshot, id: u32) -> usize {
    s.cars
        .iter()
        .position(|c| c.id == id)
        .expect("voiture connue")
}

fn push3(v: &mut Vec<f32>, x: [f32; 3], sc: [f32; 3]) {
    v.extend((0..3).map(|k| x[k] / sc[k]));
}

/// Construit l'observation de la voiture `me` ; `other` = voiture adverse éventuelle ; `mirror` retourne le monde
/// de 180° autour de Z (x → -x, y → -y). Publique : le client de jeu l'appelle sur ses snapshots pour que le bot
/// voie exactement les observations de l'entraînement (bleu : `mirror = false` ; orange : `mirror = true`).
pub fn build_obs(
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
    /// Renseigné si l'épisode était un coup d'envoi.
    pub kickoff: Option<KickoffStats>,
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
    /// Mécaniques détectées pendant ce pas (tous envs, voiture de l'agent).
    pub mechanics: mechanics::Counts,
}

struct Reply {
    first: usize,
    obs: Vec<f32>,
    opp_obs: Vec<f32>,
    reward: Vec<f32>,
    terminated: Vec<bool>,
    truncated: Vec<bool>,
    finished: Vec<EpisodeStats>,
    mechanics: mechanics::Counts,
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
            out.mechanics.add(&r.mechanics);
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
            mechanics: mechanics::Counts::default(),
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
                            kickoff: r.kickoff,
                        });
                        (ret[i], len[i], touched[i]) = (0.0, 0, false);
                        reply.obs.extend_from_slice(&e.reset());
                    } else {
                        reply.obs.extend_from_slice(&e.observe());
                    }
                    if e.has_opponent() {
                        reply.opp_obs.extend_from_slice(&e.observe_opponent());
                    }
                    reply.mechanics.add(&e.take_mechanics());
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

    #[test]
    fn observation_and_reward_follow_the_agent_car_with_an_opponent() {
        assert!(setup());
        for seed in 0..8 {
            let cfg = EnvConfig {
                kickoff_prob: 0.0,
                ..EnvConfig::for_stage(5)
            };
            let mut e = Env::new(cfg, seed).unwrap();
            let s = e.sim.snapshot();
            let me = s.cars.iter().find(|c| c.id == e.car).unwrap();
            let them = s.cars.iter().find(|c| Some(c.id) == e.opp).unwrap();
            assert!(me.blue && !them.blue);
            let o = e.observe();
            for k in 0..3 {
                assert!(
                    (o[k] * POS_SCALE[k] - me.pos[k]).abs() < 1.0,
                    "moi, seed {seed}"
                );
                let d = o[OBS_DIM_V1 + k] * POS_SCALE[k] - them.pos[k];
                assert!(d.abs() < 1.0, "adversaire, seed {seed}");
            }
            // L'adversaire statique est devant sa cage (+Y) ; vu de son côté (monde retourné), il est en -Y.
            assert!((o[OBS_DIM_V1 + 1] * POS_SCALE[1] - 4300.0).abs() < 1.0);
            let oo = e.observe_opponent();
            assert!((oo[1] * POS_SCALE[1] + 4300.0).abs() < 1.0);
            assert!((oo[OBS_DIM_V1 + 1] * POS_SCALE[1] + me.pos[1]).abs() < 1.0);
            // La récompense d'approche se calcule sur la voiture de l'agent : l'adversaire immobile la mettrait à 0.
            let mut moved = 0.0f32;
            for _ in 0..20 {
                let a = e.expert_action(1.0);
                moved += e.step(a).reward.abs();
            }
            assert!(moved > 0.0, "seed {seed}");
        }
    }

    #[test]
    fn stages_are_progressively_harder() {
        let c = |n| EnvConfig::for_stage(n);
        assert_eq!(c(2).ball_speed_max, 0.0);
        assert!(c(3).ball_speed_max > 0.0);
        assert_eq!(c(4).spawn, SpawnMode::Hard);
        assert_eq!(c(5).opponent, Opponent::Static);
        assert_eq!(c(6).opponent, Opponent::SelfPlay);
        assert_eq!(c(99).opponent, Opponent::SelfPlay);
        assert!((2..=6).all(|n| c(n).own_goal_penalty > 0.0 && c(n).goal_reward > 0.0));
    }

    #[test]
    fn moving_ball_hard_spawns_and_opponent_stay_finite() {
        assert!(setup());
        for stage in 3..=6 {
            let mut e = Env::new(EnvConfig::for_stage(stage), u64::from(stage)).unwrap();
            for _ in 0..5 {
                let o = e.reset();
                assert!(o.iter().all(|x| x.is_finite()), "palier {stage}");
            }
            let o = e.observe();
            let present = o[OBS_DIM - 1];
            assert_eq!(present, f32::from(u8::from(stage >= 5)), "palier {stage}");
            for _ in 0..40 {
                let r = e.step_vs(40, Some(40));
                assert!(r.reward.is_finite());
            }
            assert_eq!(e.has_opponent(), stage >= 5);
        }
    }

    #[test]
    fn opponent_block_is_zero_without_opponent_and_view_is_mirrored() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::for_stage(2), 4).unwrap();
        assert!(e.observe()[OBS_DIM_V1..].iter().all(|x| *x == 0.0));
        e.set_config(EnvConfig::for_stage(6));
        e.reset();
        let (a, b) = (e.observe(), e.observe_opponent());
        // Position de la voiture de l'un = position adverse (inversée en x, y) vue par l'autre.
        assert!((a[0] + b[OBS_DIM_V1]).abs() < 1e-5 && (a[1] + b[OBS_DIM_V1 + 1]).abs() < 1e-5);
        assert!((a[18] + b[18]).abs() < 1e-5, "la balle est retournée");
    }

    #[test]
    fn own_goal_is_penalized_and_reported() {
        assert!(setup());
        let cfg = EnvConfig::for_stage(2);
        let mut e = Env::new(cfg.clone(), 5).unwrap();
        // Balle lancée vers le fond côté -Y : but contre son camp.
        e.sim.reset_ball([0.0, -4000.0, 93.0], [0.0, -4000.0, 0.0]);
        let mut last = StepInfo::default();
        for _ in 0..120 {
            last = e.step(0);
            if last.terminated {
                break;
            }
        }
        assert!(last.terminated, "la balle devrait entrer dans le but");
        assert_eq!(last.outcome, Outcome::OwnGoal);
        assert!(
            last.reward <= -cfg.own_goal_penalty + 0.5,
            "{}",
            last.reward
        );
    }

    #[test]
    fn scoring_in_the_opposite_goal_is_a_goal() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::for_stage(2), 6).unwrap();
        e.sim.reset_ball([0.0, 4000.0, 93.0], [0.0, 4000.0, 0.0]);
        let mut last = StepInfo::default();
        for _ in 0..120 {
            last = e.step(0);
            if last.terminated {
                break;
            }
        }
        assert_eq!(last.outcome, Outcome::Goal);
    }

    #[test]
    fn vec_env_reports_outcomes_and_config_switch_resets() {
        assert!(setup());
        let mut v = VecEnv::new(4, 2, EnvConfig::for_stage(2), 9).unwrap();
        v.reset();
        let obs = v.set_config(&EnvConfig::for_stage(6));
        assert_eq!(obs.len(), 4 * OBS_DIM);
        assert_eq!(v.opp_obs().len(), 4 * OBS_DIM);
        let s = v.step_vs(&[0; 4], Some(&[0; 4]));
        assert_eq!(s.opp_obs.len(), 4 * OBS_DIM);
        let obs = v.set_config(&EnvConfig::for_stage(2));
        assert_eq!(obs.len(), 4 * OBS_DIM);
        assert!(v.opp_obs().is_empty());
    }

    /// Joue un kickoff complet avec `policy` (observation → action) et renvoie (retour, statistiques).
    fn play_kickoff(
        e: &mut Env,
        spawn: usize,
        policy: &dyn Fn(&[f32]) -> usize,
    ) -> (f32, KickoffStats) {
        let mut obs = e.reset_kickoff(Some(spawn));
        let mut ret = 0.0;
        for _ in 0..KICKOFF_MAX_STEPS + 5 {
            let r = e.step_vs(policy(&obs), None);
            ret += r.reward;
            if let Some(k) = r.kickoff {
                return (ret, k);
            }
            obs = e.observe();
        }
        panic!("le kickoff aurait dû se terminer");
    }

    fn charge(e: &Env) -> impl Fn(&[f32]) -> usize + use<> {
        let table = e.actions.clone();
        move |o: &[f32]| {
            let (rx, ry) = (o[24] * REL_POS_SCALE[0], o[25] * REL_POS_SCALE[1]);
            let (fx, fy) = (o[9], o[10]);
            let ang = (fx * ry - fy * rx).atan2(fx * rx + fy * ry);
            let steer = if ang.abs() > 0.1 { ang.signum() } else { 0.0 };
            (0..table.len())
                .find(|&i| {
                    let c = table.get(i);
                    c.throttle == 1.0 && c.steer == steer && c.boost && !c.jump && c.pitch == 0.0
                })
                .unwrap()
        }
    }

    #[test]
    fn kickoff_only_from_stage_5_and_spawns_are_official_and_mirrored() {
        assert!(setup());
        assert!((0..5).all(|n| EnvConfig::for_stage(n).kickoff_prob == 0.0));
        assert!((5..=6).all(|n| EnvConfig::for_stage(n).kickoff_prob > 0.0));
        let mut e = Env::new(EnvConfig::kickoff_eval(false), 1).unwrap();
        for (spawn, &[x, y, _]) in KICKOFF_SPAWNS.iter().enumerate() {
            e.reset_kickoff(Some(spawn));
            let s = e.sim.snapshot();
            let me = &s.cars[car_index(&s, e.car)];
            let them = &s.cars[car_index(&s, e.opp.unwrap())];
            assert!((me.pos[0] - x).abs() <= 51.0 && (me.pos[1] - y).abs() <= 51.0);
            assert!((them.pos[0] + x).abs() <= 101.0 && (them.pos[1] + y).abs() <= 101.0);
            assert!(me.blue && !them.blue);
            assert_eq!(s.ball.pos, [0.0, 0.0, BALL_REST_Z]);
            assert!((me.boost - 33.0).abs() < 1.0);
        }
    }

    #[test]
    fn kickoff_episodes_are_mixed_by_probability() {
        assert!(setup());
        let cfg = EnvConfig {
            kickoff_prob: 0.5,
            ..EnvConfig::for_stage(5)
        };
        let mut e = Env::new(cfg, 3).unwrap();
        let n = (0..200)
            .filter(|_| {
                e.reset();
                e.kickoff.is_some()
            })
            .count();
        assert!((60..140).contains(&n), "{n} kickoffs sur 200");
    }

    #[test]
    fn charging_agent_touches_and_idle_agent_loses_without_free_reward() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::kickoff_eval(false), 7).unwrap();
        let go = charge(&e);
        let (mut best, mut idle_worst) = (f32::MIN, f32::MIN);
        let mut touched = 0;
        for spawn in 0..5 {
            let (ret, k) = play_kickoff(&mut e, spawn, &go);
            assert!(
                k.first_touch_s.is_none_or(|t| (0.5..6.0).contains(&t)),
                "{k:?}"
            );
            touched += u32::from(k.first_touch_s.is_some());
            best = best.max(ret);
            // Immobile (gaz nul, sans boost) : jamais de toucher, kickoff perdu, retour négatif.
            let (r, k) = play_kickoff(&mut e, spawn, &|_| 4);
            assert_eq!(k.first_touch_s, None, "action 4 = gaz nul, sans boost");
            assert_eq!(k.result, KickoffResult::Lost);
            idle_worst = idle_worst.max(r);
        }
        assert!(touched >= 4, "le fonceur doit toucher : {touched}/5");
        assert!(
            idle_worst < 0.0,
            "ne rien faire ne doit rien rapporter : {idle_worst}"
        );
        assert!(best > 0.0, "{best}");
    }

    #[test]
    fn kickoff_reward_is_bounded_below_a_goal() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::kickoff_eval(true), 8).unwrap();
        let go = charge(&e);
        for spawn in 0..5 {
            let (ret, k) = play_kickoff(&mut e, spawn, &go);
            // Hors but (10 + 3), les récompenses de kickoff restent sous la valeur d'un but.
            let shaped = if k.result == KickoffResult::Won && ret > 10.0 {
                ret - 10.0
            } else {
                ret
            };
            assert!(
                shaped < 10.0,
                "un kickoff ne vaut jamais un but : {ret} ({k:?})"
            );
            assert!(k.first, "adversaire immobile : l'agent touche en premier");
        }
    }

    #[test]
    fn kickoff_step_with_selfplay_opponent_actions_stays_finite() {
        assert!(setup());
        let mut e = Env::new(
            EnvConfig {
                kickoff_prob: 1.0,
                ..EnvConfig::for_stage(6)
            },
            9,
        )
        .unwrap();
        for _ in 0..4 {
            e.reset();
            for _ in 0..100 {
                let r = e.step_vs(30, Some(25));
                assert!(r.reward.is_finite());
                if r.terminated {
                    assert!(r.kickoff.is_some());
                    break;
                }
            }
        }
    }

    #[test]
    #[ignore = "mesure : cargo test -p rl-env measure_charger -- --ignored --nocapture"]
    fn measure_charger_times() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::kickoff_eval(true), 11).unwrap();
        let go = charge(&e);
        for spawn in 0..5 {
            let ts: Vec<String> = (0..8)
                .map(|_| {
                    let (_, k) = play_kickoff(&mut e, spawn, &go);
                    format!("{:.2}", k.first_touch_s.unwrap_or(-1.0))
                })
                .collect();
            println!("spawn {spawn}: {}", ts.join(" "));
        }
    }
}
