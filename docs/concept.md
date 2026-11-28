I dug through both codebases. Good news: you're not starting from zero — `rust/` in this repo already contains a working Rust code-intelligence core (`astlens` v1.3) plus two gated ports, and a 24-unit port map for the rest of Serena.

## The shape of the thing

Serena and context-mode are **two halves of the same loop**, not competitors:

- **Serena** makes *code reads and edits* cheap and precise (LSP + tree-sitter + name paths + memories).
- **context-mode** makes *everything else* cheap and recoverable (sandboxed exec, FTS5 index, journal, resume, hooks).

A single Rust binary can own the whole loop: *understand the code structurally → do work in a sandbox → spill all bulk output into one local index → let the agent ask for the 2 KB it needs → remember what happened across compaction.*

## What already exists here

| Asset | State |
| --- | --- |
| `rust/astlens` | v1.3, 5 stateless tools, tree-sitter + lazy per-worktree LSP multiplexer, 14 languages, measured 1–19 ms structural calls |
| `rust/name-path` | Gated 464/464 cases (Serena `NamePathMatcher`) |
| `rust/diagnostics-group-rs` | Gated 92/92 cases (`GroupedDiagnostics`) |
| `rust/PORT_MAP.md` | Serena split into 24 PR-sized units (~59k lines) |
| `concept.md` | Already specifies the 5-flat-tool contract, no REPL/onboarding |
| `rust/astlens` design docs | Disk-truth sync, idle eviction, stateless per-call routing — all decisions already made |

So the real question is **what to add from context-mode and what to deliberately not port from Serena.**

## The four seams where they fuse

1. **Output discipline.** Serena caps/truncates symbol bodies; context-mode spills to FTS and returns a handle. Union: *every* tool (including `find_references`, `diagnostics`, `run`) returns a bounded reply plus a searchable source id.
2. **Execution.** Serena has a Python REPL with typed facades; context-mode has a sandboxed `ctx_execute` (node/python/shell, output caps, process-group kill). Union: one sandboxed runner, and optionally **Code Mode over the typed API** — the highest-leverage idea in either project.
3. **Memory.** Serena memories are git-reviewable markdown with a reference graph (`mem:` refs, rename-with-propagation). context-mode has an event journal + resume block. Union: markdown files as source of truth, indexed in SQLite, plus an automatic session journal.
4. **Retrieval.** Serena = structural (name path, symbols). context-mode = lexical (FTS5 porter unicode61, BM25, tri-gram, line units). Union: one `search` that can hit symbols, memories, journal and spilled outputs.

## Feature inventory: borrow / adapt / drop

**From Serena — borrow**
- LSP multiplexer: per-`(language, workspace root)` clients, disk-truth `didChange`, settle, idle eviction ✅ already done
- tree-sitter symbol model, name paths, overload index ✅ already done
- Diagnostics: file, grouped-by-owning-symbol, project-wide, snapshot diff
- Symbol edits: replace body, insert before/after, LSP rename; safe-delete is the hard one (defer)
- **Memories**: manager + reference analysis + rename propagation (974 lines, pure logic → gate-able, high value)
- Project resolution: language detection, ignore specs, `project.yml`, modes/contexts as declarative tool policy
- Multi-project query (optional, adapt)

**From Serena — adapt**
- REPL → Code Mode inside the sandbox (not Python `eval`; typed API, no dynamic imports)
- `activate_project` / `initial_instructions` / onboarding → stateless per-call root resolution (already the astlens decision)

