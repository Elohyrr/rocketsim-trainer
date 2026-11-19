//! Métriques Prometheus (`/metrics`) — liste fermée de `docs/metrics.md`, texte au format d'exposition 0.0.4.
//!
//! Aucun label à valeur libre : les seules séries étiquetées sont `result`, `reason` (énumérations fixes).

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// Entropie visée (nat), coefficient d'entropie adaptatif et lr courant (exploration, `explore.rs`).
    pub entropy_target: f64,
    pub ent_coef: f64,
    pub learning_rate: f64,
    /// Réduction de la cible d'entropie par le garde-fou (1 = aucune) et nombre de déclenchements.
    pub entropy_scale: f64,
    pub entropy_backoffs: u64,
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
    /// Mécaniques détectées depuis le démarrage, contexte `match`.
    pub mechanics: mechanics::Counts,
    /// Mécaniques détectées en drill (contexte `drill`), bonus payés par mécanique, épisodes par drill.
    pub drill_mechanics: mechanics::Counts,
    pub drill_bonus: mechanics::Counts,
    pub drill_episodes: [u64; scenarios::DRILL_COUNT],
    /// Verdicts du drill de récupération (ordre de `rl_env::drill::RecoveryOutcome::ALL`).
    pub drill_recovery: [u64; 4],
    /// Boosts ramassés par l'agent (petits / gros) depuis le démarrage ; niveau moyen (0–100) et part du temps
    /// à 0 de boost, sur la dernière itération avec des décisions de match.
    pub boost_pickups_small: u64,
    pub boost_pickups_big: u64,
    pub boost_mean: f64,
    pub boost_empty_fraction: f64,
    /// Vitesse de balle vers le but adverse juste après un toucher de l'agent en match : moyenne sur la dernière itération avec des touchers, et record depuis le démarrage (uu/s).
    pub hit_speed_mean: f64,
    pub hit_speed_record: f32,
    /// Réussites par drill, buts payés après virage/flick, arrêts du gardien (voir `docs/drills.md`).
    pub drill_success: [u64; scenarios::DRILL_COUNT],
    pub drill_dribble_goal: u64,
    pub drill_saves: u64,
    /// Clips enregistrés depuis le démarrage (`kind="known"` : mécanique du catalogue ; `novel` : à venir).
    pub clips_saved_known: u64,
    /// Plus bas score de qualité du hall of fame (0 s'il est vide).
    pub clips_top_score: f64,
    pub goals_scored_total: u64,
    pub goals_conceded_total: u64,
    /// Fraction d'épisodes finis par un but marqué (fenêtre glissante du curriculum).
    pub goal_rate: f64,
    /// Coups d'envoi terminés (paliers 5 et 6) : gagnés / perdus / neutres.
    pub kickoffs_won: u64,
    pub kickoffs_lost: u64,
    pub kickoffs_neutral: u64,
    /// Temps moyen (s) jusqu'au premier toucher et vitesse moyenne (uu/s) au contact, dernière itération avec
    /// au moins un kickoff touché (0 tant qu'il n'y en a pas eu).
    pub kickoff_first_touch_seconds: f64,
    pub kickoff_touch_speed: f64,
    /// Self-play solide (`docs/self-play.md`) : Elo lissé suivi par le curriculum et son record ; Elo estimé et taux
    /// de victoire contre chaque référence (`elo::REFS`, 0 tant que non mesuré) ; matchs d'évaluation joués ;
    /// secondes depuis la dernière mesure (0 tant qu'il n'y en a pas eu).
    pub elo: f64,
    pub elo_best: f64,
    pub elo_vs: [f64; 3],
    pub elo_winrate: [f64; 3],
    pub elo_matches_total: u64,
    pub elo_age_seconds: f64,
    /// Pool d'anciennes politiques : taille, instantanés pris depuis le démarrage.
    pub pool_size: u64,
    pub pool_snapshots_total: u64,
    /// Buts du self-play contre la copie gelée (`[0]`) et contre le pool (`[1]`) : `[marqués, encaissés]`.
    pub selfplay_goals: [[u64; 2]; 2],
    /// Changements de palier depuis le démarrage : promotions, régressions.
    pub stage_up_total: u64,
    pub stage_down_total: u64,
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
        g("trainer_entropy_target", "gauge", self.entropy_target);
        g("trainer_entropy_coef", "gauge", self.ent_coef);
        g("trainer_learning_rate", "gauge", self.learning_rate);
        g("trainer_entropy_guard_scale", "gauge", self.entropy_scale);
        g(
            "trainer_entropy_guard_backoffs_total",
            "counter",
            self.entropy_backoffs as f64,
        );
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
        g(
            "trainer_kickoff_first_touch_seconds",
            "gauge",
            self.kickoff_first_touch_seconds,
        );
        g(
            "trainer_kickoff_touch_speed",
            "gauge",
            self.kickoff_touch_speed,
        );
        g("trainer_elo", "gauge", self.elo);
        g("trainer_elo_best", "gauge", self.elo_best);
        g(
            "trainer_elo_matches_total",
            "counter",
            self.elo_matches_total as f64,
        );
        g("trainer_elo_age_seconds", "gauge", self.elo_age_seconds);
        g("trainer_pool_size", "gauge", self.pool_size as f64);
        g(
            "trainer_pool_snapshots_total",
            "counter",
            self.pool_snapshots_total as f64,
        );
        let _ = writeln!(s, "# TYPE trainer_elo_vs gauge");
        for (r, v) in crate::elo::REFS.iter().zip(self.elo_vs) {
            let _ = writeln!(s, "trainer_elo_vs{{ref=\"{r}\"}} {v}");
        }
        let _ = writeln!(s, "# TYPE trainer_elo_winrate gauge");
        for (r, v) in crate::elo::REFS.iter().zip(self.elo_winrate) {
            let _ = writeln!(s, "trainer_elo_winrate{{ref=\"{r}\"}} {v}");
        }
        let _ = writeln!(s, "# TYPE trainer_selfplay_goals_total counter");
        for (o, g) in ["current", "pool"].iter().zip(self.selfplay_goals) {
            for (side, v) in ["scored", "conceded"].iter().zip(g) {
                let _ = writeln!(
                    s,
                    "trainer_selfplay_goals_total{{opponent=\"{o}\",side=\"{side}\"}} {v}"
                );
            }
        }
        let _ = writeln!(s, "# TYPE trainer_stage_changes_total counter");
        let _ = writeln!(
            s,
            "trainer_stage_changes_total{{direction=\"up\"}} {}",
            self.stage_up_total
        );
        let _ = writeln!(
            s,
            "trainer_stage_changes_total{{direction=\"down\"}} {}",
            self.stage_down_total
        );
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
        for m in mechanics::Mechanic::ALL {
            for (ctx, n) in [
                ("match", self.mechanics.get(m)),
                ("drill", self.drill_mechanics.get(m)),
            ] {
                let _ = writeln!(
                    s,
                    "trainer_mechanic_total{{mechanic=\"{}\",context=\"{ctx}\"}} {n}",
                    m.label()
                );
            }
        }
        let _ = writeln!(s, "# TYPE trainer_mechanic_record gauge");
        for m in mechanics::Mechanic::ALL
            .into_iter()
            .filter(|m| m.has_record())
        {
            let best = self
                .mechanics
                .records()
                .get(m)
                .max(self.drill_mechanics.records().get(m));
            let _ = writeln!(
                s,
                "trainer_mechanic_record{{mechanic=\"{}\"}} {best}",
                m.label()
            );
        }
        let _ = writeln!(s, "# TYPE trainer_drill_episodes_total counter");
        for d in scenarios::Drill::ALL {
            let _ = writeln!(
                s,
                "trainer_drill_episodes_total{{drill=\"{}\"}} {}",
                d.label(),
                self.drill_episodes[d.index()]
            );
        }
        let _ = writeln!(s, "# TYPE trainer_drill_success_total counter");
        for d in scenarios::Drill::ALL {
            let _ = writeln!(
                s,
                "trainer_drill_success_total{{drill=\"{}\"}} {}",
                d.label(),
                self.drill_success[d.index()]
            );
        }
        let _ = writeln!(s, "# TYPE trainer_drill_dribble_goal_total counter");
        let _ = writeln!(
            s,
            "trainer_drill_dribble_goal_total {}",
            self.drill_dribble_goal
        );
        let _ = writeln!(s, "# TYPE trainer_drill_saves_total counter");
        let _ = writeln!(s, "trainer_drill_saves_total {}", self.drill_saves);
        let _ = writeln!(s, "# TYPE trainer_drill_bonus_total counter");
        // Seules les mécaniques qui peuvent recevoir un bonus ont une série (budget de séries, voir ci-dessus).
        for m in mechanics::Mechanic::ALL
            .into_iter()
            .filter(|m| rl_env::drill::base_bonus(*m) > 0.0)
        {
            let _ = writeln!(
                s,
                "trainer_drill_bonus_total{{mechanic=\"{}\"}} {}",
                m.label(),
                self.drill_bonus.get(m)
            );
        }
        let _ = writeln!(s, "# TYPE trainer_drill_recovery_total counter");
        for o in rl_env::drill::RecoveryOutcome::ALL {
            let _ = writeln!(
                s,
                "trainer_drill_recovery_total{{outcome=\"{}\"}} {}",
                o.label(),
                self.drill_recovery[o.index()]
            );
        }
        let _ = writeln!(s, "# TYPE trainer_boost_pickups_total counter");
        for (size, n) in [
            ("small", self.boost_pickups_small),
            ("big", self.boost_pickups_big),
        ] {
            let _ = writeln!(s, "trainer_boost_pickups_total{{size=\"{size}\"}} {n}");
        }
        let _ = writeln!(s, "# TYPE trainer_boost_mean gauge");
        let _ = writeln!(s, "trainer_boost_mean {}", self.boost_mean);
        let _ = writeln!(s, "# TYPE trainer_boost_empty_fraction gauge");
        let _ = writeln!(
            s,
            "trainer_boost_empty_fraction {}",
            self.boost_empty_fraction
        );
        let _ = writeln!(s, "# TYPE trainer_hit_speed_to_goal_mean gauge");
        let _ = writeln!(s, "trainer_hit_speed_to_goal_mean {}", self.hit_speed_mean);
        let _ = writeln!(s, "# TYPE trainer_hit_speed_to_goal_record gauge");
        let _ = writeln!(
            s,
            "trainer_hit_speed_to_goal_record {}",
            self.hit_speed_record
        );
        let _ = writeln!(s, "# TYPE trainer_clips_saved_total counter");
        for (kind, n) in [("known", self.clips_saved_known), ("novel", 0)] {
            let _ = writeln!(s, "trainer_clips_saved_total{{kind=\"{kind}\"}} {n}");
        }
        let _ = writeln!(s, "# TYPE trainer_clips_top_score gauge");
        let _ = writeln!(s, "trainer_clips_top_score {}", self.clips_top_score);
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
        let _ = writeln!(s, "# TYPE trainer_kickoffs_total counter");
        for (r, v) in [
            ("won", self.kickoffs_won),
            ("lost", self.kickoffs_lost),
            ("neutral", self.kickoffs_neutral),
        ] {
            let _ = writeln!(s, "trainer_kickoffs_total{{outcome=\"{r}\"}} {v}");
        }
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

