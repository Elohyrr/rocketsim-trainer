//! Mise à jour : API GitHub releases (sans jeton), téléchargement, vérification SHA256, remplacement atomique.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::version::Version;
use crate::{REPO, hash, replace};

/// Nom d'asset préféré, puis ancien nom gardé pour la compatibilité des liens existants.
pub const ASSET_NAMES: [&str; 2] = ["rst-linux-x86_64", "rocketsim-trainer-linux-x86_64"];
pub const SUMS_ASSET: &str = "SHA256SUMS";

pub fn latest_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Pas de réseau (DNS, connexion, délai dépassé).
    Offline(String),
    Other(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offline(e) => write!(f, "hors ligne ({e})"),
            Self::Other(e) => f.write_str(e),
        }
    }
}

/// Accès HTTP minimal (remplaçable dans les tests).
pub trait Http {
    fn get(&self, url: &str) -> Result<Vec<u8>, NetError>;
}

/// Implémentation par `curl` (déjà requis par le jeu pour les meshes).
pub struct Curl;

impl Http for Curl {
    fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
        let out = Command::new("curl")
            .args(["-fsSL", "--retry", "2", "--max-time", "300"])
            .args([
                "-H",
                "User-Agent: rst",
                "-H",
                "Accept: application/vnd.github+json",
            ])
            .arg(url)
            .output()
            .map_err(|e| NetError::Other(format!("impossible de lancer curl : {e}")))?;
        if out.status.success() {
            return Ok(out.stdout);
        }
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        match out.status.code() {
            // 5/6 résolution, 7 connexion, 28 délai
            Some(5 | 6 | 7 | 28) => Err(NetError::Offline(msg)),
            Some(22) if msg.contains("403") || msg.contains("429") => Err(NetError::Other(
                "limite de l'API GitHub atteinte, réessayez plus tard".into(),
            )),
            _ => Err(NetError::Other(format!("téléchargement de {url} : {msg}"))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(json).map_err(|e| format!("réponse GitHub illisible : {e}"))
    }

    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.tag_name)
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// Notes de release réduites à `max` lignes non vides.
    pub fn short_notes(&self, max: usize) -> String {
        let lines: Vec<&str> = self
            .body
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .collect();
        let mut s = lines
            .iter()
            .take(max)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if lines.len() > max {
            s.push_str("\n…");
        }
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    Newer,
}

pub fn fetch_latest(http: &dyn Http) -> Result<Release, NetError> {
    let body = http.get(&latest_url())?;
    Release::parse(&body).map_err(NetError::Other)
}

/// Compare la version courante à celle de la release.
pub fn status(current: &str, release: &Release) -> Result<Status, String> {
    let cur =
        Version::parse(current).ok_or_else(|| format!("version courante illisible : {current}"))?;
    let new = release
        .version()
        .ok_or_else(|| format!("tag de release illisible : {}", release.tag_name))?;
    Ok(if new > cur {
        Status::Newer
    } else {
        Status::UpToDate
    })
}

/// Télécharge l'asset, vérifie son SHA256 (fichier `SHA256SUMS` obligatoire), remplace `target`.
pub fn apply(http: &dyn Http, release: &Release, target: &Path) -> Result<(), String> {
    let asset = ASSET_NAMES
        .iter()
        .find_map(|n| release.asset(n))
        .ok_or_else(|| {
            format!(
                "aucun binaire {} dans la release {}",
                ASSET_NAMES[0], release.tag_name
            )
        })?;
    let sums = release.asset(SUMS_ASSET).ok_or_else(|| {
        format!("la release {} n'a pas de {SUMS_ASSET} : mise à jour refusée (pas de vérification possible)", release.tag_name)
    })?;
    let sums = http
        .get(&sums.browser_download_url)
        .map_err(|e| e.to_string())?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = hash::find_sum(&sums, &asset.name)
        .ok_or_else(|| format!("{} absent de {SUMS_ASSET}", asset.name))?;
    let bin = http
        .get(&asset.browser_download_url)
        .map_err(|e| e.to_string())?;
    let got = hash::sha256_hex(&bin);
    if got != expected {
        return Err(format!(
            "SHA256 différent (attendu {expected}, obtenu {got}) : binaire non installé"
        ));
    }
    if !bin.starts_with(b"\x7fELF") {
        return Err("le fichier téléchargé n'est pas un exécutable Linux".into());
    }
    replace::atomic_replace(target, &bin)
        .map_err(|e| format!("remplacement de {} impossible : {e}", target.display()))
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
                .unwrap_or(Err(NetError::Other(format!("404 {url}"))))
        }
    }

    fn release(with_sums: bool) -> Release {
        let mut assets = vec![Asset {
            name: "rst-linux-x86_64".into(),
            browser_download_url: "u/bin".into(),
        }];
        if with_sums {
            assets.push(Asset {
                name: SUMS_ASSET.into(),
                browser_download_url: "u/sums".into(),
            });
        }
        Release {
            tag_name: "v0.2.0".into(),
            body: Some("a\n\nb\nc\nd".into()),
            assets,
        }
    }

    fn fake(bin: &[u8], sum_of: &[u8]) -> Fake {
        let sums = format!("{}  rst-linux-x86_64\n", hash::sha256_hex(sum_of));
        Fake(HashMap::from([
            ("u/bin".into(), Ok(bin.to_vec())),
            ("u/sums".into(), Ok(sums.into_bytes())),
        ]))
    }

    #[test]
    fn parses_github_json_and_notes() {
        let j = br#"{"tag_name":"v0.2.0","body":"x\n\ny","assets":[{"name":"a","browser_download_url":"u","size":1}],"other":1}"#;
        let r = Release::parse(j).unwrap();
        assert_eq!(r.version(), Some(Version(0, 2, 0)));
        assert_eq!(r.short_notes(5), "x\ny");
        assert_eq!(release(true).short_notes(2), "a\nb\n…");
        assert!(Release::parse(b"nope").is_err());
    }

    #[test]
    fn status_compares() {
        let r = release(true);
        assert_eq!(status("0.1.2", &r), Ok(Status::Newer));
        assert_eq!(status("0.2.0", &r), Ok(Status::UpToDate));
        assert_eq!(status("0.3.0", &r), Ok(Status::UpToDate));
        assert!(status("bof", &r).is_err());
    }

    #[test]
    fn apply_installs_verified_binary() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("rst");
        std::fs::write(&t, b"old").unwrap();
        let bin = b"\x7fELFnouveau";
        apply(&fake(bin, bin), &release(true), &t).unwrap();
        assert_eq!(std::fs::read(&t).unwrap(), bin);
    }

    #[test]
    fn apply_rejects_bad_hash_missing_sums_and_non_elf() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("rst");
        std::fs::write(&t, b"old").unwrap();
        let e = apply(&fake(b"\x7fELFx", b"autre"), &release(true), &t).unwrap_err();
        assert!(e.contains("SHA256 différent"));
        let e = apply(&fake(b"\x7fELFx", b"\x7fELFx"), &release(false), &t).unwrap_err();
        assert!(e.contains("SHA256SUMS"));
        let e = apply(&fake(b"texte", b"texte"), &release(true), &t).unwrap_err();
        assert!(e.contains("exécutable"));
        assert_eq!(std::fs::read(&t).unwrap(), b"old");
    }

    #[test]
    fn offline_is_reported() {
        let f = Fake(HashMap::from([(
            latest_url(),
            Err(NetError::Offline("dns".into())),
        )]));
        assert!(matches!(fetch_latest(&f), Err(NetError::Offline(_))));
    }
}
