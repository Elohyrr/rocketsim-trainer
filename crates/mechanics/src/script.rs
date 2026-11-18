//! Séquences synthétiques de ticks (voiture + balle) pour tester le détecteur, rejouables depuis d'autres
//! crates : [`cases`] donne, pour chaque mécanique, au moins une séquence positive et une négative avec les
//! compteurs attendus ; [`replay`] les rejoue dans un détecteur neuf.
//!
//! Ce ne sont pas des trajectoires physiques : seules les grandeurs lues par le détecteur sont cohérentes
//! (positions intégrées à partir des vitesses, drapeaux de saut/flip, contacts, touchers).

use crate::{BallTick, CarTick, Counts, Detector, Mechanic, TICK_HZ};

/// Un tick : état voiture et balle.
pub type Frame = (CarTick, BallTick);

/// Une séquence de test avec le résultat attendu.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: &'static str,
    /// Mécanique visée par le cas.
    pub mechanic: Mechanic,
    /// Vrai si la séquence doit déclencher `mechanic`.
    pub positive: bool,
    pub frames: Vec<Frame>,
    /// Compteurs exacts attendus (conséquences naturelles comprises, ex. `Touch`).
    pub expected: Counts,
}

/// Rejoue une séquence dans un détecteur neuf (règles v2) et rend les compteurs.
pub fn replay(frames: &[Frame]) -> Counts {
    replay_mode(frames, crate::Mode::V2)
}

/// Comme [`replay`], avec les règles de la version `mode`.
pub fn replay_mode(frames: &[Frame], mode: crate::Mode) -> Counts {
    let mut det = Detector::with_mode(mode);
    let mut counts = Counts::default();
    for (car, ball) in frames {
        det.tick(car, ball, &mut counts);
    }
    counts
}

/// Construit des compteurs à partir d'une liste `(mécanique, nombre)`.
pub fn counts(list: &[(Mechanic, u64)]) -> Counts {
    let mut c = Counts::default();
    for (m, k) in list {
        c.0[m.index()] += k;
    }
    c
}

/// Générateur de séquences : chaque tick intègre les positions (`pos += vel / 120`), avance `tick` et
/// `air_time`, applique la closure puis enregistre l'état.
#[derive(Clone, Debug)]
pub struct Script {
    pub car: CarTick,
    pub ball: BallTick,
    frames: Vec<Frame>,
}

impl Script {
    pub fn new(car: CarTick, ball: BallTick) -> Self {
        Self {
            car,
            ball,
            frames: Vec::new(),
        }
    }

    /// Avance de `n` ticks ; `f(i, voiture, balle)` est appelée à chaque tick (i = 0..n) après l'intégration.
    pub fn run(
        &mut self,
        n: usize,
        mut f: impl FnMut(usize, &mut CarTick, &mut BallTick),
    ) -> &mut Self {
        let dt = 1.0 / TICK_HZ;
        for i in 0..n {
            for k in 0..3 {
                self.car.pos[k] += self.car.vel[k] * dt;
                self.ball.pos[k] += self.ball.vel[k] * dt;
            }
            self.car.tick += 1;
            f(i, &mut self.car, &mut self.ball);
            self.car.air_time = if self.car.on_ground {
                0.0
            } else {
                self.car.air_time + dt
            };
            self.frames.push((self.car, self.ball));
        }
        self
    }

    /// Avance de `n` ticks sans rien changer.
    pub fn hold(&mut self, n: usize) -> &mut Self {
        self.run(n, |_, _, _| {})
    }

    /// Un tick où `f` modifie l'état.
    pub fn once(&mut self, f: impl FnOnce(&mut CarTick, &mut BallTick)) -> &mut Self {
        let mut f = Some(f);
        self.run(1, |_, c, b| {
            if let Some(f) = f.take() {
                f(c, b)
            }
        })
    }

    pub fn frames(&self) -> Vec<Frame> {
        self.frames.clone()
    }
}

// ---------------------------------------------------------------- états et actions de base

/// Voiture au sol, à plat, face à +x, à l'arrêt, flip disponible.
pub fn car_on_ground() -> CarTick {
    CarTick {
        pos: [0.0, 0.0, 17.0],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, 1.0, 0.0],
        up: [0.0, 0.0, 1.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    }
}

/// Balle posée au sol, loin de la voiture.
pub fn ball_far() -> BallTick {
    BallTick {
        pos: [3000.0, -3000.0, 93.0],
        vel: [0.0; 3],
    }
}

/// Décollage par un saut.
pub fn jump(c: &mut CarTick) {
    c.on_ground = false;
    c.wheels_contact = 0;
    c.has_jumped = true;
}

/// Retour au sol (roues sur le terrain, drapeaux remis à zéro).
pub fn land(c: &mut CarTick) {
    c.on_ground = true;
    c.wheels_contact = 4;
    c.has_jumped = false;
    c.has_double_jumped = false;
    c.has_flipped = false;
    c.is_flipping = false;
    c.has_flip_or_jump = true;
    c.flip_rel_torque = [0.0; 3];
    c.pitch_input = 0.0;
    c.roll_input = 0.0;
}

