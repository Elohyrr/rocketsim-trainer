//! Logique de l'outil `rst` sans dépendance graphique : versions, SHA256, mise à jour, installation.
//! Isolée du client macroquad pour se tester sans GPU ni libudev.

pub mod cli;
pub mod commands;
pub mod hash;
pub mod install;
pub mod replace;
pub mod update;
pub mod version;

/// Dépôt GitHub des releases.
pub const REPO: &str = "Elohyrr/rocketsim-trainer";
