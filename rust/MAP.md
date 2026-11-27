# astlens port map

Goal: a standalone Rust stdio MCP server (`astlens`) that replaces Serena's
semantic tool surface with five stateless tools, packaged as an Antigravity
plugin. Serena stays installed as reference.

## Units

| Unit | State | Proof |
| --- | --- | --- |
| `rust/name-path` (Serena `NamePathMatcher`) | done | `rust-anything gate` passed; 464/464 recorded cases |
| `rust/diagnostics-group-rs` (`GroupedDiagnostics`) | done | `rust-anything gate` passed; 92/92 recorded cases |
| `rust/astlens` (binary, greenfield) | done v1.3 | `cargo test` 28/28, `cargo clippy --tests -D warnings` |

## Tools

`ast_outline`, `read_symbol`, `find_references`, `edit_symbol`, `diagnostics`.

## Out of scope (v1)

- Serena's memories, dashboard, GUI, JetBrains backend and REPL.
- Idle eviction of LSP children (they die with the MCP server).

## Next

- Continue the `astlens` / `rust-anything` roadmap:
  - `src/serena/lsp/` (`rust/lsp-rs`): `GroupedDiagnostics` is now ported and gated via `rust/diagnostics-group-rs` and wired into `astlens`'s `diagnostics` tool (`grouped: true`). Direct `record` of `src/serena/lsp/` hits a pre-existing circular import (`serena.util.text_utils` <-> `solidlsp.ls` when `serena.agent` isn't imported first).
  - `repo_dir_sync.py` (`rust/repo-dir-sync-rs`): bridged + recorded (144 cases, mostly errors) and set aside.
  - `src/interprompt/` (`rust/interprompt-rs`): bridged + recorded (468 cases via `.venv/bin/python`) and set aside per ADR-001.
- Completed in `astlens` v1.3 (`rust/astlens`):
  - Progress/status-driven `settle()` for all LSPs (`window/workDoneProgress/create`, `$/progress`, `experimental/serverStatus`, `$/lean/fileProgress`, `language/status`) with a 1500 ms fallback when no progress arrives.
  - Project-wide `diagnostics` (`scope: "project"`, `max_files` 1..=500) with ignore-aware file discovery (`.gitignore` / `.ignore`, default ignored dirs, symlink skipping) and shared LSP batch settle + pull-diagnostics fallback.
  - Tree-sitter overload `[i]` indexing in `ts.rs` (`finalize_paths` + `path_components` matching via `name_path::pattern_components`).
- Remaining: re-verify `rust/name-path` / `rust/diagnostics-group-rs` gates with `rust-anything`, and extend `rust/name-path` oracle/port directly once `rust-anything` is available.
- Write/update `rust/HANDOFF.md` at each checkpoint.

## Files

- `DECISIONS.md` — the three accepted decisions.
- `GLOSSARY.md` — shared terms.
- `oracle/name_path.py` — the Python oracle for the gated unit.
- `astlens/` — the binary crate.
- `name-path/` — the gated PyO3 crate.
