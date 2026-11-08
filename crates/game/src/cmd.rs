//! Sous-commandes de `rst` autres que `play` : appellent `rst_core` et ajoutent ce qui dépend du jeu
//! (config, meshes, manette).

use std::process::Command;

use game::{config, meshes};
use rst_core::cli::{Cmd, ConfigAction};
use rst_core::commands;
use rst_core::update::{self, Curl, NetError, Status};

const PKG: &str = env!("CARGO_PKG_VERSION");
const COMMIT: &str = env!("RST_COMMIT");
const DATE: &str = env!("RST_BUILD_DATE");

pub fn run(cmd: Cmd) -> i32 {
    match cmd {
        Cmd::Play(_) => unreachable!("géré par main"),
        Cmd::Version => commands::version(PKG, COMMIT, DATE),
        Cmd::Update { check } => match std::env::current_exe() {
            Ok(exe) => commands::update(PKG, check, &Curl, &exe),
            Err(e) => fail(&format!("chemin du binaire introuvable : {e}")),
        },
        Cmd::Install { no_desktop } => match std::env::current_exe() {
            Ok(exe) => commands::install(&exe, no_desktop),
            Err(e) => fail(&format!("chemin du binaire introuvable : {e}")),
        },
        Cmd::Uninstall { purge } => {
            let extra = [
                config::config_path().parent().map(ToOwned::to_owned),
                Some(meshes::data_dir()),
            ];
            commands::uninstall(purge, &extra.into_iter().flatten().collect::<Vec<_>>())
        }
        Cmd::Config { action } => config_cmd(action.unwrap_or(ConfigAction::Show)),
        Cmd::Meshes { force } => meshes_cmd(force),
        Cmd::Doctor => doctor(),
    }
}

fn fail(msg: &str) -> i32 {
    eprintln!("{msg}");
    1
}

fn config_cmd(action: ConfigAction) -> i32 {
    let path = config::config_path();
    match action {
        ConfigAction::Path => {
            println!("{}", path.display());
            0
        }
        ConfigAction::Show => commands::config_show(&path),
        ConfigAction::Edit => commands::config_edit(&path),
        ConfigAction::Reset => match config::reset(&path) {
            Ok(bak) => {
                if let Some(b) = bak {
                    println!("Ancienne config sauvegardée : {}", b.display());
                }
                println!("Config remise aux valeurs par défaut : {}", path.display());
                0
            }
            Err(e) => fail(&format!("remise à zéro impossible : {e}")),
        },
    }
}

fn meshes_cmd(force: bool) -> i32 {
    let dest = meshes::data_dir();
    if !force {
        if let Some(d) = meshes::find_existing() {
            println!("Meshes présents et valides : {}", d.display());
            return 0;
        }
    } else {
        let _ = std::fs::remove_dir_all(dest.join("soccar"));
    }
    println!("Téléchargement des meshes dans {} …", dest.display());
    match meshes::download(&dest) {
        Ok(()) => {
            println!("Meshes téléchargés et vérifiés.");
            0
        }
        Err(e) => fail(&format!("Échec : {e}")),
    }
}

/// Première ligne de la sortie d'une commande qui contient `needle`.
fn probe(prog: &str, args: &[&str], needle: &str) -> Option<String> {
    let out = Command::new(prog).args(args).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.contains(needle))
        .map(|l| l.trim().to_string())
}

fn gpu() -> String {
    let gl = probe("glxinfo", &["-B"], "OpenGL renderer").map(|r| {
        let v = probe("glxinfo", &["-B"], "OpenGL version").unwrap_or_default();
        format!("{r} ; {v}")
    });
    gl.or_else(|| probe("lspci", &[], "VGA"))
        .unwrap_or_else(|| "non détecté (glxinfo / lspci absents ; OpenGL 3.3 requis)".into())
}

fn doctor() -> i32 {
    let mut problems = 0;
    let mut line = |ok: bool, label: &str, detail: String| {
        if !ok {
            problems += 1;
        }
        println!("[{}] {label} : {detail}", if ok { "ok" } else { "!!" });
    };
    line(
        true,
        "version",
        format!("{PKG} (commit {COMMIT}, build {DATE})"),
    );

    match meshes::find_existing() {
        Some(d) => line(
            true,
            "meshes",
            format!("présents et vérifiés ({})", d.display()),
        ),
        None => line(
            false,
            "meshes",
            format!(
                "absents ou invalides ({}) → rst meshes",
                meshes::data_dir().display()
            ),
        ),
    }

    let cfg_path = config::config_path();
    let (cfg_detail, cfg_ok) = match std::fs::read_to_string(&cfg_path) {
        Err(_) => ("absente (créée au premier lancement)".to_string(), true),
        Ok(s) => match toml::from_str::<config::Config>(&s) {
            Ok(c) if c.config_version >= config::CONFIG_VERSION => ("valide".into(), true),
            Ok(_) => (
                "ancienne version (rst config reset pour les nouveaux défauts)".into(),
                true,
            ),
            Err(e) => (format!("illisible : {e}"), false),
        },
    };
    line(
        cfg_ok,
        "config",
        format!("{} — {cfg_detail}", cfg_path.display()),
    );

    match gilrs::Gilrs::new() {
        Err(e) => line(
            false,
            "manette",
            format!("système d'entrées indisponible : {e}"),
        ),
        Ok(g) => {
            let pads: Vec<String> = g
                .gamepads()
                .map(|(_, p)| format!("{} (mapping : {:?})", p.name(), p.mapping_source()))
                .collect();
            if pads.is_empty() {
                line(
                    true,
                    "manette",
                    "aucune détectée (clavier/souris utilisables)".into(),
                );
            } else {
                line(true, "manette", pads.join(" ; "));
            }
        }
    }

    println!("[..] GPU : {}", gpu());

    match update::fetch_latest(&Curl) {
        Ok(r) => match update::status(PKG, &r) {
            Ok(Status::UpToDate) => line(true, "mise à jour", "à jour".into()),
            Ok(Status::Newer) => line(
                false,
                "mise à jour",
                format!("{} disponible → rst update", r.tag_name),
            ),
            Err(e) => line(false, "mise à jour", e),
        },
        Err(NetError::Offline(_)) => println!("[..] mise à jour : non vérifiée (hors ligne)"),
        Err(e) => line(false, "mise à jour", e.to_string()),
    }

    i32::from(problems > 0)
}
