# cursor-skills — Implementation Plan

Approved 2026-06-18.

## Goal

Build `cursor-skills`: a minimal CLI to enable/disable skills for **Cursor IDE + CLI** without deleting shared skill files, achieving **full removal** of disabled skills from Cursor's agent context at session start.

## Problem

Skills live in a shared global store at `~/.agents/skills/` with symlinks for multiple agents. Grok toggles via `[skills] disabled` in `~/.grok/config.toml`. Cursor has no per-skill disable — only deletion — which breaks the shared-store workflow and loads ~30+ skill descriptions into context on every session.

## Approach

Toggle `disable-model-invocation: true` in each skill's canonical `SKILL.md`. Phase 0 verification (2026-06-18) confirmed Cursor excludes flagged skills from startup context entirely.

A registry at `~/.cursor/skills-registry.json` holds user intent; `sync` applies it to disk.

## Locked decisions

| Decision | Choice |
|----------|--------|
| Disable semantics | Full removal from Cursor context at session start |
| Scope | Cursor-only management; other agents read the same `SKILL.md` — cross-agent side effect is acceptable |
| Canonical store | Keep `~/.agents/skills/` — do not relocate (required for `npx skills`) |
| Registry location | `~/.cursor/skills-registry.json` |
| Registry keys | Folder names under `~/.agents/skills/` |
| Discovery scope | `~/.agents/skills/` only |
| v1 commands | Interactive toggle, `sync`, `doctor` — nothing else |
| Registry init | Lazy on first run of any command |
| Existing flagged skills | Imported as disabled on first scan |
| New skills (not in registry) | Honor frontmatter flag if present; otherwise default to enabled |
| TUI writes | Registry only — does not touch `SKILL.md` |
| `sync` orphans | Auto-remove registry entries for missing skill folders |
| Frontmatter-less `SKILL.md` | `sync` injects minimal frontmatter block when disabling |
| Re-enabling | Remove `disable-model-invocation` key; collapse injected frontmatter back to body-only when empty |
| Post-sync | Silent — no reload reminder |
| `doctor` | Informational only; always exit 0 |
| Drift after `npx skills update` | Document workflow; `doctor` flags registry/frontmatter mismatch; user runs `sync` |
| Implementation | Rust — single static binary; no language runtime required |

## Commands

Install binary to `~/.local/bin/cursor-skills` (e.g. `cargo install --path .` or copy release build).

| Command | Purpose |
|---------|---------|
| `cursor-skills` | Interactive TUI — filter, toggle skills, save registry |
| `cursor-skills sync` | Apply registry → `SKILL.md` frontmatter on disk |
| `cursor-skills doctor` | Report orphans, drift, missing `SKILL.md`, broken state |

## Registry schema

```json
{
  "version": 1,
  "canonicalDir": "/Users/<user>/.agents/skills",
  "skills": {
    "brainstorming": { "enabled": true },
    "triage": { "enabled": false }
  }
}
```

## Lazy init

On first run of any command, if the registry file is missing:

1. Scan `~/.agents/skills/` for skill directories
2. For each folder with `SKILL.md`: `disable-model-invocation: true` → `enabled: false`; otherwise `enabled: true`
3. Write `~/.cursor/skills-registry.json`
4. Continue with the requested command

On subsequent runs, merge new skill folders using the same default rules.

## Sync behavior

**Disabled:** add `disable-model-invocation: true` to frontmatter. If no frontmatter exists, prepend:

```yaml
---
name: <folder>
disable-model-invocation: true
---
```

**Enabled:** remove `disable-model-invocation`. If the frontmatter block becomes empty (injected-only case), remove the entire `---` block.

**Missing folder:** remove the registry entry.

Safe YAML edit — preserve all other frontmatter fields.

## Doctor checks

- Registry entry with no skill folder on disk
- Registry says disabled but `SKILL.md` lacks the flag (post-`npx skills update` drift)
- Registry says enabled but flag is present
- Skill folder with no `SKILL.md`

All reported as warnings. Exit code always 0.

## npx skills compatibility

`npx skills` always writes to `~/.agents/skills/<name>/` and owns `~/.agents/.skill-lock.json`.

- `npx skills add` installs as enabled until you disable via `cursor-skills`
- `npx skills update` recreates canonical dirs and may reset frontmatter — run `cursor-skills sync` after

Recommended shell wrappers:

```bash
skills-add()   { npx skills add "$@" && cursor-skills sync; }
skills-update() { npx skills update "$@" && cursor-skills sync; }
```

Do not move canonical skills out of `~/.agents/skills/`. Do not edit `.skill-lock.json` manually.

## Out of scope (v1)

- Import/seed from Grok or other agent configs
- `cursor-skills init`, `enable`, `disable`, `list` subcommands
- Built-in skills (`~/.cursor/skills-cursor/`)
- Plugin-delivered skills
- Project-level `.cursor/skills/` in repos
- Patching `state.vscdb` or IDE internal storage

## Verification checklist

- [ ] `cursor-skills doctor` reports clean state after `sync`
- [ ] Disabled skills absent from Cursor Settings → Rules count
- [ ] Disabled skills absent from CLI `/` menu (new session)
- [ ] `npx skills update` + `cursor-skills sync` restores disable state
- [ ] Lazy init correctly imports existing `disable-model-invocation` skills as disabled