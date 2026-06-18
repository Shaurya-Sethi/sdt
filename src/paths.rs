//! Canonical filesystem paths for skills and the registry.
//!
//! Tests may override `HOME` via [`set_home_for_tests`] to redirect all paths
//! into a temporary directory.

use std::env;
use std::io;
use std::path::PathBuf;

/// Returns the canonical skills directory: `$HOME/.agents/skills`.
pub fn canonical_skills_dir() -> io::Result<PathBuf> {
    Ok(home_dir()?.join(".agents").join("skills"))
}

/// Returns the registry file path: `$HOME/.cursor/skills-registry.json`.
pub fn registry_path() -> io::Result<PathBuf> {
    Ok(home_dir()?.join(".cursor").join("skills-registry.json"))
}

fn home_dir() -> io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Serializes `HOME` overrides so path-dependent tests do not race.
#[cfg(test)]
static HOME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Redirects path resolution to `home` for the duration of a test.
///
/// Holds a process-wide lock until the returned guard is dropped. Pair with a
/// [`tempfile::TempDir`] so the override is scoped to the test.
#[cfg(test)]
pub fn set_home_for_tests(home: &std::path::Path) -> std::sync::MutexGuard<'static, ()> {
    let guard = HOME_TEST_LOCK.lock().unwrap();
    // SAFETY: guarded by `HOME_TEST_LOCK`; tests must not set `HOME` elsewhere.
    unsafe {
        env::set_var("HOME", home);
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn paths_resolve_under_home_override() {
        let tmp = TempDir::new().unwrap();
        let _home = set_home_for_tests(tmp.path());

        let skills = canonical_skills_dir().unwrap();
        let registry = registry_path().unwrap();

        assert_eq!(skills, tmp.path().join(".agents").join("skills"));
        assert_eq!(
            registry,
            tmp.path().join(".cursor").join("skills-registry.json")
        );
    }

    #[test]
    fn canonical_skills_dir_is_absolute() {
        let tmp = TempDir::new().unwrap();
        let _home = set_home_for_tests(tmp.path());
        fs::create_dir_all(canonical_skills_dir().unwrap()).unwrap();

        assert!(canonical_skills_dir().unwrap().is_absolute());
    }
}
