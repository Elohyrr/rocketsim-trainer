//! Exploration réparée : coefficient d'entropie adaptatif (cible en nat), plafond de KL, lr décroissant.
//!
//! Constat de la prod (étude `docs/specs/decouverte-mecaniques.md`, § 3.1) : avec `ent_coef` fixe à 0,01 et
//! un lr constant de 1e-3, l'entropie de la politique s'est effondrée à 0,07 nat (max ln 78 ≈ 4,36) :
//! plus rien de nouveau ne peut émerger. Ici :
//! - `ent_coef ← ent_coef × exp(GAIN × (cible − H))`, borné à `[ENT_MIN, ENT_MAX]`, à chaque itération ;
//! - les époques PPO s'arrêtent dès que la KL moyenne d'une époque dépasse `KL_MAX` ;
//! - `lr = max(LR_MIN, LR0 / (1 + pas/LR_SCALE))`.
//!
//! L'état (`ent_coef`, pas cumulés) est persisté dans `<out>/explore.json` : une reprise ne remet ni le
//! coefficient ni le lr à zéro. Sans fichier (premier démarrage sur un checkpoint existant), valeurs initiales.

use std::path::Path;

/// Entropie visée (nat) : milieu de la plage 1,0–1,5.
pub const ENT_TARGET: f64 = 1.25;
/// Bornes de la plage visée, pour la documentation et les alertes.
pub const ENT_BAND: (f64, f64) = (1.0, 1.5);
pub const ENT_MIN: f64 = 1e-4;
/// Plafond large : à 0,07 nat le gradient de l'entropie est minuscule (softmax saturé), un plafond de 0,1
/// ne relevait l'entropie que de 0,09 à 0,11 en 500 itérations (essai local) ; à 1,0 elle monte, lentement.
pub const ENT_MAX: f64 = 1.0;
/// Gain de l'ajustement (voir la formule ci-dessus).
pub const ENT_GAIN: f64 = 0.03;
/// KL moyenne par époque au-delà de laquelle les époques suivantes sont sautées.
pub const KL_MAX: f64 = 0.02;
pub const LR0: f64 = 3e-4;
pub const LR_MIN: f64 = 5e-5;
/// Pas cumulés pour diviser le lr par 2 (≈ 1 jour de prod à 420 M pas/jour).
pub const LR_SCALE: f64 = 5e8;
/// Coefficient d'entropie initial (celui de la prod avant ce changement).
pub const ENT_INIT: f64 = 0.01;

/// Nouvelle valeur du coefficient d'entropie après une itération d'entropie mesurée `h`.
pub fn next_ent_coef(coef: f64, h: f64, target: f64) -> f64 {
    (coef * (ENT_GAIN * (target - h)).exp()).clamp(ENT_MIN, ENT_MAX)
}

/// lr après `steps` pas cumulés.
pub fn lr_at(steps: u64) -> f64 {
    (LR0 / (1.0 + steps as f64 / LR_SCALE)).max(LR_MIN)
}

/// État persistant du régulateur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Explore {
    pub ent_coef: f64,
    pub steps: u64,
}

impl Default for Explore {
    fn default() -> Self {
        Self {
            ent_coef: ENT_INIT,
            steps: 0,
        }
    }
}

impl Explore {
    pub fn lr(&self) -> f64 {
        lr_at(self.steps)
    }

    /// Fin d'itération : `steps` pas de plus, entropie mesurée `h`.
    pub fn update(&mut self, steps: u64, h: f64) {
        self.steps += steps;
        if h.is_finite() {
            self.ent_coef = next_ent_coef(self.ent_coef, h, ENT_TARGET);
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let s = format!(
            "{{\"ent_coef\":{},\"steps\":{}}}\n",
            self.ent_coef, self.steps
        );
        std::fs::write(dir.join("explore.json"), s)
    }

    /// Lit `<dir>/explore.json` ; valeurs initiales si absent ou illisible.
    pub fn load(dir: &Path) -> Self {
        let parsed = std::fs::read_to_string(dir.join("explore.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let Some(v) = parsed else {
            return Self::default();
        };
        Self {
            ent_coef: v["ent_coef"]
                .as_f64()
                .filter(|c| c.is_finite())
                .map_or(ENT_INIT, |c| c.clamp(ENT_MIN, ENT_MAX)),
            steps: v["steps"].as_u64().unwrap_or(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coef_rises_when_entropy_is_too_low_and_falls_when_too_high() {
        assert!(next_ent_coef(0.01, 0.07, ENT_TARGET) > 0.01);
        assert!(next_ent_coef(0.01, 2.5, ENT_TARGET) < 0.01);
        assert!((next_ent_coef(0.01, ENT_TARGET, ENT_TARGET) - 0.01).abs() < 1e-12);
    }

    #[test]
    fn coef_stays_in_bounds() {
        let mut c = ENT_INIT;
        for _ in 0..100_000 {
            c = next_ent_coef(c, 0.0, ENT_TARGET);
        }
        assert!((c - ENT_MAX).abs() < 1e-12);
        for _ in 0..100_000 {
            c = next_ent_coef(c, 4.0, ENT_TARGET);
        }
        assert!((c - ENT_MIN).abs() < 1e-12);
    }

    #[test]
    fn lr_decays_with_a_floor() {
        assert!((lr_at(0) - LR0).abs() < 1e-15);
        assert!((lr_at(LR_SCALE as u64) - LR0 / 2.0).abs() < 1e-9);
        assert!(lr_at(1_000_000) > lr_at(100_000_000));
        assert!((lr_at(u64::MAX / 2) - LR_MIN).abs() < 1e-15);
    }

    #[test]
    fn state_round_trips_and_missing_file_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("explore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Explore::load(&dir), Explore::default());
        let e = Explore {
            ent_coef: 0.037,
            steps: 123_456_789,
        };
        e.save(&dir).unwrap();
        assert_eq!(Explore::load(&dir), e);
        std::fs::write(dir.join("explore.json"), "pas du json").unwrap();
        assert_eq!(Explore::load(&dir), Explore::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
