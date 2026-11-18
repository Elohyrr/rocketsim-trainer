//! États de départ des drills de mécaniques (« state setters »), sans dépendance : fonctions pures qui tirent une
//! situation à partir d'un générateur `unit()` (uniforme dans [0, 1)). L'environnement (`rl-env`) les applique à la
//! simulation. Unités RocketSim (uu, uu/s, radians). Documentation : `docs/drills.md`.

use std::f32::consts::{FRAC_PI_2, PI};

type V3 = [f32; 3];

/// Hauteur du centre de la balle posée sur le toit d'une voiture à l'arrêt (origine voiture à z = 17).
const ROOF_BALL_Z: f32 = 145.0;

/// Catalogue fermé des drills (index = position dans [`Drill::ALL`], label Prometheus `drill`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Drill {
    /// Balle sur le toit, voiture à l'arrêt (dribble, flick).
    RoofStill,
    /// Balle sur le toit, voiture en mouvement (dribble, flick).
    RoofMoving,
    /// Balle haute, voiture au sol avec du boost (toucher aérien).
    AerialTouch,
    /// Balle qui monte et avance, voiture dessous lancée avec boost (air dribble).
    AirDribble,
    /// Balle qui descend près du mur latéral, voiture au pied du mur (wall shot, air dribble depuis le mur).
    WallBall,
    /// Voiture en l'air sous la balle, flip disponible une fois sur deux (flip reset, plus tard).
    FlipReset,
    /// Voiture en l'air, orientation, vitesses et boost aléatoires : retomber sur les roues, vite, bien orientée.
    Recovery,
}

/// Nombre de drills.
pub const DRILL_COUNT: usize = 7;

impl Drill {
    pub const ALL: [Drill; DRILL_COUNT] = [
        Drill::RoofStill,
        Drill::RoofMoving,
        Drill::AerialTouch,
        Drill::AirDribble,
        Drill::WallBall,
        Drill::FlipReset,
        Drill::Recovery,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Drill::RoofStill => "roof_still",
            Drill::RoofMoving => "roof_moving",
            Drill::AerialTouch => "aerial_touch",
            Drill::AirDribble => "air_dribble",
            Drill::WallBall => "wall_ball",
            Drill::FlipReset => "flip_reset",
            Drill::Recovery => "recovery",
        }
    }

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Drill tiré uniformément avec `u` dans [0, 1).
    pub fn pick(u: f32) -> Drill {
        Self::ALL[((u * Self::ALL.len() as f32) as usize).min(Self::ALL.len() - 1)]
    }
}

/// État de départ d'un drill.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSetup {
    pub drill: Drill,
    pub ball_pos: V3,
    pub ball_vel: V3,
    pub car_pos: V3,
    /// Cap de la voiture (radians) ; sert au départ au sol.
    pub car_yaw: f32,
    pub car_vel: V3,
    /// Vitesse angulaire (rad/s), en l'air seulement.
    pub car_ang_vel: V3,
    pub car_boost: f32,
    /// `Some([avant, droite, haut])` : voiture lancée en l'air (le saut est fait) ; `None` : au sol.
    pub car_axes: Option<[V3; 3]>,
    /// En l'air : le flip est encore disponible.
    pub flip_available: bool,
}

fn range(unit: &mut dyn FnMut() -> f32, lo: f32, hi: f32) -> f32 {
    lo + unit() * (hi - lo)
}

/// Boost de départ : 25 % de 0 à 20, 25 % à 100, sinon uniforme sur 0–100 (apprendre avec ET sans boost).
pub fn random_boost(unit: &mut dyn FnMut() -> f32) -> f32 {
    let u = unit();
    if u < 0.25 {
        range(unit, 0.0, 20.0)
    } else if u < 0.5 {
        100.0
    } else {
        range(unit, 0.0, 100.0)
    }
}

