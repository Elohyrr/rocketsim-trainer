//! Caméra à la Rocket League, sans dépendance graphique (testable seule).
//!
//! Principes (comportement observé de RL, formules d'origine) :
//! - la caméra ne roule jamais : son « haut » reste l'axe Z du monde ; seuls cap (yaw) et inclinaison
//!   (pitch) sont pilotés, jamais le tangage ni le roulis de la voiture ;
//! - au sol : cap = avant de la voiture projeté sur la surface de contact (sol ou mur), suivi quasi
//!   instantané (raideur `stiffness`), beaucoup plus lent sur un mur ;
//! - en l'air : la caméra est « tenue en laisse » : elle regarde la voiture depuis sa position
//!   précédente, donc suit la trajectoire (vitesse) et ignore flips et tonneaux ;
//! - sol ↔ air : fondu progressif (`transition_speed`) ; bascule voiture ↔ balle : décalage résorbé ;
//! - ball cam : cap vers la balle, inclinaison fixe tant que la balle est basse ;
//! - la position est toujours contenue dans le volume de l'arène (murs, sol, plafond, coins, cages).

use crate::arena_geo::{
    GOAL_DEPTH, GOAL_H, GOAL_HALF_W, HALF_X, HALF_Y, HEIGHT, RAMP_R, inside_footprint,
};
use crate::config::CameraCfg;

pub type V3 = [f32; 3];

/// Marge entre la caméra et les parois (uu).
pub const MARGIN: f32 = 60.0;

const PI: f32 = std::f32::consts::PI;
const MAX_PITCH: f32 = 85.0 * PI / 180.0;
/// Vitesse maximale de la voiture (uu/s), utilisée pour normaliser les effets de vitesse.
const VMAX: f32 = 2300.0;

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: V3) -> f32 {
    dot(a, a).sqrt()
}
fn norm(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-4).then(|| mul(a, 1.0 / l))
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn lerp3(a: V3, b: V3, t: f32) -> V3 {
    add(a, mul(sub(b, a), t))
}
/// Coefficient de lissage exponentiel indépendant du framerate : `1 - e^(-λ·dt)`.
fn smooth(lambda: f32, dt: f32) -> f32 {
    1.0 - (-lambda * dt).exp()
}
fn wrap(a: f32) -> f32 {
    let mut a = a % (2.0 * PI);
    if a > PI {
        a -= 2.0 * PI;
    } else if a < -PI {
        a += 2.0 * PI;
    }
    a
}

/// Orientation de la caméra : cap et inclinaison (pas de roulis), en radians.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rot {
    pub pitch: f32,
    pub yaw: f32,
}

impl Rot {
    fn of(d: V3) -> Self {
        Self {
            pitch: d[2].atan2(d[0].hypot(d[1])),
            yaw: d[1].atan2(d[0]),
        }
    }
    pub fn dir(self) -> V3 {
        let c = self.pitch.cos();
        [c * self.yaw.cos(), c * self.yaw.sin(), self.pitch.sin()]
    }
    /// Interpolation par le plus court chemin.
    fn lerp(self, to: Rot, t: f32) -> Rot {
        Rot {
            pitch: self.pitch + wrap(to.pitch - self.pitch) * t,
            yaw: wrap(self.yaw + wrap(to.yaw - self.yaw) * t),
        }
    }
    fn interp(self, to: Rot, lambda: f32, dt: f32) -> Rot {
        self.lerp(to, smooth(lambda, dt))
    }
}

/// État de la voiture utile à la caméra (repère jeu : x avant, y droite, z haut).
#[derive(Debug, Clone, Copy)]
pub struct CarIn {
    pub pos: V3,
    pub vel: V3,
    pub forward: V3,
    pub up: V3,
    pub on_ground: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameIn {
    pub car: CarIn,
    pub ball: V3,
    pub ball_cam: bool,
    pub supersonic: bool,
    pub dt: f32,
}

/// Sortie : position, point visé (le haut est toujours Z) et FOV horizontal en degrés.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamOut {
    pub pos: V3,
    pub target: V3,
    pub fov_h_deg: f32,
}

