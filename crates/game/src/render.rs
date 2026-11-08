//! Rendu 3D macroquad en primitives originales. Le monde est en unreal units, repère z haut ;
//! on convertit en mètres et on inverse y pour garder la même latéralité qu'à l'écran de jeu
//! (la droite de la voiture reste à droite de l'image).

use game::arena_geo::{ArenaGeo, BALL_RADIUS, LineKind, Surf};
use game::world::World;
use macroquad::prelude::*;

const SCALE: f32 = 0.01;

pub fn g(p: [f32; 3]) -> Vec3 {
    vec3(p[0], -p[1], p[2]) * SCALE
}

fn gv(p: Vec3) -> Vec3 {
    vec3(p.x, -p.y, p.z) * SCALE
}

fn v3(p: [f32; 3]) -> Vec3 {
    Vec3::from(p)
}

fn shade(c: Color, k: f32) -> Color {
    Color::new(c.r * k, c.g * k, c.b * k, c.a)
}

fn col(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba(r, g, b, a)
}

pub struct ArenaRender {
    opaque: Vec<Mesh>,
    ceiling: Vec<Mesh>,
    lines: Vec<(Vec3, Vec3, Color)>,
}

impl ArenaRender {
    pub fn new(geo: &ArenaGeo) -> Self {
        let light = vec3(0.35, -0.25, 0.9).normalize();
        let (mut opaque, mut ceiling) = (Vec::<Vertex>::new(), Vec::<Vertex>::new());
        for t in &geo.tris {
            let (a, b, c) = (g(t.v[0]), g(t.v[1]), g(t.v[2]));
            let n = (b - a).cross(c - a).normalize_or_zero();
            let k = 0.6 + 0.4 * n.dot(light).abs();
            let base = match t.surf {
                Surf::Floor => col(30, 74, 62, 255),
                Surf::Wall => col(52, 62, 104, 255),
                Surf::Ceiling => col(80, 90, 140, 38),
                Surf::GoalBlue => col(40, 100, 215, 255),
                Surf::GoalOrange => col(225, 125, 35, 255),
            };
            let dst = if t.surf == Surf::Ceiling {
                &mut ceiling
            } else {
                &mut opaque
            };
            for p in [a, b, c] {
                dst.push(Vertex::new2(p, Vec2::ZERO, shade(base, k)));
            }
        }
        let chunk = |verts: Vec<Vertex>| -> Vec<Mesh> {
            verts
                .chunks(3 * 4000)
                .map(|c| Mesh {
                    indices: (0..c.len() as u16).collect(),
                    vertices: c.to_vec(),
                    texture: None,
                })
                .collect()
        };
        let lines = geo
            .lines
            .iter()
            .map(|l| {
                let c = match l.kind {
                    LineKind::Marking => col(210, 215, 235, 150),
                    LineKind::Grid => col(255, 255, 255, 22),
                    LineKind::Edge => col(150, 170, 235, 190),
                    LineKind::GoalBlue => col(120, 170, 255, 255),
                    LineKind::GoalOrange => col(255, 190, 110, 255),
                };
                (g(l.a), g(l.b), c)
            })
            .collect();
        Self {
            opaque: chunk(opaque),
            ceiling: chunk(ceiling),
            lines,
        }
    }

    pub fn draw_opaque(&self) {
        for m in &self.opaque {
            draw_mesh(m);
        }
        for (a, b, c) in &self.lines {
            draw_line_3d(*a, *b, *c);
        }
    }

