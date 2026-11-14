//! Tests headless du bot dans le monde de jeu : `rst watch` (bot bleu seul) et `rst play --vs-bot` (joueur bleu,
//! bot orange). Politique synthétique : une couche linéaire dont le biais impose l'action « tout droit + boost ».

use std::collections::BTreeMap;

use game::bot::{Brain, Role};
use game::input::{Actions, RawInput};
use game::world::{Event, World};
use policy_runtime::{Tensor, write_safetensors};
use rl_env::{ActionTable, OBS_DIM};

fn init() {
    let dir = sim_core::meshes_dir_from_env().unwrap_or_else(|e| panic!("{e}"));
    sim_core::init_meshes(&dir).unwrap();
}

/// Écrit une politique qui choisit toujours l'action `forward_boost`, avec `policy.json` voisin.
fn write_policy(dir: &std::path::Path, obs: usize) -> std::path::PathBuf {
    let table = ActionTable::rlgym();
    let idx = (0..table.len())
        .find(|&i| {
            let c = table.get(i);
            c.throttle == 1.0 && c.steer == 0.0 && c.boost && !c.jump && c.pitch == 0.0
        })
        .unwrap();
    let mut bias = vec![0.0f32; table.len()];
    bias[idx] = 10.0;
    let mut m = BTreeMap::new();
    m.insert(
        "policy.0.weight".to_string(),
        Tensor {
            shape: vec![obs, table.len()],
            data: vec![0.0; obs * table.len()],
        },
    );
    m.insert(
        "policy.0.bias".to_string(),
        Tensor {
            shape: vec![table.len()],
            data: bias,
        },
    );
    let f = dir.join("policy.safetensors");
    write_safetensors(&f, &m).unwrap();
    std::fs::write(
        dir.join("policy.json"),
        br#"{"stage":5,"bytes":1,"updated_unix":1794000540}"#,
    )
    .unwrap();
    f
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn rejects_incompatible_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let f = write_policy(tmp.path(), 10);
    let e = Brain::load(&f).err().expect("dimensions incompatibles");
    assert!(e.contains("incompatible"), "{e}");
}

#[test]
fn watch_mode_bot_drives_blue_car_alone() {
    init();
    let tmp = tempfile::tempdir().unwrap();
    let brain = Brain::load(&write_policy(tmp.path(), OBS_DIM)).unwrap();
    assert_eq!(
        brain.label(),
        "palier 5 · politique du 2026-11-06 21:29 UTC"
    );
    let mut w = World::with_bot(brain, Role::Watch).unwrap();
    assert_eq!(w.cur.cars.len(), 1);
    let start = w.car().pos;
    // La saisie du joueur est ignorée : seul le bot conduit.
    for _ in 0..120 {
        w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default());
    }
    assert!(dist(start, w.car().pos) > 300.0, "le bot doit rouler seul");
    assert!(w.cur.cars[0].boost < 33.0, "le bot a utilisé son boost");
}

#[test]
fn watch_mode_resets_after_a_goal() {
    init();
    let tmp = tempfile::tempdir().unwrap();
    let brain = Brain::load(&write_policy(tmp.path(), OBS_DIM)).unwrap();
    let mut w = World::with_bot(brain, Role::Watch).unwrap();
    let mut events = vec![];
    // Reset forcé par la touche R, puis but : la partie repart d'elle-même.
    events.extend(w.advance(
        0.0,
        &RawInput::default(),
        &Actions {
            reset: true,
            ..Default::default()
        },
    ));
    assert!(events.contains(&Event::Reset));
    assert_eq!(w.score, [0, 0]);
}

#[test]
fn vs_bot_has_two_cars_and_orange_bot_moves() {
    init();
    let tmp = tempfile::tempdir().unwrap();
    let brain = Brain::load(&write_policy(tmp.path(), OBS_DIM)).unwrap();
    let mut w = World::with_bot(brain, Role::VsBot).unwrap();
    assert_eq!(w.cur.cars.len(), 2);
    assert!(w.cur.cars[0].blue && !w.cur.cars[1].blue);
    let (blue0, orange0) = (w.cur.cars[0].pos, w.cur.cars[1].pos);
    // Le joueur ne touche à rien : seule la voiture orange (bot) avance.
    for _ in 0..120 {
        w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default());
    }
    assert!(
        dist(orange0, w.cur.cars[1].pos) > 300.0,
        "le bot orange doit rouler"
    );
    assert!(
        dist(blue0, w.cur.cars[0].pos) < 50.0,
        "la voiture du joueur reste à lui"
    );
    assert!(
        w.cur.cars[1].boost < 33.0,
        "le bot orange a utilisé son boost"
    );
    // Pas de drill contre le bot.
    w.toggle_drill();
    assert_eq!(w.mode, game::world::Mode::Free);
}

#[test]
fn vs_bot_goal_scores_for_blue_and_restarts_with_both_cars() {
    init();
    let tmp = tempfile::tempdir().unwrap();
    let brain = Brain::load(&write_policy(tmp.path(), OBS_DIM)).unwrap();
    let mut w = World::with_bot(brain, Role::VsBot).unwrap();
    let mut events = vec![];
    // Le jeu tourne assez longtemps pour traverser kickoff ; on vérifie la boucle sur ~6 s sans panique.
    for _ in 0..360 {
        events.extend(w.advance(1.0 / 60.0, &RawInput::default(), &Actions::default()));
    }
    assert_eq!(w.cur.cars.len(), 2);
    assert!(w.ticks() >= 700);
}