/// Dodge dans la direction (avant, latéral) du repère voiture, avec les commandes correspondantes.
pub fn dodge(c: &mut CarTick, fwd: f32, lat: f32) {
    c.has_flipped = true;
    c.is_flipping = true;
    c.has_flip_or_jump = false;
    c.flip_rel_torque = [fwd, lat, 0.0];
    c.pitch_input = -fwd.signum() * (fwd.abs() > 0.1) as i32 as f32;
    c.roll_input = lat.signum() * (lat.abs() > 0.1) as i32 as f32;
}

/// Toucher balle enregistré à ce tick.
pub fn hit(c: &mut CarTick) {
    c.ball_hit_tick = Some(c.tick);
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

// ---------------------------------------------------------------- séquences

/// Toucher simple au sol (deux changements de `ball_hit_tick` à 2 ticks d'écart = un seul toucher).
pub fn touch_sequence() -> Vec<Frame> {
    let mut s = Script::new(car_on_ground(), ball_far());
    s.ball.pos = [150.0, 0.0, 93.0];
    s.hold(10).once(|c, b| {
        hit(c);
        b.vel = [1200.0, 0.0, 0.0];
    });
    s.hold(1).once(|c, _| hit(c)).hold(20);
    s.frames()
}

/// `ball_hit_tick` ancien (déjà présent au premier tick) et inchangé : aucun toucher.
pub fn touch_negative_sequence() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.ball_hit_tick = Some(3);
    car.vel = [500.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(60);
    s.frames()
}

/// Saut volontaire depuis le sol, 0,58 s en l'air, toucher d'une balle à 530 uu (voiture à 470 uu).
pub fn aerial_touch_sequence() -> Vec<Frame> {
    aerial_touch_with(true, 68, 800.0, 60.0)
}

/// Décollage (saut pressé ou départ sans saut), `hold` ticks d'ascension à `vz`, puis toucher d'une balle à
/// `ball_dz` au-dessus de la voiture.
fn aerial_touch_with(jumped: bool, hold: usize, vz: f32, ball_dz: f32) -> Vec<Frame> {
    let mut s = Script::new(car_on_ground(), ball_far());
    s.hold(5).once(|c, _| {
        if jumped {
            jump(c);
        } else {
            c.on_ground = false;
            c.wheels_contact = 0;
        }
        c.vel = [0.0, 0.0, vz];
    });
    s.hold(hold).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [120.0, 0.0, ball_dz]);
        b.vel = [1500.0, 0.0, 300.0];
    });
    s.hold(10);
    s.frames()
}

/// v2 : en l'air 0,45 s seulement (voiture z ≈ 380, balle z ≈ 560) : pas un toucher aérien.
pub fn aerial_touch_negative_short_sequence() -> Vec<Frame> {
    aerial_touch_with(true, 52, 800.0, 180.0)
}

/// v2 : décollage sans saut (départ du sol sans appui), puis même ascension : pas volontaire.
pub fn aerial_touch_negative_no_jump_sequence() -> Vec<Frame> {
    aerial_touch_with(false, 68, 800.0, 60.0)
}

/// v2 : voiture trop basse au toucher (z ≈ 190) alors que la balle est haute.
pub fn aerial_touch_negative_low_car_sequence() -> Vec<Frame> {
    aerial_touch_with(true, 68, 300.0, 330.0)
}

/// v2 : balle trop basse (z ≈ 430) alors que la voiture est à 470 uu.
pub fn aerial_touch_negative_low_ball_sequence() -> Vec<Frame> {
    aerial_touch_with(true, 68, 800.0, -40.0)
}

/// v2 : la voiture roule du mur sans sauter, retombe en l'air et touche une balle qui monte tout droit.
pub fn aerial_touch_negative_wall_roll_sequence() -> Vec<Frame> {
    let mut s = Script::new(on_wall_car(), ball_far());
    s.hold(20).once(|c, _| {
        // Quitte le mur sans saut : plus de contact, pas de `has_jumped`.
        c.on_ground = false;
        c.wheels_contact = 0;
        c.up = [0.0, 0.0, 1.0];
        c.forward = [1.0, 0.0, 0.0];
        c.right = [0.0, 1.0, 0.0];
        c.vel = [0.0, 0.0, 400.0];
    });
    s.hold(70).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [0.0, 0.0, 120.0]);
        b.vel = [0.0, 0.0, 900.0];
    });
    s.hold(10);
    s.frames()
}

