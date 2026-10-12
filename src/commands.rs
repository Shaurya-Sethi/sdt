//! Non-interactive commands: list skills and enable or disable them by name or
//! by pack, and manage pack membership.
//!
//! Commands that change skill state write the registry and then sync
//! `SKILL.md` files, so one command is enough for scripts and agents.

use std::collections::BTreeSet;
use std::io;

use serde_json::json;
use thiserror::Error;

use crate::discovery::list_skill_names;
use crate::pack::{is_valid_name, split_members, PackState};
use crate::registry::{ensure_loaded, is_enabled, write, Registry, SkillEntry};
use crate::sync::{run_sync, SyncError};

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
    #[error("{0}")]
    Sync(#[from] SyncError),
    /// Requested names that are not skill folders with a `SKILL.md`.
    #[error("unknown skill(s): {}", .0.join(", "))]
    UnknownSkills(Vec<String>),
    /// Requested packs that do not exist in the registry.
    #[error("unknown pack(s): {}", .0.join(", "))]
    UnknownPacks(Vec<String>),
    /// A pack name that is empty, has whitespace, or starts with `-`.
    #[error(
        "invalid pack name '{0}': must be non-empty, without whitespace, and not start with '-'"
    )]
    InvalidPackName(String),
}

/// Which skills `sdt list` shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Enabled,
    Disabled,
}

/// Prints every skill with a `SKILL.md` and whether the registry enables it.
///
/// Plain output is one `[x] name` (enabled) or `[ ] name` (disabled) line per
/// skill; `json` prints `[{"name": ..., "enabled": ...}]`. Initializes or merges
/// the registry like other commands.
pub fn run_list(filter: ListFilter, json: bool) -> Result<(), CommandError> {
    print!("{}", list_output(filter, json)?);
    Ok(())
}

/// Skills targeted by `sdt enable` / `sdt disable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Every skill with a `SKILL.md`.
    All,
    /// Specific skill folder names.
    Names(Vec<String>),
}

/// Enables the selected skills in the registry, then syncs `SKILL.md` files.
///
/// Errors with [`CommandError::UnknownSkills`] before writing anything when a
/// name is not a skill folder with a `SKILL.md`.
pub fn run_enable(selection: &Selection) -> Result<(), CommandError> {
    let count = set_enabled(selection, true)?;
    println!("Enabled {count} skill(s) and synced.");
    Ok(())
}

/// Disables the selected skills in the registry, then syncs `SKILL.md` files.
///
/// Errors like [`run_enable`] on unknown names.
pub fn run_disable(selection: &Selection) -> Result<(), CommandError> {
    let count = set_enabled(selection, false)?;
    println!("Disabled {count} skill(s) and synced.");
    Ok(())
}

/// Validates the selection, records `enabled` for each selected skill, writes
/// the registry, and syncs. Returns how many skills were selected.
fn set_enabled(selection: &Selection, enabled: bool) -> Result<usize, CommandError> {
    let mut registry = ensure_loaded()?;
    let known = list_skill_names()?;
    let targets = match selection {
        Selection::All => known,
        Selection::Names(names) => {
            check_known_skills(names, &known)?;
            names.clone()
        }
    };

    for name in &targets {
        set_skill(&mut registry, name, enabled);
    }
    write(&registry)?;
    run_sync()?;
    Ok(targets.len())
}

/// Prints each pack with its derived state, members, and missing members.
///
/// Plain output is one `name [state] member, ... (missing: ...)` line per pack;
/// `json` prints `[{"name", "state", "members", "missing"}]`.
pub fn run_pack_list(json: bool) -> Result<(), CommandError> {
    print!("{}", pack_list_output(json)?);
    Ok(())
}

