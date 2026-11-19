//! Clips rejouables : état complet au début + contrôles des voitures par décision (+ états clés réguliers).
//!
//! Un clip tient en quelques dizaines de décisions (≈ 5 s). RocketSim est déterministe à état et entrées
//! identiques sur une simulation neuve (test `replay_is_deterministic`), mais pas sur la simulation longue
//! d'un entraînement : RocketSim garde des états internes que `CarState` ne contient pas (suspension, contacts
//! de Bullet), donc un clip pris en plein entraînement rejoué sur une simulation vierge dérive de quelques
//! unités par décision. D'où un état clé tous les [`KEY_EVERY`] pas : le rejeu mesure l'écart à chacun
//! (`Replayer::max_drift`) et s'y recale, ce qui borne la dérive à [`DRIFT_BOUND`] (aussi entre machines).

use std::collections::VecDeque;

use rocketsim_rs::math::{RotMat, Vec3};
use rocketsim_rs::sim::{
    BallHitInfo, BallState, BoostPadState, CarContact, CarControls, CarState, DropshotInfo,
    HeatseekerInfo, WorldContact,
};
use serde::{Deserialize, Serialize};

use crate::{Controls, Sim, SimError};

pub const CLIP_VERSION: u32 = 1;
/// Un état clé toutes les N décisions (5 = 1/3 s à 15 Hz).
pub const KEY_EVERY: u32 = 5;
/// Les pads (34 appels C++) ne sont capturés que tous les [`PAD_EVERY`] états clés : un clip démarre sur l'un d'eux.
pub const PAD_EVERY: u32 = 4;
/// Voitures par clip au plus (agent + adversaire).
const MAX_CARS: usize = 2;
/// Longueur maximale d'un clip en décisions.
pub const MAX_STEPS: u32 = LEAD_STEPS + KEY_EVERY * PAD_EVERY + TRAIL_STEPS + 1;
/// Décisions gardées avant le déclenchement (au moins) et après.
pub const LEAD_STEPS: u32 = 45;
pub const TRAIL_STEPS: u32 = 15;
/// Écart de position (unités RocketSim ≈ cm) toléré entre le rejeu et l'état clé : au-delà, le rejeu n'est plus
/// « à l'identique ». Sur une même machine l'écart mesuré est exactement 0.
/// Âge maximal (ticks) conservé pour un toucher passé : au-delà, RocketSim ne distingue plus (le rejeu
/// fait avancer l'horloge de cette durée avant de restaurer).
const HIT_AGE_CAP: u64 = 600;
pub const DRIFT_TOLERANCE: f32 = 0.5;
/// Écart toléré aux états clés pour un clip qui ne commence pas à l'épisode (rejeu recalé chaque seconde).
pub const DRIFT_BOUND: f32 = 25.0;

/// Contrôles d'une voiture pendant une décision : gaz, direction, tangage, lacet, roulis, puis bits
/// 1 = boost, 2 = saut, 4 = frein à main.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ctl(pub f32, pub f32, pub f32, pub f32, pub f32, pub u8);

impl Ctl {
    pub fn from_controls(c: &Controls) -> Self {
        Ctl(
            c.throttle,
            c.steer,
            c.pitch,
            c.yaw,
            c.roll,
            u8::from(c.boost) | u8::from(c.jump) << 1 | u8::from(c.handbrake) << 2,
        )
    }

    pub fn to_controls(self) -> Controls {
        Controls {
            throttle: self.0,
            steer: self.1,
            pitch: self.2,
            yaw: self.3,
            roll: self.4,
            boost: self.5 & 1 != 0,
            jump: self.5 & 2 != 0,
            handbrake: self.5 & 4 != 0,
        }
    }
}

// Copie des dérivations serde distantes de rocketsim_rs (son module `serde_utils` est privé).
#[derive(Serialize, Deserialize)]
#[serde(remote = "BallHitInfo")]
pub struct BallHitInfoDerive {
    pub is_valid: bool,
    pub relative_pos_on_ball: Vec3,
    pub ball_pos: Vec3,
    pub extra_hit_vel: Vec3,
    pub tick_count_when_hit: u64,
    pub tick_count_when_extra_impulse_applied: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CarControls")]
pub struct CarControlsDerive {
    pub throttle: f32,
    pub steer: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub boost: bool,
    pub jump: bool,
    pub handbrake: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "WorldContact")]
