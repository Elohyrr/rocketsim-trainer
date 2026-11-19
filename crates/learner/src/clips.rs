//! Anneau de clips rejouables sur le disque (PVC) : `<out>/clips/<id>.json` + `index.json`.
//!
//! Borné en nombre et en octets ; quand il est plein, on retire le plus ancien clip de la mécanique la plus
//! représentée (les mécaniques rares restent). Un délai minimal par mécanique évite d'inonder l'anneau avec la
//! même action répétée. Les meilleurs clips (score de qualité) sont en plus copiés dans `clips/top/` (« hall of
//! fame ») : ils survivent à l'éviction de l'anneau et aux redémarrages. Le serveur HTTP du learner (`metrics.rs`) sert la liste et les fichiers en lecture seule.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rl_env::clip::{Clip, Trajectory};
use serde::{Deserialize, Serialize};
use sim_core::clip::{ClipDetail, ClipLabel, ClipQuality};
use sim_core::{Sim, SimError};

pub const DIR: &str = "clips";
pub const INDEX: &str = "index.json";
/// Marqueur posé au premier démarrage avec le détecteur v2 : les clips d'avant (v1, surtout du bruit) sont alors
/// supprimés une seule fois.
pub const V2_MARKER: &str = "detecteur-v2";
pub const DEFAULT_MAX_CLIPS: usize = 200;
pub const DEFAULT_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Sous-dossier du hall of fame (`clips/top/<id>.json` + `index.json`).
pub const TOP_DIR: &str = "top";
/// Nombre de clips du classement général (`--clips-top`).
pub const DEFAULT_TOP: usize = 10;
/// Meilleurs clips gardés en plus pour chaque mécanique.
pub const TOP_PER_MECHANIC: usize = 3;
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
    /// Version du détecteur qui a déclenché le clip (1 = règles d'origine, 2 = strictes) : la page web filtre.
    #[serde(default = "v1")]
    pub detector: u32,
    /// Qualité mesurée après la mécanique (absente des clips v1).
    #[serde(default)]
    pub quality: Option<ClipQuality>,
    /// Valeur et support de la mécanique (vitesse de balle d'un pinch, son support…).
    #[serde(default)]
    pub detail: Option<ClipDetail>,
    /// Toutes les mécaniques validées dans la fenêtre du clip, avec leur instant (vide pour les anciens clips :
    /// l'étiquette est alors `mechanic` seule). `mechanic` reste l'étiquette principale.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mechanics: Vec<ClipLabel>,
    /// Enchaînement de mécaniques de familles distinctes en moins de 2 s.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub combo: bool,
}

impl Entry {
    /// Le clip porte-t-il cette étiquette (principale ou secondaire) ?
    pub fn has_label(&self, m: &str) -> bool {
        self.mechanic == m || self.mechanics.iter().any(|l| l.mechanic == m)
    }

    /// Toutes les étiquettes distinctes (la principale d'abord).
    pub fn label_names(&self) -> Vec<&str> {
        let mut v = vec![self.mechanic.as_str()];
        for l in &self.mechanics {
            if !v.contains(&l.mechanic.as_str()) {
                v.push(&l.mechanic);
            }
        }
        v
    }
}

fn v1() -> u32 {
    1
}

/// `top/index.json` : classement général (meilleur en premier) et meilleurs clips de chaque mécanique.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TopIndex {
    pub global: Vec<Entry>,
    pub mechanics: BTreeMap<String, Vec<Entry>>,
}

