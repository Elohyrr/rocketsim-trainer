//! PPO discret : acteur/critique MLP, GAE(λ), objectif clippé, Adam.

use std::collections::BTreeMap;
use std::path::Path;

use crate::explore::{self, Explore};
use burn::module::{Module, Param};
use burn::nn::{Linear, LinearConfig};
use burn::optim::{AdamConfig, GradientsParams, ModuleOptimizer};
use burn::tensor::activation::log_softmax;
use burn::tensor::{Device, Tensor, TensorData};
use policy_runtime::{Tensor as StTensor, write_safetensors};
use rl_env::{EnvConfig, KickoffResult, OBS_DIM_V1, Opponent, Outcome, Rng, VecEnv};

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

    /// Met à zéro les poids des entrées `from..` de la couche d'entrée (biais et autres couches inchangés).
    pub fn zero_inputs_from(self, from: usize) -> Self {
        let w = self.l0.weight.val();
        let [rows, hidden] = w.dims();
        if from >= rows {
            return self;
        }
        let keep = w.clone().slice([0..from, 0..hidden]);
        let pad = Tensor::zeros([rows - from, hidden], &w.device());
        let weight = Param::from_tensor(Tensor::cat(vec![keep, pad], 0).detach());
        Self {
            l0: Linear {
                weight,
                bias: self.l0.bias,
            },
            ..self
        }
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
    /// Coefficient d'entropie et lr utilisés, époques PPO effectivement faites (plafond de KL).
    pub ent_coef: f64,
    pub lr: f64,
    pub epochs_run: usize,
    pub collect_seconds: f64,
    pub train_seconds: f64,
}