/// Saut simple sur une balle basse, 0,17 s après le décollage : toucher, pas aérien.
pub fn aerial_touch_negative_sequence() -> Vec<Frame> {
    let mut s = Script::new(car_on_ground(), ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.vel = [500.0, 0.0, 300.0];
    });
    s.hold(20).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [120.0, 0.0, 40.0]);
        b.vel = [1000.0, 0.0, 300.0];
    });
    s.hold(10);
    s.frames()
}

fn air_dribble_with(boost: bool, jumped: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.boosting = boost;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 217.0],
            vel: [0.0; 3],
        },
    );
    s.hold(5).once(|c, b| {
        if jumped {
            jump(c);
        } else {
            c.on_ground = false;
            c.wheels_contact = 0;
        }
        c.vel = [0.0, 0.0, 1000.0];
        b.vel = c.vel;
    });
    s.hold(60).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(10);
    s.frames()
}

/// Montée au boost avec la balle au-dessus, trois touchers aériens hauts à 0,25 s d'écart.
pub fn air_dribble_sequence() -> Vec<Frame> {
    air_dribble_with(true, true)
}

/// Même séquence sans boost (rebond passif) : pas d'air dribble.
pub fn air_dribble_negative_sequence() -> Vec<Frame> {
    air_dribble_with(false, true)
}

/// v2 : montée au boost avec la balle, mais sans saut volontaire au départ : pas d'air dribble ni de touchers
/// aériens validés.
pub fn air_dribble_negative_no_jump_sequence() -> Vec<Frame> {
    air_dribble_with(false, false)
}

/// Wall to air dribble : montée du mur avec la balle sur le capot, décollage poussé au boost (sans saut), trois
/// touchers aériens contrôlés au boost.
pub fn wall_to_air_dribble_sequence() -> Vec<Frame> {
    let mut car = on_wall_car();
    car.boosting = true;
    let mut s = Script::new(
        car,
        BallTick {
            pos: [car.pos[0] - 140.0, 0.0, car.pos[2]],
            vel: car.vel,
        },
    );
    s.hold(20).once(|c, b| {
        // La voiture quitte le mur au boost, la balle toujours sur le capot.
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [-100.0, 0.0, 600.0];
        b.vel = c.vel;
        b.pos = [c.pos[0] - 140.0, c.pos[1], c.pos[2]];
    });
    s.hold(60).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(10);
    s.frames()
}

/// Même départ du mur sans boost ni saut : la balle monte seule, la voiture la suit (cas passif).
pub fn wall_roll_follow_negative_sequence() -> Vec<Frame> {
    let mut s = Script::new(on_wall_car(), ball_far());
    s.hold(20).once(|c, b| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [-100.0, 0.0, 600.0];
        b.pos = [c.pos[0] - 140.0, c.pos[1], c.pos[2]];
        b.vel = c.vel;
    });
    s.hold(60).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(30).once(|c, _| hit(c));
    s.hold(10);
    s.frames()
}

/// Voiture en l'air, retournée, flip et double saut utilisés, qui monte vers une balle (spec §1.3 a).
fn upside_down_under_ball(roof_to_ball: bool) -> Script {
    let mut car = CarTick {
        pos: [0.0, 0.0, 800.0],
        vel: [0.0, 0.0, 300.0],
        forward: [1.0, 0.0, 0.0],
        has_jumped: true,
        has_double_jumped: true,
        has_flipped: true,
        air_time: 1.5,
        ..Default::default()
    };
    if roof_to_ball {
        car.up = [0.0, 0.0, 1.0];
        car.right = [0.0, 1.0, 0.0];
    } else {
        car.up = [0.0, 0.0, -1.0];
        car.right = [0.0, -1.0, 0.0];
    }
    Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 970.0],
            vel: [0.0; 3],
        },
    )
}

/// Flip reset (spec §1.3 a) : 2 roues puis 4 roues sur la balle, `on_ground` et flip rendu, sans toucher
/// enregistré ; un rebond des roues (on_ground faux puis vrai) ne recompte pas.
pub fn flip_reset_sequence() -> Vec<Frame> {
    let mut s = upside_down_under_ball(false);
    s.hold(10).run(4, |_, c, _| {
        c.vel = [0.0; 3];
        c.wheels_contact = 2;
    });
    s.once(|c, _| {
        c.on_ground = true;
        c.wheels_contact = 4;
        c.has_jumped = false;
        c.has_double_jumped = false;
        c.has_flipped = false;
        c.has_flip_or_jump = true;
    });
    s.hold(4)
        .once(|c, _| {
            c.on_ground = false;
            c.wheels_contact = 2;
        })
        .once(|c, _| {
            c.on_ground = true;
            c.wheels_contact = 4;
        })
        .hold(2)
        .once(|c, _| {
            c.on_ground = false;
            c.wheels_contact = 0;
            c.vel = [0.0, 0.0, -200.0];
        })
        .hold(30);
    s.frames()
}

