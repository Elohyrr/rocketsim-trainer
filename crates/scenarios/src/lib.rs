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
    /// Balle sur le toit, voiture orientée au hasard (souvent de côté ou dos au but), adversaire actif : il faut
    /// tourner avec la balle (dribble de virage), puis marquer.
    DribbleTurn,
    /// Tir à cadrer : balle qui roule au sol, voiture derrière elle.
    ShotRoll,
    /// Tir à cadrer : balle qui rebondit (descend), voiture en approche.
    ShotBounce,
    /// Centre : balle qui traverse devant le but depuis un côté.
    ShotCross,
    /// Lob : balle haute qui avance vers le but, voiture au sol.
    ShotLob,
    /// Gardien : balle tirée vers SA cage depuis divers angles, la voiture doit l'arrêter ou la dégager.
    Keeper,
    /// Aérien : balle haute en mouvement, voiture au sol avec du boost.
    AerialHigh,
    /// Mur : balle qui longe puis monte le mur latéral.
    WallRun,
    /// Récupération après rebond : balle repartie du mur du fond, la voiture doit la reprendre.
    Rebound,
    /// Balle au sol ou basse devant la voiture lancée : la frapper avec le nez pendant un dodge avant/diagonal.
    FlipShot,
    /// Demi-tour : voiture dos à la balle, faire face à la cible avec de la vitesse le plus vite possible.
    Turnaround,
    /// Balle sur le toit, voiture lancée vers le but, adversaire actif à distance moyenne : un flick qui part vite.
    Flick,
    /// Anneaux virtuels (points de passage sans collision) dans l'air et près des murs, à traverser dans l'ordre.
    Rings,
}

/// Nombre de drills.
pub const DRILL_COUNT: usize = 20;

impl Drill {
    pub const ALL: [Drill; DRILL_COUNT] = [
        Drill::RoofStill,
        Drill::RoofMoving,
        Drill::AerialTouch,
        Drill::AirDribble,
        Drill::WallBall,
        Drill::FlipReset,
        Drill::Recovery,
        Drill::DribbleTurn,
        Drill::ShotRoll,
        Drill::ShotBounce,
        Drill::ShotCross,
        Drill::ShotLob,
        Drill::Keeper,
        Drill::AerialHigh,
        Drill::WallRun,
        Drill::Rebound,
        Drill::FlipShot,
        Drill::Turnaround,
        Drill::Flick,
        Drill::Rings,
    ];

    /// Poids de tirage par défaut (même ordre que [`Drill::ALL`]) : les drills de la v1 pèsent 1, les nouvelles
    /// familles davantage (dribble de virage 3, gardien 2,5, tirs 1,5).
    pub const DEFAULT_WEIGHTS: [f32; DRILL_COUNT] = [
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 3.0, 1.5, 1.5, 1.5, 1.0, 2.5, 1.0, 1.0, 1.5, 3.0, 2.0, 2.0, 1.5,
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
            Drill::DribbleTurn => "dribble_turn",
            Drill::ShotRoll => "shot_roll",
            Drill::ShotBounce => "shot_bounce",
            Drill::ShotCross => "shot_cross",
            Drill::ShotLob => "shot_lob",
            Drill::Keeper => "keeper",
            Drill::AerialHigh => "aerial_high",
            Drill::WallRun => "wall_run",
            Drill::Rebound => "rebound",
            Drill::FlipShot => "flip_shot",
            Drill::Turnaround => "turnaround",
            Drill::Flick => "flick",
            Drill::Rings => "rings",
        }
    }

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Drill tiré uniformément avec `u` dans [0, 1).
    pub fn pick(u: f32) -> Drill {
        Self::pick_weighted(&[1.0; DRILL_COUNT], u)
    }

    /// Drill tiré avec `u` dans [0, 1) selon `weights` (≥ 0, même ordre que [`Drill::ALL`]) ; poids tous nuls =
    /// uniforme.
    pub fn pick_weighted(weights: &[f32; DRILL_COUNT], u: f32) -> Drill {
        let total: f32 = weights.iter().map(|w| w.max(0.0)).sum();
        if total <= 0.0 {
            return Self::pick(u);
        }
        let mut t = u * total;
        for (d, w) in Self::ALL.iter().zip(weights) {
            let w = w.max(0.0);
            if t < w {
                return *d;
            }
            t -= w;
        }
        // Reste d'arrondi : dernier drill de poids non nul.
        Self::ALL
            .iter()
            .zip(weights)
            .rev()
            .find(|(_, w)| **w > 0.0)
            .map_or(Drill::RoofStill, |(d, _)| *d)
    }

    /// Drill dont l'adversaire joue (politique adverse) ; les autres le laissent garé.
    pub fn active_opponent(self) -> bool {
        matches!(self, Drill::DribbleTurn | Drill::Flick)
    }

    /// Drill de dribble (clips possibles avec l'option `drill_clips`).
    pub fn is_dribble(self) -> bool {
        matches!(
            self,
            Drill::RoofStill | Drill::RoofMoving | Drill::DribbleTurn | Drill::Flick
        )
    }

    /// Valeur du label `drill` → drill.
    pub fn from_label(l: &str) -> Option<Drill> {
        Self::ALL.into_iter().find(|d| d.label() == l)
    }
}

