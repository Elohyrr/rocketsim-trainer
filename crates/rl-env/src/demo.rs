//! Démonstrations expertes scriptées (voir `docs/demonstration.md`) : half flip, flip shot, speed flip de kickoff et
//! flick, générées dans des états de départ variés (ceux des drills et des kickoffs) puis **vérifiées par le détecteur**
//! de `mechanics` : une séquence où la mécanique n'est pas reconnue est jetée.
//!
//! Les scripts tournent au rythme de décision de l'agent (`tick_skip` 8, 15 Hz) et ne choisissent que des actions de la
//! table de 90 : ce qui est vérifié est exactement ce que le clonage apprend (observation, indice d'action).
use crate::drill::speed_toward_goal;
use crate::{ActionTable, Env, EnvConfig, OBS_DIM, Opponent, Rng};
use mechanics::Mechanic;
use scenarios::Drill;
use sim_core::{Controls, SimError};

/// Mécanique démontrée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoKind {
    HalfFlip,
    FlipShot,
    SpeedFlip,
    Flick,
}

impl DemoKind {
    pub const ALL: [DemoKind; 4] = [
        DemoKind::HalfFlip,
        DemoKind::FlipShot,
        DemoKind::SpeedFlip,
        DemoKind::Flick,
    ];

    pub fn label(self) -> &'static str {
        self.mechanic().label()
    }

    pub fn mechanic(self) -> Mechanic {
        match self {
            DemoKind::HalfFlip => Mechanic::HalfFlip,
            DemoKind::FlipShot => Mechanic::FlipShot,
            DemoKind::SpeedFlip => Mechanic::SpeedFlip,
            DemoKind::Flick => Mechanic::Flick,
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&k| k == self).unwrap_or(0)
    }
}

/// Une séquence vérifiée : observations (à plat, `OBS_DIM` par décision) et actions de la table.
#[derive(Clone, Debug)]
pub struct Demo {
    pub kind: DemoKind,
    pub obs: Vec<f32>,
    pub actions: Vec<u8>,
}

impl Demo {
    pub fn len(&self) -> usize {
        self.actions.len()
    }
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// Décisions gardées après la détection (l'atterrissage qui la déclenche est montré, pas plus).
const TAIL: usize = 2;
/// Décisions maximales d'une séquence.
const MAX_STEPS: usize = 120;
/// Vitesse de balle vers le but (uu/s) exigée en plus de la détection pour un flip shot : on ne montre pas un tir
/// qui part de travers.
const FLIP_SHOT_MIN_TOWARD: f32 = 600.0;

/// Action de la table la plus proche des contrôles voulus.
pub fn nearest_action(table: &ActionTable, want: &Controls) -> usize {
    let mut best = (f32::MAX, 0);
    for i in 0..table.len() {
        let c = table.get(i);
        let b = |x: bool| if x { 1.0f32 } else { 0.0 };
        let cost = (c.throttle - want.throttle).powi(2)
            + (c.steer - want.steer).powi(2)
            + (c.pitch - want.pitch).powi(2)
            + (c.yaw - want.yaw).powi(2)
            + (c.roll - want.roll).powi(2)
            + 8.0 * (b(c.jump) - b(want.jump)).powi(2)
            + 4.0 * (b(c.boost) - b(want.boost)).powi(2)
            + 0.01 * (b(c.handbrake) - b(want.handbrake)).powi(2);
        if cost < best.0 {
            best = (cost, i);
        }
    }
    best.1
}

/// Générateur : un environnement privé (sans adversaire, détecteur actif) et ses compteurs de réussite.
pub struct DemoGen {
    env: Env,
    table: ActionTable,
    rng: Rng,
    /// Essais et réussites par mécanique (ordre de [`DemoKind::ALL`]).
    pub tried: [u64; 4],
    pub kept: [u64; 4],
    /// Paramètres du dernier script (diagnostic du banc).
    pub last: String,
    /// Trace des décisions sur stderr (banc).
    pub trace: bool,
    pub diag: bool,
}

/// État du script d'une séquence (machine à états au rythme des décisions).
#[derive(Clone, Copy, Debug, Default)]
struct Script {
    /// Décisions depuis le début de la phase aérienne (0 = pas encore parti).
    air: u32,
    started: bool,
    /// Paramètres tirés au début de la séquence.
    wait: u32,
    pre: u32,
    side: f32,
    roll: f32,
    yaw: f32,
    ttc: f32,
    cancel_n: u32,
    pre_pitch: f32,
    dodge_pitch: f32,
    /// Suite aérienne tirée au sort (tangage, lacet, roulis) pour les séquences recherchées au hasard.
    tail: [[f32; 3]; 16],
}

impl DemoGen {
    /// `init_meshes` doit avoir réussi avant.
    pub fn new(seed: u64) -> Result<Self, SimError> {
        let cfg = EnvConfig {
            end_on_touch: false,
            opponent: Opponent::None,
            max_steps: 10_000,
            detect_mechanics: true,
            ..EnvConfig::default()
        };
        Ok(Self {
            env: Env::new(cfg, seed)?,
            table: ActionTable::rlgym(),
            rng: Rng::new(seed ^ 0xD3A0),
            tried: [0; 4],
            kept: [0; 4],
            last: String::new(),
            trace: false,
            diag: false,
        })
    }

