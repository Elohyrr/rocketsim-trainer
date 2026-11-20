//! PPO discret : acteur/critique MLP, GAE(λ), objectif clippé, Adam.

use std::collections::BTreeMap;
use std::path::Path;

use crate::explore::{self, Explore};
use crate::pool::{POOL_SHARE, Pool};
use burn::module::{Module, Param};
use burn::nn::{Linear, LinearConfig};
use burn::optim::{AdamConfig, GradientsParams, ModuleOptimizer};
use burn::tensor::activation::log_softmax;
use burn::tensor::{Device, Tensor, TensorData};
use policy_runtime::{Tensor as StTensor, write_safetensors};
use rl_env::{
    EnvConfig, KickoffResult, N_ACTIONS_V1, NEW_ACTION_BIAS, OBS_DIM_V1, OBS_DIM_V2, Opponent,
    Outcome, Rng, VecEnv,
};

#[derive(Module, Debug)]
pub struct Mlp {
    l0: Linear,
    l1: Linear,
    out: Linear,
}

impl Mlp {
    pub fn new(n_in: usize, hidden: usize, n_out: usize, device: &Device) -> Self {
        Self {
            l0: LinearConfig::new(n_in, hidden).init(device),
            l1: LinearConfig::new(hidden, hidden).init(device),
            out: LinearConfig::new(hidden, n_out).init(device),
        }
    }
    /// Agrandit la couche d'entrée à `n_in` entrées ; les poids des entrées ajoutées sont nuls.
    pub fn widen_input(self, n_in: usize) -> Self {
        let w = self.l0.weight.val();
        let [rows, hidden] = w.dims();
        if rows >= n_in {
            return self;
        }
        let pad = Tensor::zeros([n_in - rows, hidden], &w.device());
        let weight = Param::from_tensor(Tensor::cat(vec![w, pad], 0).detach());
        Self {
            l0: Linear {
                weight,
                bias: self.l0.bias,
            },
            ..self
        }
    }

    /// Agrandit la couche de sortie à `n_out` sorties ; les poids des sorties ajoutées sont nuls et leur biais
    /// vaut `bias` (actions ajoutées peu probables au départ, les autres logits sont inchangés).
    pub fn widen_output(self, n_out: usize, bias: f32) -> Self {
        let w = self.out.weight.val();
        let [hidden, cols] = w.dims();
        if cols >= n_out {
            return self;
        }
        let dev = w.device();
        let weight = Tensor::cat(vec![w, Tensor::zeros([hidden, n_out - cols], &dev)], 1);
        let b = self.out.bias.as_ref().expect("biais").val();
        let bias = Tensor::cat(vec![b, Tensor::full([n_out - cols], bias, &dev)], 0);
        Self {
            out: Linear {
                weight: Param::from_tensor(weight.detach()),
                bias: Some(Param::from_tensor(bias.detach())),
            },
            ..self
        }
    }

    /// Met à zéro les poids des entrées `from..to` de la couche d'entrée (biais et autres couches inchangés).
    pub fn zero_inputs(self, from: usize, to: usize) -> Self {
        let w = self.l0.weight.val();
        let [rows, hidden] = w.dims();
        let to = to.min(rows);
        if from >= to {
            return self;
        }
        let dev = w.device();
        let mut parts = Vec::new();
        if from > 0 {
            parts.push(w.clone().slice([0..from, 0..hidden]));
        }
        parts.push(Tensor::zeros([to - from, hidden], &dev));
        if to < rows {
            parts.push(w.slice([to..rows, 0..hidden]));
        }
        let weight = Param::from_tensor(Tensor::cat(parts, 0).detach());
        Self {
            l0: Linear {
                weight,
                bias: self.l0.bias,
            },
            ..self
        }
    }

    /// Poids au format `policy.N.weight` / `policy.N.bias` (celui de `policy.safetensors`).
    pub fn to_tensors(&self) -> BTreeMap<String, StTensor> {
        let mut t = BTreeMap::new();
        let layers = [
            ("policy.0", &self.l0),
            ("policy.1", &self.l1),
            ("policy.2", &self.out),
        ];
        for (name, l) in layers {
            let w = l.weight.val().into_data();
            let shape = w.shape().to_vec();
            t.insert(
                format!("{name}.weight"),
                StTensor {
                    shape,
                    data: w.try_to_vec::<f32>().expect("f32"),
                },
            );
            let b = l.bias.as_ref().expect("biais").val().into_data();
            t.insert(
                format!("{name}.bias"),
                StTensor {
                    shape: b.shape().to_vec(),
                    data: b.try_to_vec::<f32>().expect("f32"),
                },
            );
        }
        t
    }

