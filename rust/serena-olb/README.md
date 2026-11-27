# serena-olb

Pure-Rust port of `src/serena/olb/olb_jvm_backend.py` (`OraiosJVMLanguageBackend`):

- `OLB_JVM_BACKEND_KEY` (`"olb-jvm"`) and `OLB_LANGUAGE_BACKEND` (`"jvm"`).
- `find_olb_in_path` / `check_olb_installed` with actionable install error matching Python.
- `adapt_ide_facade` (`"ide"`, `"JetBrains"` -> `"Oraios"`).
- `launch_command` / `stop_command` and bounded-timeout `shutdown_active_project` with deadlock-free stdout/stderr draining and `(stderr or stdout).strip()` non-zero output selection.

## Verification

```sh
cargo test --manifest-path rust/serena-olb/Cargo.toml --offline
cargo clippy --manifest-path rust/serena-olb/Cargo.toml --tests --offline -- -D warnings
rustfmt --edition 2024 --check rust/serena-olb/src/*.rs
```
