//! Interactive terminal UI for toggling skills and skill packs.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use console::{style, truncate_str, Key, Term};
use dialoguer::{theme::ColorfulTheme, Confirm};
use thiserror::Error;

use crate::discovery::list_skill_names;
use crate::pack::{is_valid_name, PackState};
use crate::registry::{ensure_loaded, is_enabled, write, SkillEntry};
use crate::sync::{run_sync, SyncError};

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("{0}")]
    Sync(#[from] SyncError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
    #[error("terminal error: {0}")]
    Terminal(io::Error),
    #[error("dialoguer error: {0}")]
    Dialoguer(#[from] dialoguer::Error),
    #[error("operation cancelled")]
    Cancelled,
}

/// Runs the interactive skill and pack checklist.
///
/// All skills are listed immediately; `/` filters, Space toggles, and Tab
/// switches to the Packs view, where packs are toggled, created, edited, and
/// deleted. Every change stays pending until Enter saves (after confirmation)
/// skill states and packs to the registry and syncs `SKILL.md` files; `q`/Esc
/// quits without writing. Requires an interactive terminal on stderr.
pub fn run_tui() -> Result<(), TuiError> {
    let mut registry = ensure_loaded()?;
    let names = list_skill_names()?;
    if names.is_empty() {
        println!("No skills with SKILL.md found.");
        return Ok(());
    }

    let enabled = names.iter().map(|n| is_enabled(&registry, n)).collect();
    let mut picker = Picker::new(names, enabled, registry.packs.clone());

    let term = Term::stderr();
    if !term.is_term() {
        return Err(TuiError::Terminal(io::Error::new(
            io::ErrorKind::NotConnected,
            "not a terminal",
        )));
    }
    term.hide_cursor().map_err(TuiError::Terminal)?;
    let outcome = run_picker(&term, &mut picker);
    term.show_cursor().map_err(TuiError::Terminal)?;
    if outcome.map_err(TuiError::Terminal)? == Outcome::Quit {
        return Ok(());
    }

    let confirmed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Save and sync changes?")
        .default(true)
        .interact()?;

    if !confirmed {
        return Err(TuiError::Cancelled);
    }

    for (name, enabled) in picker.names.into_iter().zip(picker.enabled) {
        registry
            .skills
            .entry(name)
            .or_insert(SkillEntry { enabled: true })
            .enabled = enabled;
    }
    registry.packs = picker.packs;

    write(&registry)?;
    run_sync()?;
    println!("Saved and synced.");
    Ok(())
}

/// Draws the picker and feeds it keys until it returns an outcome; clears its
/// own output before returning.
fn run_picker(term: &Term, picker: &mut Picker) -> io::Result<Outcome> {
    let mut drawn = 0;
    loop {
        let (rows, cols) = term.size();
        let lines = picker.render(rows as usize, cols as usize);
        term.clear_last_lines(drawn)?;
        for line in &lines {
            term.write_line(line)?;
        }
        drawn = lines.len();

        if let Some(outcome) = picker.handle(term.read_key()?) {
            term.clear_last_lines(drawn)?;
            return Ok(outcome);
        }
    }
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Save,
    Quit,
}

/// Which screen the picker shows.
#[derive(Debug, Clone, PartialEq)]
enum View {
    /// Skill checklist; checks mean enabled.
    Skills,
    /// Pack list with derived states.
    Packs,
    /// Typing a new pack's name.
    Naming,
    /// Skill checklist for one pack; checks mean membership.
    Members(String),
}

/// Terminal-free picker state: `names[i]` is toggled by `enabled[i]`, `packs`
/// holds pending pack membership, and `cursor` indexes into the currently
/// visible (filtered) skills while `pack_cursor` indexes into `packs`.
///
/// In the Skills and Members checklists, keys navigate and toggle, and Esc
/// clears an applied filter before it leaves; in filter mode (entered with `/`)
/// printable keys edit `query`, Enter keeps it, and Esc clears it.
struct Picker {
    names: Vec<String>,
    enabled: Vec<bool>,
    packs: BTreeMap<String, BTreeSet<String>>,
    view: View,
    query: String,
    filtering: bool,
    cursor: usize,
    pack_cursor: usize,
    new_name: String,
}

impl Picker {
    fn new(
        names: Vec<String>,
        enabled: Vec<bool>,
        packs: BTreeMap<String, BTreeSet<String>>,
    ) -> Self {
        Self {
            names,
            enabled,
            packs,
            view: View::Skills,
            query: String::new(),
            filtering: false,
            cursor: 0,
            pack_cursor: 0,
            new_name: String::new(),
        }
    }

    /// Indices of names containing `query`, case-insensitively.
    fn visible(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        (0..self.names.len())
            .filter(|&i| self.names[i].to_lowercase().contains(&query))
            .collect()
    }

    /// Whether skill `i` is checked in the current checklist view.
    fn checked(&self, i: usize) -> bool {
        match &self.view {
            View::Members(pack) => self.packs[pack].contains(&self.names[i]),
            _ => self.enabled[i],
        }
    }

    /// The pack under `pack_cursor`, if any.
    fn selected_pack(&self) -> Option<String> {
        self.packs.keys().nth(self.pack_cursor).cloned()
    }

    /// Derived state of `pack` from its installed members' pending flags.
    fn pack_state(&self, pack: &str) -> PackState {
        let members = &self.packs[pack];
        PackState::of(
            self.names
                .iter()
                .zip(&self.enabled)
                .filter(|(name, _)| members.contains(*name))
                .map(|(_, &enabled)| enabled),
        )
    }

    /// Sets installed members of `pack` to `enable`; with `only`, disables
    /// every other skill.
    fn apply_pack(&mut self, pack: &str, enable: bool, only: bool) {
        let members = &self.packs[pack];
        for (name, enabled) in self.names.iter().zip(self.enabled.iter_mut()) {
            if members.contains(name) {
                *enabled = enable;
            } else if only {
                *enabled = false;
            }
        }
    }

    /// Whether the typed pack name can be created.
    fn new_name_ok(&self) -> bool {
        is_valid_name(&self.new_name) && !self.packs.contains_key(&self.new_name)
    }

    /// Opens the member checklist for `pack` with a fresh filter.
    fn edit_members(&mut self, pack: String) {
        self.query.clear();
        self.cursor = 0;
        self.view = View::Members(pack);
    }

    /// Applies one key press; returns an outcome when the picker should close.
    fn handle(&mut self, key: Key) -> Option<Outcome> {
        if self.filtering {
            match key {
                Key::Enter => self.filtering = false,
                Key::Escape => {
                    self.filtering = false;
                    self.query.clear();
                }
                Key::Backspace => {
                    self.query.pop();
                }
                Key::Char(c) if !c.is_control() => self.query.push(c),
                _ => return None,
            }
            self.cursor = 0;
            return None;
        }

        match self.view.clone() {
            View::Skills => self.handle_checklist(key),
            View::Members(_) => {
                if self.handle_checklist(key).is_some() {
                    // Leaving a pack's members returns to the Packs view.
                    self.query.clear();
                    self.cursor = 0;
                    self.view = View::Packs;
                }
                None
            }
            View::Packs => self.handle_packs(key),
            View::Naming => {
                self.handle_naming(key);
                None
            }
        }
    }

    /// Keys for the Skills and Members checklists. An outcome means "leave":
    /// Save/Quit in the Skills view, done in the Members view.
    fn handle_checklist(&mut self, key: Key) -> Option<Outcome> {
        match key {
            Key::ArrowUp | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::ArrowDown | Key::Char('j') => {
                if self.cursor + 1 < self.visible().len() {
                    self.cursor += 1;
                }
            }
            Key::Char(' ') => {
                if let Some(&i) = self.visible().get(self.cursor) {
                    match &self.view {
                        View::Members(pack) => {
                            let members = self.packs.get_mut(pack).expect("pack exists");
                            if !members.remove(&self.names[i]) {
                                members.insert(self.names[i].clone());
                            }
                        }
                        _ => self.enabled[i] = !self.enabled[i],
                    }
                }
            }
            Key::Char('/') => self.filtering = true,
            Key::Tab if self.view == View::Skills => self.view = View::Packs,
            Key::Enter => return Some(Outcome::Save),
            Key::Escape if !self.query.is_empty() => {
                self.query.clear();
                self.cursor = 0;
            }
            Key::Char('q') | Key::Escape => return Some(Outcome::Quit),
            _ => {}
        }
        None
    }

    fn handle_packs(&mut self, key: Key) -> Option<Outcome> {
        let selected = self.selected_pack();
        match key {
            Key::ArrowUp | Key::Char('k') => self.pack_cursor = self.pack_cursor.saturating_sub(1),
            Key::ArrowDown | Key::Char('j') => {
                if self.pack_cursor + 1 < self.packs.len() {
                    self.pack_cursor += 1;
                }
            }
            Key::Char(' ') => {
                if let Some(pack) = selected {
                    let enable = self.pack_state(&pack) != PackState::On;
                    self.apply_pack(&pack, enable, false);
                }
            }
            Key::Char('o') => {
                if let Some(pack) = selected {
                    self.apply_pack(&pack, true, true);
                }
            }
            Key::Char('n') => {
                self.new_name.clear();
                self.view = View::Naming;
            }
            Key::Char('e') => {
                if let Some(pack) = selected {
                    self.edit_members(pack);
                }
            }
            Key::Char('d') => {
                if let Some(pack) = selected {
                    self.packs.remove(&pack);
                    self.pack_cursor = self.pack_cursor.min(self.packs.len().saturating_sub(1));
                }
            }
            Key::Tab => self.view = View::Skills,
            Key::Enter => return Some(Outcome::Save),
            Key::Char('q') | Key::Escape => return Some(Outcome::Quit),
            _ => {}
        }
        None
    }

    /// Keys while typing a new pack name: Enter creates it (when valid and
    /// unused) and opens its member checklist; Esc cancels.
    fn handle_naming(&mut self, key: Key) {
        match key {
            Key::Enter if self.new_name_ok() => {
                let pack = std::mem::take(&mut self.new_name);
                self.packs.insert(pack.clone(), BTreeSet::new());
                self.pack_cursor = self.packs.keys().position(|p| *p == pack).unwrap_or(0);
                self.edit_members(pack);
            }
            Key::Escape => self.view = View::Packs,
            Key::Backspace => {
                self.new_name.pop();
            }
            Key::Char(c) if !c.is_control() => self.new_name.push(c),
            _ => {}
        }
    }

    /// Lines to draw for a terminal of `rows` x `cols`, scrolling lists so the
    /// cursor stays on screen and truncating lines so none wrap.
    fn render(&self, rows: usize, cols: usize) -> Vec<String> {
        match &self.view {
            View::Packs => self.render_packs(rows, cols),
            View::Naming => self.render_naming(),
            _ => self.render_checklist(rows, cols),
        }
    }

    fn render_checklist(&self, rows: usize, cols: usize) -> Vec<String> {
        let checked = (0..self.names.len()).filter(|&i| self.checked(i)).count();
        let header = match &self.view {
            View::Members(pack) => format!(
                "{} {}  {}",
                style("Pack").magenta().bold(),
                style(pack).bold(),
                style(format!("{checked} member(s)")).dim()
            ),
            _ => format!(
                "{}  {}",
                style("Skills").magenta().bold(),
                style(format!("{checked} of {} enabled", self.names.len())).dim()
            ),
        };
        let mut lines = vec![header];
        if self.filtering {
            lines.push(format!(
                "{} {}{}  {}",
                style("/").yellow().bold(),
                style(&self.query).bold(),
                style("█").yellow(),
                style("esc to clear").dim()
            ));
        } else if !self.query.is_empty() {
            lines.push(format!(
                "{} {}",
                style("filter:").yellow(),
                style(&self.query).bold()
            ));
        }

        let visible = self.visible();
        if visible.is_empty() {
            lines.push(style("  No skills match").dim().italic().to_string());
        }
        // Reserve the blank + hint footer lines and one spare row.
        let height = rows.saturating_sub(lines.len() + 3).max(1);
        let start = (self.cursor + 1).saturating_sub(height);
        for (pos, &i) in visible.iter().enumerate().skip(start).take(height) {
            let current = pos == self.cursor && !self.filtering;
            let name = truncate_str(&self.names[i], cols.saturating_sub(6), "…");
            let check = if self.checked(i) {
                style("[x]").green().bold()
            } else {
                style("[ ]").dim()
            };
            let name = if current {
                style(name).cyan().bold()
            } else if self.checked(i) {
                style(name)
            } else {
                style(name).dim()
            };
            lines.push(format!("{} {check} {name}", marker(current)));
        }

        let members = matches!(self.view, View::Members(_));
        let (enter, leave) = if members {
            ("done", "done")
        } else {
            ("save", "quit")
        };
        lines.push(String::new());
        lines.push(if self.filtering {
            hints(&[("type", "search"), ("enter", "apply"), ("esc", "clear")])
        } else {
            let mut pairs = vec![("↑↓", "move"), ("space", "toggle")];
            if self.query.is_empty() {
                pairs.push(("/", "filter"));
            } else {
                pairs.extend([("/", "edit"), ("esc", "clear")]);
            }
            if !members {
                pairs.push(("tab", "packs"));
            }
            pairs.push(("enter", enter));
            if self.query.is_empty() {
                pairs.push(if members {
                    ("esc", leave)
                } else {
                    ("q", leave)
                });
            }
            hints(&pairs)
        });
        lines
    }

    fn render_packs(&self, rows: usize, cols: usize) -> Vec<String> {
        let mut lines = vec![format!(
            "{}  {}",
            style("Packs").magenta().bold(),
            style(format!("{} pack(s)", self.packs.len())).dim()
        )];
        if self.packs.is_empty() {
            lines.push(style("  No packs yet").dim().italic().to_string());
        }
        let height = rows.saturating_sub(lines.len() + 3).max(1);
        let start = (self.pack_cursor + 1).saturating_sub(height);
        for (pos, (pack, members)) in self.packs.iter().enumerate().skip(start).take(height) {
            let current = pos == self.pack_cursor;
            let state = match self.pack_state(pack) {
                PackState::On => style("[on]   ").green().bold(),
                PackState::Mixed => style("[mixed]").yellow().bold(),
                PackState::Off => style("[off]  ").dim(),
            };
            let name = truncate_str(pack, cols.saturating_sub(30), "…");
            let name = if current {
                style(name).cyan().bold()
            } else {
                style(name)
            };
            let count = style(format!("{} skill(s)", members.len())).dim();
            lines.push(format!("{} {state} {name}  {count}", marker(current)));
        }

        lines.push(String::new());
        lines.push(hints(&[
            ("↑↓", "move"),
            ("space", "toggle"),
            ("o", "only"),
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("tab", "skills"),
            ("enter", "save"),
            ("q", "quit"),
        ]));
        lines
    }

    fn render_naming(&self) -> Vec<String> {
        let mut lines = vec![
            style("New pack").magenta().bold().to_string(),
            format!(
                "{} {}{}",
                style("name:").yellow().bold(),
                style(&self.new_name).bold(),
                style("█").yellow()
            ),
        ];
        if !self.new_name.is_empty() && !self.new_name_ok() {
            lines.push(
                style("  name must be new, without spaces, and not start with '-'")
                    .red()
                    .to_string(),
            );
        }
        lines.push(String::new());
        lines.push(hints(&[
            ("type", "name"),
            ("enter", "pick skills"),
            ("esc", "cancel"),
        ]));
        lines
    }
}

/// The cursor marker for a list row.
fn marker(current: bool) -> console::StyledObject<&'static str> {
    if current {
        style("❯").cyan().bold()
    } else {
        style(" ")
    }
}

