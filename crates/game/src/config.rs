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

/// Manette. Noms PlayStation : `Cross`, `Circle`, `Square`, `Triangle`, `L1`, `R1`, `L2`, `R2`, `L3`,
/// `R3`, `Share` (ou `Create`), `Options`, `PS`. Les noms gilrs restent acceptés : `South` (Cross),
/// `East` (Circle), `West` (Square), `North` (Triangle), `LeftTrigger` (L1), `RightTrigger` (R1),
/// `LeftTrigger2` (L2), `RightTrigger2` (R2), `LeftThumb` (L3), `RightThumb` (R3), `Select` (Share),
/// `Start` (Options), `Mode` (PS), `DPadUp`… Un même bouton peut servir à plusieurs actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gamepad {
    pub deadzone: f32,
    pub accelerate: Names,
    pub brake: Names,
    pub jump: Names,
    pub boost: Names,
    pub powerslide: Names,
    /// Maintenu : le stick gauche devient un roulis libre en l'air.
    pub air_roll: Names,
    pub air_roll_left: Names,
    pub air_roll_right: Names,
    pub reset: Names,
    pub toggle_ball_cam: Names,
}

/// Défauts manette = layout d'Elohyrr (DualShock 4 / DualSense) : R2 accélère, L2 freine/recule,
/// Cross saute, R1 boost, L1 powerslide et roulis libre (même bouton), Square roulis droit,
/// L3 (clic du stick gauche) roulis gauche, Triangle caméra balle, Share remet en jeu.
impl Default for Gamepad {
    fn default() -> Self {
        Self {
            deadzone: 0.15,
            accelerate: n(&["R2"]),
            brake: n(&["L2"]),
            jump: n(&["Cross"]),
            boost: n(&["R1"]),
            powerslide: n(&["L1"]),
            air_roll: n(&["L1"]),
            air_roll_left: n(&["L3"]),
            air_roll_right: n(&["Square"]),
            reset: n(&["Share"]),
            toggle_ball_cam: n(&["Triangle"]),
        }
    }
}

/// Version du format et des défauts. Un fichier sans `config_version` (ou plus ancien) a été écrit
/// avec les défauts d'avant : voir [`migrate`].
pub const CONFIG_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub config_version: u32,
    pub keyboard: Keyboard,
    pub gamepad: Gamepad,
    pub camera: CameraCfg,
}

/// Réglages caméra à la Rocket League (défauts « pro » typiques). Distances en uu, angles en degrés.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraCfg {
    /// Champ de vision vertical.
    pub fov: f32,
    /// Distance derrière la voiture.
    pub distance: f32,
    /// Hauteur de la caméra au-dessus de la voiture.
    pub height: f32,
    /// Inclinaison de la caméra (négatif = vers le bas).
    pub angle: f32,
    /// Raideur : à 0 la caméra traîne beaucoup derrière la voiture, à 1 elle la suit rigidement.
    pub stiffness: f32,
    /// Vitesse de rotation de la caméra en lacet (ball cam et caméra voiture).
    pub swivel_speed: f32,
    /// Vitesse de transition sol ↔ air ↔ mur (orientation « haut » de la caméra).
    pub transition_speed: f32,
}

impl Default for CameraCfg {
    fn default() -> Self {
        Self {
            fov: 110.0,
            distance: 270.0,
            height: 100.0,
            angle: -4.0,
            stiffness: 0.45,
            swivel_speed: 5.0,
            transition_speed: 1.2,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            keyboard: Keyboard::default(),
            gamepad: Gamepad::default(),
            camera: CameraCfg::default(),
        }
    }
}

/// Section manette telle que les versions < 2 l'écrivaient au premier lancement.
fn gamepad_v1() -> Gamepad {
    Gamepad {
        accelerate: n(&["RightTrigger2"]),
        brake: n(&["LeftTrigger2"]),
        jump: n(&["South"]),
        boost: n(&["East"]),
        powerslide: n(&["West"]),
        air_roll: n(&["LeftTrigger"]),
        reset: n(&["Select"]),
        toggle_ball_cam: n(&["North"]),
        // absents du fichier v1 : ils prennent leur défaut à la lecture
        ..Gamepad::default()
    }
}

