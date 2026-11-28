# `codemode` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and publish **`codemode`**, a single GPL-3.0-or-later Rust binary combining `astlens`'s 40+-language structural/LSP code intelligence (`code_outline`, `code_read`, `code_references`, `code_diagnostics`, `code_edit`) with clean-room `context-mode` context discipline (`code_execute`, `code_index`, `code_search`, `code_resume`, `code_memory`, `code_batch`, `code_admin`), packaged for Antigravity + OpenCode first, then Claude Code, then Codex.

**Architecture:** One Cargo workspace (`crates/{core,ts,lsp,store,retrieval,sandbox,policy,hooks,memories,mcp,cli}`) compiling to one binary `codemode` that switches on `argv[1]` (`serve` for Tokio/MCP stdio, `hook` for `<10 ms` synchronous event checks, plus `index`/`doctor`/`stats`/`purge`/`upgrade`). All state lives in `.codemode/` (optional TOML config + git-tracked markdown memories) and `$XDG_DATA_HOME/codemode/<project-hash>/context.db` (SQLite FTS5), while pure algorithmic modules (`name-path`, `diagnostics-group`, `policy` shell lexer, `memories` reference graph, `retrieval` chunking/BM25) are parity-gated via `rust-anything` or deterministic golden cases.

**Tech Stack:** Rust (edition `2024`, GPL-3.0-or-later), `tree-sitter` `0.27.x` (+ `arborium-*` / latest compatible grammars), `rusqlite` (bundled SQLite + FTS5), `dashmap`, `similar`, `toml`, `serde`/`serde_json`, `rust-anything` (for gated oracle units).

**Spec:** [docs/superpowers/specs/2026-11-28-codemode-design.md](../specs/2026-11-28-codemode-design.md)

## Global Constraints

- License: **GPL-3.0-or-later** only; clean-room Rust reimplementation of `context-mode` behaviors from documented specs (never copy Elastic-2.0 source code).
- Brand, repo, binary, and tools: **`codemode`**, tools strictly named **`code_*`** (`code_outline`, `code_read`, `code_references`, `code_diagnostics`, `code_edit`, `code_execute`, `code_index`, `code_search`, `code_resume`, `code_memory`, `code_batch`, `code_admin`).
- Languages: Cover **all 40+ languages** from `src/solidlsp/ls_config.py` (`LanguageServerId`) using latest compatible `tree-sitter` crates where available + automatic LSP `textDocument/documentSymbol` fallback where no compatible grammar crate exists.
- Format: **Clean new format** (`.codemode/config.toml`, `.codemode/memories/*.md`, `$XDG_DATA_HOME/codemode/<hash>/context.db`); no `.serena/` legacy compatibility required.
- Hosts priority: **Antigravity + OpenCode** first, then **Claude Code**, **Codex** future.
- Hook latency: `codemode hook <event>` must run synchronously without spawning Tokio or LSPs (`<10 ms` target).
- Bounded outputs: All tool replies stay inline (`<= 6-8 KB` default) and spill large payloads to FTS5 with a `source_id` handle.
- Git author: Always commit as `Zafar Sathiq S <fairsciencenow@gmail.com>`.

## Review Focus

- **Overlapping file extensions in 40+ languages** (`.ts` for TypeScript vs Deno vs Angular; `.yml` for YAML vs Ansible; `.html` for HTML vs Angular): tests in Task 1 and Task 3 must verify `.codemode/config.toml` `languages` overrides win deterministically over extension defaults.
- **Clean-file `diagnostics()` latency**: tests in Task 3 must prove an unchanged file with 0 diagnostics returns promptly when `publishDiagnostics` / `textDocument/diagnostic` confirms emptiness rather than sleeping `1500 ms`.
- **Command-unwrapping escapes in `policy`**: tests in Task 6 must catch `env`, `bash -c`, `sh -c`, pipelines, subshells, and `cat`/`head`/`tail` targeting project source files and enforce read-deny/read-cap rules cleanly.
- **Spill-handle FTS5 integrity & compaction survival**: tests in Task 4, Task 5, and Task 8 must verify oversized `code_execute` / `code_references` outputs persist into `context.db` across process restarts and remain queryable via `code_search(source_id=...)`.
- **Parallel subagent / worktree isolation**: tests in Task 1, Task 3, and Task 4 must prove concurrent calls across two git worktrees resolve separate `(language, root)` LSP clients and separate `<project-hash>` databases without lock/state collision.

---

### Task 1: Workspace Scaffold & `codemode-core` (W0)

