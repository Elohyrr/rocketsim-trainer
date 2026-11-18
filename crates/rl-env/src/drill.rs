//! Bonus de mécanique des drills (jamais en match ni en self-play), voir `docs/drills.md`.
//!
//! Chaque occurrence détectée par `mechanics` pendant un drill peut rapporter un petit bonus : une fois par
//! occurrence, au plus [`MAX_PER_EPISODE`] par épisode, seulement si elle est utile (balle vers le but adverse
//! à plus de [`USEFUL_SPEED`] uu/s, ou but dans les [`GOAL_WINDOW_TICKS`] ticks), et décroissant avec le nombre
//! d'occurrences payées sur la dernière heure : `b0 / √(1 + n/N)`.

use mechanics::{Counts, Mechanic};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Part maximale d'un but (1,0 pour un but à +10).
pub const MAX_FRACTION_OF_GOAL: f32 = 0.1;
/// Bonus payés au plus par épisode.
pub const MAX_PER_EPISODE: u32 = 2;
/// Vitesse de la balle vers le but adverse (uu/s) au-delà de laquelle l'occurrence est utile.
pub const USEFUL_SPEED: f32 = 500.0;
/// Fenêtre « but dans les 3 s » (ticks à 120 Hz).
pub const GOAL_WINDOW_TICKS: u64 = 360;
/// Échelle de décroissance (occurrences payées sur la fenêtre glissante).
pub const DECAY_N: f32 = 1000.0;
/// Fenêtre glissante de la décroissance.
pub const WINDOW: Duration = Duration::from_secs(3600);
/// Centre du but adverse (+Y), pour la direction de la balle.
/// Case du registre du bonus de récupération (après les 12 mécaniques).
pub const RECOVERY_SLOT: usize = 12;
/// Bonus de base de la récupération (plafonné comme les autres à 10 % du but).
pub const RECOVERY_BONUS: f32 = 0.3;
const GRAVITY: f32 = 650.0;
/// Ticks entre le premier contact avec le terrain et le verdict (laisse les 4 roues se poser).
pub const RECOVERY_SETTLE_TICKS: u64 = 24;
/// Ticks entre le premier contact et la fin du drill (1 s après l'atterrissage).
pub const RECOVERY_END_TICKS: u64 = 120;
/// Cosinus mini entre le nez et la direction utile (60°), vitesse mini dans cette direction (uu/s).
pub const RECOVERY_MIN_COS: f32 = 0.5;
pub const RECOVERY_MIN_SPEED: f32 = 400.0;
pub const GOAL_CENTER: [f32; 3] = [0.0, 5120.0, 321.0];

/// Bonus de base `b0` (en récompense, pour un but à +10 ; plafonné à 10 % du but). 0 = jamais de bonus.
pub fn base_bonus(m: Mechanic) -> f32 {
    match m {
        Mechanic::FlipReset => 1.0,
        Mechanic::AirDribble | Mechanic::CeilingShot => 0.8,
        Mechanic::Flick => 0.6,
        Mechanic::WallShot => 0.5,
        Mechanic::AerialTouch | Mechanic::DoubleTouch => 0.4,
        Mechanic::Dribble => 0.3,
        // Le catalogue étendu (PR catalogue) ne rapporte aucun bonus : à décider avec les drills.
        _ => 0.0,
    }
}

/// Vitesse de la balle vers le but adverse (uu/s, négative si elle s'en éloigne).
pub fn speed_toward_goal(ball_pos: [f32; 3], ball_vel: [f32; 3]) -> f32 {
    let d = [
        GOAL_CENTER[0] - ball_pos[0],
        GOAL_CENTER[1] - ball_pos[1],
        GOAL_CENTER[2] - ball_pos[2],
    ];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1.0);
    (d[0] * ball_vel[0] + d[1] * ball_vel[1] + d[2] * ball_vel[2]) / n
}

/// Registre partagé (tous les envs, tous les threads) des bonus payés, pour la décroissance.
#[derive(Default)]
pub struct BonusLedger {
    paid: Mutex<[VecDeque<Instant>; 13]>,
}

impl std::fmt::Debug for BonusLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BonusLedger")
    }
}

impl BonusLedger {
    /// Paie une occurrence de `m` à l'instant `now` : renvoie `b0 / √(1 + n/N)` (0 si `b0 = 0`) et l'inscrit.
    pub fn pay_at(&self, m: Mechanic, b0: f32, now: Instant) -> f32 {
        self.pay_slot_at(m.index(), b0, now)
    }

