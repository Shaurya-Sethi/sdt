# Repo guidelines

- this repo implements `sdt` (Skill Discovery Toggle): an agent-agnostic CLI to enable/disable skill discovery without deleting shared skill files. Agent support depends on honoring `disable-model-invocation`.

- use `cargo` for building, testing, and linting.
- follow normal rust style, keep codebase `rustfmt` -clean.
- treat documentation as part of the code: add or update concise Rust docs (`//!`/`///`) for modules and significant items, explaining purpose, invariants, side effects, and errors without restating signatures.

- minimize software-entropy by designing with intent.
- favor deep modules: relatively few, large modules with simple interfaces that hide complexity, rather than shallow modules with complex, fragmented interfaces.
- codebase should be easy to change and testable.
- always prefer simplicity and keep implementations minimal and direct; never over-engineer, defensively handle hypothetical edge-cases, or make complex abstractions.

- follow pragmatic test driven development using the `tdd` skill.
- always read documentation before changing code or implementing anything new.
  - official upstream docs — use web search or browse local source.
  - current repo code, tests, public interfaces.
  - relevant architectural decision records or user-facing behavior docs in docs/, when present.
- keep short user-facing documentation in README.md; use docs/ only for architectural decisions or behavior topics that need more detail, linked from README.md.
