//! Client jouable : fenêtre macroquad, saisie clavier/souris/manette, boucle à pas fixe via `game::world`.

mod camera;
mod cmd;
mod render;

use std::sync::mpsc;

use game::config::{self, Bind, Config};
use game::input::{Actions, RawInput};
use game::meshes;
use game::pad::Pad;
use game::world::{Event, Mode, World};
use macroquad::miniquad::conf::{LinuxBackend, Platform};
use macroquad::prelude::*;
use rst_core::cli::{Cmd, PlayArgs};

fn window_conf() -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: macroquad::miniquad::conf::Conf {
            window_title: "rocketsim-trainer".to_string(),
            window_width: 1280,
            window_height: 720,
            sample_count: 4,
            window_resizable: true,
            platform: Platform {
                linux_backend: LinuxBackend::X11WithWaylandFallback,
                swap_interval: Some(1),
                ..Default::default()
            },
            ..Default::default()
        },
        draw_call_vertex_capacity: 40_000,
        draw_call_index_capacity: 40_000,
        ..Default::default()
    }
}

struct Binds {
    accelerate: Vec<Bind>,
    brake: Vec<Bind>,
    left: Vec<Bind>,
    right: Vec<Bind>,
    jump: Vec<Bind>,
    boost: Vec<Bind>,
    powerslide: Vec<Bind>,
    air_roll: Vec<Bind>,
    roll_left: Vec<Bind>,
    roll_right: Vec<Bind>,
    reset: Vec<Bind>,
    ball_cam: Vec<Bind>,
    slower: Vec<Bind>,
    faster: Vec<Bind>,
    drill: Vec<Bind>,
    quit: Vec<Bind>,
    mouse_air: bool,
    sens: f32,
}

impl Binds {
    fn new(c: &Config, w: &mut Vec<String>) -> Self {
        let k = &c.keyboard;
        let mut r = |n, what: &str| config::resolve(n, what, w);
        Self {
            accelerate: r(&k.accelerate, "accelerate"),
            brake: r(&k.brake, "brake"),
            left: r(&k.steer_left, "steer_left"),
            right: r(&k.steer_right, "steer_right"),
            jump: r(&k.jump, "jump"),
            boost: r(&k.boost, "boost"),
            powerslide: r(&k.powerslide, "powerslide"),
            air_roll: r(&k.air_roll, "air_roll"),
            roll_left: r(&k.air_roll_left, "air_roll_left"),
            roll_right: r(&k.air_roll_right, "air_roll_right"),
            reset: r(&k.reset, "reset"),
            ball_cam: r(&k.toggle_ball_cam, "toggle_ball_cam"),
            slower: r(&k.slower, "slower"),
            faster: r(&k.faster, "faster"),
            drill: r(&k.toggle_drill, "toggle_drill"),
            quit: r(&k.quit, "quit"),
            mouse_air: k.mouse_air_control,
            sens: k.mouse_sensitivity,
        }
    }
}

fn down(b: &[Bind]) -> bool {
    b.iter().any(|b| match b {
        Bind::Key(k) => is_key_down(*k),
        Bind::Mouse(m) => is_mouse_button_down(*m),
    })
}

fn pressed(b: &[Bind]) -> bool {
    b.iter().any(|b| match b {
        Bind::Key(k) => is_key_pressed(*k),
        Bind::Mouse(m) => is_mouse_button_pressed(*m),
    })
}

fn axis(neg: bool, pos: bool) -> f32 {
    f32::from(pos) - f32::from(neg)
}

fn sample_keyboard(b: &Binds) -> (RawInput, Actions) {
    let fwd = axis(down(&b.brake), down(&b.accelerate));
    let mouse = if b.mouse_air {
        let d = mouse_delta_position();
        [
            d.x * screen_width() * 0.5 * b.sens,
            d.y * screen_height() * 0.5 * b.sens,
        ]
    } else {
        [0.0, 0.0]
    };
    let raw = RawInput {
        throttle: fwd,
        steer: axis(down(&b.left), down(&b.right)),
        stick_y: fwd,
        mouse,
        jump: down(&b.jump) || pressed(&b.jump),
        boost: down(&b.boost),
        powerslide: down(&b.powerslide),
        air_roll: down(&b.air_roll),
        roll: axis(down(&b.roll_left), down(&b.roll_right)),
    };
    let actions = Actions {
        reset: pressed(&b.reset),
        toggle_ball_cam: pressed(&b.ball_cam),
        slower: pressed(&b.slower),
        faster: pressed(&b.faster),
        toggle_drill: pressed(&b.drill),
        quit: pressed(&b.quit),
    };
    (raw, actions)
}

