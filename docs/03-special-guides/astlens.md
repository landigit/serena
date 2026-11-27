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

`astlens` supports **14** languages/dialects: TypeScript, TSX, JavaScript, Rust, Python, Go, Java, C, C++, Ruby, PHP, C#, Kotlin, and Lean 4. *(Julia is supported in Python Serena via `julia_server.py` / `fatou_language_server.py`, not yet in `astlens`; see the [PR Roadmap](../../rust/README.md#pr-roadmap-unit-by-unit-rust-port).)* Each language starts its default LSP lazily on first semantic call (`find_references`, `diagnostics`, or `edit_symbol` with `mode: "rename"`), preferring modern servers (`tsgo` / `vtsls` for TS/JS, `ty` / `pyrefly` / `pyright` for Python, `ruby-lsp`, `intelephense` / `phpactor`, `csharp-ls`, `kotlin-lsp`, `lean --server`) and checking `~/.cargo/bin`, `~/.elan/bin`, and `~/.local/bin` in addition to `PATH`. Any server can be overridden with `ASTLENS_LSP_<LANG>` (e.g., `ASTLENS_LSP_LEAN="lean --server"`).

## Building & Installing

### Build & Test

```sh
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml
```

This outputs a single release binary at `rust/astlens/target/release/astlens` (`lto = true`, `codegen-units = 1`, `strip = true`). Tree-sitter tools (`ast_outline`, `read_symbol`, `edit_symbol` with `mode: "replace"`) work with no external tools; LSP tools (`find_references`, `diagnostics`, `edit_symbol` with `mode: "rename"`) require an appropriate language server on `PATH`, in `~/.cargo/bin`, `~/.elan/bin`, `~/.local/bin`, or configured via `ASTLENS_LSP_<LANG>`.

### Connecting an MCP Client

- **Generic stdio MCP client (Claude Code, Codex, Cursor, etc.)**: point your client's stdio MCP configuration directly at `/absolute/path/to/astlens` (no Python or `uv` environment needed). For Claude Code:
  ```sh
  claude mcp add --scope user astlens-mcp -- /absolute/path/to/astlens
  ```
- **Antigravity plugin** (`~/.gemini/config/plugins/astlens/`): bundle `plugin.json` (`"name": "astlens"`), `mcp_config.json`, `bin/astlens`, and an optional lazy `skills/astlens/SKILL.md` (no `rules/AGENTS.md`, no hooks, no `initial_instructions`). To update an already-running binary safely:
  ```sh
  cp rust/astlens/target/release/astlens ~/.gemini/config/plugins/astlens/bin/astlens.new
  mv -f ~/.gemini/config/plugins/astlens/bin/astlens.new ~/.gemini/config/plugins/astlens/bin/astlens
  ```

## `astlens` vs. Original Serena

`astlens` complements the host agent's built-in `read_file`/`edit_file`/`run_command` tools instead of replacing them:

- **Advantages over original Python Serena**: zero onboarding (`initial_instructions` / `activate_project` / REPL), no `FORBIDDEN` rules conflicting with native file tools, stateless per-`file` `(language, workspace_root)` routing that keeps parallel subagents and git worktrees isolated, automatic `(mtime, len)` disk-truth resync before every LSP call, nested TSX hook/handler extraction via tree-sitter without starting `tsserver`, and no `.serena/` workspace files.
- **Where original Serena has broader scope**: original Serena supports **40+ languages** (vs. `astlens`'s **14**), the JetBrains plugin backend (`move`, `inline`, `propagate deletions`, interactive debugging), project-wide diagnostics, persistent `.serena/` memories, dashboard/GUI, and symbol overload metadata.

## Performance, Accuracy & Efficiency

> **Measured stdio baseline (this repo, v1.3 release binary with `rust-analyzer 1.100.0` & `ty 0.0.112`; no side-by-side Python Serena run recorded):** `initialize` **1.1 ms**, `read_symbol` **2.1 ms**, `ast_outline` **7.6–18.4 ms**, warm `find_references` **18.9 ms**, cold `diagnostics` **5.9 s** (`rust-analyzer`) / **1.7 s** (`ty`), warm clean-file `diagnostics` **~1.5 s** (due to the `1500 ms` `publishDiagnostics` wait, targeted in PR #5).

- **Speed**: Structural tools (`ast_outline`, `read_symbol`, `edit_symbol` replace) run purely on in-process tree-sitter without spawning an LSP. Semantic tools spawn an LSP lazily per `(language, workspace_root)` and reuse it for subsequent calls; the first semantic call per client performs a one-time settle (up to `8s` readiness polling for `rust-analyzer`/Lean, `2000 ms` for other LSPs).
- **Accuracy**: `rust/name-path` (**464/464** cases) and `rust/diagnostics-group-rs` (**92/92** cases) pass `rust-anything gate` against Python Serena oracles, and disk-truth `(mtime, len)` resync prevents stale buffers after external edits. Current v1 limits: overload `[i]` syntax is parsed in patterns without tree-sitter overload metadata yet, `diagnostics` is single-file per call, and idle LSP children stay alive until the MCP server exits.
- **Efficiency**: Five compact tool schemas with no always-on rules budget overhead, plus built-in output bounds (`read_symbol` `6,000`-char default cap configurable `500`–`20,000`, `ast_outline` `20 KB` top-level fallback, `find_references` `200`-result default cap up to `1,000`, `diagnostics` `500`-item cap, and compact unified diffs from `edit_symbol`).

See [rust/README.md](../../rust/README.md) (including the renumbered [PR Roadmap](../../rust/README.md#pr-roadmap-unit-by-unit-rust-port)) and [rust/MAP.md](../../rust/MAP.md) for current port status and roadmap items.
