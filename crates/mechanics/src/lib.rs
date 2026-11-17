//! Détecteur de mécaniques de Rocket League, tick par tick, **sans récompense**.
//!
//! Un [`Detector`] suit une voiture : à chaque tick de physique (120 Hz), il reçoit l'état de la voiture
//! ([`CarTick`]) et de la balle ([`BallTick`]), en tire des événements discrets ([`Events`] : sol/air, saut,
//! dodge, annulation, toucher, reset…) puis fait avancer un petit automate par mécanique du catalogue
//! ([`Mechanic::ALL`]). Chaque occurrence reconnue incrémente **une fois** le compteur correspondant de
//! [`Counts`]. Aucune allocation par tick.
//!
//! Unités RocketSim (uu, uu/s). Seuils : module [`thresholds`]. Séquences synthétiques rejouables (tests,
//! autres crates) : module [`script`]. Documentation : `docs/mechanics.md`.

mod detector;
pub mod script;
pub mod thresholds;

pub use detector::{Detector, Events};

/// Fréquence de la physique (ticks par seconde).
pub const TICK_HZ: f32 = 120.0;

/// Catalogue fermé des mécaniques détectées (12, ordre de [`Mechanic::ALL`] = index des compteurs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mechanic {
    Touch,
    AerialTouch,
    AirDribble,
    FlipReset,
    CeilingShot,
    WallShot,
    DoubleTouch,
    Flick,
    Dribble,
    SpeedFlip,
    WaveDash,
    HalfFlip,
}

impl Mechanic {
    pub const ALL: [Mechanic; 12] = [
        Mechanic::Touch,
        Mechanic::AerialTouch,
        Mechanic::AirDribble,
        Mechanic::FlipReset,
        Mechanic::CeilingShot,
        Mechanic::WallShot,
        Mechanic::DoubleTouch,
        Mechanic::Flick,
        Mechanic::Dribble,
        Mechanic::SpeedFlip,
        Mechanic::WaveDash,
        Mechanic::HalfFlip,
    ];

    /// Valeur du label Prometheus `mechanic`.
    pub fn label(self) -> &'static str {
        match self {
            Mechanic::Touch => "touch",
            Mechanic::AerialTouch => "aerial_touch",
            Mechanic::AirDribble => "air_dribble",
            Mechanic::FlipReset => "flip_reset",
            Mechanic::CeilingShot => "ceiling_shot",
            Mechanic::WallShot => "wall_shot",
            Mechanic::DoubleTouch => "double_touch",
            Mechanic::Flick => "flick",
            Mechanic::Dribble => "dribble",
            Mechanic::SpeedFlip => "speed_flip",
            Mechanic::WaveDash => "wave_dash",
            Mechanic::HalfFlip => "half_flip",
        }
    }

    /// Position dans [`Mechanic::ALL`] et dans [`Counts`].
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }
}

/// État d'une voiture à un tick (champs tirés de `CarState` de RocketSim).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarTick {
    pub tick: u64,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub ang_vel: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    /// >= 3 roues en contact (`is_on_ground` de RocketSim).
    pub on_ground: bool,
    /// 0..=4.
    pub wheels_contact: u8,
    pub has_jumped: bool,
    pub has_double_jumped: bool,
    pub has_flipped: bool,
    pub is_flipping: bool,
    /// Vrai si un saut ou un flip est encore disponible.
    pub has_flip_or_jump: bool,
    /// Direction du dodge dans le repère voiture : `[0]` = composante avant (+ = dodge avant, − = arrière),
    /// `[1]` = composante latérale (+ = droite). Nul hors dodge.
    pub flip_rel_torque: [f32; 3],
    pub boosting: bool,
    pub supersonic: bool,
    /// Contact du châssis avec le terrain (mur/sol/plafond), pas la balle.
    pub world_contact: bool,
    /// Commandes du tick, -1..1 (tangage : −1 = nez vers le bas, comme RocketSim).
    pub pitch_input: f32,
    pub roll_input: f32,
    /// Tick du dernier toucher balle de cette voiture (None si jamais). Ne change pas quand seules les roues
    /// touchent la balle.
    pub ball_hit_tick: Option<u64>,
    /// Temps en l'air (s), utilisé seulement pour initialiser le détecteur après un `reset`.
    pub air_time: f32,
}

/// État de la balle à un tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallTick {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
}

/// Compteurs par mécanique (index = [`Mechanic::index`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts(pub [u64; 12]);

impl Counts {
    #[inline]
    pub fn get(&self, m: Mechanic) -> u64 {
        self.0[m.index()]
    }

    #[inline]
    pub fn inc(&mut self, m: Mechanic) {
        self.0[m.index()] += 1;
    }

    pub fn add(&mut self, other: &Counts) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a += *b;
        }
    }

    pub fn total(&self) -> u64 {
        self.0.iter().sum()
    }

    /// Compteurs non nuls, sous forme `(mécanique, nombre)` (pour les messages de test et les logs).
    pub fn nonzero(&self) -> Vec<(Mechanic, u64)> {
        Mechanic::ALL
            .iter()
            .filter(|m| self.get(**m) > 0)
            .map(|m| (*m, self.get(*m)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_ordre_et_labels() {
        for (i, m) in Mechanic::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
        }
        let labels: Vec<_> = Mechanic::ALL.iter().map(|m| m.label()).collect();
        assert_eq!(
            labels,
            [
                "touch",
                "aerial_touch",
                "air_dribble",
                "flip_reset",
                "ceiling_shot",
                "wall_shot",
                "double_touch",
                "flick",
                "dribble",
                "speed_flip",
                "wave_dash",
                "half_flip"
            ]
        );
    }

    #[test]
    fn counts_add_total() {
        let mut a = Counts::default();
        a.inc(Mechanic::Flick);
        let mut b = Counts::default();
        b.inc(Mechanic::Flick);
        b.inc(Mechanic::Touch);
        a.add(&b);
        assert_eq!(a.get(Mechanic::Flick), 2);
        assert_eq!(a.total(), 3);
        assert_eq!(
            a.nonzero(),
            vec![(Mechanic::Touch, 1), (Mechanic::Flick, 2)]
        );
    }
}