/// Adds installed skills to `pack`, creating it if absent. Membership changes
/// do not change any skill's state, so nothing is synced.
///
/// Errors before writing on an invalid pack name or unknown skills.
pub fn run_pack_add(pack: &str, skills: &[String]) -> Result<(), CommandError> {
    if !is_valid_name(pack) {
        return Err(CommandError::InvalidPackName(pack.to_owned()));
    }
    let mut registry = ensure_loaded()?;
    check_known_skills(skills, &list_skill_names()?)?;
    let members = registry.packs.entry(pack.to_owned()).or_default();
    members.extend(skills.iter().cloned());
    let count = members.len();
    write(&registry)?;
    println!("Pack '{pack}' has {count} skill(s).");
    Ok(())
}

/// Removes skills from an existing pack. Names that are not members are
/// ignored, so missing (uninstalled) members can be removed too.
pub fn run_pack_remove(pack: &str, skills: &[String]) -> Result<(), CommandError> {
    let mut registry = ensure_loaded()?;
    check_known_packs(&[pack.to_owned()], &registry)?;
    let members = registry.packs.get_mut(pack).expect("checked above");
    for skill in skills {
        members.remove(skill);
    }
    let count = members.len();
    write(&registry)?;
    println!("Pack '{pack}' has {count} skill(s).");
    Ok(())
}

/// Deletes an existing pack; its skills keep their current state.
pub fn run_pack_delete(pack: &str) -> Result<(), CommandError> {
    let mut registry = ensure_loaded()?;
    check_known_packs(&[pack.to_owned()], &registry)?;
    registry.packs.remove(pack);
    write(&registry)?;
    println!("Deleted pack '{pack}'.");
    Ok(())
}

/// Enables every installed member of `packs`, then syncs. With `only`, every
/// skill outside those packs is disabled too.
///
/// Errors before writing on unknown packs; warns about and skips members that
/// are not installed.
pub fn run_pack_enable(packs: &[String], only: bool) -> Result<(), CommandError> {
    let count = apply_packs(packs, true, only)?;
    println!("Enabled {count} skill(s) and synced.");
    Ok(())
}

/// Disables every installed member of `packs`, then syncs. Errors and warns
/// like [`run_pack_enable`].
pub fn run_pack_disable(packs: &[String]) -> Result<(), CommandError> {
    let count = apply_packs(packs, false, false)?;
    println!("Disabled {count} skill(s) and synced.");
    Ok(())
}

/// Runs [`set_packs`] and prints a warning per missing member; returns how
/// many installed skills the packs cover.
fn apply_packs(packs: &[String], enable: bool, only: bool) -> Result<usize, CommandError> {
    let (count, missing) = set_packs(packs, enable, only)?;
    for (pack, member) in &missing {
        eprintln!("warning: pack '{pack}' member '{member}' is not installed; skipped");
    }
    Ok(count)
}

/// `(pack, member)` pairs naming members that are not installed.
type Missing = Vec<(String, String)>;

/// Sets every installed member of `packs` to `enable` (and, with `only`, every
/// other skill to disabled), writes the registry, and syncs. Returns how many
/// installed skills the packs cover and the members skipped as not installed.
fn set_packs(packs: &[String], enable: bool, only: bool) -> Result<(usize, Missing), CommandError> {
    let mut registry = ensure_loaded()?;
    check_known_packs(packs, &registry)?;
    let installed = list_skill_names()?;

    let mut members = BTreeSet::new();
    let mut missing = Vec::new();
    for pack in packs {
        let (present, absent) = split_members(&registry.packs[pack], &installed);
        members.extend(present.into_iter().map(str::to_owned));
        missing.extend(absent.into_iter().map(|m| (pack.clone(), m.to_owned())));
    }

    for name in &installed {
        if members.contains(name) {
            set_skill(&mut registry, name, enable);
        } else if only {
            set_skill(&mut registry, name, false);
        }
    }
    write(&registry)?;
    run_sync()?;
    Ok((members.len(), missing))
}

fn set_skill(registry: &mut Registry, name: &str, enabled: bool) {
    registry
        .skills
        .entry(name.to_owned())
        .or_insert(SkillEntry { enabled })
        .enabled = enabled;
}