/// Toit vers la balle (spec §1.3 a) : toucher enregistré, flip non rendu.
pub fn flip_reset_negative_roof_sequence() -> Vec<Frame> {
    let mut s = upside_down_under_ball(true);
    s.hold(10).once(|c, b| {
        hit(c);
        c.vel = [0.0, 0.0, 100.0];
        b.vel = [0.0, 0.0, 1000.0];
    });
    s.hold(20);
    s.frames()
}

/// Atterrissage sur un mur (z = 800, `world_contact`) avec flip rendu, balle loin : pas un flip reset.
pub fn flip_reset_negative_wall_sequence() -> Vec<Frame> {
    let car = CarTick {
        pos: [4000.0, 0.0, 800.0],
        vel: [800.0, 0.0, 0.0],
        forward: [0.0, 0.0, 1.0],
        right: [0.0, 1.0, 0.0],
        up: [-1.0, 0.0, 0.0],
        has_jumped: true,
        has_flipped: true,
        air_time: 1.0,
        ..Default::default()
    };
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        land(c);
        c.world_contact = true;
        c.vel = [0.0, 0.0, 300.0];
    });
    s.run(20, |_, c, _| c.world_contact = false);
    s.frames()
}

/// Roues posées sur la balle alors qu'aucun saut/flip n'avait été utilisé : rien n'est rendu, pas de reset.
pub fn flip_reset_negative_unused_sequence() -> Vec<Frame> {
    let mut s = upside_down_under_ball(false);
    s.car.has_jumped = false;
    s.car.has_double_jumped = false;
    s.car.has_flipped = false;
    s.car.has_flip_or_jump = true;
    s.hold(10).once(|c, _| {
        c.vel = [0.0; 3];
        c.on_ground = true;
        c.wheels_contact = 4;
    });
    s.hold(10);
    s.frames()
}

fn ceiling(touch_delay: usize, flip_available: bool) -> Vec<Frame> {
    let car = CarTick {
        pos: [0.0, 0.0, 2010.0],
        vel: [800.0, 0.0, 0.0],
        forward: [1.0, 0.0, 0.0],
        right: [0.0, -1.0, 0.0],
        up: [0.0, 0.0, -1.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    };
    let mut s = Script::new(car, ball_far());
    s.hold(30).once(|c, _| {
        c.on_ground = false;
        c.wheels_contact = 0;
        c.vel = [300.0, 0.0, -50.0];
        c.has_flip_or_jump = flip_available;
        c.has_jumped = !flip_available;
        c.has_flipped = !flip_available;
    });
    s.hold(touch_delay).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [120.0, 0.0, -40.0]);
        b.vel = [1500.0, 0.0, -800.0];
    });
    s.hold(10);
    s.frames()
}

/// Conduite au plafond, décrochage, toucher en l'air 0,5 s plus tard avec le flip disponible.
pub fn ceiling_shot_sequence() -> Vec<Frame> {
    ceiling(60, true)
}

/// Même décrochage, mais toucher 4 s plus tard : simple toucher aérien.
pub fn ceiling_shot_negative_late_sequence() -> Vec<Frame> {
    ceiling(480, true)
}

/// Même décrochage, toucher à 0,5 s mais flip déjà utilisé : simple toucher aérien.
pub fn ceiling_shot_negative_no_flip_sequence() -> Vec<Frame> {
    ceiling(60, false)
}

fn on_wall_car() -> CarTick {
    CarTick {
        pos: [4079.0, 0.0, 600.0],
        vel: [0.0, 0.0, 500.0],
        forward: [0.0, 0.0, 1.0],
        right: [0.0, 1.0, 0.0],
        up: [-1.0, 0.0, 0.0],
        on_ground: true,
        wheels_contact: 4,
        has_flip_or_jump: true,
        ..Default::default()
    }
}

/// Toucher depuis le mur qui envoie la balle (au repos) vers le but adverse (+y) à `vy`, après `before` uu/s
/// déjà vers le but.
fn wall_hit(vy: f32, before: f32) -> Vec<Frame> {
    let mut s = Script::new(on_wall_car(), ball_far());
    s.ball.vel = [0.0, before, 0.0];
    s.hold(20).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [-60.0, 0.0, 140.0]);
        b.vel = [-300.0, vy, 500.0];
    });
    s.hold(10);
    s.frames()
}

/// Conduite sur le mur, toucher qui envoie la balle vers le but adverse à 1 800 uu/s.
pub fn wall_shot_sequence() -> Vec<Frame> {
    wall_hit(1800.0, 0.0)
}

/// v2 : saut depuis le mur puis toucher (la balle part lentement) : saut volontaire depuis le mur.
pub fn wall_shot_jump_sequence() -> Vec<Frame> {
    let mut s = Script::new(on_wall_car(), ball_far());
    s.hold(10).once(|c, _| {
        jump(c);
        c.vel = [-300.0, 0.0, 500.0];
    });
    s.hold(15).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [-60.0, 0.0, 140.0]);
        b.vel = [-200.0, 300.0, 300.0];
    });
    s.hold(10);
    s.frames()
}

