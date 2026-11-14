//! Caméra jouable : adapte le rig sans dépendance graphique (`game::rig`) à macroquad.

use game::config::CameraCfg;
use game::rig::{CarIn, FrameIn, Rig};
use game::world::World;
use macroquad::prelude::*;

use crate::render::g;

pub struct Cam {
    rig: Rig,
}

impl Cam {
    pub fn new(cfg: CameraCfg) -> Self {
        Self { rig: Rig::new(cfg) }
    }

    /// Met à jour la caméra pour la frame (dt réel en secondes) et retourne la Camera3D.
    pub fn update(&mut self, w: &World, dt: f32) -> Camera3D {
        if w.spectator {
            return spectator(w);
        }
        let car = w.car();
        let a = w.alpha();
        let p0 = w.prev.cars[0].pos;
        let pos = std::array::from_fn(|i| p0[i] + (car.pos[i] - p0[i]) * a);
        let out = self.rig.update(&FrameIn {
            car: CarIn {
                pos,
                vel: car.vel,
                forward: car.forward,
                up: car.up,
                on_ground: car.on_ground,
            },
            ball: w.cur.ball.pos,
            ball_cam: w.ball_cam,
            supersonic: w.supersonic,
            dt,
        });
        // Le FOV de RL est horizontal ; macroquad attend un FOV vertical.
        let aspect = (screen_width() / screen_height().max(1.0)).max(0.1);
        let fovy = 2.0 * ((out.fov_h_deg.to_radians() / 2.0).tan() / aspect).atan();
        Camera3D {
            position: g(out.pos),
            target: g(out.target),
            up: vec3(0.0, 0.0, 1.0),
            fovy,
            z_near: 0.2,
            z_far: 400.0,
            ..Default::default()
        }
    }
}

/// Vue d'ensemble fixe, au-dessus du fond de terrain bleu, qui pivote doucement vers la balle.
fn spectator(w: &World) -> Camera3D {
    let b = w.cur.ball.pos;
    let aspect = (screen_width() / screen_height().max(1.0)).max(0.1);
    let fovy = 2.0 * ((75.0f32.to_radians() / 2.0).tan() / aspect).atan();
    Camera3D {
        position: g([0.0, -4900.0, 1900.0]),
        target: g([b[0] * 0.5, b[1] * 0.6, 200.0]),
        up: vec3(0.0, 0.0, 1.0),
        fovy,
        z_near: 0.2,
        z_far: 400.0,
        ..Default::default()
    }
}
