//! Automates du catalogue étendu : groupe « move » (déplacements, resets, contrôle aérien).
//!
//! Signatures retenues (seuils : section « groupe move » de `thresholds.rs`) et approximations :
//!
//! - `wall_to_air_dribble` : l'air dribble de base se déclenche (`base.air_dribble`) et la voiture roulait sur un
//!   mur au plus 1,5 s avant le premier toucher de la chaîne (approché par le toucher en l'air précédent), sans
//!   autre terrain entre-temps. Compté EN PLUS de `air_dribble`. Approximation : on ne vérifie pas que la balle
//!   était sur le capot au départ du mur (le départ peut être un saut ou une poussée au boost).
//! - `double_reset` : deuxième flip reset (`base.flip_reset`) dans la même phase aérienne (sans terrain entre les
//!   deux) ; émis une seule fois, au 2e (un 3e n'est pas recompté).
//! - `ceiling_reset` : flip reset sur la balle alors que la balle est contre/près du plafond (z > 1 800).
//!   Approximation : le terme désigne aussi « roues posées au plafond » (qui rend le flip sans balle) ; ce cas-là
//!   n'est pas compté (c'est un simple retour au terrain).
//! - `reset_shot` (« reset utilisé ») : après un flip reset, sans retour au terrain, un NOUVEAU dodge (postérieur
//!   au reset) puis un toucher en l'air ≤ 0,3 s après ce dodge qui envoie la balle à > 1 500 uu/s (toucher jugé).
//!   Valeur = vitesse de balle après le toucher (record). Une fois par reset.
//! - `chain_dash` (zap dash) : deux wave dashes (`base.wave_dash`) à ≤ 1,5 s d'écart ; émis au 2e, une fois par
//!   chaîne. Approximation : on ne vérifie pas que les roues ne se sont pas reposées à plat entre les deux.
//! - `wall_dash` : dodge ≤ 0,3 s après avoir quitté un mur (saut depuis le mur), puis retour au terrain ≤ 1 s
//!   après le dodge avec un gain de vitesse > 200 uu/s par rapport au tick d'avant le dodge, sans toucher de
//!   balle. Gain mesuré sur la vitesse horizontale si l'atterrissage est au sol, sur la vitesse totale s'il est
//!   sur un mur (le dash le long du mur est surtout vertical). Un retour sur le mur ≤ 6 ticks après le dodge
//!   compte aussi comme wave dash (base).
//! - `fast_aerial` : saut (JUMP) puis second saut sans dodge (DOUBLE_JUMP) ≤ 0,35 s après, avec du boost et un
//!   tangage vers le haut (`pitch_input` > 0,3 ; −1 = nez vers le bas) vus entre les deux (inclus) ; une fois par
//!   phase aérienne. Approximation : ni le maintien continu du boost ni la direction prise ne sont vérifiés.
//! - `stall` : en l'air (z > 300), dodge annulé par les entrées : lacet et roulis opposés (≥ 0,5 chacun,
//!   |somme| < 0,1) sans tangage, ET par la physique : couple de dodge nul (aucune rotation), vitesse horizontale
//!   inchangée au dodge (pas d'impulsion), puis 0,2 s plus tard toujours en l'air (ni toucher, ni choc, ni terrain)
//!   avec |vz| < 120 uu/s (RocketSim amortit la vitesse verticale d'un flip sans direction).
//! - `tornado_spin` : en l'air depuis ≥ 0,25 s, rotation autour de l'axe `up` de la voiture |ω·up| > 4 rad/s
//!   maintenue ≥ 0,3 s (36 ticks consécutifs), sans terrain ; une fois par phase aérienne. Approximation : seule
//!   la vitesse de rotation compte (pas la trajectoire en spirale autour de la balle).
//! - `recovery` : atterrissage (`landed`) roues à plat (`up.z` > 0,8, ≥ 3 roues) après ≥ 0,6 s en l'air pendant
//!   lesquelles la voiture a été renversée ou sur le côté (min `up.z` < 0,2), en gardant ≥ 60 % de la vitesse
//!   horizontale du début de la phase aérienne et ≥ 400 uu/s ; pas de toucher exigé (un atterrissage sur la balle
//!   est exclu par `on_terrain`) ; une fois par phase aérienne.

use crate::Events;
use crate::Mechanic;
use crate::ctx::Ctx;
use crate::detail::{Out, Support};
use crate::thresholds::*;

#[inline]
fn within(since: Option<u64>, now: u64, window: u64) -> bool {
    since.is_some_and(|t| now - t <= window)
}

