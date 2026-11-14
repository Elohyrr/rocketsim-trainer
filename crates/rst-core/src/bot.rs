//! Politique du bot : téléchargement depuis le learner (`rst bot pull`), validation, métadonnées affichées par le jeu.
//!
//! Le dossier de politique contient `policy.safetensors` (poids) et `policy.json` (palier, taille, date), les deux
//! fichiers servis par le learner. Écriture atomique : une politique invalide ne remplace jamais la précédente.

use std::path::{Path, PathBuf};

use policy_runtime::Policy;
use serde::Deserialize;

use crate::update::{Http, NetError};

/// URL par défaut du learner (Tailscale) ; surchargée par `--url` ou `$RST_BOT_URL`.
pub const DEFAULT_URL: &str = "https://rocketsim.celri.dev";
pub const URL_ENV: &str = "RST_BOT_URL";
pub const WEIGHTS: &str = "policy.safetensors";
pub const META: &str = "policy.json";

/// Contenu de `policy.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PolicyMeta {
    /// Palier de curriculum de la politique.
    #[serde(default)]
    pub stage: u32,
    #[serde(default)]
    pub bytes: u64,
    /// Date d'export (secondes Unix) : sert de version.
    #[serde(default)]
    pub updated_unix: u64,
}

impl PolicyMeta {
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(json).map_err(|e| format!("policy.json illisible : {e}"))
    }

    /// Lit `policy.json` d'un dossier de politique (`None` s'il manque ou est illisible).
    pub fn load(dir: &Path) -> Option<Self> {
        Self::parse(&std::fs::read(dir.join(META)).ok()?).ok()
    }

    /// Libellé court pour l'écran : « palier 3 · politique du 2026-11-14 21:09 UTC ».
    pub fn label(&self) -> String {
        if self.updated_unix == 0 {
            format!("palier {}", self.stage)
        } else {
            format!(
                "palier {} · politique du {}",
                self.stage,
                format_utc(self.updated_unix)
            )
        }
    }
}

/// `AAAA-MM-JJ HH:MM UTC` (jours depuis 1970 → calendrier grégorien).
pub fn format_utc(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let rem = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}

