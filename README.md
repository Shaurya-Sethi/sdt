# cursor-skill-toggle

Enable and disable Cursor agent skills without deleting them. Skills stay in the shared store at `~/.agents/skills/` (used by `npx skills` and other agents); `cst` controls which ones load into Cursor's agent context at session start.

## Why

Cursor discovers every skill under `~/.agents/skills/` and loads skill descriptions into context on each session. With dozens of skills installed, that adds up fast. Cursor has no built-in per-skill disable — only deletion — which breaks a shared skill store across agents.

`cst` fixes this by toggling `disable-model-invocation: true` in each skill's `SKILL.md`. Disabled skills stop contributing descriptions to agent context. Your skill files stay on disk and remain available to other tools that read the same store.

## Install

```bash
cargo install --path .
```

The binary lands in `~/.cargo/bin/cst`. Ensure that directory is on your `PATH`, or copy the release build to `~/.local/bin/`.

Build from source:

```bash
cargo build --release
# binary at target/release/cst
```

## Quick start

```bash
cst          # interactive toggle — saves to registry
cst sync     # apply registry → SKILL.md on disk
cst doctor   # check for drift or orphans
```

1. Run `cst` to enable/disable skills interactively.
2. Run `cst sync` to write changes to `SKILL.md` frontmatter.
3. Start a new Cursor agent session to pick up the change.

The TUI updates the registry only. You must run `sync` to apply changes to disk.

## Commands

### `cst`

Interactive checklist of skills that have a `SKILL.md` file.

- Optional filter: case-insensitive substring on folder name (press Enter for all).
- Checked items are **enabled**; unchecked are **disabled**.
- Confirm to save the registry; cancel discards changes.

### `cst sync`

Reads `~/.cursor/skills-registry.json` and updates each skill's `SKILL.md`:

- **Disabled** — sets `disable-model-invocation: true` in frontmatter. If the file has no frontmatter, prepends a minimal block with `name` and the flag.
- **Enabled** — removes the flag. If the frontmatter block was injection-only, strips it entirely.
- **Missing folder** — removes the stale registry entry.

Other frontmatter fields (`description`, `paths`, etc.) are preserved.

### `cst doctor`

Prints warnings for inconsistent state. Always exits 0 — informational only.

| Warning | Meaning |
|---------|---------|
| Registry entry, no folder | Orphan registry key |
| Folder with `SKILL.md`, not in registry | New skill not yet merged into registry |
| Folder without `SKILL.md` | Empty or incomplete skill directory |
| Registry disabled, no flag on disk | Run `sync` |
| Registry enabled, flag on disk | Run `sync` (often after `npx skills update`) |
| Unreadable `SKILL.md` | Permission or parse error |

## How it works

```
cst (TUI)  →  ~/.cursor/skills-registry.json  →  cst sync  →  ~/.agents/skills/*/SKILL.md
```

- **Registry** (`~/.cursor/skills-registry.json`) holds your intent: which skills are enabled.
- **Sync** applies that intent to `SKILL.md` frontmatter on disk.
- **Cursor** reads `SKILL.md` at session start and skips disabled skills from context.

On first run of any command, the registry is created automatically by scanning `~/.agents/skills/`. Skills with `disable-model-invocation: true` are imported as disabled; everything else defaults to enabled. New skill folders are merged in on later runs.

## Working with `npx skills`

`npx skills` installs to `~/.agents/skills/<name>/` and owns `~/.agents/.skill-lock.json`. `cst` does not touch the lock file.

- **`npx skills add`** — new skills default to enabled until you disable them in `cst`.
- **`npx skills update`** — may reset frontmatter. Run `cst sync` afterward to restore disable state from the registry.

Recommended shell helpers:

```bash
skills-add()    { npx skills add "$@" && cst sync; }
skills-update() { npx skills update "$@" && cst sync; }
```

Do not move skills out of `~/.agents/skills/` or edit `.skill-lock.json` by hand.

## Cross-agent note

Skills live in a shared store. Writing `disable-model-invocation` in `SKILL.md` affects any agent that reads those files (Grok, Codex, etc.), not just Cursor. That tradeoff keeps one canonical copy compatible with `npx skills`.

## What's not covered (v1)

- Built-in Cursor skills (`~/.cursor/skills-cursor/`)
- Plugin-delivered skills
- Project-level `.cursor/skills/` in repos
- Importing disable state from Grok or other agent configs
- `enable` / `disable` / `list` subcommands

## Development

```bash
cargo test
cargo fmt
cargo clippy -- -D warnings
```

Implementation details and design decisions are in [docs/PLAN.md](docs/PLAN.md).

## License

MIT
