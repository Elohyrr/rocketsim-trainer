//! Le détecteur de mécaniques face à la vraie physique RocketSim (spec § 1.3 a) : flip reset roues vers la
//! balle (reconnu) et toit vers la balle (toucher, pas de reset). Exige les meshes (ROCKETSIM_MESHES_DIR).

use mechanics::{Counts, Detector, Mechanic};
use sim_core::{Sim, init_meshes, meshes_dir_from_env};

/// Voiture en l'air qui monte vers une balle qui retombe ; `wheels_up` : voiture retournée, roues vers la balle.
fn run(wheels_up: bool) -> (Counts, bool) {
    init_meshes(&meshes_dir_from_env().expect("meshes")).expect("init meshes");
    let mut sim = Sim::new_soccar().unwrap();
    let car = sim.add_car(true);
    let axes = if wheels_up {
        [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]
    } else {
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    };
    sim.place_car_airborne(car, [0.0, 0.0, 600.0], [0.0, 0.0, 480.0], axes)
        .unwrap();
    sim.reset_ball([0.0, 0.0, 850.0], [0.0, 0.0, 0.0]);
    let (mut det, mut counts) = (Detector::new(), Counts::default());
    let mut regained = false;
    for _ in 0..240 {
        sim.step(1);
        let (c, b) = (sim.car_tick(car), sim.ball_tick());
        regained |= c.on_ground && !c.has_jumped && c.pos[2] > 250.0;
        det.tick(&c, &b, &mut counts);
    }
    (counts, regained)
}

#[test]
fn flip_reset_in_the_real_sim_is_detected() {
    let (counts, regained) = run(true);
    assert!(
        regained,
        "le scénario doit rendre le flip (roues sur la balle)"
    );
    assert_eq!(counts.get(Mechanic::FlipReset), 1, "{counts:?}");
}

#[test]
fn roof_contact_is_a_touch_without_flip_reset() {
    let (counts, regained) = run(false);
    assert!(!regained);
    assert_eq!(counts.get(Mechanic::FlipReset), 0, "{counts:?}");
    assert!(counts.get(Mechanic::Touch) >= 1, "{counts:?}");
}
