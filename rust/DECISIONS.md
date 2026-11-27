# astlens port decisions

## ADR-001: Replace Serena's interface, do not port it wholesale

Status: accepted (2026-11-27)

Context: Serena is 59k lines / 22 units including LSP clients, dashboard, GUI and
JetBrains integration. `concept.md` specifies a greenfield 5-tool MCP server.

Decision: build `astlens`, a standalone Rust stdio MCP server with five stateless
tools: `ast_outline`, `read_symbol`, `find_references`, `edit_symbol`,
`diagnostics`. Port only behavior that has a Python oracle (name-path matching)
through rust-anything and gate it.

Consequences: Serena stays installed as reference. No PyO3 extension in the
Serena package. The plugin lives at `~/.gemini/config/plugins/astlens`.

## ADR-002: Equivalence bar

Status: accepted (2026-11-27)

Decision: results must agree with the language server / tree-sitter ground truth;
output format is astlens's own. The gated name-path unit must match Serena's
`NamePathMatcher` exactly for every recorded case.

## ADR-003: Packaging

Status: accepted (2026-11-27)

Decision: crate at `rust/astlens`, release binary copied into the plugin's `bin/`
with `plugin.json`, `mcp_config.json` and a lazy `SKILL.md`. No rules file, no
hooks, no `initial_instructions`.
