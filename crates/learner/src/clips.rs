//! Anneau de clips rejouables sur le disque (PVC) : `<out>/clips/<id>.json` + `index.json`.
//!
//! Borné en nombre et en octets ; quand il est plein, on retire le plus ancien clip de la mécanique la plus
//! représentée (les mécaniques rares restent). Un délai minimal par mécanique évite d'inonder l'anneau avec la
//! même action répétée. Le serveur HTTP du learner (`metrics.rs`) sert la liste et les fichiers en lecture seule.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rl_env::clip::{Clip, Trajectory};
use serde::{Deserialize, Serialize};
use sim_core::{Sim, SimError};

pub const DIR: &str = "clips";
pub const INDEX: &str = "index.json";
pub const DEFAULT_MAX_CLIPS: usize = 200;
pub const DEFAULT_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Délai minimal entre deux clips de la même mécanique.
pub const DEFAULT_MIN_GAP: Duration = Duration::from_secs(20);

/// Ligne de `GET /clips`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub mechanic: String,
    pub created_unix: u64,
    /// Itération du learner à l'enregistrement.
    pub train_step: u64,
    /// Décisions (pas de 8 ticks) et durée (s).
    pub steps: u32,
    pub seconds: f32,
    pub bytes: u64,
}

pub struct ClipStore {
    dir: PathBuf,
    pub max_clips: usize,
    pub max_bytes: u64,
    pub min_gap: Duration,
    /// Du plus ancien au plus récent.
    entries: Vec<Entry>,
    last: HashMap<String, Instant>,
    seq: u64,
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(data)?;
    f.sync_data()?;
    std::fs::rename(&tmp, path)
}

/// Un id de clip ne contient que `[A-Za-z0-9_-]` (jamais de séparateur de chemin).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl ClipStore {
    /// Ouvre (et crée) `<out>/clips`, reprend l'index existant (entrées sans fichier ignorées, fichiers hors
    /// index supprimés : restes d'un arrêt en cours d'écriture).
    pub fn open(out: &Path, max_clips: usize, max_bytes: u64) -> std::io::Result<Self> {
        let dir = out.join(DIR);
        std::fs::create_dir_all(&dir)?;
        let mut entries: Vec<Entry> = std::fs::read(dir.join(INDEX))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        entries.retain(|e| valid_id(&e.id) && dir.join(format!("{}.json", e.id)).is_file());
        entries.sort_by_key(|e| (e.created_unix, e.id.clone()));
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for f in rd.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let known = name == INDEX
                    || name
                        .strip_suffix(".json")
                        .is_some_and(|id| entries.iter().any(|e| e.id == id));
                if !known {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        let seq = entries.len() as u64;
        let mut s = Self {
            dir,
            max_clips,
            max_bytes,
            min_gap: DEFAULT_MIN_GAP,
            entries,
            last: HashMap::new(),
            seq,
        };
        s.evict();
        s.write_index()?;
        Ok(s)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes).sum()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Enregistre le clip, sauf si la même mécanique en a déjà donné un il y a moins de `min_gap`.
    pub fn add(&mut self, mut clip: Clip, train_step: u64) -> std::io::Result<Option<Entry>> {
        let now = Instant::now();
        if self
            .last
            .get(&clip.mechanic)
            .is_some_and(|t| now.duration_since(*t) < self.min_gap)
        {
            return Ok(None);
        }
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.seq += 1;
        clip.id = format!("{unix}-{}-{}", clip.mechanic, self.seq);
        clip.created_unix = unix;
        clip.train_step = train_step;
        let data = clip.to_json();
        write_atomic(&self.dir.join(format!("{}.json", clip.id)), &data)?;
        let e = Entry {
            id: clip.id.clone(),
            mechanic: clip.mechanic.clone(),
            created_unix: unix,
            train_step,
            steps: clip.steps.len() as u32,
            seconds: clip.seconds(),
            bytes: data.len() as u64,
        };
        self.last.insert(clip.mechanic, now);
        self.entries.push(e.clone());
        self.evict();
        self.write_index()?;
        Ok(Some(e))
    }

    /// Retire le plus ancien clip de la mécanique la plus représentée jusqu'à respecter les deux plafonds.
    fn evict(&mut self) {
        while self.entries.len() > self.max_clips.max(1)
            || (self.bytes() > self.max_bytes && self.entries.len() > 1)
        {
            let mut count: HashMap<&str, usize> = HashMap::new();
            for e in &self.entries {
                *count.entry(&e.mechanic).or_default() += 1;
            }
            let top = count
                .iter()
                .max_by_key(|(m, n)| (**n, std::cmp::Reverse(**m)))
                .map(|(m, _)| m.to_string());
            let Some(top) = top else { return };
            let Some(i) = self.entries.iter().position(|e| e.mechanic == top) else {
                return;
            };
            let gone = self.entries.remove(i);
            let _ = std::fs::remove_file(self.dir.join(format!("{}.json", gone.id)));
        }
    }

    fn write_index(&self) -> std::io::Result<()> {
        let mut newest_first = self.entries.clone();
        newest_first.reverse();
        write_atomic(
            &self.dir.join(INDEX),
            &serde_json::to_vec(&newest_first).expect("index sérialisable"),
        )
    }
}