fn sign(unit: &mut dyn FnMut() -> f32) -> f32 {
    if unit() < 0.5 { -1.0 } else { 1.0 }
}

/// Cap tourné vers le but adverse (+Y) à ± `spread` près.
fn yaw_to_goal(unit: &mut dyn FnMut() -> f32, spread: f32) -> f32 {
    FRAC_PI_2 + range(unit, -spread, spread)
}

fn heading(yaw: f32) -> V3 {
    [yaw.cos(), yaw.sin(), 0.0]
}

/// Axes (avant, droite, haut) d'une voiture de cap `yaw`, tangage `pitch` (+ = nez en haut), retournée sur le
/// dos si `upside_down`.
pub fn axes(yaw: f32, pitch: f32, upside_down: bool) -> [V3; 3] {
    axes_rolled(
        yaw,
        pitch,
        if upside_down {
            std::f32::consts::PI
        } else {
            0.0
        },
    )
}

/// Comme [`axes`], avec un roulis `roll` (radians) autour de l'axe avant.
pub fn axes_rolled(yaw: f32, pitch: f32, roll: f32) -> [V3; 3] {
    let (s, c) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let forward = [cp * c, cp * s, sp];
    let r0 = [-s, c, 0.0];
    // up = avant × droite (convention RocketSim : avant (c, s, 0), droite (−s, c, 0), haut +Z).
    let u0 = [
        forward[1] * r0[2] - forward[2] * r0[1],
        forward[2] * r0[0] - forward[0] * r0[2],
        forward[0] * r0[1] - forward[1] * r0[0],
    ];
    let (sr, cr) = roll.sin_cos();
    let mix = |a: V3, b: V3, ca: f32, cb: f32| [0, 1, 2].map(|i| ca * a[i] + cb * b[i]);
    [forward, mix(r0, u0, cr, sr), mix(u0, r0, cr, -sr)]
}

