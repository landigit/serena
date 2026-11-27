# `astlens`

A standalone, stateless Rust stdio MCP server replacing Serena's semantic tool surface with tree-sitter structural parsing and a lazy per-worktree LSP multiplexer.

## Tools

| Tool | Engine | Purpose |
| --- | --- | --- |
| `ast_outline` | Tree-sitter | Hierarchical symbol outline of a file (including nested TSX hooks/handlers) |
| `read_symbol` | Tree-sitter + `name-path` | Exact source of one symbol by Serena name path (capped at 6,000 chars by default) |
| `find_references` | Lazy LSP | Cross-file `references`, `definition`, or `implementation` by `name_path` or 1-based `(line, column)` |
| `edit_symbol` | Tree-sitter / Lazy LSP | Replace one symbol body (`mode: "replace"`, supports `dry_run`) or rename across project (`mode: "rename"`) |
| `diagnostics` | Lazy LSP + `diagnostics-group-rs` | Single-file diagnostics filtered by `min_severity`; pass `grouped: true` to group by severity and owning tree-sitter symbol `name_path` |

Every call derives `(language, workspace_root)` from the target file path and re-syncs disk truth (`didOpen` / `didChange` on `(mtime, len)` change) before querying LSPs, keeping parallel worktrees isolated and edits from external tools consistent.

## Supported Languages & LSPs

| Language | Extensions | Default Language Server | Override Env Var |
| --- | --- | --- | --- |
| TypeScript | `.ts`, `.mts`, `.cts` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_TYPESCRIPT` |
| TSX | `.tsx` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_TSX` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` | `tsgo --lsp --stdio` / `vtsls --stdio` / `typescript-language-server --stdio` | `ASTLENS_LSP_JAVASCRIPT` |
| Rust | `.rs` | `rust-analyzer` | `ASTLENS_LSP_RUST` |
| Python | `.py`, `.pyi` | `ty server` / `pyrefly lsp` / `pyright-langserver --stdio` / `basedpyright-langserver --stdio` | `ASTLENS_LSP_PYTHON` |
| Go | `.go` | `gopls` | `ASTLENS_LSP_GO` |
| Java | `.java` | `jdtls` | `ASTLENS_LSP_JAVA` |
| C | `.c`, `.h` | `clangd` | `ASTLENS_LSP_C` |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hpp`, `.hh`, `.hxx` | `clangd` | `ASTLENS_LSP_CPP` |
| Ruby | `.rb`, `.rake`, `.gemspec` | `ruby-lsp` / `solargraph stdio` | `ASTLENS_LSP_RUBY` |
| PHP | `.php`, `.phtml` | `intelephense --stdio` / `phpactor language-server` | `ASTLENS_LSP_PHP` |
| C# | `.cs` | `csharp-ls --stdio` / `OmniSharp -lsp` | `ASTLENS_LSP_CSHARP` |
| Kotlin | `.kt`, `.kts` | `kotlin-lsp --stdio` / `kotlin-language-server --stdio` | `ASTLENS_LSP_KOTLIN` |
| Lean 4 | `.lean` | `lean --server` | `ASTLENS_LSP_LEAN` |

## Shared Gated Units

`astlens` compiles in two gated `rust-anything` port modules directly without a PyO3 link:

- `../name-path/src/port.rs` (`rust/name-path`, 464/464 recorded cases)
- `../diagnostics-group-rs/src/port.rs` (`rust/diagnostics-group-rs`, 92/92 recorded cases)

## Build, Test & Install

```sh
cargo test --manifest-path rust/astlens/Cargo.toml
cargo clippy --manifest-path rust/astlens/Cargo.toml -- -D warnings
cargo build --release --manifest-path rust/astlens/Cargo.toml

# Atomic update of an installed plugin binary (avoids ETXTBSY while a server is running)
cp rust/astlens/target/release/astlens ~/.gemini/config/plugins/astlens/bin/astlens.new
mv -f ~/.gemini/config/plugins/astlens/bin/astlens.new ~/.gemini/config/plugins/astlens/bin/astlens
```
