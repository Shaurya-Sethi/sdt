//! Interactive terminal UI for toggling skills.

use std::io;

use console::{style, truncate_str, Key, Term};
use dialoguer::{theme::ColorfulTheme, Confirm};
use thiserror::Error;

use crate::discovery::{has_skill_md, list_skill_dirs, skill_name};
use crate::registry::{ensure_loaded, is_enabled, write, SkillEntry};

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("{0}")]
    Registry(#[from] crate::registry::RegistryError),
    #[error("discovery failed: {0}")]
    Discovery(#[from] io::Error),
    #[error("terminal error: {0}")]
    Terminal(io::Error),
    #[error("dialoguer error: {0}")]
    Dialoguer(#[from] dialoguer::Error),
    #[error("operation cancelled")]
    Cancelled,
}

/// Runs the interactive skill checklist.
///
/// All skills are listed immediately; `/` filters, Space toggles, Enter saves
/// (after confirmation) every skill's state to the registry, and `q`/Esc quits
/// without writing. Requires an interactive terminal on stderr.
pub fn run_tui() -> Result<(), TuiError> {
    let mut registry = ensure_loaded()?;
    let dirs = list_skill_dirs()?;

    let mut names: Vec<String> = dirs
        .iter()
        .filter(|d| has_skill_md(d))
        .filter_map(|d| skill_name(d))
        .collect();
    names.sort();

    if names.is_empty() {
        println!("No skills with SKILL.md found.");
        return Ok(());
    }

    let enabled = names.iter().map(|n| is_enabled(&registry, n)).collect();
    let mut picker = Picker::new(names, enabled);

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
        .with_prompt("Save changes to registry?")
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

    write(&registry)?;
    println!("Registry saved. Run `sdt sync` to apply changes.");
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

/// Terminal-free checklist state: `names[i]` is toggled by `enabled[i]`, and
/// `cursor` indexes into the currently visible (filtered) subset.
///
/// In browse mode, keys navigate and toggle, and Esc clears an applied filter
/// before it quits; in filter mode (entered with `/`) printable keys edit
/// `query`, Enter keeps it, and Esc clears it.
struct Picker {
    names: Vec<String>,
    enabled: Vec<bool>,
    query: String,
    filtering: bool,
    cursor: usize,
}

impl Picker {
    fn new(names: Vec<String>, enabled: Vec<bool>) -> Self {
        Self {
            names,
            enabled,
            query: String::new(),
            filtering: false,
            cursor: 0,
        }
    }

    /// Indices of names containing `query`, case-insensitively.
    fn visible(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        (0..self.names.len())
            .filter(|&i| self.names[i].to_lowercase().contains(&query))
            .collect()
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

        match key {
            Key::ArrowUp | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::ArrowDown | Key::Char('j') => {
                if self.cursor + 1 < self.visible().len() {
                    self.cursor += 1;
                }
            }
            Key::Char(' ') => {
                if let Some(&i) = self.visible().get(self.cursor) {
                    self.enabled[i] = !self.enabled[i];
                }
            }
            Key::Char('/') => self.filtering = true,
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

    /// Lines to draw for a terminal of `rows` x `cols`, scrolling the list so
    /// the cursor stays on screen and truncating lines so none wrap.
    fn render(&self, rows: usize, cols: usize) -> Vec<String> {
        let on = self.enabled.iter().filter(|&&e| e).count();
        let mut lines = vec![format!(
            "{}  {}",
            style("Skills").magenta().bold(),
            style(format!("{on} of {} enabled", self.names.len())).dim()
        )];
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
            let marker = if current {
                style("❯").cyan().bold()
            } else {
                style(" ")
            };
            let check = if self.enabled[i] {
                style("[x]").green().bold()
            } else {
                style("[ ]").dim()
            };
            let name = if current {
                style(name).cyan().bold()
            } else if self.enabled[i] {
                style(name)
            } else {
                style(name).dim()
            };
            lines.push(format!("{marker} {check} {name}"));
        }

        lines.push(String::new());
        lines.push(if self.filtering {
            hints(&[("type", "search"), ("enter", "apply"), ("esc", "clear")])
        } else if !self.query.is_empty() {
            hints(&[
                ("↑↓", "move"),
                ("space", "toggle"),
                ("/", "edit"),
                ("esc", "clear"),
                ("enter", "save"),
                ("q", "quit"),
            ])
        } else {
            hints(&[
                ("↑↓", "move"),
                ("space", "toggle"),
                ("/", "filter"),
                ("enter", "save"),
                ("q", "quit"),
            ])
        });
        lines
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
        Picker::new(names, enabled)
    }

    fn type_str(p: &mut Picker, s: &str) {
        for c in s.chars() {
            assert_eq!(p.handle(Key::Char(c)), None);
        }
    }

    fn visible_names(p: &Picker) -> Vec<&str> {
        p.visible().iter().map(|&i| p.names[i].as_str()).collect()
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
}
