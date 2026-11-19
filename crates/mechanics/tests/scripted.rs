//! Séquences scriptées : chaque mécanique a au moins un cas positif et un négatif (module `script`).

use mechanics::script::{self, Frame};
use mechanics::{Counts, Detector, Events, Mechanic};
use std::hint::black_box;
use std::time::Instant;

#[test]
fn tous_les_cas_scriptes() {
    let mut failures = Vec::new();
    for case in script::cases() {
        let got = script::replay(&case.frames);
        let seen = case.observed(&got);
        if seen != case.expected {
            failures.push(format!(
                "{} : attendu {:?}, obtenu {:?}",
                case.name,
                case.expected.nonzero(),
                seen.nonzero()
            ));
        }
        assert_eq!(
            got.get(case.mechanic) > 0,
            case.positive,
            "{} : la mécanique visée {:?} doit être {}",
            case.name,
            case.mechanic,
            if case.positive { "reconnue" } else { "absente" }
        );
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn mode_v1_garde_l_ancien_comportement() {
    use mechanics::Mode::V1;
    let v1 = |f: Vec<Frame>| script::replay_mode(&f, V1);
    // Décollage sans saut, touchers à 0,5 s : v1 les compte, v2 non.
    assert_eq!(
        v1(script::aerial_touch_negative_no_jump_sequence()).get(Mechanic::AerialTouch),
        1
    );
    assert_eq!(
        v1(script::wall_roll_follow_negative_sequence()).get(Mechanic::AerialTouch),
        3
    );
    // Wall shot lent ou vers son propre but : v1 le compte.
    assert_eq!(
        v1(script::wall_shot_negative_slow_sequence()).get(Mechanic::WallShot),
        1
    );
    assert_eq!(
        v1(script::wall_shot_negative_own_goal_sequence()).get(Mechanic::WallShot),
        1
    );
    // Dribble de 0,67 s en contact continu, ou voiture à l'arrêt : v1 le compte (60 ticks).
    assert_eq!(
        v1(script::dribble_negative_continuous_sequence()).get(Mechanic::Dribble),
        1
    );
    assert_eq!(
        v1(script::dribble_negative_stationary_sequence()).get(Mechanic::Dribble),
        1
    );
    // Et la v2 les refuse.
    for f in [
        script::aerial_touch_negative_no_jump_sequence(),
        script::wall_roll_follow_negative_sequence(),
        script::wall_shot_negative_slow_sequence(),
        script::dribble_negative_continuous_sequence(),
    ] {
        let c = script::replay(&f);
        assert_eq!(
            c.get(Mechanic::AerialTouch) + c.get(Mechanic::WallShot) + c.get(Mechanic::Dribble),
            0
        );
    }
}

#[test]
fn chaque_mecanique_a_un_positif_et_un_negatif() {
    let cases = script::cases();
    for m in Mechanic::ALL {
        // `kickoff_won` n'existe qu'en phase de coup d'envoi : tests dédiés ci-dessous.
        if m == Mechanic::KickoffWon {
            continue;
        }
        assert!(
            cases.iter().any(|c| c.mechanic == m && c.positive),
            "{m:?} sans cas positif"
        );
        assert!(
            cases.iter().any(|c| c.mechanic == m && !c.positive),
            "{m:?} sans cas négatif"
        );
        let pos = cases
            .iter()
            .find(|c| c.mechanic == m && c.positive)
            .unwrap();
        assert_eq!(
            pos.expected.get(m),
            1,
            "{m:?} : une occurrence = un seul incrément"
        );
    }
}

fn replay_events(frames: &[Frame]) -> Vec<(Events, Option<u8>)> {
    let mut det = Detector::new();
    let mut counts = Counts::default();
    frames
        .iter()
        .map(|(c, b)| {
            det.tick(c, b, &mut counts);
            (det.events(), det.dodge_sector())
        })
        .collect()
}

#[test]
fn evenements_flip_reset_sans_toucher() {
    let ev = replay_events(&script::flip_reset_sequence());
    let count = |bit| ev.iter().filter(|(e, _)| e.has(bit)).count();
    assert_eq!(count(Events::RESET), 1);
    assert_eq!(count(Events::TOUCH_WHEELS), 1);
    assert_eq!(count(Events::TOUCH_AIR) + count(Events::TOUCH_GROUND), 0);
    let ev = replay_events(&script::flip_reset_negative_roof_sequence());
    assert_eq!(
        ev.iter().filter(|(e, _)| e.has(Events::TOUCH_ROOF)).count(),
        1
    );
    assert_eq!(ev.iter().filter(|(e, _)| e.has(Events::RESET)).count(), 0);
}

#[test]
fn evenements_dodge_secteur_et_annulation() {
    let ev = replay_events(&script::speed_flip_sequence());
    let dodges: Vec<_> = ev.iter().filter_map(|(_, s)| *s).collect();
    assert_eq!(dodges, vec![1], "dodge avant-droite = secteur 1");
    assert_eq!(ev.iter().filter(|(e, _)| e.has(Events::CANCEL)).count(), 1);
    let ev = replay_events(&script::speed_flip_negative_uncancelled_sequence());
    assert_eq!(ev.iter().filter(|(e, _)| e.has(Events::CANCEL)).count(), 0);
    let ev = replay_events(&script::half_flip_sequence());
    let dodges: Vec<_> = ev.iter().filter_map(|(_, s)| *s).collect();
    assert_eq!(dodges, vec![4], "dodge arrière = secteur 4");
    let ev = replay_events(&script::ceiling_shot_sequence());
    assert_eq!(
        ev.iter().filter(|(e, _)| e.has(Events::CEILING)).count(),
        0,
        "déjà au plafond au 1er tick"
    );
    let ev = replay_events(&script::double_touch_sequence());
    assert_eq!(
        ev.iter()
            .filter(|(e, _)| e.has(Events::BALL_WALL_BOUNCE))
            .count(),
        1
    );
}

#[test]
fn reset_oublie_l_etat() {
    let frames = script::flip_reset_sequence();
    let mut det = Detector::new();
    let mut counts = Counts::default();
    for _ in 0..2 {
        det.reset();
        for (c, b) in &frames {
            det.tick(c, b, &mut counts);
        }
    }
    assert_eq!(counts.get(Mechanic::FlipReset), 2);
    assert_eq!(counts.total(), 2);

    // Coupure au milieu d'un dribble : le reset remet la durée à zéro.
    let frames = script::dribble_sequence();
    let mut det = Detector::new();
    let mut counts = Counts::default();
    for (i, (c, b)) in frames.iter().enumerate() {
        if i == 40 {
            det.reset();
        }
        det.tick(c, b, &mut counts);
    }
    assert_eq!(counts.get(Mechanic::Dribble), 0);
}

#[test]
fn concatenation_des_cas() {
    // Tous les cas bout à bout, avec reset entre chaque : la somme des attendus.
    let mut det = Detector::new();
    let mut counts = Counts::default();
    let mut expected = Counts::default();
    for case in script::cases() {
        det.reset();
        let mut local = Counts::default();
        for (c, b) in &case.frames {
            det.tick(c, b, &mut local);
        }
        counts.add(&case.observed(&local));
        expected.add(&case.expected);
    }
    assert_eq!(counts, expected);
}

#[test]
fn bench_ns_par_tick() {
    let frames: Vec<Frame> = script::cases().into_iter().flat_map(|c| c.frames).collect();
    let mut det = Detector::new();
    let mut counts = Counts::default();
    let reps = 2_000_000 / frames.len() + 1;
    let start = Instant::now();
    for _ in 0..reps {
        for (c, b) in &frames {
            det.tick(black_box(c), black_box(b), &mut counts);
        }
    }
    let el = start.elapsed();
    let ticks = (reps * frames.len()) as f64;
    println!(
        "mechanics::Detector::tick : {:.1} ns/tick ({} ticks, total {})",
        el.as_nanos() as f64 / ticks,
        ticks,
        black_box(counts.total())
    );
}

#[test]
fn anciens_cas_et_catalogue_etendu() {
    // Les cas d'origine ne listent pas les mécaniques étendues : on relève ce qu'elles y reconnaissent
    // (conséquences naturelles, ex. un flick de dribble est aussi un flick_45) pour les passer en revue.
    let mut vu = Vec::new();
    for case in script::cases().iter().filter(|c| !c.exact) {
        let got = script::replay(&case.frames);
        for m in Mechanic::ALL.iter().skip(script::LEGACY_COUNT) {
            if got.get(*m) > 0 {
                vu.push(format!("{}→{}", case.name, m.label()));
            }
        }
    }
    // Attendu : le wall to air dribble est un air dribble ; le double touch d'origine rebondit sur le mur du fond.
    assert_eq!(
        vu,
        [
            "wall_to_air_dribble→wall_to_air_dribble",
            "double_touch→backboard_read",
            "double_touch→backboard_double"
        ]
    );
}

/// Rejoue des images comme un épisode de coup d'envoi (`Detector::begin_kickoff`).
fn replay_kickoff(frames: &[Frame]) -> Counts {
    let mut det = Detector::new();
    det.begin_kickoff();
    let mut counts = Counts::default();
    for (c, b) in frames {
        det.tick(c, b, &mut counts);
    }
    counts
}

fn case_frames(name: &str) -> Vec<Frame> {
    script::cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("cas inconnu {name}"))
        .frames
}

#[test]
fn kickoff_simultane_pas_de_save() {
    // Clip 1795095498-save-2038 : balle posée au centre, deux touchers à 1 tick d'écart.
    let f = mechanics::script::kickoff_two_touch_frames(true);
    assert_eq!(script::replay(&f).get(Mechanic::Save), 0);
    assert_eq!(replay_kickoff(&f).get(Mechanic::Save), 0);
    assert_eq!(replay_kickoff(&f).get(Mechanic::KickoffWon), 0);
}

#[test]
fn vrai_save_sur_tir_adverse() {
    let f = case_frames("save");
    assert_eq!(script::replay(&f).get(Mechanic::Save), 1);
}

#[test]
fn phase_de_kickoff_ne_garde_que_le_propre_au_kickoff() {
    // Le power shot est refusé pendant le kickoff, accepté sinon.
    let f = case_frames("power_shot");
    assert_eq!(script::replay(&f).get(Mechanic::PowerShot), 1);
    assert_eq!(replay_kickoff(&f).get(Mechanic::PowerShot), 0);
    // Speed flip et car pinch survivent.
    let f = case_frames("speed_flip");
    assert_eq!(replay_kickoff(&f).get(Mechanic::SpeedFlip), 1);
    let f = case_frames("car_pinch_kickoff");
    assert_eq!(replay_kickoff(&f).get(Mechanic::Pinch), 1);
}

#[test]
fn kickoff_won_sur_premier_toucher_seul() {
    let f = mechanics::script::kickoff_won_frames();
    assert_eq!(replay_kickoff(&f).get(Mechanic::KickoffWon), 1);
    // Hors coup d'envoi : jamais.
    assert_eq!(script::replay(&f).get(Mechanic::KickoffWon), 0);
}

#[test]
fn phase_de_kickoff_finit_apres_le_premier_toucher() {
    // 1,5 s après le premier toucher, les mécaniques ordinaires reviennent.
    let mut det = Detector::new();
    det.begin_kickoff();
    let mut counts = Counts::default();
    let f = mechanics::script::kickoff_won_frames();
    for (c, b) in &f {
        det.tick(c, b, &mut counts);
    }
    assert!(det.in_kickoff());
    let (mut c, b) = *f.last().unwrap();
    for _ in 0..200 {
        c.tick += 1;
        det.tick(&c, &b, &mut counts);
    }
    assert!(!det.in_kickoff());
}