/// Section manette telle que la v2 l'écrivait au premier lancement (roulis gauche/droite inversés
/// depuis la v3).
fn gamepad_v2() -> Gamepad {
    Gamepad {
        air_roll_left: n(&["Square"]),
        air_roll_right: n(&["L3"]),
        ..Gamepad::default()
    }
}

/// Issue de [`migrate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// Fichier à jour.
    None,
    /// Section manette jamais modifiée : remplacée par les nouveaux défauts (à réécrire).
    Updated,
    /// Section manette personnalisée sur un fichier ancien : conservée telle quelle.
    KeptCustom,
}

/// Met à niveau une config lue depuis un fichier de version `file_version` (0 = absente).
/// Un fichier généré automatiquement et non modifié reçoit les nouveaux défauts ; sinon les
/// réglages de l'utilisateur sont gardés.
pub fn migrate(mut c: Config, file_version: u32) -> (Config, Migration) {
    if file_version >= CONFIG_VERSION {
        return (c, Migration::None);
    }
    c.config_version = CONFIG_VERSION;
    if c.gamepad == gamepad_v1() || c.gamepad == gamepad_v2() {
        c.gamepad = Gamepad::default();
        (c, Migration::Updated)
    } else {
        (c, Migration::KeptCustom)
    }
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

fn write_config(path: &Path, c: &Config) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = toml::to_string_pretty(c) {
        let _ = std::fs::write(path, format!("{HEADER}{body}"));
    }
}

