#!/usr/bin/env bash
# Build the quinn demo (demos/quinn_h3) for wasm32-unknown-emscripten and run it
# under Node. The crate builds inside the patched workers-rs workspace (for the
# wasm-bindgen fork + [patch.crates-io]), so we copy it in first.
#
# Run scripts/setup.sh once first. No configuration needed — just:  bash scripts/run-quinn_h3.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"
WR="$WORK/workers-rs"
CF="$WORK/emscripten"
WB="$WR/wasm-bindgen/target/release"

# Sync the demo crate into the workspace as a member. Copy files explicitly (not
# `cp -R dir dir`, which on BSD nests into an existing target dir).
SRC="$REPO/demos/quinn_h3"
DEST="$WR/examples/quic-demo"
mkdir -p "$DEST/src" "$DEST/.cargo"
cp "$SRC/Cargo.toml" "$DEST/Cargo.toml"
cp "$SRC/src/main.rs" "$DEST/src/main.rs"
cp "$SRC/.cargo/config.toml" "$DEST/.cargo/config.toml"
cp "$SRC/run_quic.mjs" "$DEST/run_quic.mjs"

export EM_CONFIG="$CF/.emscripten_cf"
export PATH="$CF:$WB:$PATH"
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$CF/emcc"

echo "==> cargo build -p quic-demo --target wasm32-unknown-emscripten --release"
( cd "$DEST" && cargo build -p quic-demo --target wasm32-unknown-emscripten --release )

OUT="$WR/target/wasm32-unknown-emscripten/release"
cp "$DEST/run_quic.mjs" "$OUT/"
echo "==> node run_quic.mjs"
( cd "$OUT" && node run_quic.mjs )
