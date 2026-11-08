//! Caméras : ball-cam (derrière la voiture, regard vers la balle) et caméra voiture (derrière, dans l'axe).

use game::arena_geo::{HALF_X, HALF_Y, HEIGHT};
use game::world::World;
use macroquad::prelude::*;

use crate::render::g;

pub struct Cam {
    /// Direction horizontale lissée (unitaire, plan xy, repère jeu).
    dir: Vec2,
    pos: [f32; 3],
    target: [f32; 3],
}

impl Cam {
    pub fn new() -> Self {
        Self {
            dir: Vec2::X,
            pos: [0.0, -600.0, 200.0],
            target: [0.0; 3],
        }
    }

    /// Met à jour la caméra pour la frame (dt réel en secondes) et retourne la Camera3D.
    pub fn update(&mut self, w: &World, dt: f32) -> Camera3D {
        let car = w.car();
        let a = w.alpha();
        let cp = [
            w.prev.cars[0].pos[0] + (car.pos[0] - w.prev.cars[0].pos[0]) * a,
            w.prev.cars[0].pos[1] + (car.pos[1] - w.prev.cars[0].pos[1]) * a,
            w.prev.cars[0].pos[2] + (car.pos[2] - w.prev.cars[0].pos[2]) * a,
        ];
        let ball = w.cur.ball.pos;
        let to_ball = vec2(ball[0] - cp[0], ball[1] - cp[1]);
        let fwd = vec2(car.forward[0], car.forward[1]);
        let want = if w.ball_cam && to_ball.length() > 250.0 {
            to_ball.normalize()
        } else if fwd.length() > 0.2 {
            fwd.normalize()
        } else {
            self.dir
        };
        // Lissage de la direction (rotation par interpolation angulaire).
        let k = 1.0 - (-dt * 7.0).exp();
        let (a0, a1) = (self.dir.y.atan2(self.dir.x), want.y.atan2(want.x));
        let mut da = a1 - a0;
        while da > std::f32::consts::PI {
            da -= std::f32::consts::TAU;
        }
        while da < -std::f32::consts::PI {
            da += std::f32::consts::TAU;
        }
        let ang = a0 + da * k;
        self.dir = vec2(ang.cos(), ang.sin());

        let (dist, height) = (380.0, 150.0);
        let mut pos = [
            cp[0] - self.dir.x * dist,
            cp[1] - self.dir.y * dist,
            cp[2] + height,
        ];
        pos[0] = pos[0].clamp(-HALF_X + 150.0, HALF_X - 150.0);
        pos[1] = pos[1].clamp(-HALF_Y - 600.0, HALF_Y + 600.0);
        pos[2] = pos[2].clamp(40.0, HEIGHT - 60.0);
        let target = if w.ball_cam {
            // Regard entre la voiture et la balle, un peu relevé.
            [
                cp[0] + (ball[0] - cp[0]) * 0.8,
                cp[1] + (ball[1] - cp[1]) * 0.8,
                (cp[2] + (ball[2] - cp[2]) * 0.8).max(90.0),
            ]
        } else {
            [
                cp[0] + self.dir.x * 400.0,
                cp[1] + self.dir.y * 400.0,
                cp[2] + 60.0,
            ]
        };
        let s = 1.0 - (-dt * 18.0).exp();
        for i in 0..3 {
            self.pos[i] += (pos[i] - self.pos[i]) * s;
            self.target[i] += (target[i] - self.target[i]) * s;
        }
        Camera3D {
            position: g(self.pos),
            target: g(self.target),
            up: vec3(0.0, 0.0, 1.0),
            fovy: 1.15,
            z_near: 0.2,
            z_far: 400.0,
            ..Default::default()
        }
    }
}