pub struct WorldContactDerive {
    pub has_contact: bool,
    pub contact_normal: Vec3,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CarContact")]
pub struct CarContactDerive {
    pub other_car_id: u32,
    pub cooldown_timer: f32,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CarState")]
pub struct CarStateDerive {
    pub tick_count_since_update: u64,
    pub pos: Vec3,
    pub rot_mat: RotMat,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub is_on_ground: bool,
    pub wheels_with_contact: [bool; 4],
    pub has_jumped: bool,
    pub has_double_jumped: bool,
    pub has_flipped: bool,
    pub flip_rel_torque: Vec3,
    pub jump_time: f32,
    pub flip_time: f32,
    pub is_flipping: bool,
    pub is_jumping: bool,
    pub air_time: f32,
    pub air_time_since_jump: f32,
    pub boost: f32,
    pub time_since_boosted: f32,
    pub is_boosting: bool,
    pub boosting_time: f32,
    pub is_supersonic: bool,
    pub supersonic_time: f32,
    pub handbrake_val: f32,
    pub is_auto_flipping: bool,
    pub auto_flip_timer: f32,
    pub auto_flip_torque_scale: f32,
    #[serde(with = "WorldContactDerive")]
    pub world_contact: WorldContact,
    #[serde(with = "CarContactDerive")]
    pub car_contact: CarContact,
    pub is_demoed: bool,
    pub demo_respawn_timer: f32,
    #[serde(with = "BallHitInfoDerive")]
    pub ball_hit_info: BallHitInfo,
    #[serde(with = "CarControlsDerive")]
    pub last_controls: CarControls,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "BallState")]
pub struct BallStateDerive {
    pub tick_count_since_update: u64,
    pub pos: Vec3,
    pub rot_mat: RotMat,
    pub vel: Vec3,
    pub ang_vel: Vec3,
    pub hs_info: HeatseekerInfo,
    pub ds_info: DropshotInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CarFull(#[serde(with = "CarStateDerive")] pub CarState);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BallFull(#[serde(with = "BallStateDerive")] pub BallState);

/// Boost au sol : (actif, temps de recharge restant).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PadFull(pub bool, pub f32);

/// État complet avant la décision `step` (relative au clip). `pads` n'est rempli que dans le premier.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keyframe {
    pub step: u32,
    pub tick: u64,
    pub ball: BallFull,
    pub cars: Vec<CarFull>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pads: Vec<PadFull>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub version: u32,
    /// Identifiant (attribué au stockage ; vide avant).
    #[serde(default)]
    pub id: String,
    /// Libellé Prometheus de la mécanique détectée (`flip_reset`…).
    pub mechanic: String,
    #[serde(default)]
    pub created_unix: u64,
    /// Pas d'entraînement (itération du learner) au moment de l'enregistrement.
    #[serde(default)]
    pub train_step: u64,
    pub tick_skip: u32,
    /// Équipe de chaque voiture (bleue = true), dans l'ordre des contrôles et des états.
    pub blue: Vec<bool>,
    /// Décision du déclenchement (relative au clip).
    pub trigger_step: u32,
    /// Premier état clé au pas 0 (avec pads), dernier au pas `steps.len()`.
    pub keyframes: Vec<Keyframe>,
    /// `steps[s][voiture]` : contrôles tenus pendant `tick_skip` ticks.
    pub steps: Vec<Vec<Ctl>>,
    /// Version du détecteur de mécaniques qui a déclenché le clip (1 = règles d'origine, 2 = strictes, 3 = strictes + collisions ; 0 =
    /// absent des anciens fichiers, à lire comme 1).
    #[serde(default)]
    pub detector: u32,
    /// Qualité mesurée après la mécanique (vitesse de balle vers le but adverse, but dans les 3 s).
    #[serde(default)]
    pub quality: Option<ClipQuality>,
    /// Valeur (vitesse de balle après toucher, uu/s) et support de la mécanique (pinch : sol, mur, coin…).
    #[serde(default)]
    pub detail: Option<ClipDetail>,
}

/// Détail d'une occurrence (métadonnées d'un clip) : mesure propre à la mécanique et support.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipDetail {
    /// Vitesse de balle après le toucher (uu/s) pour les mécaniques à record, sinon 0.
    pub value: f32,
    /// `none`, `ground`, `wall`, `backwall`, `corner`, `ceiling` (voir `mechanics::Support`).
    pub support: String,
}

/// Qualité d'une mécanique (métadonnées d'un clip).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipQuality {
    /// Vitesse maximale de la balle vers le but adverse (uu/s) dans la seconde qui suit.
    pub ball_speed_to_goal: f32,
    /// But de l'équipe dans les 3 s.
    pub goal_3s: bool,
    /// Score 0..1 (moitié vitesse, moitié but).
    pub score: f32,
}