/// Formats `(key, action)` pairs as a styled footer line.
fn hints(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, action)| format!("{} {}", style(key).cyan().bold(), style(action).dim()))
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(names: &[&str]) -> Picker {
        let names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        let enabled = vec![false; names.len()];
        Picker::new(names, enabled, BTreeMap::new())
    }

    /// Skills a, b, c (all disabled) with packs `p` = {a, b, gone} and `q` = {c}.
    fn pack_picker() -> Picker {
        let mut p = picker(&["a", "b", "c"]);
        p.packs
            .insert("p".into(), ["a", "b", "gone"].map(String::from).into());
        p.packs.insert("q".into(), ["c".to_string()].into());
        p
    }

    fn type_str(p: &mut Picker, s: &str) {
        for c in s.chars() {
            assert_eq!(p.handle(Key::Char(c)), None);
        }
    }

    fn visible_names(p: &Picker) -> Vec<&str> {
        p.visible().iter().map(|&i| p.names[i].as_str()).collect()
    }

    fn members(p: &Picker, pack: &str) -> Vec<String> {
        p.packs[pack].iter().cloned().collect()
    }

    fn renders(p: &Picker, text: &str) -> bool {
        p.render(20, 80).iter().any(|l| l.contains(text))
    }

    #[test]
    fn starts_browsing_with_all_skills_visible() {
        let p = picker(&["a", "b", "c"]);
        assert!(!p.filtering);
        assert_eq!(visible_names(&p), vec!["a", "b", "c"]);
    }

    #[test]
    fn q_and_escape_quit_while_browsing() {
        assert_eq!(picker(&["a"]).handle(Key::Char('q')), Some(Outcome::Quit));
        assert_eq!(picker(&["a"]).handle(Key::Escape), Some(Outcome::Quit));
    }

    #[test]
    fn enter_saves_while_browsing() {
        assert_eq!(picker(&["a"]).handle(Key::Enter), Some(Outcome::Save));
    }

    #[test]
    fn slash_filters_case_insensitive_substring() {
        let mut p = picker(&["Brainstorming", "triage", "my-brain"]);
        type_str(&mut p, "/brain");
        assert!(p.filtering);
        assert_eq!(visible_names(&p), vec!["Brainstorming", "my-brain"]);
    }

    #[test]
    fn q_types_into_query_while_filtering() {
        let mut p = picker(&["quick", "slow"]);
        type_str(&mut p, "/q");
        assert_eq!(p.query, "q");
        assert_eq!(visible_names(&p), vec!["quick"]);
    }

    #[test]
    fn enter_keeps_query_and_escape_clears_it() {
        let mut p = picker(&["alpha", "beta"]);
        type_str(&mut p, "/al");
        assert_eq!(p.handle(Key::Enter), None);
        assert!(!p.filtering);
        assert_eq!(visible_names(&p), vec!["alpha"]);

        type_str(&mut p, "/");
        assert_eq!(p.handle(Key::Escape), None);
        assert!(!p.filtering);
        assert_eq!(p.query, "");
        assert_eq!(visible_names(&p), vec!["alpha", "beta"]);
    }

    #[test]
    fn escape_clears_applied_filter_before_quitting() {
        let mut p = picker(&["alpha", "beta"]);
        type_str(&mut p, "/al");
        p.handle(Key::Enter);
        assert_eq!(p.handle(Key::Escape), None);
        assert_eq!(p.query, "");
        assert_eq!(p.handle(Key::Escape), Some(Outcome::Quit));
    }

    #[test]
    fn backspace_edits_query() {
        let mut p = picker(&["alpha", "beta"]);
        type_str(&mut p, "/alx");
        assert!(p.visible().is_empty());
        p.handle(Key::Backspace);
        assert_eq!(visible_names(&p), vec!["alpha"]);
    }

    #[test]
    fn toggle_in_filtered_view_persists_after_clearing() {
        let mut p = picker(&["alpha", "beta", "gamma"]);
        type_str(&mut p, "/gam");
        p.handle(Key::Enter);
        p.handle(Key::Char(' '));
        p.handle(Key::Char('/'));
        p.handle(Key::Escape);
        assert_eq!(p.enabled, vec![false, false, true]);
    }

    #[test]
    fn filter_mode_shows_how_to_clear() {
        let mut p = picker(&["alpha"]);
        let shows_clear = |p: &Picker| {
            p.render(20, 80)
                .iter()
                .any(|l| l.contains("esc") && l.contains("clear"))
        };
        assert!(!shows_clear(&p));
        type_str(&mut p, "/al");
        assert!(shows_clear(&p));
    }

    #[test]
    fn cursor_moves_and_clamps_to_visible() {
        let mut p = picker(&["alpha", "beta", "gamma"]);
        p.handle(Key::Char('j'));
        p.handle(Key::ArrowDown);
        p.handle(Key::ArrowDown);
        assert_eq!(p.cursor, 2);
        type_str(&mut p, "/beta");
        assert_eq!(p.cursor, 0);
        p.handle(Key::Enter);
        p.handle(Key::Char(' '));
        assert_eq!(p.enabled, vec![false, true, false]);
    }

    #[test]
    fn tab_switches_between_skills_and_packs() {
        let mut p = pack_picker();
        assert!(renders(&p, "packs"));
        p.handle(Key::Tab);
        assert_eq!(p.view, View::Packs);
        assert!(renders(&p, "Packs"));
        p.handle(Key::Tab);
        assert_eq!(p.view, View::Skills);
    }

    #[test]
    fn packs_view_saves_and_quits() {
        let mut p = pack_picker();
        p.handle(Key::Tab);
        assert_eq!(p.handle(Key::Enter), Some(Outcome::Save));
        let mut p = pack_picker();
        p.handle(Key::Tab);
        assert_eq!(p.handle(Key::Char('q')), Some(Outcome::Quit));
    }

    #[test]
    fn space_toggles_pack_and_mixed_turns_on() {
        let mut p = pack_picker();
        p.enabled[0] = true; // `p` is mixed.
        p.handle(Key::Tab);
        assert!(renders(&p, "mixed"));

        p.handle(Key::Char(' '));
        assert_eq!(p.enabled, vec![true, true, false]);
        assert_eq!(p.pack_state("p"), PackState::On);

        p.handle(Key::Char(' '));
        assert_eq!(p.enabled, vec![false, false, false]);
    }

    #[test]
    fn o_enables_only_the_selected_pack() {
        let mut p = pack_picker();
        p.enabled = vec![true, true, false];
        p.handle(Key::Tab);
        p.handle(Key::Char('j'));
        p.handle(Key::Char('o'));
        assert_eq!(p.enabled, vec![false, false, true]);
    }

    #[test]
    fn n_creates_pack_then_picks_members() {
        let mut p = pack_picker();
        p.handle(Key::Tab);
        p.handle(Key::Char('n'));
        type_str(&mut p, "fe q");
        assert!(renders(&p, "name must be new"));
        p.handle(Key::Enter);
        assert_eq!(p.view, View::Naming);
        p.handle(Key::Backspace);
        p.handle(Key::Backspace);
        p.handle(Key::Enter);
        assert_eq!(p.view, View::Members("fe".into()));

        p.handle(Key::Char('j'));
        p.handle(Key::Char(' '));
        assert_eq!(p.handle(Key::Enter), None);
        assert_eq!(p.view, View::Packs);
        assert_eq!(members(&p, "fe"), vec!["b"]);
        assert_eq!(p.selected_pack().as_deref(), Some("fe"));
        assert_eq!(p.enabled, vec![false, false, false]);
    }

    #[test]
    fn duplicate_name_is_rejected_and_escape_cancels() {
        let mut p = pack_picker();
        p.handle(Key::Tab);
        p.handle(Key::Char('n'));
        type_str(&mut p, "p");
        p.handle(Key::Enter);
        assert_eq!(p.view, View::Naming);
        p.handle(Key::Escape);
        assert_eq!(p.view, View::Packs);
        assert_eq!(p.packs.len(), 2);
    }

    #[test]
    fn e_edits_members_with_filter_and_keeps_missing_members() {
        let mut p = pack_picker();
        p.handle(Key::Tab);
        p.handle(Key::Char('e'));
        assert_eq!(p.view, View::Members("p".into()));
        assert!(renders(&p, "member(s)"));

        type_str(&mut p, "/c");
        p.handle(Key::Enter);
        p.handle(Key::Char(' '));
        // Esc clears the filter first, then leaves.
        assert_eq!(p.handle(Key::Escape), None);
        assert_eq!(p.view, View::Members("p".into()));
        p.handle(Key::Char(' '));
        assert_eq!(p.handle(Key::Escape), None);
        assert_eq!(p.view, View::Packs);
        assert_eq!(members(&p, "p"), vec!["b", "c", "gone"]);
    }

    #[test]
    fn d_deletes_selected_pack() {
        let mut p = pack_picker();
        p.handle(Key::Tab);
        p.handle(Key::Char('j'));
        p.handle(Key::Char('d'));
        assert_eq!(p.packs.keys().collect::<Vec<_>>(), vec!["p"]);
        assert_eq!(p.pack_cursor, 0);
        p.handle(Key::Char('d'));
        assert!(p.packs.is_empty());
        assert!(renders(&p, "No packs yet"));
        p.handle(Key::Char(' '));
        p.handle(Key::Char('e'));
        assert_eq!(p.view, View::Packs);
    }
}
