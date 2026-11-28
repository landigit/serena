# `astlens`

A standalone, stateless Rust stdio MCP server replacing Serena's semantic tool surface with tree-sitter structural parsing and a lazy per-worktree LSP multiplexer.

## Tools

| Tool | Engine | Purpose |
| --- | --- | --- |
| `ast_outline` | Tree-sitter | Hierarchical symbol outline of a file (including nested TSX hooks/handlers) |
| `read_symbol` | Tree-sitter + `name-path` | Exact source of one symbol by Serena name path (capped at 6,000 chars by default) |
| `find_references` | Lazy LSP | Cross-file `references`, `definition`, or `implementation` by `name_path` or 1-based `(line, column)` |
| `edit_symbol` | Tree-sitter / Lazy LSP | Replace one symbol body (`mode: "replace"`, supports `dry_run`) or rename across project (`mode: "rename"`) |
| `diagnostics` | Lazy LSP + `diagnostics-group-rs` | Single-file diagnostics filtered by `min_severity`; pass `grouped: true` to group by severity and owning tree-sitter symbol `name_path` |

Every call derives `(language, workspace_root)` from the target file path and re-syncs disk truth (`didOpen` / `didChange` on `(mtime, len)` change) before querying LSPs, keeping parallel worktrees isolated and edits from external tools consistent.

## Supported Languages & LSPs

| Language | Extensions | Default Language Server | Override Env Var |
| --- | --- | --- | --- |
| TypeScript | `.ts`, `.mts`, `.cts` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_TYPESCRIPT` |
| TSX | `.tsx` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_TSX` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_JAVASCRIPT` |
| Rust | `.rs` | `rust-analyzer` | `ASTLENS_LSP_RUST` |
| Python | `.py`, `.pyi` | `ty server` / `pyrefly lsp` / `pyright-langserver --stdio` / `basedpyright-langserver --stdio` | `ASTLENS_LSP_PYTHON` |
| Go | `.go` | `gopls` | `ASTLENS_LSP_GO` |
| Java | `.java` | `jdtls` | `ASTLENS_LSP_JAVA` |
| C | `.c`, `.h` | `clangd` | `ASTLENS_LSP_C` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hh`, `.hxx` | `clangd` | `ASTLENS_LSP_CPP` |
| Ruby | `.rb`, `.rake`, `.gemspec` | `ruby-lsp` / `solargraph stdio` | `ASTLENS_LSP_RUBY` |
| PHP | `.php`, `.phtml` | `intelephense --stdio` / `phpactor language-server` | `ASTLENS_LSP_PHP` |
| C# | `.cs` | `csharp-ls --stdio` / `OmniSharp -lsp` | `ASTLENS_LSP_CSHARP` |
| Kotlin | `.kt`, `.kts` | `kotlin-lsp --stdio` / `kotlin-language-server --stdio` | `ASTLENS_LSP_KOTLIN` |
| Lean 4 | `.lean` | `lean --server` | `ASTLENS_LSP_LEAN` |

> **Note on Julia:** Julia (`julia_server.py` / `fatou_language_server.py`) is supported in Python Serena's `solidlsp`, not yet in `astlens` (planned under **PR #6** / Tier 3 in [rust/README.md](../README.md#pr-roadmap-unit-by-unit-rust-port)).

## Shared Gated Units

`astlens` compiles in two gated `rust-anything` port modules directly without a PyO3 link:

- `../name-path/src/port.rs` (`rust/name-path`, 464/464 recorded cases)
- `../diagnostics-group-rs/src/port.rs` (`rust/diagnostics-group-rs`, 92/92 recorded cases)

## Build, Test & Install

### Prerequisites

- **Rust toolchain** (edition `2024`) to build `astlens` from source.
- **Language servers** (only needed for `find_references`, `diagnostics`, and `edit_symbol` with `mode: "rename"`): `astlens` checks `PATH`, `~/.cargo/bin`, `~/.elan/bin`, `~/.local/bin`, or `ASTLENS_LSP_<LANG>`. Tree-sitter tools (`ast_outline`, `read_symbol`, `edit_symbol` with `mode: "replace"`) run without any language server installed.

### Build & Verify

```sh
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml
```

This builds a single release binary at `rust/astlens/target/release/astlens` (`lto = true`, `codegen-units = 1`, `strip = true`).

### Fresh Install: Standalone Stdio MCP Client

Point any stdio MCP client (Claude Code, Codex, Cursor, etc.) at the compiled binary — no Python/`uv` environment is required:

```json
{
  "mcpServers": {
    "astlens-mcp": {
      "command": "/absolute/path/to/astlens"
    }
  }
}
```

For Claude Code:

```sh
claude mcp add --scope user astlens-mcp -- /absolute/path/to/astlens
```

### Fresh Install & Atomic Update: Antigravity Plugin