/// Page web des clips (lecteur three.js), servie sur `/` et `/index.html`.
const INDEX_HTML: &str = include_str!("../web/index.html");

/// `GET /` ou `GET /index.html` (avec ou sans paramètres).
fn is_page(req: &str) -> bool {
    let path = req
        .strip_prefix("GET ")
        .and_then(|r| r.split([' ', '?', '\r', '\n']).next());
    matches!(path, Some("/" | "/index.html"))
}

/// Sert `/metrics` sur `addr` dans un thread d'arrière-plan. Retourne l'adresse réellement liée.
pub fn serve(addr: &str, m: Arc<Mutex<Metrics>>) -> std::io::Result<std::net::SocketAddr> {
    serve_with_dir(addr, m, None)
}

/// Comme [`serve`], et sert en lecture seule la dernière politique du dossier `dir` :
/// `GET /policy.safetensors` et `GET /policy.json` (palier, taille, date ; calculé à la volée), puis les clips
/// rejouables de `dir/clips` : `GET /clips` (liste JSON), `GET /clips/<id>` et `GET /clips/<id>/trajectory`
/// (voir `clips.rs`), et la page web des clips sur `/`.
pub fn serve_with_dir(
    addr: &str,
    m: Arc<Mutex<Metrics>>,
    dir: Option<PathBuf>,
) -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    std::thread::Builder::new().name("metrics".into()).spawn(move || {
        let started = Instant::now();
        for conn in listener.incoming().flatten() {
            let mut conn = conn;
            // Un client lent ne doit pas bloquer les scrapes (serveur à un seul thread).
            let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = conn.set_write_timeout(Some(Duration::from_secs(10)));
            // Lit jusqu'à la fin des en-têtes : fermer avec des octets non lus provoquerait un RST côté client.
            let mut buf = [0u8; 2048];
            let mut n = 0;
            while n < buf.len() && !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                match conn.read(&mut buf[n..]) {
                    Ok(0) | Err(_) => break,
                    Ok(k) => n += k,
                }
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let (status, ctype, body): (&str, &str, Vec<u8>) = if req.starts_with("GET /metrics") {
                let mut body = m.lock().map(|m| m.render()).unwrap_or_default();
                let _ = writeln!(body, "# TYPE process_uptime_seconds gauge\nprocess_uptime_seconds {}", started.elapsed().as_secs_f64());
                ("200 OK", "text/plain; version=0.0.4", body.into_bytes())
            } else if is_page(&req) {
                ("200 OK", "text/html; charset=utf-8", INDEX_HTML.as_bytes().to_vec())
            } else if req.starts_with("GET /healthz") {
                ("200 OK", "text/plain", b"ok\n".to_vec())
            } else if req.starts_with("GET /policy.safetensors") {
                match dir.as_deref().and_then(|d| std::fs::read(d.join("policy.safetensors")).ok()) {
                    Some(b) => ("200 OK", "application/octet-stream", b),
                    None => ("404 Not Found", "text/plain", Vec::new()),
                }
            } else if let Some(r) = crate::clips::route(&req, dir.as_deref()) {
                r
            } else if req.starts_with("GET /policy.json") {
                match dir.as_deref().and_then(policy_info) {
                    Some(j) => ("200 OK", "application/json", j.into_bytes()),
                    None => ("404 Not Found", "text/plain", Vec::new()),
                }
            } else {
                ("404 Not Found", "text/plain", Vec::new())
            };
            let _ = write!(
                conn,
                "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = conn.write_all(&body);
        }
    })?;
    Ok(local)
}

