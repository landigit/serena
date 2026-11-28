# HANDOFF

## Where the port stands

- Goal: standalone Rust MCP server (`astlens`) replacing Serena's semantic layer, packaged as an
  Antigravity plugin. Serena stays installed as the Python reference.
- Gates passed:
  - `rust-anything gate rust/name-path` → **passed** (464/464 recorded cases).
  - `rust-anything gate rust/diagnostics-group-rs` → **passed** (92/92 recorded cases).
- Binary: `rust/astlens` — 5 tools, tree-sitter outline across 14 languages/dialects (including Ruby,
  PHP, C#, Kotlin, Lean 4) with sibling overload `[i]` indexing, lazy per-worktree LSP (including
  `tsgo`/`vtsls`, `ty`/`pyrefly`, `lean --server`, plus `~/.cargo/bin` / `~/.elan/bin` / `~/.local/bin`
  fallback), progress/status-driven `settle()`, disk-truth resync, project-wide `diagnostics`
  (`scope: "project"`, ignore-aware `max_files` cap), and optional `grouped: true` diagnostics
  using `diagnostics-group-rs` + tree-sitter owner symbols.
  `cargo test` 28/28 and `cargo clippy --tests -D warnings` pass.
  Release binary atomically updated at `~/.gemini/config/plugins/astlens/bin/astlens`.
- Commits: `f5ba5641` (astlens + name-path gate), `118e1038` (lsp-rs/repo-dir-sync-rs),
  `599290a0` / `6803f17e` (diagnostics-group-rs), `60537087` (astlens docs), plus current working
  tree adding overload-aware tree-sitter `name_path`, progress-driven `settle()`, and project-wide
  `diagnostics`.

## Next

- Next PR units (`rust/PORT_MAP.md`): `src/interprompt/util/` + `rust/interprompt-rs`,
  `sync.py` + `rust/repo-dir-sync-rs`, `src/serena/generated/`, `src/serena/olb/`.
- Extend the gated `rust/name-path` oracle/port directly once `rust-anything` is on PATH.
- Recorded-but-set-aside units (ADR-001):
  - `rust/repo-dir-sync-rs`: 144 cases recorded (mostly git subprocess errors).
  - `rust/interprompt-rs`: 468 cases recorded via `.venv/bin/python` (268 threw).
- Blocked at record: `src/serena/lsp/` (`rust/lsp-rs`) hits a pre-existing circular import between
  `serena.util.text_utils` and `solidlsp.ls` unless `serena.agent` is imported first; its pure
  `GroupedDiagnostics` logic is already ported and gated in `rust/diagnostics-group-rs`.

## Commands to re-run

```sh
export PATH="$PWD/.venv/bin:$HOME/.cargo/bin:$PATH"
rust-anything gate rust/name-path
rust-anything gate rust/diagnostics-group-rs
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml
rust-anything map . --write
```

## Skill to load next

`rust-anything` (whole-app flow: grill → record → front door → port waves → lift → cut over).