#[inline]
fn hspeed(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

#[inline]
fn speed(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Wall dash en attente : tick du dodge, vitesses (horizontale, totale) au tick d'avant le dodge.
#[derive(Clone, Copy, Debug, Default)]
struct WallDash {
    tick: u64,
    hspeed: f32,
    speed: f32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Group {
    /// Dernier tick sur un mur (oublié au retour sur un autre terrain).
    wall_last: Option<u64>,
    /// Dernier toucher en l'air (pour retrouver le premier toucher de la chaîne d'air dribble).
    air_touch_last: Option<u64>,
    /// Flip resets de la phase aérienne et tick du dernier non encore utilisé.
    resets: u8,
    reset_last: Option<u64>,
    // Chain dash.
    wd_last: Option<u64>,
    cd_counted: bool,
    // Wall dash.
    wdash: Option<WallDash>,
    // Fast aerial.
    fa_jump: Option<u64>,
    fa_boost: bool,
    fa_pitch: bool,
    fa_done: bool,
    /// Detector 6 : saut depuis un mur, nez au 2ᵉ saut, altitude au 1er saut, boost tenu après, en attente de gain.
    fa_wall: bool,
    fa_z0: f32,
    fa_second: Option<u64>,
    fa_boost_run: u32,
    fa_boost_ok: bool,
    fa_gain_ok: bool,
    // Tornado : roulis tenu.
    ts_roll_sign: f32,
    ts_roll_run: u32,
    // Recovery : ticks d'entrée d'air roll / tangage pendant le vol.
    rc_input: u32,
    /// Saut ou dodge pressé pendant ce vol (décollage voulu, pas une chute du mur).
    rc_jump: bool,
    // Stall : tick d'un dodge annulé par les entrées et par la physique.
    st_dodge: Option<u64>,
    // Tornado spin.
    ts_run: u32,
    ts_done: bool,
    // Recovery : ticks en l'air, min up.z, vitesse horizontale au début de la phase.
    rc_air: u32,
    /// Un choc a eu lieu pendant ce vol (la recovery ne compte pas).
    rc_hit: bool,
    rc_min_up: f32,
    rc_start: Option<f32>,
}

impl Group {
    #[inline]
    pub fn tick(&mut self, cx: &Ctx, out: &mut Out) {
        let n = cx.n;
        let car = cx.car;

        // --- Wall to air dribble : l'air dribble de base vient de se déclencher (au 2e toucher de la chaîne) ;
        // le toucher en l'air précédent est le premier de la chaîne.
        if cx.base.air_dribble {
            if let (Some(w), Some(first)) = (self.wall_last, self.air_touch_last) {
                if first.saturating_sub(w) <= W2AD_MAX_WALL_TICKS {
                    out.emit(Mechanic::WallToAirDribble, 0.0, Support::Wall);
                }
            }
        }
        if cx.touch && !car.on_ground {
            self.air_touch_last = Some(n);
        }

        // --- Resets : double reset, ceiling reset.
        if cx.base.flip_reset {
            self.resets = self.resets.saturating_add(1);
            self.reset_last = Some(n);
            if self.resets == 2 {
                out.emit(Mechanic::DoubleReset, 0.0, Support::None);
            }
            if cx.ball.pos[2] > CEILING_RESET_BALL_MIN_Z {
                out.emit(Mechanic::CeilingReset, 0.0, Support::Ceiling);
            }
        }

        // --- Reset shot : toucher jugé, en l'air, dodge postérieur au reset et récent, balle rapide.
        if let (Some(h), Some(r)) = (cx.hit, self.reset_last) {
            if h.tick > r
                && h.air_ticks > 0
                && h.dodge.is_some_and(|d| d.tick > r)
                && h.dodge_age.is_some_and(|a| a <= RESET_SHOT_HIT_TICKS)
                && h.speed_after > RESET_SHOT_MIN_SPEED
            {
                out.emit(Mechanic::ResetShot, h.speed_after, Support::None);
                self.reset_last = None;
            }
        }

        // --- Chain dash : deux wave dashes rapprochés.
        if cx.tainted {
            self.wd_last = None;
            self.cd_counted = true;
            self.wdash = None;
        }
        if cx.base.wave_dash {
            if within(self.wd_last, n, CHAIN_DASH_MAX_GAP_TICKS) {
                if !self.cd_counted {
                    out.emit(Mechanic::ChainDash, 0.0, Support::Ground);
                    self.cd_counted = true;
                }
            } else {
                self.cd_counted = false;
            }
            self.wd_last = Some(n);
        }

        // --- Wall dash : dodge peu après le départ du mur, retour au terrain avec gain de vitesse.
        if cx.touch {
            self.wdash = None;
        }
        if let Some(d) = self.wdash {
            if n - d.tick > WALL_DASH_LAND_TICKS {
                self.wdash = None;
            } else if cx.landed {
                let gain = if cx.on_wall {
                    speed(car.vel) - d.speed
                } else {
                    cx.hspeed - d.hspeed
                };
                if gain > WALL_DASH_MIN_GAIN {
                    out.emit(Mechanic::WallDash, 0.0, Support::Wall);
                }
                self.wdash = None;
            }
        }
        if cx.dodge_now
            && !cx.on_terrain
            && !cx.touch
            && within(self.wall_last, n, WALL_DASH_DODGE_TICKS)
        {
            self.wdash = Some(WallDash {
                tick: n,
                hspeed: hspeed(cx.prev_car.vel),
                speed: speed(cx.prev_car.vel),
            });
        }

        // --- Fast aerial : saut, second saut rapide, boost et nez vers le haut entre les deux. Detector 6 : nez ≥ 30°
        // au 2ᵉ saut, boost tenu ≥ 0,5 s après, gain d'altitude ≥ 400 uu (ou balle haute touchée ≤ 2 s), pas depuis un mur.
        if cx.on_terrain {
            self.fa_done = false;
            self.fa_second = None;
        }
        if cx.ev.has(Events::JUMP) {
            self.fa_jump = Some(n);
            self.fa_boost = false;
            self.fa_pitch = false;
            self.fa_wall = cx.on_wall || within(self.wall_last, n, VOLUNTARY_JUMP_TICKS as u64);
            self.fa_z0 = car.pos[2];
        }
        if let Some(j) = self.fa_jump {
            if n - j > FAST_AERIAL_MAX_GAP_TICKS {
                self.fa_jump = None;
            } else {
                self.fa_boost |= car.boosting;
                self.fa_pitch |= car.pitch_input > FAST_AERIAL_MIN_PITCH;
                if cx.ev.has(Events::DOUBLE_JUMP) && !cx.dodge_now {
                    if self.fa_boost && self.fa_pitch && !self.fa_done && !self.fa_wall
                        && car.forward[2] >= FAST_AERIAL_MIN_NOSE_Z
                    {
                        self.fa_second = Some(n);
                        self.fa_boost_run = 0;
                        self.fa_boost_ok = false;
                        self.fa_gain_ok = false;
                    }
                    self.fa_jump = None;
                }
            }
        }
        if let Some(t) = self.fa_second {
            if cx.on_terrain {
                self.fa_second = None;
            } else {
                if car.boosting {
                    self.fa_boost_run += 1;
                    if self.fa_boost_run >= FAST_AERIAL_BOOST_TICKS {
                        self.fa_boost_ok = true;
                    }
                } else if !self.fa_boost_ok {
                    self.fa_boost_run = 0;
                }
                if car.pos[2] - self.fa_z0 >= FAST_AERIAL_MIN_GAIN_Z
                    || (cx.touch && cx.ball.pos[2] > FAST_AERIAL_BALL_Z && n - t <= FAST_AERIAL_TOUCH_TICKS)
                {
                    self.fa_gain_ok = true;
                }
                if self.fa_boost_ok && self.fa_gain_ok {
                    out.emit(Mechanic::FastAerial, 0.0, Support::None);
                    self.fa_done = true;
                    self.fa_second = None;
                } else if n - t > FAST_AERIAL_TOUCH_TICKS {
                    self.fa_second = None;
                }
            }
        }

        // --- Stall : flip annulé (lacet et roulis opposés) qui garde la voiture en l'air, sans rotation de dodge.
        if let Some(t) = self.st_dodge {
            if cx.touch || cx.on_terrain || cx.tainted || car.world_contact || car.on_ground {
                self.st_dodge = None;
            } else if n - t >= STALL_CHECK_TICKS {
                if car.vel[2].abs() < STALL_MAX_VZ {
                    out.emit(Mechanic::Stall, 0.0, Support::None);
                }
                self.st_dodge = None;
            }
        }
        if cx.dodge_now && !cx.on_terrain && car.pos[2] > STALL_MIN_Z {
            let (y, r) = (car.yaw_input, car.roll_input);
            let inputs = y.abs() >= STALL_MIN_YAW_ROLL
                && r.abs() >= STALL_MIN_YAW_ROLL
                && y * r < 0.0
                && (y + r).abs() < STALL_MAX_YAW_ROLL_SUM
                && car.pitch_input.abs() < STALL_MAX_PITCH;
            let t = car.flip_rel_torque;
            let no_spin = t[0].hypot(t[1]) < STALL_MAX_FLIP_TORQUE;
            let (pv, cv) = (cx.prev_car.vel, car.vel);
            let no_impulse = (cv[0] - pv[0]).hypot(cv[1] - pv[1]) < STALL_MAX_HSPEED_JUMP;
            if inputs && no_spin && no_impulse {
                self.st_dodge = Some(n);
            }
        }

        // --- Tornado spin : rotation rapide autour de l'axe vertical de la voiture, maintenue. Detector 6 : le roulis
        // commandé (|roll| ≥ 0,8, même signe) est tenu ≥ 4 décisions, vol ≥ 0,5 s, sans choc (cadre du but compris).
        let roll = car.roll_input;
        if roll.abs() >= TORNADO_MIN_ROLL && (self.ts_roll_run == 0 || roll * self.ts_roll_sign > 0.0) {
            self.ts_roll_sign = roll;
            self.ts_roll_run += 1;
        } else {
            self.ts_roll_run = if roll.abs() >= TORNADO_MIN_ROLL { 1 } else { 0 };
            self.ts_roll_sign = roll;
        }
        if cx.on_terrain {
            self.ts_run = 0;
            self.ts_done = false;
        } else if cx.tainted || car.world_contact {
            // Rotation subie (choc, toucher simultané, cadre du but) : ne compte ni maintenant ni plus tard.
            self.ts_run = 0;
            self.ts_done = true;
        } else if cx.air_ticks >= TORNADO_MIN_AIR_TICKS {
            let w = car.ang_vel;
            let spin = (w[0] * car.up[0] + w[1] * car.up[1] + w[2] * car.up[2]).abs();
            if spin > TORNADO_MIN_SPIN {
                self.ts_run += 1;
                if self.ts_run >= TORNADO_V6_MIN_TICKS
                    && self.ts_roll_run >= TORNADO_HOLD_ROLL_TICKS
                    && !self.ts_done
                {
                    out.emit(Mechanic::TornadoSpin, 0.0, Support::None);
                    self.ts_done = true;
                }
            } else {
                self.ts_run = 0;
            }
        }

        // --- Recovery : renversé en l'air, retombe sur les roues en gardant sa vitesse.
        if cx.on_terrain {
            if cx.landed {
                if let Some(start) = self.rc_start {
                    if self.rc_air >= RECOVERY_MIN_AIR_TICKS
                        && self.rc_min_up < RECOVERY_FLIPPED_UP_Z
                        && car.up[2] > RECOVERY_LAND_UP_Z
                        && car.wheels_contact >= 3
                        && cx.hspeed >= RECOVERY_MIN_SPEED
                        && cx.hspeed >= RECOVERY_MIN_KEEP * start
                        && self.rc_input >= RECOVERY_MIN_INPUT_TICKS
                        && self.rc_jump
                        && !self.rc_hit
                    {
                        out.emit(Mechanic::Recovery, 0.0, Support::Ground);
                    }
                }
            }
            self.rc_start = None;
            self.rc_air = 0;
            self.rc_input = 0;
            // Un saut pressé au sol (dernier tick avant le décollage) compte pour le vol qui suit.
            self.rc_jump = cx.ev.has(Events::JUMP);
            self.rc_hit = false;
        } else {
            // Détecteur 6 : la reprise est VOULUE si le bot a donné du roulis ou du tangage en l'air.
            if cx.ev.has(Events::JUMP) || cx.ev.has(Events::DOUBLE_JUMP) || cx.ev.has(Events::DODGE) {
                self.rc_jump = true;
            }
            if car.up[2] < RECOVERY_FLIPPED_UP_Z
                && (car.roll_input.abs() >= 0.3 || car.pitch_input.abs() >= 0.3)
            {
                self.rc_input += 1;
            }
            // Retournée par un choc : le vol ne compte pas comme recovery.
            self.rc_hit |= cx.tainted;
            if self.rc_start.is_none() {
                self.rc_start = Some(hspeed(cx.prev_car.vel));
                self.rc_min_up = car.up[2];
            }
            self.rc_air = self.rc_air.saturating_add(1);
            self.rc_min_up = self.rc_min_up.min(car.up[2]);
        }

        // --- Retour au terrain : fin de la phase aérienne (après les jugements ci-dessus).
        if cx.on_wall {
            self.wall_last = Some(n);
        } else if cx.on_terrain {
            self.wall_last = None;
        }
        if cx.on_terrain {
            self.resets = 0;
            self.reset_last = None;
            self.air_touch_last = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::script::cases;
    use crate::{Counts, Detector, Mechanic};

    /// Le reset shot garde la vitesse de balle après le toucher comme valeur et record ; les autres mécaniques
    /// du groupe n'ont pas de record.
    #[test]
    fn reset_shot_valeur_et_record() {
        let case = cases()
            .into_iter()
            .find(|c| c.name == "reset_shot")
            .unwrap();
        let mut det = Detector::new();
        let mut counts = Counts::default();
        for (c, b) in &case.frames {
            det.tick(c, b, &mut counts);
        }
        let v = (2000.0f32 * 2000.0 + 300.0 * 300.0).sqrt();
        assert!((det.detail(Mechanic::ResetShot).value - v).abs() < 1.0);
        assert!((counts.records().get(Mechanic::ResetShot) - v).abs() < 1.0);
        assert!(!Mechanic::Recovery.has_record());
    }
}
