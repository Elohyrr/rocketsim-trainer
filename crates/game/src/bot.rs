//! Pilote automatique : politique du learner appliquée au monde de jeu.
//!
//! Les observations viennent de `rl_env::build_obs` (même code qu'à l'entraînement) et les actions de la même
//! `ActionTable` ; la décision est prise tous les `tick_skip` ticks et tenue entre deux (comme à l'entraînement).

use std::path::{Path, PathBuf};

use policy_runtime::Policy;
use rl_env::{ActionTable, EnvConfig, N_ACTIONS_V1, OBS_DIM, OBS_DIM_V2, build_obs};
use rst_core::bot::{META, PolicyMeta, WEIGHTS};
use sim_core::{Controls, Snapshot};

use crate::meshes;

/// Rôle du bot dans le monde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Le bot conduit la voiture bleue, seul (rst watch).
    Watch,
    /// Le bot conduit la voiture orange contre le joueur (rst play --vs-bot).
    VsBot,
}

pub struct Brain {
    policy: Policy,
    table: ActionTable,
    tick_skip: u64,
    held: Controls,
    next_decision: u64,
    pub meta: Option<PolicyMeta>,
}

/// Dossier de la politique installée par `rst bot pull`.
pub fn policy_dir() -> PathBuf {
    meshes::data_dir().join("bot")
}

impl Brain {
    pub fn load(weights: &Path) -> Result<Self, String> {
        let policy = Policy::load(weights).map_err(|e| format!("{} : {e}", weights.display()))?;
        let table = ActionTable::rlgym();
        // Politique d'avant la v3 acceptée : 43 entrées (l'observation est tronquée) et 78 actions (les 12
        // actions de frein à main, en fin de table, ne sont jamais choisies).
        let obs_ok = [OBS_DIM_V2, OBS_DIM].contains(&policy.obs_dim());
        let acts_ok = [N_ACTIONS_V1, table.len()].contains(&policy.action_count());
        if !obs_ok || !acts_ok {
            return Err(format!(
                "politique incompatible : {} entrées / {} actions, le client attend {OBS_DIM} / {}",
                policy.obs_dim(),
                policy.action_count(),
                table.len()
            ));
        }
        // Rythme de décision de la politique (`policy.json` voisin ; 15 Hz s'il manque, comme les anciennes).
        let meta = weights.parent().and_then(PolicyMeta::load);
        if let Some(m) = &meta {
            m.check_rate()?;
        }
        let tick_skip = meta
            .as_ref()
            .map_or_else(|| EnvConfig::default().tick_skip, |m| m.tick_skip);
        Ok(Self {
            policy,
            table,
            tick_skip: u64::from(tick_skip),
            held: Controls::default(),
            next_decision: 0,
            meta,
        })
    }

    /// Politique de `--policy`, sinon celle de `rst bot pull`.
    pub fn load_default(explicit: Option<&str>) -> Result<Self, String> {
        match explicit {
            Some(p) => Self::load(Path::new(p)),
            None => {
                let f = policy_dir().join(WEIGHTS);
                if !f.exists() {
                    return Err(format!(
                        "pas de politique installée ({}) : lancez « rst bot pull »",
                        policy_dir().join(META).display()
                    ));
                }
                Self::load(&f)
            }
        }
    }

    /// Ticks entre deux décisions (8 = 15 Hz, 4 = 30 Hz).
    pub fn tick_skip(&self) -> u32 {
        self.tick_skip as u32
    }

    /// Libellé « palier N · politique du … » (ou « politique locale » sans métadonnées).
    pub fn label(&self) -> String {
        self.meta
            .as_ref()
            .map_or_else(|| "politique locale".to_string(), PolicyMeta::label)
    }

    /// Contrôles à appliquer au tick de `snap` pour la voiture `me` (`other` : l'adversaire éventuel).
    /// Orange (`mirror`) : monde retourné de 180° autour de Z, comme le self-play de l'entraînement.
    pub fn controls(
        &mut self,
        snap: &Snapshot,
        me: usize,
        other: Option<usize>,
        mirror: bool,
    ) -> Controls {
        if snap.tick >= self.next_decision {
            let obs = build_obs(snap, me, other, mirror);
            self.held = self
                .table
                .get(self.policy.act(&obs[..self.policy.obs_dim()]));
            self.next_decision = snap.tick + self.tick_skip;
        }
        self.held
    }

    /// Remet à zéro la cadence (après une remise en jeu : le tick de référence peut avoir changé).
    pub fn restart(&mut self) {
        self.next_decision = 0;
    }
}
