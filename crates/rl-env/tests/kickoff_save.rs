//! Non-régression sur un vrai clip de prod : 1795096185-save-2308 était un coup d'envoi étiqueté « save » (la balle
//! posée au centre, deux voitures qui la touchent presque ensemble). Rejoué avec RocketSim puis passé au détecteur,
//! il ne doit produire ni `save` ni `aerial_save`, avec ou sans phase de kickoff annoncée. Exige les meshes
//! (ROCKETSIM_MESHES_DIR).

use mechanics::{Counts, Detector, Mechanic};
use rl_env::init_meshes_from_env;
use sim_core::Sim;
use sim_core::clip::{Clip, Replayer};

fn run(clip: &Clip, kickoff: bool) -> Counts {
    let mut sim = Sim::new_soccar().unwrap();
    let mut rp = Replayer::start(&mut sim, clip.clone()).unwrap();
    let me = rp.car_ids()[0];
    let mut det = Detector::new();
    if kickoff {
        det.begin_kickoff();
    }
    let mut counts = Counts::default();
    while !rp.tick(&mut sim) {
        let (c, b) = (sim.car_tick(me), sim.ball_tick());
        det.tick(&c, &b, &mut counts);
    }
    counts
}

#[test]
fn prod_kickoff_clip_2308_n_est_plus_un_save() {
    init_meshes_from_env().expect("meshes (ROCKETSIM_MESHES_DIR)");
    let json = include_bytes!("data/kickoff-save-2308.json");
    let clip = Clip::from_json(json).unwrap();
    assert_eq!(clip.mechanic, "save", "le clip d'origine est étiqueté save");
    for kickoff in [false, true] {
        let c = run(&clip, kickoff);
        assert_eq!(
            c.get(Mechanic::Save),
            0,
            "kickoff={kickoff} : {:?}",
            c.nonzero()
        );
        assert_eq!(c.get(Mechanic::AerialSave), 0, "kickoff={kickoff}");
    }
}
