#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --release --manifest-path "$ROOT/Cargo.toml"

BIN="$ROOT/target/release/codemode"
DEST="${CODEMODE_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST"
install -m 0755 "$BIN" "$DEST/codemode"

echo "Installed codemode to $DEST/codemode"
"$DEST/codemode" doctor
