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
    /// 1v1 contre le bot (politique téléchargée avec `rst bot pull`)
    #[arg(long)]
    pub vs_bot: bool,
    /// Politique à utiliser avec --vs-bot (défaut : celle de `rst bot pull`)
    #[arg(long, value_name = "FICHIER.safetensors", requires = "vs_bot")]
    pub policy: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Args)]
pub struct WatchArgs {
    /// Politique à utiliser (défaut : celle de `rst bot pull`)
    #[arg(long, value_name = "FICHIER.safetensors")]
    pub policy: Option<String>,
    /// Capture l'écran dans ce fichier PNG puis quitte
    #[arg(long, value_name = "FICHIER.png")]
    pub screenshot: Option<String>,
    /// Nombre d'images avant la capture
    #[arg(long, value_name = "N", default_value_t = 240)]
    pub frames: u32,
    /// Enchaîne des coups d'envoi : le bot (bleu) contre un adversaire scripté qui fonce sur la balle ;
    /// remise en jeu 2 s après le premier toucher (R pour passer au suivant)
    #[arg(long, conflicts_with_all = ["clip", "clips"])]
    pub kickoff: bool,
    /// Rejoue un clip enregistré par le learner : identifiant, URL ou fichier local (voir --clips)
    #[arg(long, value_name = "ID|URL|FICHIER", conflicts_with_all = ["policy", "clips"])]
    pub clip: Option<String>,
    /// Liste les clips disponibles sur le learner puis quitte
    #[arg(long, conflicts_with = "policy")]
    pub clips: bool,
    /// URL du learner pour --clip / --clips (défaut : $RST_BOT_URL, sinon celle du learner)
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum BotAction {
    /// Télécharge la dernière politique du bot (ou en installe une locale)
    Pull {
        /// URL du serveur de politique (défaut : $RST_BOT_URL, sinon celle du learner)
        #[arg(long, value_name = "URL", conflicts_with = "file")]
        url: Option<String>,
        /// Installe ce fichier local .safetensors au lieu de télécharger
        #[arg(long, value_name = "FICHIER")]
        file: Option<String>,
    },
    /// Affiche la politique installée (palier, date)
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Cmd {
    /// Lance le jeu (comportement par défaut)
    Play(PlayArgs),
    /// Regarde le bot jouer seul (caméra balle / voiture / spectateur)
    Watch(WatchArgs),
    /// Gère la politique du bot (téléchargement)
    Bot {
        #[command(subcommand)]
        action: BotAction,
    },
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
    fn bot_and_watch_commands() {
        assert!(matches!(p(&["watch"]).cmd, Some(Cmd::Watch(a)) if a.policy.is_none()));
        assert!(
            matches!(p(&["watch", "--clip", "x"]).cmd, Some(Cmd::Watch(a)) if a.clip.as_deref() == Some("x"))
        );
        assert!(
            matches!(p(&["watch", "--clips", "--url", "h"]).cmd, Some(Cmd::Watch(a)) if a.clips && a.url.is_some())
        );
        assert!(try_parse_from(["rst", "watch", "--clip", "x", "--clips"]).is_err());
        assert!(try_parse_from(["rst", "watch", "--clip", "x", "--kickoff"]).is_err());
        assert!(matches!(p(&["play", "--vs-bot"]).cmd, Some(Cmd::Play(a)) if a.vs_bot));
        assert!(p(&["--vs-bot"]).play.vs_bot);
        assert!(try_parse_from(["rst", "play", "--policy", "x"]).is_err());
        assert_eq!(
            p(&["bot", "pull", "--url", "http://h:1"]).cmd,
            Some(Cmd::Bot {
                action: BotAction::Pull {
                    url: Some("http://h:1".into()),
                    file: None
                }
            })
        );
        assert!(try_parse_from(["rst", "bot", "pull", "--url", "u", "--file", "f"]).is_err());
        assert_eq!(
            p(&["bot", "info"]).cmd,
            Some(Cmd::Bot {
                action: BotAction::Info
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
