//! Bonus de mécanique des drills (jamais en match ni en self-play), voir `docs/drills.md`.
//!
//! Chaque occurrence détectée par `mechanics` pendant un drill peut rapporter un petit bonus : une fois par
//! occurrence, au plus [`MAX_PER_EPISODE`] par épisode, seulement si elle est utile (balle vers le but adverse
//! à plus de [`USEFUL_SPEED`] uu/s, ou but dans les [`GOAL_WINDOW_TICKS`] ticks), et décroissant avec le nombre
//! d'occurrences payées sur la dernière heure : `b0 / √(1 + n/N)`.

use mechanics::{Counts, Mechanic};
use scenarios::Drill;
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
/// Case du registre du bonus de récupération (après les mécaniques du catalogue).
pub const RECOVERY_SLOT: usize = mechanics::MECH_COUNT;
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
/// Case du registre du bonus « but après un virage ou un flick avec la balle conservée ».
pub const DRIBBLE_GOAL_SLOT: usize = mechanics::MECH_COUNT + 1;
/// Case du registre de l'arrêt du gardien.
pub const KEEPER_SLOT: usize = mechanics::MECH_COUNT + 2;
/// But marqué dans les [`CARRY_GOAL_WINDOW_TICKS`] suivant un virage de dribble ou un flick : bonus de base.
pub const DRIBBLE_GOAL_BONUS: f32 = 0.5;
/// Fenêtre (6 s à 120 Hz) entre le virage / flick et le but.
pub const CARRY_GOAL_WINDOW_TICKS: u64 = 720;
/// Virages de dribble payés au plus par épisode.
pub const MAX_TURNS_PER_EPISODE: u32 = 3;
/// Arrêt du gardien : bonus de base ; pénalité en plus si but encaissé (en plus de la pénalité de but contre son camp).
pub const KEEPER_SAVE_BONUS: f32 = 1.0;
pub const KEEPER_CONCEDE_PENALTY: f32 = 1.0;
/// La balle menace la cage au-delà de cette vitesse vers elle (uu/s) ; arrêtée en dessous de [`KEEPER_SAVED_SPEED`].
pub const KEEPER_THREAT_SPEED: f32 = 500.0;
pub const KEEPER_SAVED_SPEED: f32 = 200.0;
/// Ticks entre l'arrêt et la fin de l'épisode (1,5 s).
pub const KEEPER_CLEAR_TICKS: u64 = 180;

