# Repo guidelines

- this repo implements `cst`: a minimal CLI to enable/disable skill discovery for agents without deleting shared skill files.

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
  - docs/PLAN.md (keep updated)
