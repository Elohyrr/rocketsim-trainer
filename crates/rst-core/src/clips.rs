//! Clips rejouables du learner : liste (`rst watch --clips`) et téléchargement (`rst watch --clip`).
//!
//! Le learner sert `GET /clips` (JSON, du plus récent au plus ancien) et `GET /clips/<id>`. Le contenu d'un clip
//! est lu par le jeu (`sim_core::clip`) ; ce module ne regarde que l'index et ne parle pas à la simulation.

use std::path::Path;

use serde::Deserialize;

use crate::bot::format_utc;
use crate::update::Http;

/// Ligne de l'index (les champs inconnus d'un learner plus récent sont ignorés).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ClipEntry {
    pub id: String,
    #[serde(default)]
    pub mechanic: String,
    #[serde(default)]
    pub created_unix: u64,
    #[serde(default)]
    pub seconds: f32,
    #[serde(default)]
    pub bytes: u64,
}

/// D'où lire un clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Url(String),
    File(String),
}

/// `--clip` : une URL (`http…`), un fichier local existant, sinon un identifiant du learner `base`.
pub fn resolve(arg: &str, base: &str) -> Source {
    if arg.starts_with("http://") || arg.starts_with("https://") {
        Source::Url(arg.to_string())
    } else if Path::new(arg).is_file() {
        Source::File(arg.to_string())
    } else {
        let id = arg.strip_suffix(".json").unwrap_or(arg);
        Source::Url(format!("{base}/clips/{id}"))
    }
}

pub fn parse_list(json: &[u8]) -> Result<Vec<ClipEntry>, String> {
    serde_json::from_slice(json).map_err(|e| format!("liste de clips illisible : {e}"))
}

/// Liste du learner `base` (sans barre finale).
pub fn fetch_list(http: &dyn Http, base: &str) -> Result<Vec<ClipEntry>, String> {
    let body = http
        .get(&format!("{base}/clips"))
        .map_err(|e| format!("liste des clips ({base}) : {e}"))?;
    parse_list(&body)
}

/// Contenu brut d'un clip.
pub fn load(http: &dyn Http, src: &Source) -> Result<Vec<u8>, String> {
    match src {
        Source::File(p) => std::fs::read(p).map_err(|e| format!("{p} : {e}")),
        Source::Url(u) => http.get(u).map_err(|e| format!("clip {u} : {e}")),
    }
}

/// Une ligne par clip : identifiant, mécanique, durée, date.
pub fn format_list(entries: &[ClipEntry]) -> String {
    if entries.is_empty() {
        return "Aucun clip pour l'instant.".into();
    }
    let mut s = String::new();
    for e in entries {
        s.push_str(&format!(
            "{}  {}  {:.1} s  {} Kio  {}\n",
            e.id,
            e.mechanic,
            e.seconds,
            e.bytes.div_ceil(1024),
            format_utc(e.created_unix)
        ));
    }
    s.push_str("\nRejouer : rst watch --clip <id>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::NetError;
    use std::collections::HashMap;

    struct Fake(HashMap<String, Vec<u8>>);
    impl Http for Fake {
        fn get(&self, url: &str) -> Result<Vec<u8>, NetError> {
            self.0
                .get(url)
                .cloned()
                .ok_or(NetError::Other("404".into()))
        }
    }

    #[test]
    fn resolves_ids_urls_and_files() {
        let b = "https://rocketsim.celri.dev";
        assert_eq!(
            resolve("1700-flick-3", b),
            Source::Url("https://rocketsim.celri.dev/clips/1700-flick-3".into())
        );
        assert_eq!(
            resolve("1700-flick-3.json", b),
            Source::Url("https://rocketsim.celri.dev/clips/1700-flick-3".into())
        );
        assert_eq!(
            resolve("http://h:1/clips/x", b),
            Source::Url("http://h:1/clips/x".into())
        );
        let d = std::env::temp_dir().join(format!("rst-clip-{}.json", std::process::id()));
        std::fs::write(&d, b"{}").unwrap();
        let p = d.to_string_lossy().into_owned();
        assert_eq!(resolve(&p, b), Source::File(p.clone()));
        assert_eq!(
            load(&Fake(HashMap::new()), &Source::File(p)).unwrap(),
            b"{}"
        );
        std::fs::remove_file(d).ok();
    }

    #[test]
    fn lists_and_downloads() {
        let json = br#"[{"id":"a-flick-1","mechanic":"flick","created_unix":1700000000,"seconds":4.5,"bytes":20480,"nouveau":1}]"#;
        let http = Fake(HashMap::from([
            ("h/clips".into(), json.to_vec()),
            ("h/clips/a-flick-1".into(), b"clip".to_vec()),
        ]));
        let l = fetch_list(&http, "h").unwrap();
        assert_eq!(l.len(), 1);
        let t = format_list(&l);
        assert!(
            t.contains("a-flick-1  flick  4.5 s  20 Kio  2023-11-14"),
            "{t}"
        );
        assert_eq!(
            load(&http, &Source::Url("h/clips/a-flick-1".into())).unwrap(),
            b"clip"
        );
        assert!(fetch_list(&Fake(HashMap::new()), "h").is_err());
        assert!(format_list(&[]).contains("Aucun clip"));
        assert!(parse_list(b"pas du json").is_err());
    }
}
