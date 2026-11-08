//! Géométrie du terrain soccar en primitives originales (quads/triangles/lignes), en unreal units.
//! Dimensions publiques du terrain standard ; coins et raccords sol-mur-plafond arrondis par des arcs.
//! Aucune donnée du jeu : le rendu n'est qu'indicatif, la collision réelle vient de RocketSim.

use std::f32::consts::FRAC_PI_2;

pub type V3 = [f32; 3];

pub const HALF_X: f32 = 4096.0;
pub const HALF_Y: f32 = 5120.0;
pub const HEIGHT: f32 = 2044.0;
pub const GOAL_HALF_W: f32 = 892.755;
pub const GOAL_H: f32 = 642.775;
pub const GOAL_DEPTH: f32 = 880.0;
/// Longueur des pans coupés des quatre coins (le long de chaque mur), mesurée sur la collision.
pub const CHAMFER: f32 = 1160.0;
/// Rayon des raccords sol-mur et mur-plafond.
pub const RAMP_R: f32 = 260.0;
pub const BALL_RADIUS: f32 = 91.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surf {
    Floor,
    Wall,
    Ceiling,
    /// But côté y < 0 (bleu défend).
    GoalBlue,
    /// But côté y > 0.
    GoalOrange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Marquage au sol.
    Marking,
    /// Quadrillage du sol.
    Grid,
    /// Arêtes du terrain.
    Edge,
    GoalBlue,
    GoalOrange,
}

#[derive(Debug, Clone, Copy)]
pub struct Tri {
    pub v: [V3; 3],
    pub surf: Surf,
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub a: V3,
    pub b: V3,
    pub kind: LineKind,
}

#[derive(Debug, Default)]
pub struct ArenaGeo {
    pub tris: Vec<Tri>,
    pub lines: Vec<Line>,
}

impl ArenaGeo {
    fn quad(&mut self, a: V3, b: V3, c: V3, d: V3, surf: Surf) {
        self.tris.push(Tri { v: [a, b, c], surf });
        self.tris.push(Tri { v: [a, c, d], surf });
    }
}

/// Point du contour au sol avec sa normale intérieure (unitaire, dans le plan xy).
#[derive(Clone, Copy)]
struct FootPt {
    p: [f32; 2],
    n: [f32; 2],
}

fn footprint() -> Vec<FootPt> {
    const ARC: usize = 6;
    // Coin à pan coupé à 45° (mesuré sur la collision réelle : plan x + y = 8056), raccordé aux murs
    // par deux arcs de rayon RA. Coin nord-est construit, puis symétrisé.
    const RA: f32 = 500.0;
    let t = RA * (std::f32::consts::FRAC_PI_8).tan();
    let c1 = [HALF_X - RA, HALF_Y - CHAMFER - t];
    let c2 = [HALF_X - CHAMFER - t, HALF_Y - RA];
    let mut ne = Vec::new();
    for (c, a0, a1) in [(c1, 0.0, 0.25), (c2, 0.25, 0.5)] {
        for k in 0..=ARC {
            let th = (a0 + (a1 - a0) * k as f32 / ARC as f32) * std::f32::consts::PI;
            let (s, co) = th.sin_cos();
            ne.push(FootPt {
                p: [c[0] + RA * co, c[1] + RA * s],
                n: [-co, -s],
            });
        }
    }
    let mir = |pts: &[FootPt], sx: f32, sy: f32, rev: bool| {
        let mut v: Vec<FootPt> = pts
            .iter()
            .map(|f| FootPt {
                p: [f.p[0] * sx, f.p[1] * sy],
                n: [f.n[0] * sx, f.n[1] * sy],
            })
            .collect();
        if rev {
            v.reverse();
        }
        v
    };
    let goal = |y: f32, ny: f32, xs: [f32; 2]| {
        xs.map(|x| FootPt {
            p: [x, y],
            n: [0.0, ny],
        })
    };
    let mut pts = ne.clone();
    pts.extend(goal(HALF_Y, -1.0, [GOAL_HALF_W, -GOAL_HALF_W]));
    pts.extend(mir(&ne, -1.0, 1.0, true));
    pts.extend(mir(&ne, -1.0, -1.0, false));
    pts.extend(goal(-HALF_Y, 1.0, [-GOAL_HALF_W, GOAL_HALF_W]));
    pts.extend(mir(&ne, 1.0, -1.0, true));
    pts
}

/// Profil de la paroi : (retrait horizontal vers l'intérieur, hauteur). Un point à la hauteur du but
/// est inséré pour pouvoir ouvrir la paroi de fond devant la cage.
fn profile() -> Vec<(f32, f32)> {
    const N: usize = 6;
    let r = RAMP_R;
    let mut p = Vec::new();
    for k in 0..=N {
        let a = k as f32 / N as f32 * FRAC_PI_2;
        p.push((r - r * a.sin(), r - r * a.cos()));
    }
    p.push((0.0, GOAL_H));
    for k in 0..=N {
        let a = k as f32 / N as f32 * FRAC_PI_2;
        p.push((r - r * a.cos(), HEIGHT - r + r * a.sin()));
    }
    p
}