    /// Comme [`BonusLedger::pay_at`] pour une case du registre (les 12 mécaniques puis [`RECOVERY_SLOT`]).
    pub fn pay_slot_at(&self, slot: usize, b0: f32, now: Instant) -> f32 {
        if b0 <= 0.0 {
            return 0.0;
        }
        let Ok(mut all) = self.paid.lock() else {
            return 0.0;
        };
        let q = &mut all[slot];
        while q
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) > WINDOW)
        {
            q.pop_front();
        }
        let b = b0 / (1.0 + q.len() as f32 / DECAY_N).sqrt();
        q.push_back(now);
        b
    }

    pub fn pay_slot(&self, slot: usize, b0: f32) -> f32 {
        self.pay_slot_at(slot, b0, Instant::now())
    }

    pub fn pay(&self, m: Mechanic, b0: f32) -> f32 {
        self.pay_at(m, b0, Instant::now())
    }

    /// Occurrences payées dans la fenêtre (pour les tests et les logs).
    pub fn recent(&self, m: Mechanic, now: Instant) -> usize {
        self.paid.lock().map_or(0, |a| {
            a[m.index()]
                .iter()
                .filter(|t| now.saturating_duration_since(**t) <= WINDOW)
                .count()
        })
    }
}

/// Compteurs d'un env (ou de tous) depuis la dernière reprise : épisodes par drill, mécaniques vues en drill,
/// bonus payés par mécanique.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrillReport {
    pub episodes: [u64; scenarios::DRILL_COUNT],
    /// Verdicts de récupération, dans l'ordre de [`RecoveryOutcome::ALL`].
    pub recovery: [u64; 4],
    pub mechanics: Counts,
    pub bonus: Counts,
}

impl DrillReport {
    pub fn add(&mut self, o: &DrillReport) {
        for (a, b) in self.episodes.iter_mut().zip(o.episodes) {
            *a += b;
        }
        for (a, b) in self.recovery.iter_mut().zip(o.recovery) {
            *a += b;
        }
        self.mechanics.add(&o.mechanics);
        self.bonus.add(&o.bonus);
    }
}

/// État du bonus pendant un épisode de drill.
#[derive(Debug, Default)]
pub struct EpisodeBonus {
    /// Suivi du drill de récupération (None dans les autres).
    pub recovery: Option<RecoveryTrack>,
    pub paid: u32,
    /// Occurrences vues sans être encore utiles : (mécanique, tick), payées si un but arrive dans la fenêtre.
    pub pending: Vec<(Mechanic, u64)>,
}

impl EpisodeBonus {
    /// Traite les occurrences vues ce pas (`seen`), à l'instant `tick`, balle en `ball_pos`/`ball_vel`. Renvoie
    /// la récompense de bonus immédiate et incrémente `report.bonus`. `bonus_cap` = plafond d'un bonus.
    pub fn on_step(
        &mut self,
        seen: &Counts,
        tick: u64,
        ball: ([f32; 3], [f32; 3]),
        bonus_cap: f32,
        ledger: &BonusLedger,
        report: &mut DrillReport,
    ) -> f32 {
        let mut reward = 0.0;
        self.pending
            .retain(|(_, t)| tick.saturating_sub(*t) <= GOAL_WINDOW_TICKS);
        let useful = speed_toward_goal(ball.0, ball.1) > USEFUL_SPEED;
        for m in Mechanic::ALL {
            let b0 = base_bonus(m).min(bonus_cap);
            if b0 <= 0.0 {
                continue;
            }
            for _ in 0..seen.get(m) {
                if useful {
                    if self.paid < MAX_PER_EPISODE {
                        reward += ledger.pay(m, b0);
                        report.bonus.inc(m);
                        self.paid += 1;
                    }
                } else if self.pending.len() < MAX_PER_EPISODE as usize {
                    self.pending.push((m, tick));
                }
            }
        }
        reward
    }