pub struct Rig {
    cfg: CameraCfg,
    /// Orientation de la caméra voiture au sol / en l'air (synchronisées hors de leur état).
    rot_g: Rot,
    rot_a: Rot,
    rot_b: Rot,
    /// 0 = air, 1 = sol.
    blend: f32,
    was_ground: bool,
    /// Normale de la surface de contact, lissée.
    normal: V3,
    /// Position retardée de la voiture (raideur).
    lag: V3,
    dist: f32,
    fov: f32,
    /// Position de la caméra voiture à la frame précédente (laisse en l'air).
    leash: V3,
    last: Option<CamOut>,
    was_ball: bool,
    delta: Option<(V3, V3)>,
    delta_t: f32,
}

impl Rig {
    pub fn new(cfg: CameraCfg) -> Self {
        let cfg = CameraCfg {
            stiffness: cfg.stiffness.clamp(0.0, 1.0),
            ..cfg
        };
        Self {
            rot_g: Rot::default(),
            rot_a: Rot::default(),
            rot_b: Rot::default(),
            blend: 1.0,
            was_ground: true,
            normal: [0.0, 0.0, 1.0],
            lag: [0.0; 3],
            dist: cfg.distance,
            fov: cfg.fov,
            leash: [0.0; 3],
            last: None,
            was_ball: false,
            delta: None,
            delta_t: 0.0,
            cfg,
        }
    }

    fn angle(&self) -> f32 {
        self.cfg.angle.to_radians()
    }

    /// Durée de la résorption du décalage voiture ↔ balle (s).
    fn transition_secs(&self) -> f32 {
        lerp(0.5, 0.0, (self.cfg.transition_speed - 1.0).clamp(0.0, 1.0))
    }

