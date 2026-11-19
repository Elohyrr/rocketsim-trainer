//! Environnement d'apprentissage par renforcement headless, dans le style RLGym.
//!
//! - [`ActionTable`] : table discrète d'actions (contrôles de voiture), comme la `LookupTableAction` de RLGym ;
//! - [`Env`] : un épisode selon le palier de curriculum (voiture bleue, balle, voiture orange aux paliers 5 et 6),
//!   action repeat configurable ;
//! - [`VecEnv`] : N environnements répartis sur des threads, remise à zéro automatique en fin d'épisode.
//!
//! Les unités sont celles de RocketSim (unreal units, 120 Hz).

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use sim_core::clip::{Clip, ClipRecorder};
use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

pub mod drill;
pub use drill::{BonusLedger, DrillReport};
pub use scenarios::Drill;
pub use sim_core::SimError;
pub use sim_core::clip;

/// Dimension de l'observation d'avant l'adversaire (checkpoints historiques, voir `docs/curriculum.md`).
pub const OBS_DIM_V1: usize = 33;
/// Dimension de l'observation d'avant la v2 : [`OBS_DIM_V1`] + bloc adversaire (position, vitesse, cap, présence).
/// Le bloc adversaire est nul sans adversaire : un réseau dont les poids d'entrée de ce bloc sont nuls (checkpoint
/// historique agrandi, voir `docs/curriculum.md`) se comporte exactement comme avant.
pub const OBS_DIM_V2: usize = OBS_DIM_V1 + 10;
/// Nombre de boosts au sol en soccar (observés dans l'ordre fixe de l'arène).
pub const N_PADS: usize = 34;
/// Dimension du vecteur d'observation (v3) : [`OBS_DIM_V2`] + 34 boosts + boost adverse + dernière action (8) + état du flip (3).
/// Les entrées ajoutées ont des poids nuls au chargement d'un réseau plus ancien (voir `docs/observation.md`).
pub const OBS_DIM: usize = OBS_DIM_V2 + N_PADS + 1 + 8 + 3;
/// Nombre d'actions de la table avant le frein à main au sol (checkpoints d'avant la v3).
pub const N_ACTIONS_V1: usize = 78;
/// Biais de sortie des actions ajoutées au chargement d'un réseau plus ancien : peu probables au départ.
pub const NEW_ACTION_BIAS: f32 = -4.0;
/// Récupération d'un boost : durée avant réapparition (s) d'une petite et d'une grosse.
const PAD_COOLDOWN_SMALL: f32 = 4.0;
const PAD_COOLDOWN_BIG: f32 = 10.0;
/// Temps depuis le saut au-delà duquel l'observation sature (s).
const JUMP_TIME_SCALE: f32 = 1.5;
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
        // Frein à main au sol (virages glissés) : les 12 actions du sol, en fin de table pour ne pas décaler
        // les indices des politiques existantes.
        for i in 0..12 {
            let mut c = controls[i];
            c.handbrake = true;
            controls.push(c);
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
    /// Boost initial tiré uniformément dans 0–100 (voiture et adversaire) à la place de `start_boost`, sauf
    /// coups d'envoi (33, comme le vrai jeu) et drills.
    pub random_start_boost: bool,
    /// Poids `c` de la récompense de ramassage `c × (√boost_t − √boost_t−1)` (boost dans 0–1), sur les gains
    /// seulement : aucune pénalité de dépense. 0 = désactivée. Hors drills.
    pub boost_reward: f32,
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
    /// Enregistre un clip rejouable quand une mécanique (hors simple toucher) est reconnue ; demande
    /// `detect_mechanics`. Voir [`Env::take_clips`].
    pub record_clips: bool,
    /// Garde les règles d'origine (v1) du détecteur de mécaniques au lieu des règles strictes (v2).
    pub detector_v1: bool,
    /// Part des épisodes qui sont des drills de mécaniques (0 = désactivé ; sans effet quand `end_on_touch`).
    /// Voir `docs/drills.md` : sans adversaire actif, exclus des stats de curriculum, bonus de méca seulement ici.
    pub drill_prob: f32,
    /// Poids de tirage des drills (même ordre que [`Drill::ALL`], voir `scenarios::parse_weights`).
    pub drill_weights: [f32; scenarios::DRILL_COUNT],
    /// Enregistre aussi les clips des drills de dribble (jamais les autres).
    pub drill_clips: bool,
    /// Registre partagé des bonus de méca payés (décroissance sur 1 h glissante) ; à partager entre les paliers.
    pub bonus_ledger: Arc<BonusLedger>,
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
            random_start_boost: false,
            boost_reward: 0.0,
            goal_reward: 0.0,
            own_goal_penalty: 0.0,
            ball_speed_max: 0.0,
            spawn: SpawnMode::Random,
            opponent: Opponent::None,
            kickoff_prob: 0.0,
            kickoff_idle_opponent: false,
            detect_mechanics: true,
            record_clips: false,
            detector_v1: false,
            drill_prob: 0.0,
            drill_weights: Drill::DEFAULT_WEIGHTS,
            drill_clips: false,
            bonus_ledger: Arc::default(),
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
                random_start_boost: true,
                boost_reward: BOOST_REWARD,
                ..scoring
            },
            _ => Self {
                ball_speed_max: 1000.0,
                spawn: SpawnMode::Mixed,
                max_steps: 400,
                opponent: Opponent::SelfPlay,
                kickoff_prob: KICKOFF_PROB,
                random_start_boost: true,
                boost_reward: BOOST_REWARD,
                // Somme nulle : but marqué +10, but encaissé −10 ; plus de prime au toucher (un comportement,
                // pas un résultat) et seulement l'approche, faible.
                own_goal_penalty: scoring.goal_reward,
                touch_reward: 0.0,
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
/// Poids de la récompense de ramassage de boost aux paliers 5 et 6 : une grosse prise à vide vaut 0,2 (un
/// toucher vaut 1 au palier 5), une petite prise à vide ≈ 0,07, une petite prise à 33 de boost ≈ 0,02.
pub const BOOST_REWARD: f32 = 0.2;
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

/// Statistiques de boost de la voiture de l'agent (épisodes de match, hors drills), depuis la dernière reprise.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoostReport {
    /// Boosts ramassés (le ramassage a fait monter le boost de l'agent).
    pub pickups_small: u64,
    pub pickups_big: u64,
    /// Somme des niveaux de boost (0–100) sur les décisions, et nombre de décisions.
    pub boost_sum: f64,
    pub steps: u64,
    /// Décisions prises sans aucun boost.
    pub empty_steps: u64,
}

