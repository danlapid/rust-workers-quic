#!/usr/bin/env bash
# Build the quiche QUIC/HTTP-3 demo (demos/quiche) for wasm32-unknown-emscripten
# and run it under Node. Cargo builds boring-sys's bundled BoringSSL; this script
# supplies its Emscripten CMake and bindgen configuration.
#
# Run scripts/setup.sh once first. No configuration needed — just:  bash scripts/run-quiche.sh
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"
CF="$WORK/emscripten"
WB="$WORK/wasm-bindgen/target/release"
LLVM="$(brew --prefix llvm)"   # libclang for boring-sys bindgen

# clang resource dir (headers for the wasm target), if this LLVM provides one.
RES="$("$LLVM/bin/clang" -print-resource-dir 2>/dev/null || true)"

SRC="$REPO/demos/quiche"
export EM_CONFIG="$CF/.emscripten_cf"
export EMSCRIPTEN="$CF"
export PATH="$CF:$WB:$PATH"
export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER="$CF/emcc"
# BoringSSL has no wasm assembly implementation. The wrapper selects its C
# implementation and then loads Emscripten's standard CMake toolchain.
export CMAKE_TOOLCHAIN_FILE="$REPO/cmake/boringssl-emscripten.cmake"
export LIBCLANG_PATH="$LLVM/lib"
export BINDGEN_EXTRA_CLANG_ARGS="-target wasm32-unknown-emscripten -fvisibility=default --sysroot=$CF/cache/sysroot${RES:+ -resource-dir $RES -isystem $RES/include} -isystem $CF/cache/sysroot/include/wasm32-emscripten -isystem $CF/cache/sysroot/include"

echo "==> cargo build -p quiche-demo --target wasm32-unknown-emscripten --release"
( cd "$SRC" && cargo build -p quiche-demo --target wasm32-unknown-emscripten --release )

OUT="$REPO/target/wasm32-unknown-emscripten/release"
cp "$SRC/run_quiche.mjs" "$OUT/"
echo "==> node run_quiche.mjs"
( cd "$OUT" && node run_quiche.mjs )
