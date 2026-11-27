# `astlens` — Standalone Rust MCP Server

`astlens` (`rust/astlens`) is a standalone Rust stdio MCP server developed in this repository alongside Python Serena. It replaces Serena's session/REPL lifecycle with five stateless tools backed by tree-sitter and lazy per-worktree LSP clients.

## Design Goals

- **Stateless per-call routing**: Every tool call resolves the nearest workspace root (`tsconfig.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `.git`, etc.) from the target `file` path, avoiding shared global state across parallel subagents or git worktrees.
- **Disk-truth synchronization**: Before querying an LSP, `astlens` checks file `(mtime, len)` on disk and sends `textDocument/didOpen` or `textDocument/didChange` if modified externally.
- **Tree-sitter first**: `ast_outline`, `read_symbol`, and single-symbol `edit_symbol` (`mode: "replace"`) run directly on tree-sitter without waiting for a language server.
- **Gated behavioral parity**: Name-path matching (`rust/name-path`, 464/464 cases) and diagnostics grouping (`rust/diagnostics-group-rs`, 92/92 cases) are ported and verified against Python Serena oracles via `rust-anything`.

## Available Tools

1. **`ast_outline`** — Hierarchical symbol outline (`file`, optional `max_depth`), including nested TSX hooks (`useState`, etc.) and arrow-function handlers.
2. **`read_symbol`** — Exact source of one symbol matched by Serena `name_path` syntax (`foo`, `Class/method`, `/Class/method`, optional `substring: true`, `max_chars`).
3. **`find_references`** — Cross-file `references`, `definition`, or `implementation` via LSP using `name_path` (preferred) or 1-based `line` and `column`.
4. **`edit_symbol`** — Replace one symbol body via tree-sitter (`mode: "replace"`, optional `dry_run: true`) or rename across the workspace via LSP (`mode: "rename"`), returning unified diffs.
5. **`diagnostics`** — Single-file compiler/linter diagnostics filtered by `min_severity` (`error`, `warning`, `info`, `hint`, `all`). Passing `grouped: true` resolves each diagnostic's innermost owning tree-sitter symbol `name_path` and returns Serena's `file -> severity -> name_path -> [...]` structure via `diagnostics-group-rs`.

## Language Support & Server Overrides

`astlens` supports **14** languages/dialects: TypeScript, TSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, PHP, C#, Kotlin, and Lean 4. Each language starts its default LSP lazily on first semantic call (`find_references`, `diagnostics`, or `edit_symbol` with `mode: "rename"`), preferring modern servers (`tsgo` / `vtsls` for TS/JS, `ty` / `pyrefly` / `pyright` for Python, `ruby-lsp`, `intelephense` / `phpactor`, `csharp-ls`, `kotlin-lsp`, `lean --server`) and checking `~/.cargo/bin`, `~/.elan/bin`, and `~/.local/bin` in addition to `PATH`. Any server can be overridden with `ASTLENS_LSP_<LANG>` (e.g., `ASTLENS_LSP_LEAN="lean --server"`).

## Building & Testing

```sh
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml
```

See [rust/README.md](https://github.com/oraios/serena/blob/main/rust/README.md) and [rust/MAP.md](https://github.com/oraios/serena/blob/main/rust/MAP.md) for current port status and roadmap items.