/// Bonus de base `b0` (en récompense, pour un but à +10 ; plafonné à 10 % du but). 0 = jamais de bonus.
pub fn base_bonus(m: Mechanic) -> f32 {
    match m {
        Mechanic::FlipReset => 1.0,
        Mechanic::AirDribble | Mechanic::CeilingShot => 0.8,
        Mechanic::Flick => 0.6,
        Mechanic::WallShot => 0.5,
        Mechanic::AerialTouch | Mechanic::DoubleTouch => 0.4,
        Mechanic::Dribble => 0.3,
        // Virage avec la balle conservée : faible, payé tout de suite (≤ 3 par épisode).
        Mechanic::DribbleTurn => 0.2,
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
pub struct BonusLedger {
    paid: Mutex<Vec<VecDeque<Instant>>>,
}

impl Default for BonusLedger {
    fn default() -> Self {
        Self {
            paid: Mutex::new(vec![VecDeque::new(); mechanics::MECH_COUNT + 3]),
        }
    }
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

    /// Comme [`BonusLedger::pay_at`] pour une case du registre (les mécaniques puis [`RECOVERY_SLOT`]).
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
    /// Réussites par drill (voir [`EpisodeBonus::success`]).
    pub success: [u64; scenarios::DRILL_COUNT],
    /// Buts payés après un virage de dribble ou un flick.
    pub dribble_goal: u64,
    /// Arrêts du gardien payés.
    pub saves: u64,
}

impl DrillReport {
    pub fn add(&mut self, o: &DrillReport) {
        for (a, b) in self.episodes.iter_mut().zip(o.episodes) {
            *a += b;
        }
        for (a, b) in self.recovery.iter_mut().zip(o.recovery) {
            *a += b;
        }
        for (a, b) in self.success.iter_mut().zip(o.success) {
            *a += b;
        }
        self.dribble_goal += o.dribble_goal;
        self.saves += o.saves;
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
    /// Le drill est réussi (but, arrêt, toucher aérien… selon la famille : voir [`EpisodeBonus::on_events`]).
    pub success: bool,
    /// Dernier virage de dribble ou flick vu (tick) : un but dans les [`CARRY_GOAL_WINDOW_TICKS`] est payé.
    pub last_carry: Option<u64>,
    pub turns_paid: u32,
    pub goal_paid: bool,
    /// Suivi du gardien (drill `keeper` seulement).
    pub keeper: Option<KeeperTrack>,
}

/// Ce qu'un pas de drill a produit, pour [`EpisodeBonus::on_events`].
#[derive(Clone, Copy, Debug)]
pub struct StepEvents<'a> {
    pub tick: u64,
    pub touched: bool,
    pub ball: ([f32; 3], [f32; 3]),
    /// La voiture est en l'air (moins de 3 roues au sol).
    pub car_airborne: bool,
    pub our_goal: bool,
    pub own_goal: bool,
    pub seen: &'a Counts,
}

/// Vitesse de la balle vers SA cage (−Y), uu/s.
pub fn speed_toward_own_goal(ball_pos: [f32; 3], ball_vel: [f32; 3]) -> f32 {
    speed_toward_goal(
        [-ball_pos[0], -ball_pos[1], ball_pos[2]],
        [-ball_vel[0], -ball_vel[1], ball_vel[2]],
    )
}

/// Suivi du gardien : un toucher qui arrête une balle qui menaçait la cage est un arrêt ; fin 1,5 s après.
#[derive(Debug)]
pub struct KeeperTrack {
    prev: ([f32; 3], [f32; 3]),
    pub saved_tick: Option<u64>,
}

impl KeeperTrack {
    pub fn new(ball_pos: [f32; 3], ball_vel: [f32; 3]) -> Self {
        Self {
            prev: (ball_pos, ball_vel),
            saved_tick: None,
        }
    }

    /// Avance d'une décision : `(arrêt nouveau, épisode fini)`.
    pub fn step(&mut self, tick: u64, touched: bool, ball: ([f32; 3], [f32; 3])) -> (bool, bool) {
        let mut saved = false;
        if touched
            && self.saved_tick.is_none()
            && speed_toward_own_goal(self.prev.0, self.prev.1) > KEEPER_THREAT_SPEED
            && speed_toward_own_goal(ball.0, ball.1) < KEEPER_SAVED_SPEED
        {
            self.saved_tick = Some(tick);
            saved = true;
        }
        self.prev = ball;
        let done = self
            .saved_tick
            .is_some_and(|t| tick >= t + KEEPER_CLEAR_TICKS);
        (saved, done)
    }
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
        if seen.get(Mechanic::Flick) > 0 || seen.get(Mechanic::DribbleTurn) > 0 {
            self.last_carry = Some(tick);
        }
        for m in Mechanic::ALL {
            let b0 = base_bonus(m).min(bonus_cap);
            if b0 <= 0.0 {
                continue;
            }
            if m == Mechanic::DribbleTurn {
                for _ in 0..seen.get(m) {
                    if self.turns_paid < MAX_TURNS_PER_EPISODE {
                        reward += ledger.pay(m, b0);
                        report.bonus.inc(m);
                        self.turns_paid += 1;
                    }
                }
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
        if !self.goal_paid
            && self
                .last_carry
                .is_some_and(|t| tick.saturating_sub(t) <= CARRY_GOAL_WINDOW_TICKS)
        {
            self.goal_paid = true;
            reward += ledger.pay_slot(DRIBBLE_GOAL_SLOT, DRIBBLE_GOAL_BONUS.min(bonus_cap));
            report.dribble_goal += 1;
        }
        reward
    }

    /// Réussite de la famille et récompenses propres au gardien. Renvoie `(récompense, épisode fini)`.
    /// Réussite : but pour les tirs, dribbles, murs, rebonds ; toucher aérien (balle > 300 uu, voiture en l'air)
    /// pour `aerial_touch`, `air_dribble`, `aerial_high` ; flip reset détecté pour `flip_reset` ; arrêt sans but
    /// encaissé pour `keeper`. La récupération est jugée par [`RecoveryTrack`].
    pub fn on_events(
        &mut self,
        d: Drill,
        e: &StepEvents,
        bonus_cap: f32,
        ledger: &BonusLedger,
        report: &mut DrillReport,
    ) -> (f32, bool) {
        let mut reward = 0.0;
        let mut done = false;
        match d {
            Drill::Keeper => {
                if let Some(k) = self.keeper.as_mut() {
                    let (saved, fin) = k.step(e.tick, e.touched, e.ball);
                    if saved {
                        self.success = true;
                        report.saves += 1;
                        reward += ledger.pay_slot(KEEPER_SLOT, KEEPER_SAVE_BONUS.min(bonus_cap));
                    }
                    done = fin;
                }
                if e.own_goal {
                    self.success = false;
                    reward -= KEEPER_CONCEDE_PENALTY;
                }
            }
            Drill::AerialTouch | Drill::AirDribble | Drill::AerialHigh => {
                if e.touched && e.car_airborne && e.ball.0[2] > 300.0 {
                    self.success = true;
                }
            }
            Drill::FlipReset => {
                if e.seen.get(Mechanic::FlipReset) > 0 {
                    self.success = true;
                }
            }
            Drill::Recovery => {}
            _ => {
                if e.our_goal {
                    self.success = true;
                }
            }
        }
        (reward, done)
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
        // Plus de bonus différé en but (plafond atteint) : seul reste le bonus « but après flick ».
        let g = e.on_goal(220, 1.0, &l, &mut r);
        assert!((g - DRIBBLE_GOAL_BONUS).abs() < 1e-5, "{g}");
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
    fn virage_paye_tout_de_suite_et_but_apres_virage_ou_flick() {
        let (l, mut r, mut e) = (
            BonusLedger::default(),
            DrillReport::default(),
            EpisodeBonus::default(),
        );
        // Balle portée (vitesse vers le but nulle) : le virage est payé quand même, 3 au plus par épisode.
        let carried = ([0.0, 0.0, 145.0], [0.0, 0.0, 0.0]);
        let g = e.on_step(&one(Mechanic::DribbleTurn), 100, carried, 1.0, &l, &mut r);
        assert!((g - 0.2).abs() < 1e-5, "{g}");
        for t in 101..110 {
            e.on_step(&one(Mechanic::DribbleTurn), t, carried, 1.0, &l, &mut r);
        }
        assert_eq!(r.bonus.get(Mechanic::DribbleTurn), 3);
        // Un virage ne consomme pas le plafond des autres bonus.
        assert_eq!(e.paid, 0);
        // But dans les 6 s : bonus unique ; trop tard : rien.
        let g = e.on_goal(100 + 600, 1.0, &l, &mut r);
        assert!((g - DRIBBLE_GOAL_BONUS).abs() < 1e-5, "{g}");
        assert_eq!(e.on_goal(100 + 610, 1.0, &l, &mut r), 0.0);
        assert_eq!(r.dribble_goal, 1);
        let mut e2 = EpisodeBonus::default();
        e2.on_step(&one(Mechanic::Flick), 100, carried, 1.0, &l, &mut r);
        assert_eq!(e2.on_goal(100 + 800, 1.0, &l, &mut r), 0.0);
        // Un simple dribble en ligne droite : seul le bonus différé du dribble (0,3), pas le bonus de virage.
        let mut e3 = EpisodeBonus::default();
        e3.on_step(&one(Mechanic::Dribble), 100, carried, 1.0, &l, &mut r);
        let g = e3.on_goal(150, 1.0, &l, &mut r);
        assert!((g - 0.3).abs() < 1e-5, "{g}");
        assert!(!e3.goal_paid);
    }

    fn ev<'a>(
        tick: u64,
        touched: bool,
        ball: ([f32; 3], [f32; 3]),
        seen: &'a Counts,
    ) -> StepEvents<'a> {
        StepEvents {
            tick,
            touched,
            ball,
            car_airborne: false,
            our_goal: false,
            own_goal: false,
            seen,
        }
    }

    #[test]
    fn gardien_arret_but_encaisse_et_fin() {
        let l = BonusLedger::default();
        let none = Counts::default();
        // Balle tirée vers sa cage (−Y), arrêtée par un toucher : arrêt payé une fois, fin 1,5 s après.
        let mut r = DrillReport::default();
        let mut e = EpisodeBonus {
            keeper: Some(KeeperTrack::new([0.0, -1000.0, 200.0], [0.0, -2000.0, 0.0])),
            ..Default::default()
        };
        let stopped = ([0.0, -1500.0, 200.0], [0.0, 100.0, 0.0]);
        let (g, done) = e.on_events(
            Drill::Keeper,
            &ev(10, true, stopped, &none),
            1.0,
            &l,
            &mut r,
        );
        assert!((g - KEEPER_SAVE_BONUS).abs() < 1e-5 && !done && e.success);
        assert_eq!(r.saves, 1);
        let (g, _) = e.on_events(
            Drill::Keeper,
            &ev(20, true, stopped, &none),
            1.0,
            &l,
            &mut r,
        );
        assert_eq!(g, 0.0);
        let (_, done) = e.on_events(
            Drill::Keeper,
            &ev(10 + KEEPER_CLEAR_TICKS, false, stopped, &none),
            1.0,
            &l,
            &mut r,
        );
        assert!(done);
        // Toucher qui laisse la balle filer vers la cage : pas d'arrêt ; but encaissé : pénalité, échec.
        let mut e = EpisodeBonus {
            keeper: Some(KeeperTrack::new([0.0, -1000.0, 200.0], [0.0, -2000.0, 0.0])),
            ..Default::default()
        };
        let still_going = ([0.0, -1500.0, 200.0], [0.0, -1500.0, 0.0]);
        e.on_events(
            Drill::Keeper,
            &ev(10, true, still_going, &none),
            1.0,
            &l,
            &mut r,
        );
        assert!(!e.success);
        let mut conceded = ev(30, false, still_going, &none);
        conceded.own_goal = true;
        let (g, _) = e.on_events(Drill::Keeper, &conceded, 1.0, &l, &mut r);
        assert!((g + KEEPER_CONCEDE_PENALTY).abs() < 1e-5 && !e.success);
    }

    #[test]
    fn reussite_par_famille() {
        let (l, mut r) = (BonusLedger::default(), DrillReport::default());
        let none = Counts::default();
        let ball = ([0.0, 0.0, 600.0], [0.0; 3]);
        // Aérien : toucher en l'air avec la balle haute.
        let mut e = EpisodeBonus::default();
        e.on_events(
            Drill::AerialHigh,
            &ev(1, true, ball, &none),
            1.0,
            &l,
            &mut r,
        );
        assert!(!e.success, "voiture au sol");
        let mut air = ev(2, true, ball, &none);
        air.car_airborne = true;
        e.on_events(Drill::AerialHigh, &air, 1.0, &l, &mut r);
        assert!(e.success);
        // Tirs : le but.
        let mut e = EpisodeBonus::default();
        e.on_events(Drill::ShotRoll, &ev(1, true, ball, &none), 1.0, &l, &mut r);
        assert!(!e.success);
        let mut goal = ev(2, false, ball, &none);
        goal.our_goal = true;
        e.on_events(Drill::ShotRoll, &goal, 1.0, &l, &mut r);
        assert!(e.success);
        // Flip reset : la méca vue.
        let mut e = EpisodeBonus::default();
        e.on_events(
            Drill::FlipReset,
            &ev(1, false, ball, &one(Mechanic::FlipReset)),
            1.0,
            &l,
            &mut r,
        );
        assert!(e.success);
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