fn merge_actions(a: Actions, b: Actions) -> Actions {
    Actions {
        reset: a.reset || b.reset,
        toggle_ball_cam: a.toggle_ball_cam || b.toggle_ball_cam,
        slower: a.slower || b.slower,
        faster: a.faster || b.faster,
        toggle_drill: a.toggle_drill || b.toggle_drill,
        quit: a.quit || b.quit,
    }
}

fn message(lines: &[&str]) {
    clear_background(Color::from_rgba(18, 22, 34, 255));
    let mut y = screen_height() / 2.0 - 30.0;
    for (i, l) in lines.iter().enumerate() {
        let size = if i == 0 { 34.0 } else { 22.0 };
        let d = measure_text(l, None, size as u16, 1.0);
        draw_text(l, (screen_width() - d.width) / 2.0, y, size, WHITE);
        y += 36.0;
    }
}

/// Retourne le dossier de meshes, en le téléchargeant si besoin (écran d'attente pendant ce temps).
async fn ensure_meshes() -> Result<std::path::PathBuf, String> {
    if let Some(d) = meshes::find_existing() {
        return Ok(d);
    }
    let dest = meshes::data_dir();
    eprintln!("meshes absents : téléchargement dans {}", dest.display());
    let (tx, rx) = mpsc::channel();
    let d2 = dest.clone();
    std::thread::spawn(move || {
        let _ = tx.send(meshes::download(&d2));
    });
    let mut t = 0.0;
    loop {
        t += get_frame_time();
        let dots = ".".repeat((t * 2.0) as usize % 4);
        message(&[
            "Premier lancement",
            &format!("Telechargement des donnees de collision{dots}"),
            "(une seule fois, verification d'integrite incluse)",
        ]);
        if let Ok(res) = rx.try_recv() {
            return res.map(|()| dest);
        }
        next_frame().await;
    }
}

fn hud(w: &World, pad_name: Option<&str>, binds_warn: &[String], show_help: bool) {
    let (sw, sh) = (screen_width(), screen_height());
    // Score
    let score = format!("BLEU {}  -  {} ORANGE", w.score[0], w.score[1]);
    let d = measure_text(&score, None, 34, 1.0);
    draw_rectangle(
        (sw - d.width) / 2.0 - 16.0,
        8.0,
        d.width + 32.0,
        46.0,
        Color::from_rgba(0, 0, 0, 120),
    );
    draw_text(&score, (sw - d.width) / 2.0, 42.0, 34.0, WHITE);
    if w.mode == Mode::Drill {
        let t = format!(
            "DRILL tir au but : {}/{}   {:.1} s",
            w.drill.successes,
            w.drill.attempts,
            (game::world::DRILL_TIME - w.drill.elapsed).max(0.0)
        );
        let d = measure_text(&t, None, 24, 1.0);
        draw_text(&t, (sw - d.width) / 2.0, 80.0, 24.0, YELLOW);
    }
    // Boost
    let car = w.car();
    let (bw, bh) = (280.0, 22.0);
    let (bx, by) = (sw - bw - 30.0, sh - 70.0);
    draw_rectangle(
        bx - 4.0,
        by - 4.0,
        bw + 8.0,
        bh + 8.0,
        Color::from_rgba(0, 0, 0, 140),
    );
    draw_rectangle(
        bx,
        by,
        bw * (car.boost / 100.0).clamp(0.0, 1.0),
        bh,
        Color::from_rgba(255, 170, 40, 255),
    );
    draw_text(
        format!("BOOST {:.0}", car.boost),
        bx,
        by - 10.0,
        26.0,
        WHITE,
    );
    // Vitesse (uu/s -> km/h, échelle usuelle x0,036)
    let v = car.vel;
    let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() * 0.036;
    draw_text(format!("{speed:.0} km/h"), bx, by + 56.0, 34.0, WHITE);
    // Etat
    let info = format!(
        "{} FPS | cam {} | x{:.2} | {}",
        get_fps(),
        if w.ball_cam { "balle" } else { "voiture" },
        w.time_scale,
        match pad_name {
            Some(n) => format!("manette : {n}"),
            None => "aucune manette (appuyer sur PS)".to_string(),
        }
    );
    draw_text(
        &info,
        14.0,
        24.0,
        22.0,
        Color::from_rgba(220, 220, 230, 255),
    );
    if let Some(wrn) = binds_warn.first() {
        draw_text(wrn, 14.0, 48.0, 20.0, Color::from_rgba(255, 120, 120, 255));
    }
    if w.goal_timer.is_some() {
        let t = "BUT !";
        let d = measure_text(t, None, 120, 1.0);
        draw_text(
            t,
            (sw - d.width) / 2.0,
            sh / 2.0 - 40.0,
            120.0,
            Color::from_rgba(255, 220, 80, 255),
        );
    }
    if show_help {
        let lines = [
            "W/S gaz-frein  A/D direction  Espace saut (2e appui = flip, avec W/S/A/D)",
            "Clic gauche / Ctrl boost  Maj / clic droit drift + roulis aerien  Q/E roulis",
            "R reset  C camera balle/voiture  [ ] ralenti  F2 drill  Echap quitter",
        ];
        for (i, l) in lines.iter().enumerate() {
            draw_text(
                l,
                14.0,
                sh - 66.0 + i as f32 * 22.0,
                20.0,
                Color::from_rgba(230, 230, 240, 200),
            );
        }
    }
}

