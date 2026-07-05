#!/usr/bin/env bash
# Build the QUIC demo (this repo's root crate) for wasm32-unknown-emscripten and
# run it under Node. The crate builds inside the patched workers-rs workspace
# (for the wasm-bindgen fork + [patch.crates-io]), so we copy it in first.
set -euo pipefail

: "${RWQ_REPO:?source scripts/env.sh first}"
: "${RWQ_WORK:?source scripts/env.sh first}"

WR="$RWQ_WORK/workers-rs"
CF="$RWQ_WORK/emscripten"
WB="$WR/wasm-bindgen/target/release"

# Sync the root crate into the workspace as a member.
DEST="$WR/examples/quic-demo"
mkdir -p "$DEST"
cp "$RWQ_REPO/Cargo.toml" "$DEST/Cargo.toml"
cp -R "$RWQ_REPO/src" "$DEST/src"
cp -R "$RWQ_REPO/.cargo" "$DEST/.cargo"
cp "$RWQ_REPO/run_quic.mjs" "$DEST/run_quic.mjs"

export EM_CONFIG="$CF/.emscripten_cf"
export PATH="$CF:$WB:$PATH"
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$CF/emcc"

echo "==> cargo build -p quic-demo --target wasm32-unknown-emscripten --release"
( cd "$DEST" && cargo build -p quic-demo --target wasm32-unknown-emscripten --release )

OUT="$WR/target/wasm32-unknown-emscripten/release"
cp "$DEST/run_quic.mjs" "$OUT/"
echo "==> ${RWQ_NODE:-node} run_quic.mjs"
( cd "$OUT" && "${RWQ_NODE:-node}" run_quic.mjs )
