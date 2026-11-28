# `codemode` Design Specification

Date: 2026-11-28
Status: Draft (awaiting user review)

## 1. Intent & Why

Codebases and long agent sessions suffer from two separate context leaks:

1. **Code exploration & editing** wastes tokens when agents read whole files and run fragile text edits instead of operating on symbols and LSP relations (what `rust/astlens` solves).
2. **Tool outputs, shell execution, and session compaction** waste tokens when raw command stdout/stderr, logs, and search results flood the conversation window and get lost across compactions (what `context-mode` solves).

`codemode` fuses both halves into a **single publishable Rust binary** (`codemode`, repository `codemode`) that owns the entire loop:

`understand code structurally -> execute in a sandbox -> index bulk output into local FTS5 -> retrieve bounded slices -> preserve state across compactions`

## 2. Goals, Non-Goals, and Anti-Goals

### Goals
- **One publishable binary (`codemode`)** licensed **GPL-3.0-or-later**, reusing `rust/astlens`, `rust/name-path`, and `rust/diagnostics-group-rs`, and reimplementing `context-mode` capabilities **clean-room** from documented behavior (zero Elastic-2.0 source code).
- **Unified `code_*` MCP tool surface (~12 tools)**: `code_outline`, `code_read`, `code_references`, `code_diagnostics`, `code_edit`, `code_execute`, `code_index`, `code_search`, `code_resume`, `code_memory`, `code_batch`, `code_admin`.
- **All 40+ Serena languages** (`src/solidlsp/ls_config.py` `LanguageServerId`) using `tree-sitter 0.27` with uniform `arborium-*` grammar crates and automatic LSP `textDocument/documentSymbol` fallback for niche languages not bundled by `arborium`.
- **Host adapters in rollout order**: **Antigravity + OpenCode** first (v1), **Claude Code** next (v1.1), **Codex** future (v2).
- **Clean new native format**: `.codemode/config.toml` + `.codemode/memories/*.md` in projects, and `$XDG_DATA_HOME/codemode/<project-hash>/context.db` for local SQLite/FTS5 state (no `.serena/` legacy compatibility burden).
- **Fast CLI dispatch**: `codemode serve` (MCP stdio), `codemode hook <event>` (lightweight no-Tokio-spinup path targeting `<10 ms`), `codemode index`, `codemode doctor`, `codemode stats`, `codemode purge`, `codemode upgrade`.

### Non-Goals (v1)
- No Flask dashboard, tray app, pywebview GUI, JetBrains IDE plugin client, `agno` integration, or Python `eval` REPL.
- No `.serena/project.yml` or legacy `.serena/memories/` auto-migration in v1 (clean new format).
- No Elastic-2.0 source reuse, hosted gateway service, or Node-only sandbox shims.
- No Windows native sandbox path in v1 (Linux/macOS first; Windows path deferred unless needed).

### Anti-Goals
- Never expose overlapping generic `read_file` / `search_text` / `list_dir` tools that fight the host agent's native built-ins.
- Never ship always-on `rules/AGENTS.md` files that mark native host tools `FORBIDDEN` or burn prompt rule budgets.
- Never allow stale LSP buffers after native host edits (`(mtime, len)` disk-truth check before every LSP call).
- Never allow shared global project state to collide across parallel subagents or git worktrees.

## 3. Constraints

- **License**: `GPL-3.0-or-later` only. All `context-mode` behaviors (`store`, `retrieval`, `sandbox`, `policy`, `hooks`) must be clean-room Rust implementations written from specifications and conformance tests.
- **Rust toolchain & crates**: Rust edition `2024`, `tree-sitter` `0.27.x` with uniform `arborium-*` (`2.18.x`) grammar crates, `rusqlite` with bundled SQLite + FTS5, `dashmap`, `similar`, `toml`, `serde`/`serde_json`.
- **Hook latency budget**: `codemode hook <event>` must avoid starting Tokio or spawning LSPs so pre/post-tool hooks stay within `<10 ms`.
- **Stateless per-call routing**: Every tool call derives `(language, workspace_root)` from the target path, never from mutable global `active_project` state.
- **Bounded inline outputs**: Every tool response stays inline (`<= 6-8 KB` default) and spills oversized output into SQLite FTS5 with a `source_id` handle.

