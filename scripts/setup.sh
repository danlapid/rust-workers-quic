#!/usr/bin/env bash
# One-time setup: clone the pinned forks, apply this repo's patches, configure the
# emscripten `cf` toolchain, build the patched wasm-bindgen CLI, and compile
# BoringSSL to wasm (for the quiche demo).
#
# No configuration needed — just:  bash scripts/setup.sh
# Idempotent-ish: safe to re-run; it skips clones that already exist.
# Requires: git, rustup, python3, node, and Homebrew with `brew install emscripten llvm cmake`.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"   # git-ignored: forks + toolchain clone here
mkdir -p "$WORK"

# Pinned refs — see docs/branch-map.md.
EMSCRIPTEN_REF="cf"
WORKERS_RS_REF="emscripten"
GB="https://github.com/guybedford"

clone() { # url ref dir
  local url="$1" ref="$2" dir="$3"
  if [ -d "$dir/.git" ]; then echo "  exists: $dir"; else
    git clone --filter=blob:none --branch "$ref" --single-branch "$url" "$dir"
  fi
}

echo "==> Cloning emscripten ($EMSCRIPTEN_REF) and workers-rs ($WORKERS_RS_REF)"
clone "$GB/emscripten"  "$EMSCRIPTEN_REF"  "$WORK/emscripten"
clone "$GB/workers-rs"  "$WORKERS_RS_REF"  "$WORK/workers-rs"

echo "==> Populating core workers-rs submodules from guybedford forks"
WR="$WORK/workers-rs"
( cd "$WR"
  for pair in \
    "wasm-bindgen:$GB/wasm-bindgen" "ts-gen:$GB/ts-gen" "tokio:$GB/tokio" \
    "libc:$GB/libc" "ring:$GB/ring"; do
    # Split on the FIRST colon only: the URL contains colons (https://), so a
    # greedy `##*:` would strip the scheme and yield `//github.com/...`.
    name="${pair%%:*}"; url="${pair#*:}"
    git config -f .gitmodules "submodule.$name.url" "$url"
  done
  git submodule sync >/dev/null
  for m in wasm-bindgen tokio libc ring ts-gen; do
    git -c protocol.version=2 submodule update --init --filter=blob:none "$m"
  done
)

echo "==> Applying tokio patch (adds the emscripten UdpSocket module + wiring)"
( cd "$WR/tokio" && git apply "$REPO/patches/tokio-emscripten-udp.patch" || \
  echo "  (tokio patch may already be applied)" )

echo "==> Applying libc patch (adds emscripten in6_pktinfo)"
# Lets stock quinn-udp's portable unix.rs compile unmodified — see the patch header.
( cd "$WR/libc" && git apply "$REPO/patches/libc-emscripten-in6_pktinfo.patch" || \
  echo "  (libc patch may already be applied)" )

echo "==> Applying workers-rs workspace patch (drops goose-only deps/example)"
( cd "$WR" && git apply "$REPO/patches/workers-rs-workspace-Cargo.toml.patch" || \
  echo "  (workspace patch may already be applied)" )

echo "==> Configuring emscripten cf toolchain"
CF="$WORK/emscripten"
( cd "$CF" && npm install --no-audit --no-fund >/dev/null && python3 bootstrap.py >/dev/null )

# The emscripten *frontend* (emcc + the epoll/UDP/DNS/wasm-bindgen JS libs) is the
# guybedford `cf` fork cloned above — that IS the "patched emscripten". It has no
# compiler backend, so we borrow LLVM/binaryen/node from Homebrew's emscripten
# (proven: the 6.0.1 backend) and point the fork's emcc at it. (emsdk is NOT used:
# it has no installable prebuilt backend for 6.0.x on Apple Silicon.)
EM_PREFIX="$(brew --prefix emscripten)" || { echo "error: run 'brew install emscripten'"; exit 1; }
cat > "$CF/.emscripten_cf" <<EOF
LLVM_ROOT='$EM_PREFIX/libexec/llvm/bin'
BINARYEN_ROOT='$EM_PREFIX/libexec/binaryen'
NODE_JS='$(command -v node)'
EOF
echo "  wrote $CF/.emscripten_cf"

echo "==> Building the patched wasm-bindgen CLI (host)"
( cd "$WR/wasm-bindgen" && cargo build --release -p wasm-bindgen-cli )

# ---------------------------------------------------------------------------
# quiche demo: clone cloudflare/boring + cloudflare/quiche at the pinned commits,
# apply this repo's patches, and compile BoringSSL to wasm.
# ---------------------------------------------------------------------------
BORING_COMMIT="931385ed41df448c82d150219f69af56e3c8399f"   # cloudflare/boring (~5.2.0)
QUICHE_COMMIT="c4c0b978461aa153399a90217d85bebd1800f84d"   # cloudflare/quiche 0.29.2-3

echo "==> Cloning cloudflare/boring @ $BORING_COMMIT (+ BoringSSL submodule)"
BORING="$WORK/boring"
if [ ! -d "$BORING/.git" ]; then
  git clone --filter=blob:none "https://github.com/cloudflare/boring" "$BORING"
  git -C "$BORING" checkout --detach "$BORING_COMMIT"
  git -C "$BORING" submodule update --init --filter=blob:none boring-sys/deps/boringssl
else
  echo "  exists: $BORING"
fi

echo "==> Cloning cloudflare/quiche @ $QUICHE_COMMIT"
QUICHE="$WORK/quiche"
if [ ! -d "$QUICHE/.git" ]; then
  git clone --filter=blob:none "https://github.com/cloudflare/quiche" "$QUICHE"
  git -C "$QUICHE" checkout --detach "$QUICHE_COMMIT"
else
  echo "  exists: $QUICHE"
fi

echo "==> Applying quiche patches (local boring path + the -> c_void FFI fix)"
# Point quiche's `boring` workspace dep at the local checkout, drop the
# staticlib/cdylib crate-types, and fix the wasm-only `-> c_void` FFI trap.
git -C "$QUICHE" apply "$REPO/patches/quiche-boringssl/quiche-cargo.patch" \
  || echo "  (quiche-cargo.patch may already be applied)"
git -C "$QUICHE" apply "$REPO/patches/quiche-boringssl/quiche-crypto-cvoid-return.patch" \
  || echo "  (quiche-crypto-cvoid-return.patch may already be applied)"

echo "==> Compiling BoringSSL for wasm32-unknown-emscripten (OPENSSL_NO_ASM)"
BSSL="$BORING/boring-sys/deps/boringssl"
if [ -f "$BSSL/build-wasm/libcrypto.a" ] && [ -f "$BSSL/build-wasm/libssl.a" ]; then
  echo "  exists: $BSSL/build-wasm/{libcrypto,libssl}.a"
else
  ( cd "$BSSL" \
    && EM_CONFIG="$CF/.emscripten_cf" PATH="$CF:$PATH" emcmake cmake -G "Unix Makefiles" \
         -B build-wasm -DOPENSSL_NO_ASM=ON -DBUILD_SHARED_LIBS=OFF \
         -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_FLAGS=-Wno-error -DCMAKE_CXX_FLAGS=-Wno-error \
    && EM_CONFIG="$CF/.emscripten_cf" PATH="$CF:$PATH" cmake --build build-wasm --target crypto ssl -j4 )
fi

echo ""
echo "Setup complete. Run a demo:"
echo "  ./scripts/run-quinn_h3.sh   (quinn — rustls + ring)"
echo "  ./scripts/run-quiche.sh     (quiche — BoringSSL)"
