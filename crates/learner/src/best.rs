//! Points de restauration dans le dossier `--out` (voir `docs/exploration.md`, « Reprise ») :
//! - `best-stage-N/` : meilleur état mesuré au palier N (taux de but sur la fenêtre du garde-fou), remplacé
//!   seulement par mieux ; écrit en continu, donc toujours présent avant un changement de palier ou d'exploration ;
//! - `last-start/` : état relu au démarrage, avant toute itération (point de retour avant un déploiement) ;
//! - `restore-from` : fichier contenant un dossier (absolu ou relatif à `--out`) ; au démarrage, son état est
//!   recopié dans `--out` avant la reprise, puis le fichier devient `restore-from.done`.
//!
//! Un état = `checkpoint.*`, `policy.safetensors` (+ `.json`), `stage`, `explore.json`, `opponent-obs-v2`, plus
//! `best.json` (taux mesuré, pas cumulés, date) pour les instantanés.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ppo::Trainer;

/// Marqueur : checkpoint entraîné avec le bloc adversaire correct (voitures repérées par id).
pub const OPPONENT_OBS_MARKER: &str = "opponent-obs-v2";
pub const RESTORE_FILE: &str = "restore-from";
pub const META: &str = "best.json";
pub const LAST_START: &str = "last-start";
/// Amélioration minimale du taux de but pour remplacer un meilleur état.
pub const MARGIN: f64 = 0.02;
/// Itérations minimales entre deux écritures de meilleur état.
pub const MIN_ITERS_BETWEEN: usize = 100;

