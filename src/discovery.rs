//! Discover skill directories under the canonical skills store.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

/// Lists immediate child directories of the canonical skills dir, sorted by name.
///
/// Skips dot-directories and non-directories. Returns an empty vec if the skills
/// dir does not exist.
pub fn list_skill_dirs() -> io::Result<Vec<PathBuf>> {
    let skills_dir = paths::canonical_skills_dir()?;
    if !skills_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut dirs: Vec<PathBuf> = fs::read_dir(&skills_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| !name.starts_with('.'))
        })
        .map(|entry| entry.path())
        .collect();

    dirs.sort_by_cached_key(|p| p.file_name().map(|n| n.to_os_string()));
    Ok(dirs)
}

/// Returns the path to `SKILL.md` inside a skill directory.
pub fn skill_md_path(skill_dir: &Path) -> PathBuf {
    skill_dir.join("SKILL.md")
}

/// Returns whether `skill_dir/SKILL.md` exists.
pub fn has_skill_md(skill_dir: &Path) -> bool {
    skill_md_path(skill_dir).is_file()
}

/// Returns the folder name (registry key) for a skill directory.
pub fn skill_name(skill_dir: &Path) -> Option<String> {
    skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::set_home_for_tests;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf, std::sync::MutexGuard<'static, ()>) {
        let tmp = TempDir::new().unwrap();
        let home = set_home_for_tests(tmp.path());
        let skills = paths::canonical_skills_dir().unwrap();
        fs::create_dir_all(&skills).unwrap();
        (tmp, skills, home)
    }

    #[test]
    fn list_skill_dirs_skips_dot_dirs_and_files() {
        let (_tmp, skills, _home) = setup();
        fs::create_dir(skills.join("alpha")).unwrap();
        fs::create_dir(skills.join(".hidden")).unwrap();
        fs::write(skills.join("not-a-dir.txt"), "x").unwrap();
        fs::create_dir(skills.join("beta")).unwrap();

        let dirs = list_skill_dirs().unwrap();
        let names: Vec<_> = dirs.iter().filter_map(|p| skill_name(p)).collect();

        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn list_skill_dirs_empty_when_missing() {
        let tmp = TempDir::new().unwrap();
        let _home = set_home_for_tests(tmp.path());
        assert!(list_skill_dirs().unwrap().is_empty());
    }

    #[test]
    fn has_skill_md_detects_file() {
        let (_tmp, skills, _home) = setup();
        let dir = skills.join("foo");
        fs::create_dir(&dir).unwrap();
        assert!(!has_skill_md(&dir));
        fs::write(skill_md_path(&dir), "# Foo").unwrap();
        assert!(has_skill_md(&dir));
    }
}