/// v2 : toucher depuis le mur qui envoie la balle vers le but adverse à 700 uu/s seulement : trop mou.
pub fn wall_shot_negative_slow_sequence() -> Vec<Frame> {
    wall_hit(700.0, 0.0)
}

/// v2 : toucher depuis le mur qui envoie la balle vers SON propre but à 1 800 uu/s.
pub fn wall_shot_negative_own_goal_sequence() -> Vec<Frame> {
    wall_hit(-1800.0, 0.0)
}

/// v2 : la balle roulait déjà à 1 800 uu/s vers le but et le toucher n'y ajoute presque rien.
pub fn wall_shot_negative_no_gain_sequence() -> Vec<Frame> {
    wall_hit(1900.0, 1800.0)
}

/// Conduite sur le mur, retour au sol, puis toucher au sol : pas un wall shot.
pub fn wall_shot_negative_sequence() -> Vec<Frame> {
    let mut s = Script::new(on_wall_car(), ball_far());
    s.hold(20).once(|c, _| {
        c.pos = [3900.0, 0.0, 17.0];
        c.vel = [-800.0, 0.0, 0.0];
        c.forward = [-1.0, 0.0, 0.0];
        c.right = [0.0, -1.0, 0.0];
        c.up = [0.0, 0.0, 1.0];
    });
    s.hold(30).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [-140.0, 0.0, 76.0]);
        b.vel = [-300.0, 1800.0, 0.0];
    });
    s.hold(10);
    s.frames()
}

fn double_touch(bounce: bool) -> Vec<Frame> {
    let car = CarTick {
        pos: [0.0, 4300.0, 600.0],
        vel: [0.0, 300.0, 0.0],
        forward: [0.0, 1.0, 0.0],
        right: [-1.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        has_jumped: true,
        air_time: 1.0,
        ..Default::default()
    };
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 4400.0, 650.0],
            vel: [0.0; 3],
        },
    );
    s.hold(5).once(|c, b| {
        hit(c);
        b.vel = if bounce {
            [0.0, 2000.0, 0.0]
        } else {
            [1000.0, 0.0, 0.0]
        };
    });
    s.run(140, |_, _, b| {
        if b.pos[1] > 5120.0 - 91.25 && b.vel[1] > 0.0 {
            b.vel[1] = -1500.0;
        }
    });
    s.once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [0.0, 120.0, 50.0]);
        b.vel = [0.0, 1500.0, 500.0];
    });
    s.hold(10);
    s.frames()
}

/// Toucher en l'air vers le mur du fond, rebond, second toucher en l'air 1,2 s après, sans retour au sol.
pub fn double_touch_sequence() -> Vec<Frame> {
    double_touch(true)
}

/// Deux touchers en l'air sans rebond mur entre les deux.
pub fn double_touch_negative_sequence() -> Vec<Frame> {
    double_touch(false)
}

/// Voiture au sol qui roule avec la balle sur le toit pendant `ticks`, touchers répétés tous les 3 ticks.
/// Balle sur le toit, voiture à 800 uu/s ; un toucher tous les `every` ticks (si `every` < 5 les contacts
/// fusionnent en un seul toucher).
fn dribble_script_with(ticks: usize, every: usize, speed: f32) -> Script {
    let mut car = car_on_ground();
    car.vel = [speed, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 157.0],
            vel: [speed, 0.0, 0.0],
        },
    );
    s.run(ticks, |i, c, _| {
        if i % every == 0 {
            hit(c);
        }
    });
    s
}

/// 100 ticks (0,83 s), 7 touchers distincts à 0,125 s, 667 uu parcourus.
fn dribble_script(ticks: usize) -> Script {
    dribble_script_with(ticks, 15, 800.0)
}

/// Balle sur le toit 0,83 s avec 7 touchers distincts, la voiture avance de 667 uu.
pub fn dribble_sequence() -> Vec<Frame> {
    let mut s = dribble_script(100);
    s.hold(5);
    s.frames()
}

/// v2 (équivalent « contrôle ≥ 2 s ») : 2,1 s de contrôle à 800 uu/s, un re-contact tous les 0,67 s (4 touchers).
pub fn dribble_long_sequence() -> Vec<Frame> {
    let mut s = dribble_script_with(252, 80, 800.0);
    s.hold(5);
    s.frames()
}

/// 0,67 s de contact continu (= un seul toucher) : l'ancien dribble de 60 ticks, refusé en v2.
pub fn dribble_negative_continuous_sequence() -> Vec<Frame> {
    let mut s = dribble_script_with(80, 3, 800.0);
    s.hold(5);
    s.frames()
}

/// 6 touchers distincts mais la voiture est à l'arrêt (équilibre sur place, 0 uu parcouru) : refusé.
pub fn dribble_negative_stationary_sequence() -> Vec<Frame> {
    let mut s = dribble_script_with(100, 15, 0.0);
    s.hold(5);
    s.frames()
}

