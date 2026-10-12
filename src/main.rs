//! `sdt` CLI — toggle, sync, and diagnose shared agent skill state.

use clap::{Args, Parser, Subcommand};
use sdt::commands::{
    run_pack_add, run_pack_delete, run_pack_disable, run_pack_enable, run_pack_list,
    run_pack_remove,
};
use sdt::{
    run_disable, run_doctor, run_enable, run_list, run_sync, run_tui, tui::TuiError, ListFilter,
    Selection,
};

/// Skill Discovery Toggle: manage shared agent skills via SKILL.md frontmatter.
#[derive(Parser)]
#[command(name = "sdt", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Apply registry state to SKILL.md frontmatter on disk.
    Sync,
    /// Report drift, orphans, and missing SKILL.md (informational; exit 0).
    Doctor,
    /// List skills and whether each is enabled.
    List {
        /// Show only enabled skills.
        #[arg(long, conflicts_with = "disabled")]
        enabled: bool,
        /// Show only disabled skills.
        #[arg(long)]
        disabled: bool,
        /// Print a JSON array of `{"name", "enabled"}` objects.
        #[arg(long)]
        json: bool,
    },
    /// Enable skills for automatic invocation and sync SKILL.md.
    Enable(Targets),
    /// Disable skills for automatic invocation and sync SKILL.md.
    Disable(Targets),
    /// Manage and toggle named packs of skills.
    Pack {
        #[command(subcommand)]
        command: PackCommand,
    },
}

#[derive(Subcommand)]
enum PackCommand {
    /// List packs with their state (on/off/mixed) and members.
    List {
        /// Print a JSON array of `{"name", "state", "members", "missing"}` objects.
        #[arg(long)]
        json: bool,
    },
    /// Add skills to a pack, creating it if needed.
    Add {
        pack: String,
        #[arg(required = true)]
        skills: Vec<String>,
    },
    /// Remove skills from a pack.
    Remove {
        pack: String,
        #[arg(required = true)]
        skills: Vec<String>,
    },
    /// Delete a pack (its skills keep their state).
    Delete { pack: String },
    /// Enable every skill in the packs and sync SKILL.md.
    Enable {
        #[arg(required = true)]
        packs: Vec<String>,
        /// Also disable every skill outside these packs.
        #[arg(long)]
        only: bool,
    },
    /// Disable every skill in the packs and sync SKILL.md.
    Disable {
        #[arg(required = true)]
        packs: Vec<String>,
    },
}

/// Skill names or `--all`; exactly one is required.
#[derive(Args)]
#[group(required = true, multiple = false)]
struct Targets {
    /// Skill folder names.
    skills: Vec<String>,
    /// Select every skill.
    #[arg(long)]
    all: bool,
}

impl From<Targets> for Selection {
    fn from(targets: Targets) -> Self {
        if targets.all {
            Selection::All
        } else {
            Selection::Names(targets.skills)
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let err = match cli.command {
        None => match run_tui() {
            Ok(()) => None,
            Err(TuiError::Cancelled) => return,
            Err(e) => Some(e.to_string()),
        },
        Some(Command::Sync) => run_sync().err().map(|e| e.to_string()),
        Some(Command::Doctor) => run_doctor().err().map(|e| e.to_string()),
        Some(Command::List {
            enabled,
            disabled,
            json,
        }) => {
            let filter = match (enabled, disabled) {
                (true, _) => ListFilter::Enabled,
                (_, true) => ListFilter::Disabled,
                _ => ListFilter::All,
            };
            run_list(filter, json).err().map(|e| e.to_string())
        }
        Some(Command::Enable(targets)) => run_enable(&targets.into()).err().map(|e| e.to_string()),
        Some(Command::Disable(targets)) => {
            run_disable(&targets.into()).err().map(|e| e.to_string())
        }
        Some(Command::Pack { command }) => {
            let result = match command {
                PackCommand::List { json } => run_pack_list(json),
                PackCommand::Add { pack, skills } => run_pack_add(&pack, &skills),
                PackCommand::Remove { pack, skills } => run_pack_remove(&pack, &skills),
                PackCommand::Delete { pack } => run_pack_delete(&pack),
                PackCommand::Enable { packs, only } => run_pack_enable(&packs, only),
                PackCommand::Disable { packs } => run_pack_disable(&packs),
            };
            result.err().map(|e| e.to_string())
        }
    };
    if let Some(e) = err {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("sdt").chain(args.iter().copied()))
    }

    #[test]
    fn list_filter_flags_conflict() {
        assert!(parse(&["list", "--enabled", "--json"]).is_ok());
        assert!(parse(&["list", "--enabled", "--disabled"]).is_err());
    }

    #[test]
    fn enable_and_disable_take_names_or_all() {
        for cmd in ["enable", "disable"] {
            assert!(parse(&[cmd, "a", "b"]).is_ok());
            assert!(parse(&[cmd, "--all"]).is_ok());
            assert!(parse(&[cmd]).is_err());
            assert!(parse(&[cmd, "a", "--all"]).is_err());
        }
    }

    #[test]
    fn targets_convert_to_selection() {
        let Some(Command::Enable(targets)) = parse(&["enable", "a"]).unwrap().command else {
            panic!("expected enable");
        };
        assert_eq!(Selection::from(targets), Selection::Names(vec!["a".into()]));
        let Some(Command::Disable(targets)) = parse(&["disable", "--all"]).unwrap().command else {
            panic!("expected disable");
        };
        assert_eq!(Selection::from(targets), Selection::All);
    }

    #[test]
    fn pack_subcommands_parse() {
        assert!(parse(&["pack", "list", "--json"]).is_ok());
        assert!(parse(&["pack", "add", "p", "a", "b"]).is_ok());
        assert!(parse(&["pack", "add", "p"]).is_err());
        assert!(parse(&["pack", "remove", "p", "a"]).is_ok());
        assert!(parse(&["pack", "delete", "p"]).is_ok());
        assert!(parse(&["pack", "enable", "p", "q", "--only"]).is_ok());
        assert!(parse(&["pack", "enable"]).is_err());
        assert!(parse(&["pack", "disable", "p"]).is_ok());
        assert!(parse(&["pack", "disable", "p", "--only"]).is_err());
    }
}
