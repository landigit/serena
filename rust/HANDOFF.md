# HANDOFF

## Where the port stands

- Goal: standalone Rust MCP server (`astlens`) replacing Serena's semantic layer, packaged as an
  Antigravity plugin. Serena stays installed as the Python reference.
- Gate so far: `rust-anything gate rust/name-path` → **passed** (464/464 recorded cases).
- Binary: `rust/astlens` — 5 tools, tree-sitter outline, lazy per-worktree LSP, disk-truth resync.
  Release binary in place at `~/.gemini/config/plugins/astlens/bin/astlens`; plugin validated by
  `agy plugin validate --strict`.
- Commits: `f5ba5641` (astlens + passing gate), `118e1038` (second bridge/record).

## Next

- Continue porting from the `rust-anything` plan order (`rust/MAP.md`, generated `rust/PORT_MAP.md`).
- Blocked at record: `src/serena/lsp/` (driver module-loading failure).
- Low-value unit surfaced: `repo_dir_sync.py` (bridge + record OK, 144 cases, mostly error paths).

## Commands to re-run

```sh
rust-anything gate rust/name-path
rust-anything diff rust/name-path
rust-anything build rust/astlens          # dev
cargo build --release --manifest-path rust/astlens/Cargo.toml
rust-anything map . --write
```

## Skill to load next

`rust-anything` (whole-app flow: grill → record → front door → port waves → lift → cut over).
