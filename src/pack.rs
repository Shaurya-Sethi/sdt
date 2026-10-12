//! Skill packs: named sets of skills toggled together.
//!
//! Packs live in [`Registry::packs`](crate::registry::Registry::packs). A pack's
//! on/off state is never stored; it is derived from its installed members, so
//! toggling a single skill can never leave it stale. Members that are not
//! installed are ignored for state and skipped when a pack is applied.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

/// Derived state of a pack from its installed members' enabled flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackState {
    On,
    Off,
    Mixed,
}

impl PackState {
    /// Derives the state from installed members' enabled flags. A pack with no
    /// installed members is `Off`.
    pub fn of(flags: impl IntoIterator<Item = bool>) -> Self {
        let (mut any_on, mut any_off) = (false, false);
        for enabled in flags {
            if enabled {
                any_on = true;
            } else {
                any_off = true;
            }
        }
        match (any_on, any_off) {
            (true, false) => Self::On,
            (true, true) => Self::Mixed,
            _ => Self::Off,
        }
    }
}

impl fmt::Display for PackState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Mixed => "mixed",
        })
    }
}

/// Whether `name` may name a pack: non-empty, no whitespace, no leading `-`
/// (which would read as a CLI flag).
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && !name.chars().any(char::is_whitespace)
}

/// Splits pack members into those in `installed` and those missing, keeping
/// member order.
pub fn split_members<'a>(
    members: &'a BTreeSet<String>,
    installed: &[String],
) -> (Vec<&'a str>, Vec<&'a str>) {
    members
        .iter()
        .map(String::as_str)
        .partition(|m| installed.iter().any(|i| i == m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_derived_from_member_flags() {
        assert_eq!(PackState::of([true, true]), PackState::On);
        assert_eq!(PackState::of([false, false]), PackState::Off);
        assert_eq!(PackState::of([true, false]), PackState::Mixed);
        assert_eq!(PackState::of([]), PackState::Off);
    }

    #[test]
    fn state_displays_and_serializes_lowercase() {
        assert_eq!(PackState::Mixed.to_string(), "mixed");
        assert_eq!(serde_json::to_string(&PackState::On).unwrap(), "\"on\"");
    }

    #[test]
    fn names_reject_empty_whitespace_and_leading_dash() {
        assert!(is_valid_name("frontend"));
        assert!(is_valid_name("azure-devops"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("my pack"));
        assert!(!is_valid_name("-x"));
    }

    #[test]
    fn split_members_separates_missing() {
        let members: BTreeSet<String> = ["a", "b", "c"].map(String::from).into();
        let installed = vec!["a".to_string(), "c".to_string(), "z".to_string()];
        assert_eq!(
            split_members(&members, &installed),
            (vec!["a", "c"], vec!["b"])
        );
    }
}
