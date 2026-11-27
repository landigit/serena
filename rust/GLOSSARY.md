# astlens glossary

| Term | Meaning | Rust |
| --- | --- | --- |
| name path | Slash-separated path of a symbol inside one file, e.g. `Class/method` | `String`, `Symbol::name_path` |
| pattern | A name path expression with optional leading `/` and `[i]` overload index | `name_path::Matcher` (private) |
| symbol | A tree-sitter declaration node with a name (function, class, method, variable, ...) | `ts::Symbol` |
| workspace root | Nearest ancestor with a language marker (`tsconfig.json`, `Cargo.toml`, ...) or `.git` | `roots::workspace_root` |
| language server | Lazy child process serving one `(language, workspace root)` | `lsp::LspClient` |
| settle | One-time wait after the first `didOpen` so cross-file queries see the loaded project | `LspClient::settle` |
| disk truth | Re-reading a file and sending `didChange` before every LSP query | `LspClient::ensure_open` |
| plugin | Antigravity bundle at `~/.gemini/config/plugins/astlens` | n/a |
