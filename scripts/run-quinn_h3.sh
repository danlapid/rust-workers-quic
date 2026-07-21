#!/usr/bin/env bash
# Build the quinn demo (demos/quinn_h3) for wasm32-unknown-emscripten and run it
# under Node. The root Cargo workspace references the pinned forks provisioned
# under .work by setup.sh; only wasm-bindgen has a local source patch.
#
# Run scripts/setup.sh once first. No configuration needed — just:  bash scripts/run-quinn_h3.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"
CF="$WORK/emscripten"
WB="$WORK/wasm-bindgen/target/release"

SRC="$REPO/demos/quinn_h3"

export EM_CONFIG="$CF/.emscripten_cf"
export PATH="$CF:$WB:$PATH"
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$CF/emcc"

echo "==> cargo build -p quic-demo --target wasm32-unknown-emscripten --release"
( cd "$SRC" && cargo build -p quic-demo --target wasm32-unknown-emscripten --release )

OUT="$REPO/target/wasm32-unknown-emscripten/release"
cp "$SRC/run_quic.mjs" "$OUT/"
echo "==> node run_quic.mjs"
( cd "$OUT" && node run_quic.mjs )