fn at(f: FootPt, d: f32, z: f32) -> V3 {
    [f.p[0] + f.n[0] * d, f.p[1] + f.n[1] * d, z]
}

fn in_goal_gap(f: FootPt) -> bool {
    f.p[1].abs() > HALF_Y - 1.0 && f.p[0].abs() <= GOAL_HALF_W + 1.0
}

/// Vrai si (x, y) est dans le contour du terrain (pans coupés), réduit de `inset`.
pub fn inside_footprint(x: f32, y: f32, inset: f32) -> bool {
    let (ax, ay) = (x.abs(), y.abs());
    ax <= HALF_X - inset
        && ay <= HALF_Y - inset
        && ax + ay <= HALF_X + HALF_Y - CHAMFER - inset * std::f32::consts::SQRT_2
}

pub fn build() -> ArenaGeo {
    let mut g = ArenaGeo::default();
    let fp = footprint();
    let prof = profile();
    let n = fp.len();

    // Parois : une bande par segment du contour et par segment du profil.
    for i in 0..n {
        let (a, b) = (fp[i], fp[(i + 1) % n]);
        let gap = in_goal_gap(a) && in_goal_gap(b);
        for w in prof.windows(2) {
            let (p0, p1) = (w[0], w[1]);
            if gap && p0.1 <= GOAL_H + 0.5 && p1.1 <= GOAL_H + 0.5 {
                continue;
            }
            g.quad(
                at(a, p0.0, p0.1),
                at(b, p0.0, p0.1),
                at(b, p1.0, p1.1),
                at(a, p1.0, p1.1),
                Surf::Wall,
            );
        }
    }

    // Sol et plafond : éventails depuis le centre sur le contour réduit du rayon des raccords.
    for i in 0..n {
        let (a, b) = (fp[i], fp[(i + 1) % n]);
        for (z, surf) in [(0.0, Surf::Floor), (HEIGHT, Surf::Ceiling)] {
            let (pa, pb) = (at(a, RAMP_R, z), at(b, RAMP_R, z));
            g.tris.push(Tri {
                v: [[0.0, 0.0, z], pa, pb],
                surf,
            });
        }
    }
    // Sol devant les cages (le raccord est ouvert à cet endroit).
    for s in [-1.0_f32, 1.0] {
        let surf = if s < 0.0 {
            Surf::GoalBlue
        } else {
            Surf::GoalOrange
        };
        let (y0, y1) = (s * (HALF_Y - RAMP_R), s * (HALF_Y + GOAL_DEPTH));
        g.quad(
            [-GOAL_HALF_W, y0, 0.0],
            [GOAL_HALF_W, y0, 0.0],
            [GOAL_HALF_W, y1, 0.0],
            [-GOAL_HALF_W, y1, 0.0],
            Surf::Floor,
        );
        // Cage : fond, côtés, toit.
        let (yb, y_in) = (s * (HALF_Y + GOAL_DEPTH), s * HALF_Y);
        let (w, h) = (GOAL_HALF_W, GOAL_H);
        g.quad([-w, yb, 0.0], [w, yb, 0.0], [w, yb, h], [-w, yb, h], surf);
        g.quad(
            [-w, y_in, 0.0],
            [-w, yb, 0.0],
            [-w, yb, h],
            [-w, y_in, h],
            surf,
        );
        g.quad([w, y_in, 0.0], [w, yb, 0.0], [w, yb, h], [w, y_in, h], surf);
        g.quad([-w, y_in, h], [w, y_in, h], [w, yb, h], [-w, yb, h], surf);
        let kind = if s < 0.0 {
            LineKind::GoalBlue
        } else {
            LineKind::GoalOrange
        };
        let mut seg = |a: V3, b: V3| g.lines.push(Line { a, b, kind });
        seg([-w, y_in, 0.0], [-w, y_in, h]);
        seg([w, y_in, 0.0], [w, y_in, h]);
        seg([-w, y_in, h], [w, y_in, h]);
        seg([-w, yb, h], [w, yb, h]);
        seg([-w, y_in, h], [-w, yb, h]);
        seg([w, y_in, h], [w, yb, h]);
    }

    // Arêtes : contours du terrain aux hauteurs clés du profil.
    for &(d, z) in &[
        (RAMP_R, 0.0),
        (0.0, RAMP_R),
        (0.0, HEIGHT - RAMP_R),
        (RAMP_R, HEIGHT),
    ] {
        for i in 0..n {
            let (a, b) = (fp[i], fp[(i + 1) % n]);
            if in_goal_gap(a) && in_goal_gap(b) && z <= GOAL_H {
                continue;
            }
            g.lines.push(Line {
                a: at(a, d, z),
                b: at(b, d, z),
                kind: LineKind::Edge,
            });
        }
    }
    // Montants verticaux sur les parois droites (repères de défilement).
    for i in 0..n {
        let (a, b) = (fp[i], fp[(i + 1) % n]);
        if (a.n == b.n) && !in_goal_gap(a) {
            let len = ((b.p[0] - a.p[0]).powi(2) + (b.p[1] - a.p[1]).powi(2)).sqrt();
            let k = (len / 1024.0).floor() as i32;
            for j in 1..=k {
                let t = j as f32 / (k + 1) as f32;
                let f = FootPt {
                    p: [
                        a.p[0] + (b.p[0] - a.p[0]) * t,
                        a.p[1] + (b.p[1] - a.p[1]) * t,
                    ],
                    n: a.n,
                };
                g.lines.push(Line {
                    a: at(f, 0.0, RAMP_R),
                    b: at(f, 0.0, HEIGHT - RAMP_R),
                    kind: LineKind::Edge,
                });
            }
        }
    }

    // Quadrillage du sol (tous les 512) et marquages.
    let step = 512.0;
    let mut x = -HALF_X + step;
    while x < HALF_X {
        let mut y = -HALF_Y;
        while y < HALF_Y {
            let y2 = y + 256.0;
            if inside_footprint(x, y, RAMP_R) && inside_footprint(x, y2, RAMP_R) {
                g.lines.push(Line {
                    a: [x, y, 0.5],
                    b: [x, y2, 0.5],
                    kind: LineKind::Grid,
                });
            }
            y = y2;
        }
        x += step;
    }
    let mut y = -HALF_Y + step;
    while y < HALF_Y {
        let mut x = -HALF_X;
        while x < HALF_X {
            let x2 = x + 256.0;
            if inside_footprint(x, y, RAMP_R) && inside_footprint(x2, y, RAMP_R) {
                g.lines.push(Line {
                    a: [x, y, 0.5],
                    b: [x2, y, 0.5],
                    kind: LineKind::Grid,
                });
            }
            x = x2;
        }
        y += step;
    }
    g.lines.push(Line {
        a: [-HALF_X + RAMP_R, 0.0, 1.0],
        b: [HALF_X - RAMP_R, 0.0, 1.0],
        kind: LineKind::Marking,
    });
    const SEG: usize = 48;
    for k in 0..SEG {
        let (a0, a1) = (
            k as f32 / SEG as f32 * std::f32::consts::TAU,
            (k + 1) as f32 / SEG as f32 * std::f32::consts::TAU,
        );
        g.lines.push(Line {
            a: [1000.0 * a0.cos(), 1000.0 * a0.sin(), 1.0],
            b: [1000.0 * a1.cos(), 1000.0 * a1.sin(), 1.0],
            kind: LineKind::Marking,
        });
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_stays_inside_arena_bounds() {
        let g = build();
        assert!(g.tris.len() > 500 && g.lines.len() > 200);
        for t in &g.tris {
            for p in t.v {
                assert!(p[0].abs() <= HALF_X + 0.1, "{p:?}");
                assert!(p[1].abs() <= HALF_Y + GOAL_DEPTH + 0.1, "{p:?}");
                assert!((-0.1..=HEIGHT + 0.1).contains(&p[2]), "{p:?}");
            }
        }
    }

    #[test]
    fn has_floor_ceiling_and_both_goals() {
        let g = build();
        for s in [
            Surf::Floor,
            Surf::Wall,
            Surf::Ceiling,
            Surf::GoalBlue,
            Surf::GoalOrange,
        ] {
            assert!(g.tris.iter().any(|t| t.surf == s), "{s:?}");
        }
        assert!(
            g.tris
                .iter()
                .any(|t| t.surf == Surf::Ceiling && t.v.iter().all(|p| p[2] == HEIGHT))
        );
    }

    #[test]
    fn back_wall_is_open_in_front_of_goal_below_crossbar() {
        let g = build();
        // Aucun quad de paroi à y = ±HALF_Y au centre sous la hauteur du but.
        let blocked = g.tris.iter().any(|t| {
            t.surf == Surf::Wall
                && t.v.iter().all(|p| {
                    p[1].abs() > HALF_Y - 1.0
                        && p[0].abs() < GOAL_HALF_W - 1.0
                        && p[2] < GOAL_H - 1.0
                })
        });
        assert!(!blocked);
    }

    #[test]
    fn footprint_test_cuts_the_corners() {
        assert!(inside_footprint(0.0, 0.0, 0.0));
        assert!(!inside_footprint(HALF_X - 10.0, HALF_Y - 10.0, 0.0));
        assert!(inside_footprint(HALF_X - 10.0, 0.0, 0.0));
    }
}