    /// Une séquence de `kind` (None si le détecteur ne l'a pas reconnue : échantillon jeté).
    pub fn sample(&mut self, kind: DemoKind) -> Option<Demo> {
        let i = kind.index();
        self.tried[i] += 1;
        let d = self.run(kind)?;
        self.kept[i] += 1;
        Some(d)
    }

    fn seen(&mut self, m: Mechanic) -> u64 {
        let a = self.env.take_drill_report().mechanics.get(m);
        let b = self.env.take_mechanics().get(m);
        a + b
    }

    fn run(&mut self, kind: DemoKind) -> Option<Demo> {
        match kind {
            DemoKind::HalfFlip => {
                self.env.reset_drill_with(Drill::Turnaround);
            }
            DemoKind::FlipShot => {
                self.env.reset_drill_with(Drill::FlipShot);
            }
            DemoKind::Flick => {
                self.env.reset_drill_with(Drill::Flick);
            }
            DemoKind::SpeedFlip => {
                let s = (self.rng.next_u64() % 5) as usize;
                self.env.reset_kickoff(Some(s));
            }
        }
        // Les compteurs du reset (rien) sont vidés.
        let _ = self.env.take_drill_report();
        let _ = self.env.take_mechanics();
        let mut sc = self.draw(kind);
        self.last = format!(
            "wait={} pre={} cancel={} roll={} yaw={} ttc={:.2} pp={}",
            sc.wait, sc.pre, sc.cancel_n, sc.roll, sc.yaw, sc.ttc, sc.pre_pitch
        );
        let (mut obs, mut actions) = (Vec::new(), Vec::new());
        let mut found: Option<usize> = None;
        let mut toward_max = f32::MIN;
        let (mut t_dodge, mut t_land, mut vmax) = (None, None, 0.0f32);
        for step in 0..MAX_STEPS {
            let o = self.env.observe();
            let want = self.control(kind, &mut sc, step);
            let a = nearest_action(&self.table, &want);
            obs.extend_from_slice(&o);
            actions.push(a as u8);
            if self.trace {
                let c = self.env.sim.car_tick(self.env.car);
                eprintln!(
                    "{step:3} a={a:2} {:?} gnd={} z={:.0} v=({:.0},{:.0},{:.0}) fwd=({:.2},{:.2},{:.2}) flip={} tq=({:.1},{:.1})",
                    self.table.get(a),
                    c.on_ground,
                    c.pos[2],
                    c.vel[0],
                    c.vel[1],
                    c.vel[2],
                    c.forward[0],
                    c.forward[1],
                    c.forward[2],
                    c.has_flipped,
                    c.flip_rel_torque[0],
                    c.flip_rel_torque[1]
                );
            }
            let info = self.env.step(a);
            if self.diag {
                let c = self.env.sim.car_tick(self.env.car);
                if c.has_flipped && t_dodge.is_none() {
                    t_dodge = Some(step);
                }
                if t_dodge.is_some() && c.on_ground && t_land.is_none() {
                    t_land = Some(step);
                }
                vmax = vmax.max(c.vel[0].hypot(c.vel[1]));
            }
            let b = self.env.sim.ball_tick();
            toward_max = toward_max.max(speed_toward_goal(b.pos, b.vel));
            if found.is_none() && self.seen(kind.mechanic()) > 0 {
                found = Some(step);
            }
            if let Some(f) = found
                && step >= f + TAIL
            {
                break;
            }
            if info.terminated || info.truncated {
                break;
            }
        }
        if self.diag {
            self.last = format!("dt={:?} vmax={:.0} {}", t_dodge.zip(t_land).map(|(a, b)| b - a), vmax, self.last);
        }
        let f = found?;
        if kind == DemoKind::FlipShot && toward_max < FLIP_SHOT_MIN_TOWARD {
            return None;
        }
        let n = (f + 1 + TAIL).min(actions.len());
        obs.truncate(n * OBS_DIM);
        actions.truncate(n);
        Some(Demo { kind, obs, actions })
    }