impl Clip {
    pub fn from_json(b: &[u8]) -> Result<Self, String> {
        let c: Clip = serde_json::from_slice(b).map_err(|e| format!("clip illisible : {e}"))?;
        if c.version != CLIP_VERSION {
            return Err(format!(
                "clip de version {} (ce client lit la {CLIP_VERSION} : rst update ?)",
                c.version
            ));
        }
        let n = c.blue.len();
        let ok = n > 0
            && !c.steps.is_empty()
            && c.tick_skip > 0
            && c.steps.iter().all(|s| s.len() == n)
            && c.keyframes
                .first()
                .is_some_and(|k| k.step == 0 && k.cars.len() == n)
            && c.keyframes.iter().all(|k| k.cars.len() == n);
        if ok {
            Ok(c)
        } else {
            Err("clip incohérent".into())
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("clip sérialisable")
    }

    /// Durée en secondes.
    pub fn seconds(&self) -> f32 {
        self.steps.len() as f32 * self.tick_skip as f32 / crate::TICK_RATE as f32
    }
}

impl Sim {
    /// Capture l'état complet (voitures triées par id, balle ; pads si demandé). `step` à remplir par l'appelant.
    pub fn capture(&mut self, with_pads: bool) -> Keyframe {
        let mut ids = self.arena.get_cars();
        ids.sort_unstable();
        let cars = ids
            .iter()
            .map(|&id| CarFull(self.arena.pin_mut().get_car(id)))
            .collect();
        let pads = if with_pads {
            (0..self.arena.num_pads())
                .map(|i| {
                    let p = self.arena.get_pad_state(i);
                    PadFull(p.is_active, p.cooldown)
                })
                .collect()
        } else {
            Vec::new()
        };
        Keyframe {
            step: 0,
            tick: self.arena.get_tick_count(),
            ball: BallFull(self.arena.pin_mut().get_ball()),
            cars,
            pads,
        }
    }