/// Ordre du classement : score de qualité décroissant, puis vitesse de balle, puis id (déterministe).
fn rank_cmp(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    let k = |e: &Entry| {
        e.quality
            .map_or((f32::MIN, f32::MIN), |q| (q.score, q.ball_speed_to_goal))
    };
    let (ka, kb) = (k(a), k(b));
    kb.0.total_cmp(&ka.0)
        .then(kb.1.total_cmp(&ka.1))
        .then_with(|| a.id.cmp(&b.id))
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
    /// Taille du classement général (0 = pas de hall of fame) et meilleurs par mécanique.
    pub top_n: usize,
    pub top_per_mechanic: usize,
    top: TopIndex,
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
        Self::open_for(out, max_clips, max_bytes, false)
    }

    /// Comme [`ClipStore::open`] ; avec `purge_v1`, vide l'anneau une seule fois (marqueur [`V2_MARKER`]) au
    /// premier démarrage avec le détecteur v2.
    pub fn open_for(
        out: &Path,
        max_clips: usize,
        max_bytes: u64,
        purge_v1: bool,
    ) -> std::io::Result<Self> {
        Self::open_with(out, max_clips, max_bytes, purge_v1, DEFAULT_TOP)
    }

    /// Comme [`ClipStore::open_for`], avec `top_n` clips au classement général (0 : hall of fame coupé).
    pub fn open_with(
        out: &Path,
        max_clips: usize,
        max_bytes: u64,
        purge_v1: bool,
        top_n: usize,
    ) -> std::io::Result<Self> {
        let dir = out.join(DIR);
        std::fs::create_dir_all(&dir)?;
        if purge_v1 && !dir.join(V2_MARKER).exists() {
            for f in std::fs::read_dir(&dir)?.flatten() {
                if f.path().extension().is_some_and(|e| e == "json") {
                    let _ = std::fs::remove_file(f.path());
                }
            }
            write_atomic(&dir.join(V2_MARKER), b"2\n")?;
        }
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
                    || name == V2_MARKER
                    || name == TOP_DIR
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
            top_n,
            top_per_mechanic: TOP_PER_MECHANIC,
            top: TopIndex::default(),
        };
        s.load_top();
        s.evict();
        s.rebuild_top()?;
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
            detector: clip.detector.max(1),
            quality: clip.quality,
            detail: clip.detail.clone(),
            mechanics: clip.mechanics.clone(),
            combo: clip.combo,
        };
        self.last.insert(clip.mechanic, now);
        self.entries.push(e.clone());
        if e.quality.is_some() {
            self.rebuild_top()?;
        }
        self.evict();
        self.write_index()?;
        Ok(Some(e))
    }

    /// Classement général du hall of fame (meilleur en premier).
    pub fn top(&self) -> &[Entry] {
        &self.top.global
    }

    /// Plus bas score du classement général (0 s'il est vide).
    pub fn top_min_score(&self) -> f32 {
        self.top
            .global
            .last()
            .and_then(|e| e.quality)
            .map_or(0.0, |q| q.score)
    }

    fn top_dir(&self) -> PathBuf {
        self.dir.join(TOP_DIR)
    }

    /// Reprend `top/index.json` (entrées sans fichier ignorées).
    fn load_top(&mut self) {
        let td = self.top_dir();
        let mut t: TopIndex = std::fs::read(td.join(INDEX))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let ok = |e: &Entry| valid_id(&e.id) && td.join(format!("{}.json", e.id)).is_file();
        t.global.retain(ok);
        for v in t.mechanics.values_mut() {
            v.retain(ok);
        }
        self.top = t;
    }

    /// Recalcule le hall of fame : candidats = hall actuel + clips de l'anneau avec qualité ; garde les `top_n`
    /// meilleurs et les `top_per_mechanic` meilleurs de chaque mécanique, copie les fichiers manquants dans
    /// `top/` et supprime ceux qui n'y ont plus leur place. Les clips v1 (sans qualité) sont exclus.
    fn rebuild_top(&mut self) -> std::io::Result<()> {
        if self.top_n == 0 {
            return Ok(());
        }
        let td = self.top_dir();
        std::fs::create_dir_all(&td)?;
        let mut cands: HashMap<String, Entry> = HashMap::new();
        for e in self
            .top
            .global
            .iter()
            .chain(self.top.mechanics.values().flatten())
            .chain(self.entries.iter())
        {
            let has_file = td.join(format!("{}.json", e.id)).is_file()
                || self.dir.join(format!("{}.json", e.id)).is_file();
            if e.quality.is_some() && has_file {
                cands.entry(e.id.clone()).or_insert_with(|| e.clone());
            }
        }
        let mut ranked: Vec<Entry> = cands.into_values().collect();
        ranked.sort_by(rank_cmp);
        let mut t = TopIndex {
            global: ranked.iter().take(self.top_n).cloned().collect(),
            mechanics: BTreeMap::new(),
        };
        // Un clip à plusieurs étiquettes concourt dans chacune de ses mécaniques.
        for e in &ranked {
            for name in e.label_names() {
                let v = t.mechanics.entry(name.to_string()).or_default();
                if v.len() < self.top_per_mechanic {
                    v.push(e.clone());
                }
            }
        }
        let keep: HashSet<&str> = t
            .global
            .iter()
            .chain(t.mechanics.values().flatten())
            .map(|e| e.id.as_str())
            .collect();
        for id in &keep {
            let to = td.join(format!("{id}.json"));
            if !to.is_file() {
                let tmp = to.with_extension("tmp");
                std::fs::copy(self.dir.join(format!("{id}.json")), &tmp)?;
                std::fs::rename(&tmp, &to)?;
            }
        }
        for f in std::fs::read_dir(&td)?.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let stale = match name.strip_suffix(".json") {
                Some("index") => false,
                Some(id) => !keep.contains(id),
                None => true,
            };
            if stale {
                let _ = std::fs::remove_file(f.path());
            }
        }
        write_atomic(
            &td.join(INDEX),
            &serde_json::to_vec(&t).expect("index sérialisable"),
        )?;
        self.top = t;
        Ok(())
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

