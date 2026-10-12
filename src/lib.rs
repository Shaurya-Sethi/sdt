//! Skill Discovery Toggle — manage shared agent skill enable/disable state.
//!
//! Registry at `~/.agents/skills-registry.json` is the source of truth for user
//! intent; [`sync`] applies it to `SKILL.md` frontmatter under `~/.agents/skills/`.
//! Agents must honor `disable-model-invocation` for the toggle to take effect.

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