    /// Rétablit un état capturé sur les voitures `ids` (même ordre que la capture). Les ticks absolus des
    /// touchers sont décalés pour rester relatifs à l'horloge de cette simulation.
    pub fn restore(&mut self, ids: &[u32], kf: &Keyframe) -> Result<(), SimError> {
        // Les ticks de dernier toucher sont absolus : on garde leur âge (borné), pas leur valeur.
        let (now, then) = (self.arena.get_tick_count(), kf.tick);
        let shift = |t: u64| now.saturating_sub(then.saturating_sub(t).min(HIT_AGE_CAP));
        for (&id, c) in ids.iter().zip(&kf.cars) {
            let mut st = c.0;
            st.ball_hit_info.tick_count_when_hit = shift(st.ball_hit_info.tick_count_when_hit);
            st.ball_hit_info.tick_count_when_extra_impulse_applied =
                shift(st.ball_hit_info.tick_count_when_extra_impulse_applied);
            self.arena
                .pin_mut()
                .set_car(id, st)
                .map_err(|_| SimError::UnknownCar(id))?;
        }
        self.arena.pin_mut().set_ball(kf.ball.0);
        for (i, p) in kf.pads.iter().enumerate().take(self.arena.num_pads()) {
            self.arena.pin_mut().set_pad_state(
                i,
                BoostPadState {
                    is_active: p.0,
                    cooldown: p.1,
                    cur_locked_car_id: 0,
                    prev_locked_car_id: 0,
                },
            );
        }
        Ok(())
    }
}

/// Plus grand écart de position (balle ou voiture) entre la simulation et un état clé.
fn drift(sim: &mut Sim, kf: &Keyframe) -> f32 {
    let d = |a: rocketsim_rs::math::Vec3, b: [f32; 3]| {
        ((a.x - b[0]).powi(2) + (a.y - b[1]).powi(2) + (a.z - b[2]).powi(2)).sqrt()
    };
    let snap = sim.snapshot();
    let mut m = d(kf.ball.0.pos, snap.ball.pos);
    for (c, s) in kf.cars.iter().zip(&snap.cars) {
        m = m.max(d(c.0.pos, s.pos));
    }
    m
}

/// Rejoue un clip tick par tick sur une simulation vierge.
pub struct Replayer {
    clip: Clip,
    ids: Vec<u32>,
    step: usize,
    sub: u32,
    /// Se recaler sur les états clés (vrai par défaut ; faux pour mesurer le déterminisme brut).
    pub resync: bool,
    /// Plus grand écart de position constaté aux états clés avant recalage (unités RocketSim).
    pub max_drift: f32,
}

impl Replayer {
    /// `sim` ne doit contenir aucune voiture : le rejeu les ajoute (équipes du clip).
    pub fn start(sim: &mut Sim, clip: Clip) -> Result<Self, SimError> {
        if sim.tick_count() < HIT_AGE_CAP {
            sim.step((HIT_AGE_CAP - sim.tick_count()) as u32);
        }
        let ids: Vec<u32> = clip.blue.iter().map(|&b| sim.add_car(b)).collect();
        let mut r = Self {
            clip,
            ids,
            step: 0,
            sub: 0,
            resync: true,
            max_drift: 0.0,
        };
        r.restart(sim)?;
        Ok(r)
    }

    /// Ids des voitures dans la simulation (ordre du clip).
    pub fn car_ids(&self) -> &[u32] {
        &self.ids
    }

    pub fn clip(&self) -> &Clip {
        &self.clip
    }

    /// Décision en cours et nombre total.
    pub fn progress(&self) -> (usize, usize) {
        (self.step, self.clip.steps.len())
    }

    pub fn restart(&mut self, sim: &mut Sim) -> Result<(), SimError> {
        sim.restore(&self.ids, &self.clip.keyframes[0])?;
        for &id in &self.ids {
            sim.set_controls(id, Controls::default())?;
        }
        (self.step, self.sub) = (0, 0);
        Ok(())
    }

    /// Avance d'un tick de physique ; vrai quand le clip est fini (état final atteint et mesuré).
    pub fn tick(&mut self, sim: &mut Sim) -> bool {
        if self.sub == 0 {
            let step = self.step as u32;
            if let Some(kf) = self.clip.keyframes.iter().find(|k| k.step == step) {
                self.max_drift = self.max_drift.max(drift(sim, kf));
                if self.resync && step > 0 && (step as usize) < self.clip.steps.len() {
                    let _ = sim.restore(&self.ids, kf);
                }
            }
            if self.step >= self.clip.steps.len() {
                return true;
            }
            for (&id, ctl) in self.ids.iter().zip(&self.clip.steps[self.step]) {
                let _ = sim.set_controls(id, ctl.to_controls());
            }
        }
        sim.step(1);
        self.sub += 1;
        if self.sub == self.clip.tick_skip {
            self.sub = 0;
            self.step += 1;
        }
        false
    }