/// 4 touchers distincts seulement, contrôle de 0,8 s : refusé (il en faut 5, ou 2 s de contrôle).
pub fn dribble_negative_few_touches_sequence() -> Vec<Frame> {
    let mut s = dribble_script_with(100, 25, 800.0);
    s.hold(5);
    s.frames()
}

/// Balle qui rebondit une fois sur le toit (dans la zone 15 ticks).
pub fn dribble_negative_bounce_sequence() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [800.0, 0.0, 0.0];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [0.0, 0.0, 400.0],
            vel: [800.0, 0.0, 0.0],
        },
    );
    s.hold(10).once(|c, b| {
        hit(c);
        b.pos = add(c.pos, [0.0, 0.0, 140.0]);
    });
    s.hold(14).once(|_, b| b.vel = [800.0, 0.0, 900.0]);
    s.hold(50);
    s.frames()
}

/// Voiture contre un mur (up horizontal) avec la balle « sur le toit » : pas un dribble.
pub fn dribble_negative_wall_sequence() -> Vec<Frame> {
    let mut car = on_wall_car();
    car.pos = [4079.0, 0.0, 150.0];
    car.vel = [0.0; 3];
    let mut s = Script::new(
        car,
        BallTick {
            pos: [3939.0, 0.0, 150.0],
            vel: [0.0; 3],
        },
    );
    s.hold(100);
    s.frames()
}

fn flick(with_hit: bool) -> Vec<Frame> {
    let mut s = dribble_script(100);
    s.once(|c, b| {
        jump(c);
        c.vel = [800.0, 0.0, 300.0];
        b.vel = c.vel;
    });
    s.hold(8).once(|c, _| dodge(c, 1.0, 0.0));
    s.hold(3).once(|c, b| {
        c.pitch_input = 0.0;
        if with_hit {
            hit(c);
        }
        b.vel = [2200.0, 0.0, 600.0];
    });
    s.hold(40).once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [900.0, 0.0, 0.0];
    });
    s.hold(5);
    s.frames()
}

/// Dribble, saut, dodge avant, toucher : balle à 2 300 uu/s.
pub fn flick_sequence() -> Vec<Frame> {
    flick(true)
}

/// Dribble, saut, dodge sans toucher de la balle : pas un flick.
pub fn flick_negative_sequence() -> Vec<Frame> {
    flick(false)
}

/// Dodge après un saut depuis le sol à 1 400 uu/s ; `cancel` = tangage opposé maintenu 12 ticks.
fn dodge_after_jump(
    fwd: f32,
    lat: f32,
    cancel: bool,
    land_after: usize,
    land_speed: f32,
) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1400.0, 0.0, 0.0];
    car.boosting = true;
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.vel[2] = 300.0;
    });
    s.hold(5).once(|c, _| {
        dodge(c, fwd, lat);
        c.vel = [1700.0, 300.0 * lat.signum(), 200.0];
    });
    s.run(3, |_, c, _| {
        c.pitch_input = 0.0;
        c.roll_input = 0.0;
    });
    s.run(12, |_, c, _| {
        if cancel {
            c.pitch_input = fwd.signum();
        }
    });
    s.run(land_after - 16, |_, c, _| {
        c.pitch_input = 0.0;
        c.vel = [land_speed, 0.0, -100.0];
    });
    s.once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [land_speed, 0.0, 0.0];
    });
    s.hold(10);
    s.frames()
}

/// Dodge diagonal avant-droite annulé (tangage +1 à +4 ticks), au sol 0,5 s après à 2 100 uu/s.
pub fn speed_flip_sequence() -> Vec<Frame> {
    let d = core::f32::consts::FRAC_1_SQRT_2;
    dodge_after_jump(d, d, true, 60, 2100.0)
}

/// Dodge diagonal non annulé (rotation complète).
pub fn speed_flip_negative_uncancelled_sequence() -> Vec<Frame> {
    let d = core::f32::consts::FRAC_1_SQRT_2;
    dodge_after_jump(d, d, false, 60, 2100.0)
}

/// Dodge droit avant annulé : flip cancel, pas un speed flip.
pub fn speed_flip_negative_forward_sequence() -> Vec<Frame> {
    dodge_after_jump(1.0, 0.0, true, 60, 2100.0)
}

fn wave_dash(land_after: usize, land_speed: f32) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [1000.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.vel[2] = 300.0;
    });
    s.hold(20).run(20, |_, c, _| c.vel[2] = -300.0);
    s.once(|c, _| {
        dodge(c, 1.0, 0.0);
        c.vel = [land_speed, 0.0, -300.0];
    });
    s.run(land_after - 1, |_, c, _| c.pitch_input = 0.0);
    s.once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [land_speed, 0.0, 0.0];
    });
    s.hold(10);
    s.frames()
}