**From Serena — drop**
- Flask dashboard + tray + pywebview GUI, analytics, agno integration, JetBrains HTTP client, interprompt codegen, `repo_dir_sync`, reflection/“think about…” prompt tools, 40-language matrix (keep astlens' 14 and grow by demand)

**From context-mode — borrow**
- Sandbox: runtime detection, env scrubbing, process-group kill, timeouts, byte caps, background jobs
- FTS5 store: sources + chunks + tri-gram + line units, BM25, snippets, token counts, source labels
- Event journal: `prompt|edit|cmd|toolerr|index|cost|decision`, turn numbers, bytes in/out → `stats`
- Resume/compaction: summary block, last task, files changed, open errors, search terms
- Hooks: pre-tool read-deny/read-cap, session-start injection, post-tool indexing, throttle/spool
- **Policy engine**: bash + PowerShell command lexing/unwrapping to decide read-deny — genuinely hard, worth porting
- Indexing: gitignore-aware walk, shape-based chunking, HTML→text
- Lifecycle: `doctor`, `purge`, `stats`, `upgrade`

**From context-mode — adapt**
- 11-host adapter matrix → 3–4 hosts (opencode, claude, codex, gemini) behind one generic hook/manifest layer
- `ctx_batch` → parallel dispatch of any of our tools, one summary

**From context-mode — drop**
- Gateway/hosted service, v1 migration cruft, omp import, Node-specific sandbox shims, Windows build path (unless you need it)

## Proposed shape

One Cargo workspace, one static binary, three entrypoints (the way context-mode does `entries/server|hook|cli`):

```
crates/
  core      config, paths, project root resolution, errors
  ts        tree-sitter symbol model, name-path, outline, chunk shape
  lsp       multiplexer, disk-truth sync, settle, diagnostics grouping
  store     rusqlite (bundled FTS5): sources, chunks, events, memory index
  retrieval chunking, BM25, snippet, ranking
  sandbox   runtime detect, spawn, caps, kill, path policy
  policy    shell lexer/unwrapper, read-deny, own-file detection
  hooks     pre-tool, session-start, compaction recall, spool/throttle
  mcp       tool registry, JSON schemas, stdio server
  cli       serve | hook | index | doctor | stats | purge | upgrade
```

One binary, dispatch on argv: `sgl serve` (MCP stdio), `sgl hook <event>` (no tokio spin-up, <10 ms budget), `sgl index …`, `sgl doctor`. Data in `$XDG_DATA_HOME/sgl/<project-hash>/context.db`; memories stay plain markdown so they remain reviewable in git.

**Unified tool surface (~12 instead of 5 + 9):**

| Tool | Source | Engine |
| --- | --- | --- |
| `outline` | astlens | tree-sitter |
| `read_symbol` | astlens | tree-sitter + name-path |
| `find_references` | astlens | lazy LSP |
| `diagnostics` | astlens | lazy LSP + grouping |
| `edit_symbol` | astlens | tree-sitter / LSP rename |
| `run` | ctx_execute + Serena shell | sandbox |
| `index` | ctx_index | walk + chunk |
| `search` | ctx_search + Serena query | FTS5 + symbols + memories |
| `resume` | ctx_resume | journal + compaction |
| `memory` | Serena memories | markdown + index |
| `admin` | stats/doctor/purge | lifecycle |
| `batch` | ctx_batch | parallel dispatch |

That is the whole product: five code tools, five context tools, one memory tool, one admin.

## How rust-anything fits

Its model is **record cases from the oracle → port → gate on the recording**, so split the work honestly:

- **Gate-able (has a Python/TS oracle):** memory reference analysis, memory manager, symbol/name-path computation, text/line utilities, config parsing, ignore specs, **policy lexer/parser**, chunking, BM25 ranking + tokenizer parity, compaction scoring. These get recorded cases and CI gates.
- **Greenfield (no oracle value):** LSP transport (already in astlens), SQLite schema/migrations, sandbox process control, hook I/O, CLI, MCP schemas.
- **Never port:** dashboard, JetBrains client, Python eval REPL, prompt-codegen.

## Waves

1. **W0 — decide** license, hosts, naming, frozen tool contract
2. **W1 — promote astlens** into the workspace crates (`ts`, `lsp`) with the 5 tools still green
3. **W2 — store + run + index + search** (context-mode's core value; conformance tests from its `e2e/cases`)
4. **W3 — hooks + policy + journal + resume/stats/doctor**
5. **W4 — memories + project config/modes** (the last Serena value)
6. **W5 — Code Mode** over the typed API (optional, the differentiator)
7. **W6 — packaging** and cutover

## The one blocker you must decide first

**License conflict.** Serena is GPL-3.0-or-later; context-mode is Elastic-2.0 (source-available, not GPL-compatible). A single distributed binary containing both is not licensable as-is. Options:

- **A. GPL-3 binary, context-mode features reimplemented clean-room** from documented behavior (safest if you publish)
- **B. Two binaries sharing GPL-3 crates** (code-intel + context), installed together
- **C. Personal/internal only** — fine technically, but you can't ship it

## Questions before I write a plan

1. Publish or personal? (decides the license path above)
2. One binary or two? (A/B above; two also isolates crash blast radius)
3. Hosts first: opencode, Claude Code, Codex, Gemini, Antigravity — which two?
4. v1 scope: astlens' 14 languages + memories, or full Serena parity (40+ languages, JetBrains)?
5. Is Code Mode (script calling typed tools) in v1, or v2?
6. Keep `.serena/memories` + `project.yml` compatibility, or a new format?

Answer those and I'll turn this into an ordered, PR-sized plan that fits the existing `rust/PORT_MAP.md` numbering.
