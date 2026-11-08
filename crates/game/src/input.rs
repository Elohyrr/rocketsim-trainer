//! Entrées du joueur : état brut (clavier, souris, manette) → commandes RocketSim.

use sim_core::Controls;

/// État brut d'un tick de saisie, indépendant du périphérique. Axes dans [-1, 1].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RawInput {
    /// Gaz (+1) / frein-marche arrière (-1).
    pub throttle: f32,
    /// Direction : -1 gauche, +1 droite (au sol : braquage ; en l'air : lacet, ou roulis si `air_roll`).
    pub steer: f32,
    /// Avant/arrière (+1 = avant = nez vers le bas) : tangage en l'air.
    pub stick_y: f32,
    /// Déplacement souris cumulé sur la frame, déjà mis à l'échelle (contrôle aérien optionnel).
    pub mouse: [f32; 2],
    pub jump: bool,
    pub boost: bool,
    pub powerslide: bool,
    /// Maintenu : la direction devient un roulis.
    pub air_roll: bool,
    /// Roulis direct : -1 gauche, +1 droite (touches dédiées).
    pub roll: f32,
}

/// Actions ponctuelles (front montant), non transmises à la simulation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Actions {
    pub reset: bool,
    pub toggle_ball_cam: bool,
    pub slower: bool,
    pub faster: bool,
    pub toggle_drill: bool,
    pub quit: bool,
}

impl RawInput {
    /// Fusionne deux périphériques : à chaque axe, la plus grande amplitude gagne ; boutons en OU.
    pub fn merge(self, o: Self) -> Self {
        let big = |a: f32, b: f32| if b.abs() > a.abs() { b } else { a };
        Self {
            throttle: big(self.throttle, o.throttle),
            steer: big(self.steer, o.steer),
            stick_y: big(self.stick_y, o.stick_y),
            mouse: [
                big(self.mouse[0], o.mouse[0]),
                big(self.mouse[1], o.mouse[1]),
            ],
            jump: self.jump || o.jump,
            boost: self.boost || o.boost,
            powerslide: self.powerslide || o.powerslide,
            air_roll: self.air_roll || o.air_roll,
            roll: big(self.roll, o.roll),
        }
    }

    /// Convertit en commandes RocketSim. Conventions RLBot : pitch +1 = nez en haut, yaw +1 = vers la
    /// droite, roll +1 = roulis vers la droite (vérifié par `world::control_signs`).
    pub fn to_controls(self) -> Controls {
        let c = |x: f32| x.clamp(-1.0, 1.0);
        let (mut yaw, mut roll) = (0.0, self.roll);
        if self.air_roll {
            roll = if self.steer != 0.0 { self.steer } else { roll };
        } else {
            yaw = self.steer;
        }
        Controls {
            throttle: c(self.throttle),
            steer: c(self.steer),
            pitch: c(-self.stick_y + self.mouse[1]),
            yaw: c(yaw + self.mouse[0]),
            roll: c(roll),
            jump: self.jump,
            boost: self.boost,
            handbrake: self.powerslide,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_key_means_throttle_and_nose_down() {
        let c = RawInput {
            throttle: 1.0,
            stick_y: 1.0,
            ..Default::default()
        }
        .to_controls();
        assert_eq!((c.throttle, c.pitch), (1.0, -1.0));
    }

    #[test]
    fn air_roll_turns_steer_into_roll() {
        let c = RawInput {
            steer: -1.0,
            air_roll: true,
            ..Default::default()
        }
        .to_controls();
        assert_eq!((c.yaw, c.roll, c.steer), (0.0, -1.0, -1.0));
    }

    #[test]
    fn merge_keeps_largest_axis_and_or_buttons() {
        let a = RawInput {
            steer: 0.2,
            jump: true,
            ..Default::default()
        };
        let b = RawInput {
            steer: -0.8,
            boost: true,
            ..Default::default()
        };
        let m = a.merge(b);
        assert_eq!(m.steer, -0.8);
        assert!(m.jump && m.boost);
    }
}
