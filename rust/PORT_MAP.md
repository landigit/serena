# Port map: serena

5 of 24 units have a Rust port; no original changed since it was recorded.

## Ports

| Unit | Crate | In Rust | Kept in the original | Cases | Last diff | Original |
| --- | --- | --- | --- | ---: | ---: | --- |
| `repo_dir_sync.py` | `rust/repo-dir-sync-rs` | `popen`, `call`, `execute`, `gitLog`, `gitCommit`, `gitCommitWithMessageFromFile` | `OtherRepo`, `LibRepo` | 144 | not run | unchanged |
| `rust/oracle/diagnostics_group.py` | `rust/diagnostics-group-rs` | `severity_name`, `diagnostic_output`, `group` | - | 92 | 92/92 | unchanged |
| `rust/oracle/name_path.py` | `rust/name-path` | `match_name_path`, `pattern_components` | - | 464 | 464/464 | unchanged |
| `src/interprompt/` | `rust/interprompt-rs` | `autogenerate_prompt_factory_module`, `ParameterizedTemplateInterface` | `JinjaTemplate`, `PromptTemplate`, `PromptList`, `LanguageFallbackMode`, `MultiLangPromptTemplate`, `MultiLangPromptList`, `MultiLangPromptCollection`, `PromptFactoryBase` | 468 | not run | unchanged |
| `src/serena/lsp/` | `rust/lsp-rs` | `GroupedDiagnostics`, `DiagnosticsDiff` | `LSPLanguageBackend`, `EditedFilePath`, `DiagnosticIdentity`, `PublishedDiagnosticsSnapshot`, `DiagnosticsContext` | 0 | not run | unchanged |

## Not ported yet

In the order `analyze` suggests.

| Unit | Status | Lines | Exports | Readiness |
| --- | --- | ---: | ---: | ---: |
| `src/serena/generated/` | caution | 62 | 1 | 93 |
| `src/solidlsp/language_servers/elixir_tools/` | caution | 364 | 1 | 81 |
| `src/serena/olb/` | caution | 54 | 1 | 94 |
| `src/interprompt/util/` | caution | 9 | 1 | 100 |
| `sync.py` | ready | 5 | 0 | 100 |
| `src/solidlsp/util/` | caution | 773 | 16 | 42 |
| `src/serena/jetbrains/` | caution | 898 | 23 | 42 |
| `src/serena/memories/` | caution | 974 | 16 | 20 |
| `src/serena/repl/` | blocked | 1102 | 0 | 16 |
| `src/serena/repl/api/` | caution | 2039 | 36 | 25 |
| `src/serena/dashboard/` | blocked | 1121 | 0 | 17 |
| `src/serena/util/` | caution | 2439 | 59 | 0 |
| `src/serena/tools/` | caution | 1922 | 74 | 10 |
| `src/serena/` | blocked | 7757 | 0 | 0 |
| `src/serena/config/` | caution | 1664 | 24 | 0 |
| `src/solidlsp/` | caution | 6466 | 59 | 0 |
| `src/solidlsp/language_servers/` | caution | 23983 | 120 | 0 |
| `src/solidlsp/lsp_protocol_handler/` | caution | 5013 | 360 | 0 |
| `scripts/` | caution | 2113 | 46 | 0 |

3 of these units have no port on purpose (3 blocked, 0 kept in the original), so 16 units are left to port:

- `src/serena/repl/`: blocked, Uses dynamic code (eval, new Function or computed imports)
- `src/serena/dashboard/`: blocked, `flask` has no Rust equivalent
- `src/serena/`: blocked, `flask` has no Rust equivalent; `uvicorn` has no Rust equivalent

Regenerate with `rust-anything map --write`. Commit this file: when the original changes upstream, the next map marks every port whose recording it outdated.
