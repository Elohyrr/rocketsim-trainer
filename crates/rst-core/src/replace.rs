//! Remplacement atomique d'un fichier exécutable : temporaire dans le même dossier puis `rename`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Remplace `target` par `new_content`, en conservant ses droits (0755 s'il n'existait pas).
/// Sous Linux, remplacer le binaire en cours d'exécution est sûr : l'ancien inode reste ouvert.
pub fn atomic_replace(target: &Path, new_content: &[u8]) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .map_or_else(|| "rst".into(), |n| n.to_string_lossy());
    let tmp: PathBuf = dir.join(format!(".{name}.new.{}", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(new_content)?;
        f.sync_all()?;
        drop(f);
        set_mode(&tmp, target)?;
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn set_mode(tmp: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(target).map_or(0o755, |m| m.permissions().mode() & 0o7777);
    fs::set_permissions(tmp, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_and_keeps_mode_and_leaves_no_temp() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("rst");
        fs::write(&t, b"old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&t, fs::Permissions::from_mode(0o750)).unwrap();
        }
        atomic_replace(&t, b"new").unwrap();
        assert_eq!(fs::read(&t).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&t).unwrap().permissions().mode() & 0o777,
                0o750
            );
        }
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn creates_when_missing_and_cleans_on_error() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("rst");
        atomic_replace(&t, b"x").unwrap();
        assert!(t.exists());
        let bad = d.path().join("nodir").join("rst");
        assert!(atomic_replace(&bad, b"x").is_err());
    }
}