impl BoostReport {
    pub fn add(&mut self, o: &BoostReport) {
        self.pickups_small += o.pickups_small;
        self.pickups_big += o.pickups_big;
        self.boost_sum += o.boost_sum;
        self.steps += o.steps;
        self.empty_steps += o.empty_steps;
    }
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
    /// Enregistreur de clips, clips finis en attente d'être repris, dernier contrôle de l'adversaire.
    rec: ClipRecorder,
    clips: Vec<Clip>,
    opp_ctl: Controls,
    /// Clip construit qui attend sa qualité (mesurée 3 s après la mécanique) et le suivi correspondant.
    clip_wait: Option<Clip>,
    quality: Option<mechanics::QualityTracker>,
    /// Valeur et support de la mécanique qui a déclenché le clip en attente.
    clip_detail: Option<clip::ClipDetail>,
    /// Drill en cours (None en match) et son bonus ; compteurs de drill depuis la dernière reprise.
    drill: Option<(Drill, drill::EpisodeBonus)>,
    drill_report: DrillReport,
    /// Boost de l'agent et état des pads à la décision précédente ; compteurs de boost depuis la dernière reprise.
    prev_boost: f32,
    prev_pads: Vec<bool>,
    boost_report: BoostReport,
}

impl Env {
    /// `init_meshes` doit avoir réussi avant (une fois par processus).
    pub fn new(cfg: EnvConfig, seed: u64) -> Result<Self, SimError> {
        let (sim, car, opp) = Self::build_sim(&cfg)?;
        let cfg_skip = cfg.tick_skip;
        let detector = mechanics::Detector::with_mode(Self::mode(&cfg));
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
            detector,
            mech: mechanics::Counts::default(),
            rec: ClipRecorder::new(cfg_skip),
            clips: Vec::new(),
            opp_ctl: Controls::default(),
            clip_wait: None,
            quality: None,
            clip_detail: None,
            drill: None,
            drill_report: DrillReport::default(),
            prev_boost: 0.0,
            prev_pads: Vec::new(),
            boost_report: BoostReport::default(),
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

    fn mode(cfg: &EnvConfig) -> mechanics::Mode {
        if cfg.detector_v1 {
            mechanics::Mode::V1
        } else {
            mechanics::Mode::V2
        }
    }

    /// Termine le clip en attente : y joint la qualité mesurée (partielle si l'épisode s'arrête avant 3 s).
    fn flush_clip(&mut self) {
        let q = self.quality.take();
        if let (Some(mut c), Some(t)) = (self.clip_wait.take(), q) {
            let q = t.quality();
            c.detector = if self.cfg.detector_v1 { 1 } else { 2 };
            c.detail = self.clip_detail.take();
            c.quality = Some(clip::ClipQuality {
                ball_speed_to_goal: q.speed,
                goal_3s: q.goal,
                score: q.score(),
            });
            self.clips.push(c);
        }
    }

    /// Compteurs de drill depuis le dernier appel (remis à zéro).
    /// Reprend (et remet à zéro) les statistiques de boost.
    pub fn take_boost_report(&mut self) -> BoostReport {
        std::mem::take(&mut self.boost_report)
    }

    pub fn take_drill_report(&mut self) -> DrillReport {
        std::mem::take(&mut self.drill_report)
    }

    /// Drill de l'épisode en cours (None en match).
    pub fn drill(&self) -> Option<Drill> {
        self.drill.as_ref().map(|d| d.0)
    }

    /// Clips finis depuis le dernier appel (vide sans `record_clips`).
    pub fn take_clips(&mut self) -> Vec<Clip> {
        std::mem::take(&mut self.clips)
    }

    /// Fin d'épisode : fige le clip en attente (la simulation est encore dans l'état final) et repart à zéro.
    fn end_episode(&mut self) {
        if self.cfg.record_clips
            && let Some(c) = self.rec.finish(&mut self.sim)
        {
            self.clip_wait = Some(c);
        }
        self.flush_clip();
        self.clip_wait = None;
        self.quality = None;
        self.clip_detail = None;
        self.rec.reset();
        self.opp_ctl = Controls::default();
        if let Some((d, bonus)) = self.drill.take() {
            self.drill_report.episodes[d.index()] += 1;
            if bonus.success {
                self.drill_report.success[d.index()] += 1;
            }
            if bonus.recovery.as_ref().is_some_and(|t| t.verdict.is_none()) {
                self.drill_report.recovery[drill::RecoveryOutcome::None.index()] += 1;
            }
        }
    }

    /// Change la configuration (palier de curriculum). Si la présence d'un adversaire change, la simulation est
    /// reconstruite ; dans tous les cas l'appelant doit faire un [`Env::reset`] ensuite.
    pub fn set_config(&mut self, cfg: EnvConfig) {
        let had = self.opp.is_some();
        let wants = cfg.opponent != Opponent::None;
        self.rec = ClipRecorder::new(cfg.tick_skip);
        self.detector = mechanics::Detector::with_mode(Self::mode(&cfg));
        self.clip_wait = None;
        self.quality = None;
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
        self.end_episode();
        self.kickoff = None;
        self.detector.reset();
        if self.cfg.drill_prob > 0.0
            && !self.cfg.end_on_touch
            && self.rng.unit() < self.cfg.drill_prob
        {
            return self.reset_drill();
        }
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
        let boost = self.start_boost();
        self.sim
            .reset_car(self.car, car_pos, yaw, [0.0; 3], boost)
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
            let boost = self.start_boost();
            self.sim
                .reset_car(opp, pos, yaw, [0.0; 3], boost)
                .expect("voiture adverse connue");
            self.sim.set_controls(opp, Controls::default()).ok();
        }
        self.steps = 0;
        self.last_touch = self.sim.last_ball_touch_tick(self.car);
        self.observe_synced()
    }