    /// Rejoue tout le clip d'un coup (tests, vérification) et retourne l'écart maximal aux états clés.
    pub fn run_all(&mut self, sim: &mut Sim) -> f32 {
        while !self.tick(sim) {}
        self.max_drift
    }
}

/// Bits de [`CarTrack::flags`].
pub const FLAG_GROUND: u8 = 1;
pub const FLAG_BOOST: u8 = 2;
pub const FLAG_JUMP: u8 = 4;
pub const FLAG_FLIP: u8 = 8;
pub const FLAG_DEMO: u8 = 16;

/// Une voiture au fil du temps, tableaux plats : `pose` = 6 flottants par image (position x y z en uu, puis
/// axes avant et haut ; la droite s'en déduit), `boost` (0–100) et `flags` une valeur par image.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CarTrack {
    pub blue: bool,
    pub pose: Vec<f32>,
    pub boost: Vec<f32>,
    pub flags: Vec<u8>,
    /// Vitesse linéaire (uu/s) et angulaire (rad/s, repère monde), 3 flottants par image. Absentes des réponses
    /// d'avant l'ajout du champ.
    #[serde(default)]
    pub vel: Vec<f32>,
    #[serde(default)]
    pub ang: Vec<f32>,
}

/// Boosts au sol d'un rejeu : liste fixe (une fois) puis changements d'état échantillonnés.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PadTrack {
    /// Position x y z (uu) de chaque pad, 3 flottants par pad.
    pub pos: Vec<f32>,
    /// Gros pad (100 de boost) ou petit (12).
    pub big: Vec<bool>,
    /// Actif à l'image 0.
    pub active0: Vec<bool>,
    /// Changements, 3 entiers par événement : image, pad, 1 = réactivé / 0 = ramassé.
    pub events: Vec<u32>,
}

/// Rejeu échantillonné d'un clip (repère RocketSim : z vers le haut, unités ≈ cm).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trajectory {
    pub hz: u32,
    pub frames: usize,
    /// Écart maximal aux états clés avant recalage (uu).
    pub max_drift: f32,
    /// Balle : même disposition que [`CarTrack::pose`].
    pub ball: Vec<f32>,
    /// Vitesse (uu/s) et vitesse angulaire (rad/s) de la balle, 3 flottants par image (champs optionnels).
    #[serde(default)]
    pub ball_vel: Vec<f32>,
    #[serde(default)]
    pub ball_ang: Vec<f32>,
    /// Voitures dans l'ordre du clip ; la 0 est l'agent.
    pub cars: Vec<CarTrack>,
    /// Pads (absent des réponses d'avant l'ajout du champ).
    #[serde(default)]
    pub pads: PadTrack,
}

fn r1(x: f32) -> f32 {
    (x * 10.0).round() / 10.0
}

fn r4(x: f32) -> f32 {
    (x * 10_000.0).round() / 10_000.0
}

fn push_pose(out: &mut Vec<f32>, p: Vec3, f: Vec3, u: Vec3) {
    out.extend([r1(p.x), r1(p.y), r1(p.z)]);
    out.extend([r4(f.x), r4(f.y), r4(f.z), r4(u.x), r4(u.y), r4(u.z)]);
}

fn push_vec(out: &mut Vec<f32>, v: Vec3) {
    out.extend([r1(v.x), r1(v.y), r1(v.z)]);
}

