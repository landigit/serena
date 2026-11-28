# serena-generated

Pure-Rust port of `src/serena/generated/`:

- `tool_capabilities`: deterministic `EDIT_CAPABLE_TOOL_NAMES` manifest, binary-search lookup (`is_edit_capable_tool`), and `render_tool_capabilities_python_module` matching `scripts/gen_tool_capabilities.py`.
- `prompt_factory`: typed `PromptFactory<B: PromptBackend>`, parameter structs, and `GENERATED_PROMPTS` descriptor catalog mirroring `src/serena/generated/generated_prompt_factory.py`.

## Verification

```sh
cargo test --manifest-path rust/serena-generated/Cargo.toml --offline
cargo clippy --manifest-path rust/serena-generated/Cargo.toml --tests --offline -- -D warnings
rustfmt --edition 2024 --check rust/serena-generated/src/*.rs
```