/// Métadonnées de la politique exportée : `{"stage":S,"bytes":N,"updated_unix":T}` (`None` sans politique).
/// `updated_unix` sert de version (la politique est réécrite à chaque checkpoint).
fn policy_info(dir: &Path) -> Option<String> {
    let meta = std::fs::metadata(dir.join("policy.safetensors")).ok()?;
    let updated = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let stage: u32 = std::fs::read_to_string(dir.join("stage"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Some(format!(
        "{{\"stage\":{stage},\"bytes\":{},\"updated_unix\":{updated}}}\n",
        meta.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scenarios::DRILL_COUNT;

    #[test]
    fn series_stay_far_below_the_cap() {
        let mut m = Metrics::default();
        for i in 0..1000 {
            m.observe_iteration(f64::from(i) * 0.1);
        }
        let n = count_series(&m.render());
        assert!(
            n < 250,
            "objectif < 250 séries (plafond dur : 500), obtenu {n}"
        );
        assert!(n < MAX_SERIES);
    }

    #[test]
    fn mechanic_records_are_exposed() {
        let mut m = Metrics::default();
        m.mechanics.1.observe(mechanics::Mechanic::Pinch, 4200.0);
        m.drill_mechanics
            .1
            .observe(mechanics::Mechanic::Pinch, 4800.0);
        let t = m.render();
        assert!(t.contains("trainer_mechanic_record{mechanic=\"pinch\"} 4800"));
        assert!(t.contains("trainer_mechanic_record{mechanic=\"power_shot\"} 0"));
        assert!(!t.contains("trainer_mechanic_record{mechanic=\"touch\"}"));
    }

    #[test]
    fn clip_counter_is_exposed() {
        let m = Metrics {
            clips_saved_known: 4,
            ..Default::default()
        };
        let t = m.render();
        assert!(t.contains("trainer_clips_saved_total{kind=\"known\"} 4"));
        assert!(t.contains("trainer_clips_saved_total{kind=\"novel\"} 0"));
    }

    #[test]
    fn kickoff_series_are_exposed() {
        let m = Metrics {
            kickoffs_won: 3,
            kickoffs_lost: 2,
            kickoff_first_touch_seconds: 2.5,
            ..Default::default()
        };
        let t = m.render();
        assert!(t.contains("trainer_kickoffs_total{outcome=\"won\"} 3"));
        assert!(t.contains("trainer_kickoffs_total{outcome=\"lost\"} 2"));
        assert!(t.contains("trainer_kickoff_first_touch_seconds 2.5"));
        assert!(t.contains("trainer_kickoff_touch_speed 0"));
    }

    #[test]
    fn selfplay_series_are_exposed() {
        let m = Metrics {
            elo: 1042.5,
            pool_size: 7,
            elo_vs: [1100.0, 1000.0, 1040.0],
            selfplay_goals: [[5, 4], [3, 2]],
            ..Default::default()
        };
        let t = m.render();
        assert!(t.contains("trainer_elo 1042.5"));
        assert!(t.contains("trainer_pool_size 7"));
        assert!(t.contains("trainer_elo_vs{ref=\"stage5\"} 1000"));
        assert!(t.contains("trainer_selfplay_goals_total{opponent=\"pool\",side=\"conceded\"} 2"));
        assert!(t.contains("trainer_stage_changes_total{direction=\"down\"} 0"));
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
                "mechanic=\"",
                "drill=\"",
                "outcome=\"good\"",
                "outcome=\"wheels\"",
                "outcome=\"bad\"",
                "outcome=\"none\"",
                "side=\"scored\"",
                "side=\"conceded\"",
                "outcome=\"won\"",
                "outcome=\"lost\"",
                "outcome=\"neutral\"",
                "kind=\"known\"",
                "kind=\"novel\"",
                "ref=\"charger\"",
                "ref=\"stage5\"",
                "ref=\"best\"",
                "opponent=\"current\"",
                "opponent=\"pool\"",
                "direction=\"up\"",
                "direction=\"down\"",
                "size=\"small\"",
                "size=\"big\"",
                "le=\"",
            ];
            assert!(ok.iter().any(|k| l.contains(k)), "{l}");
            if l.contains("mechanic=") {
                assert!(
                    mechanics::Mechanic::ALL
                        .iter()
                        .any(|m| l.contains(&format!("mechanic=\"{}\"", m.label()))),
                    "{l}"
                );
                if l.starts_with("trainer_mechanic_total") {
                    assert!(
                        l.contains("context=\"match\"") || l.contains("context=\"drill\""),
                        "{l}"
                    );
                }
            }
        }
    }

    #[test]
    fn mechanic_counters_are_exposed_per_context() {
        let mut m = Metrics::default();
        m.mechanics.0[mechanics::Mechanic::FlipReset.index()] = 3;
        let t = m.render();
        assert!(t.contains("trainer_mechanic_total{mechanic=\"flip_reset\",context=\"match\"} 3"));
        assert!(t.contains("trainer_mechanic_total{mechanic=\"flip_reset\",context=\"drill\"} 0"));
        assert_eq!(
            t.lines()
                .filter(|l| l.starts_with("trainer_mechanic_total{"))
                .count(),
            mechanics::MECH_COUNT * 2
        );
    }

    #[test]
    fn drill_series_are_exposed() {
        let mut m = Metrics::default();
        m.drill_episodes[scenarios::Drill::WallBall.index()] = 7;
        m.drill_bonus.0[mechanics::Mechanic::Flick.index()] = 2;
        m.drill_recovery[0] = 4;
        m.drill_success[scenarios::Drill::Keeper.index()] = 3;
        m.drill_mechanics.0[mechanics::Mechanic::Dribble.index()] = 5;
        let t = m.render();
        assert!(t.contains("trainer_drill_episodes_total{drill=\"wall_ball\"} 7"));
        assert!(t.contains("trainer_drill_bonus_total{mechanic=\"flick\"} 2"));
        assert!(t.contains("trainer_drill_recovery_total{outcome=\"good\"} 4"));
        assert!(t.contains("trainer_drill_success_total{drill=\"keeper\"} 3"));
        assert!(t.contains("trainer_mechanic_total{mechanic=\"dribble\",context=\"drill\"} 5"));
        assert_eq!(
            t.lines()
                .filter(|l| l.starts_with("trainer_drill_"))
                .count(),
            DRILL_COUNT * 2 + 9 + 4 + 2
        );
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

    #[test]
    fn serves_policy_read_only() {
        let dir = std::env::temp_dir().join(format!("lm-pol-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let get = |addr, path: &str| {
            let mut c = std::net::TcpStream::connect(addr).unwrap();
            write!(c, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut out = Vec::new();
            c.read_to_end(&mut out).unwrap();
            out
        };
        let m = Arc::new(Mutex::new(Metrics::default()));
        let addr = serve_with_dir("127.0.0.1:0", m, Some(dir.clone())).unwrap();
        assert!(get(addr, "/policy.safetensors").starts_with(b"HTTP/1.1 404"));
        std::fs::write(dir.join("policy.safetensors"), b"\x01\x02\x03").unwrap();
        std::fs::write(dir.join("stage"), "4\n").unwrap();
        let b = get(addr, "/policy.safetensors");
        assert!(b.starts_with(b"HTTP/1.1 200") && b.ends_with(b"\x01\x02\x03"));
        let j = String::from_utf8(get(addr, "/policy.json")).unwrap();
        assert!(
            j.contains("\"stage\":4") && j.contains("\"bytes\":3"),
            "{j}"
        );
        assert!(get(addr, "/etc/passwd").starts_with(b"HTTP/1.1 404"));
        for p in ["/", "/index.html", "/?x=1"] {
            let b = String::from_utf8(get(addr, p)).unwrap();
            assert!(
                b.starts_with("HTTP/1.1 200") && b.contains("text/html"),
                "{p}"
            );
            assert!(b.contains("three@0.160.0") && b.contains("clips/"), "{p}");
        }
        assert!(get(addr, "/metrics").starts_with(b"HTTP/1.1 200"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