## 4. Approaches Considered (and Calls Made)

### Approach A: Repository & Workspace Layout
- **Option 1 (Chosen)**: Seed `codemode` as a **new Cargo workspace in this repo (`crates/*`) during bootstrap waves**, then extract cleanly to standalone `landigit/codemode` once W1-W2 pass conformance.
  - *Why*: Immediate reuse of `rust/astlens`, `rust/name-path`, `rust/diagnostics-group-rs`, and Python `rust-anything` oracles without breaking `landigit/serena` links mid-flight.
- **Option 2**: Create an empty `landigit/codemode` repo immediately and copy crates over.
  - *Tradeoff*: Loses direct side-by-side access to `rust/oracle/*` and `src/` during oracle extraction.

### Approach B: `code_execute` Scope (Sandboxed Exec vs. Typed Code Mode)
- **Option 1 (Chosen)**: Ship **sandboxed runtime/shell execution + output spilling in v1 (W2)**, and add **typed Code Mode (`codemode` SDK bindings callable from sandboxed scripts in `code_execute`) in W5 / v1.1**.
  - *Why*: Lets `codemode` ship and replace both `astlens` and `context-mode` immediately without blocking v1 on cross-language IPC bindings for JS/Python scripts.
- **Option 2**: Require full typed Code Mode IPC in W2 before any `code_execute` works.
  - *Tradeoff*: Delays usable execution/spilling and couples sandbox security with RPC socket/pipe plumbing too early.

### Approach C: 40+ Language Support Strategy
- **Option 1 (Chosen)**: **Hybrid structural engine** — compiled-in `tree-sitter 0.27`-compatible grammars for tier-1/available languages + **automatic LSP `textDocument/documentSymbol` fallback** for any of the 40+ `LanguageServerId` languages lacking a compatible `tree-sitter` crate, plus `.codemode/config.toml` disambiguation for overlapping extensions (`YAML` vs `ANSIBLE`, `TS` vs `DENO` vs `ANGULAR`).
  - *Why*: Guarantees full 40+ language coverage immediately without waiting on upstream `tree-sitter-*` crate updates.
- **Option 2**: Strictly tree-sitter for `code_outline` / `code_read` / `code_edit`.
  - *Tradeoff*: Leaves ~20-30 languages unsupported until someone writes/updates Rust bindings for every grammar.

## 5. Architecture & Decided Design

### Workspace & Binary Layout
One binary `codemode` with fast `argv[1]` subcommand dispatch before initializing Tokio or LSP managers:

```text
codemode/
├── Cargo.toml
└── crates/
    ├── core/        # .codemode/config.toml, XDG paths, project-hash, ignore rules, LanguageRegistry (40+)
    ├── ts/          # tree-sitter 0.27 structural parser, name-path matching, chunk-shape splitter
    ├── lsp/         # lazy per-(lang, root) LSP multiplexer, disk-truth sync, settle, documentSymbol fallback, diagnostics
    ├── store/       # rusqlite (bundled FTS5): sources, chunks, trigrams, event journal, memory index
    ├── retrieval/   # BM25 ranking, Porter/trigram search, bounded snippets, compaction summary builder
    ├── sandbox/     # runtime detection, env scrubbing, process-group kill, timeouts, byte caps, spilled handles
    ├── policy/      # bash/sh/PowerShell command lexer & unwrapper, read-deny/read-cap decisions
    ├── hooks/       # fast (<10ms) event handlers (pre-tool, post-tool index, session-start, pre-compact)
    ├── memories/    # .codemode/memories/*.md CRUD, mem: reference graph, rename propagation
    ├── mcp/         # JSON-RPC 2.0 stdio server, 12 code_* schemas, bounded reply + spill-handle formatter
    └── cli/         # binary entry: serve | hook | index | doctor | stats | purge | upgrade
```