/// Lit des poids `label=poids,label=poids` (labels de [`Drill::label`]) par-dessus [`Drill::DEFAULT_WEIGHTS`].
/// Erreur sur un label inconnu ou un poids négatif / illisible.
pub fn parse_weights(spec: &str) -> Result<[f32; DRILL_COUNT], String> {
    let mut w = Drill::DEFAULT_WEIGHTS;
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part
            .split_once('=')
            .ok_or_else(|| format!("« {part} » : attendu label=poids"))?;
        let d = Drill::from_label(k.trim()).ok_or_else(|| format!("drill inconnu : {k}"))?;
        let v: f32 = v
            .trim()
            .parse()
            .map_err(|_| format!("poids illisible : {v}"))?;
        if !(v >= 0.0 && v.is_finite()) {
            return Err(format!("poids négatif : {v}"));
        }
        w[d.index()] = v;
    }
    Ok(w)
}

/// Adversaire d'un drill : position, cap, boost ; `active` = il joue (politique adverse), sinon garé.
#[derive(Clone, Debug, PartialEq)]
pub struct OppSetup {
    pub pos: V3,
    pub yaw: f32,
    pub boost: f32,
    pub active: bool,
}

/// Anneau virtuel : point de passage (centre, rayon en uu), sans collision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub pos: V3,
    pub radius: f32,
}

/// État de départ d'un drill.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSetup {
    /// Anneaux à traverser dans l'ordre (drill `rings` seulement).
    pub rings: Vec<Ring>,
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
    /// Adversaire (None : l'environnement le gare près de sa cage, inactif).
    pub opponent: Option<OppSetup>,
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

/// Cap (radians) de `from` vers `to` dans le plan horizontal.
fn yaw_towards(from: V3, to: V3) -> f32 {
    (to[1] - from[1]).atan2(to[0] - from[0])
}

/// Garde (x, y) dans une marge du terrain.
fn clamp_xy(p: V3) -> V3 {
    [
        p[0].clamp(-3800.0, 3800.0),
        p[1].clamp(-4800.0, 4800.0),
        p[2],
    ]
}

/// Vitesse initiale (balle) pour aller de `from` à `to` à `speed` uu/s à l'horizontale 3D, gravité comprise.
fn aim(from: V3, to: V3, speed: f32) -> V3 {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
    let t = dist / speed.max(1.0);
    [
        d[0] / t,
        d[1] / t,
        ((d[2] + 0.5 * 650.0 * t * t) / t).clamp(-1500.0, 2200.0),
    ]
}