    /// But marqué : paie les occurrences en attente (dans la fenêtre, dans la limite de l'épisode).
    pub fn on_goal(
        &mut self,
        tick: u64,
        bonus_cap: f32,
        ledger: &BonusLedger,
        report: &mut DrillReport,
    ) -> f32 {
        let mut reward = 0.0;
        for (m, t) in std::mem::take(&mut self.pending) {
            if self.paid >= MAX_PER_EPISODE {
                break;
            }
            if tick.saturating_sub(t) <= GOAL_WINDOW_TICKS {
                reward += ledger.pay(m, base_bonus(m).min(bonus_cap));
                report.bonus.inc(m);
                self.paid += 1;
            }
        }
        reward
    }
}

/// Verdict d'un drill de récupération.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// Retombée sur les roues, vite, nez vers la direction utile, vitesse conservée : bonus.
    Good,
    /// Retombée sur les roues, sans remplir toutes les conditions.
    Wheels,
    /// Retombée sur le toit ou le côté.
    Bad,
    /// Jamais retombée avant la fin de l'épisode.
    None,
}

impl RecoveryOutcome {
    pub const ALL: [RecoveryOutcome; 4] = [
        RecoveryOutcome::Good,
        RecoveryOutcome::Wheels,
        RecoveryOutcome::Bad,
        RecoveryOutcome::None,
    ];

