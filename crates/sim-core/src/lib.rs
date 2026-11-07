//! Wrapper autour de `rocketsim_rs` : chargement des meshes, création d'arène,
//! simulation et état sérialisable.
//!
//! Les meshes de collision ne sont jamais fournis par ce dépôt : ils sont lus depuis
//! le dossier désigné par la variable d'environnement [`MESHES_ENV`].

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rocketsim_rs::sim::{Arena, CarConfig, CarControls, Team};
use serde::{Deserialize, Serialize};

pub use rocketsim_rs::sim::CarControls as Controls;

/// Variable d'environnement donnant le dossier des meshes (contenant `soccar/*.cco`).
pub const MESHES_ENV: &str = "ROCKETSIM_MESHES_DIR";
/// Fréquence de simulation de référence (Hz).
pub const TICK_RATE: u8 = 120;

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("variable d'environnement {MESHES_ENV} absente")]
    MeshesEnvMissing,
    #[error("dossier de meshes introuvable : {0}")]
    MeshesDirMissing(PathBuf),
    #[error("aucun fichier .cco dans {0} (attendu : <dossier>/soccar/*.cco)")]
    NoMeshFiles(PathBuf),
    #[error("voiture inconnue : {0}")]
    UnknownCar(u32),
}

/// Dossier de meshes configuré, s'il existe et contient des `.cco`. `None` sinon (tests sautés).
pub fn meshes_dir_from_env() -> Result<PathBuf, SimError> {
    let dir = std::env::var_os(MESHES_ENV).ok_or(SimError::MeshesEnvMissing)?;
    check_meshes_dir(Path::new(&dir))?;
    Ok(PathBuf::from(dir))
}

/// Vérifie la présence des meshes. Indispensable avant `init` : RocketSim lève une
/// exception C++ (abort côté Rust) si l'initialisation échoue.
pub fn check_meshes_dir(dir: &Path) -> Result<(), SimError> {
    let soccar = dir.join("soccar");
    if !soccar.is_dir() {
        return Err(SimError::MeshesDirMissing(soccar));
    }
    let has_cco = std::fs::read_dir(&soccar)
        .map(|it| {
            it.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "cco"))
        })
        .unwrap_or(false);
    if has_cco {
        Ok(())
    } else {
        Err(SimError::NoMeshFiles(soccar))
    }
}

static INIT: OnceLock<PathBuf> = OnceLock::new();

/// Initialise RocketSim une seule fois par processus (état global côté C++).
pub fn init_meshes(dir: &Path) -> Result<(), SimError> {
    check_meshes_dir(dir)?;
    INIT.get_or_init(|| {
        rocketsim_rs::init(dir.to_str(), true);
        dir.to_path_buf()
    });
    Ok(())
}

/// Vecteur 3D sérialisable (unités RocketSim : unreal units).
pub type V3 = [f32; 3];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BallSnapshot {
    pub pos: V3,
    pub vel: V3,
    pub ang_vel: V3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarSnapshot {
    pub id: u32,
    pub blue: bool,
    pub pos: V3,
    pub vel: V3,
    pub ang_vel: V3,
    /// Axes avant / droite / haut de la matrice de rotation.
    pub forward: V3,
    pub right: V3,
    pub up: V3,
    pub boost: f32,
    pub on_ground: bool,
    pub has_flip_or_jump: bool,
}

/// État complet observable de la simulation (JSON/bincode-friendly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    pub ball: BallSnapshot,
    pub cars: Vec<CarSnapshot>,
}

fn v(x: rocketsim_rs::math::Vec3) -> V3 {
    [x.x, x.y, x.z]
}

/// Simulation soccar à 120 Hz.
pub struct Sim {
    arena: cxx::UniquePtr<Arena>,
}

impl Sim {
    /// Crée une arène soccar. `init_meshes` doit avoir réussi avant.
    pub fn new_soccar() -> Result<Self, SimError> {
        if INIT.get().is_none() {
            return Err(SimError::MeshesEnvMissing);
        }
        Ok(Self {
            arena: Arena::default_standard(),
        })
    }

