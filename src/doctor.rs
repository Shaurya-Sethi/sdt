//! Informational health checks for registry and SKILL.md drift.

use std::collections::BTreeSet;
use std::io;

use thiserror::Error;

use crate::discovery::{has_skill_md, list_skill_dirs, skill_md_path, skill_name};
use crate::frontmatter::{is_disabled, parse_skill_md};
use crate::registry::read_from_disk;

#[derive(Debug, Error)]
pub enum DoctorError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
}

/// Collects all doctor warning messages without printing.
pub fn gather_warnings() -> Result<Vec<String>, DoctorError> {
    let registry = read_from_disk()?;
    let dirs = list_skill_dirs()?;
    let mut warnings = Vec::new();

    let on_disk: BTreeSet<String> = dirs.iter().filter_map(|d| skill_name(d)).collect();

    for name in registry.skills.keys() {
        if !on_disk.contains(name) {
            warnings.push(format!(
                "warning: registry entry '{name}' has no skill folder on disk"
            ));
        }
    }

    for dir in &dirs {
        let Some(name) = skill_name(dir) else {
            continue;
        };

        if has_skill_md(dir) && !registry.skills.contains_key(&name) {
            warnings.push(format!(
                "warning: skill folder '{name}' with SKILL.md is not in registry"
            ));
        }

        if !has_skill_md(dir) {
            warnings.push(format!("warning: skill folder '{name}' has no SKILL.md"));
            continue;
        }

        let path = skill_md_path(dir);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!(
                    "warning: could not read SKILL.md for '{name}': {e}"
                ));
                continue;
            }
        };
        let parsed = match parse_skill_md(&content) {
            Ok(p) => p,
            Err(e) => {
                warnings.push(format!(
                    "warning: could not parse SKILL.md for '{name}': {e}"
                ));
                continue;
            }
        };

        let Some(entry) = registry.skills.get(&name) else {
            continue;
        };
        let frontmatter_disabled = is_disabled(&parsed.frontmatter);

        if !entry.enabled && !frontmatter_disabled {
            warnings.push(format!(
                "warning: registry says '{name}' is disabled but SKILL.md lacks disable-model-invocation"
            ));
        }
        if entry.enabled && frontmatter_disabled {
            warnings.push(format!(
                "warning: registry says '{name}' is enabled but SKILL.md has disable-model-invocation"
            ));
        }
    }

    Ok(warnings)
}

/// Runs all doctor checks, printing warnings to stdout. Prints `all good` when
/// no issues are found. Always returns `Ok`.
pub fn run_doctor() -> Result<(), DoctorError> {
    let warnings = gather_warnings()?;
    if warnings.is_empty() {
        println!("all good");
    } else {
        for warning in warnings {
            println!("{warning}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{registry_path, set_home_for_tests};
    use crate::registry::{write, Registry, SkillEntry};
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (
        TempDir,
        std::path::PathBuf,
        std::sync::MutexGuard<'static, ()>,
    ) {
        let tmp = TempDir::new().unwrap();
        let home = set_home_for_tests(tmp.path());
        let skills = crate::paths::canonical_skills_dir().unwrap();
        fs::create_dir_all(&skills).unwrap();
        fs::create_dir_all(registry_path().unwrap().parent().unwrap()).unwrap();
        (tmp, skills, home)
    }

    #[test]
    fn warns_on_registry_orphan() {
        let (_tmp, _skills, _home) = setup();
        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("missing".into(), SkillEntry { enabled: true });
        write(&reg).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings.iter().any(|w| {
            w.contains("registry entry 'missing'") && w.contains("no skill folder on disk")
        }));
    }

    #[test]
    fn warns_on_unregistered_folder_with_skill_md() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("new-skill");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# x\n").unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings
            .iter()
            .any(|w| { w.contains("skill folder 'new-skill'") && w.contains("not in registry") }));
    }

    #[test]
    fn warns_on_drift_disabled() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("drift");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# no flag\n").unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("drift".into(), SkillEntry { enabled: false });
        write(&reg).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings.iter().any(|w| {
            w.contains("registry says 'drift' is disabled")
                && w.contains("lacks disable-model-invocation")
        }));
    }

    #[test]
    fn warns_on_missing_skill_md() {
        let (_tmp, skills, _home) = setup();
        fs::create_dir(skills.join("empty")).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings
            .iter()
            .any(|w| w.contains("skill folder 'empty'") && w.contains("has no SKILL.md")));
    }

    #[test]
    fn warns_on_drift_enabled() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("drift-on");
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: drift-on\ndisable-model-invocation: true\n---\n# x\n",
        )
        .unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("drift-on".into(), SkillEntry { enabled: true });
        write(&reg).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings.iter().any(|w| {
            w.contains("registry says 'drift-on' is enabled")
                && w.contains("has disable-model-invocation")
        }));
    }

    #[test]
    fn warns_on_unreadable_skill_md() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("locked");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# x\n").unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("locked".into(), SkillEntry { enabled: true });
        write(&reg).unwrap();

        let mut perms = fs::metadata(dir.join("SKILL.md")).unwrap().permissions();
        perms.set_readonly(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o000);
        }
        fs::set_permissions(dir.join("SKILL.md"), perms).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings
            .iter()
            .any(|w| w.contains("could not read SKILL.md for 'locked'")));
    }

    #[test]
    fn warns_on_unparseable_skill_md() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("broken");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "---\nunclosed\n").unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("broken".into(), SkillEntry { enabled: true });
        write(&reg).unwrap();

        let warnings = gather_warnings().unwrap();
        assert!(warnings
            .iter()
            .any(|w| w.contains("could not parse SKILL.md for 'broken'")));
    }

    #[test]
    fn run_doctor_returns_ok() {
        let (_tmp, _skills, _home) = setup();
        assert!(run_doctor().is_ok());
    }

    #[test]
    fn run_doctor_reports_all_good_when_clean() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("synced");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# x\n").unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("synced".into(), SkillEntry { enabled: true });
        write(&reg).unwrap();

        assert!(gather_warnings().unwrap().is_empty());
        assert!(run_doctor().is_ok());
    }
}
