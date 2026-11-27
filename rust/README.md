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

## PR Roadmap (Unit-by-Unit Rust Port)

Track all 24 units from [PORT_MAP.md](PORT_MAP.md) plus `astlens` improvements in priority order (`landigit/serena`, keeping **PR #1–#4** fixed as already opened):

### Already Created (`landigit/serena`)

*(Pre-existing on `main`: `rust/name-path` 464/464 gated, `rust/diagnostics-group-rs` 92/92 gated, `astlens` v1.2 base)*

| PR | Branch / Scope | Purpose |
| --- | --- | --- |
| **PR #1** | `docs/astlens-guide-and-readme` | `astlens` install, Serena comparison, measured latency profile & PR roadmap |
| **PR #2** | `feat/astlens-overload-settle-project-diagnostics` | `astlens` v1.3: overload-aware `name_path`, progress-driven `settle()`, project-wide `diagnostics` |
| **PR #3** | `feat/port-serena-generated-and-olb` | `src/serena/generated/` + `src/serena/olb/` (`rust/serena-generated` & `rust/serena-olb`) |
| **PR #4** | `feat/port-elixir-tools` | `src/solidlsp/language_servers/elixir_tools/` (`rust/elixir-tools`) |

### Tier 1 — Immediate Daily-Use Wins (`astlens` Speed & Language Coverage)

| PR | Scope | Why Now |
| --- | --- | --- |
| **PR #5** | `astlens` `diagnostics()` clean-file latency + idle LSP eviction (`astlens/src/lsp.rs`) | Avoids the `1500 ms` `publishDiagnostics` wait on clean files and frees idle LSP RAM |
| **PR #6** | Extend `rust/name-path` oracle with overload `[i]` metadata + additional languages/LSPs (e.g. Julia) | Removes remaining `astlens` oracle divergence and expands beyond the current 14 languages |

### Tier 2 — Core Utilities, Config & LSP Bridge

| PR | Unit(s) | Lines | Notes |
| --- | --- | ---: | --- |
| **PR #7** | `src/solidlsp/util/` | 773 | Subprocess, cache, zip, metals DB utilities needed by `solidlsp` |
| **PR #8** | `src/serena/util/` | 2,439 | `text_utils.py`, `file_system.py`, `yaml.py`, `git.py` |
| **PR #9** | `src/serena/config/` | 1,664 | `serena_config.py`, `context_mode.py`, `serena_paths.py`, `client_setup.py` |
| **PR #10** | `src/serena/lsp/` (`rust/lsp-rs`) | — | Unblock circular import (`serena.util.text_utils` <-> `solidlsp.ls`) and port remaining LSP backend/diagnostics context |

### Tier 3 — SolidLSP Backbone (40+ Languages, Including Julia)

| PR | Unit(s) | Lines | Notes |
| --- | --- | ---: | --- |
| **PR #11** | `src/solidlsp/lsp_protocol_handler/` | 5,013 | LSP protocol types, constants, requests, server handler |
| **PR #12** | `src/solidlsp/` | 6,466 | Core SolidLSP engine (`ls.py`, `ls_config.py`, `ls_process.py`, `ls_utils.py`, `dependency_provider.py`) |
| **PR #13–#17** | `src/solidlsp/language_servers/` (83 remaining entries) | ~23,600 | Split into ~5 PRs by language family (includes `julia_server.py` & `fatou_language_server.py` for Julia) |

### Tier 4 — Serena Workflows & Runnable Full-Serena Binary

| PR | Unit(s) | Lines | Notes |
| --- | --- | ---: | --- |
| **PR #18** | `src/serena/memories/` | 974 | `.serena/memories/` manager + reference analysis |
| **PR #19** | `src/serena/tools/` | 1,922 | Full Serena tool surface (symbol, file, memory, cmd, config, workflow, query-project, JetBrains, REPL tools) |
| **PR #20** | `src/serena/jetbrains/` | 898 | JetBrains plugin HTTP client & backend (`move`, `inline`, debugger) |
| **PR #21** | `src/serena/repl/api/` | 2,039 | Programmatic REPL API surface |
| **PR #22** | `src/serena/repl/` (`blocked`) | 1,102 | Manual Rust rewrite replacing Python `eval`/dynamic imports |
| **PR #23** | `src/serena/dashboard/` (`blocked`) | 1,121 | Manual Rust HTTP/UI rewrite replacing Flask |
| **PR #24** | `src/serena/` top-level (`blocked`) | 7,757 | `agent.py`, `cli.py`, `mcp.py`, `project.py` wiring all crates into the runnable Rust `serena` binary |

### Tier 5 — Internal / Dev Utilities

| PR | Unit(s) | Lines | Notes |
| --- | --- | ---: | --- |
| **PR #25** | `src/interprompt/` (`rust/interprompt-rs`, 468 cases) + `src/interprompt/util/` | 9 + crate | Prompt codegen & template helpers |
| **PR #26** | `repo_dir_sync.py` (`rust/repo-dir-sync-rs`, 144 cases) + `sync.py` | 5 + crate | Internal subtree sync helpers |
| **PR #27** | `scripts/` | 2,113 | Dev/demo/codegen/profiling scripts (optional) |

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
