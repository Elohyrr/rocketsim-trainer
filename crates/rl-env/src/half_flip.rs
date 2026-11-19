//! Half flip scripté de référence (banc `examples/turnaround_bench.rs`, mesures dans `docs/drills.md`) : saut court,
//! tangage arrière, dodge arrière, annulation du flip (tangage inverse), roulis pour remettre les roues au sol, boost
//! maintenu dès que le nez regarde vers la cible (et à l'atterrissage), puis sortie en ligne droite.
//! Voiture partie dos à la cible ; le cap visé est +Y (le repère se retourne en changeant l'axe lu dans `controls`).
use sim_core::Controls;

/// Paramètres d'un half flip ; les durées sont en ticks (120 Hz).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalfFlipScript {
    /// Ticks de maintien du premier saut (≥ 1).
    pub hold: u32,
    /// Tangage arrière entre les deux sauts.
    pub pre_pitch: bool,
    /// Ticks entre la fin du maintien du saut et le dodge.
    pub dodge_at: u32,
    /// Composante latérale (lacet + braquage) du dodge.
    pub dodge_side: f32,
    /// Début de l'annulation (ticks après le dodge) et sa durée.
    pub cancel_after: u32,
    pub cancel_len: u32,
    /// Roulis (±1) : début après le dodge, durée.
    pub roll: f32,
    pub roll_after: u32,
    pub roll_len: u32,
    /// Lacet pendant l'annulation.
    pub yaw: f32,
    /// Boost dès que le nez fait un cosinus ≥ à cette valeur avec la cible (−2 = dès le dodge, 2 = jamais en l'air).
    pub boost_cos: f32,
}

/// Meilleur jeu de paramètres unique (somme des temps pour parcourir 2 000 uu vers la cible depuis l'arrêt, en marche
/// arrière 500 et 1 400 uu/s, en marche avant 500 uu/s, boost 100) trouvé par le banc `turnaround_bench ref` ; voir
/// `docs/drills.md`. Saut de 2 ticks, tangage arrière jusqu'au dodge (4 ticks plus tard), annulation 2 ticks après le
/// dodge, roulis 8 ticks après, lacet pendant l'annulation, boost dès que le nez passe l'horizontale vers la cible.
pub const REFERENCE: HalfFlipScript = HalfFlipScript {
    hold: 1,
    pre_pitch: true,
    dodge_at: 4,
    dodge_side: 0.0,
    cancel_after: 2,
    cancel_len: 45,
    roll: 1.0,
    roll_after: 8,
    roll_len: 45,
    yaw: 1.0,
    boost_cos: 0.0,
};

impl HalfFlipScript {
    /// Commandes du tick `t` (depuis le début). `forward` = nez de la voiture ; la cible est vers +Y.
    pub fn controls(
        &self,
        t: u32,
        forward: [f32; 3],
        on_ground: bool,
        has_boost: bool,
    ) -> Controls {
        let dodge_at = self.hold.max(1) + self.dodge_at;
        let mut c = Controls {
            throttle: 1.0,
            ..Default::default()
        };
        if on_ground && t > dodge_at + 10 {
            // Sortie en ligne droite : cap corrigé, boost maintenu.
            let ang = forward[0].atan2(forward[1]);
            c.steer = (ang * 2.0).clamp(-1.0, 1.0);
            c.boost = has_boost;
            return c;
        }
        if t < self.hold {
            c.jump = true;
        } else if t < dodge_at {
            c.pitch = if self.pre_pitch { 1.0 } else { 0.0 };
        } else if t == dodge_at {
            c.jump = true;
            c.pitch = 1.0; // dodge arrière
            c.yaw = self.dodge_side;
            c.steer = self.dodge_side;
        } else {
            let d = t - dodge_at;
            let cancel = d >= self.cancel_after && d < self.cancel_after + self.cancel_len;
            c.pitch = if cancel { -1.0 } else { 1.0 };
            if cancel {
                c.yaw = self.yaw;
            }
            if d >= self.roll_after && d < self.roll_after + self.roll_len {
                c.roll = self.roll;
            }
        }
        c.boost = has_boost && t >= dodge_at && forward[1] >= self.boost_cos;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{Sim, init_meshes, meshes_dir_from_env};

    /// Départ en marche arrière rapide vers la cible : la référence doit refaire face à +Y, au sol, à ≥ 1 400 uu/s en
    /// moins de 1,3 s (le demi-tour en roulant met ≈ 1,4 s). Ignoré sans meshes.
    #[test]
    fn reference_bat_le_demi_tour_en_marche_arriere() {
        let Ok(dir) = meshes_dir_from_env() else {
            return;
        };
        if init_meshes(&dir).is_err() {
            return;
        }
        let mut sim = Sim::new_soccar().unwrap();
        let car = sim.add_car(true);
        let yaw = -std::f32::consts::FRAC_PI_2;
        sim.reset_car(car, [0.0, 0.0, 17.0], yaw, [0.0, 1400.0, 0.0], 100.0)
            .unwrap();
        sim.reset_ball([0.0, 4000.0, 93.0], [0.0; 3]);
        let mut done = None;
        for t in 0..240 {
            let ct = sim.car_tick(car);
            sim.set_controls(car, REFERENCE.controls(t, ct.forward, ct.on_ground, true))
                .unwrap();
            sim.step(1);
            let ct = sim.car_tick(car);
            if ct.on_ground && ct.forward[1] >= 0.866 && ct.vel[1] >= 1400.0 {
                done = Some((t + 1) as f32 / 120.0);
                break;
            }
        }
        let secs = done.expect("critère jamais rempli");
        assert!(secs < 1.3, "{secs}");
    }
}