/// Fréquences d'échantillonnage acceptées par `/trajectory` (Hz).
const HZ_OK: [u32; 3] = [15, 30, 60];
/// Trajectoires gardées en mémoire (≈ 40 Kio chacune) : rejouer à la demande, pas en continu.
const CACHE_MAX: usize = 48;

type CacheKey = (String, u32);
static CACHE: Mutex<VecDeque<(CacheKey, Arc<[u8]>)>> = Mutex::new(VecDeque::new());

enum Fail {
    /// Meshes pas encore chargés dans ce processus.
    NotReady,
    Bad,
}

/// Réponse de `GET /clips/<id>/trajectory` : métadonnées du clip + [`Trajectory`] (`agent` = index de la voiture
/// de l'agent dans `cars`, `dt` = secondes entre deux images).
#[derive(Serialize)]
struct TrajectoryReply<'a> {
    id: &'a str,
    mechanic: &'a str,
    created_unix: u64,
    train_step: u64,
    tick_skip: u32,
    steps: usize,
    seconds: f32,
    dt: f32,
    trigger_time: f32,
    agent: usize,
    #[serde(flatten)]
    t: &'a Trajectory,
}

/// JSON de la trajectoire de `<clips>/<id>.json` à `hz`, d'après le cache ou un rejeu à la demande.
/// `None` si le clip n'existe pas.
fn trajectory_json(clips: &Path, id: &str, hz: u32) -> Option<Result<Arc<[u8]>, Fail>> {
    let file = std::fs::read(clips.join(format!("{id}.json"))).ok()?;
    let key = (id.to_string(), hz);
    if let Some(hit) = CACHE
        .lock()
        .ok()
        .and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()))
    {
        return Some(Ok(hit));
    }
    let out = (|| {
        let clip = Clip::from_json(&file).map_err(|_| Fail::Bad)?;
        let mut sim = Sim::new_soccar().map_err(|e| match e {
            SimError::NotInitialized => Fail::NotReady,
            _ => Fail::Bad,
        })?;
        let meta = (
            clip.mechanic.clone(),
            clip.created_unix,
            clip.train_step,
            clip.tick_skip,
            clip.steps.len(),
            clip.seconds(),
            clip.trigger_step,
        );
        let t = sim.trajectory(clip, hz).map_err(|_| Fail::Bad)?;
        let reply = TrajectoryReply {
            id,
            mechanic: &meta.0,
            created_unix: meta.1,
            train_step: meta.2,
            tick_skip: meta.3,
            steps: meta.4,
            seconds: meta.5,
            dt: 1.0 / hz as f32,
            trigger_time: meta.6 as f32 * meta.3 as f32 / sim_core::TICK_RATE as f32,
            agent: 0,
            t: &t,
        };
        let b: Arc<[u8]> = serde_json::to_vec(&reply).map_err(|_| Fail::Bad)?.into();
        Ok(b)
    })();
    if let (Ok(b), Ok(mut c)) = (&out, CACHE.lock()) {
        if c.len() >= CACHE_MAX {
            c.pop_front();
        }
        c.push_back((key, b.clone()));
    }
    Some(out)
}

type Reply = (&'static str, &'static str, Vec<u8>);

