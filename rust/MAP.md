# astlens port map

Goal: a standalone Rust stdio MCP server (`astlens`) that replaces Serena's
semantic tool surface with five stateless tools, packaged as an Antigravity
plugin. Serena stays installed as reference.

## Units

| Unit | State | Proof |
| --- | --- | --- |
| `rust/name-path` (Serena `NamePathMatcher`) | done | `rust-anything gate` passed; 464/464 recorded cases |
| `rust/astlens` (binary, greenfield) | done v1 | `cargo test` 8/8, `cargo clippy -D warnings`, release build |

## Tools

`ast_outline`, `read_symbol`, `find_references`, `edit_symbol`, `diagnostics`.

## Out of scope (v1)

- Serena's memories, dashboard, GUI, JetBrains backend and REPL.
- Project-wide diagnostics (one file per call).
- Idle eviction of LSP children (they die with the MCP server).
- Overload indexes on symbol components (pattern parsing supports `[i]`; the
  tree-sitter symbol side has no overload metadata yet).

## Next

- Add `tree-sitter` grammars for more languages (Ruby, PHP, C#, Kotlin).
- Use a project-load notification instead of the first-call settle for LSP.
- Add a project-wide `diagnostics` mode with an ignore-aware file cap.
- Extend the gated name-path unit with overload-aware components.

## Files

- `DECISIONS.md` — the three accepted decisions.
- `GLOSSARY.md` — shared terms.
- `oracle/name_path.py` — the Python oracle for the gated unit.
- `astlens/` — the binary crate.
- `name-path/` — the gated PyO3 crate.
