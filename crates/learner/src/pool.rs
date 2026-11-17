//! Pool d'anciennes politiques pour le self-play : `POOL_MAX` instantanés (un toutes les `SNAPSHOT_EVERY`
//! itérations), sauvegardés sur le disque (`<out>/pool/NNNNNN.safetensors`, ≈ 130 ko chacun) à côté du
//! checkpoint. Tirage pondéré vers les adversaires qui battent encore l'agent. Voir `docs/self-play.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use burn::tensor::Device;
use policy_runtime::{read_safetensors, write_safetensors};
use rl_env::Rng;

use crate::ppo::Mlp;

/// Instantanés gardés.
pub const POOL_MAX: usize = 20;
/// Itérations entre deux instantanés (≈ 400 k pas).
pub const SNAPSHOT_EVERY: usize = 200;
/// Part des envs qui jouent contre le pool (le reste : copie gelée de la politique courante).
pub const POOL_SHARE: f32 = 0.3;

pub struct Entry {
    pub serial: u64,
    pub net: Mlp,
    /// Épisodes joués contre cet adversaire et buts encaissés (pour la pondération du tirage ; non persisté).
    pub episodes: u64,
    pub conceded: u64,
}

pub struct Pool {
    dir: Option<PathBuf>,
    entries: Vec<Entry>,
    next_serial: u64,
    /// Itérations écoulées depuis le dernier instantané.
    pub since_snapshot: usize,
    pub every: usize,
    /// Instantanés pris depuis le démarrage du processus.
    pub snapshots_taken: u64,
}

fn file(dir: &Path, serial: u64) -> PathBuf {
    dir.join(format!("{serial:06}.safetensors"))
}

