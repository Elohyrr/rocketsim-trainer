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

mod ctx;
mod detail;
mod detector;
mod ext_carry;
mod ext_hit;
mod ext_move;
mod quality;
pub mod script;
mod script_carry;
mod script_hit;
mod script_move;
pub mod thresholds;

pub use detector::{Detector, Events, Mode};
pub use ctx::{Base, Ctx, DodgeInfo, Hit};
pub use detail::{Detail, Records, Support};
pub use quality::{Quality, QualityTracker};

/// Nombre de mécaniques du catalogue.
pub const MECH_COUNT: usize = 41;

/// Fréquence de la physique (ticks par seconde).
pub const TICK_HZ: f32 = 120.0;

/// Catalogue fermé des mécaniques détectées (41, ordre de [`Mechanic::ALL`] = index des compteurs).
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
    PowerShot,
    Pinch,
    KuxirPinch,
    Cushion,
    BackboardRead,
    BackboardDouble,
    Redirect,
    AirRollShot,
    ResetShot,
    DoubleReset,
    CeilingReset,
    WallToAirDribble,
    BounceDribble,
    HoodDribble,
    ReverseDribble,
    WallDribble,
    CeilingDribble,
    Flick45,
    Flick90,
    MustyFlick,
    DelayedFlick,
    BreeziFlick,
    ChainDash,
    WallDash,
    FastAerial,
    Stall,
    TornadoSpin,
    Recovery,
    HalfVolley,
}

impl Mechanic {
    pub const ALL: [Mechanic; MECH_COUNT] = [
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
        Mechanic::PowerShot,
        Mechanic::Pinch,
        Mechanic::KuxirPinch,
        Mechanic::Cushion,
        Mechanic::BackboardRead,
        Mechanic::BackboardDouble,
        Mechanic::Redirect,
        Mechanic::AirRollShot,
        Mechanic::ResetShot,
        Mechanic::DoubleReset,
        Mechanic::CeilingReset,
        Mechanic::WallToAirDribble,
        Mechanic::BounceDribble,
        Mechanic::HoodDribble,
        Mechanic::ReverseDribble,
        Mechanic::WallDribble,
        Mechanic::CeilingDribble,
        Mechanic::Flick45,
        Mechanic::Flick90,
        Mechanic::MustyFlick,
        Mechanic::DelayedFlick,
        Mechanic::BreeziFlick,
        Mechanic::ChainDash,
        Mechanic::WallDash,
        Mechanic::FastAerial,
        Mechanic::Stall,
        Mechanic::TornadoSpin,
        Mechanic::Recovery,
        Mechanic::HalfVolley,
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
            Mechanic::PowerShot => "power_shot",
            Mechanic::Pinch => "pinch",
            Mechanic::KuxirPinch => "kuxir_pinch",
            Mechanic::Cushion => "cushion",
            Mechanic::BackboardRead => "backboard_read",
            Mechanic::BackboardDouble => "backboard_double",
            Mechanic::Redirect => "redirect",
            Mechanic::AirRollShot => "air_roll_shot",
            Mechanic::ResetShot => "reset_shot",
            Mechanic::DoubleReset => "double_reset",
            Mechanic::CeilingReset => "ceiling_reset",
            Mechanic::WallToAirDribble => "wall_to_air_dribble",
            Mechanic::BounceDribble => "bounce_dribble",
            Mechanic::HoodDribble => "hood_dribble",
            Mechanic::ReverseDribble => "reverse_dribble",
            Mechanic::WallDribble => "wall_dribble",
            Mechanic::CeilingDribble => "ceiling_dribble",
            Mechanic::Flick45 => "flick_45",
            Mechanic::Flick90 => "flick_90",
            Mechanic::MustyFlick => "musty_flick",
            Mechanic::DelayedFlick => "delayed_flick",
            Mechanic::BreeziFlick => "breezi_flick",
            Mechanic::ChainDash => "chain_dash",
            Mechanic::WallDash => "wall_dash",
            Mechanic::FastAerial => "fast_aerial",
            Mechanic::Stall => "stall",
            Mechanic::TornadoSpin => "tornado_spin",
            Mechanic::Recovery => "recovery",
            Mechanic::HalfVolley => "half_volley",
        }
    }

    /// Vrai si la mécanique garde un record (meilleure vitesse de balle, uu/s) : métrique
    /// `trainer_mechanic_record{mechanic}`.
    pub fn has_record(self) -> bool {
        use Mechanic::*;
        matches!(
            self,
            PowerShot
                | Pinch
                | KuxirPinch
                | Flick
                | Flick45
                | Flick90
                | MustyFlick
                | DelayedFlick
                | BreeziFlick
                | WallShot
                | CeilingShot
                | DoubleTouch
                | BackboardDouble
                | Redirect
                | AirRollShot
                | ResetShot
        )
    }

    /// Priorité du clip quand plusieurs mécaniques se déclenchent dans le même pas (plus grand = plus précieux ;
    /// à égalité, l'index le plus élevé gagne). Le simple toucher n'est jamais clippé.
    pub fn clip_priority(self) -> u8 {
        use Mechanic::*;
        match self {
            FlipReset | ResetShot | DoubleReset | CeilingReset | KuxirPinch => 9,
            Pinch | CeilingShot | AirDribble | WallToAirDribble | BackboardDouble | DoubleTouch
            | Flick | Flick45 | Flick90 | MustyFlick | DelayedFlick | BreeziFlick | Redirect
            | AirRollShot => 8,
            PowerShot | WallShot | SpeedFlip | ChainDash | WallDash | Stall | TornadoSpin
            | BackboardRead | BounceDribble | WallDribble | CeilingDribble | ReverseDribble => 6,
            Touch => 0,
            _ => 3,
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
#[derive(Clone, Debug)]
pub struct Counts(pub [u64; MECH_COUNT], pub Records);

impl Default for Counts {
    fn default() -> Self {
        Self([0; MECH_COUNT], Records::default())
    }
}

/// Égalité des compteurs seulement : les records ([`Counts::records`]) accompagnent les compteurs mais ne
/// font pas partie de l'attendu des séquences scriptées.
impl PartialEq for Counts {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Counts {}

impl Counts {
    #[inline]
    pub fn get(&self, m: Mechanic) -> u64 {
        self.0[m.index()]
    }

    #[inline]
    pub fn inc(&mut self, m: Mechanic) {
        self.0[m.index()] += 1;
    }

    /// Additionne les compteurs et garde le maximum des records.
    pub fn add(&mut self, other: &Counts) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a += *b;
        }
        self.1.merge(&other.1);
    }

    /// Records (meilleure vitesse de balle par mécanique, [`Mechanic::has_record`]).
    #[inline]
    pub fn records(&self) -> &Records {
        &self.1
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
                "half_flip",
                "power_shot",
                "pinch",
                "kuxir_pinch",
                "cushion",
                "backboard_read",
                "backboard_double",
                "redirect",
                "air_roll_shot",
                "reset_shot",
                "double_reset",
                "ceiling_reset",
                "wall_to_air_dribble",
                "bounce_dribble",
                "hood_dribble",
                "reverse_dribble",
                "wall_dribble",
                "ceiling_dribble",
                "flick_45",
                "flick_90",
                "musty_flick",
                "delayed_flick",
                "breezi_flick",
                "chain_dash",
                "wall_dash",
                "fast_aerial",
                "stall",
                "tornado_spin",
                "recovery",
                "half_volley"
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