fn dist_xy(a: V3, b: V3) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Voiture au sol placée à `back` uu derrière la balle (par rapport à `dir`, vecteur horizontal unitaire) avec un
/// décalage latéral, cap vers la balle à `spread` près, vitesse `speed` le long du cap.
fn car_behind(
    unit: &mut dyn FnMut() -> f32,
    s: &mut DrillSetup,
    dir: [f32; 2],
    back: (f32, f32),
    side: f32,
    spread: f32,
    speed: (f32, f32),
) {
    let b = range(unit, back.0, back.1);
    let lat = range(unit, -side, side);
    let p = clamp_xy([
        s.ball_pos[0] - dir[0] * b - dir[1] * lat,
        s.ball_pos[1] - dir[1] * b + dir[0] * lat,
        17.0,
    ]);
    s.car_pos = p;
    s.car_yaw = yaw_towards(p, s.ball_pos) + range(unit, -spread, spread);
    let h = heading(s.car_yaw);
    let v = range(unit, speed.0, speed.1);
    s.car_vel = [h[0] * v, h[1] * v, 0.0];
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
        rings: Vec::new(),
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
        opponent: None,
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
        Drill::DribbleTurn => {
            // Souvent de côté ou dos au but : il faut tourner avec la balle avant de tirer.
            let off = if unit() < 0.75 {
                sign(unit) * range(unit, 0.8, PI)
            } else {
                range(unit, -0.4, 0.4)
            };
            let yaw = FRAC_PI_2 + off;
            let h = heading(yaw);
            let speed = if unit() < 0.4 {
                0.0
            } else {
                range(unit, 300.0, 1100.0)
            };
            s.car_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, -3000.0, 2500.0),
                17.0,
            ];
            s.car_yaw = yaw;
            s.car_vel = [h[0] * speed, h[1] * speed, 0.0];
            s.ball_pos = [
                s.car_pos[0] + h[0] * 25.0,
                s.car_pos[1] + h[1] * 25.0,
                ROOF_BALL_Z,
            ];
            s.ball_vel = s.car_vel;
            // Adversaire actif entre la voiture et son but, face à la balle.
            let opp = loop {
                let p = [
                    range(unit, -2500.0, 2500.0),
                    range(unit, 1500.0, 4300.0),
                    17.0,
                ];
                if dist_xy(p, s.car_pos) > 700.0 {
                    break p;
                }
            };
            s.opponent = Some(OppSetup {
                pos: opp,
                yaw: yaw_towards(opp, s.car_pos) + range(unit, -0.3, 0.3),
                boost: random_boost(unit),
                active: true,
            });
        }
        Drill::ShotRoll => {
            s.ball_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, 500.0, 3000.0),
                93.0,
            ];
            let goal = yaw_to_goal(unit, 0.9);
            let h = heading(goal);
            let sp = range(unit, 400.0, 1500.0);
            s.ball_vel = [h[0] * sp, h[1] * sp, 0.0];
            car_behind(
                unit,
                &mut s,
                [h[0], h[1]],
                (900.0, 2500.0),
                800.0,
                0.7,
                (0.0, 1400.0),
            );
        }
        Drill::ShotBounce => {
            s.ball_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, 500.0, 3000.0),
                range(unit, 400.0, 1000.0),
            ];
            let goal = yaw_to_goal(unit, 0.8);
            let h = heading(goal);
            let sp = range(unit, 300.0, 1300.0);
            s.ball_vel = [h[0] * sp, h[1] * sp, range(unit, -300.0, 200.0)];
            car_behind(
                unit,
                &mut s,
                [h[0], h[1]],
                (700.0, 2200.0),
                900.0,
                0.7,
                (200.0, 1400.0),
            );
        }
        Drill::ShotCross => {
            let side = sign(unit);
            s.ball_pos = [
                side * range(unit, 2600.0, 3600.0),
                range(unit, 2200.0, 3800.0),
                range(unit, 150.0, 700.0),
            ];
            let target = [
                range(unit, -500.0, 500.0),
                range(unit, 4000.0, 4700.0),
                250.0,
            ];
            let v = aim(s.ball_pos, target, range(unit, 900.0, 1700.0));
            s.ball_vel = [v[0], v[1], v[2].min(500.0)];
            let yaw = yaw_to_goal(unit, 0.6);
            let h = heading(yaw);
            let sp = range(unit, 300.0, 1400.0);
            s.car_pos = [
                -side * range(unit, 0.0, 1500.0),
                range(unit, 1000.0, 3000.0),
                17.0,
            ];
            s.car_yaw = yaw;
            s.car_vel = [h[0] * sp, h[1] * sp, 0.0];
        }
        Drill::ShotLob => {
            s.ball_pos = [
                range(unit, -2000.0, 2000.0),
                range(unit, 0.0, 2500.0),
                range(unit, 500.0, 1300.0),
            ];
            let goal = yaw_to_goal(unit, 0.5);
            let h = heading(goal);
            let sp = range(unit, 500.0, 1400.0);
            s.ball_vel = [h[0] * sp, h[1] * sp, range(unit, -100.0, 500.0)];
            car_behind(
                unit,
                &mut s,
                [h[0], h[1]],
                (600.0, 1800.0),
                900.0,
                0.6,
                (200.0, 1400.0),
            );
        }
        Drill::Keeper => {
            s.ball_pos = [
                range(unit, -3000.0, 3000.0),
                range(unit, -3500.0, 0.0),
                range(unit, 93.0, 600.0),
            ];
            let target = [
                range(unit, -800.0, 800.0),
                -5120.0,
                range(unit, 100.0, 600.0),
            ];
            s.ball_vel = aim(s.ball_pos, target, range(unit, 900.0, 2200.0));
            loop {
                s.car_pos = [
                    range(unit, -1500.0, 1500.0),
                    range(unit, -4600.0, -2200.0),
                    17.0,
                ];
                if dist_xy(s.car_pos, s.ball_pos) > 700.0 {
                    break;
                }
            }
            s.car_yaw = yaw_towards(s.car_pos, s.ball_pos) + range(unit, -0.5, 0.5);
            let h = heading(s.car_yaw);
            let sp = range(unit, 0.0, 900.0);
            s.car_vel = [h[0] * sp, h[1] * sp, 0.0];
        }
        Drill::AerialHigh => {
            s.ball_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, -1500.0, 2500.0),
                range(unit, 900.0, 1700.0),
            ];
            s.ball_vel = [
                range(unit, -600.0, 600.0),
                range(unit, 200.0, 900.0),
                range(unit, -300.0, 500.0),
            ];
            let away = range(unit, 800.0, 2200.0);
            let a = range(unit, -PI, PI);
            s.car_pos = clamp_xy([
                s.ball_pos[0] + a.cos() * away,
                s.ball_pos[1] + a.sin() * away,
                17.0,
            ]);
            s.car_yaw = yaw_towards(s.car_pos, s.ball_pos) + range(unit, -0.4, 0.4);
            let h = heading(s.car_yaw);
            let sp = range(unit, 0.0, 1200.0);
            s.car_vel = [h[0] * sp, h[1] * sp, 0.0];
        }
        Drill::WallRun => {
            let side = sign(unit);
            s.ball_pos = [
                side * range(unit, 3700.0, 3980.0),
                range(unit, -2000.0, 1500.0),
                range(unit, 250.0, 800.0),
            ];
            s.ball_vel = [
                side * range(unit, -200.0, 150.0),
                range(unit, 600.0, 1600.0),
                range(unit, 0.0, 600.0),
            ];
            s.car_pos = [
                side * range(unit, 3300.0, 3800.0),
                s.ball_pos[1] - range(unit, 800.0, 2000.0),
                17.0,
            ];
            s.car_yaw = FRAC_PI_2 + range(unit, -0.25, 0.25);
            let h = heading(s.car_yaw);
            let sp = range(unit, 700.0, 1500.0);
            s.car_vel = [h[0] * sp, h[1] * sp, 0.0];
        }
        Drill::FlipShot => {
            // Balle au sol (roulante ou à l'arrêt) ou basse, but dans l'axe ou en angle, voiture lancée derrière.
            s.ball_pos = [
                range(unit, -2200.0, 2200.0),
                range(unit, -500.0, 2800.0),
                if unit() < 0.7 {
                    93.0
                } else {
                    range(unit, 110.0, 260.0)
                },
            ];
            let goal = [
                range(unit, -800.0, 800.0) + 0.0,
                5120.0,
                300.0,
            ];
            let to_goal = yaw_towards(s.ball_pos, goal);
            let h = heading(to_goal);
            if unit() < 0.5 {
                let sp = range(unit, 200.0, 900.0);
                let a = to_goal + range(unit, -0.5, 0.5);
                s.ball_vel = [a.cos() * sp, a.sin() * sp, 0.0];
            }
            car_behind(
                unit,
                &mut s,
                [h[0], h[1]],
                (700.0, 1600.0),
                450.0,
                0.25,
                (700.0, 1500.0),
            );
        }
        Drill::Turnaround => {
            // Dos à la balle (cap tourné de 150° à 180°), à l'arrêt (40 %) ou roulant vers l'avant (le nez s'éloigne)
            // ou en marche arrière vers la balle.
            s.car_pos = [
                range(unit, -2200.0, 2200.0),
                range(unit, -2500.0, 2000.0),
                17.0,
            ];
            let a = range(unit, -PI, PI);
            let dist = range(unit, 1500.0, 3200.0);
            s.ball_pos = [
                (s.car_pos[0] + a.cos() * dist).clamp(-3500.0, 3500.0),
                (s.car_pos[1] + a.sin() * dist).clamp(-4500.0, 4500.0),
                93.0,
            ];
            s.car_yaw = yaw_towards(s.car_pos, s.ball_pos) + sign(unit) * range(unit, 2.62, PI);
            let h = heading(s.car_yaw);
            let u = unit();
            let v = if u < 0.4 {
                0.0
            } else if u < 0.7 {
                range(unit, 300.0, 1000.0)
            } else {
                -range(unit, 400.0, 1400.0)
            };
            s.car_vel = [h[0] * v, h[1] * v, 0.0];
            s.ball_vel = [0.0; 3];
        }
        Drill::Flick => {
            // Comme `roof_moving`, cap vers le but ou en angle, avec un adversaire actif à distance moyenne.
            let yaw = yaw_to_goal(unit, 0.9);
            let h = heading(yaw);
            let speed = range(unit, 500.0, 1400.0);
            s.car_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, -3500.0, 500.0),
                17.0,
            ];
            s.car_yaw = yaw;
            s.car_vel = [h[0] * speed, h[1] * speed, 0.0];
            s.ball_pos = [
                s.car_pos[0] + h[0] * 25.0,
                s.car_pos[1] + h[1] * 25.0,
                ROOF_BALL_Z,
            ];
            s.ball_vel = s.car_vel;
            let opp = loop {
                let d = range(unit, 1200.0, 2600.0);
                let a = yaw + range(unit, -0.6, 0.6);
                let p = clamp_xy([
                    s.car_pos[0] + a.cos() * d,
                    s.car_pos[1] + a.sin() * d,
                    17.0,
                ]);
                if dist_xy(p, s.car_pos) > 900.0 {
                    break p;
                }
            };
            s.opponent = Some(OppSetup {
                pos: opp,
                yaw: yaw_towards(opp, s.car_pos) + range(unit, -0.3, 0.3),
                boost: random_boost(unit),
                active: true,
            });
        }
        Drill::Rings => {
            s.car_boost = 100.0;
            s.car_pos = [range(unit, -2500.0, 2500.0), range(unit, -3800.0, 1500.0), 17.0];
            let n = 3 + (unit() * 4.0) as usize % 4;
            let mut dir = yaw_to_goal(unit, 1.2);
            s.car_yaw = dir;
            let mut p = s.car_pos;
            for i in 0..n {
                dir += range(unit, -0.9, 0.9);
                let step = range(unit, 800.0, 1500.0);
                let near_wall = unit() < 0.4;
                let z = if i == 0 {
                    range(unit, 250.0, 600.0)
                } else if near_wall {
                    range(unit, 300.0, 1100.0)
                } else {
                    range(unit, 400.0, 1400.0)
                };
                p = clamp_xy([p[0] + dir.cos() * step, p[1] + dir.sin() * step, z]);
                if near_wall {
                    p[0] = p[0].signum() * range(unit, 3500.0, 3850.0);
                }
                // Reste dans le terrain et fait demi-tour contre un mur au lieu d'y rester collé.
                if p[0].abs() > 3700.0 || p[1].abs() > 4600.0 {
                    dir += PI;
                }
                s.rings.push(Ring { pos: p, radius: range(unit, 260.0, 380.0) });
            }
            // Balle (inutile ici : la balle vue par l'agent est le prochain anneau) loin de tout.
            loop {
                s.ball_pos = [range(unit, -3000.0, 3000.0), range(unit, -4000.0, 4000.0), 93.0];
                if dist_xy(s.car_pos, s.ball_pos) > 800.0
                    && s.rings.iter().all(|r| dist_xy(r.pos, s.ball_pos) > 800.0)
                {
                    break;
                }
            }
        }
        Drill::Rebound => {
            s.ball_pos = [
                range(unit, -2500.0, 2500.0),
                range(unit, 3800.0, 4900.0),
                range(unit, 150.0, 800.0),
            ];
            s.ball_vel = [
                range(unit, -500.0, 500.0),
                -range(unit, 500.0, 1800.0),
                range(unit, -200.0, 400.0),
            ];
            s.car_pos = [
                range(unit, -2000.0, 2000.0),
                range(unit, 800.0, 2800.0),
                17.0,
            ];
            s.car_yaw = yaw_towards(s.car_pos, s.ball_pos) + range(unit, -0.8, 0.8);
            let h = heading(s.car_yaw);
            let sp = range(unit, 400.0, 1500.0);
            s.car_vel = [h[0] * sp, h[1] * sp, 0.0];
        }
    }
    s.car_boost = random_boost(unit);
    if drill == Drill::Rings {
        s.car_boost = 100.0;
    }
    if matches!(drill, Drill::AerialHigh | Drill::WallRun | Drill::ShotLob) {
        // Ces tirs demandent du boost pour être joués.
        s.car_boost = s.car_boost.max(range(unit, 40.0, 100.0));
    }
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
        assert_eq!(Drill::pick(0.999_999), Drill::Rings);
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
    fn poids_tirage_et_lecture() {
        let mut w = [0.0; DRILL_COUNT];
        w[Drill::Keeper.index()] = 2.0;
        w[Drill::Rebound.index()] = 2.0;
        assert_eq!(Drill::pick_weighted(&w, 0.1), Drill::Keeper);
        assert_eq!(Drill::pick_weighted(&w, 0.9), Drill::Rebound);
        assert_eq!(Drill::pick_weighted(&w, 0.999_999), Drill::Rebound);
        assert_eq!(
            Drill::pick_weighted(&[0.0; DRILL_COUNT], 0.0),
            Drill::RoofStill
        );
        let mut u = lcg(9);
        let mut seen = [0u32; DRILL_COUNT];
        for _ in 0..20000 {
            seen[Drill::pick_weighted(&Drill::DEFAULT_WEIGHTS, u()).index()] += 1;
        }
        assert!(seen.iter().all(|n| *n > 400), "{seen:?}");
        assert!(seen[Drill::DribbleTurn.index()] > 2 * seen[Drill::RoofStill.index()]);
        let w = parse_weights("dribble_turn=0, keeper=5").unwrap();
        assert_eq!(w[Drill::DribbleTurn.index()], 0.0);
        assert_eq!(w[Drill::Keeper.index()], 5.0);
        assert_eq!(w[Drill::Recovery.index()], 1.0);
        assert_eq!(parse_weights("").unwrap(), Drill::DEFAULT_WEIGHTS);
        assert!(parse_weights("inconnu=1").is_err());
        assert!(parse_weights("keeper=-1").is_err());
        assert!(parse_weights("keeper").is_err());
    }

    #[test]
    fn dribble_de_virage_adversaire_actif_et_cap_varie() {
        let mut u = lcg(21);
        let mut turn_needed = 0;
        for _ in 0..500 {
            let s = sample(Drill::DribbleTurn, &mut u);
            let o = s.opponent.as_ref().expect("adversaire");
            assert!(o.active && o.pos[1] > 1000.0);
            assert!(dist_xy(o.pos, s.car_pos) > 700.0);
            assert_eq!(s.ball_vel, s.car_vel);
            assert!(s.ball_pos[2] > 100.0);
            let off = (s.car_yaw - FRAC_PI_2 + PI).rem_euclid(2.0 * PI) - PI;
            if off.abs() > 0.78 {
                turn_needed += 1;
            }
        }
        assert!(turn_needed > 300, "{turn_needed}");
        // Les autres drills laissent l'adversaire garé.
        assert!(sample(Drill::Keeper, &mut u).opponent.is_none());
        assert!(Drill::DribbleTurn.active_opponent() && !Drill::RoofStill.active_opponent());
    }

    #[test]
    fn gardien_balle_vers_sa_cage_et_tirs_vers_le_but() {
        let mut u = lcg(33);
        for _ in 0..300 {
            let s = sample(Drill::Keeper, &mut u);
            assert!(s.ball_vel[1] < -200.0, "{:?}", s.ball_vel);
            // Atteint le fond (y = -5120) à l'intérieur de la largeur du but (± 893, marge de la gravité).
            let t = (-5120.0 - s.ball_pos[1]) / s.ball_vel[1];
            let x = s.ball_pos[0] + s.ball_vel[0] * t;
            assert!(x.abs() < 900.0, "{x}");
        }
        for d in [
            Drill::ShotRoll,
            Drill::ShotBounce,
            Drill::ShotLob,
            Drill::Rebound,
        ] {
            let s = sample(d, &mut u);
            assert!(s.car_axes.is_none() && s.car_pos[2] == 17.0, "{d:?}");
        }
    }

    #[test]
    fn drills_v3_etats() {
        let mut u = lcg(11);
        for _ in 0..500 {
            let r = sample(Drill::Rings, &mut u);
            assert!((3..=6).contains(&r.rings.len()), "{}", r.rings.len());
            assert_eq!(r.car_boost, 100.0);
            for g in &r.rings {
                assert!(g.pos[0].abs() < 4000.0 && g.pos[1].abs() < 5000.0);
                assert!((200.0..=1500.0).contains(&g.pos[2]) && g.radius >= 250.0);
            }
            // Turnaround : cap tourné de 150° à 180° par rapport à la balle.
            let t = sample(Drill::Turnaround, &mut u);
            let to = yaw_towards(t.car_pos, t.ball_pos);
            let d = (t.car_yaw - to).sin().atan2((t.car_yaw - to).cos()).abs();
            assert!(d >= 2.6 && d <= PI + 1e-3, "{d}");
            // Flip shot : balle basse, devant la voiture.
            let f = sample(Drill::FlipShot, &mut u);
            assert!(f.ball_pos[2] <= 260.0);
            let h = heading(f.car_yaw);
            let fwd = (f.ball_pos[0] - f.car_pos[0]) * h[0] + (f.ball_pos[1] - f.car_pos[1]) * h[1];
            assert!(fwd > 0.0);
            // Flick : adversaire actif, balle sur le toit.
            let k = sample(Drill::Flick, &mut u);
            assert!(k.opponent.as_ref().is_some_and(|o| o.active));
            assert!((k.ball_pos[2] - ROOF_BALL_Z).abs() < 1.0);
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
