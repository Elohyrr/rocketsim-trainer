//! Métriques Prometheus (`/metrics`) — liste fermée de `docs/metrics.md`, texte au format d'exposition 0.0.4.
//!
//! Aucun label à valeur libre : les seules séries étiquetées sont `result`, `reason` (énumérations fixes).

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Plafond dur de séries (voir `docs/metrics.md`).
pub const MAX_SERIES: usize = 500;
const BUCKETS: [f64; 7] = [0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0];

/// Valeurs courantes de l'entraînement.
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub steps_total: u64,
    pub steps_per_second: f64,
    pub episodes_total: u64,
    pub reward_mean: f64,
    pub episode_length_mean: f64,
    pub policy_loss: f64,
    pub value_loss: f64,
    pub entropy: f64,
    pub kl: f64,
    pub iter_bucket_counts: [u64; BUCKETS.len()],
    pub iter_count: u64,
    pub iter_sum: f64,
    pub checkpoint_age_seconds: f64,
    pub checkpoints_ok: u64,
    pub checkpoints_error: u64,
    pub paused: bool,
    /// La pause en cours vient d'une lecture vidéo Jellyfin (D6) ; sinon elle est « manual ».
    pub pause_jellyfin: bool,
    pub curriculum_stage: u32,
    pub unrecognized_sequences_total: u64,
    pub touch_total: u64,
    pub goals_scored_total: u64,
    pub goals_conceded_total: u64,
    /// Fraction d'épisodes finis par un but marqué (fenêtre glissante du curriculum).
    pub goal_rate: f64,
}

impl Metrics {
    pub fn observe_iteration(&mut self, seconds: f64) {
        self.iter_count += 1;
        self.iter_sum += seconds;
        for (i, b) in BUCKETS.iter().enumerate() {
            if seconds <= *b {
                self.iter_bucket_counts[i] += 1;
            }
        }
    }

    /// Texte d'exposition Prometheus.
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut g = |name: &str, kind: &str, v: f64| {
            let _ = writeln!(s, "# TYPE {name} {kind}\n{name} {v}");
        };
        g("trainer_steps_total", "counter", self.steps_total as f64);
        g("trainer_steps_per_second", "gauge", self.steps_per_second);
        g(
            "trainer_episodes_total",
            "counter",
            self.episodes_total as f64,
        );
        g("trainer_reward_mean", "gauge", self.reward_mean);
        g(
            "trainer_episode_length_mean",
            "gauge",
            self.episode_length_mean,
        );
        g("trainer_policy_loss", "gauge", self.policy_loss);
        g("trainer_value_loss", "gauge", self.value_loss);
        g("trainer_entropy", "gauge", self.entropy);
        g("trainer_kl", "gauge", self.kl);
        g(
            "trainer_checkpoint_age_seconds",
            "gauge",
            self.checkpoint_age_seconds,
        );
        g("trainer_paused", "gauge", f64::from(u8::from(self.paused)));
        g(
            "trainer_curriculum_stage",
            "gauge",
            f64::from(self.curriculum_stage),
        );
        g(
            "trainer_unrecognized_sequences_total",
            "counter",
            self.unrecognized_sequences_total as f64,
        );
        g("trainer_goal_rate", "gauge", self.goal_rate);
        let _ = writeln!(s, "# TYPE trainer_checkpoints_total counter");
        let _ = writeln!(
            s,
            "trainer_checkpoints_total{{result=\"ok\"}} {}",
            self.checkpoints_ok
        );
        let _ = writeln!(
            s,
            "trainer_checkpoints_total{{result=\"error\"}} {}",
            self.checkpoints_error
        );
        let _ = writeln!(s, "# TYPE trainer_pause_reason gauge");
        for r in ["none", "jellyfin", "manual"] {
            let on = match r {
                "none" => !self.paused,
                "jellyfin" => self.paused && self.pause_jellyfin,
                _ => self.paused && !self.pause_jellyfin,
            };
            let _ = writeln!(s, "trainer_pause_reason{{reason=\"{r}\"}} {}", u8::from(on));
        }
        let _ = writeln!(s, "# TYPE trainer_mechanic_total counter");
        let _ = writeln!(
            s,
            "trainer_mechanic_total{{mechanic=\"touch\"}} {}",
            self.touch_total
        );
        let _ = writeln!(s, "# TYPE trainer_goals_total counter");
        let _ = writeln!(
            s,
            "trainer_goals_total{{side=\"scored\"}} {}",
            self.goals_scored_total
        );
        let _ = writeln!(
            s,
            "trainer_goals_total{{side=\"conceded\"}} {}",
            self.goals_conceded_total
        );
        let _ = writeln!(s, "# TYPE trainer_iteration_seconds histogram");
        for (b, c) in BUCKETS.iter().zip(&self.iter_bucket_counts) {
            let _ = writeln!(s, "trainer_iteration_seconds_bucket{{le=\"{b}\"}} {c}");
        }
        let _ = writeln!(
            s,
            "trainer_iteration_seconds_bucket{{le=\"+Inf\"}} {}",
            self.iter_count
        );
        let _ = writeln!(s, "trainer_iteration_seconds_sum {}", self.iter_sum);
        let _ = writeln!(s, "trainer_iteration_seconds_count {}", self.iter_count);
        s
    }
}

/// Nombre de séries (lignes d'échantillons) d'un texte d'exposition.
pub fn count_series(text: &str) -> usize {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .count()
}

/// Sert `/metrics` sur `addr` dans un thread d'arrière-plan. Retourne l'adresse réellement liée.
pub fn serve(addr: &str, m: Arc<Mutex<Metrics>>) -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    std::thread::Builder::new().name("metrics".into()).spawn(move || {
        let started = Instant::now();
        for conn in listener.incoming().flatten() {
            let mut conn = conn;
            let mut buf = [0u8; 1024];
            let n = conn.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let (status, body) = if req.starts_with("GET /metrics") {
                let mut body = m.lock().map(|m| m.render()).unwrap_or_default();
                let _ = writeln!(body, "# TYPE process_uptime_seconds gauge\nprocess_uptime_seconds {}", started.elapsed().as_secs_f64());
                ("200 OK", body)
            } else if req.starts_with("GET /healthz") {
                ("200 OK", "ok\n".to_string())
            } else {
                ("404 Not Found", String::new())
            };
            let _ = write!(
                conn,
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    })?;
    Ok(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_stay_far_below_the_cap() {
        let mut m = Metrics::default();
        for i in 0..1000 {
            m.observe_iteration(f64::from(i) * 0.1);
        }
        let n = count_series(&m.render());
        assert!(n < 100, "objectif < 100 séries, obtenu {n}");
        assert!(n < MAX_SERIES);
    }

    #[test]
    fn labels_are_from_closed_enums() {
        let t = Metrics::default().render();
        for l in t.lines().filter(|l| l.contains('{')) {
            let ok = [
                "result=\"ok\"",
                "result=\"error\"",
                "reason=\"none\"",
                "reason=\"jellyfin\"",
                "reason=\"manual\"",
                "mechanic=\"touch\"",
                "side=\"scored\"",
                "side=\"conceded\"",
                "le=\"",
            ];
            assert!(ok.iter().any(|k| l.contains(k)), "{l}");
        }
    }

    #[test]
    fn serves_metrics_over_http() {
        let m = Arc::new(Mutex::new(Metrics {
            steps_total: 42,
            ..Default::default()
        }));
        let addr = serve("127.0.0.1:0", m).unwrap();
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 200"));
        assert!(out.contains("trainer_steps_total 42"));
    }
}
