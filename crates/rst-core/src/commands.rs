//! Sous-commandes sans dépendance au jeu : update, install, uninstall, version, config (chemin, show, edit).
//! Chaque fonction affiche en français et retourne le code de sortie.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::install::{self, Dirs};
use crate::update::{self, Http, NetError, Status};

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Copie `path` en `path.bak` (si elle existe) ; retourne la copie.
pub fn backup(path: &Path) -> std::io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    let bak = path.with_file_name(name);
    std::fs::copy(path, &bak)?;
    Ok(Some(bak))
}

/// Éditeur : `$VISUAL`, `$EDITOR`, sinon `nano`/`vi`.
pub fn editor(visual: Option<String>, editor: Option<String>) -> String {
    visual
        .filter(|s| !s.trim().is_empty())
        .or(editor.filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| {
            if which("nano") {
                "nano".into()
            } else {
                "vi".into()
            }
        })
}

fn which(prog: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

pub fn config_show(path: &Path) -> i32 {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            println!("# {}\n{s}", path.display());
            0
        }
        Err(_) => {
            eprintln!(
                "Pas encore de config ({}). Lancez le jeu une fois, ou : rst config reset",
                path.display()
            );
            1
        }
    }
}

pub fn config_edit(path: &Path) -> i32 {
    if !path.exists() {
        eprintln!(
            "Pas encore de config ({}). Lancez le jeu une fois, ou : rst config reset",
            path.display()
        );
        return 1;
    }
    let ed = editor(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok());
    // $EDITOR peut contenir des arguments (« code -w »).
    let mut parts = ed.split_whitespace();
    let status = Command::new(parts.next().unwrap_or("vi"))
        .args(parts)
        .arg(path)
        .status();
    match status {
        Ok(s) if s.success() => 0,
        Ok(_) => 1,
        Err(e) => {
            eprintln!("Impossible de lancer l'éditeur « {ed} » : {e} (définissez $EDITOR)");
            1
        }
    }
}

pub fn version(pkg: &str, commit: &str, date: &str) -> i32 {
    println!("rst {pkg} (commit {commit}, build {date})");
    0
}

/// `rst update [--check]`.
pub fn update(current: &str, check: bool, http: &dyn Http, exe: &Path) -> i32 {
    let release = match update::fetch_latest(http) {
        Ok(r) => r,
        Err(NetError::Offline(_)) => {
            eprintln!(
                "Hors ligne : impossible de vérifier les mises à jour (version actuelle {current})."
            );
            return 1;
        }
        Err(e) => {
            eprintln!("Vérification impossible : {e}");
            return 1;
        }
    };
    let latest = release.tag_name.trim_start_matches('v').to_string();
    match update::status(current, &release) {
        Err(e) => {
            eprintln!("{e}");
            1
        }
        Ok(Status::UpToDate) => {
            println!("Déjà à jour : {current}");
            0
        }
        Ok(Status::Newer) if check => {
            println!("Mise à jour disponible : {current} → {latest}\nLancez : rst update");
            0
        }
        Ok(Status::Newer) => match update::apply(http, &release, exe) {
            Ok(()) => {
                println!("Mis à jour : {current} → {latest}  ({})", exe.display());
                let notes = release.short_notes(8);
                if !notes.is_empty() {
                    println!("\nNotes de release :\n{notes}");
                }
                0
            }
            Err(e) => {
                eprintln!("Échec de la mise à jour : {e}");
                1
            }
        },
    }
}

/// `rst install`.
pub fn install(exe: &Path, no_desktop: bool) -> i32 {
    let Some(h) = home() else {
        eprintln!("$HOME n'est pas défini");
        return 1;
    };
    let path_var = std::env::var("PATH").unwrap_or_default();
    let d = Dirs::new(h);
    match install::install(exe, &d, &path_var, !no_desktop) {
        Ok(r) => {
            println!("Installé : {}", r.bin.display());
            if r.desktop {
                println!("Entrée de menu : {}", d.desktop().display());
            }
            if !r.in_path {
                println!(
                    "\n{} n'est pas dans votre PATH. Ajoutez à ~/.bashrc :\n  export PATH=\"$HOME/.local/bin:$PATH\"\npuis rouvrez le terminal.",
                    d.bin_dir().display()
                );
            }
            0
        }
        Err(e) => {
            eprintln!("Installation impossible : {e}");
            1
        }
    }
}

/// `rst uninstall [--purge]` ; `extra` = dossiers config et meshes supprimés avec `--purge`.
pub fn uninstall(purge: bool, extra: &[PathBuf]) -> i32 {
    let Some(h) = home() else {
        eprintln!("$HOME n'est pas défini");
        return 1;
    };
    match install::uninstall(&Dirs::new(h), purge, extra) {
        Ok(gone) if gone.is_empty() => {
            println!("Rien à désinstaller.");
            0
        }
        Ok(gone) => {
            for p in gone {
                println!("Supprimé : {}", p.display());
            }
            if !purge {
                println!("Config et meshes conservés (rst uninstall --purge pour tout retirer).");
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake(HashMap<String, Result<Vec<u8>, NetError>>);
    impl Http for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
            self.0
                .get(url)
                .cloned()
                .unwrap_or(Err(NetError::Other("404".into())))
        }
    }

    #[test]
    fn backup_copies() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        assert_eq!(backup(&p).unwrap(), None);
        std::fs::write(&p, "a").unwrap();
        let b = backup(&p).unwrap().unwrap();
        assert_eq!(b.file_name().unwrap(), "config.toml.bak");
        assert_eq!(std::fs::read_to_string(b).unwrap(), "a");
    }

    #[test]
    fn editor_choice() {
        assert_eq!(
            editor(Some("code -w".into()), Some("vim".into())),
            "code -w"
        );
        assert_eq!(editor(None, Some("vim".into())), "vim");
        assert_eq!(editor(Some(" ".into()), Some("vim".into())), "vim");
    }

    #[test]
    fn update_flow_exit_codes() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("rst");
        std::fs::write(&exe, b"old").unwrap();
        let offline = Fake(HashMap::from([(
            update::latest_url(),
            Err(NetError::Offline("x".into())),
        )]));
        assert_eq!(update("0.1.2", false, &offline, &exe), 1);

        let bin = b"\x7fELFnew".to_vec();
        let sums = format!("{}  rst-linux-x86_64\n", crate::hash::sha256_hex(&bin));
        let json = r#"{"tag_name":"v0.2.0","body":"notes","assets":[
            {"name":"rst-linux-x86_64","browser_download_url":"u/bin"},
            {"name":"SHA256SUMS","browser_download_url":"u/sums"}]}"#;
        let ok = Fake(HashMap::from([
            (update::latest_url(), Ok(json.as_bytes().to_vec())),
            ("u/bin".into(), Ok(bin.clone())),
            ("u/sums".into(), Ok(sums.into_bytes())),
        ]));
        assert_eq!(update("0.2.0", false, &ok, &exe), 0);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert_eq!(update("0.1.2", true, &ok, &exe), 0);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert_eq!(update("0.1.2", false, &ok, &exe), 0);
        assert_eq!(std::fs::read(&exe).unwrap(), bin);
    }
}