**Files:**
- Create: `codemode/Cargo.toml`
- Create: `codemode/LICENSE`
- Create: `codemode/crates/core/Cargo.toml`
- Create: `codemode/crates/core/src/lib.rs`
- Create: `codemode/crates/core/src/config.rs`
- Create: `codemode/crates/core/src/paths.rs`
- Create: `codemode/crates/core/src/languages.rs`
- Test: `codemode/crates/core/tests/config_and_languages.rs`

**Interfaces:**
- Produces:
  - `codemode_core::config::{ProjectConfig, load_project_config}`
  - `codemode_core::paths::{project_hash, data_dir, context_db_path, memories_dir}`
  - `codemode_core::languages::{LanguageId, LanguageSpec, resolve_language, all_languages}`

- [ ] **Step 1: Write failing tests for config loading, XDG hashing, and 40+ language registry**
  In `codemode/crates/core/tests/config_and_languages.rs`, assert:
  - `all_languages()` covers every non-alias language in `src/solidlsp/ls_config.py` `LanguageServerId` (>= 40 distinct languages including `julia`, `dart`, `elixir`, `zig`, `haskell`, `scala`, `vue`, `svelte`, `astro`, `bash`, `powershell`).
  - `resolve_language(Path::new("script.ts"), Some(&ProjectConfig { languages: vec!["deno".into()], .. }))` returns `LanguageId::Deno` instead of `TypeScript`.
  - `project_hash(root)` is deterministic, 16+ hex chars, and distinct across two git worktree paths.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-core`
  Expected: FAIL (workspace/crate does not exist yet).

- [ ] **Step 3: Create Cargo workspace, GPL-3.0-or-later license, and implement `codemode-core`**
  Copy GPL-3.0-or-later license from repo root; configure `[workspace]` with `crates/*` and `[workspace.package] edition = "2024", license = "GPL-3.0-or-later"`. Implement `.codemode/config.toml` parsing, XDG path resolution, and the declarative 40+ `LanguageSpec` registry.

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-core && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-core --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode` W0 scaffold with `Zafar Sathiq S <fairsciencenow@gmail.com>`.

---

### Task 2: `codemode-ts` Structural Engine & Overload-Aware Name Path (W1a)

**Files:**
- Create: `codemode/crates/ts/Cargo.toml`
- Create: `codemode/crates/ts/src/lib.rs`
- Create: `codemode/crates/ts/src/grammars.rs`
- Create: `codemode/crates/ts/src/outline.rs`
- Create: `codemode/crates/ts/src/chunk.rs`
- Reference: `rust/astlens/src/ts.rs`, `rust/name-path/src/port.rs`
- Test: `codemode/crates/ts/tests/structural_parity.rs`

**Interfaces:**
- Consumes: `codemode_core::languages::LanguageId`
- Produces:
  - `codemode_ts::{ParsedFile, Symbol, OutlineResult, parse_file, find_symbols, best_match, replace_symbol_body, chunk_file}`

- [ ] **Step 1: Write failing tests for tree-sitter grammar coverage, TSX nested hooks, overload `[i]` matching, and shape chunking**
  Assert `parse_file` succeeds for existing 14 languages plus newly added `tree-sitter 0.27`-compatible grammars (e.g. Julia, Bash, Lua, Zig, Elixir, CSS/HTML/YAML/TOML/Markdown), returns `Err(UnsupportedGrammar)` cleanly for LSP-only languages, preserves `overload_idx` on sibling symbols, and splits long files into symbol-aligned chunks via `chunk_file`.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-ts`
  Expected: FAIL (missing `codemode-ts`).

- [ ] **Step 3: Port `astlens/src/ts.rs` + `name-path/src/port.rs`, expand `grammars.rs`, and add `chunk_file`**
  Also extend `rust/oracle/name_path.py` and `rust/name-path/src/port.rs` so overload `[i]` matching is gated directly when `rust-anything` runs.

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-ts && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-ts --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-ts` task.

---

### Task 3: `codemode-lsp` Multiplexer, `documentSymbol` Fallback & Fast Diagnostics (W1b)

**Files:**
- Create: `codemode/crates/lsp/Cargo.toml`
- Create: `codemode/crates/lsp/src/lib.rs`
- Create: `codemode/crates/lsp/src/client.rs`
- Create: `codemode/crates/lsp/src/diagnostics.rs`
- Create: `codemode/crates/lsp/src/document_symbol.rs`
- Reference: `rust/astlens/src/lsp.rs`, `rust/diagnostics-group-rs/src/port.rs`
- Test: `codemode/crates/lsp/tests/lsp_multiplexer.rs`

**Interfaces:**
- Consumes: `codemode_core::languages::{LanguageId, LanguageSpec}`, `codemode_ts::Symbol`
- Produces:
  - `codemode_lsp::{LspManager, Location, Diagnostic, document_symbols_outline, evict_idle_clients}`

- [ ] **Step 1: Write failing tests for fast clean-file diagnostics, idle LSP eviction, `documentSymbol` fallback, and worktree isolation**
  Assert against mock LSP servers:
  - Clean unchanged file with pull-diagnostics empty report or settled empty `publishDiagnostics` completes in `<150 ms` (eliminating the fixed `1500 ms` sleep).
  - `evict_idle_clients(Duration::from_millis(50))` terminates idle LSP children cleanly.
  - `document_symbols_outline` translates hierarchical LSP `DocumentSymbol[]` and flat `SymbolInformation[]` into `codemode_ts::Symbol` trees (for languages without a tree-sitter grammar).
  - Concurrent queries on two git worktrees spawn distinct LSP clients and never cross-contaminate.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-lsp`
  Expected: FAIL.

- [ ] **Step 3: Port `astlens/src/lsp.rs` + `diagnostics-group-rs/src/port.rs`, add `documentSymbol` fallback, fast clean-file path, and idle eviction**

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-lsp && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-lsp --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-lsp` task.

---

### Task 4: `codemode-store` & `codemode-retrieval` (SQLite FTS5, Spill Index, BM25 Search, Compaction Journal) (W2a)

**Files:**
- Create: `codemode/crates/store/Cargo.toml`
- Create: `codemode/crates/store/src/lib.rs`
- Create: `codemode/crates/store/src/schema.rs`
- Create: `codemode/crates/store/src/events.rs`
- Create: `codemode/crates/retrieval/Cargo.toml`
- Create: `codemode/crates/retrieval/src/lib.rs`
- Create: `codemode/crates/retrieval/src/bm25.rs`
- Create: `codemode/crates/retrieval/src/html.rs`
- Test: `codemode/crates/retrieval/tests/store_and_search.rs`

**Interfaces:**
- Consumes: `codemode_core::paths::context_db_path`, `codemode_ts::chunk_file`
- Produces:
  - `codemode_store::{ContextDb, SourceRecord, EventKind, JournalEvent, SpillHandle, StatsReport}`
  - `codemode_retrieval::{SearchQuery, SearchHit, html_to_text, index_path_gitignore, search_index, build_resume_summary}`

- [ ] **Step 1: Write failing tests for FTS5 indexing, line units, token counts, source labels, HTML→text, BM25 + trigram fallback, and resume summary**
  Assert:
  - Storing a 200 KB output with a source label creates a `SpillHandle { source_id, preview, total_bytes, token_count, chunk_count }` where `preview.len() <= 2048`.
  - Indexing preserves line-unit ranges (`start_line..=end_line`), `html_to_text` strips boilerplate tags (`<script>`, `<style>`) while keeping headings/code text, and `index_path_gitignore` skips `.gitignore`-excluded files.
  - Reopening `ContextDb` from disk and running `search_index` with `source_id` or label filter returns ranked BM25 snippets; typo/identifier substring queries fall back to trigram matching.
  - Recording all journal `EventKind` variants (`prompt`, `edit`, `cmd`, `toolerr`, `index`, `cost`, `decision`) with `turn_number`, `bytes_in`, and `bytes_out` produces accurate `StatsReport` savings and a deterministic `build_resume_summary` (`last_task`, changed files, open errors, decisions, search terms).

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-store -p codemode-retrieval`
  Expected: FAIL.

- [ ] **Step 3: Implement `rusqlite` FTS5 (sources, chunks, line units, trigrams, token counts, source labels), full event journal, gitignore-aware indexer, HTML→text extractor, and clean-room BM25/resume retrieval**

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-store -p codemode-retrieval && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-store -p codemode-retrieval --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-store` and `codemode-retrieval`.

---

### Task 5: `codemode-sandbox` (`code_execute` Runtime, Env Scrubbing, Process-Group Kill, Background Jobs & Spill) (W2b)

**Files:**
- Create: `codemode/crates/sandbox/Cargo.toml`
- Create: `codemode/crates/sandbox/src/lib.rs`
- Create: `codemode/crates/sandbox/src/runtimes.rs`
- Create: `codemode/crates/sandbox/src/jobs.rs`
- Test: `codemode/crates/sandbox/tests/sandbox_exec.rs`

**Interfaces:**
- Consumes: `codemode_store::ContextDb`
- Produces:
  - `codemode_sandbox::{ExecRequest, ExecOutcome, JobId, JobStatus, execute_sandboxed, job_status}`

- [ ] **Step 1: Write failing tests for runtime execution, env scrubbing, timeout process-group kill, background jobs, and auto-spill**
  Assert:
  - Sensitive env vars (`GITHUB_TOKEN`, `OPENAI_API_KEY`, `AWS_SECRET_ACCESS_KEY`) are stripped unless explicitly allowlisted.
  - A subprocess spawning a child sleep exceeding `timeout_ms` kills the full process group without orphan processes.
  - Background mode returns a `JobId` immediately, polls via `job_status`, and indexes completed output into `ContextDb`.
  - Output exceeding `inline_cap_bytes` is spilled to `ContextDb` and returns `SpillHandle` + exit code.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-sandbox`
  Expected: FAIL.

- [ ] **Step 3: Implement clean-room sandbox runner with `setsid`/PGID kill, background job table, runtime detection (`sh`/`bash`/`python3`/`node`), and spill integration**

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-sandbox && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-sandbox --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-sandbox`.

---

### Task 6: `codemode-policy` & `codemode-hooks` (`<10 ms` Synchronous Hook Engine + Throttle/Spool) (W3)

**Files:**
- Create: `codemode/crates/policy/Cargo.toml`
- Create: `codemode/crates/policy/src/lib.rs`
- Create: `codemode/crates/policy/src/shell_lexer.rs`
- Create: `codemode/crates/hooks/Cargo.toml`
- Create: `codemode/crates/hooks/src/lib.rs`
- Create: `codemode/crates/hooks/src/spool.rs`
- Test: `codemode/crates/policy/tests/policy_decisions.rs`

**Interfaces:**
- Consumes: `codemode_core::config::ProjectConfig`, `codemode_store::ContextDb`
- Produces:
  - `codemode_policy::{CommandDecision, evaluate_shell_command, evaluate_file_read}`
  - `codemode_hooks::{HookEvent, HookOutput, run_hook_sync}`

- [ ] **Step 1: Write failing tests for shell wrapper unwrapping, read-deny/read-cap policy, throttle/spool, and `<10 ms` synchronous hook dispatch**
  Assert `evaluate_shell_command` unwraps `env`, `bash -c`, `sh -c`, pipelines, and detects bulk reads (`cat`, `head`, `tail`, `sed`) on large tracked files, while permitting small targeted reads and non-source commands. Assert `run_hook_sync` injects session-start/compaction recall, throttles repeated noisy post-tool outputs, spools overflow into `ContextDb`, and completes without initializing a Tokio runtime.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-policy -p codemode-hooks`
  Expected: FAIL.

- [ ] **Step 3: Implement shell lexer/unwrapper, read-policy rules, and synchronous hook handlers (`pre-tool`, `post-tool`, `session-start`, `pre-compact`)**

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-policy -p codemode-hooks && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-policy -p codemode-hooks --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-policy` and `codemode-hooks`.

---

### Task 7: `codemode-memories` (Markdown Memory Store + `mem:` Reference Graph) (W4)

**Files:**
- Create: `codemode/crates/memories/Cargo.toml`
- Create: `codemode/crates/memories/src/lib.rs`
- Create: `codemode/crates/memories/src/refs.rs`
- Reference: `src/serena/memories/memory_manager.py`, `src/serena/memories/memory_reference_analysis.py`
- Test: `codemode/crates/memories/tests/memories_graph.rs`

**Interfaces:**
- Consumes: `codemode_core::paths::memories_dir`, `codemode_store::ContextDb`
- Produces:
  - `codemode_memories::{MemoryStore, MemoryEntry, ReferenceGraph}`

- [ ] **Step 1: Write failing tests for `.codemode/memories/*.md` CRUD, `mem:` link extraction, and rename propagation**
  Assert saving/reading/deleting memories under `.codemode/memories/`, indexing them into `ContextDb`, detecting dangling `mem:` links, and renaming a memory updates referencing `mem:<old>` tokens across sibling markdown files.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-memories`
  Expected: FAIL.

- [ ] **Step 3: Port pure memory management & reference propagation logic using `.codemode/memories/` format**

- [ ] **Step 4: Run tests and clippy**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-memories && cargo clippy --manifest-path codemode/Cargo.toml -p codemode-memories --tests -- -D warnings`
  Expected: PASS, 0 warnings.

- [ ] **Step 5: Commit**
  Commit `codemode-memories`.

---

### Task 8: `codemode-mcp` & `codemode-cli` (`code_*` Unified Server & Subcommands) (W5a)

**Files:**
- Create: `codemode/crates/mcp/Cargo.toml`
- Create: `codemode/crates/mcp/src/lib.rs`
- Create: `codemode/crates/mcp/src/tools.rs`
- Create: `codemode/crates/cli/Cargo.toml`
- Create: `codemode/crates/cli/src/main.rs`
- Test: `codemode/crates/cli/tests/mcp_e2e.rs`

**Interfaces:**
- Consumes: All 10 library crates (`core`, `ts`, `lsp`, `store`, `retrieval`, `sandbox`, `policy`, `hooks`, `memories`)
- Produces:
  - Binary `codemode` supporting `serve | hook <event> | index <path> | doctor | stats | purge | upgrade`
  - 12 MCP tools: `code_outline`, `code_read`, `code_references`, `code_diagnostics`, `code_edit`, `code_execute`, `code_index`, `code_search`, `code_resume`, `code_memory`, `code_batch`, `code_admin`

- [ ] **Step 1: Write failing E2E stdio MCP tests for all 12 `code_*` tools and CLI subcommands**
  Assert `codemode serve` lists all 12 `code_*` schemas, executes `code_outline`, `code_read`, `code_edit`, `code_execute` -> `code_search` round-trip, `code_memory` write/read/rename, `code_batch` parallel dispatch, `code_resume`, and `code_admin`, while `codemode doctor` / `stats` / `hook` exit 0.

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --manifest-path codemode/Cargo.toml -p codemode-cli`
  Expected: FAIL.

- [ ] **Step 3: Implement unified `code_*` MCP registry, bounded-output + FTS5 spill wrapper, and `cli` entrypoint**

- [ ] **Step 4: Run full workspace tests, clippy, and release build**
  Run: `rustfmt --edition 2024 --check codemode/crates/*/src/*.rs codemode/crates/*/tests/*.rs && cargo test --manifest-path codemode/Cargo.toml && cargo clippy --manifest-path codemode/Cargo.toml --tests -- -D warnings && cargo build --release --manifest-path codemode/Cargo.toml`
  Expected: PASS, 0 warnings, release binary at `codemode/target/release/codemode`.

- [ ] **Step 5: Commit**
  Commit `codemode-mcp` and `codemode-cli`.

---

### Task 9: Typed Code Mode Dispatch Inside `code_execute` (W5b / v1.1)

**Files:**
- Modify: `codemode/crates/sandbox/src/lib.rs`
- Modify: `codemode/crates/mcp/src/tools.rs`
- Create: `codemode/crates/sandbox/src/codemode_rpc.rs`
- Test: `codemode/crates/cli/tests/codemode_script.rs`

**Interfaces:**
- Consumes: `codemode_mcp::tools` dispatch handler
- Produces:
  - Per-invocation local Unix domain socket / JSON-line pipe + thin Python/Node `codemode` helper injected into `code_execute` when `mode: "codemode"` is requested, allowing one script to chain `code_outline`, `code_read`, `code_references`, and `code_search` without round-tripping LLM turns.

- [ ] **Step 1: Write failing test for a multi-step Python/Node script calling `codemode.outline` + `codemode.read` inside `code_execute`**
- [ ] **Step 2: Run test to verify it fails**
- [ ] **Step 3: Implement scoped loopback IPC bridge + minimal `codemode` Python/JS runtime prelude**
- [ ] **Step 4: Run tests and clippy (`cargo test -p codemode-cli --test codemode_script`)**
- [ ] **Step 5: Commit**

---

### Task 10: Host Packaging & Repository Cutover (W6)

**Files:**
- Create: `codemode/packaging/antigravity/plugin.json`
- Create: `codemode/packaging/antigravity/mcp_config.json`
- Create: `codemode/packaging/antigravity/hooks.json`
- Create: `codemode/packaging/antigravity/skills/codemode/SKILL.md`
- Create: `codemode/packaging/opencode/opencode.json`
- Create: `codemode/packaging/opencode/plugins/codemode-hooks.ts`
- Create: `codemode/packaging/claude-code/.claude-plugin/plugin.json` (next)
- Create: `codemode/packaging/codex/config.toml` (future)
- Create: `codemode/README.md`

- [ ] **Step 1: Write smoke validation tests for all host JSON/TOML manifests and hook invocations**
- [ ] **Step 2: Run validation to verify manifests fail before creation**
- [ ] **Step 3: Create host bundles (Antigravity + OpenCode first, Claude Code next, Codex future template) and install script**
- [ ] **Step 4: Verify `codemode doctor`, Antigravity `GetAllPlugins` load check, and OpenCode config schema check**
- [ ] **Step 5: Commit and prepare `landigit/codemode` standalone repo cutover**