    fn draw(&mut self, kind: DemoKind) -> Script {
        let r = &mut self.rng;
        let sign = |r: &mut Rng| if r.unit() < 0.5 { -1.0 } else { 1.0 };
        let mut s = Script {
            side: sign(r),
            ..Script::default()
        };
        match kind {
            DemoKind::HalfFlip => {
                s.wait = (r.next_u64() % 2) as u32;
                s.cancel_n = 4 + (r.next_u64() % 3) as u32;
                s.roll = sign(r);
                s.yaw = if r.unit() < 0.5 { s.roll } else { 0.0 };
            }
            DemoKind::FlipShot => {
                s.ttc = r.range(0.28, 0.5);
                s.wait = (r.next_u64() % 2) as u32;
                s.roll = [0.0, 0.0, 1.0, -1.0][(r.next_u64() % 4) as usize];
                s.pre_pitch = if r.unit() < 0.3 { 1.0 } else { 0.0 };
            }
            DemoKind::SpeedFlip => {
                s.pre = 3 + (r.next_u64() % 8) as u32;
                s.wait = (r.next_u64() % 2) as u32;
                s.roll = sign(r);
                s.pre_pitch = [-1.0f32, 0.0, 1.0][(r.next_u64() % 3) as usize];
                s.cancel_n = 1 + (r.next_u64() % 3) as u32;
                let mut cur = [1.0, 0.0, s.roll];
                for t in s.tail.iter_mut() {
                    if r.unit() < 0.4 {
                        let pick = |r: &mut Rng| [-1.0f32, 0.0, 1.0][(r.next_u64() % 3) as usize];
                        let p = [-1.0f32, -1.0, 0.0, 1.0][(r.next_u64() % 4) as usize];
                        cur = [p, pick(r), pick(r)];
                    }
                    *t = cur;
                }
            }
            DemoKind::Flick => {
                s.pre = 10 + (r.next_u64() % 20) as u32;
                s.wait = (r.next_u64() % 2) as u32;
                s.roll = [0.0, 1.0, -1.0][(r.next_u64() % 3) as usize];
                s.pre_pitch = if r.unit() < 0.5 { 1.0 } else { 0.0 };
            }
        }
        s
    }

