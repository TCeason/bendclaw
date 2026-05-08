# Coding Standards

- Never use `unwrap()` or `expect()`
- Always propagate errors with `?` or handle them explicitly using `match` / `if let`

# Architecture

- **Workspace members**: `src/engine`, `src/app`, `cli/addon`
  - `evotengine` (src/engine) — agent runtime: provider abstraction, agent loop, context, tools, retry
  - `evot` (src/app) — application layer: session, storage, config, server, commands, skills, delivery, search
  - `evotaddon` (cli/addon) — Rust NAPI addon bridging engine/app to the TypeScript CLI
- **CLI**: TypeScript (Bun) in `cli/src/`, renders TUI, handles input, sessions, updates
- `mod.rs` / `lib.rs`: only module declarations, re-exports, and `use` statements — no business logic

# Testing

- All tests go in the crate's `tests/` directory, never inline
- Rust targeted tests: `cargo test -p <crate> <test-name>` or the narrowest relevant `cargo test` command
- TS targeted tests: `cd cli && bun test <test-file>` for the changed area
- Run full `cargo test` or full `cd cli && bun test` only when changes are broad or cross-cutting
- Keep tests explicit and fast; focus on core logic

# Pre-commit

- Before committing, run the relevant targeted tests for the files changed
- Run `make check` before committing only when Rust code, shared build config, or cross-workspace behavior changed