/// Charge la config ; la crée avec les valeurs par défaut si absente. Un fichier d'une version
/// plus ancienne est mis à niveau (sauvegarde en `config.toml.bak`) si la manette n'a jamais été
/// personnalisée ; sinon il est gardé et un avertissement est retourné. En cas d'erreur de lecture,
/// retourne les valeurs par défaut et un message.
pub fn load_or_create(path: &Path) -> (Config, Option<String>) {
    let Ok(s) = std::fs::read_to_string(path) else {
        let c = Config::default();
        write_config(path, &c);
        return (c, None);
    };
    let parsed = toml::from_str::<Config>(&s).and_then(|c| {
        let v = toml::from_str::<toml::Table>(&s)?
            .get("config_version")
            .and_then(toml::Value::as_integer)
            .map_or(0, |v| u32::try_from(v).unwrap_or(0));
        Ok((c, v))
    });
    let (c, version) = match parsed {
        Ok(x) => x,
        Err(e) => {
            return (
                Config::default(),
                Some(format!(
                    "{} illisible, valeurs par défaut : {e}",
                    path.display()
                )),
            );
        }
    };
    let (c, outcome) = migrate(c, version);
    let note = match outcome {
        Migration::None => None,
        Migration::Updated => {
            let _ = std::fs::copy(path, path.with_extension("toml.bak"));
            write_config(path, &c);
            eprintln!(
                "config : {} mis à jour avec les nouveaux défauts manette (ancienne copie : config.toml.bak)",
                path.display()
            );
            None
        }
        Migration::KeptCustom => {
            // Sauvegarde puis réécriture : ajoute la section [camera] sans toucher aux réglages.
            let _ = std::fs::copy(path, path.with_extension("toml.bak"));
            write_config(path, &c);
            Some(format!(
                "{} est ancien et la manette y est personnalisée : réglages gardés (copie : config.toml.bak), nouveaux défauts manette (roulis, etc.) non appliqués ; supprimez la section [gamepad] pour les obtenir",
                path.display()
            ))
        }
    };
    (c, note)
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

    #[test]
    fn leo_gamepad_defaults() {
        let g = Gamepad::default();
        let one = |s: &str| n(&[s]);
        assert_eq!(g.accelerate, one("R2"));
        assert_eq!(g.brake, one("L2"));
        assert_eq!(g.jump, one("Cross"));
        assert_eq!(g.boost, one("R1"));
        assert_eq!(g.powerslide, one("L1"));
        assert_eq!(g.air_roll, one("L1"));
        assert_eq!(g.air_roll_left, one("L3"));
        assert_eq!(g.air_roll_right, one("Square"));
        assert_eq!(g.toggle_ball_cam, one("Triangle"));
        assert_eq!(g.reset, one("Share"));
    }

    #[test]
    fn unmodified_v1_file_is_upgraded() {
        // Section manette telle que l'ancienne version l'écrivait (sans les champs de roulis).
        let text = V1_GAMEPAD;
        let parsed: Config = toml::from_str(text).unwrap();
        assert_eq!(parsed.gamepad, gamepad_v1());
        let (c, m) = migrate(parsed, 0);
        assert_eq!(m, Migration::Updated);
        assert_eq!(c, Config::default());
    }

    #[test]
    fn customized_v1_gamepad_is_kept() {
        let c = Config {
            gamepad: Gamepad {
                jump: n(&["East"]),
                ..gamepad_v1()
            },
            ..Config::default()
        };
        let (k, m) = migrate(c, 1);
        assert_eq!(m, Migration::KeptCustom);
        assert_eq!(k.gamepad.jump, n(&["East"]));
        assert_eq!(k.config_version, CONFIG_VERSION);
    }

    #[test]
    fn current_version_is_untouched() {
        let c = Config {
            gamepad: Gamepad {
                jump: n(&["Circle"]),
                ..Gamepad::default()
            },
            ..Config::default()
        };
        let (k, m) = migrate(c.clone(), CONFIG_VERSION);
        assert_eq!((k, m), (c, Migration::None));
    }

    #[test]
    fn unmodified_v2_gamepad_is_upgraded_and_custom_one_kept() {
        let text = toml::to_string_pretty(&Config {
            config_version: 2,
            gamepad: gamepad_v2(),
            ..Config::default()
        })
        .unwrap();
        // fichier v2 sans section [camera]
        let text = text.split("[camera]").next().unwrap().to_string();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.camera, CameraCfg::default());
        let (c, m) = migrate(parsed.clone(), 2);
        assert_eq!(m, Migration::Updated);
        assert_eq!(c.gamepad.air_roll_right, n(&["Square"]));
        assert_eq!(c.config_version, 3);
        let mut custom = parsed;
        custom.gamepad.jump = n(&["Circle"]);
        let (k, m) = migrate(custom, 2);
        assert_eq!(m, Migration::KeptCustom);
        assert_eq!(k.gamepad.air_roll_left, n(&["Square"]));
    }

    #[test]
    fn camera_section_is_optional_and_partial() {
        let c: Config = toml::from_str("[camera]\nfov = 90.0\n").unwrap();
        assert_eq!(c.camera.fov, 90.0);
        assert_eq!(c.camera.distance, 270.0);
    }

    const V1_GAMEPAD: &str = "[gamepad]\ndeadzone = 0.15\naccelerate = [\"RightTrigger2\"]\nbrake = [\"LeftTrigger2\"]\njump = [\"South\"]\nboost = [\"East\"]\npowerslide = [\"West\"]\nair_roll = [\"LeftTrigger\"]\nreset = [\"Select\"]\ntoggle_ball_cam = [\"North\"]\n";

    #[test]
    fn file_upgrade_writes_backup_then_is_stable() {
        let dir = std::env::temp_dir().join(format!("rst-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, V1_GAMEPAD).unwrap();
        let (c, warn) = load_or_create(&path);
        assert!(warn.is_none());
        assert_eq!(c, Config::default());
        assert!(dir.join("config.toml.bak").exists());
        let (c2, warn2) = load_or_create(&path);
        assert!(warn2.is_none());
        assert_eq!(c2, Config::default());
        // fichier ancien personnalisé : gardé, avec avertissement
        std::fs::write(&path, V1_GAMEPAD.replace("South", "East")).unwrap();
        let (c3, warn3) = load_or_create(&path);
        assert!(warn3.is_some());
        assert_eq!(c3.gamepad.jump, Names::Many(vec!["East".into()]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
