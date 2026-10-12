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
sdt                                  # interactive toggle for skills and packs
sdt disable azure-devops             # toggle skills by name
sdt pack add frontend design-review ui-polish
sdt pack enable frontend --only      # just the frontend skills
sdt doctor                           # check for drift or orphans
```

Every command that changes state writes the registry and syncs `SKILL.md` frontmatter immediately. Reload skills or start a new agent session afterward, depending on your agent.

## Commands

### `sdt`

Interactive checklist of skills that have a `SKILL.md` file, shown immediately. `Tab` switches between the **Skills** and **Packs** views. All changes stay pending until `Enter` saves (after confirmation) and syncs.

| Skills view | Action |
|-----|--------|
| `↑`/`↓`, `k`/`j` | Move |
| `Space` | Toggle skill |
| `/` | Filter by folder name (case-insensitive substring); `Enter` applies, `Esc` clears |
| `Enter` | Save and sync (after confirmation) |
| `Esc` | Clear the applied filter, or quit without saving if none |
| `q` | Quit without saving |

| Packs view | Action |
|-----|--------|
| `Space` | Toggle pack (a `mixed` pack turns on) |
| `o` | Enable only this pack; disable every other skill |
| `n` | New pack: type a name, then check its skills |
| `e` | Edit the pack's skills (`/` filters; `Enter`/`Esc` returns) |
| `d` | Delete the pack (its skills keep their state) |

- Checked skills are **enabled**; unchecked are **disabled** for automatic invocation in compatible agents.
- Toggles persist across filter changes; saving writes every skill's state and all packs.

### `sdt list`, `sdt enable`, `sdt disable`

```bash
sdt list [--enabled | --disabled] [--json]   # [x] name / [ ] name
sdt enable <skill>... | --all
sdt disable <skill>... | --all
```

Unknown skill names are an error (exit 1) and nothing is written. `--json` prints `[{"name", "enabled"}]` for scripts and agents.

### `sdt pack`

Packs are named sets of skills toggled together, e.g. `frontend`, `azure-devops`, `media`. See [docs/packs.md](docs/packs.md) for semantics.

```bash
sdt pack list [--json]               # name [on|off|mixed] members (missing: ...)
sdt pack add <pack> <skill>...       # creates the pack if needed
sdt pack remove <pack> <skill>...
sdt pack delete <pack>
sdt pack enable <pack>... [--only]   # --only disables every skill outside the packs
sdt pack disable <pack>...
```

- A pack's state is derived from its installed skills; it is never stored.
- A skill can be in several packs; the last command wins.
- Pack members that are not installed are skipped with a warning.

### `sdt sync`

Reads `~/.agents/skills-registry.json` and updates each skill's `SKILL.md`. Other commands sync automatically; run it directly to restore registry choices (for example after `npx skills update`).

- **Disabled** — sets `disable-model-invocation: true`. Adds a `name` when missing; files without frontmatter receive a minimal block.
- **Enabled** — removes the flag. A disabled block containing only `name` and the flag is stripped entirely.
- **Missing folder** — removes the stale registry entry. Pack membership is kept, so a reinstalled skill rejoins its packs.

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
| Pack member not installed | Pack lists a skill missing from the store |

## How it works

```text
sdt (TUI or command) → ~/.agents/skills-registry.json → sync → ~/.agents/skills/*/SKILL.md
```

- **Registry** holds your intent: which skills are enabled, and your packs.
- **Sync** applies that intent to shared files on disk.
- **Compatible agents** interpret the frontmatter when loading skills.

On the first run of any command except `doctor`, the registry is created by scanning the store. Skills with `disable-model-invocation: true` are imported as disabled; other readable, valid files default to enabled. Unreadable or invalid files are imported as disabled, and sync reports an error if it cannot process them. New skill folders are merged on later runs.

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
- No importing or writing agent configuration or per-agent profiles.

## Development

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

MIT