impl Pool {
    /// Pool vide ; `dir` = dossier de sauvegarde (`None` : en mémoire seulement).
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            entries: Vec::new(),
            next_serial: 1,
            since_snapshot: 0,
            every: SNAPSHOT_EVERY,
            snapshots_taken: 0,
        }
    }

    /// Recharge les instantanés de `dir` (les `POOL_MAX` plus récents) ; les fichiers illisibles sont ignorés.
    pub fn load(dir: &Path, template: &Mlp, device: &Device) -> Self {
        let mut p = Self::new(Some(dir.to_path_buf()));
        let mut serials: Vec<u64> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().into_string().ok()?;
                n.strip_suffix(".safetensors")?.parse().ok()
            })
            .collect();
        serials.sort_unstable();
        for serial in serials {
            let net = read_safetensors(&file(dir, serial))
                .map_err(|e| e.to_string())
                .and_then(|t| template.from_tensors(&t, device));
            match net {
                Ok(net) => p.entries.push(Entry {
                    serial,
                    net,
                    episodes: 0,
                    conceded: 0,
                }),
                Err(e) => eprintln!("pool : instantané {serial} ignoré ({e})"),
            }
            p.next_serial = p.next_serial.max(serial + 1);
        }
        let extra = p.entries.len().saturating_sub(POOL_MAX);
        p.entries.drain(..extra);
        p
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Mlp> {
        self.entries.get(i).map(|e| &e.net)
    }

    /// Ajoute un instantané (écrit sur le disque) ; le plus ancien sort au-delà de `POOL_MAX`.
    pub fn push(&mut self, net: Mlp) -> std::io::Result<()> {
        let serial = self.next_serial;
        self.next_serial += 1;
        if let Some(d) = &self.dir {
            std::fs::create_dir_all(d)?;
            // Écriture atomique : un arrêt en plein écrit ne laisse pas de fichier tronqué.
            let tmp = d.join(format!("{serial:06}.tmp"));
            write_safetensors(&tmp, &net.to_tensors())
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            std::fs::rename(&tmp, file(d, serial))?;
        }
        self.entries.push(Entry {
            serial,
            net,
            episodes: 0,
            conceded: 0,
        });
        while self.entries.len() > POOL_MAX {
            let old = self.entries.remove(0);
            if let Some(d) = &self.dir {
                let _ = std::fs::remove_file(file(d, old.serial));
            }
        }
        self.snapshots_taken += 1;
        self.since_snapshot = 0;
        Ok(())
    }

    /// Poids de tirage : 0,2 + taux de buts encaissés lissé (un adversaire qui bat l'agent pèse jusqu'à ≈ 6×).
    fn weight(e: &Entry) -> f64 {
        0.2 + (e.conceded as f64 + 1.0) / (e.episodes as f64 + 2.0)
    }

    /// Indice d'un adversaire tiré au hasard, pondéré ; `None` si le pool est vide.
    pub fn pick(&self, rng: &mut Rng) -> Option<usize> {
        let total: f64 = self.entries.iter().map(Self::weight).sum();
        if self.entries.is_empty() {
            return None;
        }
        let mut u = f64::from(rng.unit()) * total;
        for (i, e) in self.entries.iter().enumerate() {
            u -= Self::weight(e);
            if u <= 0.0 {
                return Some(i);
            }
        }
        Some(self.entries.len() - 1)
    }

    /// Résultat de `episodes` épisodes contre l'entrée `i`, dont `conceded` buts encaissés.
    pub fn record(&mut self, i: usize, episodes: u64, conceded: u64) {
        if let Some(e) = self.entries.get_mut(i) {
            e.episodes += episodes;
            e.conceded += conceded;
        }
    }

    /// Instantané nommé (à garder à part, hors pool) : écrit `net` dans `path`.
    pub fn write_net(net: &Mlp, path: &Path) -> std::io::Result<()> {
        write_safetensors(path, &net.to_tensors()).map_err(|e| std::io::Error::other(e.to_string()))
    }

    pub fn read_net(path: &Path, template: &Mlp, device: &Device) -> Result<Mlp, String> {
        let t: BTreeMap<_, _> = read_safetensors(path).map_err(|e| e.to_string())?;
        template.from_tensors(&t, device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(seed: f32) -> (Mlp, Device) {
        let dev = Device::flex();
        let mut m = Mlp::new(4, 8, 3, &dev);
        // Poids reconnaissables : `seed` sur le biais de sortie.
        let mut t = m.to_tensors();
        t.get_mut("policy.2.bias").unwrap().data = vec![seed; 3];
        m = m.from_tensors(&t, &dev).unwrap();
        (m, dev)
    }

    #[test]
    fn keeps_the_latest_twenty_on_disk_and_reloads_them() {
        let dir = std::env::temp_dir().join(format!("pool-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Pool::new(Some(dir.clone()));
        let (tpl, dev) = net(0.0);
        for i in 0..(POOL_MAX + 5) {
            p.push(net(i as f32).0).unwrap();
        }
        assert_eq!(p.len(), POOL_MAX);
        let files = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, POOL_MAX, "les plus anciens sont supprimés du disque");
        let q = Pool::load(&dir, &tpl, &dev);
        assert_eq!(q.len(), POOL_MAX);
        let b = q.get(POOL_MAX - 1).unwrap().to_tensors();
        assert_eq!(b["policy.2.bias"].data, vec![(POOL_MAX + 4) as f32; 3]);
        // Le numéro suivant continue après le plus récent.
        let mut q = q;
        q.push(net(99.0).0).unwrap();
        assert_eq!(q.next_serial, (POOL_MAX + 5 + 2) as u64);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sampling_favors_opponents_that_beat_the_agent() {
        let mut p = Pool::new(None);
        p.push(net(0.0).0).unwrap();
        p.push(net(1.0).0).unwrap();
        p.record(0, 100, 0);
        p.record(1, 100, 90);
        let mut rng = Rng::new(7);
        let hard = (0..2000).filter(|_| p.pick(&mut rng) == Some(1)).count();
        assert!(hard > 1300 && hard < 1900, "{hard}");
        assert_eq!(Pool::new(None).pick(&mut rng), None);
    }
}