fn check_known_skills(names: &[String], known: &[String]) -> Result<(), CommandError> {
    let unknown: Vec<String> = names
        .iter()
        .filter(|n| !known.contains(n))
        .cloned()
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(CommandError::UnknownSkills(unknown))
    }
}

fn check_known_packs(packs: &[String], registry: &Registry) -> Result<(), CommandError> {
    let unknown: Vec<String> = packs
        .iter()
        .filter(|p| !registry.packs.contains_key(*p))
        .cloned()
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(CommandError::UnknownPacks(unknown))
    }
}

fn pack_list_output(json: bool) -> Result<String, CommandError> {
    let registry = ensure_loaded()?;
    let installed = list_skill_names()?;
    let rows: Vec<(&String, PackState, Vec<&String>, Vec<&str>)> = registry
        .packs
        .iter()
        .map(|(name, members)| {
            let (present, missing) = split_members(members, &installed);
            let state = PackState::of(present.iter().map(|m| is_enabled(&registry, m)));
            (name, state, members.iter().collect(), missing)
        })
        .collect();

    if json {
        let items: Vec<_> = rows
            .iter()
            .map(|(name, state, members, missing)| {
                json!({ "name": name, "state": state, "members": members, "missing": missing })
            })
            .collect();
        return Ok(format!("{:#}\n", serde_json::Value::Array(items)));
    }
    Ok(rows
        .iter()
        .map(|(name, state, members, missing)| {
            let mut line = format!("{name} [{state}]");
            if !members.is_empty() {
                let members: Vec<&str> = members.iter().map(|m| m.as_str()).collect();
                line += &format!(" {}", members.join(", "));
            }
            if !missing.is_empty() {
                line += &format!(" (missing: {})", missing.join(", "));
            }
            line + "\n"
        })
        .collect())
}