    pub fn draw_ceiling(&self) {
        for m in &self.ceiling {
            draw_mesh(m);
        }
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Boîte orientée : `axes` = vecteurs unitaires (monde jeu), `half` = demi-dimensions en uu.
fn draw_obb(center: Vec3, axes: [Vec3; 3], half: [f32; 3], color: Color, edge: Option<Color>) {
    let corner = |sx: f32, sy: f32, sz: f32| {
        gv(center + axes[0] * (half[0] * sx) + axes[1] * (half[1] * sy) + axes[2] * (half[2] * sz))
    };
    let c: Vec<Vec3> = (0..8)
        .map(|i| {
            corner(
                if i & 1 == 0 { -1. } else { 1. },
                if i & 2 == 0 { -1. } else { 1. },
                if i & 4 == 0 { -1. } else { 1. },
            )
        })
        .collect();
    // (indices des 4 coins, axe normal) par face
    let faces: [([usize; 4], usize, f32); 6] = [
        ([1, 3, 7, 5], 0, 1.0),
        ([0, 2, 6, 4], 0, -1.0),
        ([2, 3, 7, 6], 1, 1.0),
        ([0, 1, 5, 4], 1, -1.0),
        ([4, 5, 7, 6], 2, 1.0),
        ([0, 1, 3, 2], 2, -1.0),
    ];
    let light = vec3(0.35, -0.25, 0.9).normalize();
    let mut verts = Vec::with_capacity(24);
    let mut idx = Vec::with_capacity(36);
    for (q, ax, sign) in faces {
        let n = axes[ax] * sign;
        let k = 0.55 + 0.45 * (n.dot(light) * 0.5 + 0.5);
        let base = verts.len() as u16;
        for i in q {
            verts.push(Vertex::new2(c[i], Vec2::ZERO, shade(color, k)));
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    draw_mesh(&Mesh {
        vertices: verts,
        indices: idx,
        texture: None,
    });
    if let Some(e) = edge {
        for (a, b) in [
            (0, 1),
            (2, 3),
            (4, 5),
            (6, 7),
            (0, 2),
            (1, 3),
            (4, 6),
            (5, 7),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            draw_line_3d(c[a], c[b], e);
        }
    }
}

fn draw_disc(center: [f32; 3], radius: f32, color: Color) {
    const N: u16 = 20;
    let mut verts = vec![Vertex::new2(g(center), Vec2::ZERO, color)];
    for k in 0..N {
        let a = k as f32 / N as f32 * std::f32::consts::TAU;
        verts.push(Vertex::new2(
            g([
                center[0] + radius * a.cos(),
                center[1] + radius * a.sin(),
                center[2],
            ]),
            Vec2::ZERO,
            color,
        ));
    }
    let mut idx = Vec::new();
    for k in 0..N {
        idx.extend_from_slice(&[0, 1 + k, 1 + (k + 1) % N]);
    }
    draw_mesh(&Mesh {
        vertices: verts,
        indices: idx,
        texture: None,
    });
}

pub fn draw_world(w: &World, arena: &ArenaRender) {
    arena.draw_opaque();
    let a = w.alpha();

    // Boosts.
    for p in &w.pads {
        let (size, h) = if p.big { (150.0, 28.0) } else { (70.0, 10.0) };
        let c = if p.active {
            col(255, 170, 40, 255)
        } else {
            col(60, 60, 70, 255)
        };
        draw_cube(
            g([p.pos[0], p.pos[1], h * 0.5]),
            vec3(size, size, h) * SCALE,
            None,
            c,
        );
    }

    // Balle : ombre, sphère, trois grands cercles qui tournent.
    let bp = lerp3(w.prev.ball.pos, w.cur.ball.pos, a);
    draw_disc([bp[0], bp[1], 2.0], BALL_RADIUS * 0.9, col(0, 0, 0, 90));
    draw_sphere(g(bp), BALL_RADIUS * SCALE, None, col(235, 235, 240, 255));
    let ctr = v3(bp);
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let (u, v) = match axis {
            x if x == Vec3::X => (Vec3::Y, Vec3::Z),
            y if y == Vec3::Y => (Vec3::Z, Vec3::X),
            _ => (Vec3::X, Vec3::Y),
        };
        let (u, v) = (w.ball_rot * u, w.ball_rot * v);
        let n = 24;
        let pt = |k: usize| {
            let t = k as f32 / n as f32 * std::f32::consts::TAU;
            gv(ctr + (u * t.cos() + v * t.sin()) * (BALL_RADIUS * 1.01))
        };
        for k in 0..n {
            draw_line_3d(pt(k), pt(k + 1), col(40, 40, 60, 255));
        }
    }

    // Voiture : boîte aux dimensions Octane (hitbox publique), cabine, nez, roues.
    let c = w.car();
    let pos = lerp3(w.prev.cars[0].pos, c.pos, a);
    let (f, r, u) = (v3(c.forward), v3(c.right), v3(c.up));
    let team = if c.blue {
        col(45, 120, 255, 255)
    } else {
        col(255, 130, 30, 255)
    };
    let hit_center = v3(pos) + f * 13.88 + u * 20.75;
    let axes = [f, r, u];
    draw_disc([pos[0], pos[1], 2.0], 55.0, col(0, 0, 0, 90));
    draw_obb(
        hit_center - u * 6.0,
        axes,
        [60.0, 42.0, 11.0],
        team,
        Some(col(10, 20, 40, 255)),
    );
    draw_obb(
        hit_center + u * 9.0 - f * 8.0,
        axes,
        [30.0, 36.0, 8.0],
        shade(team, 0.6),
        None,
    );
    draw_obb(
        hit_center + f * 56.0 + u * 2.0,
        axes,
        [4.0, 40.0, 5.0],
        col(250, 250, 250, 255),
        None,
    );
    for (fx, sy) in [(38.0, 1.0), (38.0, -1.0), (-36.0, 1.0), (-36.0, -1.0)] {
        let wc = hit_center + f * fx + r * (sy * 40.0) - u * 16.0;
        draw_obb(wc, axes, [11.0, 6.0, 11.0], col(25, 25, 30, 255), None);
    }
    if w.boosting {
        let flame = hit_center - f * 78.0;
        draw_obb(flame, axes, [22.0, 10.0, 8.0], col(255, 150, 40, 255), None);
        draw_obb(
            flame - f * 16.0,
            axes,
            [12.0, 6.0, 5.0],
            col(255, 235, 140, 255),
            None,
        );
    }

    arena.draw_ceiling();
}