/// Dodge avant juste au-dessus du sol, roues au sol 3 ticks après, 1 000 → 1 500 uu/s.
pub fn wave_dash_sequence() -> Vec<Frame> {
    wave_dash(3, 1500.0)
}

/// Même dodge, retour au sol 0,6 s après (flip complet).
pub fn wave_dash_negative_late_sequence() -> Vec<Frame> {
    wave_dash(72, 1500.0)
}

/// Dodge et retour au sol rapide, sans gain de vitesse.
pub fn wave_dash_negative_no_gain_sequence() -> Vec<Frame> {
    wave_dash(3, 1100.0)
}

fn half_flip(cancel: bool) -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [-600.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.hold(5).once(|c, _| {
        jump(c);
        c.vel[2] = 300.0;
    });
    s.hold(6).once(|c, _| {
        dodge(c, -1.0, 0.0);
        c.vel = [-1100.0, 0.0, 200.0];
    });
    s.run(14, |i, c, _| {
        c.pitch_input = if cancel && i >= 1 { -1.0 } else { 0.0 };
    });
    s.run(30, |i, c, _| {
        c.pitch_input = 0.0;
        c.roll_input = 1.0;
        if cancel && i == 15 {
            c.forward = [-1.0, 0.0, 0.0];
            c.right = [0.0, -1.0, 0.0];
        }
    });
    s.run(15, |_, c, _| c.roll_input = 0.0);
    s.once(|c, _| {
        land(c);
        c.pos[2] = 17.0;
        c.vel = [-1100.0, 0.0, 0.0];
    });
    s.hold(10);
    s.frames()
}

/// Recul, saut, dodge arrière, annulation (tangage −1), roulis, retour au sol face à −x.
pub fn half_flip_sequence() -> Vec<Frame> {
    half_flip(true)
}

/// Dodge arrière sans annulation (backflip) : pas de half flip.
pub fn half_flip_negative_backflip_sequence() -> Vec<Frame> {
    half_flip(false)
}

/// Demi-tour au sol (cap retourné en 0,5 s), sans dodge.
pub fn half_flip_negative_ground_turn_sequence() -> Vec<Frame> {
    let mut car = car_on_ground();
    car.vel = [600.0, 0.0, 0.0];
    let mut s = Script::new(car, ball_far());
    s.run(60, |i, c, _| {
        let a = core::f32::consts::PI * (i as f32 + 1.0) / 60.0;
        c.forward = [a.cos(), a.sin(), 0.0];
        c.right = [-a.sin(), a.cos(), 0.0];
        c.vel = [600.0 * a.cos(), 600.0 * a.sin(), 0.0];
    });
    s.hold(10);
    s.frames()
}

