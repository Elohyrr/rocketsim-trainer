//! Le détecteur reconnaît un speed flip scripté à 30 Hz (seuils révisés, voir `docs/trente-hz.md`) et pas le même
//! dodge sans annulation. Exige les meshes (ROCKETSIM_MESHES_DIR).

use mechanics::{Counts, Detector, Mechanic};
use sim_core::{Controls, Sim};

/// Coup d'envoi diagonal, boost permanent : saut tenu `hold` décisions, relâché `rel`, dodge diagonal, annulation
/// (`p+1`) `cancel_len` décisions après le dodge ; décisions de `q` ticks.
fn speed_flip(q: u32, hold: u32, rel: u32, cancel_len: u32) -> Counts {
    let mut sim = Sim::new_soccar().unwrap();
    let car = sim.add_car(true);
    sim.reset_car(
        car,
        [-2048.0, -2560.0, 17.0],
        std::f32::consts::FRAC_PI_4,
        [0.0; 3],
        33.0,
    )
    .unwrap();
    sim.reset_ball([0.0, 0.0, 93.0], [0.0; 3]);
    let mut det = Detector::new();
    let mut counts = Counts::default();
    let dodge = hold + rel;
    for t in 0..400u32 {
        let step = t / q;
        let mut c = Controls {
            throttle: 1.0,
            boost: true,
            ..Default::default()
        };
        if step < hold {
            c.jump = true;
        } else if step == dodge {
            c.jump = true;
            c.pitch = -1.0;
            c.roll = 1.0;
        } else if step > dodge && step <= dodge + cancel_len {
            c.pitch = 1.0;
        }
        sim.set_controls(car, c).unwrap();
        sim.step(1);
        let (ct, bt) = (sim.car_tick(car), sim.ball_tick());
        det.tick(&ct, &bt, &mut counts);
    }
    counts
}

#[test]
fn speed_flip_a_30_hz_reconnu_sans_annulation_refuse() {
    rl_env::init_meshes_from_env().expect("meshes (ROCKETSIM_MESHES_DIR)");
    // Paramètres tirés du banc `speed_flip_bench` (Q = 4) : retour au sol en 139 ticks, 1 844 uu/s.
    let ok = speed_flip(4, 4, 2, 20);
    assert_eq!(ok.get(Mechanic::SpeedFlip), 1, "{ok:?}");
    // Même dodge sans annulation : rotation complète, pas un speed flip.
    let none = speed_flip(4, 4, 2, 0);
    assert_eq!(none.get(Mechanic::SpeedFlip), 0, "{none:?}");
}
