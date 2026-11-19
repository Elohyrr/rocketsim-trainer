//! Mesure du drill `turnaround` : temps pour faire face à la cible (+Y) avec de la vitesse dans cette direction,
//! voiture partie dos à la cible, en demi-tour classique (roulant, boost + frein à main) ou en half flip scripté.
//! Les deux familles de scripts sont optimisées avec le même budget (tirage aléatoire + descente locale).
//! Sert à régler le barème de `rl_env::drill::TurnTrack` (voir `docs/drills.md`).
//! Usage : `turnaround_bench [SAMPLES=n] [VMIN=1400]` (meshes via ROCKETSIM_MESHES_DIR) ;
//! `turnaround_bench best` rejoue la meilleure séquence (arrêt, boost 100) et trace la trajectoire.
use sim_core::{Controls, Sim, init_meshes, meshes_dir_from_env};

const MIN_COS: f32 = 0.866;
const MAX_TICKS: u32 = 480;

/// Critère d'arrivée : faire face à la cible (au sol) avec `Face` uu/s, ou avoir parcouru `Reach` uu vers la cible
/// (métrique de distance : tient compte du temps perdu ET du chemin gagné, sans exiger de vitesse).
#[derive(Clone, Copy)]
enum Goal {
    Face(f32),
    Reach(f32),
}

/// Résultat d'une exécution.
#[derive(Clone, Copy, Debug)]
struct Out {
    /// Premier tick où la voiture, AU SOL, fait face à la cible avec la vitesse demandée.
    ground: u32,
    /// Idem sans exiger le sol (diagnostic de métrique).
    air: u32,
    /// Position Y à `ground` (le départ est en 0 ; négatif = on est parti du mauvais côté).
    y_end: f32,
    /// Recul maximal (distance « perdue » : −min Y).
    lost: f32,
}

impl Out {
    fn secs(t: u32) -> f32 {
        t as f32 / 120.0
    }
}

/// Une exécution ; abandonne au-delà de `cap` ticks (pour accélérer la recherche).
fn run(
    goal: Goal,
    v0: f32,
    boost: f32,
    cap: u32,
    mut ctl: impl FnMut(u32, &mut Sim, u32) -> Controls,
) -> Option<Out> {
    // Une arène par thread, réutilisée (la créer coûte plus cher qu'un essai) ; `reset_car` remet tout l'état à zéro.
    thread_local! {
        static ARENA: std::cell::RefCell<Option<(Sim, u32)>> = const { std::cell::RefCell::new(None) };
    }
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        let (sim, car) = a.get_or_insert_with(|| {
            let mut sim = Sim::new_soccar().unwrap();
            let car = sim.add_car(true);
            (sim, car)
        });
        let car = *car;
        run_in(sim, car, goal, v0, boost, cap, &mut ctl)
    })
}

fn run_in(
    sim: &mut Sim,
    car: u32,
    goal: Goal,
    v0: f32,
    boost: f32,
    cap: u32,
    ctl: &mut impl FnMut(u32, &mut Sim, u32) -> Controls,
) -> Option<Out> {
    let yaw = -std::f32::consts::FRAC_PI_2; // dos à la cible (+Y)
    sim.reset_car(
        car,
        [0.0, 0.0, 17.0],
        yaw,
        [yaw.cos() * v0, yaw.sin() * v0, 0.0],
        boost,
    )
    .unwrap();
    sim.reset_ball([0.0, 4000.0, 93.0], [0.0; 3]);
    let mut air = u32::MAX;
    let mut ymin = 0.0f32;
    for t in 0..cap.min(MAX_TICKS) {
        let c = ctl(t, sim, car);
        sim.set_controls(car, c).unwrap();
        sim.step(1);
        let ct = sim.car_tick(car);
        ymin = ymin.min(ct.pos[1]);
        let h = ct.forward[0].hypot(ct.forward[1]).max(1e-3);
        let (ok, done) = match goal {
            Goal::Face(v) => {
                let ok = ct.forward[1] / h >= MIN_COS && ct.vel[1] >= v;
                (ok, ok && ct.on_ground)
            }
            Goal::Reach(y) => (ct.pos[1] >= y, ct.pos[1] >= y),
        };
        if ok && air == u32::MAX {
            air = t + 1;
        }
        if done {
            return Some(Out {
                ground: t + 1,
                air,
                y_end: ct.pos[1],
                lost: -ymin,
            });
        }
    }
    None
}

