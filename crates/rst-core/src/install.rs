//! Installation de `rst` dans `~/.local/bin`, entrée de menu `.desktop`, désinstallation.

use std::fs;
use std::path::{Path, PathBuf};

use crate::replace;

const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="12" fill="#0e121e"/><circle cx="32" cy="32" r="14" fill="#ff9d1e"/><path d="M8 48h48" stroke="#3a8dff" stroke-width="4" stroke-linecap="round"/></svg>
"##;

/// Emplacements, tous dérivés d'un `HOME` (testable avec un dossier temporaire).
#[derive(Debug, Clone)]
pub struct Dirs {
    pub home: PathBuf,
}

impl Dirs {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.home.join(".local/bin")
    }
    pub fn bin(&self) -> PathBuf {
        self.bin_dir().join("rst")
    }
    pub fn desktop(&self) -> PathBuf {
        self.home.join(".local/share/applications/rst.desktop")
    }
    pub fn icon(&self) -> PathBuf {
        self.home
            .join(".local/share/icons/hicolor/scalable/apps/rst.svg")
    }
}

pub fn desktop_entry(bin: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Rocket Sim Trainer\nComment=Entraînement aux mécaniques de jeu de voiture-football\nExec={} play\nIcon=rst\nTerminal=false\nCategories=Game;\n",
        bin.display()
    )
}

/// `dir` est-il dans la chaîne `PATH` ?
pub fn in_path(dir: &Path, path_var: &str) -> bool {
    std::env::split_paths(path_var).any(|p| p == dir)
}

#[derive(Debug)]
pub struct InstallReport {
    pub bin: PathBuf,
    pub in_path: bool,
    pub desktop: bool,
}

/// Copie `exe` vers `<home>/.local/bin/rst` (atomique) et crée l'entrée de menu si `desktop`.
pub fn install(
    exe: &Path,
    d: &Dirs,
    path_var: &str,
    desktop: bool,
) -> Result<InstallReport, String> {
    let io = |what: &str, e: std::io::Error| format!("{what} : {e}");
    let dir = d.bin_dir();
    fs::create_dir_all(&dir).map_err(|e| io(&format!("création de {}", dir.display()), e))?;
    let data = fs::read(exe).map_err(|e| io(&format!("lecture de {}", exe.display()), e))?;
    if fs::canonicalize(exe).ok() != fs::canonicalize(d.bin()).ok() {
        replace::atomic_replace(&d.bin(), &data).map_err(|e| io("installation", e))?;
    }
    if desktop {
        for (p, body) in [
            (d.icon(), ICON_SVG.to_string()),
            (d.desktop(), desktop_entry(&d.bin())),
        ] {
            fs::create_dir_all(p.parent().unwrap())
                .map_err(|e| io("création du dossier de menu", e))?;
            fs::write(&p, body).map_err(|e| io(&format!("écriture de {}", p.display()), e))?;
        }
    }
    Ok(InstallReport {
        bin: d.bin(),
        in_path: in_path(&dir, path_var),
        desktop,
    })
}

/// Retire binaire, `.desktop` et icône ; avec `purge`, supprime aussi les dossiers `extra` (config, meshes).
/// Retourne les chemins réellement supprimés.
pub fn uninstall(d: &Dirs, purge: bool, extra: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut gone = vec![];
    let mut files = vec![d.bin(), d.desktop(), d.icon()];
    if purge {
        files.extend(extra.iter().cloned());
    }
    for p in files {
        let r = if p.is_dir() {
            fs::remove_dir_all(&p)
        } else {
            fs::remove_file(&p)
        };
        match r {
            Ok(()) => gone.push(p),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("suppression de {} : {e}", p.display())),
        }
    }
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_exe(dir: &Path) -> PathBuf {
        let p = dir.join("game");
        fs::write(&p, b"\x7fELFx").unwrap();
        p
    }

    #[test]
    fn install_then_uninstall_in_temp_home() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let d = Dirs::new(&home);
        let exe = fake_exe(t.path());
        let r = install(&exe, &d, "/usr/bin", true).unwrap();
        assert_eq!(fs::read(&r.bin).unwrap(), b"\x7fELFx");
        assert!(!r.in_path && r.desktop);
        let entry = fs::read_to_string(d.desktop()).unwrap();
        assert!(entry.contains(&format!("Exec={} play", d.bin().display())));
        assert!(d.icon().exists());

        let cfg = home.join(".config/rocketsim-trainer");
        fs::create_dir_all(&cfg).unwrap();
        let gone = uninstall(&d, false, std::slice::from_ref(&cfg)).unwrap();
        assert_eq!(gone.len(), 3);
        assert!(cfg.exists() && !d.bin().exists() && !d.desktop().exists());
        assert!(uninstall(&d, false, &[]).unwrap().is_empty());
        uninstall(&d, true, std::slice::from_ref(&cfg)).unwrap();
        assert!(!cfg.exists());
    }

    #[test]
    fn install_without_desktop_and_path_detection() {
        let t = tempfile::tempdir().unwrap();
        let d = Dirs::new(t.path().join("h"));
        let exe = fake_exe(t.path());
        let path = format!("/usr/bin:{}", d.bin_dir().display());
        let r = install(&exe, &d, &path, false).unwrap();
        assert!(r.in_path && !d.desktop().exists());
        // réinstallation depuis le binaire installé : pas d'erreur
        install(&d.bin(), &d, &path, false).unwrap();
        assert_eq!(fs::read(d.bin()).unwrap(), b"\x7fELFx");
    }
}