fn main() {
    let cli = rst_core::cli::parse();
    match cli.cmd {
        None => play(cli.play),
        Some(Cmd::Play(a)) => play(a),
        Some(other) => std::process::exit(cmd::run(other)),
    }
}

fn play(args: PlayArgs) {
    macroquad::Window::from_config(window_conf(), run(args));
}

async fn run(args: PlayArgs) {
    let screenshot = args.screenshot;
    let demo = args.demo;
    let frames = args.frames;

    let dir = match ensure_meshes().await {
        Ok(d) => d,
        Err(e) => loop {
            message(&["Meshes de collision introuvables", &e, "Echap pour quitter"]);
            if is_key_pressed(KeyCode::Escape) {
                return;
            }
            next_frame().await;
        },
    };
    if let Err(e) = sim_core::init_meshes(&dir) {
        eprintln!("{e}");
        return;
    }
    let mut world = World::new().expect("arène");
    if args.drill {
        world.toggle_drill();
    }
    let (cfg, cfg_err) = config::load_or_create(&config::config_path());
    let mut warnings: Vec<String> = cfg_err.into_iter().collect();
    let binds = Binds::new(&cfg, &mut warnings);
    let mut pad = Pad::new(&cfg.gamepad, &mut warnings);
    for w in &warnings {
        eprintln!("config : {w}");
    }
    let arena = render::ArenaRender::new(&game::arena_geo::build());
    let mut cam = camera::Cam::new();
    if binds.mouse_air {
        set_cursor_grab(true);
        show_mouse(false);
    }
    let (mut frame, mut help_t) = (0u32, 12.0f32);
    let mut goal_flash = 0.0f32;

    loop {
        let dt = get_frame_time();
        let (kraw, kact) = sample_keyboard(&binds);
        let (praw, pact) = pad.sample();
        let mut raw = kraw.merge(praw);
        let mut actions = merge_actions(kact, pact);
        if demo {
            raw = RawInput {
                throttle: 1.0,
                stick_y: 1.0,
                boost: true,
                steer: if frame % 180 < 90 { 0.3 } else { -0.3 },
                jump: frame == 60,
                ..Default::default()
            };
            actions = Actions::default();
        }
        if actions.quit {
            if binds.mouse_air {
                set_cursor_grab(false);
                show_mouse(true);
            }
            break;
        }
        if kraw.jump || kraw.boost || kraw.throttle != 0.0 {
            help_t = help_t.min(4.0);
        }
        help_t -= dt;
        for e in world.advance(dt, &raw, &actions) {
            if matches!(e, Event::Goal { .. }) {
                goal_flash = 1.0;
            }
        }
        goal_flash = (goal_flash - dt).max(0.0);

        clear_background(Color::from_rgba(14, 18, 30, 255));
        set_camera(&cam.update(&world, dt));
        render::draw_world(&world, &arena);
        set_default_camera();
        if goal_flash > 0.0 {
            draw_rectangle(
                0.0,
                0.0,
                screen_width(),
                screen_height(),
                Color::new(1.0, 0.9, 0.4, goal_flash * 0.25),
            );
        }
        hud(&world, pad.name().as_deref(), &warnings, help_t > 0.0);

        frame += 1;
        if let Some(path) = &screenshot {
            if frame >= frames {
                get_screen_data().export_png(path);
                eprintln!("capture : {path}");
                break;
            }
        }
        next_frame().await;
    }
}