    /// Boost de départ d'un épisode de match : `start_boost`, ou tiré dans 0–100 si `random_start_boost`.
    fn start_boost(&mut self) -> f32 {
        if self.cfg.random_start_boost {
            self.rng.range(0.0, 100.0)
        } else {
            self.cfg.start_boost
        }
    }

    /// Observation de fin de reset : mémorise aussi le boost et les pads (référence de la récompense de ramassage).
    fn observe_synced(&mut self) -> [f32; OBS_DIM] {
        let s = self.sim.snapshot();
        let me = car_index(&s, self.car);
        self.prev_boost = s.cars[me].boost;
        self.prev_pads = s.pads.iter().map(|p| p.active).collect();
        let other = self.opp.map(|o| car_index(&s, o));
        build_obs(&s, me, other, false)
    }

    /// Reset en drill : état de départ tiré par `scenarios` (drill uniforme parmi [`Drill::ALL`]). L'adversaire,
    /// s'il existe, est garé immobile près de sa cage et ignore la politique adverse : on montre la mécanique sans
    /// opposition (voir `docs/drills.md`).
    pub fn reset_drill(&mut self) -> [f32; OBS_DIM] {
        let d = Drill::pick_weighted(&self.cfg.drill_weights, self.rng.unit());
        self.reset_drill_with(d)
    }

    /// Comme [`Env::reset_drill`], pour un drill donné.
    pub fn reset_drill_with(&mut self, d: Drill) -> [f32; OBS_DIM] {
        self.end_episode();
        self.kickoff = None;
        self.detector.reset();
        let rng = &mut self.rng;
        let s = scenarios::sample(d, &mut || rng.unit());
        match s.car_axes {
            Some(axes) => self
                .sim
                .place_car_free(
                    self.car,
                    s.car_pos,
                    s.car_vel,
                    s.car_ang_vel,
                    axes,
                    s.car_boost,
                    s.flip_available,
                )
                .expect("voiture connue"),
            None => self
                .sim
                .reset_car(self.car, s.car_pos, s.car_yaw, s.car_vel, s.car_boost)
                .expect("voiture connue"),
        }
        self.sim.reset_ball(s.ball_pos, s.ball_vel);
        self.sim.set_controls(self.car, Controls::default()).ok();
        if let Some(opp) = self.opp {
            let (pos, yaw, boost) = match &s.opponent {
                Some(o) => (o.pos, o.yaw, o.boost),
                None => {
                    let x = self.rng.range(-1500.0, 1500.0);
                    ([x, 4600.0, 17.0], -std::f32::consts::FRAC_PI_2, 33.0)
                }
            };
            self.sim
                .reset_car(opp, pos, yaw, [0.0; 3], boost)
                .expect("voiture adverse connue");
            self.sim.set_controls(opp, Controls::default()).ok();
        }
        self.steps = 0;
        self.last_touch = self.sim.last_ball_touch_tick(self.car);
        let mut bonus = drill::EpisodeBonus::default();
        if d == Drill::Recovery {
            bonus.recovery = Some(drill::RecoveryTrack::new(
                self.sim.tick_count(),
                s.car_pos[2],
                s.car_vel[2],
            ));
        }
        if d == Drill::Keeper {
            bonus.keeper = Some(drill::KeeperTrack::new(s.ball_pos, s.ball_vel));
        }
        self.drill = Some((d, bonus));
        self.observe_synced()
    }