    /// Réseau de mêmes dimensions que `self`, avec les poids de `t` (format de [`Mlp::to_tensors`]).
    pub fn from_tensors(
        &self,
        t: &BTreeMap<String, StTensor>,
        device: &Device,
    ) -> Result<Mlp, String> {
        // `grow_in` : une couche d'entrée plus étroite (réseau d'avant une observation élargie) est complétée de
        // poids nuls ; `grow_out` : une sortie plus étroite (table d'actions élargie) l'est de poids nuls et de
        // biais `NEW_ACTION_BIAS` (politique seulement : une sortie unique de critique ne grandit pas).
        let layer =
            |name: &str, l: &Linear, grow_in: bool, grow_out: bool| -> Result<Linear, String> {
                let w = t
                    .get(&format!("{name}.weight"))
                    .ok_or(format!("{name}.weight absent"))?;
                let b = t
                    .get(&format!("{name}.bias"))
                    .ok_or(format!("{name}.bias absent"))?;
                let [r, c] = l.weight.dims();
                let [fr, fc] = match w.shape[..] {
                    [a, b] => [a, b],
                    _ => return Err(format!("{name} : dimensions {:?} inattendues", w.shape)),
                };
                let ok_in = fr == r || (grow_in && fr < r);
                let ok_out = fc == c || (grow_out && c > 1 && fc < c && fc > 1);
                if !ok_in || !ok_out || b.shape != [fc] || w.data.len() != fr * fc {
                    return Err(format!(
                        "{name} : dimensions {:?} au lieu de [{r}, {c}]",
                        w.shape
                    ));
                }
                let mut wd = vec![0.0f32; r * c];
                for i in 0..fr {
                    wd[i * c..i * c + fc].copy_from_slice(&w.data[i * fc..(i + 1) * fc]);
                }
                let mut bd = vec![NEW_ACTION_BIAS; c];
                bd[..fc].copy_from_slice(&b.data);
                Ok(Linear {
                    weight: Param::from_tensor(tensor2(wd, r, c, device).detach()),
                    bias: Some(Param::from_tensor(
                        Tensor::<1>::from_data(TensorData::new(bd, [c]), device).detach(),
                    )),
                })
            };
        Ok(Mlp {
            l0: layer("policy.0", &self.l0, true, false)?,
            l1: layer("policy.1", &self.l1, false, false)?,
            out: layer("policy.2", &self.out, false, true)?,
        })
    }

    pub fn forward(&self, x: Tensor<2>) -> Tensor<2> {
        let x = self.l0.forward(x).tanh();
        let x = self.l1.forward(x).tanh();
        self.out.forward(x)
    }
}

#[derive(Module, Debug)]
pub struct ActorCritic {
    pub policy: Mlp,
    pub value: Mlp,
}

#[derive(Clone, Debug)]
pub struct PpoConfig {
    pub n_envs: usize,
    pub threads: usize,
    pub horizon: usize,
    pub hidden: usize,
    pub epochs: usize,
    pub minibatch: usize,
    pub gamma: f32,
    pub lambda: f32,
    pub clip: f32,
    pub vf_coef: f32,
    pub seed: u64,
}

impl Default for PpoConfig {
    fn default() -> Self {
        Self {
            n_envs: 16,
            threads: 2,
            horizon: 128,
            hidden: 128,
            epochs: 4,
            minibatch: 512,
            gamma: 0.99,
            lambda: 0.95,
            clip: 0.2,
            vf_coef: 0.5,
            seed: 1,
        }
    }
}

/// Statistiques d'une itération.
#[derive(Clone, Debug, Default)]
pub struct IterStats {
    pub steps: u64,
    pub episodes: u64,
    pub touches: u64,
    /// Épisodes finis par un but marqué (+Y) / un but contre son camp (-Y).
    pub goals: u64,
    pub own_goals: u64,
    /// Coups d'envoi terminés (exclus de `episodes`, `touches`, `goals` et `own_goals` : ils ne doivent pas
    /// peser sur la promotion du curriculum) : gagnés / perdus / neutres.
    pub kick_won: u64,
    pub kick_lost: u64,
    pub kick_neutral: u64,
    /// Somme des temps (s) jusqu'au premier toucher de l'agent et nombre de kickoffs touchés.
    pub kick_touch_s_sum: f64,
    pub kick_touched: u64,
    /// Self-play (hors coups d'envoi) : buts marqués / encaissés contre la copie gelée (`cur_*`) et contre le pool.
    pub cur_goals: u64,
    pub cur_conceded: u64,
    pub pool_goals: u64,
    pub pool_conceded: u64,
    /// Somme des vitesses (uu/s) au contact.
    pub kick_speed_sum: f64,
    pub reward_mean: f64,
    pub length_mean: f64,
    pub touch_rate: f64,
    pub goal_rate: f64,
    pub policy_loss: f64,
    pub value_loss: f64,
    pub entropy: f64,
    pub kl: f64,
    /// Mécaniques détectées pendant l'itération (voiture de l'agent).
    pub mechanics: mechanics::Counts,
    /// Drills : épisodes par drill, mécaniques vues en drill, bonus payés (voir `docs/drills.md`).
    pub drill: rl_env::DrillReport,
    /// Boost de l'agent en match : ramassages, niveau moyen, part du temps à 0.
    pub boost: rl_env::BoostReport,
    /// Clips rejouables finis pendant l'itération (vide sans `EnvConfig::record_clips`).
    pub clips: Vec<rl_env::clip::Clip>,
    /// Perte de clonage moyenne (entropie croisée sur les démonstrations) et poids λ utilisé (0 si coupé).
    pub bc_loss: f64,
    pub bc_lambda: f64,
    /// Coefficient d'entropie et lr utilisés, époques PPO effectivement faites (plafond de KL).
    pub ent_coef: f64,
    pub lr: f64,
    pub epochs_run: usize,
    pub collect_seconds: f64,
    /// Part de `collect_seconds` passée en inférence (hors simulation).
    pub infer_seconds: f64,
    pub train_seconds: f64,
}

