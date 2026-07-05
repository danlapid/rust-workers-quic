#!/usr/bin/env bash
# Build the quiche QUIC/HTTP-3 demo (demos/quiche) for wasm32-unknown-emscripten
# and run it under Node. Like run-quinn_h3.sh it builds inside the patched
# workers-rs workspace; it additionally uses the BoringSSL-for-wasm that setup.sh
# compiled, plus a libclang for boring-sys bindgen (see patches/quiche-boringssl/).
#
# Run scripts/setup.sh once first. No configuration needed — just:  bash scripts/run-quiche.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"
WR="$WORK/workers-rs"
CF="$WORK/emscripten"
WB="$WR/wasm-bindgen/target/release"
BSSL="$WORK/boring/boring-sys/deps/boringssl"
BW="$BSSL/build-wasm"
QUICHE="$WORK/quiche"
LLVM="$(brew --prefix llvm)"   # libclang for boring-sys bindgen

# clang resource dir (headers for the wasm target), if this LLVM provides one.
RES="$("$LLVM/bin/clang" -print-resource-dir 2>/dev/null || true)"

# Sync the quiche demo crate into the workspace as a member. Copy files explicitly
# (not `cp -R dir dir`, which on BSD nests into an existing target dir).
SRC="$REPO/demos/quiche"
DEST="$WR/examples/quiche-demo"
mkdir -p "$DEST/src" "$DEST/.cargo"
cp "$SRC/Cargo.toml" "$DEST/Cargo.toml"
cp "$SRC/src/main.rs" "$DEST/src/main.rs"
cp "$SRC/.cargo/config.toml" "$DEST/.cargo/config.toml"
cp "$SRC/run_quiche.mjs" "$DEST/run_quiche.mjs"
# The workspace copy lives elsewhere, so rewrite the relative quiche path dep to
# the absolute vendored checkout.
sed -i.bak "s#\.\./\.\./vendor/quiche/quiche#$QUICHE/quiche#" "$DEST/Cargo.toml" && rm -f "$DEST/Cargo.toml.bak"

export EM_CONFIG="$CF/.emscripten_cf"
export PATH="$CF:$WB:$PATH"
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$CF/emcc"
# boring-sys: use the prebuilt BoringSSL + the bindgen visibility/sysroot fix
# (see patches/quiche-boringssl/README.md for why `-fvisibility=default`).
export BORING_BSSL_PATH="$BW"
export BORING_BSSL_INCLUDE_PATH="$BSSL/include"
export LIBCLANG_PATH="$LLVM/lib"
export BINDGEN_EXTRA_CLANG_ARGS="-target wasm32-unknown-emscripten -fvisibility=default --sysroot=$CF/cache/sysroot${RES:+ -resource-dir $RES -isystem $RES/include} -isystem $CF/cache/sysroot/include/wasm32-emscripten -isystem $CF/cache/sysroot/include"

echo "==> cargo build -p quiche-demo --target wasm32-unknown-emscripten --release"
( cd "$DEST" && cargo build -p quiche-demo --target wasm32-unknown-emscripten --release )

OUT="$WR/target/wasm32-unknown-emscripten/release"
cp "$DEST/run_quiche.mjs" "$OUT/"
echo "==> node run_quiche.mjs"
( cd "$OUT" && node run_quiche.mjs )