    /// Valeur du label Prometheus `outcome`.
    pub fn label(self) -> &'static str {
        match self {
            RecoveryOutcome::Good => "good",
            RecoveryOutcome::Wheels => "wheels",
            RecoveryOutcome::Bad => "bad",
            RecoveryOutcome::None => "none",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Ce que le suivi de récupération lit de la voiture et de la balle à chaque décision.
#[derive(Clone, Copy, Debug)]
pub struct RecoveryObs {
    pub tick: u64,
    /// Au moins 3 roues au sol (ou au mur / plafond).
    pub on_ground: bool,
    /// Contact du châssis avec le terrain.
    pub world_contact: bool,
    pub car_pos: [f32; 3],
    pub car_vel: [f32; 3],
    pub forward: [f32; 3],
    pub ball_pos: [f32; 3],
}

/// Direction horizontale utile (unitaire) : vers la balle, ou vers son propre but (−Y) si la balle est derrière la
/// voiture (plus près de son but que la voiture de plus de 500 uu).
pub fn recovery_target(car_pos: [f32; 3], ball_pos: [f32; 3]) -> [f32; 2] {
    let to = if ball_pos[1] < car_pos[1] - 500.0 {
        [-car_pos[0], -5120.0 - car_pos[1]]
    } else {
        [ball_pos[0] - car_pos[0], ball_pos[1] - car_pos[1]]
    };
    let n = to[0].hypot(to[1]).max(1.0);
    [to[0] / n, to[1] / n]
}

/// Durée de chute libre (ticks) depuis `z0` avec la vitesse verticale `vz0`, jusqu'au sol (z = 17).
pub fn free_fall_ticks(z0: f32, vz0: f32) -> f32 {
    let disc = vz0 * vz0 + 2.0 * GRAVITY * (z0 - 17.0).max(0.0);
    (vz0 + disc.sqrt()) / GRAVITY * crate::TICKS_PER_SECOND
}

/// Suivi d'un drill de récupération : détecte le premier contact, rend un verdict 0,2 s plus tard, finit le drill
/// 1 s après le contact.
#[derive(Debug)]
pub struct RecoveryTrack {
    start_tick: u64,
    /// Chute libre prévue depuis l'état de départ (ticks).
    fall_ticks: f32,
    contact_tick: Option<u64>,
    pub verdict: Option<RecoveryOutcome>,
}

impl RecoveryTrack {
    pub fn new(start_tick: u64, z0: f32, vz0: f32) -> Self {
        Self {
            start_tick,
            fall_ticks: free_fall_ticks(z0, vz0),
            contact_tick: None,
            verdict: None,
        }
    }

    /// Avance d'une décision. Renvoie `(verdict nouveau, drill fini)`.
    pub fn step(&mut self, o: &RecoveryObs) -> (Option<RecoveryOutcome>, bool) {
        if self.contact_tick.is_none() && (o.world_contact || o.on_ground) {
            self.contact_tick = Some(o.tick);
        }
        let Some(c) = self.contact_tick else {
            return (None, false);
        };
        let mut new = None;
        if self.verdict.is_none() && o.tick >= c + RECOVERY_SETTLE_TICKS {
            let v = if !o.on_ground {
                RecoveryOutcome::Bad
            } else {
                let airtime = c.saturating_sub(self.start_tick) as f32;
                let fast = airtime <= 1.3 * self.fall_ticks + 30.0;
                let t = recovery_target(o.car_pos, o.ball_pos);
                let h = o.forward[0].hypot(o.forward[1]).max(1e-3);
                let cos = (o.forward[0] * t[0] + o.forward[1] * t[1]) / h;
                let speed = o.car_vel[0] * t[0] + o.car_vel[1] * t[1];
                if fast && cos >= RECOVERY_MIN_COS && speed >= RECOVERY_MIN_SPEED {
                    RecoveryOutcome::Good
                } else {
                    RecoveryOutcome::Wheels
                }
            };
            self.verdict = Some(v);
            new = Some(v);
        }
        (new, o.tick >= c + RECOVERY_END_TICKS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(m: Mechanic) -> Counts {
        let mut c = Counts::default();
        c.inc(m);
        c
    }

    #[test]
    fn decroissance_en_racine() {
        let l = BonusLedger::default();
        let now = Instant::now();
        let first = l.pay_at(Mechanic::Flick, 0.6, now);
        assert!((first - 0.6).abs() < 1e-6);
        for _ in 0..998 {
            l.pay_at(Mechanic::Flick, 0.6, now);
        }
        // n = 999 → 0,6 / √1,999
        let b = l.pay_at(Mechanic::Flick, 0.6, now);
        assert!((b - 0.6 / (1.999f32).sqrt()).abs() < 1e-4, "{b}");
        // Autre mécanique indépendante.
        assert!((l.pay_at(Mechanic::Dribble, 0.3, now) - 0.3).abs() < 1e-6);
        // Hors fenêtre : on repart de b0.
        let later = now + WINDOW + Duration::from_secs(1);
        assert!((l.pay_at(Mechanic::Flick, 0.6, later) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn jamais_de_bonus_pour_le_toucher_simple() {
        assert_eq!(base_bonus(Mechanic::Touch), 0.0);
        let (l, mut r, mut e) = (
            BonusLedger::default(),
            DrillReport::default(),
            EpisodeBonus::default(),
        );
        let g = e.on_step(
            &one(Mechanic::Touch),
            10,
            ([0.0, 0.0, 100.0], [0.0, 3000.0, 0.0]),
            1.0,
            &l,
            &mut r,
        );
        assert_eq!(g, 0.0);
        assert_eq!(r.bonus.total(), 0);
    }

    #[test]
    fn utile_paye_inutile_attend_et_deux_max() {
        let (l, mut r, mut e) = (
            BonusLedger::default(),
            DrillReport::default(),
            EpisodeBonus::default(),
        );
        // Balle qui s'éloigne du but : rien de versé, mise en attente.
        let away = ([0.0, 0.0, 100.0], [0.0, -2000.0, 0.0]);
        assert_eq!(
            e.on_step(&one(Mechanic::Flick), 100, away, 1.0, &l, &mut r),
            0.0
        );
        assert_eq!(e.pending.len(), 1);
        // Balle vers le but : payé (0,6) ; la 3e occurrence ne paie plus.
        let toward = ([0.0, 0.0, 100.0], [0.0, 2000.0, 0.0]);
        let g = e.on_step(&one(Mechanic::Flick), 200, toward, 1.0, &l, &mut r);
        assert!((g - 0.6).abs() < 1e-5);
        let mut two = one(Mechanic::Dribble);
        two.inc(Mechanic::Dribble);
        let g2 = e.on_step(&two, 210, toward, 1.0, &l, &mut r);
        assert!((g2 - 0.3).abs() < 1e-5, "un seul des deux paie : {g2}");
        assert_eq!(e.paid, 2);
        assert_eq!(r.bonus.get(Mechanic::Dribble), 1);
        // Plus rien en but : plafond atteint.
        assert_eq!(e.on_goal(220, 1.0, &l, &mut r), 0.0);
    }

    #[test]
    fn but_dans_les_3_secondes_paie_le_differe() {
        let (l, mut r, mut e) = (
            BonusLedger::default(),
            DrillReport::default(),
            EpisodeBonus::default(),
        );
        let slow = ([0.0, 0.0, 100.0], [0.0, 100.0, 0.0]);
        e.on_step(&one(Mechanic::AerialTouch), 100, slow, 1.0, &l, &mut r);
        let g = e.on_goal(100 + 300, 1.0, &l, &mut r);
        assert!((g - 0.4).abs() < 1e-5);
        // Trop tard (> 3 s).
        let mut e2 = EpisodeBonus::default();
        e2.on_step(&one(Mechanic::AerialTouch), 100, slow, 1.0, &l, &mut r);
        assert_eq!(e2.on_goal(100 + 400, 1.0, &l, &mut r), 0.0);
    }

    #[test]
    fn plafond_dix_pour_cent_du_but() {
        let (l, mut r, mut e) = (
            BonusLedger::default(),
            DrillReport::default(),
            EpisodeBonus::default(),
        );
        let toward = ([0.0, 0.0, 100.0], [0.0, 2000.0, 0.0]);
        // But à +5 → plafond 0,5 : FlipReset (b0 = 1,0) est ramené à 0,5.
        let g = e.on_step(&one(Mechanic::FlipReset), 1, toward, 0.5, &l, &mut r);
        assert!((g - 0.5).abs() < 1e-5);
    }

    fn recovery_obs(tick: u64, on_ground: bool) -> RecoveryObs {
        RecoveryObs {
            tick,
            on_ground,
            world_contact: true,
            car_pos: [0.0, 0.0, 17.0],
            car_vel: [0.0, 800.0, 0.0],
            forward: [0.0, 1.0, 0.0],
            ball_pos: [0.0, 2000.0, 93.0],
        }
    }

    #[test]
    fn recovery_bonne_retombee_mauvaise_et_cible() {
        // Départ à z = 600 (chute libre ≈ 1,36 s ≈ 163 ticks) : atterrir au tick 170 est rapide.
        let mut t = RecoveryTrack::new(0, 600.0, 0.0);
        let airborne = RecoveryObs {
            world_contact: false,
            ..recovery_obs(100, false)
        };
        assert_eq!(t.step(&airborne), (None, false));
        assert_eq!(t.step(&recovery_obs(170, true)).0, None);
        let (v, done) = t.step(&recovery_obs(170 + RECOVERY_SETTLE_TICKS, true));
        assert_eq!(v, Some(RecoveryOutcome::Good));
        assert!(!done);
        assert!(t.step(&recovery_obs(170 + RECOVERY_END_TICKS, true)).1);
        // Sur le toit : mauvais.
        let mut t = RecoveryTrack::new(0, 600.0, 0.0);
        t.step(&recovery_obs(170, false));
        assert_eq!(
            t.step(&recovery_obs(200, false)).0,
            Some(RecoveryOutcome::Bad)
        );
        // Nez à l'opposé de la balle : sur les roues mais pas de bonus.
        let mut t = RecoveryTrack::new(0, 600.0, 0.0);
        t.step(&recovery_obs(170, true));
        let back = RecoveryObs {
            forward: [0.0, -1.0, 0.0],
            ..recovery_obs(200, true)
        };
        assert_eq!(t.step(&back).0, Some(RecoveryOutcome::Wheels));
        // Trop lent à retomber (rattrapée par la chute libre prévue ×1,3 + 0,25 s).
        let mut t = RecoveryTrack::new(0, 600.0, 0.0);
        t.step(&recovery_obs(400, true));
        assert_eq!(
            t.step(&recovery_obs(430, true)).0,
            Some(RecoveryOutcome::Wheels)
        );
        // Balle derrière la voiture : on vise son propre but (−Y).
        let t = recovery_target([0.0, 2000.0, 17.0], [0.0, 0.0, 93.0]);
        assert!(t[1] < -0.9);
        let t = recovery_target([0.0, 0.0, 17.0], [0.0, 2000.0, 93.0]);
        assert!(t[1] > 0.9);
    }

    #[test]
    fn chute_libre() {
        let t = free_fall_ticks(17.0 + 650.0 * 0.5, 0.0);
        assert!(
            (t - 2.0f32.sqrt() * 120.0 * 0.5 * 2.0f32.sqrt()).abs() < 1.0,
            "{t}"
        );
    }

    #[test]
    fn vitesse_vers_le_but() {
        assert!(speed_toward_goal([0.0, 0.0, 321.0], [0.0, 1000.0, 0.0]) > 900.0);
        assert!(speed_toward_goal([0.0, 0.0, 321.0], [0.0, -1000.0, 0.0]) < 0.0);
    }
}