/// Mémoire tampon bornée des états de démonstration (anneau : au-delà de `cap`, les plus anciens sont écrasés).
#[derive(Clone, Debug)]
pub struct DemoBuffer {
    od: usize,
    cap: usize,
    obs: Vec<f32>,
    act: Vec<u8>,
    next: usize,
}

impl DemoBuffer {
    pub fn new(od: usize, cap: usize) -> Self {
        Self {
            od,
            cap: cap.max(1),
            obs: Vec::new(),
            act: Vec::new(),
            next: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.act.len()
    }
    pub fn is_empty(&self) -> bool {
        self.act.is_empty()
    }
    pub fn push(&mut self, obs: &[f32], action: u8) {
        debug_assert_eq!(obs.len(), self.od);
        if self.act.len() < self.cap {
            self.obs.extend_from_slice(obs);
            self.act.push(action);
        } else {
            let i = self.next % self.cap;
            self.obs[i * self.od..(i + 1) * self.od].copy_from_slice(obs);
            self.act[i] = action;
        }
        self.next += 1;
    }
    /// `m` échantillons tirés au hasard : observations à plat et actions.
    pub fn sample(&self, rng: &mut Rng, m: usize) -> (Vec<f32>, Vec<usize>) {
        let (mut o, mut a) = (Vec::with_capacity(m * self.od), Vec::with_capacity(m));
        for _ in 0..m {
            let i = (rng.next_u64() % self.act.len() as u64) as usize;
            o.extend_from_slice(&self.obs[i * self.od..(i + 1) * self.od]);
            a.push(self.act[i] as usize);
        }
        (o, a)
    }
}

/// Perte de clonage de comportement mélangée à PPO (voir `docs/demonstration.md`).
#[derive(Clone, Debug)]
pub struct BcConfig {
    /// Poids initial λ0 (0 = désactivé).
    pub lambda0: f32,
    /// Pas de collecte au bout desquels λ atteint 0 (décroissance linéaire).
    pub decay_steps: u64,
    /// Échantillons de démonstration par minibatch PPO.
    pub batch: usize,
}

impl Default for BcConfig {
    fn default() -> Self {
        Self {
            lambda0: 0.0,
            decay_steps: 30_000_000,
            batch: 128,
        }
    }
}

impl BcConfig {
    /// Poids après `steps` pas de collecte.
    pub fn lambda(&self, steps: u64) -> f32 {
        if self.decay_steps == 0 {
            return 0.0;
        }
        (self.lambda0 * (1.0 - steps as f32 / self.decay_steps as f32)).max(0.0)
    }
}

/// Statistiques de perte de l'acteur sur un minibatch.
struct PolStats {
    bc: f64,
    en_match: f64,
    en_n: f64,
    en_all: f64,
    pl: f64,
    kl: f64,
}

pub struct Trainer {
    pub cfg: PpoConfig,
    env: VecEnv,
    model: ActorCritic,
    opt: ModuleOptimizer,
    /// Optimiseur du critique (l'acteur a `opt`) : les deux sont entraînés en parallèle.
    opt_value: ModuleOptimizer,
    device: Device,
    plain: Device,
    obs: Vec<f32>,
    rng: Rng,
    obs_dim: usize,
    n_actions: usize,
    /// Adversaire self-play : copie gelée de la politique (mise à jour toutes les `FROZEN_EVERY` itérations).
    frozen: Option<Mlp>,
    frozen_age: usize,
    selfplay: bool,
    /// Anciennes politiques (30 % des envs en self-play).
    pub pool: Pool,
    /// Régulateur d'exploration (ent_coef adaptatif, lr décroissant) ; état persisté par le learner.
    pub explore: Explore,
    /// Échauffement du critique : la politique n'est pas mise à jour (voir [`Trainer::load_actor`]).
    pub freeze_policy: bool,
    /// Démonstrations et réglage du clonage ; pas de collecte déjà passés sous clonage (décroissance de λ).
    pub demo: DemoBuffer,
    pub bc: BcConfig,
    pub bc_steps: u64,
}

/// Itérations entre deux mises à jour de la copie gelée en self-play.
pub const FROZEN_EVERY: usize = 50;

fn tensor2(v: Vec<f32>, rows: usize, cols: usize, d: &Device) -> Tensor<2> {
    Tensor::from_data(TensorData::new(v, [rows, cols]), d)
}

fn to_vec<const D: usize>(t: Tensor<D>) -> Vec<f32> {
    t.into_data().try_to_vec::<f32>().expect("f32")
}

fn scalar(t: Tensor<1>) -> f64 {
    f64::from(to_vec(t)[0])
}

impl Trainer {
    pub fn new(cfg: PpoConfig, env_cfg: EnvConfig) -> Result<Self, rl_env::SimError> {
        let env_cfg_selfplay = env_cfg.opponent == Opponent::SelfPlay;
        let mut env = VecEnv::new(cfg.n_envs, cfg.threads, env_cfg, cfg.seed)?;
        let (obs_dim, n_actions) = (env.obs_dim(), env.action_count());
        let plain = Device::flex();
        let device = plain.clone().autodiff();
        device.seed(cfg.seed);
        let model = ActorCritic {
            policy: Mlp::new(obs_dim, cfg.hidden, n_actions, &device),
            value: Mlp::new(obs_dim, cfg.hidden, 1, &device),
        };
        let opt = AdamConfig::new().with_epsilon(1e-5).init();
        let opt_value = AdamConfig::new().with_epsilon(1e-5).init();
        let obs = env.reset();
        let selfplay = env_cfg_selfplay;
        Ok(Self {
            rng: Rng::new(cfg.seed ^ 0xA5A5),
            explore: Explore::default(),
            freeze_policy: false,
            demo: DemoBuffer::new(obs_dim, 50_000),
            bc: BcConfig::default(),
            bc_steps: 0,
            cfg,
            env,
            model,
            opt,
            opt_value,
            device,
            plain,
            obs,
            obs_dim,
            n_actions,
            frozen: None,
            frozen_age: 0,
            selfplay,
            pool: Pool::new(None),
        })
    }

