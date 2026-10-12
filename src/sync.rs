//! Apply registry state to `SKILL.md` frontmatter on disk.

use std::fs;
use std::io;

use thiserror::Error;

use crate::discovery::{has_skill_md, list_skill_dirs, skill_md_path, skill_name};
use crate::frontmatter::{has_disable_key, is_disabled, parse_skill_md, set_disabled};
use crate::registry::{ensure_loaded, write};

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
    #[error("failed to sync {path}: {source}")]
    SkillMd { path: String, source: io::Error },
    #[error("invalid SKILL.md at {path}: {source}")]
    Frontmatter {
        path: String,
        #[source]
        source: crate::frontmatter::FrontmatterError,
    },
}

/// Applies the registry to all known skills and prunes orphan registry entries.
pub fn run_sync() -> Result<(), SyncError> {
    let mut registry = ensure_loaded()?;
    let dirs = list_skill_dirs()?;
    let on_disk: std::collections::BTreeSet<String> =
        dirs.iter().filter_map(|d| skill_name(d)).collect();

    registry.skills.retain(|name, _| on_disk.contains(name));

    for dir in &dirs {
        let Some(name) = skill_name(dir) else {
            continue;
        };
        if !has_skill_md(dir) {
            continue;
        }

        let path = skill_md_path(dir);
        let content = fs::read_to_string(&path).map_err(|e| SyncError::SkillMd {
            path: path.display().to_string(),
            source: e,
        })?;
        let parsed = parse_skill_md(&content).map_err(|e| SyncError::Frontmatter {
            path: path.display().to_string(),
            source: e,
        })?;

        let enabled = registry
            .skills
            .get(&name)
            .map(|e| e.enabled)
            .unwrap_or(true);
        let want_disabled = !enabled;
        let currently_disabled = is_disabled(&parsed.frontmatter);

        let needs_normalization =
            !want_disabled && has_disable_key(&parsed.frontmatter) && !currently_disabled;
        if want_disabled == currently_disabled && !needs_normalization {
            continue;
        }

        let new_content =
            set_disabled(&parsed, want_disabled, &name).map_err(|e| SyncError::Frontmatter {
                path: path.display().to_string(),
                source: e,
            })?;

        if new_content != content {
            atomic_write(&path, &new_content).map_err(|e| SyncError::SkillMd {
                path: path.display().to_string(),
                source: e,
            })?;
        }
    }

    write(&registry)?;
    Ok(())
}

fn atomic_write(path: &std::path::Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io::Error::new(
            e.kind(),
            format!(
                "failed to rename {} to {}: {e}",
                tmp.display(),
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{registry_path, set_home_for_tests};
    use crate::registry::{Registry, SkillEntry};
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
    fn sync_disables_and_enables() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("foo");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# body\n").unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("foo".into(), SkillEntry { enabled: false });
        crate::registry::write(&reg).unwrap();

        run_sync().unwrap();
        let content = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let parsed = parse_skill_md(&content).unwrap();
        assert!(is_disabled(&parsed.frontmatter));

        reg.skills.get_mut("foo").unwrap().enabled = true;
        crate::registry::write(&reg).unwrap();
        run_sync().unwrap();
        let content = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert_eq!(content, "# body\n");
    }

    #[test]
    fn sync_preserves_name_only_frontmatter_for_enabled_skill() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("foo");
        fs::create_dir(&dir).unwrap();
        let original = "---\nname: foo\n---\n# body\n";
        fs::write(dir.join("SKILL.md"), original).unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("foo".into(), SkillEntry { enabled: true });
        crate::registry::write(&reg).unwrap();

        run_sync().unwrap();
        let content = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert_eq!(content, original);
    }

    #[test]
    fn sync_removes_falsy_disable_key_when_enabled() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("foo");
        fs::create_dir(&dir).unwrap();
        let original = "---\nname: foo\ndisable-model-invocation: false\n---\n# body\n";
        fs::write(dir.join("SKILL.md"), original).unwrap();

        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("foo".into(), SkillEntry { enabled: true });
        crate::registry::write(&reg).unwrap();

        run_sync().unwrap();
        let content = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        let parsed = parse_skill_md(&content).unwrap();
        assert!(!has_disable_key(&parsed.frontmatter));
        assert_eq!(parsed.body, "# body\n");
    }

    #[test]
    fn sync_prunes_orphan_registry_entries() {
        let (_tmp, skills, _home) = setup();
        let mut reg = Registry::empty().unwrap();
        reg.skills
            .insert("gone".into(), SkillEntry { enabled: true });
        reg.packs.insert("p".into(), ["gone".to_string()].into());
        crate::registry::write(&reg).unwrap();

        let dir = skills.join("stay");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), "# x\n").unwrap();

        run_sync().unwrap();
        let reg = crate::registry::ensure_loaded().unwrap();
        assert!(!reg.skills.contains_key("gone"));
        assert!(reg.skills.contains_key("stay"));
        // Pack members survive so a reinstalled skill rejoins its pack.
        assert!(reg.packs["p"].contains("gone"));
    }
}
