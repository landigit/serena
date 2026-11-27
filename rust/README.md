# Rust Port & `astlens` Workspace

This directory contains the Rust rewrite work for Serena:

- **`astlens/`** ([README](astlens/README.md)) — standalone Rust stdio MCP server replacing Serena's semantic tool surface with five stateless tools (`ast_outline`, `read_symbol`, `find_references`, `edit_symbol`, `diagnostics`).
- **`name-path/`** ([README](name-path/README.md)) — PyO3-bridged port of Serena's `NamePathMatcher` (`rust/oracle/name_path.py`), gated at **464/464** cases and compiled directly into `astlens`.
- **`diagnostics-group-rs/`** ([README](diagnostics-group-rs/README.md)) — PyO3-bridged port of `GroupedDiagnostics` (`rust/oracle/diagnostics_group.py`), gated at **92/92** cases and wired into `astlens`'s `diagnostics` tool (`grouped: true`).
- **`interprompt-rs/`** ([README](interprompt-rs/README.md)) — PyO3 bridge + **468** recorded cases for `src/interprompt/` (set aside per ADR-001).
- **`repo-dir-sync-rs/`** ([README](repo-dir-sync-rs/README.md)) — PyO3 bridge + **144** recorded cases for `repo_dir_sync.py` (set aside per ADR-001).
- **`lsp-rs/`** ([README](lsp-rs/README.md)) — PyO3 bridge for `src/serena/lsp/` (blocked at `record` by a pre-existing circular import between `serena.util.text_utils` and `solidlsp.ls`; its pure `GroupedDiagnostics` logic is ported and gated in `diagnostics-group-rs`).

## Documentation

| Document | Purpose |
| --- | --- |
| [DECISIONS.md](DECISIONS.md) | Architectural decision records (ADR-001 to ADR-003) |
| [GLOSSARY.md](GLOSSARY.md) | Shared terms (`name path`, `pattern`, `settle`, `disk truth`) |
| [MAP.md](MAP.md) | `astlens` status and remaining roadmap |
| [PORT_MAP.md](PORT_MAP.md) | Machine-generated `rust-anything` port map |
| [HANDOFF.md](HANDOFF.md) | Checkpoint handoff notes and commands |

## Verification Commands

```sh
export PATH="$PWD/.venv/bin:$HOME/.cargo/bin:$PATH"

# Verify gated rust-anything units
rust-anything gate rust/name-path
rust-anything gate rust/diagnostics-group-rs

# Test, lint, and build astlens
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml

# Regenerate PORT_MAP.md
rust-anything map . --write
```
