//! Configuration des touches : fichier TOML (`~/.config/rocketsim-trainer/config.toml`), créé avec les
//! valeurs par défaut au premier lancement. Chaque action accepte un nom ou une liste de noms.

use std::path::{Path, PathBuf};

use macroquad::input::{KeyCode, MouseButton};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Names {
    One(String),
    Many(Vec<String>),
}

impl Names {
    fn list(&self) -> Vec<&str> {
        match self {
            Names::One(s) => vec![s.as_str()],
            Names::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

fn n(names: &[&str]) -> Names {
    Names::Many(names.iter().map(|s| s.to_string()).collect())
}

/// Clavier et souris. Noms : lettres `A`…`Z`, chiffres `0`…`9`, `Space`, `Enter`, `Tab`, `Escape`,
/// `LeftShift`, `LeftControl`, `Up`/`Down`/`Left`/`Right`, `F1`…`F12`, `LeftBracket`, `RightBracket`,
/// `MouseLeft`, `MouseRight`, `MouseMiddle`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    pub accelerate: Names,
    pub brake: Names,
    pub steer_left: Names,
    pub steer_right: Names,
    pub jump: Names,
    pub boost: Names,
    pub powerslide: Names,
    /// Maintenu : gauche/droite deviennent un roulis en l'air.
    pub air_roll: Names,
    pub air_roll_left: Names,
    pub air_roll_right: Names,
    pub reset: Names,
    pub toggle_ball_cam: Names,
    pub slower: Names,
    pub faster: Names,
    pub toggle_drill: Names,
    pub quit: Names,
    /// Vrai : la souris pilote tangage et lacet en l'air (curseur capturé).
    pub mouse_air_control: bool,
    pub mouse_sensitivity: f32,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            accelerate: n(&["W", "Up"]),
            brake: n(&["S", "Down"]),
            steer_left: n(&["A", "Left"]),
            steer_right: n(&["D", "Right"]),
            jump: n(&["Space"]),
            boost: n(&["MouseLeft", "LeftControl"]),
            powerslide: n(&["LeftShift", "MouseRight"]),
            air_roll: n(&["LeftShift", "MouseRight"]),
            air_roll_left: n(&["Q"]),
            air_roll_right: n(&["E"]),
            reset: n(&["R"]),
            toggle_ball_cam: n(&["C", "Tab"]),
            slower: n(&["LeftBracket"]),
            faster: n(&["RightBracket"]),
            toggle_drill: n(&["F2"]),
            quit: n(&["Escape"]),
            mouse_air_control: false,
            mouse_sensitivity: 0.05,
        }
    }
}

/// Manette (noms gilrs) : `South` (A), `East` (B), `West` (X), `North` (Y), `LeftTrigger` (LB),
/// `RightTrigger` (RB), `LeftTrigger2` (LT), `RightTrigger2` (RT), `Select`, `Start`, `LeftThumb`,
/// `RightThumb`, `DPadUp`…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gamepad {
    pub deadzone: f32,
    pub accelerate: Names,
    pub brake: Names,
    pub jump: Names,
    pub boost: Names,
    pub powerslide: Names,
    pub air_roll: Names,
    pub reset: Names,
    pub toggle_ball_cam: Names,
}