/// Routes en lecture seule : `GET /clips` (index, du plus récent au plus ancien) et `GET /clips/<id>`.
/// `None` si la requête n'est pas pour les clips.
pub fn route(req: &str, dir: Option<&Path>) -> Option<Reply> {
    let path = req.strip_prefix("GET ")?.split([' ', '?']).next()?;
    let rest = if path == "/clips" || path == "/clips/" {
        ""
    } else {
        path.strip_prefix("/clips/")?
    };
    let clips = dir.map(|d| d.join(DIR));
    let not_found = ("404 Not Found", "text/plain", Vec::new());
    if let Some(id) = rest.strip_suffix("/trajectory") {
        let hz = req
            .strip_prefix("GET ")
            .and_then(|r| r.split(' ').next())
            .and_then(|p| p.split_once('?'))
            .and_then(|(_, q)| q.split('&').find_map(|kv| kv.strip_prefix("hz=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        if !valid_id(id) || id == "index" || !HZ_OK.contains(&hz) {
            return Some(not_found);
        }
        return Some(match clips.and_then(|c| trajectory_json(&c, id, hz)) {
            Some(Ok(b)) => ("200 OK", "application/json", b.to_vec()),
            Some(Err(Fail::NotReady)) => ("503 Service Unavailable", "text/plain", Vec::new()),
            Some(Err(Fail::Bad)) => ("500 Internal Server Error", "text/plain", Vec::new()),
            None => not_found,
        });
    }
    Some(if rest.is_empty() {
        match clips.and_then(|c| std::fs::read(c.join(INDEX)).ok()) {
            Some(b) => ("200 OK", "application/json", b),
            None => ("200 OK", "application/json", b"[]".to_vec()),
        }
    } else {
        let id = rest.strip_suffix(".json").unwrap_or(rest);
        if !valid_id(id) || id == "index" {
            return Some(not_found);
        }
        match clips.and_then(|c| std::fs::read(c.join(format!("{id}.json"))).ok()) {
            Some(b) => ("200 OK", "application/json", b),
            None => not_found,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_env::clip::{BallFull, Ctl, Keyframe};

    fn clip(mech: &str) -> Clip {
        // Un clip minimal suffit : le magasin ne le rejoue pas (voiture absente, donc `from_json` le refuserait).
        let ball = rocketsim_rs::sim::BallState::default();
        let kf = |step| Keyframe {
            step,
            tick: 0,
            ball: BallFull(ball),
            cars: vec![],
            pads: vec![],
        };
        Clip {
            version: rl_env::clip::CLIP_VERSION,
            id: String::new(),
            mechanic: mech.into(),
            created_unix: 0,
            train_step: 0,
            tick_skip: 8,
            blue: vec![],
            trigger_step: 0,
            keyframes: vec![kf(0), kf(2)],
            steps: vec![vec![Ctl(0.0, 0.0, 0.0, 0.0, 0.0, 0)]; 2],
        }
    }

    fn store(max: usize) -> (tempfile::TempDir, ClipStore) {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open(d.path(), max, u64::MAX).unwrap();
        s.min_gap = Duration::ZERO;
        (d, s)
    }

    #[test]
    fn ring_is_bounded_and_keeps_rare_mechanics() {
        let (d, mut s) = store(5);
        let rare = s.add(clip("flip_reset"), 1).unwrap().unwrap();
        for i in 0..20 {
            s.add(clip("wave_dash"), i).unwrap();
        }
        assert_eq!(s.len(), 5);
        assert!(
            s.entries().iter().any(|e| e.id == rare.id),
            "le clip rare est gardé"
        );
        let files = std::fs::read_dir(d.path().join(DIR)).unwrap().count();
        assert_eq!(files, 5 + 1, "5 clips + index.json");
    }

    #[test]
    fn byte_cap_is_enforced() {
        let (_d, mut s) = store(100);
        let one = s.add(clip("half_flip"), 0).unwrap().unwrap().bytes;
        s.max_bytes = one * 3;
        for _ in 0..10 {
            s.add(clip("half_flip"), 0).unwrap();
        }
        assert!(
            s.bytes() <= one * 3 + 8 && s.len() <= 3,
            "{} clips, {} octets",
            s.len(),
            s.bytes()
        );
    }

    #[test]
    fn min_gap_limits_one_mechanic() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open(d.path(), 10, u64::MAX).unwrap();
        assert!(s.add(clip("flick"), 0).unwrap().is_some());
        assert!(s.add(clip("flick"), 0).unwrap().is_none());
        assert!(s.add(clip("dribble"), 0).unwrap().is_some());
    }

    #[test]
    fn reopens_with_index_and_drops_orphans() {
        let (d, mut s) = store(10);
        let e = s.add(clip("flick"), 7).unwrap().unwrap();
        std::fs::write(d.path().join(DIR).join("orphelin.json"), b"{}").unwrap();
        let s2 = ClipStore::open(d.path(), 10, u64::MAX).unwrap();
        assert_eq!(s2.entries(), [e]);
        assert!(!d.path().join(DIR).join("orphelin.json").exists());
    }

    #[test]
    fn routes_list_and_fetch_and_refuse_traversal() {
        let (d, mut s) = store(10);
        let e = s.add(clip("flick"), 3).unwrap().unwrap();
        let get = |p: &str| route(&format!("GET {p} HTTP/1.1\r\n\r\n"), Some(d.path()));
        let (st, ct, body) = get("/clips").unwrap();
        assert_eq!((st, ct), ("200 OK", "application/json"));
        let list: Vec<Entry> = serde_json::from_slice(&body).unwrap();
        assert_eq!(list, std::slice::from_ref(&e));
        let (st, _, body) = get(&format!("/clips/{}", e.id)).unwrap();
        assert_eq!(st, "200 OK");
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], e.id.as_str());
        assert_eq!(get("/clips/inconnu").unwrap().0, "404 Not Found");
        assert_eq!(
            get("/clips/..%2f..%2fcheckpoint").unwrap().0,
            "404 Not Found"
        );
        assert_eq!(get("/clips/../checkpoint").unwrap().0, "404 Not Found");
        assert!(get("/policy.json").is_none());
        assert_eq!(route("GET /clips HTTP/1.1", None).unwrap().2, b"[]");
    }

    /// Vrai clip court (deux voitures, ≈ 3 s) enregistré sur une simulation réelle. Exige les meshes.
    fn real_clip() -> Clip {
        use rl_env::clip::ClipRecorder;
        use sim_core::Controls;
        rl_env::init_meshes_from_env().expect("meshes (ROCKETSIM_MESHES_DIR)");
        let mut sim = Sim::new_soccar().unwrap();
        let (a, b) = (sim.add_car(true), sim.add_car(false));
        sim.reset_kickoff(3);
        let mut rec = ClipRecorder::new(8);
        let go = Controls {
            throttle: 1.0,
            boost: true,
            ..Controls::default()
        };
        for step in 0..24 {
            rec.begin_step(&mut sim, &[go, go]);
            sim.set_controls(a, go).unwrap();
            sim.set_controls(b, go).unwrap();
            sim.step(8);
            if step == 5 {
                rec.trigger("dribble");
            }
            if let Some(c) = rec.end_step(&mut sim) {
                return c;
            }
        }
        panic!("pas de clip");
    }

    #[test]
    fn trajectory_route_replays_caches_and_validates() {
        let (d, mut s) = store(10);
        let e = s.add(real_clip(), 9).unwrap().unwrap();
        let get = |p: &str| route(&format!("GET {p} HTTP/1.1\r\n\r\n"), Some(d.path())).unwrap();
        let (st, ct, body) = get(&format!("/clips/{}/trajectory", e.id));
        assert_eq!((st, ct), ("200 OK", "application/json"));
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["id"], e.id.as_str());
        assert_eq!(v["mechanic"], "dribble");
        assert_eq!((v["hz"].as_u64(), v["agent"].as_u64()), (Some(30), Some(0)));
        let frames = v["frames"].as_u64().unwrap() as usize;
        assert_eq!(frames as u32, e.steps * 2 + 1);
        assert_eq!(v["ball"].as_array().unwrap().len(), frames * 9);
        assert_eq!(v["cars"].as_array().unwrap().len(), 2);
        assert_eq!(v["cars"][1]["blue"], false);
        // Deuxième appel : servi par le cache, identique ; autre fréquence : autre résultat.
        assert_eq!(get(&format!("/clips/{}/trajectory", e.id)).2, body);
        let v60: serde_json::Value =
            serde_json::from_slice(&get(&format!("/clips/{}/trajectory?hz=60", e.id)).2).unwrap();
        assert_eq!(v60["frames"].as_u64().unwrap() as u32, e.steps * 4 + 1);
        for bad in [
            "/clips/inconnu/trajectory",
            "/clips/index/trajectory",
            "/clips/../x/trajectory",
        ] {
            assert_eq!(get(bad).0, "404 Not Found", "{bad}");
        }
        assert_eq!(
            get(&format!("/clips/{}/trajectory?hz=7", e.id)).0,
            "404 Not Found"
        );
    }
}