impl Sim {
    /// Rejoue `clip` sur cette simulation (sans voiture) et échantillonne à `hz` (1, 15, 30, 60 ou 120 : un
    /// diviseur de [`crate::TICK_RATE`] et de `tick_skip`).
    pub fn trajectory(&mut self, clip: Clip, hz: u32) -> Result<Trajectory, SimError> {
        let every = (crate::TICK_RATE as u32 / hz.max(1)).max(1) as u64;
        let mut rp = Replayer::start(self, clip)?;
        let ids = rp.car_ids().to_vec();
        let mut t = Trajectory {
            hz,
            frames: 0,
            max_drift: 0.0,
            ball: Vec::new(),
            ball_vel: Vec::new(),
            ball_ang: Vec::new(),
            cars: rp
                .clip()
                .blue
                .iter()
                .map(|&blue| CarTrack {
                    blue,
                    pose: Vec::new(),
                    boost: Vec::new(),
                    flags: Vec::new(),
                    vel: Vec::new(),
                    ang: Vec::new(),
                })
                .collect(),
            pads: PadTrack::default(),
        };
        for c in self.pads() {
            t.pads
                .pos
                .extend([r1(c.pos[0]), r1(c.pos[1]), r1(c.pos[2])]);
            t.pads.big.push(c.big);
        }
        let mut prev: Vec<bool> = Vec::new();
        let mut sample = |sim: &mut Sim| {
            let now: Vec<bool> = sim.pads().iter().map(|p| p.active).collect();
            if prev.is_empty() {
                t.pads.active0 = now.clone();
            } else {
                for (i, (&a, &b)) in prev.iter().zip(&now).enumerate() {
                    if a != b {
                        t.pads
                            .events
                            .extend([t.frames as u32, i as u32, u32::from(b)]);
                    }
                }
            }
            prev = now;
            let b = sim.arena.pin_mut().get_ball();
            push_pose(&mut t.ball, b.pos, b.rot_mat.forward, b.rot_mat.up);
            push_vec(&mut t.ball_vel, b.vel);
            push_vec(&mut t.ball_ang, b.ang_vel);
            for (&id, track) in ids.iter().zip(&mut t.cars) {
                let c = sim.arena.pin_mut().get_car(id);
                push_pose(&mut track.pose, c.pos, c.rot_mat.forward, c.rot_mat.up);
                push_vec(&mut track.vel, c.vel);
                push_vec(&mut track.ang, c.ang_vel);
                track.boost.push(r1(c.boost));
                let on = |cond: bool, bit: u8| if cond { bit } else { 0 };
                track.flags.push(
                    on(c.is_on_ground, FLAG_GROUND)
                        + on(c.is_boosting, FLAG_BOOST)
                        + on(c.has_jumped, FLAG_JUMP)
                        + on(c.is_flipping, FLAG_FLIP)
                        + on(c.is_demoed, FLAG_DEMO),
                );
            }
            t.frames += 1;
        };
        sample(self);
        let mut n = 0u64;
        loop {
            if rp.tick(self) {
                // État final (déjà pris si la durée tombe juste sur une image).
                if n % every != 0 {
                    sample(self);
                }
                break;
            }
            n += 1;
            if n % every == 0 {
                sample(self);
            }
        }
        t.max_drift = rp.max_drift;
        Ok(t)
    }
}

struct Pending {
    mechanic: &'static str,
    trigger: u32,
    start: u32,
}

/// Enregistre en continu les contrôles et des états clés ; un déclenchement fige un clip après [`TRAIL_STEPS`].
pub struct ClipRecorder {
    tick_skip: u32,
    step: u32,
    keys: VecDeque<(u32, Keyframe)>,
    /// Contrôles des décisions `ctl_first ..`.
    ctl: VecDeque<[Ctl; MAX_CARS]>,
    cars: usize,
    ctl_first: u32,
    pending: Option<Pending>,
    quiet_until: u32,
    /// Silence (décisions) après un clip, pour ne pas enregistrer deux fois la même action.
    pub cooldown: u32,
}

impl ClipRecorder {
    pub fn new(tick_skip: u32) -> Self {
        Self {
            tick_skip,
            step: 0,
            keys: VecDeque::new(),
            ctl: VecDeque::new(),
            cars: 0,
            ctl_first: 0,
            pending: None,
            quiet_until: 0,
            cooldown: 150,
        }
    }

    /// Nouvel épisode (appelé avant de toucher à la simulation, après [`ClipRecorder::finish`]).
    pub fn reset(&mut self) {
        self.step = 0;
        self.keys.clear();
        self.ctl.clear();
        self.ctl_first = 0;
        self.pending = None;
        self.quiet_until = 0;
    }

