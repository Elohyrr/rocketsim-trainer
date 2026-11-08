//! Wrapper autour de `rocketsim_rs` : chargement des meshes, création d'arène,
//! simulation et état sérialisable.
//!
//! Les meshes de collision ne sont jamais fournis par ce dépôt : `scripts/fetch-meshes.sh` les
//! télécharge, et ils sont lus depuis le dossier désigné par [`MESHES_ENV`]
//! (défaut : [`DEFAULT_MESHES_DIR`]).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rocketsim_rs::sim::{Arena, CarConfig, CarControls, Team};
use serde::{Deserialize, Serialize};

pub use rocketsim_rs::sim::CarControls as Controls;

/// Variable d'environnement donnant le dossier des meshes (contenant `soccar/mesh_0..15.cmf`).
pub const MESHES_ENV: &str = "ROCKETSIM_MESHES_DIR";
/// Dossier utilisé si [`MESHES_ENV`] est absente (relatif au dossier courant).
pub const DEFAULT_MESHES_DIR: &str = "collision_meshes";
/// Nombre de meshes soccar attendus (`mesh_0.cmf` … `mesh_15.cmf`).
pub const SOCCAR_MESH_COUNT: usize = 16;
/// Fréquence de simulation de référence (Hz).
pub const TICK_RATE: u8 = 120;

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error(
        "dossier de meshes introuvable : {0}\nLancer : scripts/fetch-meshes.sh (puis définir {MESHES_ENV})"
    )]
    MeshesDirMissing(PathBuf),
    #[error("mesh manquant : {0}\nLancer : scripts/fetch-meshes.sh (puis définir {MESHES_ENV})")]
    MeshFileMissing(PathBuf),
    #[error("RocketSim non initialisé : appeler init_meshes d'abord")]
    NotInitialized,
    #[error("voiture inconnue : {0}")]
    UnknownCar(u32),
}

/// Dossier de meshes configuré : `$ROCKETSIM_MESHES_DIR`, sinon `./collision_meshes`. Vérifié.
pub fn meshes_dir_from_env() -> Result<PathBuf, SimError> {
    let dir = std::env::var_os(MESHES_ENV)
        .map_or_else(|| PathBuf::from(DEFAULT_MESHES_DIR), PathBuf::from);
    check_meshes_dir(&dir)?;
    Ok(dir)
}

/// Vérifie la présence des 16 meshes. Indispensable avant `init` : RocketSim plante (abort C++)
/// si un fichier est illisible, et se contente d'ignorer les fichiers absents.
pub fn check_meshes_dir(dir: &Path) -> Result<(), SimError> {
    let soccar = dir.join("soccar");
    if !soccar.is_dir() {
        return Err(SimError::MeshesDirMissing(soccar));
    }
    for i in 0..SOCCAR_MESH_COUNT {
        let f = soccar.join(format!("mesh_{i}.cmf"));
        if !f.is_file() {
            return Err(SimError::MeshFileMissing(f));
        }
    }
    Ok(())
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
            return Err(SimError::NotInitialized);
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

    /// Place la balle (position et vitesse, unités RocketSim) en gardant le reste de son état.
    pub fn set_ball(&mut self, pos: V3, vel: V3) {
        let mut b = self.arena.pin_mut().get_ball();
        b.pos = rocketsim_rs::math::Vec3::new(pos[0], pos[1], pos[2]);
        b.vel = rocketsim_rs::math::Vec3::new(vel[0], vel[1], vel[2]);
        self.arena.pin_mut().set_ball(b);
    }

    /// Remet une voiture dans un état « au sol » : position, cap (yaw, radians), vitesse, boost (0–100).
    pub fn reset_car(
        &mut self,
        car: u32,
        pos: V3,
        yaw: f32,
        vel: V3,
        boost: f32,
    ) -> Result<(), SimError> {
        let mut st = rocketsim_rs::sim::CarState::default();
        let (s, c) = yaw.sin_cos();
        st.pos = rocketsim_rs::math::Vec3::new(pos[0], pos[1], pos[2]);
        st.vel = rocketsim_rs::math::Vec3::new(vel[0], vel[1], vel[2]);
        st.rot_mat.forward = rocketsim_rs::math::Vec3::new(c, s, 0.0);
        st.rot_mat.right = rocketsim_rs::math::Vec3::new(-s, c, 0.0);
        st.rot_mat.up = rocketsim_rs::math::Vec3::new(0.0, 0.0, 1.0);
        st.boost = boost;
        self.arena
            .pin_mut()
            .set_car(car, st)
            .map_err(|_| SimError::UnknownCar(car))
    }

    /// Tick du dernier toucher de la balle par cette voiture, s'il y en a eu un depuis le dernier reset.
    pub fn last_ball_touch_tick(&mut self, car: u32) -> Option<u64> {
        let h = self.arena.pin_mut().get_car(car).ball_hit_info;
        h.is_valid.then_some(h.tick_count_when_hit)
    }

    /// La balle est-elle dans un but ?
    pub fn is_ball_scored(&self) -> bool {
        self.arena.is_ball_scored()
    }

    /// Remet la balle au repos, vitesse angulaire nulle.
    pub fn reset_ball(&mut self, pos: V3, vel: V3) {
        let b = rocketsim_rs::sim::BallState {
            pos: rocketsim_rs::math::Vec3::new(pos[0], pos[1], pos[2]),
            vel: rocketsim_rs::math::Vec3::new(vel[0], vel[1], vel[2]),
            ..Default::default()
        };
        self.arena.pin_mut().set_ball(b);
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
    fn incomplete_soccar_dir_names_missing_mesh() {
        let d = std::env::temp_dir().join(format!("sim-core-empty-{}", std::process::id()));
        std::fs::create_dir_all(d.join("soccar")).unwrap();
        let err = check_meshes_dir(&d).unwrap_err();
        std::fs::remove_dir_all(&d).ok();
        assert!(matches!(err, SimError::MeshFileMissing(_)));
        assert!(err.to_string().contains("scripts/fetch-meshes.sh"));
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

    /// Exige les vrais meshes : `scripts/fetch-meshes.sh` puis `cargo test` (CI : toujours exécuté).
    #[test]
    fn simulates_1800_ticks_with_meshes() {
        let dir = meshes_dir_from_env().unwrap_or_else(|e| panic!("{e}"));
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

    /// La balle lancée vers le mur latéral (x = 4096) doit rebondir : prouve que la géométrie est chargée.
    #[test]
    fn ball_bounces_off_side_wall() {
        let dir = meshes_dir_from_env().unwrap_or_else(|e| panic!("{e}"));
        init_meshes(&dir).unwrap();
        let mut sim = Sim::new_soccar().unwrap();
        sim.set_ball([3500., 0., 500.], [2000., 0., 0.]);
        sim.step(120);
        let b = sim.snapshot().ball;
        assert!(
            b.vel[0] < 0.,
            "la balle aurait dû repartir vers le centre : {:?}",
            b.vel
        );
        assert!(
            b.pos[0] < 4096.,
            "la balle ne doit pas traverser le mur : {:?}",
            b.pos
        );
    }
}