fn list_output(filter: ListFilter, json: bool) -> Result<String, CommandError> {
    let registry = ensure_loaded()?;
    let skills: Vec<(String, bool)> = list_skill_names()?
        .into_iter()
        .map(|name| {
            let enabled = is_enabled(&registry, &name);
            (name, enabled)
        })
        .filter(|&(_, enabled)| match filter {
            ListFilter::All => true,
            ListFilter::Enabled => enabled,
            ListFilter::Disabled => !enabled,
        })
        .collect();

    if json {
        let items: Vec<_> = skills
            .iter()
            .map(|(name, enabled)| json!({ "name": name, "enabled": enabled }))
            .collect();
        return Ok(format!("{:#}\n", serde_json::Value::Array(items)));
    }
    Ok(skills
        .iter()
        .map(|(name, enabled)| format!("[{}] {name}\n", if *enabled { 'x' } else { ' ' }))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{registry_path, set_home_for_tests};
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf, std::sync::MutexGuard<'static, ()>) {
        let tmp = TempDir::new().unwrap();
        let home = set_home_for_tests(tmp.path());
        let skills = crate::paths::canonical_skills_dir().unwrap();
        fs::create_dir_all(&skills).unwrap();
        fs::create_dir_all(registry_path().unwrap().parent().unwrap()).unwrap();
        (tmp, skills, home)
    }

    fn add_skill(skills: &Path, name: &str, content: &str) {
        fs::create_dir(skills.join(name)).unwrap();
        fs::write(skills.join(name).join("SKILL.md"), content).unwrap();
    }

    /// Creates enabled `on` and disabled `off`.
    fn two_skills(skills: &Path) {
        add_skill(skills, "on", "# on\n");
        add_skill(
            skills,
            "off",
            "---\nname: off\ndisable-model-invocation: true\n---\n# off\n",
        );
    }

    #[test]
    fn list_prints_checkbox_per_skill() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);
        assert_eq!(
            list_output(ListFilter::All, false).unwrap(),
            "[ ] off\n[x] on\n"
        );
    }

    #[test]
    fn list_filters_by_state() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);
        assert_eq!(list_output(ListFilter::Enabled, false).unwrap(), "[x] on\n");
        assert_eq!(
            list_output(ListFilter::Disabled, false).unwrap(),
            "[ ] off\n"
        );
    }

    #[test]
    fn list_json() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);
        let out: serde_json::Value =
            serde_json::from_str(&list_output(ListFilter::All, true).unwrap()).unwrap();
        assert_eq!(
            out,
            serde_json::json!([
                {"name": "off", "enabled": false},
                {"name": "on", "enabled": true},
            ])
        );
        let out: serde_json::Value =
            serde_json::from_str(&list_output(ListFilter::Disabled, true).unwrap()).unwrap();
        assert_eq!(out, serde_json::json!([{"name": "off", "enabled": false}]));
    }

    fn registry_enabled(name: &str) -> Option<bool> {
        crate::registry::read_from_disk()
            .unwrap()
            .skills
            .get(name)
            .map(|e| e.enabled)
    }

    fn read_skill(skills: &Path, name: &str) -> String {
        fs::read_to_string(skills.join(name).join("SKILL.md")).unwrap()
    }

    fn names(names: &[&str]) -> Selection {
        Selection::Names(names.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn disable_and_enable_named_skill_update_registry_and_skill_md() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);

        run_disable(&names(&["on"])).unwrap();
        assert_eq!(registry_enabled("on"), Some(false));
        assert_eq!(
            read_skill(&skills, "on"),
            "---\nname: on\ndisable-model-invocation: true\n---\n# on\n"
        );

        run_enable(&names(&["on", "off"])).unwrap();
        assert_eq!(registry_enabled("on"), Some(true));
        assert_eq!(registry_enabled("off"), Some(true));
        assert_eq!(read_skill(&skills, "on"), "# on\n");
        assert_eq!(read_skill(&skills, "off"), "# off\n");
    }

    #[test]
    fn all_selects_every_skill() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);

        run_disable(&Selection::All).unwrap();
        assert_eq!(registry_enabled("on"), Some(false));
        assert_eq!(registry_enabled("off"), Some(false));
        assert!(read_skill(&skills, "on").contains("disable-model-invocation: true"));

        run_enable(&Selection::All).unwrap();
        assert_eq!(registry_enabled("on"), Some(true));
        assert_eq!(registry_enabled("off"), Some(true));
        assert_eq!(read_skill(&skills, "off"), "# off\n");
    }

    #[test]
    fn unknown_names_error_without_writing() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);
        fs::create_dir(skills.join("empty")).unwrap();
        ensure_loaded().unwrap();
        let registry_before = fs::read_to_string(registry_path().unwrap()).unwrap();

        let err = run_disable(&names(&["on", "nope", "empty"])).unwrap_err();
        assert!(matches!(&err, CommandError::UnknownSkills(n) if n == &["nope", "empty"]));
        assert_eq!(err.to_string(), "unknown skill(s): nope, empty");
        assert_eq!(
            fs::read_to_string(registry_path().unwrap()).unwrap(),
            registry_before
        );
        assert_eq!(read_skill(&skills, "on"), "# on\n");
    }

    fn strings(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn pack_members(pack: &str) -> Option<Vec<String>> {
        crate::registry::read_from_disk()
            .unwrap()
            .packs
            .get(pack)
            .map(|m| m.iter().cloned().collect())
    }

    #[test]
    fn pack_add_creates_and_extends_pack() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);

        run_pack_add("p", &strings(&["on"])).unwrap();
        assert_eq!(pack_members("p"), Some(strings(&["on"])));
        run_pack_add("p", &strings(&["off", "on"])).unwrap();
        assert_eq!(pack_members("p"), Some(strings(&["off", "on"])));
    }

    #[test]
    fn pack_add_rejects_bad_name_and_unknown_skills() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);

        let err = run_pack_add("my pack", &strings(&["on"])).unwrap_err();
        assert!(matches!(err, CommandError::InvalidPackName(_)));
        let err = run_pack_add("p", &strings(&["on", "nope"])).unwrap_err();
        assert!(matches!(&err, CommandError::UnknownSkills(n) if n == &["nope"]));
        assert_eq!(pack_members("p"), None);
    }

    #[test]
    fn pack_remove_and_delete() {
        let (_tmp, skills, _home) = setup();
        two_skills(&skills);
        run_pack_add("p", &strings(&["on", "off"])).unwrap();

        run_pack_remove("p", &strings(&["on", "not-a-member"])).unwrap();
        assert_eq!(pack_members("p"), Some(strings(&["off"])));

        run_pack_delete("p").unwrap();
        assert_eq!(pack_members("p"), None);

        let err = run_pack_delete("p").unwrap_err();
        assert!(matches!(&err, CommandError::UnknownPacks(n) if n == &["p"]));
        assert!(run_pack_remove("p", &strings(&["on"])).is_err());
    }

    /// Writes a registry with pack `p` = {on, gone} and pack `q` = {off}.
    fn packs_fixture(skills: &Path) {
        two_skills(skills);
        add_skill(skills, "other", "# other\n");
        let mut reg = ensure_loaded().unwrap();
        reg.packs
            .insert("p".into(), ["on", "gone"].map(String::from).into());
        reg.packs.insert("q".into(), ["off".to_string()].into());
        write(&reg).unwrap();
    }

    #[test]
    fn pack_enable_and_disable_flip_members_and_sync() {
        let (_tmp, skills, _home) = setup();
        packs_fixture(&skills);

        let (count, missing) = set_packs(&strings(&["p"]), false, false).unwrap();
        assert_eq!(count, 1);
        assert_eq!(missing, vec![("p".to_string(), "gone".to_string())]);
        assert_eq!(registry_enabled("on"), Some(false));
        assert_eq!(registry_enabled("other"), Some(true));
        assert!(read_skill(&skills, "on").contains("disable-model-invocation: true"));

        set_packs(&strings(&["p", "q"]), true, false).unwrap();
        assert_eq!(registry_enabled("on"), Some(true));
        assert_eq!(registry_enabled("off"), Some(true));
        assert_eq!(read_skill(&skills, "off"), "# off\n");
    }

    #[test]
    fn pack_enable_only_disables_everything_else() {
        let (_tmp, skills, _home) = setup();
        packs_fixture(&skills);

        set_packs(&strings(&["q"]), true, true).unwrap();
        assert_eq!(registry_enabled("off"), Some(true));
        assert_eq!(registry_enabled("on"), Some(false));
        assert_eq!(registry_enabled("other"), Some(false));
    }

    #[test]
    fn unknown_pack_errors_without_writing() {
        let (_tmp, skills, _home) = setup();
        packs_fixture(&skills);
        let before = fs::read_to_string(registry_path().unwrap()).unwrap();

        let err = set_packs(&strings(&["p", "nope"]), false, false).unwrap_err();
        assert!(matches!(&err, CommandError::UnknownPacks(n) if n == &["nope"]));
        assert_eq!(
            fs::read_to_string(registry_path().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn pack_list_shows_state_members_and_missing() {
        let (_tmp, skills, _home) = setup();
        packs_fixture(&skills);

        assert_eq!(
            pack_list_output(false).unwrap(),
            "p [on] gone, on (missing: gone)\nq [off] off\n"
        );
        let out: serde_json::Value =
            serde_json::from_str(&pack_list_output(true).unwrap()).unwrap();
        assert_eq!(
            out,
            serde_json::json!([
                {"name": "p", "state": "on", "members": ["gone", "on"], "missing": ["gone"]},
                {"name": "q", "state": "off", "members": ["off"], "missing": []},
            ])
        );
    }
}
