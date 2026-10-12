# sdt — Skill Discovery Toggle

An agent-agnostic CLI to enable and disable automatic skill discovery without deleting shared skill files.

Skills stay in `~/.agents/skills/`. `sdt` records your choices in a registry and applies them through `disable-model-invocation` in each skill's `SKILL.md`.

**Agent compatibility matters:** the toggle works only for agents that read this store (directly or through symlinks) and honor that frontmatter field. It is not a universal agent-disable mechanism or a per-agent setting.

## Why

Installed skills can add descriptions to agent context even when you rarely use them. Keep one shared skill store, choose which skills are available for automatic invocation, and restore those choices after skill updates.

## Install

```bash
cargo install --path .
```

The binary lands in `~/.cargo/bin/sdt`. Ensure that directory is on your `PATH`.

Build from source:

```bash
cargo build --release
# binary at target/release/sdt
```

## Quick start

```bash
sdt          # interactive toggle — saves to registry
sdt sync     # apply registry → SKILL.md on disk
sdt doctor   # check for drift or orphans
```

1. Run `sdt` to enable/disable skills interactively.
2. Run `sdt sync` to write changes to `SKILL.md` frontmatter.
3. Reload skills or start a new agent session, depending on your agent.

The TUI updates the registry only. You must run `sync` to apply changes to disk.

## Commands

### `sdt`

Interactive checklist of skills that have a `SKILL.md` file.

- Optional filter: case-insensitive substring on folder name (press Enter for all).
- Checked items are **enabled**; unchecked are **disabled** for automatic invocation in compatible agents.
- Confirm to save the registry; cancel leaves existing choices unchanged.

### `sdt sync`

Reads `~/.agents/skills-registry.json` and updates each skill's `SKILL.md`:

- **Disabled** — sets `disable-model-invocation: true`. Adds a `name` when missing; files without frontmatter receive a minimal block.
- **Enabled** — removes the flag. A disabled block containing only `name` and the flag is stripped entirely.
- **Missing folder** — removes the stale registry entry.

Other frontmatter values and the Markdown body are preserved. Modified frontmatter is reserialized, so YAML formatting and comments are not preserved.

### `sdt doctor`

Prints warnings for inconsistent state. Warnings exit 0; registry or discovery errors exit nonzero. It does not create or modify the registry.

| Warning | Meaning |
|---------|---------|
| Registry entry, no folder | Orphan registry key |
| Folder with `SKILL.md`, not in registry | New skill not yet merged into registry |
| Folder without `SKILL.md` | Empty or incomplete skill directory |
| Registry disabled, no flag on disk | Run `sync` |
| Registry enabled, flag on disk | Run `sync` (often after a skill update) |
| Unreadable or invalid `SKILL.md` | Permission or parse error |

## How it works

```text
sdt (TUI) → ~/.agents/skills-registry.json → sdt sync → ~/.agents/skills/*/SKILL.md
```

- **Registry** holds your intent: which skills are enabled.
- **Sync** applies that intent to shared files on disk.
- **Compatible agents** interpret the frontmatter when loading skills.

On first TUI or sync run, the registry is created by scanning the store. Skills with `disable-model-invocation: true` are imported as disabled; other readable, valid files default to enabled. Unreadable or invalid files are imported as disabled, and sync reports an error if it cannot process them. New skill folders are merged on later TUI/sync runs.

## Agent compatibility

`disable-model-invocation` is an agent extension, not a field in the core [Agent Skills specification](https://agentskills.io/specification).

- Agents honoring the field can exclude disabled skills from automatic invocation. Manual invocation may remain available; this is not an access-control boundary.
- Agents ignoring the field will continue discovering skills normally.
- Agents using separate skill stores are unaffected unless those stores point to these same files.
- Every compatible agent reading the shared files sees the same choices; `sdt` does not maintain per-agent profiles or edit agent configuration.

For example, [Claude Code documents this field](https://code.claude.com/docs/en/skills#control-who-invokes-a-skill), while [Codex documents its own configuration-based toggle](https://developers.openai.com/codex/skills/). Check your agent's documentation rather than assuming support.

## Working with `npx skills`

`npx skills` installs shared skills under `~/.agents/skills/<name>/` and manages `~/.agents/.skill-lock.json`. `sdt` does not touch the lock file.

- **`npx skills add`** — new skills import their current frontmatter state when next discovered.
- **`npx skills update`** — may reset frontmatter. Run `sdt sync` afterward to restore registry choices.

Recommended shell helpers:

```bash
skills-add()    { npx skills add "$@" && sdt sync; }
skills-update() { npx skills update "$@" && sdt sync; }
```

Do not move skills out of the shared store or edit `.skill-lock.json` by hand.

## Scope

- Immediate, non-hidden skill directories in `~/.agents/skills/` only; symlinked skill directories are currently skipped.
- No built-in, plugin-delivered, project-level, or separate agent-specific stores.
- No importing or writing agent configuration, per-agent profiles, or `enable` / `disable` / `list` subcommands.

## Development

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

MIT