    pub fn update(&mut self, f: &FrameIn) -> CamOut {
        let dt = f.dt.clamp(1e-4, 0.1);
        let car = &f.car;
        let c = &self.cfg;
        let z = [0.0, 0.0, 1.0];
        let speed = len(car.vel);
        let swivel = (c.swivel_speed / 5.0).max(0.05);

        if self.last.is_none() {
            // Première frame : repart derrière la voiture, dans son axe.
            let h = Rot::of(norm([car.forward[0], car.forward[1], 0.0]).unwrap_or([1.0, 0.0, 0.0]));
            let vxy = car.vel[0].hypot(car.vel[1]);
            let yaw = if car.on_ground || vxy < 1.0 {
                h.yaw
            } else {
                let v = Rot::of([car.vel[0], car.vel[1], 0.0]);
                h.lerp(v, (vxy / 500.0).min(1.0)).yaw
            };
            self.rot_g = Rot {
                pitch: self.angle(),
                yaw,
            };
            self.rot_a = self.rot_g;
            self.rot_b = self.rot_g;
            self.lag = car.pos;
            self.leash = sub(car.pos, mul(self.rot_g.dir(), c.distance));
            self.normal = z;
            self.blend = f32::from(car.on_ground);
            self.was_ground = car.on_ground;
        }

        // Fondu sol ↔ air.
        let ts = c.transition_speed.max(0.1);
        self.blend = if car.on_ground {
            self.blend + 2.0 * ts * dt
        } else {
            self.blend - 4.0 * ts * dt
        }
        .clamp(0.0, 1.0);

        // Normale de contact (sol, mur) ; retombe sur Z en l'air ou si la voiture est sur le toit.
        let contact = norm(car.up).filter(|u| u[2] > -0.3).unwrap_or(z);
        let n_target = if car.on_ground { contact } else { z };
        if car.on_ground && !self.was_ground {
            self.normal = n_target; // atterrissage : reprend directement la normale
        } else {
            let s = smooth(10.46, dt);
            self.normal = norm(lerp3(self.normal, n_target, s)).unwrap_or(z);
        }
        let u = self.normal;

        // Focus : voiture + hauteur le long de la normale (sol) ou de Z (air), avec retard (raideur).
        self.lag = lerp3(self.lag, car.pos, smooth(6.32, dt));
        let over = sub(self.lag, car.pos);
        if len(over) > 100.0 {
            self.lag = add(car.pos, mul(norm(over).unwrap_or([0.0; 3]), 100.0));
        }

        // --- Caméra voiture au sol / sur un mur ---
        if car.on_ground && !self.was_ground {
            self.rot_g = self.rot_a; // atterrissage : repart de la vue en l'air
        } else if !car.on_ground && self.was_ground {
            self.rot_a = self.rot_g; // décollage : repart de la vue au sol
        }
        // Au sol seulement : en l'air, le cap « sol » reste figé (flips et tonneaux ignorés).
        if car.on_ground {
            let g_dir_prev = self.rot_g.dir();
            let fwd_p = sub(car.forward, mul(u, dot(car.forward, u)));
            let fwd_p = norm(fwd_p).unwrap_or(g_dir_prev);
            let p = fwd_p[2].clamp(-1.0, 1.0).asin();
            let t = (p.abs() / (264.0f32).to_radians()).min(1.0);
            let tilt = if p > 0.0 { mul(u, -1.0) } else { u };
            let d = norm(lerp3(fwd_p, tilt, t)).unwrap_or(fwd_p);
            let mut tgt_g = Rot::of(d);
            tgt_g.pitch += self.angle().min(PI / 2.0 - tgt_g.pitch);
            let k = lerp(1.0, 3.427, c.stiffness);
            let (r_ground, r_wall) = if c.stiffness >= 1.0 {
                (tgt_g, tgt_g)
            } else {
                (
                    self.rot_g.interp(tgt_g, 13.39 * k, dt),
                    self.rot_g.interp(tgt_g, 2.03 * k, dt),
                )
            };
            self.rot_g = r_ground.lerp(r_wall, 1.0 - u[2].abs());
            self.rot_g.pitch = self.rot_g.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        }

        // --- Caméra voiture en l'air : laisse, indépendante de l'orientation de la voiture ---
        let focus_a = add(car.pos, mul(z, c.height));
        let tgt_a = Rot::of(norm(sub(focus_a, self.leash)).unwrap_or_else(|| self.rot_a.dir()));
        let r = lerp(35.0, 10.0, (speed / VMAX).min(1.0)) / 60.0;
        let ka = 1.0 - (1.0 - r.min(1.0)).powf(60.0 * dt * swivel);
        let mut rot_a = self.rot_a.lerp(tgt_a, ka);
        if dot(car.vel, rot_a.dir()) < 0.0 {
            let vxy = car.vel[0].hypot(car.vel[1]);
            rot_a.pitch *= (1.0 - vxy / VMAX).clamp(0.0, 1.0);
        }
        rot_a.pitch = rot_a.pitch.clamp(-MAX_PITCH, MAX_PITCH);
        self.rot_a = rot_a;
        self.was_ground = car.on_ground;

        let rot = self.rot_g.lerp(self.rot_a, 1.0 - self.blend);
        let dir = rot.dir();

        // Distance : s'allonge un peu avec la vitesse vers l'avant, d'autant moins que la raideur est forte.
        let want = c.distance + (dot(car.vel, dir) * 0.05 * (1.0 - c.stiffness)).max(-50.0);
        self.dist = lerp(self.dist, want, smooth(4.14, dt));

        let focus_g = add(car.pos, mul(u, c.height));
        let focus_true = lerp3(focus_a, focus_g, self.blend);
        // Retard de la voiture : seules les composantes latérale et verticale (par rapport à la vue).
        let lag_off = sub(self.lag, car.pos);
        let lag_off = sub(lag_off, mul(dir, dot(lag_off, dir)));
        let focus = add(focus_true, mul(lag_off, 1.0 - c.stiffness));
        let car_pos = sub(focus, mul(dir, self.dist));
        self.leash = car_pos;

        // --- Ball cam : cap vers la balle, horizon stable ---
        let (pos0, dir_used) = if f.ball_cam {
            let mut fb = add(car.pos, mul(z, c.height));
            let mut rb = Rot::of(norm(sub(f.ball, fb)).unwrap_or(dir));
            fb[2] += rb.pitch.abs().to_degrees() * 0.91;
            rb = Rot::of(norm(sub(f.ball, fb)).unwrap_or(dir));
            // Au-dessus d'environ 22°, l'inclinaison suit la balle ; en dessous, angle fixe.
            let s =
                ((rb.pitch.abs() - 21.97f32.to_radians()) / 21.97f32.to_radians()).clamp(0.0, 1.0);
            rb.pitch = lerp(self.angle(), rb.pitch, 0.8 * s);
            if sub(f.ball, car.pos)
                .iter()
                .take(2)
                .map(|v| v * v)
                .sum::<f32>()
                < 150.0 * 150.0
            {
                rb.yaw = self.rot_b.yaw; // balle collée à la voiture : garde le cap
            }
            if !self.was_ball {
                self.rot_b = rot; // reprend la vue voiture, le fondu fait le reste
            }
            self.rot_b = self.rot_b.interp(rb, 8.59 * swivel, dt);
            self.rot_b.pitch = self.rot_b.pitch.clamp(-MAX_PITCH, MAX_PITCH);
            let d = self.rot_b.dir();
            (sub(fb, mul(d, self.dist)), d)
        } else {
            (car_pos, dir)
        };
        let tgt0 = add(pos0, mul(dir_used, self.dist));

        // --- Bascule voiture ↔ balle : décalage résorbé en quelques dixièmes de seconde ---
        let (mut pos1, mut tgt1) = (pos0, tgt0);
        if let Some(last) = self.last {
            if f.ball_cam != self.was_ball && self.transition_secs() > 0.0 {
                self.delta = Some((sub(last.pos, pos0), sub(last.target, tgt0)));
                self.delta_t = self.transition_secs();
            }
        }
        if let Some((dp, dtg)) = self.delta {
            let q = (self.delta_t / self.transition_secs().max(1e-3)).clamp(0.0, 1.0);
            pos1 = add(pos0, mul(dp, q * q));
            tgt1 = add(tgt0, mul(dtg, q * q));
            self.delta_t -= dt;
            if self.delta_t <= 0.0 {
                self.delta = None;
            }
        }
        self.was_ball = f.ball_cam;

        // --- Contenue dans l'arène ---
        let pos2 = clamp_to_arena(pos1);
        let tgt2 = add(tgt1, sub(pos2, pos1));

        // FOV : s'élargit un peu avec la vitesse, davantage en supersonique.
        let fov_t = if f.supersonic {
            c.fov + 10.0
        } else {
            c.fov + 5.0 * (speed / VMAX).min(1.0)
        };
        let rate = if f.supersonic { 40.0 } else { 25.0 } * dt;
        self.fov += (fov_t - self.fov).clamp(-rate, rate);

        let out = CamOut {
            pos: pos2,
            target: tgt2,
            fov_h_deg: self.fov,
        };
        self.last = Some(out);
        out
    }
}

