//! Manette via gilrs : échantillonnage de l'état et des fronts montants.

use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};

use crate::config::{Gamepad, Names};
use crate::input::{Actions, RawInput};

pub fn parse_button(name: &str) -> Option<Button> {
    Some(match name {
        "South" => Button::South,
        "East" => Button::East,
        "North" => Button::North,
        "West" => Button::West,
        "LeftTrigger" => Button::LeftTrigger,
        "LeftTrigger2" => Button::LeftTrigger2,
        "RightTrigger" => Button::RightTrigger,
        "RightTrigger2" => Button::RightTrigger2,
        "Select" => Button::Select,
        "Start" => Button::Start,
        "Mode" => Button::Mode,
        "LeftThumb" => Button::LeftThumb,
        "RightThumb" => Button::RightThumb,
        "DPadUp" => Button::DPadUp,
        "DPadDown" => Button::DPadDown,
        "DPadLeft" => Button::DPadLeft,
        "DPadRight" => Button::DPadRight,
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
            .and_then(|g| g.gamepads().next().map(|(id, _)| id));
        Self {
            gilrs,
            active,
            accelerate: buttons(&cfg.accelerate, "accelerate", warnings),
            brake: buttons(&cfg.brake, "brake", warnings),
            jump: buttons(&cfg.jump, "jump", warnings),
            boost: buttons(&cfg.boost, "boost", warnings),
            powerslide: buttons(&cfg.powerslide, "powerslide", warnings),
            air_roll: buttons(&cfg.air_roll, "air_roll", warnings),
            reset: buttons(&cfg.reset, "reset", warnings),
            toggle_cam: buttons(&cfg.toggle_ball_cam, "toggle_ball_cam", warnings),
            cfg: cfg.clone(),
            pressed: Vec::new(),
        }
    }

    pub fn connected(&self) -> bool {
        self.active.is_some()
    }

    /// À appeler une fois par frame : vide les évènements et retourne l'état de la manette active.
    pub fn sample(&mut self) -> (RawInput, Actions) {
        self.pressed.clear();
        let Some(g) = self.gilrs.as_mut() else {
            return Default::default();
        };
        while let Some(ev) = g.next_event() {
            match ev.event {
                EventType::Connected => self.active = Some(ev.id),
                EventType::Disconnected if self.active == Some(ev.id) => self.active = None,
                EventType::ButtonPressed(b, _) => {
                    self.active = Some(ev.id);
                    self.pressed.push(b);
                }
                _ => {}
            }
        }
        let Some(gp) = self.active.and_then(|id| g.connected_gamepad(id)) else {
            return Default::default();
        };
        let value = |bs: &[Button]| {
            bs.iter()
                .map(|b| gp.button_data(*b).map_or(0.0, |d| d.value()))
                .fold(0.0_f32, f32::max)
        };
        let dz = self.cfg.deadzone.clamp(0.0, 0.9);
        let raw = RawInput {
            throttle: value(&self.accelerate) - value(&self.brake),
            steer: deadzone(gp.value(Axis::LeftStickX), dz),
            stick_y: deadzone(gp.value(Axis::LeftStickY), dz),
            mouse: [0.0, 0.0],
            jump: value(&self.jump) > 0.5,
            boost: value(&self.boost) > 0.5,
            powerslide: value(&self.powerslide) > 0.5,
            air_roll: value(&self.air_roll) > 0.5,
            roll: 0.0,
        };
        let any = |bs: &[Button]| bs.iter().any(|b| self.pressed.contains(b));
        let actions = Actions {
            reset: any(&self.reset),
            toggle_ball_cam: any(&self.toggle_cam),
            ..Default::default()
        };
        (raw, actions)
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
            &g.reset,
            &g.toggle_ball_cam,
        ] {
            assert!(!buttons(n, "x", &mut w).is_empty());
        }
        assert!(w.is_empty());
    }
}
