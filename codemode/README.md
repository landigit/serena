# `codemode`

Single **GPL-3.0-or-later** Rust binary combining 40+-language structural/LSP code intelligence (uniform `arborium-*` tree-sitter grammars on `tree-sitter 0.27` with automatic LSP `textDocument/documentSymbol` fallback) and clean-room context discipline (scrubbed PGID-isolated sandbox, local SQLite FTS5 + trigram spill store, `<10 ms` synchronous hooks, `.codemode/memories/*.md`, and compaction recovery).

## Build & Install

```bash
cargo build --release --manifest-path codemode/Cargo.toml
./codemode/packaging/install.sh
```

## Subcommands

- `codemode serve` — JSON-RPC 2.0 stdio MCP server exposing 12 `code_*` tools (`code_outline`, `code_read`, `code_references`, `code_diagnostics`, `code_edit`, `code_execute`, `code_index`, `code_search`, `code_resume`, `code_memory`, `code_batch`, `code_admin`).
- `codemode hook <pre-tool|post-tool|session-start|pre-compact>` — Synchronous `<10 ms` hook handler (no Tokio/LSP spinup).
- `codemode index [PATH]` — Gitignore-aware indexing into `$XDG_DATA_HOME/codemode/<project-hash>/context.db`.
- `codemode doctor | stats | purge | upgrade` — Operational diagnostics and maintenance.

## Project Configuration (`.codemode/config.toml`)

```toml
languages = ["deno", "python"]

[lsp_overrides]
python = "ty server"

[ignore]
extra_patterns = ["*.generated.ts", "dist/"]

[policy]
inline_cap_bytes = 6144
read_cap_bytes = 8192
deny_bulk_reads = true
```

## Host Packaging

- **Antigravity**: `codemode/packaging/antigravity/` (`plugin.json`, `mcp_config.json`, `hooks.json`, `skills/codemode/SKILL.md`)
- **OpenCode**: `codemode/packaging/opencode/` (`opencode.json`, `plugins/codemode-hooks.ts`)
- **Claude Code**: `codemode/packaging/claude-code/` (`.claude-plugin/plugin.json`, `.mcp.json`, `hooks/hooks.json`)
- **Codex**: `codemode/packaging/codex/config.toml`
