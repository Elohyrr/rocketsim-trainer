//! Récupération automatique des meshes soccar au premier lancement.
//!
//! Réutilise `scripts/fetch-meshes.sh` (même source PyPI, mêmes sha256) : le script et la liste
//! d'empreintes sont embarqués dans le binaire et exécutés avec `bash` (+ `curl`, `tar`, `sha256sum`).

use std::path::{Path, PathBuf};
use std::process::Command;

use sim_core::{MESHES_ENV, check_meshes_dir};

const SCRIPT: &str = include_str!("../../../scripts/fetch-meshes.sh");
const SUMS: &str = include_str!("../../../scripts/meshes.sha256");

/// `~/.local/share/rocketsim-trainer` (ou `$XDG_DATA_HOME/rocketsim-trainer`).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("rocketsim-trainer")
}

/// Dossier de meshes déjà utilisable : `$ROCKETSIM_MESHES_DIR`, `./collision_meshes`, puis le dossier de données.
pub fn find_existing() -> Option<PathBuf> {
    let env = std::env::var_os(MESHES_ENV).map(PathBuf::from);
    env.into_iter()
        .chain([PathBuf::from("collision_meshes"), data_dir()])
        .find(|d| check_meshes_dir(d).is_ok())
}

/// Télécharge et vérifie les meshes dans `dest` (bloquant ; à lancer dans un thread).
pub fn download(dest: &Path) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("rocketsim-trainer-fetch-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        std::fs::write(tmp.join("meshes.sha256"), SUMS).map_err(|e| e.to_string())?;
        let script = tmp.join("fetch-meshes.sh");
        std::fs::write(&script, SCRIPT).map_err(|e| e.to_string())?;
        let out = Command::new("bash")
            .arg(&script)
            .arg(dest)
            .output()
            .map_err(|e| format!("impossible de lancer bash : {e}"))?;
        if out.status.success() {
            check_meshes_dir(dest).map_err(|e| e.to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let msg = err.lines().last().unwrap_or("échec du téléchargement");
            Err(format!("{msg} (curl, tar et accès Internet requis)"))
        }
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le script embarqué cherche `meshes.sha256` à côté de lui : c'est ce que `download` recrée.
    #[test]
    fn embedded_script_and_sums_are_consistent() {
        assert!(SCRIPT.contains("meshes.sha256"));
        assert_eq!(SUMS.lines().count(), 16);
    }
}
