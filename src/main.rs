//! `sdt` CLI — toggle, sync, and diagnose shared agent skill state.

use clap::{Parser, Subcommand};
use sdt::{run_doctor, run_sync, run_tui, tui::TuiError};

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
    };
    if let Some(e) = err {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
