# astlens port map

Goal: a standalone Rust stdio MCP server (`astlens`) that replaces Serena's
semantic tool surface with five stateless tools, packaged as an Antigravity
plugin. Serena stays installed as reference.

## Units

| Unit | State | Proof |
| --- | --- | --- |
| `rust/name-path` (Serena `NamePathMatcher`) | done | `rust-anything gate` passed; 464/464 recorded cases |
| `rust/diagnostics-group-rs` (`GroupedDiagnostics`) | done | `rust-anything gate` passed; 92/92 recorded cases |
| `rust/astlens` (binary, greenfield) | done v1.2 | `cargo test` 18/18, `cargo clippy -D warnings`, release build |

## Tools

`ast_outline`, `read_symbol`, `find_references`, `edit_symbol`, `diagnostics`.

## Out of scope (v1)

- Serena's memories, dashboard, GUI, JetBrains backend and REPL.
- Project-wide diagnostics (one file per call).
- Idle eviction of LSP children (they die with the MCP server).
- Overload indexes on symbol components (pattern parsing supports `[i]`; the
  tree-sitter symbol side has no overload metadata yet).

## Next

- Continue the `astlens` / `rust-anything` roadmap:
  - `src/serena/lsp/` (`rust/lsp-rs`): `GroupedDiagnostics` is now ported and gated via `rust/diagnostics-group-rs` and wired into `astlens`'s `diagnostics` tool (`grouped: true`). Direct `record` of `src/serena/lsp/` hits a pre-existing circular import (`serena.util.text_utils` <-> `solidlsp.ls` when `serena.agent` isn't imported first).
  - `repo_dir_sync.py` (`rust/repo-dir-sync-rs`): bridged + recorded (144 cases, mostly errors) and set aside.
  - `src/interprompt/` (`rust/interprompt-rs`): bridged + recorded (468 cases via `.venv/bin/python`) and set aside per ADR-001.
- Use a project-load notification instead of the first-call settle for non-Rust/non-Lean LSPs.
- Add a project-wide `diagnostics` mode with an ignore-aware file cap.
- Extend the gated name-path unit with overload-aware components.
- Write/update `rust/HANDOFF.md` at each checkpoint.

## Files

- `DECISIONS.md` — the three accepted decisions.
- `GLOSSARY.md` — shared terms.
- `oracle/name_path.py` — the Python oracle for the gated unit.
- `astlens/` — the binary crate.
- `name-path/` — the gated PyO3 crate.