impl Default for Gamepad {
    fn default() -> Self {
        Self {
            deadzone: 0.15,
            accelerate: n(&["RightTrigger2"]),
            brake: n(&["LeftTrigger2"]),
            jump: n(&["South"]),
            boost: n(&["East"]),
            powerslide: n(&["West"]),
            air_roll: n(&["LeftTrigger"]),
            reset: n(&["Select"]),
            toggle_ball_cam: n(&["North"]),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub keyboard: Keyboard,
    pub gamepad: Gamepad,
}

/// Une touche ou un bouton de souris.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bind {
    Key(KeyCode),
    Mouse(MouseButton),
}

macro_rules! key_table {
    ($name:expr, $($k:ident),+ $(,)?) => {
        match $name { $(stringify!($k) => Some(KeyCode::$k),)+ _ => None }
    };
}

/// Nom → touche (insensible à la casse pour les lettres seules).
pub fn parse_bind(name: &str) -> Option<Bind> {
    match name {
        "MouseLeft" => return Some(Bind::Mouse(MouseButton::Left)),
        "MouseRight" => return Some(Bind::Mouse(MouseButton::Right)),
        "MouseMiddle" => return Some(Bind::Mouse(MouseButton::Middle)),
        _ => {}
    }
    let up = if name.len() == 1 {
        name.to_uppercase()
    } else {
        name.to_string()
    };
    let up = match up.as_str() {
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => format!("Key{d}"),
        other => other.to_string(),
    };
    key_table!(
        up.as_str(),
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
        I,
        J,
        K,
        L,
        M,
        N,
        O,
        P,
        Q,
        R,
        S,
        T,
        U,
        V,
        W,
        X,
        Y,
        Z,
        Key0,
        Key1,
        Key2,
        Key3,
        Key4,
        Key5,
        Key6,
        Key7,
        Key8,
        Key9,
        Space,
        Enter,
        Tab,
        Escape,
        Backspace,
        LeftShift,
        RightShift,
        LeftControl,
        RightControl,
        LeftAlt,
        RightAlt,
        Up,
        Down,
        Left,
        Right,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        LeftBracket,
        RightBracket,
        Comma,
        Period,
        Minus,
        Equal
    )
    .map(Bind::Key)
}

/// Résout une liste de noms ; les noms inconnus sont rapportés dans `warnings`.
pub fn resolve(names: &Names, what: &str, warnings: &mut Vec<String>) -> Vec<Bind> {
    names
        .list()
        .into_iter()
        .filter_map(|s| {
            let b = parse_bind(s);
            if b.is_none() {
                warnings.push(format!("touche inconnue « {s} » pour {what} (ignorée)"));
            }
            b
        })
        .collect()
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rocketsim-trainer").join("config.toml")
}

const HEADER: &str = "# rocketsim-trainer : touches et manette. Supprimer ce fichier pour revenir aux valeurs par défaut.\n\
# Chaque action prend un nom ou une liste de noms (voir docs dans le README).\n\n";

/// Charge la config ; la crée avec les valeurs par défaut si absente. En cas d'erreur de lecture,
/// retourne les valeurs par défaut et un message.
pub fn load_or_create(path: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(s) => match toml::from_str::<Config>(&s) {
            Ok(c) => (c, None),
            Err(e) => (
                Config::default(),
                Some(format!(
                    "{} illisible, valeurs par défaut : {e}",
                    path.display()
                )),
            ),
        },
        Err(_) => {
            let c = Config::default();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(body) = toml::to_string_pretty(&c) {
                let _ = std::fs::write(path, format!("{HEADER}{body}"));
            }
            (c, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips_and_all_names_resolve() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
        let mut w = vec![];
        let k = &c.keyboard;
        for (what, names) in [
            ("a", &k.accelerate),
            ("b", &k.brake),
            ("c", &k.steer_left),
            ("d", &k.steer_right),
            ("e", &k.jump),
            ("f", &k.boost),
            ("g", &k.powerslide),
            ("h", &k.air_roll),
            ("i", &k.air_roll_left),
            ("j", &k.air_roll_right),
            ("k", &k.reset),
            ("l", &k.toggle_ball_cam),
            ("m", &k.slower),
            ("n", &k.faster),
            ("o", &k.toggle_drill),
            ("p", &k.quit),
        ] {
            assert!(!resolve(names, what, &mut w).is_empty());
        }
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn partial_file_keeps_defaults_and_accepts_single_name() {
        let c: Config = toml::from_str("[keyboard]\njump = \"J\"\n").unwrap();
        assert_eq!(c.keyboard.jump, Names::One("J".into()));
        assert_eq!(c.keyboard.reset, Keyboard::default().reset);
    }

    #[test]
    fn unknown_key_is_reported() {
        let mut w = vec![];
        assert!(resolve(&Names::One("Banane".into()), "saut", &mut w).is_empty());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn digits_and_lowercase_letters_parse() {
        assert_eq!(parse_bind("w"), Some(Bind::Key(KeyCode::W)));
        assert_eq!(parse_bind("1"), Some(Bind::Key(KeyCode::Key1)));
    }
}