    /// Collecte `horizon × n_envs` pas puis fait `epochs` passes PPO.
    pub fn iteration(&mut self) -> IterStats {
        let (n, h, od, na) = (
            self.cfg.n_envs,
            self.cfg.horizon,
            self.obs_dim,
            self.n_actions,
        );
        let t0 = std::time::Instant::now();
        let inner = self.model.valid();
        let idev = self.plain.clone();
        if self.selfplay && (self.frozen.is_none() || self.frozen_age >= FROZEN_EVERY) {
            self.frozen = Some(inner.policy.clone());
            self.frozen_age = 0;
        }
        self.frozen_age += 1;
        if self.selfplay {
            self.pool.since_snapshot += 1;
            if self.pool.is_empty() || self.pool.since_snapshot >= self.pool.every {
                if let Err(e) = self.pool.push(inner.policy.clone()) {
                    eprintln!("pool : instantané non sauvegardé ({e})");
                }
                eprintln!("pool : instantané pris, {} anciens", self.pool.len());
            }
        }
        // Adversaires de l'itération : copie gelée, sauf `POOL_SHARE` des envs contre un ancien tiré du pool.
        let pool_pick = self.pool.pick(&mut self.rng).filter(|_| self.selfplay);
        let on_pool: Vec<bool> = (0..n)
            .map(|_| pool_pick.is_some() && self.rng.unit() < POOL_SHARE)
            .collect();
        let pool_net = pool_pick.and_then(|i| self.pool.get(i)).cloned();
        let mut vs_pool = (0u64, 0u64);

        let mut obs_buf = Vec::with_capacity(h * n * od);
        let mut act_buf = Vec::with_capacity(h * n);
        let mut logp_buf = Vec::with_capacity(h * n);
        let mut val_buf = Vec::with_capacity((h + 1) * n);
        let mut rew_buf = Vec::with_capacity(h * n);
        let mut done_buf = Vec::with_capacity(h * n);
        let mut drill_buf: Vec<bool> = Vec::with_capacity(h * n);
        let mut st = IterStats::default();
        let (mut ret_sum, mut len_sum) = (0.0f64, 0.0f64);

        for _ in 0..h {
            let x = tensor2(self.obs.clone(), n, od, &idev);
            // Acteur, critique et adversaires sont indépendants : un thread chacun (les tirages restent dans
            // le même ordre, donc mêmes actions).
            let ox = self
                .frozen
                .is_some()
                .then(|| tensor2(self.env.opp_obs().to_vec(), n, od, &idev));
            let ti = std::time::Instant::now();
            let (lp, values, opp_lps) = std::thread::scope(|sc| {
                let hv = sc.spawn(|| to_vec(inner.value.forward(x.clone())));
                let ho = ox.as_ref().map(|ox| {
                    let (frozen, pool_net) = (self.frozen.as_ref(), pool_net.as_ref());
                    sc.spawn(move || {
                        let lps = |net: &Mlp| to_vec(log_softmax(net.forward(ox.clone()), 1));
                        (lps(frozen.expect("adversaire gelé")), pool_net.map(lps))
                    })
                });
                let lp = to_vec(log_softmax(inner.policy.forward(x.clone()), 1));
                (
                    lp,
                    hv.join().expect("thread critique"),
                    ho.map(|h| h.join().expect("thread adversaire")),
                )
            });
            st.infer_seconds += ti.elapsed().as_secs_f64();
            let mut acts = Vec::with_capacity(n);
            for e in 0..n {
                let row = &lp[e * na..(e + 1) * na];
                let u = self.rng.unit();
                let (mut c, mut a) = (0.0f32, na - 1);
                for (i, l) in row.iter().enumerate() {
                    c += l.exp();
                    if u < c {
                        a = i;
                        break;
                    }
                }
                acts.push(a);
                act_buf.push(a as f32);
                logp_buf.push(row[a]);
            }
            obs_buf.extend_from_slice(&self.obs);
            val_buf.extend(values);
            let opp_acts = opp_lps.map(|(lp_f, lp_pool)| {
                let sample = |lp: &[f32], rng: &mut Rng| {
                    (0..n)
                        .map(|e| {
                            let row = &lp[e * na..(e + 1) * na];
                            let u = rng.unit();
                            let mut c = 0.0f32;
                            row.iter()
                                .position(|l| {
                                    c += l.exp();
                                    u < c
                                })
                                .unwrap_or(na - 1)
                        })
                        .collect::<Vec<usize>>()
                };
                let mut acts = sample(&lp_f, &mut self.rng);
                if let Some(lp_p) = &lp_pool {
                    let from_pool = sample(lp_p, &mut self.rng);
                    for e in (0..n).filter(|&e| on_pool[e]) {
                        acts[e] = from_pool[e];
                    }
                }
                acts
            });
            let step = self.env.step_vs(&acts, opp_acts.as_deref());
            st.mechanics.add(&step.mechanics);
            st.drill.add(&step.drill_report);
            st.boost.add(&step.boost_report);
            drill_buf.extend(step.in_drill.iter().copied());
            st.clips.extend(step.clips);
            for e in 0..n {
                rew_buf.push(step.reward[e]);
                done_buf.push(step.terminated[e] || step.truncated[e]);
            }
            // Troncature (timeout) : on traite comme une fin d'épisode (valeur future nulle) — approximation
            // acceptable pour des épisodes courts ; voir docs/bench.md.
            for ep in &step.finished {
                // Épisodes de drill : hors stats de curriculum (but, Elo, promotion) et hors compteurs d'épisodes.
                if ep.drill.is_some() {
                    continue;
                }
                if let Some(k) = &ep.kickoff {
                    match k.result {
                        KickoffResult::Won => st.kick_won += 1,
                        KickoffResult::Lost => st.kick_lost += 1,
                        KickoffResult::Neutral => st.kick_neutral += 1,
                    }
                    if let (Some(t), Some(v)) = (k.first_touch_s, k.touch_speed) {
                        st.kick_touched += 1;
                        st.kick_touch_s_sum += f64::from(t);
                        st.kick_speed_sum += f64::from(v);
                    }
                    continue;
                }
                st.episodes += 1;
                if self.selfplay {
                    let (g, c) = (
                        u64::from(ep.outcome == Outcome::Goal),
                        u64::from(ep.outcome == Outcome::OwnGoal),
                    );
                    if on_pool[ep.env] {
                        st.pool_goals += g;
                        st.pool_conceded += c;
                        vs_pool = (vs_pool.0 + 1, vs_pool.1 + c);
                    } else {
                        st.cur_goals += g;
                        st.cur_conceded += c;
                    }
                }
                st.touches += u64::from(ep.touched);
                st.goals += u64::from(ep.outcome == Outcome::Goal);
                st.own_goals += u64::from(ep.outcome == Outcome::OwnGoal);
                ret_sum += f64::from(ep.ret);
                len_sum += f64::from(ep.length);
            }
            self.obs = step.obs;
        }
        if let Some(i) = pool_pick {
            self.pool.record(i, vs_pool.0, vs_pool.1);
        }
        // Valeur de bootstrap.
        let x = tensor2(self.obs.clone(), n, od, &idev);
        val_buf.extend(to_vec(inner.value.forward(x)));

        // GAE.
        let mut adv = vec![0.0f32; h * n];
        let mut last = vec![0.0f32; n];
        for t in (0..h).rev() {
            for e in 0..n {
                let i = t * n + e;
                let nonterm = if done_buf[i] { 0.0 } else { 1.0 };
                let delta =
                    rew_buf[i] + self.cfg.gamma * val_buf[(t + 1) * n + e] * nonterm - val_buf[i];
                last[e] = delta + self.cfg.gamma * self.cfg.lambda * nonterm * last[e];
                adv[i] = last[e];
            }
        }
        let ret: Vec<f32> = (0..h * n).map(|i| adv[i] + val_buf[i]).collect();
        let mean = adv.iter().sum::<f32>() / adv.len() as f32;
        let var = adv.iter().map(|a| (a - mean).powi(2)).sum::<f32>() / adv.len() as f32;
        let adv: Vec<f32> = adv
            .iter()
            .map(|a| (a - mean) / (var.sqrt() + 1e-8))
            .collect();
        st.collect_seconds = t0.elapsed().as_secs_f64();

        // Optimisation.
        let t1 = std::time::Instant::now();
        let total = h * n;
        let mut idx: Vec<usize> = (0..total).collect();
        let (mut pl, mut vl, mut kl, mut cnt) = (0.0, 0.0, 0.0, 0.0);
        // Entropie du garde-fou : seulement sur les décisions de match (les drills l'entraînent mais ne la mesurent pas).
        let (mut en_match, mut en_n, mut en_all) = (0.0f64, 0.0f64, 0.0f64);
        let (ent_coef, lr) = (self.explore.ent_coef as f32, self.explore.lr());
        let bc_lambda = if self.demo.is_empty() {
            0.0
        } else {
            self.bc.lambda(self.bc_steps)
        };
        let mut bc_sum = 0.0f64;
        for _ in 0..self.cfg.epochs {
            let (kl_before, cnt_before) = (kl, cnt);
            for i in (1..total).rev() {
                idx.swap(i, (self.rng.next_u64() % (i as u64 + 1)) as usize);
            }
            for chunk in idx.chunks(self.cfg.minibatch) {
                let m = chunk.len();
                let mut ob = Vec::with_capacity(m * od);
                let mut oh = vec![0.0f32; m * na];
                let (mut olp, mut ad, mut rt) = (Vec::new(), Vec::new(), Vec::new());
                let mut match_w = Vec::with_capacity(m);
                for (j, &i) in chunk.iter().enumerate() {
                    match_w.push(f32::from(u8::from(!drill_buf[i])));
                    ob.extend_from_slice(&obs_buf[i * od..(i + 1) * od]);
                    oh[j * na + act_buf[i] as usize] = 1.0;
                    olp.push(logp_buf[i]);
                    ad.push(adv[i]);
                    rt.push(ret[i]);
                }
                let d = &self.device;
                let device_p = d.clone();
                let x = tensor2(ob, m, od, d);
                let onehot = tensor2(oh, m, na, d);
                let old_lp = tensor2(olp, m, 1, d);
                let adv_t = tensor2(ad, m, 1, d);
                let ret_t = tensor2(rt, m, 1, d);

                // Acteur et critique sont indépendants (pertes et optimiseurs séparés) : un thread chacun.
                // Mêmes gradients et même Adam que sur une seule perte somme : les paramètres ne se croisent pas.
                let (freeze, clip, vf_coef) = (self.freeze_policy, self.cfg.clip, self.cfg.vf_coef);
                // Démonstrations : un lot tiré au hasard, perte appliquée à elles seules (jamais aux états de match).
                let bc_batch = (bc_lambda > 0.0 && !freeze).then(|| {
                    let bm = self.bc.batch.max(1);
                    let (bo, ba) = self.demo.sample(&mut self.rng, bm);
                    let mut oh = vec![0.0f32; bm * na];
                    for (j, &a) in ba.iter().enumerate() {
                        oh[j * na + a.min(na - 1)] = 1.0;
                    }
                    (tensor2(bo, bm, od, d), tensor2(oh, bm, na, d))
                });
                let (policy, value) = (self.model.policy.clone(), self.model.value.clone());
                let (opt_p, opt_v) = (&mut self.opt, &mut self.opt_value);
                let (xp, xv) = (x.clone(), x);
                let (pol, val) = std::thread::scope(|sc| {
                    let hp = sc.spawn(move || {
                        let logits = policy.forward(xp);
                        let logits = if freeze { logits.detach() } else { logits };
                        let logp_all = log_softmax(logits, 1);
                        let logp = (logp_all.clone() * onehot).sum_dim(1);
                        let ratio = (logp.clone() - old_lp.clone()).exp();
                        let s1 = ratio.clone() * adv_t.clone();
                        let s2 = ratio.clamp(1.0 - clip, 1.0 + clip) * adv_t;
                        let pol_loss = -(s1.min_pair(s2)).mean();
                        let ent_vec = -(logp_all.clone().exp() * logp_all).sum_dim(1);
                        let w = tensor2(match_w, m, 1, &device_p);
                        let mut bc_val = 0.0;
                        let stats = PolStats {
                            bc: 0.0,
                            en_match: scalar((ent_vec.clone() * w.clone()).sum().reshape([1])),
                            en_n: scalar(w.sum().reshape([1])),
                            en_all: scalar(ent_vec.clone().sum().reshape([1])),
                            pl: scalar(pol_loss.clone().reshape([1])),
                            kl: scalar((old_lp - logp).mean().reshape([1])),
                        };
                        let entropy = ent_vec.mean();
                        if freeze {
                            return (policy, stats);
                        }
                        let mut loss = pol_loss - entropy * ent_coef;
                        if let Some((bx, boh)) = bc_batch {
                            let lp = log_softmax(policy.forward(bx), 1);
                            let ce = -(lp * boh).sum_dim(1).mean();
                            bc_val = scalar(ce.clone().reshape([1]));
                            loss = loss + ce * bc_lambda;
                        }
                        let stats = PolStats { bc: bc_val, ..stats };
                        let grads = GradientsParams::from_grads(loss.backward(), &policy);
                        (opt_p.step(lr, policy, grads), stats)
                    });
                    let vloss = {
                        let v = value.forward(xv);
                        (v - ret_t).powf_scalar(2.0).mean()
                    };
                    let vl = scalar(vloss.clone().reshape([1]));
                    let grads = GradientsParams::from_grads((vloss * vf_coef).backward(), &value);
                    let value = opt_v.step(lr, value, grads);
                    (hp.join().expect("thread acteur"), (value, vl))
                });
                let (policy, stats) = pol;
                let (value, vloss) = val;
                self.model = ActorCritic { policy, value };
                en_match += stats.en_match;
                en_n += stats.en_n;
                en_all += stats.en_all;
                bc_sum += stats.bc;
                pl += stats.pl;
                kl += stats.kl;
                vl += vloss;
                cnt += 1.0;
            }
            st.epochs_run += 1;
            // Plafond de KL : la politique a trop bougé sur cette époque, on garde les données pour la suivante.
            if (kl - kl_before) / (cnt - cnt_before) > explore::KL_MAX {
                break;
            }
        }
        st.train_seconds = t1.elapsed().as_secs_f64();
        st.steps = total as u64;
        st.policy_loss = pl / cnt;
        st.value_loss = vl / cnt;
        // Sans décision de match (rollout entièrement en drill) : repli sur toutes les décisions.
        st.entropy = if en_n > 0.0 {
            en_match / en_n
        } else {
            en_all / total as f64
        };
        st.kl = kl / cnt;
        st.bc_loss = bc_sum / cnt;
        st.bc_lambda = f64::from(bc_lambda);
        if bc_lambda > 0.0 {
            self.bc_steps += total as u64;
        }
        st.ent_coef = f64::from(ent_coef);
        st.lr = lr;
        self.explore.update(total as u64, st.entropy, na);
        if st.episodes > 0 {
            st.reward_mean = ret_sum / st.episodes as f64;
            st.length_mean = len_sum / st.episodes as f64;
            st.touch_rate = st.touches as f64 / st.episodes as f64;
            st.goal_rate = st.goals as f64 / st.episodes as f64;
        }
        st
    }

