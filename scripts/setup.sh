#!/usr/bin/env bash
# One-time setup: clone the pinned forks, apply this repo's patches, configure
# the emscripten `cf` toolchain, and build the patched wasm-bindgen CLI.
#
# Idempotent-ish: safe to re-run; it skips clones that already exist.
# Requires: git, rustup, python3, a C toolchain, and (for RWQ_USE_EMSDK=1) the
# ability to download emsdk tools.
set -euo pipefail

: "${RWQ_REPO:?source scripts/env.sh first}"
: "${RWQ_WORK:?source scripts/env.sh first}"
mkdir -p "$RWQ_WORK"

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
clone "$GB/emscripten"  "$EMSCRIPTEN_REF"  "$RWQ_WORK/emscripten"
clone "$GB/workers-rs"  "$WORKERS_RS_REF"  "$RWQ_WORK/workers-rs"

echo "==> Populating core workers-rs submodules from guybedford forks"
WR="$RWQ_WORK/workers-rs"
( cd "$WR"
  for pair in \
    "wasm-bindgen:$GB/wasm-bindgen" "ts-gen:$GB/ts-gen" "tokio:$GB/tokio" \
    "libc:$GB/libc" "ring:$GB/ring"; do
    name="${pair%%:*}"; url="${pair##*:}"
    git config -f .gitmodules "submodule.$name.url" "$url"
  done
  git submodule sync >/dev/null
  for m in wasm-bindgen tokio libc ring ts-gen; do
    git -c protocol.version=2 submodule update --init --filter=blob:none "$m"
  done
)

echo "==> Vendoring + patching quinn-udp 0.5.15"
QUDP="$RWQ_WORK/quinn-udp"
if [ ! -d "$QUDP" ]; then
  # Fetch the crate source via cargo, then copy it out.
  TMP="$(mktemp -d)"; ( cd "$TMP" && cargo new --lib _q >/dev/null 2>&1 \
    && cd _q && cargo add quinn-udp@=0.5.15 >/dev/null 2>&1 && cargo fetch >/dev/null 2>&1 )
  SRC="$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 1 -type d -name 'quinn-udp-0.5.15' | head -1)"
  [ -n "$SRC" ] || { echo "could not locate quinn-udp-0.5.15 source"; exit 1; }
  cp -R "$SRC" "$QUDP"; chmod -R u+w "$QUDP"
  git -C "$QUDP" init -q 2>/dev/null || true
fi
git apply --directory="$QUDP" -p1 "$RWQ_REPO/patches/quinn-udp-emscripten-fallback.patch" 2>/dev/null \
  || ( cd "$QUDP" && patch -p1 < "$RWQ_REPO/patches/quinn-udp-emscripten-fallback.patch" ) || true

echo "==> Applying tokio patch + installing the new UdpSocket module"
cp "$RWQ_REPO/patches/tokio/src/net/udp_emscripten.rs" "$WR/tokio/tokio/src/net/udp_emscripten.rs"
( cd "$WR/tokio" && git apply "$RWQ_REPO/patches/tokio-emscripten-udp.patch" || \
  echo "  (tokio patch may already be applied)" )

echo "==> Applying workers-rs workspace patch (points quinn-udp at $QUDP)"
# Rewrite the patch's relative quinn-udp path to the actual vendored location.
sed "s#\.\./quinn-udp#$QUDP#" "$RWQ_REPO/patches/workers-rs-workspace-Cargo.toml.patch" \
  | ( cd "$WR" && git apply - || echo "  (workspace patch may already be applied)" )

echo "==> Configuring emscripten cf toolchain"
CF="$RWQ_WORK/emscripten"
( cd "$CF" && npm install --no-audit --no-fund >/dev/null && python3 bootstrap.py >/dev/null )

# Point the cf checkout at an LLVM/binaryen/node backend.
if [ "${RWQ_USE_EMSDK:-1}" = "1" ]; then
  clone "https://github.com/emscripten-core/emsdk" "main" "$RWQ_WORK/emsdk" || \
    git clone "https://github.com/emscripten-core/emsdk" "$RWQ_WORK/emsdk" || true
  ( cd "$RWQ_WORK/emsdk" && ./emsdk install tot && ./emsdk activate tot )
  # emsdk writes an .emscripten with LLVM_ROOT/BINARYEN_ROOT/NODE_JS; reuse it.
  cp "$RWQ_WORK/emsdk/.emscripten" "$CF/.emscripten_cf"
else
  : "${RWQ_LLVM_ROOT:?set RWQ_LLVM_ROOT or RWQ_USE_EMSDK=1}"
  : "${RWQ_BINARYEN_ROOT:?set RWQ_BINARYEN_ROOT}"
  : "${RWQ_NODE_JS:?set RWQ_NODE_JS}"
  cat > "$CF/.emscripten_cf" <<EOF
LLVM_ROOT='$RWQ_LLVM_ROOT'
BINARYEN_ROOT='$RWQ_BINARYEN_ROOT'
NODE_JS='$RWQ_NODE_JS'
EOF
fi
echo "  wrote $CF/.emscripten_cf"

echo "==> Building the patched wasm-bindgen CLI (host)"
( cd "$WR/wasm-bindgen" && cargo build --release -p wasm-bindgen-cli )

echo ""
echo "Setup complete. Now: ./scripts/run.sh"
