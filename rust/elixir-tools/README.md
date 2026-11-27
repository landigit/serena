# elixir-tools

Pure-Rust port of `src/solidlsp/language_servers/elixir_tools/` (`ElixirTools` / Expert):

- Constants & release metadata (`EXPERT_VERSION`, `EXPERT_ALLOWED_HOSTS`, `EXPERT_OUTDATED_SHA256S`, `PlatformId` mapping across Linux/macOS/Windows).
- Pure-Rust FIPS 180-4 SHA-256 verification, allowed-host checks, outdated `v0.1.0-rc.6` install cleanup (`remove_outdated_expert_install`), PATH lookup, and injectable runtime dependency setup (`setup_runtime_dependencies`).
- Ignore rules (`is_ignored_dirname`, `is_ignored_path` including `mix.exs` exclusion and gitwildmatch patterns) and symbol name normalization (`normalize_symbol_name`).
- `mix.exs` root/monorepo discovery (`find_mix_exs`), Expert `initialize` params (`create_base_initialize_params`), `$/progress` readiness state machine (`ProgressTracker`), `window/logMessage` level classification, and JSON-RPC stdio startup handshake (`run_expert_startup_handshake`).

## Verification

```sh
rustfmt --edition 2024 --check rust/elixir-tools/src/*.rs rust/elixir-tools/tests/*.rs
cargo test --manifest-path rust/elixir-tools/Cargo.toml --offline
cargo clippy --manifest-path rust/elixir-tools/Cargo.toml --tests --offline -- -D warnings
```