    /// Taille de l'observation.
    pub fn obs_dim(&self) -> usize {
        self.obs_dim
    }

    /// Nombre d'actions discrètes (table d'actions de l'env).
    pub fn n_actions(&self) -> usize {
        self.n_actions
    }

    /// Change la config de l'env (palier de curriculum) : tous les envs sont remis à zéro immédiatement.
    pub fn set_env_config(&mut self, cfg: &EnvConfig) {
        self.obs = self.env.set_config(cfg);
        self.selfplay = cfg.opponent == Opponent::SelfPlay;
        if !self.selfplay {
            self.frozen = None;
        }
    }

    /// Logits de la politique pour une observation (backend burn), pour les tests d'équivalence.
    pub fn policy_logits(&self, obs: &[f32]) -> Vec<f32> {
        let x = tensor2(obs.to_vec(), 1, self.obs_dim, &self.plain);
        to_vec(self.model.valid().policy.forward(x))
    }

    /// Exporte la politique en safetensors (lisible par `policy-runtime`) + métadonnées JSON voisines.
    pub fn export(&self, path: &Path) -> std::io::Result<()> {
        let t = self.model.policy.to_tensors();
        write_safetensors(path, &t).map_err(|e| std::io::Error::other(e.to_string()))?;
        let meta = format!(
            "{{\"obs_dim\":{},\"actions\":{},\"hidden\":{},\"activation\":\"tanh\",\"action_table\":\"rlgym-90\"}}\n",
            self.obs_dim, self.n_actions, self.cfg.hidden
        );
        std::fs::write(path.with_extension("json"), meta)
    }