    /// À appeler avant d'appliquer les contrôles d'une décision (`ctls` dans l'ordre des ids triés).
    pub fn begin_step(&mut self, sim: &mut Sim, ctls: &[Controls]) {
        if self.step % KEY_EVERY == 0 {
            let mut kf = sim.capture(self.step % (KEY_EVERY * PAD_EVERY) == 0);
            kf.step = self.step;
            if self.keys.is_empty() {
                self.ctl.clear();
                self.ctl_first = self.step;
            }
            self.keys.push_back((self.step, kf));
        }
        let mut row = [Ctl(0.0, 0.0, 0.0, 0.0, 0.0, 0); MAX_CARS];
        for (r, c) in row.iter_mut().zip(ctls) {
            *r = Ctl::from_controls(c);
        }
        self.cars = ctls.len().min(MAX_CARS);
        self.ctl.push_back(row);
    }

    /// Une mécanique vient d'être reconnue pendant la décision en cours (ignoré pendant un clip en cours ou le
    /// silence qui suit un clip). Vrai si le déclenchement est retenu.
    pub fn trigger(&mut self, mechanic: &'static str) -> bool {
        if self.pending.is_none() && self.step >= self.quiet_until {
            let start = self.keys.front().map_or(self.step, |k| k.0);
            self.pending = Some(Pending {
                mechanic,
                trigger: self.step,
                start,
            });
            true
        } else {
            false
        }
    }

    /// À appeler après la décision. Retourne le clip fini si le délai après déclenchement est écoulé.
    pub fn end_step(&mut self, sim: &mut Sim) -> Option<Clip> {
        self.step += 1;
        let done = self
            .pending
            .as_ref()
            .is_some_and(|p| self.step >= p.trigger + 1 + TRAIL_STEPS);
        let out = if done { self.build(sim) } else { None };
        // Garder LEAD_STEPS avant le pas courant (+ jusqu'à l'état clé qui précède), ou le début d'un clip en attente.
        let keep_from = self
            .pending
            .as_ref()
            .map_or(self.step.saturating_sub(LEAD_STEPS), |p| p.start);
        // Le premier état clé garde toujours ses pads : un clip démarre dessus.
        if let Some(j) = self
            .keys
            .iter()
            .rposition(|k| k.0 <= keep_from && !k.1.pads.is_empty())
        {
            self.keys.drain(..j);
        }
        if let Some(first) = self.keys.front().map(|k| k.0) {
            while self.ctl_first < first && !self.ctl.is_empty() {
                self.ctl.pop_front();
                self.ctl_first += 1;
            }
        }
        out
    }

    /// Fin d'épisode : fige le clip en attente avec ce qu'on a (ou rien).
    pub fn finish(&mut self, sim: &mut Sim) -> Option<Clip> {
        let c = self.build(sim);
        self.reset();
        c
    }

    fn build(&mut self, sim: &mut Sim) -> Option<Clip> {
        let p = self.pending.take()?;
        self.quiet_until = self.step + self.cooldown;
        let start_idx = self.keys.iter().position(|k| k.0 >= p.start)?;
        let start = self.keys[start_idx].0;
        let end = self.step;
        let mut keyframes: Vec<Keyframe> = self
            .keys
            .iter()
            .skip(start_idx)
            .filter(|k| k.0 < end)
            .map(|(s, kf)| {
                let mut k = kf.clone();
                k.step = s - start;
                if *s != start {
                    k.pads.clear();
                }
                k
            })
            .collect();
        let mut last = sim.capture(false);
        last.step = end - start;
        keyframes.push(last);
        let n = self.cars;
        let steps: Vec<Vec<Ctl>> = self
            .ctl
            .iter()
            .skip((start - self.ctl_first) as usize)
            .take((end - start) as usize)
            .map(|r| r[..n].to_vec())
            .collect();
        if steps.len() != (end - start) as usize {
            return None;
        }
        let blue = {
            let mut ids = sim.arena.get_cars();
            ids.sort_unstable();
            ids.iter()
                .map(|&i| sim.arena.get_car_team(i) == rocketsim_rs::sim::Team::Blue)
                .collect()
        };
        Some(Clip {
            version: CLIP_VERSION,
            id: String::new(),
            mechanic: p.mechanic.to_string(),
            created_unix: 0,
            train_step: 0,
            tick_skip: self.tick_skip,
            blue,
            trigger_step: p.trigger - start,
            keyframes,
            steps,
            detector: 0,
            quality: None,
            detail: None,
        })
    }
}
