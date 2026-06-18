//! `cursor-skill-toggle` library — manage Cursor skill enable/disable state.
//!
//! Registry at `~/.cursor/skills-registry.json` is the source of truth for user
//! intent; [`sync`] applies it to `SKILL.md` frontmatter under `~/.agents/skills/`.

pub mod discovery;
pub mod doctor;
pub mod frontmatter;
pub mod paths;
pub mod registry;
pub mod sync;
pub mod tui;

pub use doctor::run_doctor;
pub use registry::Registry;
pub use sync::run_sync;
pub use tui::run_tui;