    /// Checkpoint complet des poids (acteur + critique). L'état d'Adam n'est pas sauvegardé (reprise avec
    /// moments remis à zéro : acceptable pour le spike).
    pub fn save_checkpoint(&self, path: &Path) -> Result<(), String> {
        self.model
            .clone()
            .save_file(path)
            .map_err(|e| e.to_string())
    }

    /// Charge un checkpoint. Un checkpoint d'avant l'observation adversaire (entrée de `OBS_DIM_V1`) est
    /// reconnu automatiquement : la couche d'entrée est agrandie, les poids des nouvelles entrées sont nuls
    /// (comportement identique tant que l'adversaire est absent).
    /// Remet à zéro les poids d'entrée du bloc adversaire (acteur et critique) : le réseau se comporte alors comme
    /// sans adversaire et réapprend ce bloc. Sert à migrer les checkpoints entraînés avant la correction de
    /// l'ordre des voitures, où ce bloc décrivait la voiture de l'agent (voir `docs/curriculum.md`).
    /// Remplace l'acteur par celui d'une politique exportée (`policy.safetensors`, même architecture) ; le
    /// critique reste tel quel. Sert à repartir d'une politique servie (outil d'essai local) : prévoir un
    /// échauffement du critique (`freeze_policy`) avant d'entraîner la politique.
    pub fn load_actor(&mut self, path: &Path) -> Result<(), String> {
        let t = policy_runtime::read_safetensors(path).map_err(|e| e.to_string())?;
        let policy = self.model.policy.from_tensors(&t, &self.device)?;
        self.model = ActorCritic {
            policy,
            value: self.model.value.clone(),
        };
        Ok(())
    }