/// Fichier d'un clip : l'anneau, sinon le hall of fame (le clip a pu quitter l'anneau).
fn clip_path(clips: &Path, id: &str) -> Option<PathBuf> {
    [
        clips.join(format!("{id}.json")),
        clips.join(TOP_DIR).join(format!("{id}.json")),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Valeur du paramètre `key` de la requête.
fn query<'a>(req: &'a str, key: &str) -> Option<&'a str> {
    req.strip_prefix("GET ")?
        .split(' ')
        .next()?
        .split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
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
    /// Étiquettes (mécaniques validées dans le clip) et instant de chacune ; `combo` si enchaînement.
    labels: Vec<LabelAt>,
    combo: bool,
    agent: usize,
    #[serde(flatten)]
    t: &'a Trajectory,
}

#[derive(Serialize)]
struct LabelAt {
    mechanic: String,
    t: f32,
}

/// JSON de la trajectoire de `<clips>/<id>.json` à `hz`, d'après le cache ou un rejeu à la demande.
/// `None` si le clip n'existe pas.
fn trajectory_json(clips: &Path, id: &str, hz: u32) -> Option<Result<Arc<[u8]>, Fail>> {
    let file = std::fs::read(clip_path(clips, id)?).ok()?;
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
        let combo = clip.combo;
        let labels: Vec<LabelAt> = if clip.mechanics.is_empty() {
            vec![LabelAt {
                mechanic: clip.mechanic.clone(),
                t: clip.trigger_step as f32 * clip.tick_skip as f32 / sim_core::TICK_RATE as f32,
            }]
        } else {
            clip.mechanics
                .iter()
                .map(|l| LabelAt {
                    mechanic: l.mechanic.clone(),
                    t: l.t,
                })
                .collect()
        };
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
            labels,
            combo,
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

/// Routes en lecture seule : `GET /clips` (index, du plus récent au plus ancien), `GET /clips?top=1` ou
/// `GET /clips/top` (hall of fame ; `&mechanic=` pour une mécanique) et `GET /clips/<id>` (anneau puis hall).
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
        let hz = query(req, "hz").and_then(|v| v.parse().ok()).unwrap_or(30);
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
    let top = rest == "top" || (rest.is_empty() && query(req, "top").is_some_and(|v| v != "0"));
    Some(if top {
        // Hall of fame : classement général, ou meilleurs d'une mécanique (`?mechanic=`).
        let t: TopIndex = clips
            .and_then(|c| std::fs::read(c.join(TOP_DIR).join(INDEX)).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let list = match query(req, "mechanic") {
            Some(m) => t.mechanics.get(m).cloned().unwrap_or_default(),
            None => t.global,
        };
        (
            "200 OK",
            "application/json",
            serde_json::to_vec(&list).expect("sérialisable"),
        )
    } else if rest.is_empty() {
        match clips.and_then(|c| std::fs::read(c.join(INDEX)).ok()) {
            Some(b) => ("200 OK", "application/json", b),
            None => ("200 OK", "application/json", b"[]".to_vec()),
        }
    } else {
        let id = rest.strip_suffix(".json").unwrap_or(rest);
        if !valid_id(id) || id == "index" {
            return Some(not_found);
        }
        match clips.and_then(|c| std::fs::read(clip_path(&c, id)?).ok()) {
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
            detector: 2,
            quality: None,
            detail: None,
            mechanics: vec![],
            combo: false,
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
        let files = std::fs::read_dir(d.path().join(DIR))
            .unwrap()
            .flatten()
            .filter(|f| f.path().is_file())
            .count();
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
    fn first_v2_start_purges_ring_once() {
        let (d, mut s) = store(10);
        s.add(clip("wall_shot"), 1).unwrap().unwrap();
        // Mode v1 : rien n'est supprimé, pas de marqueur.
        let s1 = ClipStore::open_for(d.path(), 10, u64::MAX, false).unwrap();
        assert_eq!(s1.len(), 1);
        assert!(!d.path().join(DIR).join(V2_MARKER).exists());
        // Premier démarrage v2 : anneau vidé, marqueur posé.
        let mut s2 = ClipStore::open_for(d.path(), 10, u64::MAX, true).unwrap();
        assert!(s2.is_empty());
        assert!(d.path().join(DIR).join(V2_MARKER).exists());
        s2.min_gap = Duration::ZERO;
        let mut c = clip("dribble");
        c.quality = Some(ClipQuality {
            ball_speed_to_goal: 2000.0,
            goal_3s: true,
            score: 0.83,
        });
        let e = s2.add(c, 2).unwrap().unwrap();
        assert_eq!(e.detector, 2);
        assert_eq!(e.quality.map(|q| q.goal_3s), Some(true));
        // Démarrages suivants : les clips v2 survivent (le marqueur n'est pas pris pour un orphelin).
        let s3 = ClipStore::open_for(d.path(), 10, u64::MAX, true).unwrap();
        assert_eq!(s3.entries(), [e]);
        assert!(d.path().join(DIR).join(V2_MARKER).exists());
    }

    #[test]
    fn old_index_entries_read_as_v1() {
        let e: Entry = serde_json::from_str(
            r#"{"id":"1-wall_shot-1","mechanic":"wall_shot","created_unix":1,"train_step":2,"steps":3,"seconds":1.0,"bytes":10}"#,
        )
        .unwrap();
        assert_eq!((e.detector, e.quality), (1, None));
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

    fn scored(mech: &str, score: f32, speed: f32) -> Clip {
        let mut c = clip(mech);
        c.quality = Some(ClipQuality {
            ball_speed_to_goal: speed,
            goal_3s: score > 0.5,
            score,
        });
        c
    }

    fn top_ids(s: &ClipStore) -> Vec<String> {
        s.top().iter().map(|e| e.id.clone()).collect()
    }

    #[test]
    fn hall_ranks_by_score_then_speed_and_excludes_v1() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open_with(d.path(), 100, u64::MAX, false, 3).unwrap();
        s.min_gap = Duration::ZERO;
        s.add(clip("flick"), 0).unwrap(); // sans qualité
        let a = s.add(scored("dribble", 0.5, 100.0), 0).unwrap().unwrap();
        let b = s.add(scored("dribble", 0.9, 100.0), 0).unwrap().unwrap();
        let c = s.add(scored("dribble", 0.9, 300.0), 0).unwrap().unwrap();
        assert_eq!(top_ids(&s), [c.id.clone(), b.id.clone(), a.id.clone()]);
        assert_eq!(s.top_min_score(), 0.5);
    }

    #[test]
    fn better_clip_replaces_tenth_and_survives_ring_eviction() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open_with(d.path(), 2, u64::MAX, false, 2).unwrap();
        s.min_gap = Duration::ZERO;
        s.top_per_mechanic = 0;
        let best = s.add(scored("a", 0.9, 1.0), 0).unwrap().unwrap();
        let mid = s.add(scored("b", 0.5, 1.0), 0).unwrap().unwrap();
        let low = s.add(scored("c", 0.1, 1.0), 0).unwrap().unwrap();
        assert_eq!(top_ids(&s), [best.id.clone(), mid.id.clone()]);
        let top = d.path().join(DIR).join(TOP_DIR);
        assert!(!top.join(format!("{}.json", low.id)).exists());
        // Le meilleur est évincé de l'anneau (2 places) mais reste servi.
        for _ in 0..4 {
            s.add(scored("d", 0.2, 1.0), 0).unwrap();
        }
        assert!(!s.entries().iter().any(|e| e.id == best.id));
        let get = |p: &str| route(&format!("GET {p} HTTP/1.1\r\n\r\n"), Some(d.path())).unwrap();
        assert_eq!(get(&format!("/clips/{}", best.id)).0, "200 OK");
        // Un meilleur clip chasse le dernier.
        let new = s.add(scored("e", 0.7, 1.0), 0).unwrap().unwrap();
        assert_eq!(top_ids(&s), [best.id.clone(), new.id.clone()]);
        assert_eq!(get(&format!("/clips/{}", mid.id)).0, "404 Not Found");
    }

    #[test]
    fn hall_persists_across_restart_and_keeps_best_per_mechanic() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open_with(d.path(), 100, u64::MAX, false, 1).unwrap();
        s.min_gap = Duration::ZERO;
        for i in 0..5 {
            s.add(scored("dribble", 0.9 - i as f32 * 0.1, 1.0), 0)
                .unwrap();
        }
        let first = s.top()[0].clone();
        let per = s.top.mechanics["dribble"].len();
        assert_eq!(per, 3);
        drop(s);
        // Anneau perdu : seul le hall survit.
        for f in std::fs::read_dir(d.path().join(DIR)).unwrap().flatten() {
            if f.path().extension().is_some_and(|e| e == "json") {
                std::fs::remove_file(f.path()).unwrap();
            }
        }
        let s2 = ClipStore::open_with(d.path(), 100, u64::MAX, false, 1).unwrap();
        assert_eq!(s2.top(), [first]);
        assert_eq!(s2.top.mechanics["dribble"].len(), 3);
    }

    #[test]
    fn top_routes() {
        let d = tempfile::tempdir().unwrap();
        let mut s = ClipStore::open_with(d.path(), 100, u64::MAX, false, 2).unwrap();
        s.min_gap = Duration::ZERO;
        let a = s.add(scored("flick", 0.4, 1.0), 0).unwrap().unwrap();
        let b = s.add(scored("dribble", 0.8, 1.0), 0).unwrap().unwrap();
        let get = |p: &str| route(&format!("GET {p} HTTP/1.1\r\n\r\n"), Some(d.path())).unwrap();
        for p in ["/clips?top=1", "/clips/top"] {
            let (st, ct, body) = get(p);
            assert_eq!((st, ct), ("200 OK", "application/json"));
            let l: Vec<Entry> = serde_json::from_slice(&body).unwrap();
            assert_eq!(l, [b.clone(), a.clone()], "{p}");
        }
        let l: Vec<Entry> = serde_json::from_slice(&get("/clips/top?mechanic=flick").2).unwrap();
        assert_eq!(l, [a]);
        assert_eq!(get("/clips").0, "200 OK");
        let all: Vec<Entry> = serde_json::from_slice(&get("/clips").2).unwrap();
        assert_eq!(all.len(), 2);
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
        assert_eq!(v["cars"][0]["boost"].as_array().unwrap().len(), frames);
        assert_eq!(v["ball_vel"].as_array().unwrap().len(), frames * 3);
        assert_eq!(v["ball_ang"].as_array().unwrap().len(), frames * 3);
        assert_eq!(v["cars"][0]["vel"].as_array().unwrap().len(), frames * 3);
        assert_eq!(v["cars"][1]["ang"].as_array().unwrap().len(), frames * 3);
        assert_eq!(v["pads"]["big"].as_array().unwrap().len(), 34);
        assert_eq!(v["pads"]["pos"].as_array().unwrap().len(), 34 * 3);
        assert_eq!(v["pads"]["active0"].as_array().unwrap().len(), 34);
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