    /// Reset en coup d'envoi : une des 5 positions officielles (tirée si `spawn` est `None`), l'adversaire en
    /// miroir, balle au centre, pads remis. Léger bruit de position/cap (±50 uu, ±0,05 rad) pour que les
    /// épisodes ne soient pas identiques. Sans adversaire, seule la voiture de l'agent est placée.
    pub fn reset_kickoff(&mut self, spawn: Option<usize>) -> [f32; OBS_DIM] {
        self.end_episode();
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
        self.observe_synced()
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
            let a = if self.drill.as_ref().is_some_and(|d| !d.0.active_opponent()) {
                None
            } else if charging {
                Some(self.charger_action(opp))
            } else {
                opp_action
            };
            if let Some(a) = a {
                self.opp_ctl = self.actions.get(a);
                self.sim
                    .set_controls(opp, self.opp_ctl)
                    .expect("voiture adverse connue");
            }
        }
        let in_drill = self.drill.is_some();
        // Les clips ne montrent que des matchs : une méca provoquée par un état de départ n'est pas une découverte.
        let clip_drill =
            self.cfg.drill_clips && self.drill.as_ref().is_some_and(|d| d.0.is_dribble());
        let recording =
            self.cfg.detect_mechanics && self.cfg.record_clips && (!in_drill || clip_drill);
        let mut seen = mechanics::Counts::default();
        let mut recovery_done = false;
        if recording {
            // Ids triés = ordre d'ajout : l'agent d'abord, puis l'adversaire.
            let mine = self.actions.get(action);
            if self.opp.is_some() {
                self.rec.begin_step(&mut self.sim, &[mine, self.opp_ctl]);
            } else {
                self.rec.begin_step(&mut self.sim, &[mine]);
            }
        }
        if self.cfg.detect_mechanics {
            // Détection au tick près : on avance tick par tick (même physique que `step(tick_skip)`).
            for _ in 0..self.cfg.tick_skip {
                self.sim.step(1);
                let (c, b) = (self.sim.car_tick(self.car), self.sim.ball_tick());
                self.detector.tick(&c, &b, &mut seen);
                if let Some(t) = self.quality.as_mut() {
                    t.observe(&b);
                }
            }
            if in_drill {
                self.drill_report.mechanics.add(&seen);
            } else {
                self.mech.add(&seen);
            }
            if recording {
                // Tout sauf le simple toucher (trop fréquent) ; la plus rare des mécaniques vues ce pas.
                let rarest = mechanics::Mechanic::ALL
                    .into_iter()
                    .filter(|&m| m != mechanics::Mechanic::Touch && seen.get(m) > 0)
                    .max_by_key(|m| (m.clip_priority(), m.index()));
                if let Some(m) = rarest
                    && self.rec.trigger(m.label())
                {
                    self.quality = Some(mechanics::QualityTracker::start(true));
                    let d = self.detector.detail(m);
                    self.clip_detail = Some(clip::ClipDetail {
                        value: d.value,
                        support: d.support.label().to_string(),
                    });
                }
                if let Some(c) = self.rec.end_step(&mut self.sim) {
                    self.clip_wait = Some(c);
                }
                if self.clip_wait.is_some() && self.quality.is_some_and(|t| t.done()) {
                    self.flush_clip();
                }
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
        if !in_drill {
            reward += self.boost_step(&snap, car_index(&snap, self.car));
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
        let mut drill_done = false;
        if let Some((d, bonus)) = self.drill.as_mut() {
            let d = *d;
            // Bonus de méca : seulement en drill, plafonné à 10 % d'un but.
            let cap = drill::MAX_FRACTION_OF_GOAL * self.cfg.goal_reward;
            let tick = self.sim.tick_count();
            let ledger = &self.cfg.bonus_ledger;
            reward += bonus.on_step(
                &seen,
                tick,
                (snap.ball.pos, snap.ball.vel),
                cap,
                ledger,
                &mut self.drill_report,
            );
            if our_goal {
                reward += bonus.on_goal(tick, cap, ledger, &mut self.drill_report);
            }
            if let Some(track) = bonus.recovery.as_mut() {
                let c = self.sim.car_tick(self.car);
                let (verdict, done) = track.step(&drill::RecoveryObs {
                    tick,
                    on_ground: c.on_ground,
                    world_contact: c.world_contact,
                    car_pos: c.pos,
                    car_vel: c.vel,
                    forward: c.forward,
                    ball_pos: snap.ball.pos,
                });
                if let Some(v) = verdict {
                    self.drill_report.recovery[v.index()] += 1;
                    if v == drill::RecoveryOutcome::Good {
                        bonus.success = true;
                        reward +=
                            ledger.pay_slot(drill::RECOVERY_SLOT, drill::RECOVERY_BONUS.min(cap));
                    }
                }
                recovery_done = done;
            }
            let (extra, done) = bonus.on_events(
                d,
                &drill::StepEvents {
                    tick,
                    touched,
                    ball: (snap.ball.pos, snap.ball.vel),
                    car_airborne: !car.on_ground,
                    our_goal,
                    own_goal,
                    seen: &seen,
                },
                cap,
                ledger,
                &mut self.drill_report,
            );
            reward += extra;
            drill_done = done;
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
            terminated: scored
                || touch_end
                || kickoff_done.is_some()
                || recovery_done
                || drill_done,
            truncated: self.steps >= self.max_steps(),
            touched,
            outcome,
            kickoff: kickoff_done,
        }
    }

    /// Récompense et statistiques de boost de cette décision (match seulement) : `c × (√b_t − √b_t−1)` sur les
    /// gains, rien sur les pertes ; le ramassage est attribué à la pad inactive la plus proche (≤ 500 uu).
    fn boost_step(&mut self, snap: &sim_core::Snapshot, me: usize) -> f32 {
        let car = &snap.cars[me];
        let (now, prev) = (car.boost, self.prev_boost);
        let rep = &mut self.boost_report;
        rep.steps += 1;
        rep.boost_sum += f64::from(now);
        rep.empty_steps += u64::from(now < 0.5);
        let mut r = 0.0;
        if now > prev + 0.01 {
            r = self.cfg.boost_reward * ((now / 100.0).sqrt() - (prev / 100.0).sqrt());
            let taken = snap
                .pads
                .iter()
                .enumerate()
                .filter(|(i, p)| !p.active && self.prev_pads.get(*i).copied().unwrap_or(false))
                .map(|(_, p)| {
                    let d = (p.pos[0] - car.pos[0]).hypot(p.pos[1] - car.pos[1]);
                    (d, p.big)
                })
                .filter(|(d, _)| *d <= 500.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match taken {
                Some((_, true)) => rep.pickups_big += 1,
                Some((_, false)) => rep.pickups_small += 1,
                None => {}
            }
        }
        self.prev_boost = now;
        self.prev_pads.clear();
        self.prev_pads.extend(snap.pads.iter().map(|p| p.active));
        r
    }

    /// Horizon de l'épisode en décisions : `max_steps`, raccourci pour les drills courts (gardien 6 s, tirs 8–10 s,
    /// dribble de virage 20 s).
    fn max_steps(&self) -> u32 {
        let secs = match self.drill.as_ref().map(|d| d.0) {
            Some(Drill::Keeper) => 6.0,
            Some(Drill::ShotRoll | Drill::ShotBounce | Drill::ShotCross | Drill::Rebound) => 10.0,
            Some(Drill::ShotLob | Drill::AerialHigh | Drill::WallRun) => 8.0,
            Some(Drill::DribbleTurn) => 20.0,
            _ => return self.cfg.max_steps,
        };
        let steps = (secs * TICKS_PER_SECOND / self.cfg.tick_skip.max(1) as f32) as u32;
        steps.min(self.cfg.max_steps)
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

/// État des 34 pads : 1 si active, sinon la part du temps de réapparition déjà écoulée (0 = vient d'être prise).
/// Orange (`mirror`) : le pad `j` est celui qui occupe la place symétrique (x → -x, y → -y) de la pad `j` bleue.
fn push_pads(v: &mut Vec<f32>, pads: &[sim_core::PadSnapshot], mirror: bool) {
    if pads.len() != N_PADS {
        v.extend([0.0; N_PADS]);
        return;
    }
    let value = |p: &sim_core::PadSnapshot| {
        if p.active {
            1.0
        } else {
            let cd = if p.big {
                PAD_COOLDOWN_BIG
            } else {
                PAD_COOLDOWN_SMALL
            };
            (1.0 - p.cooldown / cd).clamp(0.0, 1.0)
        }
    };
    for (j, p) in pads.iter().enumerate() {
        let src = if mirror {
            pads.iter()
                .min_by(|a, b| {
                    let d = |q: &sim_core::PadSnapshot| {
                        (q.pos[0] + p.pos[0]).powi(2) + (q.pos[1] + p.pos[1]).powi(2)
                    };
                    d(a).total_cmp(&d(b))
                })
                .unwrap_or(&pads[j])
        } else {
            p
        };
        v.push(value(src));
    }
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
    v.resize(OBS_DIM_V2, 0.0);
    push_pads(&mut v, &s.pads, mirror);
    v.push(other.map_or(0.0, |j| s.cars[j].boost / 100.0));
    v.extend(c.last_controls);
    v.extend([
        f32::from(u8::from(c.is_flipping)),
        (c.air_time_since_jump / JUMP_TIME_SCALE).clamp(0.0, 1.0),
        f32::from(c.wheels_contact) / 4.0,
    ]);
    debug_assert_eq!(v.len(), OBS_DIM);
    let mut o = [0.0f32; OBS_DIM];
    o[..v.len()].copy_from_slice(&v);
    o
}

// ---------------------------------------------------------------- VecEnv

/// Épisode terminé, remonté par [`VecEnv::step`].
#[derive(Clone, Debug)]
pub struct EpisodeStats {
    /// Indice de l'env qui a fini l'épisode.
    pub env: usize,
    pub ret: f32,
    pub length: u32,
    pub touched: bool,
    pub outcome: Outcome,
    /// Renseigné si l'épisode était un coup d'envoi.
    pub kickoff: Option<KickoffStats>,
    /// Renseigné si l'épisode était un drill (à exclure des stats de curriculum).
    pub drill: Option<Drill>,
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
    /// Clips rejouables finis pendant ce pas (vide sans `EnvConfig::record_clips`).
    pub clips: Vec<Clip>,
    /// Par env : la décision qui vient d'être prise appartenait à un drill (à exclure de l'entropie du garde-fou).
    pub in_drill: Vec<bool>,
    /// Compteurs de drill (épisodes, mécaniques vues, bonus payés) de ce pas.
    pub drill_report: DrillReport,
    /// Statistiques de boost (match) de ce pas.
    pub boost_report: BoostReport,
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
    clips: Vec<Clip>,
    in_drill: Vec<bool>,
    drill_report: DrillReport,
    boost_report: BoostReport,
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
            out.clips.extend(r.clips);
            out.in_drill.extend(r.in_drill);
            out.drill_report.add(&r.drill_report);
            out.boost_report.add(&r.boost_report);
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
            clips: Vec::new(),
            in_drill: Vec::new(),
            drill_report: DrillReport::default(),
            boost_report: BoostReport::default(),
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
                    let drill = e.drill();
                    reply.in_drill.push(drill.is_some());
                    let r = e.step_vs(actions[i], opp.as_ref().map(|o| o[i]));
                    ret[i] += r.reward;
                    len[i] += 1;
                    touched[i] |= r.touched;
                    reply.reward.push(r.reward);
                    reply.terminated.push(r.terminated);
                    reply.truncated.push(r.truncated);
                    if r.terminated || r.truncated {
                        reply.finished.push(EpisodeStats {
                            env: first + i,
                            ret: ret[i],
                            length: len[i],
                            touched: touched[i],
                            outcome: r.outcome,
                            kickoff: r.kickoff,
                            drill,
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
                    reply.drill_report.add(&e.take_drill_report());
                    reply.boost_report.add(&e.take_boost_report());
                    reply.clips.extend(e.take_clips());
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
        // 12 actions au sol + 66 aériennes + 12 actions au sol avec frein à main
        assert_eq!(t.len(), 90);
        assert!((0..N_ACTIONS_V1).all(|i| (12..78).contains(&i) || !t.get(i).handbrake));
        assert!((78..90).all(|i| t.get(i).handbrake && !t.get(i).jump));
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
    fn drills_off_by_default_and_on_when_asked() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::for_stage(6), 3).unwrap();
        for _ in 0..30 {
            e.reset();
            assert!(e.drill().is_none());
        }
        let cfg = EnvConfig {
            drill_prob: 1.0,
            ..EnvConfig::for_stage(6)
        };
        let mut e = Env::new(cfg, 4).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..300 {
            let o = e.reset();
            assert!(o.iter().all(|x| x.is_finite() && x.abs() <= 1.5), "{o:?}");
            seen.insert(e.drill().expect("drill"));
            for _ in 0..3 {
                let r = e.step(0);
                assert!(r.reward.is_finite());
            }
        }
        assert_eq!(seen.len(), Drill::ALL.len());
        let rep = e.take_drill_report();
        assert!(rep.episodes.iter().sum::<u64>() >= 299);
        // Palier de toucher : jamais de drill.
        let cfg = EnvConfig {
            drill_prob: 1.0,
            ..EnvConfig::for_stage(1)
        };
        let mut e = Env::new(cfg, 5).unwrap();
        e.reset();
        assert!(e.drill().is_none());
    }

    #[test]
    fn recovery_drill_ends_after_landing_and_reports_verdicts() {
        assert!(setup());
        let cfg = EnvConfig {
            drill_prob: 1.0,
            max_steps: 400,
            ..EnvConfig::for_stage(6)
        };
        let mut e = Env::new(cfg, 21).unwrap();
        let mut ended = 0;
        for i in 0..12 {
            e.reset_drill_with(Drill::Recovery);
            if i == 0 {
                // L'épisode du constructeur n'est pas un des 12.
                e.take_drill_report();
            }
            assert_eq!(e.drill(), Some(Drill::Recovery));
            let mut done = false;
            for _ in 0..390 {
                let r = e.step(0);
                assert!(r.reward.is_finite());
                if r.terminated || r.truncated {
                    done = true;
                    break;
                }
            }
            ended += usize::from(done);
        }
        // Retomber prend au plus ~4 s : tous les épisodes finissent bien avant 390 décisions (26 s).
        assert_eq!(ended, 12);
        e.reset();
        let rep = e.take_drill_report();
        assert_eq!(rep.recovery.iter().sum::<u64>(), 12, "{rep:?}");
        assert_eq!(
            rep.recovery[drill::RecoveryOutcome::None.index()],
            0,
            "{rep:?}"
        );
    }

    #[test]
    fn drill_mechanics_are_counted_apart_and_opponent_is_parked() {
        assert!(setup());
        // Le dribble de virage a un adversaire actif : testé à part.
        let mut drill_weights = Drill::DEFAULT_WEIGHTS;
        drill_weights[Drill::DribbleTurn.index()] = 0.0;
        let cfg = EnvConfig {
            drill_prob: 1.0,
            drill_weights,
            ..EnvConfig::for_stage(6)
        };
        let mut e = Env::new(cfg, 9).unwrap();
        let mut total = 0;
        for _ in 0..40 {
            e.reset();
            for _ in 0..60 {
                // Boost + saut + avant : action de la table au hasard.
                let a = (e.rng.next_u64() % 78) as usize;
                // L'action adverse est ignorée en drill.
                e.step_vs(a, Some(77));
            }
            let snap = e.sim.snapshot();
            let opp = &snap.cars[car_index(&snap, e.opp.unwrap())];
            assert!(opp.pos[1] > 4000.0, "adversaire garé : {:?}", opp.pos);
        }
        // Aucun compteur de match, tout en drill.
        assert_eq!(e.take_mechanics().total(), 0);
        total += e.take_drill_report().mechanics.total();
        assert!(total > 0, "au moins des touchers en drill");
    }

    #[test]
    fn dribble_turn_has_an_active_opponent_and_all_families_run() {
        assert!(setup());
        let cfg = EnvConfig {
            drill_prob: 1.0,
            ..EnvConfig::for_stage(6)
        };
        let mut e = Env::new(cfg, 4).unwrap();
        // Adversaire actif : il obéit à l'action adverse (accélération + boost) et quitte sa place de départ.
        e.reset_drill_with(Drill::DribbleTurn);
        let start = e.sim.snapshot().cars[car_index(&e.sim.snapshot(), e.opp.unwrap())].pos;
        assert!(start[1] < 4400.0, "pas garé : {start:?}");
        for _ in 0..30 {
            e.step_vs(0, Some(e.expert_action(0.0)));
        }
        let snap = e.sim.snapshot();
        let now = snap.cars[car_index(&snap, e.opp.unwrap())].pos;
        assert!(
            (now[0] - start[0]).hypot(now[1] - start[1]) > 300.0,
            "{start:?} {now:?}"
        );
        // Chaque famille démarre, avance et rend des observations finies ; l'horizon des drills courts est tenu.
        for d in Drill::ALL {
            let o = e.reset_drill_with(d);
            assert!(o.iter().all(|x| x.is_finite()), "{d:?}");
            let mut ended = false;
            for _ in 0..e.cfg.max_steps {
                let info = e.step(0);
                assert!(info.reward.is_finite());
                if info.terminated || info.truncated {
                    ended = true;
                    break;
                }
            }
            assert!(ended, "{d:?} ne finit pas");
        }
        let rep = e.take_drill_report();
        assert!(
            Drill::ALL
                .iter()
                .all(|d| rep.episodes[d.index()] >= 1 || *d == Drill::Rebound)
        );
    }

    #[test]
    fn keeper_scores_saves_when_the_car_charges_the_ball() {
        assert!(setup());
        let cfg = EnvConfig {
            drill_prob: 1.0,
            ..EnvConfig::for_stage(6)
        };
        let mut e = Env::new(cfg, 6).unwrap();
        assert_eq!(
            {
                e.reset_drill_with(Drill::Keeper);
                e.max_steps()
            },
            (6.0 * 120.0 / 8.0) as u32
        );
        // Le fonceur scripté (cap vers la balle, gaz, boost) croise souvent la trajectoire : des arrêts doivent
        // être comptés, et la réussite = arrêt sans but encaissé.
        for _ in 0..80 {
            e.reset_drill_with(Drill::Keeper);
            for _ in 0..e.max_steps() {
                let s = e.sim.snapshot();
                let a = e.expert_action(charger_steer(&s, e.car));
                let info = e.step(a);
                if info.terminated || info.truncated {
                    break;
                }
            }
        }
        e.reset();
        let rep = e.take_drill_report();
        assert!(rep.saves > 0, "aucun arrêt en 80 épisodes");
        // Réussite = arrêt sans but encaissé ensuite : jamais plus que d'arrêts.
        assert!(rep.success[Drill::Keeper.index()] <= rep.saves);
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
            let present = o[OBS_DIM_V2 - 1];
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
        assert!(
            e.observe()[OBS_DIM_V1..OBS_DIM_V2]
                .iter()
                .all(|x| *x == 0.0)
        );
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

    fn boost_cfg() -> EnvConfig {
        EnvConfig {
            end_on_touch: false,
            approach_weight: 0.0,
            touch_reward: 0.0,
            boost_reward: BOOST_REWARD,
            ..EnvConfig::default()
        }
    }

    #[test]
    fn observation_v3_layout_pads_previous_action_and_flip_state() {
        assert!(setup());
        let mut e = Env::new(boost_cfg(), 4).unwrap();
        e.reset();
        let a = e.expert_action(1.0);
        e.step(a);
        let o = e.observe();
        assert_eq!(o.len(), OBS_DIM);
        let pads = &o[OBS_DIM_V2..OBS_DIM_V2 + N_PADS];
        assert!(pads.iter().all(|p| (0.0..=1.0).contains(p)));
        // Pas d'adversaire : boost adverse nul ; dernière action = contrôles de l'action jouée.
        assert_eq!(o[OBS_DIM_V2 + N_PADS], 0.0);
        let c = ActionTable::rlgym().get(a);
        let prev = &o[OBS_DIM_V2 + N_PADS + 1..OBS_DIM_V2 + N_PADS + 9];
        assert_eq!(prev[0], c.throttle);
        assert_eq!(prev[1], c.steer);
        assert_eq!((prev[5], prev[6], prev[7]), (0.0, 1.0, 0.0));
        // Au sol : 4 roues, pas de dodge.
        let flip = &o[OBS_DIM - 3..];
        assert_eq!((flip[0], flip[2]), (0.0, 1.0));
    }

    #[test]
    fn orange_view_of_pads_is_mirrored() {
        assert!(setup());
        let mut e = Env::new(EnvConfig::for_stage(5), 5).unwrap();
        e.reset_kickoff(Some(0));
        // Vide le premier pad côté bleu : la vue orange le voit à la place symétrique.
        let s = e.sim.snapshot();
        let k = (0..N_PADS).find(|&i| s.pads[i].pos[1] < -3000.0).unwrap();
        let mut st = s.clone();
        st.pads[k].active = false;
        st.pads[k].cooldown = 2.0;
        let blue = build_obs(&st, 0, Some(1), false);
        let orange = build_obs(&st, 1, Some(0), true);
        let m = (0..N_PADS)
            .find(|&j| {
                (st.pads[j].pos[0] + st.pads[k].pos[0]).abs() < 1.0
                    && (st.pads[j].pos[1] + st.pads[k].pos[1]).abs() < 1.0
            })
            .unwrap();
        assert!(blue[OBS_DIM_V2 + k] < 1.0);
        assert!(orange[OBS_DIM_V2 + m] < 1.0);
        assert_eq!(
            orange
                .iter()
                .skip(OBS_DIM_V2)
                .take(N_PADS)
                .filter(|v| **v < 1.0)
                .count(),
            1
        );
    }

    #[test]
    fn pickup_pays_on_gains_only_and_counts_the_pad_size() {
        assert!(setup());
        let mut e = Env::new(boost_cfg(), 6).unwrap();
        e.reset();
        e.take_boost_report();
        let pads = e.sim.pads();
        let small = pads.iter().find(|p| !p.big).unwrap().pos;
        e.sim
            .place_car(e.car, [small[0], small[1], 17.0], 0.0, 0.0)
            .unwrap();
        e.sim.set_controls(e.car, Controls::default()).ok();
        e.observe_synced();
        // Une décision à l'arrêt sur la pad vide : gain de 12 (√0,12 × c).
        let idle = (0..e.actions.len())
            .find(|&i| {
                let c = e.actions.get(i);
                c.throttle == 0.0
                    && c.steer == 0.0
                    && !c.boost
                    && !c.jump
                    && c.pitch == 0.0
                    && c.roll == 0.0
            })
            .unwrap();
        let r = e.step(idle).reward;
        let rep = e.take_boost_report();
        assert!((r - BOOST_REWARD * 0.12f32.sqrt()).abs() < 1e-3, "{r}");
        assert_eq!((rep.pickups_small, rep.pickups_big), (1, 0), "{rep:?}");
        // Dépenser du boost ne coûte rien.
        e.sim
            .place_car(e.car, [0.0, -2000.0, 17.0], 0.0, 80.0)
            .unwrap();
        e.observe_synced();
        let a = e.expert_action(0.0);
        for _ in 0..5 {
            assert!(e.step(a).reward >= 0.0);
        }
        let rep = e.take_boost_report();
        assert_eq!(rep.empty_steps, 0);
        assert!(rep.boost_sum / (rep.steps as f64) < 80.0);
    }

    #[test]
    fn random_start_boost_applies_to_matches_but_not_kickoffs() {
        assert!(setup());
        let cfg = EnvConfig {
            random_start_boost: true,
            ..EnvConfig::for_stage(2)
        };
        let mut e = Env::new(cfg, 8).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..30 {
            e.reset();
            let b = e.sim.snapshot().cars[0].boost;
            assert!((0.0..=100.0).contains(&b));
            seen.insert(b as i32);
        }
        assert!(seen.len() > 10, "boost de départ non varié : {seen:?}");
        e.reset_kickoff(Some(0));
        assert!((e.sim.snapshot().cars[0].boost - 33.0).abs() < 0.5);
    }
}
