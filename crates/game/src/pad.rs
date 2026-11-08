//! Manette via gilrs : échantillonnage de l'état et des fronts montants.

use gilrs::{Axis, Button, EventType, Gamepad as GilrsPad, GamepadId, Gilrs, MappingSource};

use crate::config::{Gamepad, Names};
use crate::input::{Actions, RawInput};

/// Nom de bouton → bouton gilrs. Accepte les noms gilrs (`South`, `LeftTrigger2`…) et les noms
/// PlayStation (`Cross`, `L1`, `R2`, `Share`…), sans tenir compte de la casse.
pub fn parse_button(name: &str) -> Option<Button> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "south" | "cross" => Button::South,
        "east" | "circle" => Button::East,
        "north" | "triangle" => Button::North,
        "west" | "square" => Button::West,
        "lefttrigger" | "l1" => Button::LeftTrigger,
        "lefttrigger2" | "l2" => Button::LeftTrigger2,
        "righttrigger" | "r1" => Button::RightTrigger,
        "righttrigger2" | "r2" => Button::RightTrigger2,
        "select" | "share" | "create" => Button::Select,
        "start" | "options" => Button::Start,
        "mode" | "ps" => Button::Mode,
        "leftthumb" | "l3" => Button::LeftThumb,
        "rightthumb" | "r3" => Button::RightThumb,
        "dpadup" => Button::DPadUp,
        "dpaddown" => Button::DPadDown,
        "dpadleft" => Button::DPadLeft,
        "dpadright" => Button::DPadRight,
        _ => return None,
    })
}

fn buttons(n: &Names, what: &str, warnings: &mut Vec<String>) -> Vec<Button> {
    let list: Vec<&str> = match n {
        Names::One(s) => vec![s.as_str()],
        Names::Many(v) => v.iter().map(String::as_str).collect(),
    };
    list.into_iter()
        .filter_map(|s| {
            let b = parse_button(s);
            if b.is_none() {
                warnings.push(format!(
                    "bouton de manette inconnu « {s} » pour {what} (ignoré)"
                ));
            }
            b
        })
        .collect()
}

/// Rescale un axe avec zone morte : 0 sous le seuil, puis rampe jusqu'à ±1.
pub fn deadzone(v: f32, dz: f32) -> f32 {
    if v.abs() <= dz {
        0.0
    } else {
        (v.abs() - dz) / (1.0 - dz) * v.signum()
    }
}

/// Écarte les nœuds evdev annexes des manettes Sony (pavé tactile, capteurs de mouvement), que le
/// noyau expose comme des périphériques séparés et que gilrs peut prendre pour une manette.
fn usable(gp: &GilrsPad<'_>) -> bool {
    let name = gp.name().to_ascii_lowercase();
    gp.mapping_source() != MappingSource::None
        && !name.contains("touchpad")
        && !name.contains("motion")
}

pub struct Pad {
    gilrs: Option<Gilrs>,
    active: Option<GamepadId>,
    cfg: Gamepad,
    accelerate: Vec<Button>,
    brake: Vec<Button>,
    jump: Vec<Button>,
    boost: Vec<Button>,
    powerslide: Vec<Button>,
    air_roll: Vec<Button>,
    roll_left: Vec<Button>,
    roll_right: Vec<Button>,
    reset: Vec<Button>,
    toggle_cam: Vec<Button>,
    pressed: Vec<Button>,
}

