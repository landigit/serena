---
name: codemode
description: >
  Guide for using codemode MCP tools (code_outline, code_read, code_references,
  code_diagnostics, code_edit, code_execute, code_index, code_search, code_resume,
  code_memory, code_batch, code_admin) alongside native host tools.
---

# `codemode` Usage Guide

`codemode` complements native file tools with 12 bounded `code_*` tools:

- **`code_outline` / `code_read`** — Inspect symbol trees across 40+ languages (uniform `arborium-*` tree-sitter grammars with automatic LSP `documentSymbol` fallback) and fetch exact symbol bodies by Serena `name_path` (including `[i]` overloads).
- **`code_references` / `code_diagnostics` / `code_edit`** — Cross-file LSP navigation, grouped/fast clean-file diagnostics, and unified-diff symbol replace or project-wide rename with `(mtime, len)` disk-truth re-sync.
- **`code_execute`** — Run shell/Python/Node commands in a scrubbed, PGID-isolated sandbox; oversized stdout/stderr spills to FTS5 with a `source_id`. Set `mode: "codemode"` to chain `codemode.outline`, `codemode.read`, `codemode.references`, and `codemode.search` inside one script.
- **`code_index` / `code_search`** — Index repo trees (gitignore-aware), HTML, or text into local SQLite FTS5 and query with BM25 + trigram typo fallback.
- **`code_resume` / `code_memory` / `code_batch` / `code_admin`** — Recover state across compactions, manage `.codemode/memories/*.md` (`mem:` link graph), run parallel tool batches, and inspect stats/doctor/LSP status.