    /// Contrôles voulus à cette décision.
    fn control(&mut self, kind: DemoKind, s: &mut Script, step: usize) -> Controls {
        let car = self.env.sim.car_tick(self.env.car);
        let ball = self.env.sim.ball_tick();
        // Cible : la balle (half flip, flip shot, kickoff) ou le but (flick).
        let goal = [0.0f32, 5120.0];
        let tgt = match kind {
            DemoKind::Flick => goal,
            _ => [ball.pos[0], ball.pos[1]],
        };
        let d = [tgt[0] - car.pos[0], tgt[1] - car.pos[1]];
        let dn = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1.0);
        let d = [d[0] / dn, d[1] / dn];
        let f = [car.forward[0], car.forward[1]];
        let fn_ = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-3);
        let cos = (f[0] * d[0] + f[1] * d[1]) / fn_;
        let cross = (f[0] * d[1] - f[1] * d[0]) / fn_;
        let ang = cross.atan2(cos);
        let steer = (ang * 3.0).clamp(-1.0, 1.0);
        let drive = |boost: bool| Controls {
            throttle: 1.0,
            steer,
            yaw: steer,
            boost,
            ..Default::default()
        };
        let a = s.air;
        let aerial = !car.on_ground;
        // Remise à plat en l'air : tangage et roulis pour ramener nez et côté à l'horizontale, lacet vers la cible.
        let level = |boost: bool| Controls {
            pitch: if car.forward[2] > 0.25 {
                -1.0
            } else if car.forward[2] < -0.25 {
                1.0
            } else {
                0.0
            },
            roll: if car.right[2] > 0.25 {
                1.0
            } else if car.right[2] < -0.25 {
                -1.0
            } else {
                0.0
            },
            yaw: if ang.abs() > 0.4 { steer.signum() } else { 0.0 },
            steer: if ang.abs() > 0.4 { steer.signum() } else { 0.0 },
            boost,
            throttle: if boost { 1.0 } else { 0.0 },
            ..Default::default()
        };
        match kind {
            DemoKind::HalfFlip => {
                // Au sol, avant le saut : saut ; entre : tangage arrière ; dodge arrière ; annulation + roulis.
                if !s.started {
                    s.started = true;
                    s.air = 1;
                    return Controls {
                        jump: true,
                        ..Default::default()
                    };
                }
                s.air += 1;
                if !aerial && a > 2 {
                    return drive(true);
                }
                let w = s.wait + 2;
                if a < w {
                    Controls {
                        pitch: 1.0,
                        ..Default::default()
                    }
                } else if a == w {
                    Controls {
                        jump: true,
                        pitch: 1.0,
                        ..Default::default()
                    }
                } else if a <= w + s.cancel_n {
                    Controls {
                        pitch: -1.0,
                        roll: s.roll,
                        yaw: s.yaw,
                        steer: s.yaw,
                        boost: cos > 0.0,
                        throttle: 1.0,
                        ..Default::default()
                    }
                } else {
                    level(cos > 0.3)
                }
            }
            DemoKind::SpeedFlip => {
                if step < s.pre as usize && !s.started {
                    return drive(true);
                }
                if !s.started {
                    s.started = true;
                    s.air = 1;
                    return Controls {
                        jump: true,
                        boost: true,
                        throttle: 1.0,
                        pitch: s.pre_pitch,
                        ..Default::default()
                    };
                }
                s.air += 1;
                if !aerial && a > 2 {
                    return drive(true);
                }
                let w = s.wait + 2;
                if a < w {
                    Controls {
                        boost: true,
                        throttle: 1.0,
                        pitch: s.pre_pitch,
                        ..Default::default()
                    }
                } else if a == w {
                    Controls {
                        jump: true,
                        pitch: -1.0,
                        roll: s.roll,
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                } else if a <= w + s.cancel_n {
                    Controls {
                        pitch: 1.0,
                        roll: s.roll,
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                } else {
                    let [p, y, r] = s.tail[((a - w - s.cancel_n - 1) as usize).min(15)];
                    Controls {
                        pitch: p,
                        yaw: y,
                        steer: y,
                        roll: r,
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                }
            }
            DemoKind::FlipShot => {
                let rel = [ball.pos[0] - car.pos[0], ball.pos[1] - car.pos[1]];
                let dist = (rel[0] * rel[0] + rel[1] * rel[1]).sqrt();
                let closing = ((car.vel[0] - ball.vel[0]) * d[0] + (car.vel[1] - ball.vel[1]) * d[1])
                    .max(200.0);
                if !s.started {
                    if dist / closing <= s.ttc && cos > 0.9 {
                        s.started = true;
                        s.air = 1;
                        return Controls {
                            jump: true,
                            boost: true,
                            throttle: 1.0,
                            ..Default::default()
                        };
                    }
                    return drive(true);
                }
                s.air += 1;
                if !aerial && a > 2 {
                    return drive(true);
                }
                let w = s.wait + 2;
                if a < w {
                    Controls {
                        boost: true,
                        throttle: 1.0,
                        pitch: s.pre_pitch,
                        ..Default::default()
                    }
                } else if a == w {
                    Controls {
                        jump: true,
                        pitch: -1.0,
                        roll: s.roll,
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                } else {
                    Controls {
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                }
            }
            DemoKind::Flick => {
                if step < s.pre as usize && !s.started {
                    // Dribble : la balle reste au centre du toit (avant/arrière par l'accélérateur, côtés par le braquage).
                    let rel = [
                        ball.pos[0] - car.pos[0],
                        ball.pos[1] - car.pos[1],
                        ball.pos[2] - car.pos[2],
                    ];
                    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                    let fwd_off = dot(rel, car.forward);
                    let side_off = dot(rel, car.right);
                    let dv = [
                        ball.vel[0] - car.vel[0],
                        ball.vel[1] - car.vel[1],
                        ball.vel[2] - car.vel[2],
                    ];
                    let (fwd_v, side_v) = (dot(dv, car.forward), dot(dv, car.right));
                    let want = fwd_off + 0.25 * fwd_v;
                    let throttle = if want > 12.0 {
                        1.0
                    } else if want < -12.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    let st = (side_off / 35.0 + side_v / 250.0 + 0.4 * ang).clamp(-1.0, 1.0);
                    return Controls {
                        throttle,
                        steer: st,
                        yaw: st,
                        ..Default::default()
                    };
                }
                if !s.started {
                    s.started = true;
                    s.air = 1;
                    return Controls {
                        jump: true,
                        throttle: 1.0,
                        ..Default::default()
                    };
                }
                s.air += 1;
                if !aerial && a > 2 {
                    return drive(true);
                }
                let w = s.wait + 2;
                if a < w {
                    Controls {
                        pitch: s.pre_pitch,
                        ..Default::default()
                    }
                } else if a == w {
                    Controls {
                        jump: true,
                        pitch: -1.0,
                        roll: s.roll,
                        ..Default::default()
                    }
                } else {
                    Controls {
                        boost: true,
                        throttle: 1.0,
                        ..Default::default()
                    }
                }
            }
        }
    }
}

/// Compte rendu de la génération d'un lot de démonstrations.
#[derive(Clone, Debug, Default)]
pub struct PoolReport {
    /// Essais et séquences gardées (vérifiées par le détecteur), par mécanique (ordre de [`DemoKind::ALL`]).
    pub tried: [u64; 4],
    pub kept: [u64; 4],
    /// Décisions (échantillons de clonage) gardées par mécanique.
    pub samples: [u64; 4],
}

/// Génère jusqu'à `per_kind` séquences vérifiées par mécanique (au plus `per_kind × max_ratio` essais chacune), une
/// mécanique par thread. `init_meshes` doit avoir réussi avant.
pub fn build_pool(
    per_kind: usize,
    max_ratio: usize,
    seed: u64,
) -> Result<(Vec<Demo>, PoolReport), SimError> {
    let results: Vec<Result<(Vec<Demo>, u64, u64), SimError>> = std::thread::scope(|sc| {
        let hs: Vec<_> = DemoKind::ALL
            .iter()
            .enumerate()
            .map(|(i, &kind)| {
                sc.spawn(move || {
                    let mut g = DemoGen::new(seed.wrapping_add(i as u64 * 7919))?;
                    let mut out = Vec::new();
                    for _ in 0..per_kind * max_ratio.max(1) {
                        if out.len() >= per_kind {
                            break;
                        }
                        out.extend(g.sample(kind));
                    }
                    Ok((out, g.tried[i], g.kept[i]))
                })
            })
            .collect();
        hs.into_iter()
            .map(|h| h.join().expect("thread de démonstrations"))
            .collect()
    });
    let (mut all, mut rep) = (Vec::new(), PoolReport::default());
    for (i, r) in results.into_iter().enumerate() {
        let (d, tried, kept) = r?;
        rep.tried[i] = tried;
        rep.kept[i] = kept;
        rep.samples[i] = d.iter().map(|x| x.len() as u64).sum();
        all.extend(d);
    }
    Ok((all, rep))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{init_meshes, meshes_dir_from_env};

    fn ready() -> bool {
        meshes_dir_from_env()
            .ok()
            .is_some_and(|d| init_meshes(&d).is_ok())
    }

    #[test]
    fn action_proche_retrouve_les_actions_de_la_table() {
        let t = ActionTable::rlgym();
        for i in 0..t.len() {
            let c = t.get(i);
            // Une action de la table est son propre plus proche voisin (les doublons de contrôles sont permis).
            let j = nearest_action(&t, &c);
            assert_eq!(t.get(j), c, "{i}");
        }
    }

    /// Chaque mécanique sort au moins une séquence vérifiée sur 80 essais ; la séquence finit par la détection.
    #[test]
    fn chaque_mecanique_a_des_sequences_verifiees() {
        if !ready() {
            return;
        }
        let mut g = DemoGen::new(7).unwrap();
        for kind in DemoKind::ALL {
            let mut ok = 0;
            for _ in 0..80 {
                if let Some(d) = g.sample(kind) {
                    assert_eq!(d.obs.len(), d.len() * OBS_DIM);
                    assert!(d.actions.iter().all(|&a| (a as usize) < 90));
                    ok += 1;
                }
            }
            assert!(ok >= 1, "{}: 0 séquence sur 80", kind.label());
        }
    }
}
