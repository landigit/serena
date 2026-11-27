Analyzed SKILL.md:L1-100
Analyzed SKILL.md:L101-109
Analyzed SKILL.md:L109
Analyzed hooks.json:L1-37
Analyzed SKILL.md:L1-100
Analyzed SKILL.md:L101-200
Analyzed SKILL.md:L201-309
Analyzed read-cap.mjs:L1-53
Ran check agy customization MCP and hook docs
Analyzed plugins.md:L1-100
Analyzed rules.md:L1-40
Analyzed hooks.md:L36-135
Analyzed hooks.md:L136-235

The cleanest way to build an Antigravity-native replacement for Serena without conflicts is a **single static Rust binary over Stdio MCP, packaged as a standalone Antigravity Plugin (`plugin.json` + `mcp_config.json` + one lazy `SKILL.md`)** — with **Tree-sitter first, lazy per-worktree LSP second, zero REPL/onboarding ceremony, and zero hooks/always-on rules blocking native tools.**

Here are the exact conflict points observed in Antigravity’s plugin/MCP layer, followed by the ground-up plan.

---

### 1. Why Existing Setups Conflict (and How to Avoid Every Trap)

From [`plugin/SKILL.md`](file:///home/dev/.gemini/antigravity/builtin/skills/plugin/SKILL.md), [`agy-customizations/SKILL.md`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/SKILL.md), and [`hooks.md`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/docs/hooks.md):

1. **Global namespace collisions:** Both plugin `name`s and MCP server keys share a **single global namespace**. In this environment, two Serena servers (`serena` and `universal-skills_serena`) exist side by side, and a colliding plugin `name` causes one plugin to be **dropped silently**.
   - **Rule:** Never name your plugin/server/skill `serena`, `lsp`, or `search`. Prefix everything distinctly (e.g., plugin `astlens`, MCP key `astlens-mcp`, skill `astlens`).
2. **Instruction & tool warfare (`rules/AGENTS.md` & `instructions.md`):** Serena’s `instructions.md` marks native `Read`/`Edit` `FORBIDDEN`, fighting Antigravity’s built-in `read_file`/`edit_file` prompts, and plugin `rules/AGENTS.md` is always-on and counts against Antigravity’s `20,000`-token `defaultRulesBudget` ([`rules.md`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/docs/rules.md)).
   - **Rule:** Ship **no `rules/AGENTS.md`**, **no `initial_instructions` step**, and **no `PreToolUse` hooks** (avoid colliding with existing hooks like [`read-cap.mjs`](file:///home/dev/.gemini/config/plugins/universal-skills/scripts/read-cap.mjs) or slowing every tool call). Teach when to call the tool via an on-demand `SKILL.md` (progressive disclosure) and short tool descriptions.
3. **LSP staleness vs. native `edit_file` / `run_command`:** If an agent edits via `edit_file` or formats via `run_command`, a long-lived LSP that caches buffers returns stale line numbers.
   - **Rule:** Always check file mtime/hash on disk before any LSP call and send `didOpen` / `didChange` from disk if changed.
4. **Parallel subagent & worktree collisions:** Antigravity spawns `research` / `run_workflow` / `/swarm` subagents in separate `share`/`branch` worktrees. A single global `activate_project` state causes agents in different worktrees to clobber each other.
   - **Rule:** Keep every MCP call **stateless**: derive `(language, canonical_workspace_root)` from the file path argument per call (`DashMap`), never from global `active_project` state.
5. **Repo pollution:** Writing indexes/memories into `.serena/` dirties `git status` and branches.
   - **Rule:** Store caches strictly under the `PLUGIN_DATA` env directory that Antigravity automatically injects ([`hooks.md`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/docs/hooks.md#L135-L138)).

---

### 2. Ground-Up Architecture Plan

#### Phase 0: Non-Conflicting Tool Contract (5 Flat Tools, No REPL)
Replace Serena’s Python REPL + onboarding (`initial_instructions` → `activate_project` → `s.info`) with **5 stateless JSON MCP tools** complementing (never duplicating) native `read_file`/`edit_file`/`run_command`:

| Tool | Engine | Purpose |
| :--- | :--- | :--- |
| `ast_outline` | Tree-sitter | Hierarchical outline of a file (including React hooks, handlers, JSX regions inside big TSX components where LSP fails) |
| `read_symbol` | Tree-sitter | Return exact body + signature of a symbol path (hard-capped output so responses stay inline and never spill to temp files) |
| `find_references` | Lazy LSP | Cross-file references, definitions, or implementations (`workspace_root` auto-resolved from file path) |
| `edit_symbol` | Tree-sitter + LSP | Replace a single symbol/node or execute an LSP `workspace/rename`, syncing disk state + returning a compact unified diff |
| `diagnostics` | Lazy LSP | Fast per-file/project compiler errors without running full build scripts |

*(Wait: intentionally omit generic `read_file`, `list_dir`, `search_text`, and `run_shell` so Antigravity never hesitates between built-in tools and MCP.)*

#### Phase 1: Rust Core — Tree-Sitter & Fast Structural Engine
- **Stack:** `rmcp` (stdio transport) + `tokio`, `tree-sitter` (+ grammars for TypeScript, TSX, Rust, Python, etc.), `ignore` (respects `.gitignore`, automatically skipping `node_modules`, `target`, `android/build`, `dist`), `similar` (compact diffs).
- **Zero-wait startup:** Single static release binary (~`<20 ms` launch, no `uvx`/venv).
- **TSX/React advantage:** Custom Tree-sitter `.scm` queries capture top-level exports **plus** nested `const handle* = ...`, `useEffect`/`useMemo`/`useCallback`, and JSX landmarks inside 1,000+ line components.
- **Output discipline:** Bound response size in-process (e.g., `<= 6–8 KB` or paginated cursor) so Antigravity never dumps tool responses into disk output artifacts.

#### Phase 2: Worktree-Safe Lazy LSP Multiplexer
- **On-demand boot:** Only `find_references`, `diagnostics`, and cross-file `edit_symbol` (`mode: "rename"`) spawn child LSPs (`typescript-language-server`, `rust-analyzer`, `pyright`). Outline/read/single-file symbol edits run in milliseconds via Tree-sitter alone.
- **Per-worktree isolation:** Cache LSP processes in `DashMap<(LanguageId, PathBuf), Arc<LspClient>>`, keyed by the nearest project root (`tsconfig.json`, `Cargo.toml`, `pyproject.toml`, `.git`) of the target file.
- **Disk-truth synchronization:** Track `(mtime, len)` per URI; automatically re-sync changed files via LSP `textDocument/didChange` before every semantic query so native `edit_file` and MCP stay 100% consistent.
- **Idle eviction:** Shut down unused LSP children after ~5–10 minutes of inactivity so Android/Gradle/emulator RAM isn’t starved.

#### Phase 3: Native Antigravity Plugin Packaging
Package cleanly outside the repo at `~/.gemini/config/plugins/astlens/` following [`plugins.md`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/docs/plugins.md):

```text
~/.gemini/config/plugins/astlens/
├── plugin.json          # unique "name": "astlens" (no collision with serena)
├── mcp_config.json      # { "mcpServers": { "astlens-mcp": { "command": "/path/to/astlens-mcp" } } }
├── bin/astlens-mcp      # compiled static Rust release binary
└── skills/
    └── astlens/
        └── SKILL.md     # short progressive-disclosure guide (when AST/LSP beats grep/read_file)
```

- **Important hygiene:** Disable `serena` / overlapping `universal-skills` entries via the Customizations UI if you want only one semantic toolset exposed (per [`plugin/SKILL.md`](file:///home/dev/.gemini/antigravity/builtin/skills/plugin/SKILL.md), never hand-edit `~/.gemini/config/config.json`). Keep [`mcp_config.json`](file:///home/dev/.gemini/antigravity/builtin/skills/agy-customizations/docs/mcp_servers.md) focused on the 5 tools so lazy-loaded `.json` schemas in `<appDataDir>/mcp/` stay tiny.

#### Phase 4: Side-by-Side Verification Checklist
1. **Schema & collision check:** Confirm only `astlens-mcp` appears in Active MCP Servers with 5 small schemas and no `FORBIDDEN` rules injected into the prompt.
2. **Monolith TSX test:** Run `ast_outline` on [`src/App.tsx`](file:///home/dev/antigravity/Ownlyst-Studio-%E2%80%94-QuestPDF-&-Swiss-QR-for-Hisaabo/src/App.tsx) and verify inner handlers/hooks resolve cleanly in `<50 ms` without booting `tsserver`.
3. **Sync test:** Edit a symbol with native `edit_file`, immediately run `find_references` + `diagnostics`, and confirm zero stale-buffer errors.
4. **Worktree isolation test:** Query two files across separate git worktrees concurrently (`run_workflow` / parallel subagent) and verify references resolve independently per root.