    /// Copie de la politique courante, sur le backend sans autodiff (adversaire, référence d'Elo).
    pub fn policy_snapshot(&self) -> Mlp {
        self.model.valid().policy
    }

    /// Backend sans autodiff (celui de [`Trainer::policy_snapshot`]).
    pub fn plain_device(&self) -> Device {
        self.plain.clone()
    }

    pub fn reset_opponent_inputs(&mut self) {
        let m = self.model.clone();
        self.model = ActorCritic {
            policy: m.policy.zero_inputs(OBS_DIM_V1, OBS_DIM_V2),
            value: m.value.zero_inputs(OBS_DIM_V1, OBS_DIM_V2),
        };
    }

    pub fn load_checkpoint(&mut self, path: &Path) -> Result<(), String> {
        let current = self.model.clone();
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            current.try_load_file(path).map_err(|e| e.to_string())
        }));
        if let Ok(Ok(m)) = loaded {
            self.model = m;
            return Ok(());
        }
        // Formats historiques : (entrées, actions) d'avant la v3 (43 / 78) et d'avant l'adversaire (33 / 78).
        for (n_in, n_out) in [(OBS_DIM_V2, N_ACTIONS_V1), (OBS_DIM_V1, N_ACTIONS_V1)] {
            let legacy = ActorCritic {
                policy: Mlp::new(n_in, self.cfg.hidden, n_out, &self.device),
                value: Mlp::new(n_in, self.cfg.hidden, 1, &self.device),
            };
            let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                legacy.try_load_file(path).map_err(|e| e.to_string())
            }));
            if let Ok(Ok(m)) = loaded {
                self.model = ActorCritic {
                    policy: m
                        .policy
                        .widen_input(self.obs_dim)
                        .widen_output(self.n_actions, NEW_ACTION_BIAS),
                    value: m.value.widen_input(self.obs_dim),
                };
                eprintln!(
                    "checkpoint au format historique ({n_in} entrées, {n_out} actions) : réseau agrandi à {} / {}",
                    self.obs_dim, self.n_actions
                );
                return Ok(());
            }
        }
        Err("checkpoint illisible (formats actuel et historiques)".to_string())
    }
}