// ---------------------------------------------------------------- générateur pseudo-aléatoire
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.next() % v.len() as u64) as usize]
    }
}

// ---------------------------------------------------------------- demi-tour en roulant
/// Paramètres du demi-tour au sol : braquage (gain, ou plein braquage), frein à main, marche arrière initiale, boost.
#[derive(Clone, Copy, Debug)]
struct Roll {
    gain: f32,
    /// Frein à main tant que |écart| > cet angle (rad) ; 4 = jamais.
    hb_ang: f32,
    /// Ticks de marche arrière (throttle −1) au début, si le nez est encore à l'opposé.
    rev: u32,
    /// Accélérateur pendant le virage.
    thr: f32,
    /// Boost dès que |écart| < cet angle (rad) ; 4 = tout de suite, 0 = jamais.
    boost_ang: f32,
}

const ROLL_GAIN: [f32; 4] = [1.0, 2.0, 4.0, 8.0];
const ROLL_HB: [f32; 6] = [0.3, 0.8, 1.5, 2.2, 2.8, 4.0];
const ROLL_REV: [u32; 7] = [0, 10, 20, 30, 45, 60, 90];
const ROLL_THR: [f32; 3] = [0.3, 0.7, 1.0];
const ROLL_BOOST: [f32; 6] = [0.0, 0.6, 1.0, 1.5, 2.5, 4.0];

impl Roll {
    fn random(r: &mut Rng) -> Roll {
        Roll {
            gain: r.pick(&ROLL_GAIN),
            hb_ang: r.pick(&ROLL_HB),
            rev: r.pick(&ROLL_REV),
            thr: r.pick(&ROLL_THR),
            boost_ang: r.pick(&ROLL_BOOST),
        }
    }
    fn mutate(mut self, r: &mut Rng) -> Roll {
        match r.next() % 5 {
            0 => self.gain = r.pick(&ROLL_GAIN),
            1 => self.hb_ang = r.pick(&ROLL_HB),
            2 => self.rev = r.pick(&ROLL_REV),
            3 => self.thr = r.pick(&ROLL_THR),
            _ => self.boost_ang = r.pick(&ROLL_BOOST),
        }
        self
    }
    fn ctl(self, has_boost: bool) -> impl FnMut(u32, &mut Sim, u32) -> Controls {
        move |t, sim, car| {
            let ct = sim.car_tick(car);
            // ang = écart horaire au cap +Y ; près de ±π le sens est imposé (+1) pour ne pas osciller.
            let ang = ct.forward[0].atan2(ct.forward[1]);
            let steer = if ang.abs() > 2.9 {
                1.0
            } else {
                (ang * self.gain).clamp(-1.0, 1.0)
            };
            let reversing = t < self.rev && ct.forward[1] < 0.0;
            Controls {
                throttle: if reversing {
                    -1.0
                } else if ang.abs() < 0.6 {
                    1.0
                } else {
                    self.thr
                },
                steer,
                boost: has_boost && !reversing && ang.abs() < self.boost_ang,
                handbrake: ang.abs() > self.hb_ang,
                ..Default::default()
            }
        }
    }
}

// ---------------------------------------------------------------- half flip
use rl_env::half_flip::{HalfFlipScript as Hf, REFERENCE};

// ---------------------------------------------------------------- half flip (grille de recherche)
const HF_HOLD: [u32; 4] = [1, 2, 4, 8];
const HF_DODGE: [u32; 9] = [3, 4, 6, 8, 10, 14, 18, 24, 32];
const HF_SIDE: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];
const HF_CANCEL_AFTER: [u32; 8] = [0, 2, 4, 6, 8, 12, 16, 24];
const HF_CANCEL_LEN: [u32; 7] = [4, 8, 14, 20, 30, 45, 200];
const HF_ROLL: [f32; 3] = [-1.0, 0.0, 1.0];
const HF_ROLL_AFTER: [u32; 7] = [0, 2, 5, 8, 12, 20, 30];
const HF_ROLL_LEN: [u32; 7] = [10, 20, 30, 45, 60, 90, 200];
const HF_YAW: [f32; 3] = [-1.0, 0.0, 1.0];
const HF_BOOST: [f32; 7] = [-2.0, -0.5, 0.0, 0.3, 0.6, 0.85, 2.0];

fn hf_roll(r: &mut Rng, strict: bool) -> f32 {
    if strict {
        r.pick(&[-1.0, 1.0])
    } else {
        r.pick(&HF_ROLL)
    }
}