### Unified `code_*` Tool Contract (12 Tools)
| Tool | Backend | Role |
| :--- | :--- | :--- |
| `code_outline` | `ts` (fallback: `lsp` `documentSymbol`) | Hierarchical symbol outline of a file (`max_depth`, bounded `20 KB`) |
| `code_read` | `ts` + `name-path` (fallback: `lsp` range) | Exact symbol body by `name_path` (`6 KB` default cap + `source_id` spill on overflow) |
| `code_references` | `lsp` | Cross-file `references`, `definition`, `implementation` (`max_results` + spill) |
| `code_diagnostics` | `lsp` + `diagnostics-group-rs` | File or project diagnostics (`grouped: true`, clean-file fast path, no fixed 1.5s sleep) |
| `code_edit` | `ts` / `lsp` | Replace symbol body (`mode: "replace"`, `dry_run`) or project-wide `rename` (`mode: "rename"`), returning unified diff |
| `code_execute` | `sandbox` + `store` | Run code/commands in scrubbed sandbox (foreground or background job); stdout/stderr capped inline & auto-indexed (`source_id`); W5 adds typed `code_*` SDK |
| `code_index` | `core` + `ts` + `store` | Index files/dirs (gitignore-aware), HTML→text, or raw text into project FTS5 (`shape` + line-unit chunking with source labels) |
| `code_search` | `retrieval` + `ts` + `memories` | Unified BM25/trigram/symbol search across indexed outputs, repo files, journal, and memories |
| `code_resume` | `store` + `retrieval` | Compaction recovery summary (last task, changed files, open errors, key decisions, search terms) |
| `code_memory` | `memories` + `store` | Read/write/list/delete/rename `.codemode/memories/*.md` with `mem:` link graph propagation |
| `code_batch` | `mcp` | Execute multiple `code_*` calls in parallel, returning one bounded summary |
| `code_admin` | `store` + `lsp` | `stats` (bytes/tokens saved), `doctor`, `purge`, `upgrade`, `lsp_status` |

### New Native Format
- **Project config (optional)**: `.codemode/config.toml`
  - `languages = ["typescript", "python"]` (disambiguates overlapping extensions like `deno`/`angular`/`ansible`)
  - `[lsp_overrides]` (equivalent to `ASTLENS_LSP_<LANG>`)
  - `[ignore]` extra ignore patterns beyond `.gitignore`
  - `[policy]` sandbox/read-cap overrides
- **Project memories**: `.codemode/memories/<slug>.md` (git-reviewable plain markdown, indexed into `context.db` on edit/startup).
- **Local runtime DB**: `$XDG_DATA_HOME/codemode/<canonical-root-blake3>/context.db` (never pollutes git status).

### Host Rollout Packaging
1. **Antigravity** (Wave 1): Plugin bundle (`plugin.json` `"name": "codemode"`, `mcp_config.json`, `bin/codemode serve`, `hooks.json` calling `codemode hook <event>`, lazy `skills/codemode/SKILL.md`).
2. **OpenCode** (Wave 1): `opencode.json` MCP server entry + OpenCode TypeScript/plugin thin hook shim delegating to `codemode hook <event>`.
3. **Claude Code** (Wave 2): `.claude-plugin/plugin.json` + `.mcp.json` + `hooks/hooks.json` delegating to `codemode hook <event>`.
4. **Codex** (Future): `.codex/config.toml` MCP entry + AGENTS/skill adapter.

## 6. Left to the Builder

- Exact crate versions for compatible `tree-sitter-*` / `arborium-*` grammars audited per language in W1.
- Exact SQLite schema migration numbering and FTS5 tokenizer tuning (`porter unicode61` + trigram table) in W2.
- Clean-room oracle test corpus exported from `context-mode` behavior docs/public test cases for `policy`, `retrieval`, and `hooks` in W2-W3.
- Exact extraction script/PR to split `codemode` into standalone `landigit/codemode` repository at W6.