Per [ADR-003](../DECISIONS.md), package `astlens` outside the workspace at `~/.gemini/config/plugins/astlens/` with:

```text
~/.gemini/config/plugins/astlens/
├── plugin.json          # unique "name": "astlens" (avoids colliding with serena)
├── mcp_config.json      # stdio command pointing to bin/astlens
├── bin/astlens          # compiled release binary
└── skills/astlens/SKILL.md  # optional lazy skill (no rules/AGENTS.md, no hooks)
```

To upgrade an already-running Antigravity plugin binary without hitting `ETXTBSY`:

```sh
cp rust/astlens/target/release/astlens ~/.gemini/config/plugins/astlens/bin/astlens.new
mv -f ~/.gemini/config/plugins/astlens/bin/astlens.new ~/.gemini/config/plugins/astlens/bin/astlens
```

## `astlens` vs. Original Python Serena

| Aspect | `astlens` (Rust) | Original Serena (Python) |
| --- | --- | --- |
| Tool surface | 5 stateless semantic tools (`ast_outline`, `read_symbol`, `find_references`, `edit_symbol`, `diagnostics`) | Full toolset + REPL, onboarding, memories, dashboard/GUI |
| Session & rules | No `initial_instructions`, no `activate_project`, no `FORBIDDEN` overrides on native `read_file`/`edit_file` | Project activation + context/mode prompts |
| Parallel subagents & worktrees | Stateless per-call `(language, workspace_root)` routing via `DashMap` | Single active project per server instance |
| Disk sync | Checks `(mtime, len)` before every LSP call and sends `didOpen`/`didChange` | Relies on Serena's own buffer/editing lifecycle |
| Structural outline | In-process tree-sitter (captures nested TSX hooks & arrow handlers) | Language server or JetBrains plugin |
| Languages | 14 languages/dialects | 40+ languages (LSP) or JetBrains-supported languages |
| Advanced refactoring & backends | Symbol body replace + LSP rename | Move, inline, propagate deletions, interactive debugging (JetBrains plugin) |
| Workspace artifacts | None (no `.serena/` directory written) | `.serena/` project config & memories |

## Speed, Accuracy & Efficiency

> **Measured stdio baseline (on this repo, v1.3 release binary with `rust-analyzer 1.100.0` & `ty 0.0.112`; no side-by-side Python Serena run recorded):**
> - `initialize`: **1.1 ms**
> - `read_symbol` (`ElixirTools/new`): **2.1 ms**
> - `ast_outline` (`rust/astlens/src/lsp.rs`, ~34 KB): **7.6 ms**; (`src/serena/agent.py`, ~85 KB): **18.4 ms**
> - `find_references` warm (`rust-analyzer`): **18.9 ms**
> - `diagnostics` cold (`rust-analyzer` / `ty`): **5,923.9 ms** / **1,673.2 ms**; warm clean/unchanged file: **1,508.5 ms** / **1,514.7 ms** (due to the `1500 ms` `publishDiagnostics` wait in `lsp.rs`, targeted for optimization in PR #5)

- **Speed**:
  - `ast_outline`, `read_symbol`, and `edit_symbol` (`mode: "replace"`) execute purely on in-process tree-sitter with zero LSP startup.
  - Semantic calls (`find_references`, `diagnostics`, `edit_symbol` rename) spawn the language server lazily on first use per `(language, workspace_root)` and reuse it for subsequent calls (no idle eviction in v1; children exit with the MCP server).
  - On the first semantic call per LSP client, `LspClient::settle` waits up to `8s` for `rust-analyzer`/Lean readiness (100 ms polling with 500 ms fallback) or sleeps `2000 ms` for other LSPs before querying.
- **Accuracy**:
  - `NamePathMatcher` (`rust/name-path`) is gated at **464/464** recorded cases against the Python Serena oracle; `GroupedDiagnostics` (`rust/diagnostics-group-rs`) is gated at **92/92** cases and paired with tree-sitter smallest-byte-span owner lookup when `diagnostics` is called with `grouped: true`.
  - Pre-query `(mtime, len)` disk-truth synchronization ensures external edits from native tools or formatters do not produce stale line numbers or stale diagnostics.
  - **v1 scope limits**: pattern parsing accepts `[i]` overload indexes without tree-sitter overload metadata yet; `diagnostics` operates on one file per call; very large repositories on non-Rust/non-Lean LSPs may still be indexing after the initial `2000 ms` settle.
- **Token & context efficiency**:
  - Minimal 5-tool schema with no `rules/AGENTS.md` prompt budget cost and no onboarding round-trip.
  - Bounded responses keep output inline: `read_symbol` defaults to `6,000` chars (`500`–`20,000`), `ast_outline` falls back to top-level symbols if output exceeds `20 KB`, `find_references` caps at `200` results by default (max `1,000`), `diagnostics` truncates at `500` items, and `edit_symbol` returns compact unified diffs.
