#!/usr/bin/env bash
# One-time setup: clone the pinned dependencies, apply this repo's patches, configure the
# emscripten `cf` toolchain, and build the patched wasm-bindgen CLI.
#
# No configuration needed — just:  bash scripts/setup.sh
# Idempotent at the selected revisions; clean existing clones are repinned and patches are detected.
# Requires: git, rustup, python3, node, and Homebrew's `emscripten` package.
# The quiche demo additionally needs Homebrew `llvm` and `cmake`.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
WORK="$REPO/.work"   # git-ignored: forks + toolchain clone here
mkdir -p "$WORK"

# Pinned branch heads — see docs/branch-map.md.
EMSCRIPTEN_BRANCH="cf"
EMSCRIPTEN_COMMIT="21166256c4c4d73d39b3685c8973d7cbe427ce8c"
TOKIO_BRANCH="emscripten-layering"
TOKIO_COMMIT="7c1d4977c510866775ed6164b58b2218a6a2955b"
LIBC_BRANCH="libc-0.2-emscripten"
LIBC_COMMIT="4091fe0b0dc5f9c1a27bed75be1ff02bb27e756d"
WASM_BINDGEN_BRANCH="emscripten-non-identifier-names"
WASM_BINDGEN_COMMIT="4b69f3b3ba4212c857be6854f77fa5aec8b62871"
RING_BRANCH="emscripten"
RING_COMMIT="6671f7cfbb13f249b571ffa6326275a8596e0ca2"
QUANTA_BRANCH="main"
QUANTA_COMMIT="bb6ca3f82b0b0cfbd7c04a0221f82e63d06a47ed"
QUICHE_TOKIO_BRANCH="0.29.3"
QUICHE_TOKIO_COMMIT="55886df3be579579207104c8e645825b6347a209"
GB="https://github.com/guybedford"

clone() { # url ref dir
  local url="$1" ref="$2" dir="$3"
  if [ -d "$dir/.git" ]; then echo "  exists: $dir"; else
    git clone --filter=blob:none --branch "$ref" --single-branch "$url" "$dir"
  fi
}

pin() { # dir branch commit
  local dir="$1" branch="$2" commit="$3"
  git -C "$dir" fetch --filter=blob:none origin "$branch"
  git -C "$dir" switch --detach "$commit"
}

apply() { # dir patch
  local dir="$1" patch="$2"
  if git -C "$dir" apply --reverse --check "$patch" 2>/dev/null; then
    echo "  already applied: $(basename "$patch")"
  else
    git -C "$dir" apply --check "$patch"
    git -C "$dir" apply "$patch"
  fi
}

echo "==> Cloning pinned Emscripten and Rust dependency forks"
clone "$GB/emscripten" "$EMSCRIPTEN_BRANCH" "$WORK/emscripten"
clone "$GB/wasm-bindgen" "$WASM_BINDGEN_BRANCH" "$WORK/wasm-bindgen"
clone "$GB/tokio" "$TOKIO_BRANCH" "$WORK/tokio"
clone "$GB/libc" "$LIBC_BRANCH" "$WORK/libc"
clone "$GB/ring" "$RING_BRANCH" "$WORK/ring"
clone "https://github.com/metrics-rs/quanta" "$QUANTA_BRANCH" "$WORK/quanta"
clone "https://github.com/cloudflare/quiche" "$QUICHE_TOKIO_BRANCH" "$WORK/quiche-tokio"
pin "$WORK/emscripten" "$EMSCRIPTEN_BRANCH" "$EMSCRIPTEN_COMMIT"
pin "$WORK/wasm-bindgen" "$WASM_BINDGEN_BRANCH" "$WASM_BINDGEN_COMMIT"
pin "$WORK/tokio" "$TOKIO_BRANCH" "$TOKIO_COMMIT"
pin "$WORK/libc" "$LIBC_BRANCH" "$LIBC_COMMIT"
pin "$WORK/ring" "$RING_BRANCH" "$RING_COMMIT"
pin "$WORK/quanta" "$QUANTA_BRANCH" "$QUANTA_COMMIT"
pin "$WORK/quiche-tokio" "$QUICHE_TOKIO_BRANCH" "$QUICHE_TOKIO_COMMIT"

echo "==> Updating wasm-bindgen's tokio export bridge for HostedRuntime"
apply "$WORK/wasm-bindgen" "$REPO/patches/wasm-bindgen-tokio-hosted-runtime.patch"

echo "==> Adapting tokio-quiche dependencies for Emscripten"
apply "$WORK/quiche-tokio" "$REPO/patches/tokio-quiche-emscripten.patch"

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
( cd "$WORK/wasm-bindgen" && cargo build --release -p wasm-bindgen-cli )

echo ""
echo "Setup complete. Run a demo:"
echo "  ./scripts/run-quinn_h3.sh   (quinn — rustls + ring)"
echo "  ./scripts/run-quiche.sh     (quiche — BoringSSL)"