fn hf_random(r: &mut Rng, strict: bool) -> Hf {
    Hf {
        hold: r.pick(&HF_HOLD),
        pre_pitch: r.next() % 2 == 0,
        dodge_at: r.pick(&HF_DODGE),
        dodge_side: r.pick(&HF_SIDE),
        cancel_after: r.pick(&HF_CANCEL_AFTER),
        cancel_len: r.pick(&HF_CANCEL_LEN),
        roll: hf_roll(r, strict),
        roll_after: r.pick(&HF_ROLL_AFTER),
        roll_len: r.pick(&HF_ROLL_LEN),
        yaw: r.pick(&HF_YAW),
        boost_cos: r.pick(&HF_BOOST),
    }
}

fn hf_mutate(mut p: Hf, r: &mut Rng, strict: bool) -> Hf {
    match r.next() % 11 {
        0 => p.hold = r.pick(&HF_HOLD),
        1 => p.pre_pitch = !p.pre_pitch,
        2 => p.dodge_at = r.pick(&HF_DODGE),
        3 => p.dodge_side = r.pick(&HF_SIDE),
        4 => p.cancel_after = r.pick(&HF_CANCEL_AFTER),
        5 => p.cancel_len = r.pick(&HF_CANCEL_LEN),
        6 => p.roll = hf_roll(r, strict),
        7 => p.roll_after = r.pick(&HF_ROLL_AFTER),
        8 => p.roll_len = r.pick(&HF_ROLL_LEN),
        9 => p.yaw = r.pick(&HF_YAW),
        _ => p.boost_cos = r.pick(&HF_BOOST),
    }
    p
}

fn hf_ctl(p: Hf, has_boost: bool) -> impl FnMut(u32, &mut Sim, u32) -> Controls {
    move |t, sim, car| {
        let ct = sim.car_tick(car);
        p.controls(t, ct.forward, ct.on_ground, has_boost)
    }
}

// ---------------------------------------------------------------- recherche
struct Setup {
    goal: Goal,
    v0: f32,
    boost: f32,
}

/// Tirage + descente locale. Renvoie le meilleur paramètre et sa sortie ; `eval` donne le résultat d'un essai.
fn search<P: Copy>(
    seed: u64,
    start_cap: u32,
    samples: u32,
    random: impl Fn(&mut Rng) -> P,
    mutate: impl Fn(P, &mut Rng) -> P,
    eval: impl Fn(&P, u32) -> Option<Out>,
) -> Option<(P, Out)> {
    let mut r = Rng(seed);
    let mut best: Option<(P, Out)> = None;
    let cap = |b: &Option<(P, Out)>| b.as_ref().map_or(start_cap, |(_, o)| o.ground);
    for _ in 0..samples {
        let p = random(&mut r);
        if let Some(o) = eval(&p, cap(&best)) {
            best = Some((p, o));
        }
    }
    // Descente locale : mutations d'un paramètre depuis le meilleur.
    for _ in 0..samples {
        let Some((p0, _)) = best else { break };
        let p = mutate(p0, &mut r);
        if let Some(o) = eval(&p, cap(&best)) {
            best = Some((p, o));
        }
    }
    best
}

fn best_roll(s: &Setup, samples: u32) -> Option<(Roll, Out)> {
    let has_boost = s.boost > 0.0;
    search(
        1,
        MAX_TICKS,
        samples,
        Roll::random,
        |p, r| p.mutate(r),
        |p, cap| run(s.goal, s.v0, s.boost, cap, p.ctl(has_boost)),
    )
}

/// `strict` : vrai half flip (roulis ±1 imposé) ; sinon tout est permis (y compris des tours en l'air sans roulis).
fn best_hf(s: &Setup, samples: u32, strict: bool) -> Option<(Hf, Out)> {
    let has_boost = s.boost > 0.0;
    search(
        2,
        MAX_TICKS,
        samples,
        |r| hf_random(r, strict),
        |p, r| hf_mutate(p, r, strict),
        |p, cap| run(s.goal, s.v0, s.boost, cap, hf_ctl(*p, has_boost)),
    )
}

fn fmt(o: &Option<Out>) -> String {
    match o {
        Some(o) => format!(
            "{:.2} s (air {:.2}) y={:+.0} perdu={:.0} retard={:+.2}",
            Out::secs(o.ground),
            Out::secs(o.air),
            o.y_end,
            o.lost,
            Out::secs(o.ground) - o.y_end / 1400.0
        ),
        None => "—".into(),
    }
}