/// Hauteur minimale du sol près d'un mur : le raccord sol-mur est un arc de rayon `RAMP_R`.
fn ramp(dist_to_wall: f32) -> f32 {
    if dist_to_wall >= RAMP_R {
        0.0
    } else {
        let d = dist_to_wall.max(0.0);
        RAMP_R
            - (RAMP_R * RAMP_R - (RAMP_R - d) * (RAMP_R - d))
                .max(0.0)
                .sqrt()
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Ramène un point dans le volume de l'arène (marge [`MARGIN`]), de façon continue : cages comprises
/// (la limite en y s'étend progressivement devant l'ouverture), pans coupés et raccords arrondis.
pub fn clamp_to_arena(p: V3) -> V3 {
    let m = MARGIN;
    // Ouverture de cage : poids 1 au centre de la cage et sous la barre, 0 hors de l'ouverture.
    let w = smoothstep(GOAL_HALF_W - m, GOAL_HALF_W - m - 500.0, p[0].abs())
        * smoothstep(GOAL_H - m, GOAL_H - m - 150.0, p[2]);
    let lim_y = lerp(HALF_Y - m, HALF_Y + GOAL_DEPTH - m, w);
    let (lim_x, mut x, mut y) = (HALF_X - m, p[0], p[1]);
    x = x.clamp(-lim_x, lim_x);
    y = y.clamp(-lim_y, lim_y);
    // Pans coupés des quatre coins.
    let max_sum = HALF_X + HALF_Y - crate::arena_geo::CHAMFER - m * std::f32::consts::SQRT_2;
    let over = x.abs() + y.abs() - max_sum;
    if over > 0.0 && y.abs() <= HALF_Y - m + 1.0 {
        x -= x.signum() * over * 0.5;
        y -= y.signum() * over * 0.5;
    }
    // Sol et plafond, relevés près des murs par les raccords arrondis.
    let wall = (HALF_X - x.abs()).min(HALF_Y - y.abs() + 1000.0 * w);
    let chamfer = (HALF_X + HALF_Y - crate::arena_geo::CHAMFER - x.abs() - y.abs())
        / std::f32::consts::SQRT_2;
    let d = wall.min(chamfer.max(0.0) + 0.0).max(0.0);
    let r = ramp(d);
    let z = p[2].clamp(40.0 + r, HEIGHT - 40.0 - r.max(0.0));
    [x, y, z.max(40.0)]
}

/// Vrai si le point est dans le volume de l'arène (sol, plafond, murs, pans coupés, cages), réduit de `inset`.
pub fn inside_arena(p: V3, inset: f32) -> bool {
    if p[2] < inset || p[2] > HEIGHT - inset {
        return false;
    }
    if inside_footprint(p[0], p[1], inset) {
        return true;
    }
    p[0].abs() <= GOAL_HALF_W - inset
        && p[2] <= GOAL_H - inset
        && p[1].abs() <= HALF_Y + GOAL_DEPTH - inset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rig {
        Rig::new(CameraCfg::default())
    }

    fn car(pos: V3, vel: V3, fwd: V3, up: V3, on_ground: bool) -> CarIn {
        CarIn {
            pos,
            vel,
            forward: fwd,
            up,
            on_ground,
        }
    }

    fn frame(car: CarIn, ball: V3, ball_cam: bool) -> FrameIn {
        FrameIn {
            car,
            ball,
            ball_cam,
            supersonic: false,
            dt: 1.0 / 60.0,
        }
    }

    fn yaw_of(o: &CamOut) -> f32 {
        let d = sub(o.target, o.pos);
        d[1].atan2(d[0])
    }

    /// Rotation d'un vecteur autour d'un axe unitaire (Rodrigues).
    fn rot_axis(v: V3, a: V3, ang: f32) -> V3 {
        let (s, c) = ang.sin_cos();
        let cross = [
            a[1] * v[2] - a[2] * v[1],
            a[2] * v[0] - a[0] * v[2],
            a[0] * v[1] - a[1] * v[0],
        ];
        add(add(mul(v, c), mul(cross, s)), mul(a, dot(a, v) * (1.0 - c)))
    }

    const BALL: V3 = [0.0, 2000.0, 100.0];

    #[test]
    fn follows_yaw_on_the_ground() {
        let mut r = rig();
        let p = [0.0, -2000.0, 17.0];
        let o = r.update(&frame(
            car(p, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], true),
            BALL,
            false,
        ));
        assert!(yaw_of(&o).abs() < 0.05);
        // la voiture tourne de 90° vers +y : la caméra suit en une fraction de seconde
        let f = [0.0, 1.0, 0.0];
        let mut o = o;
        for _ in 0..12 {
            o = r.update(&frame(
                car(p, [0.0; 3], f, [0.0, 0.0, 1.0], true),
                BALL,
                false,
            ));
        }
        assert!((yaw_of(&o) - PI / 2.0).abs() < 0.1, "yaw {}", yaw_of(&o));
        // derrière la voiture, au-dessus, à la distance voulue
        assert!(o.pos[1] < p[1] - 150.0 && o.pos[2] > p[2] + 50.0);
    }

    #[test]
    fn camera_does_not_roll_with_barrel_rolls_or_flips() {
        let vel = [800.0, 0.0, 200.0];
        let p0 = [0.0, -2000.0, 900.0];
        let ang = |t: f32| {
            // trajectoire balistique simple, orientation qui tourne dans tous les sens
            let p = [
                p0[0] + vel[0] * t,
                p0[1],
                p0[2] + vel[2] * t - 325.0 * t * t,
            ];
            (p, [vel[0], 0.0, vel[2] - 650.0 * t])
        };
        for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0f32]] {
            let mut r = rig();
            let mut outs = vec![];
            for i in 0..240 {
                let t = i as f32 / 60.0;
                let (p, v) = ang(t);
                let spin = t * 6.0; // plusieurs tours sur l'axe
                let fwd = rot_axis([1.0, 0.0, 0.0], axis, spin);
                let up = rot_axis([0.0, 0.0, 1.0], axis, spin);
                outs.push(r.update(&frame(car(p, v, fwd, up, false), BALL, false)));
            }
            // même trajectoire, voiture immobile en orientation : la caméra est identique
            let mut r2 = rig();
            for (i, o) in outs.iter().enumerate() {
                let t = i as f32 / 60.0;
                let (p, v) = ang(t);
                let o2 = r2.update(&frame(
                    car(p, v, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], false),
                    BALL,
                    false,
                ));
                assert!(len(sub(o.pos, o2.pos)) < 1e-2, "axe {axis:?} t={t}");
                assert!(len(sub(o.target, o2.target)) < 1e-2);
            }
        }
    }

    #[test]
    fn airborne_camera_is_calm_during_flip_and_aerial() {
        // aérienne : vitesse constante, la voiture bascule nez en haut puis fait un tonneau
        let mut r = rig();
        let mut prev: Option<CamOut> = None;
        let mut worst = 0.0f32;
        for i in 0..180 {
            let t = i as f32 / 60.0;
            let p = [-500.0 + 700.0 * t, -2500.0 + 900.0 * t, 600.0 + 300.0 * t];
            let v = [700.0, 900.0, 300.0];
            let f = rot_axis([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], -0.3 - 4.0 * t);
            let u = rot_axis([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], -0.3 - 4.0 * t);
            let o = r.update(&frame(car(p, v, f, u, false), BALL, false));
            if let Some(q) = prev {
                if i > 20 {
                    let dy = wrap(yaw_of(&o) - yaw_of(&q)).abs();
                    worst = worst.max(dy);
                }
            }
            // l'horizon reste fixe : le haut de la caméra est Z, le regard reste modérément incliné
            let d = sub(o.target, o.pos);
            assert!((d[2] / len(d)).abs() < 0.7);
            prev = Some(o);
        }
        assert!(worst < 0.01, "variation de cap par frame {worst}");
    }

    #[test]
    fn no_jump_at_landing() {
        let mut r = rig();
        let mut prev: Option<CamOut> = None;
        let mut worst = 0.0f32;
        for i in 0..240 {
            let t = i as f32 / 60.0;
            let landed = t >= 1.2;
            let z = if landed {
                17.0
            } else {
                (500.0 - 400.0 * t).max(17.0)
            };
            let p = [1000.0 * t, -3000.0, z];
            let v = [1000.0, 0.0, if landed { 0.0 } else { -400.0 }];
            // la voiture atterrit un peu de travers (tangage et roulis)
            let (f, u) = if landed {
                ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
            } else {
                (
                    rot_axis([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], -0.5),
                    rot_axis([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.4),
                )
            };
            let o = r.update(&frame(car(p, v, f, u, landed), BALL, false));
            if let Some(q) = prev {
                // le déplacement attendu (vitesse de la voiture) est retiré
                let step = sub(sub(o.pos, q.pos), [1000.0 / 60.0, 0.0, 0.0]);
                worst = worst.max(len(step));
            }
            prev = Some(o);
        }
        assert!(worst < 25.0, "saut de {worst} uu par frame");
    }

    #[test]
    fn ball_cam_aims_at_ball_with_stable_horizon() {
        let mut r = rig();
        let p = [0.0, -2000.0, 17.0];
        let ball = [1500.0, 1000.0, 120.0];
        let mut o = r.update(&frame(
            car(p, [0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], true),
            ball,
            true,
        ));
        for _ in 0..120 {
            o = r.update(&frame(
                car(p, [0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], true),
                ball,
                true,
            ));
        }
        let want = (ball[1] - p[1]).atan2(ball[0] - p[0]);
        assert!(
            wrap(yaw_of(&o) - want).abs() < 0.05,
            "{} vs {want}",
            yaw_of(&o)
        );
        let d = sub(o.target, o.pos);
        assert!((d[2] / len(d)).abs() < 0.15, "horizon stable");
        // la caméra est du côté opposé à la balle par rapport à la voiture
        let to_cam = sub(o.pos, p);
        assert!(to_cam[0] * (ball[0] - p[0]) + to_cam[1] * (ball[1] - p[1]) < 0.0);
    }

    #[test]
    fn camera_stays_inside_arena_on_every_wall_and_corner() {
        let spots: [(V3, V3, V3); 9] = [
            // (position, avant, haut) : mur +x, -x, +y, -y, plafond, coins
            (
                [HALF_X - 17.0, 0.0, 900.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
            ),
            (
                [-HALF_X + 17.0, 500.0, 900.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
            ),
            (
                [1500.0, HALF_Y - 17.0, 900.0],
                [1.0, 0.0, 0.0],
                [0.0, -1.0, 0.0],
            ),
            (
                [-1500.0, -HALF_Y + 17.0, 900.0],
                [0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0],
            ),
            ([0.0, 0.0, HEIGHT - 17.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([3400.0, 4500.0, 600.0], [0.0, 0.0, 1.0], [-0.7, -0.7, 0.0]),
            ([-3400.0, 4500.0, 1500.0], [1.0, 0.0, 0.0], [0.7, -0.7, 0.0]),
            ([3300.0, -4600.0, 17.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            // dans la cage
            ([0.0, 5800.0, 17.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ];
        for (p, fwd, up) in spots {
            for ball_cam in [false, true] {
                let mut r = rig();
                for i in 0..90 {
                    let o = r.update(&frame(
                        car(p, [0.0; 3], fwd, up, i > 5),
                        [0.0, 0.0, 100.0],
                        ball_cam,
                    ));
                    assert!(
                        inside_arena(o.pos, 20.0),
                        "caméra hors de l'arène : {:?} pour la voiture {p:?} (ball_cam {ball_cam})",
                        o.pos
                    );
                }
            }
        }
    }

    #[test]
    fn arena_clamp_is_continuous() {
        // aucune marche le long d'une ligne traversant l'ouverture de cage et un coin
        for (a, b) in [
            ([-1500.0, 5300.0, 200.0], [1500.0, 5300.0, 200.0]),
            ([2900.0, 4900.0, 400.0], [4200.0, 5400.0, 400.0]),
            ([100.0, 4500.0, 0.0], [100.0, 4500.0, 2300.0]),
        ] {
            let mut prev = clamp_to_arena(a);
            for i in 1..=400 {
                let q = clamp_to_arena(lerp3(a, b, i as f32 / 400.0));
                assert!(len(sub(q, prev)) < 25.0, "marche en {q:?}");
                prev = q;
            }
        }
    }

    #[test]
    fn ball_cam_switch_is_smooth() {
        let mut r = rig();
        let p = [0.0, -2000.0, 17.0];
        let c = car(p, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], true);
        let mut prev = r.update(&frame(c, BALL, false));
        for _ in 0..60 {
            prev = r.update(&frame(c, BALL, false));
        }
        let o = r.update(&frame(c, BALL, true));
        assert!(len(sub(o.pos, prev.pos)) < 60.0, "bascule brutale");
    }

    #[test]
    fn supersonic_widens_fov() {
        let mut r = rig();
        let c = car(
            [0.0, -2000.0, 17.0],
            [0.0, 2300.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            true,
        );
        let base = r.update(&frame(c, BALL, false)).fov_h_deg;
        let mut f = frame(c, BALL, false);
        f.supersonic = true;
        let mut o = base;
        for _ in 0..60 {
            o = r.update(&f).fov_h_deg;
        }
        assert!(o > base + 4.0);
    }
}