    pub fn add_car(&mut self, blue: bool) -> u32 {
        let team = if blue { Team::Blue } else { Team::Orange };
        self.arena.pin_mut().add_car(team, CarConfig::octane())
    }

    pub fn set_controls(&mut self, car: u32, c: CarControls) -> Result<(), SimError> {
        self.arena
            .pin_mut()
            .set_car_controls(car, c)
            .map_err(|_| SimError::UnknownCar(car))
    }

    pub fn step(&mut self, ticks: u32) {
        self.arena.pin_mut().step(ticks);
    }

    pub fn tick_count(&self) -> u64 {
        self.arena.get_tick_count()
    }

    pub fn snapshot(&mut self) -> Snapshot {
        let ids = self.arena.get_cars();
        let b = self.arena.pin_mut().get_ball();
        let mut cars = Vec::with_capacity(ids.len());
        for id in ids {
            let blue = self.arena.get_car_team(id) == Team::Blue;
            let c = self.arena.pin_mut().get_car(id);
            cars.push(CarSnapshot {
                id,
                blue,
                pos: v(c.pos),
                vel: v(c.vel),
                ang_vel: v(c.ang_vel),
                forward: v(c.rot_mat.forward),
                right: v(c.rot_mat.right),
                up: v(c.rot_mat.up),
                boost: c.boost,
                on_ground: c.is_on_ground,
                has_flip_or_jump: c.has_flip_or_jump(),
            });
        }
        Snapshot {
            tick: self.arena.get_tick_count(),
            ball: BallSnapshot {
                pos: v(b.pos),
                vel: v(b.vel),
                ang_vel: v(b.ang_vel),
            },
            cars,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_dir_is_reported_not_aborted() {
        let err = check_meshes_dir(Path::new("/nonexistent/xyz")).unwrap_err();
        assert!(matches!(err, SimError::MeshesDirMissing(_)));
    }

    #[test]
    fn empty_soccar_dir_has_no_meshes() {
        let d = std::env::temp_dir().join(format!("sim-core-empty-{}", std::process::id()));
        std::fs::create_dir_all(d.join("soccar")).unwrap();
        let err = check_meshes_dir(&d).unwrap_err();
        std::fs::remove_dir_all(&d).ok();
        assert!(matches!(err, SimError::NoMeshFiles(_)));
    }

    #[test]
    fn new_without_init_fails_cleanly() {
        if INIT.get().is_none() {
            assert!(Sim::new_soccar().is_err());
        }
    }

    #[test]
    fn snapshot_roundtrips_json() {
        let s = Snapshot {
            tick: 3,
            ball: BallSnapshot {
                pos: [0., 0., 93.],
                vel: [0.; 3],
                ang_vel: [0.; 3],
            },
            cars: vec![],
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Snapshot>(&j).unwrap(), s);
    }

    /// Exige les meshes : `ROCKETSIM_MESHES_DIR=… cargo test -p sim-core -- --ignored`.
    #[test]
    #[ignore = "nécessite les meshes (ROCKETSIM_MESHES_DIR)"]
    fn simulates_1800_ticks_with_meshes() {
        let dir = meshes_dir_from_env().expect("meshes requis pour ce test");
        init_meshes(&dir).unwrap();
        let mut sim = Sim::new_soccar().unwrap();
        let car = sim.add_car(true);
        sim.set_controls(
            car,
            CarControls {
                throttle: 1.0,
                boost: true,
                ..Default::default()
            },
        )
        .unwrap();
        let before = sim.snapshot();
        sim.step(1800);
        let after = sim.snapshot();
        assert_eq!(after.tick - before.tick, 1800);
        assert_ne!(before.cars[0].pos, after.cars[0].pos);
        let j = serde_json::to_string(&after).unwrap();
        assert_eq!(serde_json::from_str::<Snapshot>(&j).unwrap(), after);
    }
}
