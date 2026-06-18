//! Interactive terminal UI for toggling skills.

use std::io;

use dialoguer::{theme::ColorfulTheme, Confirm, Input, MultiSelect};
use thiserror::Error;

use crate::discovery::{has_skill_md, list_skill_dirs, skill_name};
use crate::registry::{ensure_loaded, is_enabled, write, SkillEntry};

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
    #[error("dialoguer error: {0}")]
    Dialoguer(#[from] dialoguer::Error),
    #[error("operation cancelled")]
    Cancelled,
}

/// Runs the interactive multi-select TUI; saves registry on confirm, discards on cancel.
pub fn run_tui() -> Result<(), TuiError> {
    let mut registry = ensure_loaded()?;
    let dirs = list_skill_dirs()?;

    let mut names: Vec<String> = dirs
        .iter()
        .filter(|d| has_skill_md(d))
        .filter_map(|d| skill_name(d))
        .collect();
    names.sort();

    if names.is_empty() {
        println!("No skills with SKILL.md found.");
        return Ok(());
    }

    let filter: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Filter skills (substring, case-insensitive)")
        .allow_empty(true)
        .interact_text()?;

    let filter_lower = filter.to_lowercase();
    let filtered: Vec<&String> = if filter_lower.is_empty() {
        names.iter().collect()
    } else {
        names
            .iter()
            .filter(|n| n.to_lowercase().contains(&filter_lower))
            .collect()
    };

    if filtered.is_empty() {
        println!("No skills match the filter.");
        return Ok(());
    }

    let defaults: Vec<bool> = filtered.iter().map(|n| is_enabled(&registry, n)).collect();

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Toggle skills (space to select, enter to confirm)")
        .items(&filtered.iter().map(|s| s.as_str()).collect::<Vec<_>>())
        .defaults(&defaults)
        .interact()?;

    let confirmed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Save changes to registry?")
        .default(true)
        .interact()?;

    if !confirmed {
        return Err(TuiError::Cancelled);
    }

    for (idx, name) in filtered.iter().enumerate() {
        let enabled = selections.contains(&idx);
        registry
            .skills
            .entry((*name).clone())
            .or_insert(SkillEntry { enabled: true })
            .enabled = enabled;
    }

    write(&registry)?;
    println!("Registry saved. Run `cst sync` to apply changes.");
    Ok(())
}

/// Non-interactive helper for testing filter logic.
#[cfg(test)]
pub fn filter_skill_names(names: &[String], filter: &str) -> Vec<String> {
    let filter_lower = filter.to_lowercase();
    if filter_lower.is_empty() {
        return names.to_vec();
    }
    names
        .iter()
        .filter(|n| n.to_lowercase().contains(&filter_lower))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_is_case_insensitive_substring() {
        let names = vec!["Brainstorming".into(), "triage".into(), "my-brain".into()];
        let out = filter_skill_names(&names, "brain");
        assert_eq!(out, vec!["Brainstorming", "my-brain"]);
    }

    #[test]
    fn empty_filter_returns_all() {
        let names = vec!["a".into(), "b".into()];
        assert_eq!(filter_skill_names(&names, ""), names);
    }
}
