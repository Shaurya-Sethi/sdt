//! Skills registry — source of truth for enable/disable intent.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::discovery::{self, has_skill_md, skill_name};
use crate::frontmatter::{is_disabled, parse_skill_md};
use crate::paths;

/// Current registry schema version.
pub const REGISTRY_VERSION: u32 = 1;

/// A single skill entry in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Whether the skill should be enabled (no disable flag in frontmatter).
    pub enabled: bool,
}

/// On-disk registry at `~/.agents/skills-registry.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    #[serde(rename = "canonicalDir")]
    pub canonical_dir: String,
    pub skills: BTreeMap<String, SkillEntry>,
    /// Named skill packs mapped to their member skill names. Members may name
    /// skills that are no longer installed; `sync` never prunes them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packs: BTreeMap<String, BTreeSet<String>>,
}

impl Registry {
    /// Creates an empty registry pointing at the canonical skills directory.
    pub fn empty() -> Result<Self, RegistryError> {
        let dir = paths::canonical_skills_dir()?;
        Ok(Self {
            version: REGISTRY_VERSION,
            canonical_dir: dir.to_string_lossy().into_owned(),
            skills: BTreeMap::new(),
            packs: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("failed to parse registry: {0}")]
    Parse(#[from] serde_json::Error),
}

/// Reads the registry from disk without merging discovered folders.
///
/// Returns an empty registry when the file does not exist.
pub fn read_from_disk() -> Result<Registry, RegistryError> {
    let path = paths::registry_path()?;
    if !path.is_file() {
        return Registry::empty();
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Loads the registry from disk, or lazily initializes it if missing.
///
/// On first run (no registry file), scans skills, imports frontmatter flags, and
/// writes the registry. Merges newly discovered skill folders on every call.
pub fn ensure_loaded() -> Result<Registry, RegistryError> {
    let path = paths::registry_path()?;
    let was_missing = !path.is_file();
    let mut registry = read_from_disk()?;
    merge_discovered_skills(&mut registry)?;
    if was_missing {
        write(&registry)?;
    }
    Ok(registry)
}

/// Writes the registry atomically to disk.
pub fn write(registry: &Registry) -> Result<(), RegistryError> {
    let path = paths::registry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(registry)?;

    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    if let Err(e) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(io::Error::new(
            e.kind(),
            format!(
                "failed to rename {} to {}: {e}",
                tmp.display(),
                path.display()
            ),
        )
        .into());
    }
    Ok(())
}

/// Returns whether a skill is enabled according to the registry (defaults to true).
pub fn is_enabled(registry: &Registry, name: &str) -> bool {
    registry.skills.get(name).map(|e| e.enabled).unwrap_or(true)
}

fn merge_discovered_skills(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.canonical_dir = paths::canonical_skills_dir()?
        .to_string_lossy()
        .into_owned();

    for dir in discovery::list_skill_dirs()? {
        let Some(name) = skill_name(&dir) else {
            continue;
        };
        if !has_skill_md(&dir) || registry.skills.contains_key(&name) {
            continue;
        }
        let enabled = import_enabled_from_frontmatter(&dir);
        registry.skills.insert(name, SkillEntry { enabled });
    }

    Ok(())
}

/// Reads `SKILL.md` frontmatter to determine enabled state for a new registry entry.
pub fn import_enabled_from_frontmatter(skill_dir: &Path) -> bool {
    if !has_skill_md(skill_dir) {
        return true;
    }
    let path = discovery::skill_md_path(skill_dir);
    let Ok(content) = fs::read_to_string(&path) else {
        return false;
    };
    let Ok(parsed) = parse_skill_md(&content) else {
        return false;
    };
    !is_disabled(&parsed.frontmatter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::set_home_for_tests;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (
        TempDir,
        std::path::PathBuf,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let tmp = TempDir::new().unwrap();
        let home = set_home_for_tests(tmp.path());
        let skills = paths::canonical_skills_dir().unwrap();
        fs::create_dir_all(&skills).unwrap();
        fs::create_dir_all(paths::registry_path().unwrap().parent().unwrap()).unwrap();
        (tmp, skills, home)
    }

    #[test]
    fn lazy_init_creates_registry_from_scan() {
        let (_tmp, skills, _home) = setup();
        let disabled = skills.join("off");
        fs::create_dir(&disabled).unwrap();
        fs::write(
            disabled.join("SKILL.md"),
            "---\nname: off\ndisable-model-invocation: true\n---\n# x\n",
        )
        .unwrap();
        let enabled = skills.join("on");
        fs::create_dir(&enabled).unwrap();
        fs::write(enabled.join("SKILL.md"), "# enabled\n").unwrap();

        let reg = ensure_loaded().unwrap();
        assert_eq!(reg.skills.get("off").map(|e| e.enabled), Some(false));
        assert_eq!(reg.skills.get("on").map(|e| e.enabled), Some(true));
    }

    #[test]
    fn merge_adds_new_folders_on_subsequent_load() {
        let (_tmp, skills, _home) = setup();
        let first = skills.join("first");
        fs::create_dir(&first).unwrap();
        fs::write(first.join("SKILL.md"), "# a\n").unwrap();

        let mut reg = ensure_loaded().unwrap();
        assert!(reg.skills.contains_key("first"));

        let second = skills.join("second");
        fs::create_dir(&second).unwrap();
        fs::write(
            second.join("SKILL.md"),
            "---\nname: second\ndisable-model-invocation: true\n---\n# b\n",
        )
        .unwrap();

        reg = ensure_loaded().unwrap();
        assert!(reg.skills.contains_key("second"));
        assert_eq!(reg.skills.get("second").map(|e| e.enabled), Some(false));
    }

    #[test]
    fn merge_skips_folders_without_skill_md() {
        let (_tmp, skills, _home) = setup();
        fs::create_dir(skills.join("empty")).unwrap();

        let reg = ensure_loaded().unwrap();
        assert!(!reg.skills.contains_key("empty"));
    }

    #[test]
    fn import_disabled_on_unreadable_or_unparseable_skill_md() {
        let (_tmp, skills, _home) = setup();
        let corrupt = skills.join("corrupt");
        fs::create_dir(&corrupt).unwrap();
        fs::write(corrupt.join("SKILL.md"), "---\nunclosed\n").unwrap();
        assert!(!import_enabled_from_frontmatter(&corrupt));

        let unreadable = skills.join("unreadable");
        fs::create_dir(&unreadable).unwrap();
        fs::write(unreadable.join("SKILL.md"), "# x\n").unwrap();
        let mut perms = fs::metadata(unreadable.join("SKILL.md"))
            .unwrap()
            .permissions();
        perms.set_readonly(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o000);
        }
        fs::set_permissions(unreadable.join("SKILL.md"), perms).unwrap();
        assert!(!import_enabled_from_frontmatter(&unreadable));
    }

    #[test]
    fn write_roundtrip() {
        let (_tmp, _skills, _home) = setup();
        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("foo".into(), SkillEntry { enabled: false });
        write(&reg).unwrap();

        let data = fs::read_to_string(paths::registry_path().unwrap()).unwrap();
        let loaded: Registry = serde_json::from_str(&data).unwrap();
        assert_eq!(loaded.skills.get("foo").map(|e| e.enabled), Some(false));
        assert!(data.contains("canonicalDir"));
        assert!(!data.contains("packs"));
    }

    #[test]
    fn packs_roundtrip_and_default_when_absent() {
        let (_tmp, _skills, _home) = setup();
        let mut reg = Registry::empty().unwrap();
        reg.packs
            .insert("fe".into(), ["a".to_string(), "b".to_string()].into());
        write(&reg).unwrap();
        assert_eq!(read_from_disk().unwrap(), reg);

        let old: Registry =
            serde_json::from_str(r#"{"version":1,"canonicalDir":"/x","skills":{}}"#).unwrap();
        assert!(old.packs.is_empty());
    }
}
