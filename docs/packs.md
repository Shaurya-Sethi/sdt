# Skill packs

Packs are named sets of skills you can turn on or off together. For example, you might enable `frontend` for UI work and `azure-devops` for pipeline work, without toggling dozens of skills one at a time.

## Storage

Packs live in the registry next to per-skill state:

```json
{
  "version": 1,
  "canonicalDir": "/Users/me/.agents/skills",
  "skills": { "ui-polish": { "enabled": true } },
  "packs": { "frontend": ["design-review", "ui-polish"] }
}
```

Members are an explicit list of skill folder names; sdt does not match patterns. A registry with no `packs` key loads as having no packs, so older registries need no migration.

## Semantics

- **Group toggle.** Enabling or disabling a pack sets only its members. Every other skill is left as it is, so several packs can be on at once.
- **`--only` (TUI `o`).** Enables the members of the named pack or packs and disables every other skill. Use it to focus on one kind of task.
- **Derived state.** A pack is `on` when all of its installed members are enabled, `off` when none are, and `mixed` otherwise. A pack with no installed members is `off`. The state is never stored, so toggling a single skill can never leave a pack's state stale.
- **Overlap: the last action wins.** A skill can belong to several packs. Disabling pack A disables a shared skill even if pack B is on, and pack B then shows `mixed`. There is no reference counting, because hidden counts would make the result hard to predict.
- **Missing members.** A member that is not installed is ignored when computing state, skipped with a warning when the pack is applied, and reported by `doctor`. `sync` never removes it, so a skill you reinstall rejoins its packs. Remove it explicitly with `sdt pack remove`.
- **Names.** A pack name must be non-empty, contain no whitespace, and not start with `-`. Pack names and skill names are separate namespaces.
- **Membership changes** (`add`, `remove`, `delete`) never change any skill's enabled state.

## Why commands sync automatically

Earlier versions wrote only to the registry and needed a separate `sdt sync`. Scripts and agents now drive sdt directly, and a two-step update is easy to leave half done, so every command that changes skill state writes the registry and syncs in one step. Saving in the TUI also syncs. `sdt sync` remains for restoring registry choices after something else edits `SKILL.md` files, such as `npx skills update`.