fn main() {
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("init");
    let samples: u32 = std::env::var("SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let env_f32 = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok());
    let vmin = env_f32("VMIN").unwrap_or(1400.0);
    // REACH=2000 : métrique de distance (temps pour avoir parcouru 2 000 uu vers la cible) au lieu de « face + vitesse ».
    let goal = env_f32("REACH").map_or(Goal::Face(vmin), Goal::Reach);
    let goal_txt = match goal {
        Goal::Face(v) => format!("face à la cible au sol avec ≥ {v} uu/s"),
        Goal::Reach(y) => format!("{y} uu parcourus vers la cible"),
    };

    if std::env::args().nth(1).as_deref() == Some("ref") {
        // Un seul jeu de paramètres, robuste : somme des temps sur les 4 départs avec boost.
        let starts = [0.0f32, -500.0, -1400.0, 500.0];
        let (p, o) = search(
            7,
            starts.len() as u32 * MAX_TICKS,
            samples,
            |r| hf_random(r, true),
            |p, r| hf_mutate(p, r, true),
            |p, cap| {
                let mut sum = 0;
                for v0 in starts {
                    sum += run(goal, v0, 100.0, MAX_TICKS, hf_ctl(*p, true))?.ground;
                    if sum > cap {
                        return None;
                    }
                }
                Some(Out {
                    ground: sum,
                    air: 0,
                    y_end: 0.0,
                    lost: 0.0,
                })
            },
        )
        .expect("aucun half flip");
        println!("{p:?}\nsomme des 4 temps : {:.2} s", Out::secs(o.ground));
        for v0 in starts {
            let r = run(goal, v0, 100.0, MAX_TICKS, hf_ctl(p, true));
            println!("  départ {v0:+}: {}", fmt(&r));
        }
        return;
    }

    if std::env::args().nth(1).as_deref() == Some("best") {
        let v0 = env_f32("V0").unwrap_or(0.0);
        let s = Setup {
            goal,
            v0,
            boost: 100.0,
        };
        let (p, o) = best_hf(&s, samples * 4, true).expect("aucun half flip");
        println!("{p:#?}\n{o:?}");
        let mut f = hf_ctl(p, true);
        run(goal, v0, 100.0, MAX_TICKS, |t, sm, c| {
            let ct = sm.car_tick(c);
            if t % 12 == 0 {
                println!(
                    "t={t:3} y={:6.0} fwd=({:+.2},{:+.2},{:+.2}) v=({:5.0},{:5.0}) sol={} flip={}",
                    ct.pos[1],
                    ct.forward[0],
                    ct.forward[1],
                    ct.forward[2],
                    ct.vel[0],
                    ct.vel[1],
                    ct.on_ground,
                    ct.is_flipping
                );
            }
            f(t, sm, c)
        });
        return;
    }

    println!("critère : {goal_txt} ; {samples}×2 essais par script");
    let cases = [
        ("à l'arrêt", 0.0, 100.0),
        ("marche arrière lente 500", -500.0, 100.0),
        ("marche arrière rapide 1400", -1400.0, 100.0),
        ("marche avant lente 500 (s'éloigne)", 500.0, 100.0),
        ("à l'arrêt, boost 0", 0.0, 0.0),
        ("marche arrière 1400, boost 0", -1400.0, 0.0),
    ];
    let results: Vec<_> = std::thread::scope(|sc| {
        let hs: Vec<_> = cases
            .iter()
            .map(|&(label, v0, boost)| {
                sc.spawn(move || {
                    let s = Setup { goal, v0, boost };
                    (
                        label,
                        v0,
                        boost,
                        best_roll(&s, samples),
                        best_hf(&s, samples, true),
                        best_hf(&s, samples, false),
                    )
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (label, v0, boost, r, h, hl) in results {
        println!("## {label}");
        println!("  roulant   : {}", fmt(&r.map(|x| x.1)));
        println!("  half flip : {}", fmt(&h.map(|x| x.1)));
        println!("  libre     : {}", fmt(&hl.map(|x| x.1)));
        let rf = run(goal, v0, boost, MAX_TICKS, hf_ctl(REFERENCE, boost > 0.0));
        println!("  référence : {}", fmt(&rf));
        if let Some((p, _)) = r {
            println!("    {p:?}");
        }
        if let Some((p, _)) = h {
            println!("    {p:?}");
        }
        if let Some((p, _)) = hl {
            println!("    libre {p:?}");
        }
    }
}