#[cfg(test)]
mod bc_tests {
    use super::*;

    #[test]
    fn lambda_decroit_lineairement_jusqu_a_zero() {
        let c = BcConfig {
            lambda0: 0.1,
            decay_steps: 1000,
            batch: 8,
        };
        assert!((c.lambda(0) - 0.1).abs() < 1e-7);
        assert!((c.lambda(500) - 0.05).abs() < 1e-7);
        assert_eq!(c.lambda(1000), 0.0);
        assert_eq!(c.lambda(5000), 0.0);
        let off = BcConfig {
            lambda0: 0.0,
            ..c
        };
        assert_eq!(off.lambda(0), 0.0);
    }

    #[test]
    fn la_memoire_tampon_est_bornee_et_ecrase_les_plus_anciens() {
        let mut b = DemoBuffer::new(2, 3);
        for i in 0..5u8 {
            b.push(&[f32::from(i), 0.0], i);
        }
        assert_eq!(b.len(), 3);
        let mut rng = Rng::new(1);
        let (o, a) = b.sample(&mut rng, 50);
        assert_eq!(o.len(), 100);
        // Restent les actions 2, 3, 4 (0 et 1 écrasées) ; l'observation suit l'action.
        assert!(a.iter().all(|&x| (2..5).contains(&x)));
        assert!(a.iter().enumerate().all(|(j, &x)| o[j * 2] == x as f32));
    }
}