impl Pad {
    pub fn new(cfg: &Gamepad, warnings: &mut Vec<String>) -> Self {
        let gilrs = match Gilrs::new() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("manette indisponible ({e}) : clavier/souris seulement");
                None
            }
        };
        let active = gilrs
            .as_ref()
            .and_then(|g| g.gamepads().find(|(_, gp)| usable(gp)).map(|(id, _)| id));
        Self {
            gilrs,
            active,
            accelerate: buttons(&cfg.accelerate, "accelerate", warnings),
            brake: buttons(&cfg.brake, "brake", warnings),
            jump: buttons(&cfg.jump, "jump", warnings),
            boost: buttons(&cfg.boost, "boost", warnings),
            powerslide: buttons(&cfg.powerslide, "powerslide", warnings),
            air_roll: buttons(&cfg.air_roll, "air_roll", warnings),
            roll_left: buttons(&cfg.air_roll_left, "air_roll_left", warnings),
            roll_right: buttons(&cfg.air_roll_right, "air_roll_right", warnings),
            reset: buttons(&cfg.reset, "reset", warnings),
            toggle_cam: buttons(&cfg.toggle_ball_cam, "toggle_ball_cam", warnings),
            cfg: cfg.clone(),
            pressed: Vec::new(),
        }
    }

    pub fn connected(&self) -> bool {
        self.active.is_some()
    }

    /// Nom de la manette active (ex. « Wireless Controller »).
    pub fn name(&self) -> Option<String> {
        let g = self.gilrs.as_ref()?;
        Some(g.connected_gamepad(self.active?)?.name().to_string())
    }

    /// Traduit l'état des boutons et du stick gauche en entrée brute. Un même bouton peut servir à
    /// plusieurs actions (ex. L1 : powerslide et roulis libre).
    fn raw_input(&self, btn: impl Fn(Button) -> f32, stick: [f32; 2]) -> RawInput {
        let value = |bs: &[Button]| bs.iter().map(|b| btn(*b)).fold(0.0_f32, f32::max);
        let dz = self.cfg.deadzone.clamp(0.0, 0.9);
        RawInput {
            throttle: value(&self.accelerate) - value(&self.brake),
            steer: deadzone(stick[0], dz),
            stick_y: deadzone(stick[1], dz),
            mouse: [0.0, 0.0],
            jump: value(&self.jump) > 0.5,
            boost: value(&self.boost) > 0.5,
            powerslide: value(&self.powerslide) > 0.5,
            air_roll: value(&self.air_roll) > 0.5,
            roll: f32::from(value(&self.roll_right) > 0.5)
                - f32::from(value(&self.roll_left) > 0.5),
        }
    }

    /// À appeler une fois par frame : vide les évènements et retourne l'état de la manette active.
    pub fn sample(&mut self) -> (RawInput, Actions) {
        self.pressed.clear();
        let Some(mut g) = self.gilrs.take() else {
            return Default::default();
        };
        while let Some(ev) = g.next_event() {
            let ok = g.connected_gamepad(ev.id).is_some_and(|gp| usable(&gp));
            match ev.event {
                EventType::Connected if ok => self.active = Some(ev.id),
                EventType::Disconnected if self.active == Some(ev.id) => self.active = None,
                EventType::ButtonPressed(b, _) if ok => {
                    self.active = Some(ev.id);
                    self.pressed.push(b);
                }
                _ => {}
            }
        }
        let out = match self.active.and_then(|id| g.connected_gamepad(id)) {
            Some(gp) => {
                let raw = self.raw_input(
                    |b| gp.button_data(b).map_or(0.0, |d| d.value()),
                    [gp.value(Axis::LeftStickX), gp.value(Axis::LeftStickY)],
                );
                let any = |bs: &[Button]| bs.iter().any(|b| self.pressed.contains(b));
                let actions = Actions {
                    reset: any(&self.reset),
                    toggle_ball_cam: any(&self.toggle_cam),
                    ..Default::default()
                };
                (raw, actions)
            }
            None => Default::default(),
        };
        self.gilrs = Some(g);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_zeroes_small_values_and_rescales() {
        assert_eq!(deadzone(0.1, 0.15), 0.0);
        assert!((deadzone(1.0, 0.15) - 1.0).abs() < 1e-6);
        assert!(deadzone(-0.575, 0.15) < 0.0);
    }

    #[test]
    fn default_gamepad_buttons_all_parse() {
        let g = Gamepad::default();
        let mut w = vec![];
        for n in [
            &g.accelerate,
            &g.brake,
            &g.jump,
            &g.boost,
            &g.powerslide,
            &g.air_roll,
            &g.air_roll_left,
            &g.air_roll_right,
            &g.reset,
            &g.toggle_ball_cam,
        ] {
            assert!(!buttons(n, "x", &mut w).is_empty());
        }
        assert!(w.is_empty());
    }

    #[test]
    fn playstation_aliases_map_to_gilrs_buttons() {
        for (alias, b) in [
            ("Cross", Button::South),
            ("Circle", Button::East),
            ("Square", Button::West),
            ("Triangle", Button::North),
            ("L1", Button::LeftTrigger),
            ("R1", Button::RightTrigger),
            ("L2", Button::LeftTrigger2),
            ("R2", Button::RightTrigger2),
            ("L3", Button::LeftThumb),
            ("R3", Button::RightThumb),
            ("Share", Button::Select),
            ("Create", Button::Select),
            ("Options", Button::Start),
            ("PS", Button::Mode),
            ("cross", Button::South),
            ("South", Button::South),
        ] {
            assert_eq!(parse_button(alias), Some(b), "{alias}");
        }
        assert_eq!(parse_button("Banane"), None);
    }

    fn pad(cfg: &Gamepad) -> Pad {
        let mut w = vec![];
        let p = Pad {
            gilrs: None,
            active: None,
            accelerate: buttons(&cfg.accelerate, "a", &mut w),
            brake: buttons(&cfg.brake, "b", &mut w),
            jump: buttons(&cfg.jump, "c", &mut w),
            boost: buttons(&cfg.boost, "d", &mut w),
            powerslide: buttons(&cfg.powerslide, "e", &mut w),
            air_roll: buttons(&cfg.air_roll, "f", &mut w),
            roll_left: buttons(&cfg.air_roll_left, "g", &mut w),
            roll_right: buttons(&cfg.air_roll_right, "h", &mut w),
            reset: buttons(&cfg.reset, "i", &mut w),
            toggle_cam: buttons(&cfg.toggle_ball_cam, "j", &mut w),
            cfg: cfg.clone(),
            pressed: Vec::new(),
        };
        assert!(w.is_empty());
        p
    }

    fn held(bs: &[Button]) -> impl Fn(Button) -> f32 + '_ {
        move |b| if bs.contains(&b) { 1.0 } else { 0.0 }
    }

    #[test]
    fn leo_default_layout() {
        let p = pad(&Gamepad::default());
        let r = |bs: &[Button]| p.raw_input(held(bs), [0.0, 0.0]);
        assert_eq!(r(&[Button::RightTrigger2]).throttle, 1.0);
        assert_eq!(r(&[Button::LeftTrigger2]).throttle, -1.0);
        assert!(r(&[Button::South]).jump);
        assert!(r(&[Button::RightTrigger]).boost);
        assert!(!r(&[Button::East]).boost);
        let l1 = r(&[Button::LeftTrigger]);
        assert!(l1.powerslide && l1.air_roll);
        assert_eq!(r(&[Button::West]).roll, -1.0);
        assert_eq!(r(&[Button::LeftThumb]).roll, 1.0);
        assert_eq!(r(&[Button::West, Button::LeftThumb]).roll, 0.0);
        assert_eq!(p.reset, vec![Button::Select]);
        assert_eq!(p.toggle_cam, vec![Button::North]);
    }

    #[test]
    fn shared_button_drives_several_actions() {
        let one = |s: &str| Names::One(s.into());
        let cfg = Gamepad {
            jump: one("L1"),
            powerslide: one("L1"),
            air_roll_left: one("L1"),
            ..Gamepad::default()
        };
        let r = pad(&cfg).raw_input(held(&[Button::LeftTrigger]), [0.0, 0.0]);
        assert!(r.jump && r.powerslide && r.air_roll);
        assert_eq!(r.roll, -1.0);
    }

    #[test]
    fn roll_buttons_give_left_and_right_in_controls() {
        let p = pad(&Gamepad::default());
        let left = p.raw_input(held(&[Button::West]), [0.0, 0.0]).to_controls();
        let right = p
            .raw_input(held(&[Button::LeftThumb]), [0.0, 0.0])
            .to_controls();
        assert_eq!((left.roll, right.roll), (-1.0, 1.0));
        // L1 + stick : roulis libre proportionnel au stick, pas de lacet
        let free = p
            .raw_input(held(&[Button::LeftTrigger]), [1.0, 0.0])
            .to_controls();
        assert_eq!((free.roll, free.yaw), (1.0, 0.0));
    }
}
