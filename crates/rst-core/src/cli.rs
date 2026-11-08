//! Ligne de commande `rst` (clap, aide en français). Sans sous-commande : jouer.

use clap::{Arg, ArgAction, Args, Command, CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "rst",
    about = "Rocket Sim Trainer : entraînement aux mécaniques de jeu de voiture-football (sans sous-commande : jouer)",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
    #[command(flatten)]
    pub play: PlayArgs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct PlayArgs {
    /// Capture l'écran dans ce fichier PNG puis quitte
    #[arg(long, value_name = "FICHIER.png")]
    pub screenshot: Option<String>,
    /// Nombre d'images avant la capture
    #[arg(long, value_name = "N", default_value_t = 240)]
    pub frames: u32,
    /// Conduite automatique de démonstration
    #[arg(long)]
    pub demo: bool,
    /// Démarre directement dans le drill « tir au but »
    #[arg(long)]
    pub drill: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Cmd {
    /// Lance le jeu (comportement par défaut)
    Play(PlayArgs),
    /// Met à jour rst depuis la dernière release GitHub (SHA256 vérifié)
    Update {
        /// Vérifie seulement s'il existe une version plus récente
        #[arg(long)]
        check: bool,
    },
    /// Se copie dans ~/.local/bin/rst et crée une entrée de menu
    Install {
        /// Pas d'entrée de menu (.desktop)
        #[arg(long)]
        no_desktop: bool,
    },
    /// Retire le binaire et l'entrée de menu
    Uninstall {
        /// Supprime aussi la config et les meshes
        #[arg(long)]
        purge: bool,
    },
    /// Affiche la version, le commit et la date de build
    Version,
    /// Diagnostic : meshes, manette, GPU, chemins, version à jour
    Doctor,
    /// Fichier de configuration (par défaut : show)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// (Re)télécharge et vérifie les meshes de collision
    Meshes {
        /// Retélécharge même s'ils sont déjà valides
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum ConfigAction {
    /// Affiche le chemin du fichier
    Path,
    /// Affiche le contenu
    Show,
    /// Ouvre le fichier dans $EDITOR
    Edit,
    /// Remet les valeurs par défaut (copie dans config.toml.bak)
    Reset,
}

/// Remplace les libellés anglais de clap (`-h`, sous-commande `help`, titres).
fn frenchify(c: Command) -> Command {
    let names: Vec<String> = c
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    let mut c = c
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .subcommand_help_heading("Commandes")
        .next_help_heading("Options")
        .help_template("{about}\n\nUsage : {usage}\n\n{all-args}")
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Affiche l'aide"),
        );
    for n in names {
        c = c.mut_subcommand(n, frenchify);
    }
    c
}

pub fn command() -> Command {
    frenchify(Cli::command())
}

pub fn try_parse_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let m = command().try_get_matches_from(args)?;
    Cli::from_arg_matches(&m)
}

/// Lit `std::env::args` ; quitte avec l'aide ou l'erreur (en français) si besoin.
pub fn parse() -> Cli {
    match try_parse_from(std::env::args_os()) {
        Ok(c) => c,
        Err(e) => e.exit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Cli {
        try_parse_from(std::iter::once("rst").chain(a.iter().copied())).unwrap()
    }

    #[test]
    fn no_args_means_play() {
        let c = p(&[]);
        assert!(c.cmd.is_none());
        assert_eq!(c.play.frames, 240);
    }

    #[test]
    fn legacy_flags_without_subcommand() {
        let c = p(&["--screenshot", "a.png", "--frames", "10", "--demo"]);
        assert!(c.cmd.is_none());
        assert_eq!(c.play.screenshot.as_deref(), Some("a.png"));
        assert_eq!((c.play.frames, c.play.demo), (10, true));
    }

    #[test]
    fn subcommands() {
        assert!(matches!(p(&["play", "--drill"]).cmd, Some(Cmd::Play(a)) if a.drill));
        assert_eq!(
            p(&["update", "--check"]).cmd,
            Some(Cmd::Update { check: true })
        );
        assert_eq!(p(&["update"]).cmd, Some(Cmd::Update { check: false }));
        assert_eq!(
            p(&["install"]).cmd,
            Some(Cmd::Install { no_desktop: false })
        );
        assert_eq!(
            p(&["uninstall", "--purge"]).cmd,
            Some(Cmd::Uninstall { purge: true })
        );
        assert_eq!(p(&["version"]).cmd, Some(Cmd::Version));
        assert_eq!(p(&["doctor"]).cmd, Some(Cmd::Doctor));
        assert_eq!(
            p(&["meshes", "--force"]).cmd,
            Some(Cmd::Meshes { force: true })
        );
        assert_eq!(p(&["config"]).cmd, Some(Cmd::Config { action: None }));
        assert_eq!(
            p(&["config", "reset"]).cmd,
            Some(Cmd::Config {
                action: Some(ConfigAction::Reset)
            })
        );
    }

    #[test]
    fn errors_and_french_help() {
        assert!(try_parse_from(["rst", "nimporte"]).is_err());
        assert!(try_parse_from(["rst", "config", "x"]).is_err());
        let e = try_parse_from(["rst", "-h"]).unwrap_err();
        let h = e.to_string();
        assert!(
            h.contains("Commandes") && h.contains("Affiche l'aide") && h.contains("update"),
            "{h}"
        );
    }
}