/// Tous les cas : au moins un positif et un négatif par mécanique du catalogue.
pub fn cases() -> Vec<Case> {
    use Mechanic::*;
    let c = |name, mechanic, positive, frames, expected: &[(Mechanic, u64)]| Case {
        name,
        mechanic,
        positive,
        frames,
        expected: counts(expected),
    };
    vec![
        c("touch", Touch, true, touch_sequence(), &[(Touch, 1)]),
        c(
            "touch_negative",
            Touch,
            false,
            touch_negative_sequence(),
            &[],
        ),
        c(
            "aerial_touch",
            AerialTouch,
            true,
            aerial_touch_sequence(),
            &[(Touch, 1), (AerialTouch, 1)],
        ),
        c(
            "aerial_touch_negative",
            AerialTouch,
            false,
            aerial_touch_negative_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "aerial_touch_negative_short",
            AerialTouch,
            false,
            aerial_touch_negative_short_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "aerial_touch_negative_no_jump",
            AerialTouch,
            false,
            aerial_touch_negative_no_jump_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "aerial_touch_negative_low_car",
            AerialTouch,
            false,
            aerial_touch_negative_low_car_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "aerial_touch_negative_low_ball",
            AerialTouch,
            false,
            aerial_touch_negative_low_ball_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "aerial_touch_negative_wall_roll",
            AerialTouch,
            false,
            aerial_touch_negative_wall_roll_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "air_dribble",
            AirDribble,
            true,
            air_dribble_sequence(),
            &[(Touch, 3), (AerialTouch, 3), (AirDribble, 1)],
        ),
        c(
            "air_dribble_negative_no_boost",
            AirDribble,
            false,
            air_dribble_negative_sequence(),
            &[(Touch, 3), (AerialTouch, 3)],
        ),
        c(
            "air_dribble_negative_no_jump",
            AirDribble,
            false,
            air_dribble_negative_no_jump_sequence(),
            &[(Touch, 3)],
        ),
        c(
            "wall_to_air_dribble",
            AirDribble,
            true,
            wall_to_air_dribble_sequence(),
            &[(Touch, 3), (AerialTouch, 3), (AirDribble, 1)],
        ),
        c(
            "wall_roll_follow_negative",
            AerialTouch,
            false,
            wall_roll_follow_negative_sequence(),
            &[(Touch, 3)],
        ),
        c(
            "flip_reset",
            FlipReset,
            true,
            flip_reset_sequence(),
            &[(FlipReset, 1)],
        ),
        c(
            "flip_reset_negative_roof",
            FlipReset,
            false,
            flip_reset_negative_roof_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "flip_reset_negative_wall",
            FlipReset,
            false,
            flip_reset_negative_wall_sequence(),
            &[],
        ),
        c(
            "flip_reset_negative_unused",
            FlipReset,
            false,
            flip_reset_negative_unused_sequence(),
            &[],
        ),
        c(
            "ceiling_shot",
            CeilingShot,
            true,
            ceiling_shot_sequence(),
            &[(Touch, 1), (CeilingShot, 1)],
        ),
        c(
            "ceiling_shot_negative_late",
            CeilingShot,
            false,
            ceiling_shot_negative_late_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "ceiling_shot_negative_no_flip",
            CeilingShot,
            false,
            ceiling_shot_negative_no_flip_sequence(),
            &[(Touch, 1), (AerialTouch, 1)],
        ),
        c(
            "wall_shot",
            WallShot,
            true,
            wall_shot_sequence(),
            &[(Touch, 1), (WallShot, 1)],
        ),
        c(
            "wall_shot_jump",
            WallShot,
            true,
            wall_shot_jump_sequence(),
            &[(Touch, 1), (WallShot, 1)],
        ),
        c(
            "wall_shot_negative",
            WallShot,
            false,
            wall_shot_negative_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "wall_shot_negative_slow",
            WallShot,
            false,
            wall_shot_negative_slow_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "wall_shot_negative_own_goal",
            WallShot,
            false,
            wall_shot_negative_own_goal_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "wall_shot_negative_no_gain",
            WallShot,
            false,
            wall_shot_negative_no_gain_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "double_touch",
            DoubleTouch,
            true,
            double_touch_sequence(),
            &[(Touch, 2), (DoubleTouch, 1)],
        ),
        c(
            "double_touch_negative",
            DoubleTouch,
            false,
            double_touch_negative_sequence(),
            &[(Touch, 2)],
        ),
        c(
            "dribble",
            Dribble,
            true,
            dribble_sequence(),
            &[(Touch, 6), (Dribble, 1)],
        ),
        c(
            "dribble_long",
            Dribble,
            true,
            dribble_long_sequence(),
            &[(Touch, 3), (Dribble, 1)],
        ),
        c(
            "dribble_negative_bounce",
            Dribble,
            false,
            dribble_negative_bounce_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "dribble_negative_wall",
            Dribble,
            false,
            dribble_negative_wall_sequence(),
            &[],
        ),
        c(
            "dribble_negative_continuous",
            Dribble,
            false,
            dribble_negative_continuous_sequence(),
            &[(Touch, 1)],
        ),
        c(
            "dribble_negative_stationary",
            Dribble,
            false,
            dribble_negative_stationary_sequence(),
            &[(Touch, 6)],
        ),
        c(
            "dribble_negative_few_touches",
            Dribble,
            false,
            dribble_negative_few_touches_sequence(),
            &[(Touch, 3)],
        ),
        c(
            "flick",
            Flick,
            true,
            flick_sequence(),
            &[(Touch, 7), (Dribble, 1), (Flick, 1)],
        ),
        c(
            "flick_negative",
            Flick,
            false,
            flick_negative_sequence(),
            &[(Touch, 6), (Dribble, 1)],
        ),
        c(
            "speed_flip",
            SpeedFlip,
            true,
            speed_flip_sequence(),
            &[(SpeedFlip, 1)],
        ),
        c(
            "speed_flip_negative_uncancelled",
            SpeedFlip,
            false,
            speed_flip_negative_uncancelled_sequence(),
            &[],
        ),
        c(
            "speed_flip_negative_forward",
            SpeedFlip,
            false,
            speed_flip_negative_forward_sequence(),
            &[],
        ),
        c(
            "wave_dash",
            WaveDash,
            true,
            wave_dash_sequence(),
            &[(WaveDash, 1)],
        ),
        c(
            "wave_dash_negative_late",
            WaveDash,
            false,
            wave_dash_negative_late_sequence(),
            &[],
        ),
        c(
            "wave_dash_negative_no_gain",
            WaveDash,
            false,
            wave_dash_negative_no_gain_sequence(),
            &[],
        ),
        c(
            "half_flip",
            HalfFlip,
            true,
            half_flip_sequence(),
            &[(HalfFlip, 1)],
        ),
        c(
            "half_flip_negative_backflip",
            HalfFlip,
            false,
            half_flip_negative_backflip_sequence(),
            &[],
        ),
        c(
            "half_flip_negative_ground_turn",
            HalfFlip,
            false,
            half_flip_negative_ground_turn_sequence(),
            &[],
        ),
    ]
}