/// Tire l'état de départ de `drill`. Toutes les positions restent dans le terrain (|x| < 4096, |y| < 5120).
pub fn sample(drill: Drill, unit: &mut dyn FnMut() -> f32) -> DrillSetup {
    let mut s = DrillSetup {
        drill,
        ball_pos: [0.0; 3],
        ball_vel: [0.0; 3],
        car_pos: [0.0, 0.0, 17.0],
        car_yaw: 0.0,
        car_vel: [0.0; 3],
        car_ang_vel: [0.0; 3],
        car_boost: 33.0,
        car_axes: None,
        flip_available: false,
    };
    match drill {
        Drill::RoofStill | Drill::RoofMoving => {
            let yaw = yaw_to_goal(unit, 0.6);
            let h = heading(yaw);
            let speed = if drill == Drill::RoofMoving {
                range(unit, 600.0, 1400.0)
            } else {
                0.0
            };
            s.car_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, -3500.0, 1000.0),
                17.0,
            ];
            s.car_yaw = yaw;
            s.car_vel = [h[0] * speed, h[1] * speed, 0.0];
            // Balle légèrement vers le nez, même vitesse que la voiture.
            s.ball_pos = [
                s.car_pos[0] + h[0] * 25.0,
                s.car_pos[1] + h[1] * 25.0,
                ROOF_BALL_Z,
            ];
            s.ball_vel = s.car_vel;
        }
        Drill::AerialTouch => {
            let yaw = yaw_to_goal(unit, 0.4);
            let h = heading(yaw);
            s.car_pos = [
                range(unit, -2000.0, 2000.0),
                range(unit, -3000.0, 1500.0),
                17.0,
            ];
            s.car_yaw = yaw;
            let ahead = range(unit, 300.0, 900.0);
            s.ball_pos = [
                s.car_pos[0] + h[0] * ahead,
                s.car_pos[1] + h[1] * ahead,
                range(unit, 800.0, 1600.0),
            ];
            s.ball_vel = [
                range(unit, -200.0, 200.0),
                range(unit, 0.0, 400.0),
                range(unit, 0.0, 700.0),
            ];
        }
        Drill::AirDribble => {
            let yaw = yaw_to_goal(unit, 0.5);
            let h = heading(yaw);
            let bspeed = range(unit, 300.0, 900.0);
            s.ball_pos = [
                range(unit, -2000.0, 2000.0),
                range(unit, -3000.0, 500.0),
                range(unit, 300.0, 600.0),
            ];
            s.ball_vel = [h[0] * bspeed, h[1] * bspeed, range(unit, 500.0, 900.0)];
            let back = range(unit, 300.0, 600.0);
            let cspeed = range(unit, 600.0, 1100.0);
            s.car_pos = [
                s.ball_pos[0] - h[0] * back,
                s.ball_pos[1] - h[1] * back,
                17.0,
            ];
            s.car_yaw = yaw;
            s.car_vel = [h[0] * cspeed, h[1] * cspeed, 0.0];
        }
        Drill::WallBall => {
            let side = sign(unit);
            s.ball_pos = [
                side * range(unit, 3500.0, 3950.0),
                range(unit, -2500.0, 2000.0),
                range(unit, 700.0, 1400.0),
            ];
            s.ball_vel = [
                -side * range(unit, 0.0, 300.0),
                range(unit, 300.0, 1000.0),
                range(unit, -200.0, 300.0),
            ];
            let behind = range(unit, 600.0, 1500.0);
            s.car_pos = [
                side * range(unit, 3400.0, 3800.0),
                s.ball_pos[1] - behind,
                17.0,
            ];
            s.car_yaw = FRAC_PI_2;
            s.car_vel = [0.0, range(unit, 500.0, 1000.0), 0.0];
        }
        Drill::FlipReset => {
            let yaw = yaw_to_goal(unit, 0.5);
            s.ball_pos = [
                range(unit, -2000.0, 2000.0),
                range(unit, -2500.0, 1000.0),
                range(unit, 700.0, 1300.0),
            ];
            s.ball_vel = [0.0, range(unit, 200.0, 600.0), range(unit, -100.0, 200.0)];
            let below = range(unit, 250.0, 400.0);
            s.car_pos = [s.ball_pos[0], s.ball_pos[1] - 100.0, s.ball_pos[2] - below];
            s.car_yaw = yaw;
            s.car_vel = [
                s.ball_vel[0],
                s.ball_vel[1],
                s.ball_vel[2] + range(unit, 300.0, 600.0),
            ];
            // Sur le dos : les roues vers la balle.
            s.car_axes = Some(axes(yaw, range(unit, -0.2, 0.2), true));
            s.flip_available = unit() < 0.5;
        }
        Drill::Recovery => {
            s.car_pos = [
                range(unit, -3000.0, 3000.0),
                range(unit, -4000.0, 4000.0),
                range(unit, 300.0, 1500.0),
            ];
            let heading_vel = range(unit, -PI, PI);
            let hspeed = range(unit, 0.0, 1500.0);
            s.car_vel = [
                hspeed * heading_vel.cos(),
                hspeed * heading_vel.sin(),
                range(unit, -500.0, 500.0),
            ];
            s.car_yaw = range(unit, -PI, PI);
            s.car_ang_vel = [
                range(unit, -4.0, 4.0),
                range(unit, -4.0, 4.0),
                range(unit, -4.0, 4.0),
            ];
            s.car_axes = Some(axes_rolled(
                s.car_yaw,
                range(unit, -FRAC_PI_2, FRAC_PI_2),
                range(unit, -PI, PI),
            ));
            s.flip_available = unit() < 0.5;
            // Balle posée au sol à plus de 800 uu de la voiture (en projection).
            loop {
                s.ball_pos = [
                    range(unit, -3500.0, 3500.0),
                    range(unit, -4500.0, 4500.0),
                    93.0,
                ];
                let d = (s.ball_pos[0] - s.car_pos[0]).hypot(s.ball_pos[1] - s.car_pos[1]);
                if d > 800.0 {
                    break;
                }
            }
        }
    }
    s.car_boost = random_boost(unit);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: u64) -> impl FnMut() -> f32 {
        let mut x = seed;
        move || {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((x >> 40) as f32) / (1u64 << 24) as f32
        }
    }

    #[test]
    fn labels_et_index() {
        for (i, d) in Drill::ALL.iter().enumerate() {
            assert_eq!(d.index(), i);
        }
        let mut labels: Vec<_> = Drill::ALL.iter().map(|d| d.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Drill::ALL.len());
        assert_eq!(Drill::pick(0.0), Drill::RoofStill);
        assert_eq!(Drill::pick(0.999_999), Drill::Recovery);
    }

    #[test]
    fn etats_dans_le_terrain_et_coherents() {
        let mut u = lcg(7);
        for d in Drill::ALL {
            for _ in 0..500 {
                let s = sample(d, &mut u);
                for p in [s.ball_pos, s.car_pos] {
                    assert!(p[0].abs() < 4000.0 && p[1].abs() < 5000.0, "{d:?} {p:?}");
                    assert!(p[2] > 0.0 && p[2] < 1900.0, "{d:?} {p:?}");
                }
                assert!(s.car_boost <= 100.0);
                // La balle ne démarre pas dans la voiture.
                let dz = s.ball_pos[2] - s.car_pos[2];
                let dxy = ((s.ball_pos[0] - s.car_pos[0]).powi(2)
                    + (s.ball_pos[1] - s.car_pos[1]).powi(2))
                .sqrt();
                assert!((dz * dz + dxy * dxy).sqrt() > 100.0, "{d:?}");
                assert_eq!(
                    s.car_axes.is_some(),
                    d == Drill::FlipReset || d == Drill::Recovery
                );
            }
        }
    }

    #[test]
    fn toit_balle_au_dessus_de_la_voiture_et_meme_vitesse() {
        let mut u = lcg(3);
        let s = sample(Drill::RoofMoving, &mut u);
        assert_eq!(s.ball_vel, s.car_vel);
        assert!(s.ball_pos[2] > 100.0);
        let s = sample(Drill::RoofStill, &mut u);
        assert_eq!(s.car_vel, [0.0; 3]);
    }

    #[test]
    fn boost_aleatoire_couvre_vide_et_plein() {
        let mut u = lcg(11);
        let b: Vec<f32> = (0..2000).map(|_| random_boost(&mut u)).collect();
        assert!(b.iter().all(|x| (0.0..=100.0).contains(x)));
        let low = b.iter().filter(|x| **x <= 20.0).count();
        let full = b.iter().filter(|x| **x >= 100.0).count();
        assert!(low > 500 && full > 400, "{low} {full}");
    }

    #[test]
    fn recovery_en_l_air_vitesses_bornees() {
        let mut u = lcg(5);
        for _ in 0..300 {
            let s = sample(Drill::Recovery, &mut u);
            assert!(s.car_pos[2] >= 300.0);
            assert!(s.car_ang_vel.iter().all(|w| w.abs() <= 4.0));
            let [f, r, up] = s.car_axes.unwrap();
            let n = |a: V3| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            assert!(
                (n(f) - 1.0).abs() < 1e-4
                    && (n(r) - 1.0).abs() < 1e-4
                    && (n(up) - 1.0).abs() < 1e-4
            );
        }
    }

    #[test]
    fn axes_orthonormes_et_sur_le_dos() {
        let [f, r, up] = axes(0.7, 0.1, true);
        let dot = |a: V3, b: V3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        assert!(dot(f, r).abs() < 1e-5 && dot(f, up).abs() < 1e-5 && dot(r, up).abs() < 1e-5);
        assert!(up[2] < -0.9);
        assert!(axes(0.7, 0.0, false)[2][2] > 0.99);
    }
}
