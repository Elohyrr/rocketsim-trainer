//! Métadonnées d'une occurrence (valeur, support) et records par mécanique.

use crate::{MECH_COUNT, Mechanic};

/// Surface (ou objet) contre laquelle la balle est coincée ou qui sert d'appui : précisé pour les pinchs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Support {
    #[default]
    None,
    Ground,
    /// Mur latéral.
    Wall,
    /// Mur du fond (backboard).
    BackWall,
    /// Coin (deux murs, ou mur et sol près d'un coin).
    Corner,
    Ceiling,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Support::None => "none",
            Support::Ground => "ground",
            Support::Wall => "wall",
            Support::BackWall => "backwall",
            Support::Corner => "corner",
            Support::Ceiling => "ceiling",
        }
    }
}

/// Dernière occurrence d'une mécanique : valeur (vitesse de balle après le toucher, uu/s ; 0 si sans objet)
/// et support.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Detail {
    pub value: f32,
    pub support: Support,
}

/// Dernier détail de chaque mécanique (index = [`Mechanic::index`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Details(pub [Detail; MECH_COUNT]);

impl Default for Details {
    fn default() -> Self {
        Self([Detail::default(); MECH_COUNT])
    }
}

/// Meilleure valeur par mécanique ([`Mechanic::has_record`] ; 0 = aucun record).
#[derive(Clone, Debug, PartialEq)]
pub struct Records(pub [f32; MECH_COUNT]);

impl Default for Records {
    fn default() -> Self {
        Self([0.0; MECH_COUNT])
    }
}

impl Records {
    #[inline]
    pub fn get(&self, m: Mechanic) -> f32 {
        self.0[m.index()]
    }

    #[inline]
    pub fn observe(&mut self, m: Mechanic, v: f32) {
        if m.has_record() && v > self.0[m.index()] {
            self.0[m.index()] = v;
        }
    }

    /// Garde le maximum des deux.
    pub fn merge(&mut self, other: &Records) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            if *b > *a {
                *a = *b;
            }
        }
    }
}

/// Sortie des automates : compteurs, dernier détail et records. Passée aux automates du catalogue étendu.
pub(crate) struct Out<'a> {
    pub counts: &'a mut crate::Counts,
    pub details: &'a mut [Detail; MECH_COUNT],
    pub records: &'a mut Records,
}

impl Out<'_> {
    /// Une occurrence de `m` (compte une fois), avec sa valeur et son support.
    #[inline]
    pub fn emit(&mut self, m: Mechanic, value: f32, support: Support) {
        self.counts.inc(m);
        self.details[m.index()] = Detail { value, support };
        self.records.observe(m, value);
    }
}