pub fn best_dir(out: &Path, stage: u32) -> PathBuf {
    out.join(format!("best-stage-{stage}"))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Écrit l'état complet de `tr` (palier `stage`) dans `<out>/<name>/`, atomiquement (dossier temporaire puis
/// renommage). `goal_rate` < 0 = non mesuré.
pub fn snapshot(
    tr: &Trainer,
    out: &Path,
    name: &str,
    stage: u32,
    goal_rate: f64,
) -> std::io::Result<PathBuf> {
    let tmp = out.join(format!(".{name}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    tr.save_checkpoint(&tmp.join("checkpoint"))
        .map_err(std::io::Error::other)?;
    tr.export(&tmp.join("policy.safetensors"))?;
    std::fs::write(tmp.join("stage"), format!("{stage}\n"))?;
    tr.explore.save(&tmp)?;
    std::fs::write(tmp.join(OPPONENT_OBS_MARKER), "1\n")?;
    std::fs::write(
        tmp.join(META),
        format!(
            "{{\"stage\":{stage},\"goal_rate\":{goal_rate},\"steps\":{},\"unix\":{}}}\n",
            tr.explore.steps,
            now_unix()
        ),
    )?;
    let dst = out.join(name);
    let old = out.join(format!(".{name}.old"));
    let _ = std::fs::remove_dir_all(&old);
    if dst.exists() {
        std::fs::rename(&dst, &old)?;
    }
    std::fs::rename(&tmp, &dst)?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(dst)
}

/// Taux de but enregistré dans un instantané (`-1` si non mesuré ou illisible).
pub fn recorded_rate(dir: &Path) -> Option<f64> {
    let s = std::fs::read_to_string(dir.join(META)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    Some(v["goal_rate"].as_f64().unwrap_or(-1.0))
}

/// Meilleurs taux de but connus par palier (relus depuis les `best-stage-N/best.json`).
#[derive(Clone, Debug, Default)]
pub struct Best {
    rates: BTreeMap<u32, f64>,
    last_iter: Option<usize>,
}

impl Best {
    pub fn load(out: &Path) -> Self {
        let mut b = Self::default();
        for s in 0..=crate::curriculum::LAST_STAGE {
            if let Some(r) = recorded_rate(&best_dir(out, s)) {
                b.rates.insert(s, r);
            }
        }
        b
    }

    pub fn get(&self, stage: u32) -> Option<f64> {
        self.rates.get(&stage).copied()
    }

    /// Le taux `rate` mesuré à l'itération `iter` mérite-t-il un nouvel instantané du palier ?
    pub fn wants(&self, stage: u32, rate: f64, iter: usize) -> bool {
        let spaced = self.last_iter.is_none_or(|l| iter >= l + MIN_ITERS_BETWEEN);
        spaced && self.get(stage).is_none_or(|b| rate >= b + MARGIN)
    }

    pub fn record(&mut self, stage: u32, rate: f64, iter: usize) {
        self.rates.insert(stage, rate);
        self.last_iter = Some(iter);
    }
}

/// Fichiers d'état effacés de `--out` lors d'une restauration si la source ne les a pas (sinon l'état restauré
/// serait mélangé à l'ancien : `explore.json` d'un autre réglage, marqueur d'un autre checkpoint).
const OPTIONAL_STATE: [&str; 2] = ["explore.json", OPPONENT_OBS_MARKER];

/// Applique `<out>/restore-from` s'il existe ; renvoie le dossier restauré.
pub fn apply_restore(out: &Path) -> Result<Option<PathBuf>, String> {
    let f = out.join(RESTORE_FILE);
    let Ok(s) = std::fs::read_to_string(&f) else {
        return Ok(None);
    };
    let src = PathBuf::from(s.trim());
    let src = if src.is_absolute() {
        src
    } else {
        out.join(src)
    };
    let files: Vec<PathBuf> = std::fs::read_dir(&src)
        .map_err(|e| format!("{RESTORE_FILE} : {} illisible ({e})", src.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    let name = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned()
    };
    if !files.iter().any(|p| name(p).starts_with("checkpoint.")) {
        return Err(format!(
            "{RESTORE_FILE} : pas de checkpoint dans {}",
            src.display()
        ));
    }
    for opt in OPTIONAL_STATE {
        if !files.iter().any(|p| name(p) == opt) {
            let _ = std::fs::remove_file(out.join(opt));
        }
    }
    for p in &files {
        let n = name(p);
        if n == META {
            continue;
        }
        let tmp = out.join(format!(".{n}.restore"));
        std::fs::copy(p, &tmp).map_err(|e| format!("copie de {n} : {e}"))?;
        std::fs::rename(&tmp, out.join(&n)).map_err(|e| format!("copie de {n} : {e}"))?;
    }
    std::fs::rename(&f, out.join(format!("{RESTORE_FILE}.done")))
        .map_err(|e| format!("{RESTORE_FILE} : {e}"))?;
    Ok(Some(src))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("best-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn best_is_replaced_only_by_a_clear_improvement_and_not_too_often() {
        let mut b = Best::default();
        assert!(
            b.wants(5, 0.0, 0),
            "rien d'enregistré : tout est bon à prendre"
        );
        b.record(5, 0.6, 0);
        assert!(!b.wants(5, 0.61, 500));
        assert!(!b.wants(5, 0.7, 50), "trop tôt après la dernière écriture");
        assert!(b.wants(5, 0.7, 100));
        assert!(b.wants(4, 0.1, 100), "paliers indépendants");
    }

    #[test]
    fn rates_are_read_back_from_disk() {
        let out = tmp("load");
        let d = best_dir(&out, 5);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(META), "{\"stage\":5,\"goal_rate\":0.72}").unwrap();
        let d2 = best_dir(&out, 2);
        std::fs::create_dir_all(&d2).unwrap();
        std::fs::write(d2.join(META), "{\"stage\":2,\"goal_rate\":-1}").unwrap();
        let b = Best::load(&out);
        assert_eq!(b.get(5), Some(0.72));
        assert_eq!(b.get(2), Some(-1.0));
        assert_eq!(b.get(3), None);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn restore_copies_the_snapshot_and_drops_stale_optional_state() {
        let out = tmp("restore");
        assert_eq!(apply_restore(&out), Ok(None));
        for (f, c) in [
            ("checkpoint.bpk", "neuf"),
            ("stage", "2\n"),
            ("explore.json", "{\"ent_coef\":0.049}"),
            (OPPONENT_OBS_MARKER, "1\n"),
        ] {
            std::fs::write(out.join(f), c).unwrap();
        }
        // Sauvegarde ancienne : ni explore.json ni marqueur.
        let bak = out.join("sauvegarde");
        std::fs::create_dir_all(&bak).unwrap();
        std::fs::write(bak.join("checkpoint.bpk"), "vieux").unwrap();
        std::fs::write(bak.join("stage"), "5\n").unwrap();
        std::fs::write(bak.join(META), "{}").unwrap();
        std::fs::write(out.join(RESTORE_FILE), "sauvegarde\n").unwrap();
        assert_eq!(apply_restore(&out), Ok(Some(bak.clone())));
        let read = |f: &str| std::fs::read_to_string(out.join(f)).ok();
        assert_eq!(read("checkpoint.bpk").as_deref(), Some("vieux"));
        assert_eq!(read("stage").as_deref(), Some("5\n"));
        assert_eq!(read("explore.json"), None);
        assert_eq!(read(OPPONENT_OBS_MARKER), None);
        assert_eq!(read(META), None);
        assert_eq!(read(RESTORE_FILE), None);
        assert!(read("restore-from.done").is_some());
        assert_eq!(apply_restore(&out), Ok(None), "appliqué une seule fois");
        // Source sans checkpoint : refus, rien n'est touché.
        std::fs::write(
            out.join(RESTORE_FILE),
            out.join("vide").display().to_string(),
        )
        .unwrap();
        std::fs::create_dir_all(out.join("vide")).unwrap();
        assert!(apply_restore(&out).is_err());
        assert_eq!(read("checkpoint.bpk").as_deref(), Some("vieux"));
        let _ = std::fs::remove_dir_all(&out);
    }
}