pub struct Trainer {
    pub cfg: PpoConfig,
    env: VecEnv,
    model: ActorCritic,
    opt: ModuleOptimizer,
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
    /// Régulateur d'exploration (ent_coef adaptatif, lr décroissant) ; état persisté par le learner.
    pub explore: Explore,
    /// Échauffement du critique : la politique n'est pas mise à jour (voir [`Trainer::load_actor`]).
    pub freeze_policy: bool,
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
        let obs = env.reset();
        let selfplay = env_cfg_selfplay;
        Ok(Self {
            rng: Rng::new(cfg.seed ^ 0xA5A5),
            explore: Explore::default(),
            freeze_policy: false,
            cfg,
            env,
            model,
            opt,
            device,
            plain,
            obs,
            obs_dim,
            n_actions,
            frozen: None,
            frozen_age: 0,
            selfplay,
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

        let mut obs_buf = Vec::with_capacity(h * n * od);
        let mut act_buf = Vec::with_capacity(h * n);
        let mut logp_buf = Vec::with_capacity(h * n);
        let mut val_buf = Vec::with_capacity((h + 1) * n);
        let mut rew_buf = Vec::with_capacity(h * n);
        let mut done_buf = Vec::with_capacity(h * n);
        let mut st = IterStats::default();
        let (mut ret_sum, mut len_sum) = (0.0f64, 0.0f64);

        for _ in 0..h {
            let x = tensor2(self.obs.clone(), n, od, &idev);
            let logp_all = log_softmax(inner.policy.forward(x.clone()), 1);
            let values = to_vec(inner.value.forward(x));
            let lp = to_vec(logp_all);
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
            let opp_acts = self.frozen.as_ref().map(|f| {
                let ox = tensor2(self.env.opp_obs().to_vec(), n, od, &idev);
                let lp = to_vec(log_softmax(f.forward(ox), 1));
                (0..n)
                    .map(|e| {
                        let row = &lp[e * na..(e + 1) * na];
                        let u = self.rng.unit();
                        let mut c = 0.0f32;
                        row.iter()
                            .position(|l| {
                                c += l.exp();
                                u < c
                            })
                            .unwrap_or(na - 1)
                    })
                    .collect::<Vec<usize>>()
            });
            let step = self.env.step_vs(&acts, opp_acts.as_deref());
            st.mechanics.add(&step.mechanics);
            for e in 0..n {
                rew_buf.push(step.reward[e]);
                done_buf.push(step.terminated[e] || step.truncated[e]);
            }
            // Troncature (timeout) : on traite comme une fin d'épisode (valeur future nulle) — approximation
            // acceptable pour des épisodes courts ; voir docs/bench.md.
            for ep in &step.finished {
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
                st.touches += u64::from(ep.touched);
                st.goals += u64::from(ep.outcome == Outcome::Goal);
                st.own_goals += u64::from(ep.outcome == Outcome::OwnGoal);
                ret_sum += f64::from(ep.ret);
                len_sum += f64::from(ep.length);
            }
            self.obs = step.obs;
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
        let (mut pl, mut vl, mut en, mut kl, mut cnt) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let (ent_coef, lr) = (self.explore.ent_coef as f32, self.explore.lr());
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
                for (j, &i) in chunk.iter().enumerate() {
                    ob.extend_from_slice(&obs_buf[i * od..(i + 1) * od]);
                    oh[j * na + act_buf[i] as usize] = 1.0;
                    olp.push(logp_buf[i]);
                    ad.push(adv[i]);
                    rt.push(ret[i]);
                }
                let d = &self.device;
                let x = tensor2(ob, m, od, d);
                let onehot = tensor2(oh, m, na, d);
                let old_lp = tensor2(olp, m, 1, d);
                let adv_t = tensor2(ad, m, 1, d);
                let ret_t = tensor2(rt, m, 1, d);

                let logits = self.model.policy.forward(x.clone());
                let logits = if self.freeze_policy {
                    logits.detach()
                } else {
                    logits
                };
                let logp_all = log_softmax(logits, 1);
                let logp = (logp_all.clone() * onehot).sum_dim(1);
                let ratio = (logp.clone() - old_lp.clone()).exp();
                let s1 = ratio.clone() * adv_t.clone();
                let s2 = ratio
                    .clone()
                    .clamp(1.0 - self.cfg.clip, 1.0 + self.cfg.clip)
                    * adv_t;
                let pol_loss = -(s1.min_pair(s2)).mean();
                let v = self.model.value.forward(x);
                let val_loss = (v - ret_t).powf_scalar(2.0).mean();
                let entropy = -(logp_all.clone().exp() * logp_all).sum_dim(1).mean();
                let loss = if self.freeze_policy {
                    val_loss.clone() * self.cfg.vf_coef
                } else {
                    pol_loss.clone() + val_loss.clone() * self.cfg.vf_coef
                        - entropy.clone() * ent_coef
                };

                pl += scalar(pol_loss.clone().reshape([1]));
                vl += scalar(val_loss.clone().reshape([1]));
                en += scalar(entropy.reshape([1]));
                kl += scalar((old_lp - logp).mean().reshape([1]));
                cnt += 1.0;

                let grads = GradientsParams::from_grads(loss.backward(), &self.model);
                self.model = self.opt.step(lr, self.model.clone(), grads);
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
        st.entropy = en / cnt;
        st.kl = kl / cnt;
        st.ent_coef = f64::from(ent_coef);
        st.lr = lr;
        self.explore.update(total as u64, st.entropy);
        if st.episodes > 0 {
            st.reward_mean = ret_sum / st.episodes as f64;
            st.length_mean = len_sum / st.episodes as f64;
            st.touch_rate = st.touches as f64 / st.episodes as f64;
            st.goal_rate = st.goals as f64 / st.episodes as f64;
        }
        st
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
        let mut t = BTreeMap::new();
        let layers = [
            ("policy.0", &self.model.policy.l0),
            ("policy.1", &self.model.policy.l1),
            ("policy.2", &self.model.policy.out),
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
        write_safetensors(path, &t).map_err(|e| std::io::Error::other(e.to_string()))?;
        let meta = format!(
            "{{\"obs_dim\":{},\"actions\":{},\"hidden\":{},\"activation\":\"tanh\",\"action_table\":\"rlgym-78\"}}\n",
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
        let layer = |name: &str, l: &Linear| -> Result<Linear, String> {
            let w = t
                .get(&format!("{name}.weight"))
                .ok_or(format!("{name}.weight absent"))?;
            let b = t
                .get(&format!("{name}.bias"))
                .ok_or(format!("{name}.bias absent"))?;
            let [r, c] = l.weight.dims();
            if w.shape != [r, c] || b.shape != [c] {
                return Err(format!(
                    "{name} : dimensions {:?} au lieu de [{r}, {c}]",
                    w.shape
                ));
            }
            Ok(Linear {
                weight: Param::from_tensor(tensor2(w.data.clone(), r, c, &self.device).detach()),
                bias: Some(Param::from_tensor(
                    Tensor::<1>::from_data(TensorData::new(b.data.clone(), [c]), &self.device)
                        .detach(),
                )),
            })
        };
        let p = &self.model.policy;
        let policy = Mlp {
            l0: layer("policy.0", &p.l0)?,
            l1: layer("policy.1", &p.l1)?,
            out: layer("policy.2", &p.out)?,
        };
        self.model = ActorCritic {
            policy,
            value: self.model.value.clone(),
        };
        Ok(())
    }

    pub fn reset_opponent_inputs(&mut self) {
        let m = self.model.clone();
        self.model = ActorCritic {
            policy: m.policy.zero_inputs_from(OBS_DIM_V1),
            value: m.value.zero_inputs_from(OBS_DIM_V1),
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
        let legacy = ActorCritic {
            policy: Mlp::new(OBS_DIM_V1, self.cfg.hidden, self.n_actions, &self.device),
            value: Mlp::new(OBS_DIM_V1, self.cfg.hidden, 1, &self.device),
        };
        let legacy = legacy
            .try_load_file(path)
            .map_err(|e| format!("checkpoint illisible (formats actuel et historique) : {e}"))?;
        self.model = ActorCritic {
            policy: legacy.policy.widen_input(self.obs_dim),
            value: legacy.value.widen_input(self.obs_dim),
        };
        eprintln!(
            "checkpoint au format historique ({OBS_DIM_V1} entrées) : couche d'entrée agrandie à {}",
            self.obs_dim
        );
        Ok(())
    }
}