/// URL effective : `--url`, sinon `$RST_BOT_URL`, sinon la valeur par défaut. Sans barre finale.
pub fn resolve_url(flag: Option<&str>, env: Option<&str>) -> String {
    flag.filter(|s| !s.trim().is_empty())
        .or(env.filter(|s| !s.trim().is_empty()))
        .unwrap_or(DEFAULT_URL)
        .trim()
        .trim_end_matches('/')
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulled {
    pub dir: PathBuf,
    pub meta: Option<PolicyMeta>,
    pub bytes: usize,
}

/// Vérifie qu'un contenu safetensors est une politique utilisable avec `obs_dim` entrées et `actions` sorties.
fn validate(dir: &Path, data: &[u8], obs_dim: usize, actions: usize) -> Result<(), String> {
    let tmp = dir.join(format!(".{WEIGHTS}.check.{}", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let r = Policy::load(&tmp).map_err(|e| format!("politique invalide : {e}"));
    let _ = std::fs::remove_file(&tmp);
    let p = r?;
    if (p.obs_dim(), p.action_count()) != (obs_dim, actions) {
        return Err(format!(
            "politique incompatible : {} entrées / {} actions, le client attend {obs_dim} / {actions} (rst update ?)",
            p.obs_dim(),
            p.action_count()
        ));
    }
    Ok(())
}

fn install(dir: &Path, weights: &[u8], meta: Option<&[u8]>) -> Result<(), String> {
    let put = |name: &str, content: &[u8]| -> Result<(), String> {
        let tmp = dir.join(format!(".{name}.new"));
        std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
    };
    put(WEIGHTS, weights)?;
    match meta {
        Some(m) => put(META, m)?,
        // Pas de métadonnées : ne pas laisser celles d'une politique plus ancienne.
        None => {
            let _ = std::fs::remove_file(dir.join(META));
        }
    }
    Ok(())
}

/// Télécharge la politique depuis `base_url` dans `dir`.
pub fn pull(
    http: &dyn Http,
    base_url: &str,
    dir: &Path,
    obs_dim: usize,
    actions: usize,
) -> Result<Pulled, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    let get = |name: &str| {
        let url = format!("{}/{name}", base_url.trim_end_matches('/'));
        http.get(&url).map_err(|e| match e {
            NetError::Offline(m) => format!("serveur injoignable ({m}) : {url}"),
            NetError::Other(m) => m,
        })
    };
    let meta_raw = get(META)?;
    let meta = PolicyMeta::parse(&meta_raw)?;
    let weights = get(WEIGHTS)?;
    validate(dir, &weights, obs_dim, actions)?;
    install(dir, &weights, Some(&meta_raw))?;
    Ok(Pulled {
        dir: dir.to_path_buf(),
        meta: Some(meta),
        bytes: weights.len(),
    })
}

/// Installe une politique depuis un fichier local (`policy.json` voisin copié s'il existe).
pub fn pull_file(
    file: &Path,
    dir: &Path,
    obs_dim: usize,
    actions: usize,
) -> Result<Pulled, String> {
    let weights = std::fs::read(file).map_err(|e| format!("{} : {e}", file.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    validate(dir, &weights, obs_dim, actions)?;
    let meta_raw = std::fs::read(file.with_file_name(META)).ok();
    let meta = meta_raw.as_deref().and_then(|m| PolicyMeta::parse(m).ok());
    install(
        dir,
        &weights,
        meta_raw.as_deref().filter(|_| meta.is_some()),
    )?;
    Ok(Pulled {
        dir: dir.to_path_buf(),
        meta,
        bytes: weights.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use policy_runtime::{Tensor, write_safetensors};
    use std::collections::{BTreeMap, HashMap};

    struct Fake(HashMap<String, Result<Vec<u8>, NetError>>);
    impl Http for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
            self.0
                .get(url)
                .cloned()
                .unwrap_or(Err(NetError::Other(format!("404 {url}"))))
        }
    }

    fn tiny_file(path: &Path) -> Vec<u8> {
        let mut m = BTreeMap::new();
        let t = |shape: Vec<usize>| Tensor {
            data: vec![0.5; shape.iter().product()],
            shape,
        };
        m.insert("policy.0.weight".into(), t(vec![4, 3]));
        m.insert("policy.0.bias".into(), t(vec![3]));
        write_safetensors(path, &m).unwrap();
        std::fs::read(path).unwrap()
    }

    #[test]
    fn url_precedence_and_trailing_slash() {
        assert_eq!(resolve_url(None, None), DEFAULT_URL);
        assert_eq!(resolve_url(None, Some("http://a:1/")), "http://a:1");
        assert_eq!(resolve_url(Some("http://b"), Some("http://a")), "http://b");
        assert_eq!(resolve_url(Some(" "), None), DEFAULT_URL);
    }

    #[test]
    fn utc_format_and_label() {
        assert_eq!(format_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_utc(1_794_000_540), "2026-11-06 21:29 UTC");
        let m = PolicyMeta {
            stage: 3,
            bytes: 1,
            updated_unix: 0,
        };
        assert_eq!(m.label(), "palier 3");
    }

    #[test]
    fn pull_installs_valid_policy_and_rejects_bad_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let w = tiny_file(&tmp.path().join("src.safetensors"));
        let base = "http://x:9090";
        let ok = || {
            Fake(HashMap::from([
                (
                    format!("{base}/policy.json"),
                    Ok(br#"{"stage":4,"bytes":1,"updated_unix":1794000540}"#.to_vec()),
                ),
                (format!("{base}/policy.safetensors"), Ok(w.clone())),
            ]))
        };
        let dir = tmp.path().join("bot");
        let r = pull(&ok(), &format!("{base}/"), &dir, 4, 3).unwrap();
        assert_eq!(r.meta.as_ref().unwrap().stage, 4);
        assert_eq!(std::fs::read(dir.join(WEIGHTS)).unwrap(), w);
        assert_eq!(PolicyMeta::load(&dir).unwrap().stage, 4);

        // Dimensions incompatibles : refus, ancienne politique intacte.
        let e = pull(&ok(), base, &dir, 43, 78).unwrap_err();
        assert!(e.contains("incompatible"), "{e}");
        assert_eq!(std::fs::read(dir.join(WEIGHTS)).unwrap(), w);

        // Hors ligne.
        let off = Fake(HashMap::from([(
            format!("{base}/policy.json"),
            Err(NetError::Offline("dns".into())),
        )]));
        assert!(
            pull(&off, base, &dir, 4, 3)
                .unwrap_err()
                .contains("injoignable")
        );
    }

    #[test]
    fn pull_file_copies_neighbour_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("in/policy.safetensors");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        tiny_file(&src);
        std::fs::write(src.with_file_name(META), br#"{"stage":2}"#).unwrap();
        let dir = tmp.path().join("bot");
        let r = pull_file(&src, &dir, 4, 3).unwrap();
        assert_eq!(r.meta.unwrap().stage, 2);
        assert!(pull_file(&tmp.path().join("absent"), &dir, 4, 3).is_err());
        std::fs::write(&src, b"garbage").unwrap();
        assert!(pull_file(&src, &dir, 4, 3).is_err());
    }
}
