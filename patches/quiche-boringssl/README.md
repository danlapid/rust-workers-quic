# quiche + BoringSSL on `wasm32-unknown-emscripten`

This captures the (experimental) work making **quiche** — Cloudflare's own QUIC/HTTP-3
library, which uses **BoringSSL** via the `boring` crate — build and run on
`wasm32-unknown-emscripten` under Node.js.

## Status

- ✅ **BoringSSL compiles to wasm** (`libcrypto.a` + `libssl.a`, `OPENSSL_NO_ASM`) via `emcmake`.
- ✅ **The `boring` crate builds and runs on Node** — `sha256` correct, and **`rand_bytes`
  works**, i.e. BoringSSL's RNG runtime (`getentropy` → JS crypto) works on emscripten.
- ✅ **quiche 0.29.2 builds and runs on Node** — compiled against `boring` 5.2 with **no API
  changes**.
- ✅ **Full QUIC + HTTP/3 GET on Node** — `quiche-demo/` (a second root demo) does a real
  QUIC/TLS-1.3 handshake and an `quiche::h3` GET to `cloudflare-quic.com` over the emscripten
  `tokio::net::UdpSocket`: **`status=200`, ~126 KB body, ~30 ms RTT**. Same headline result
  as the quinn demo, now on Cloudflare's own stack.

## The key discovery (and why no bindgen patch is needed)

`boring-sys` runs bindgen at build time. On wasm, **bindgen emitted every BoringSSL *type*
but *zero functions*** — because emscripten's clang defaults to **hidden** symbol
visibility, so BoringSSL's functions (its `OPENSSL_EXPORT` macro expands to empty) parse as
`CXVisibility_Hidden`, and bindgen **drops non-`Default`-visibility functions**
(`bindgen/ir/function.rs::Function::parse`).

**Fix: pass `-fvisibility=default` to bindgen's clang args.** One flag; no bindgen patch.
(Confirmed with a minimal repro: with `-target wasm32-*` bindgen emits 0 functions from a
3-function header; adding `-fvisibility=default` emits all of them.)

Note: bindgen also drops functions for *all* wasm targets if you rely on it to infer the
calling convention — but the visibility flag is the actual root cause here. A layout-safe
fallback (only if needed) is to run bindgen with `-target armv7-unknown-linux-gnueabihf`,
which is ILP32 with 8-byte `i64`/`f64` alignment — **byte-identical struct layouts to
wasm32** (verified). `i686` is *not* safe (4-byte alignment).

## Reproduce (verified, env-var recipe)

1. Build BoringSSL for wasm (no source changes needed):
   ```sh
   cd <boring-sys>/deps/boringssl
   emcmake cmake -G "Unix Makefiles" -B build-wasm \
       -DOPENSSL_NO_ASM=ON -DBUILD_SHARED_LIBS=OFF -DCMAKE_BUILD_TYPE=Release \
       -DCMAKE_C_FLAGS="-Wno-error" -DCMAKE_CXX_FLAGS="-Wno-error"
   cmake --build build-wasm --target crypto ssl -j4
   ```

2. Build the `boring` crate (and quiche) with these env vars (point boring-sys at the
   prebuilt libs + fix bindgen):
   ```sh
   export CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER=<emscripten>/emcc
   export EM_CONFIG=<emscripten>/.emscripten_cf
   export BORING_BSSL_PATH=<boringssl>/build-wasm          # has libcrypto.a/libssl.a
   export BORING_BSSL_INCLUDE_PATH=<boringssl>/include
   export LIBCLANG_PATH=<llvm>/lib                          # a libclang.dylib (e.g. Homebrew llvm)
   export BINDGEN_EXTRA_CLANG_ARGS="-target wasm32-unknown-emscripten -fvisibility=default \
       --sysroot=<emscripten>/cache/sysroot \
       -isystem <emscripten>/cache/sysroot/include/wasm32-emscripten \
       -isystem <emscripten>/cache/sysroot/include"
   # link: -Crelocation-model=static, and DO NOT set -sEXIT_RUNTIME (conflicts with cdylib)
   ```

The repeatable scripts live in `vendor/` (git-ignored): `build_boringssl_wasm.sh`,
`build_boring_smoke.sh`, `build_quiche_smoke.sh`.

## The second bug: a wasm-only `-> c_void` FFI trap in quiche

With everything linked, the demo built and ran up to the first `conn.send()` and then hit a
hard wasm trap (`RuntimeError: unreachable`, escaping `catch_unwind`, so *not* a Rust panic)
inside `quiche::crypto::boringssl::HeaderProtectionKey::new_mask` — QUIC Initial-packet
header protection, the first AES call.

Cause: quiche declares two BoringSSL functions with **`-> c_void`**:

```rust
fn AES_ecb_encrypt(...) -> c_void;   // BoringSSL: returns C `void`
fn CRYPTO_chacha_20(...) -> c_void;
```

Both C functions return `void` (a wasm function type with **no** result), but `-> c_void`
makes Rust expect a returned value. On native ELF this mismatch is silently harmless (the
unused return register is ignored), which is why upstream never caught it — but **wasm
type-checks the full function signature (including results) at call time**, so the call
traps. Fix: declare them returning `()` (i.e. drop `-> c_void`). See
`quiche-crypto-cvoid-return.patch`. This is the only quiche *source* change needed; it's a
real, upstreamable wasm-correctness fix.

## Patches in this dir

- `quiche-cargo.patch` — point quiche at the local `boring` 5.2 and drop the `staticlib`/
  `cdylib` crate-types (they try to link as standalone reactors with no `main`).
- `quiche-crypto-cvoid-return.patch` — the `-> c_void` → `()` FFI fix above (the runtime bug).
- `boring-sys-emscripten-bindgen.patch` — proposed clean `boring-sys` `build.rs` arm so
  `boring` "just works" on emscripten without the env vars above (the upstreamable form of
  the `-fvisibility=default` + target/sysroot fix). Derived from the verified env recipe.

## Known rough edges

- Single-datagram UDP only (no GSO/GRO), like the quinn demo — a perf follow-up.
- Cert verification is disabled (`verify_peer(false)`) in the demo — BoringSSL still runs the
  full TLS 1.3 handshake + key schedule; only the trust-anchor path is skipped. PoC only.
- The `boring`/BoringSSL used is 5.2's bundled BoringSSL, unpatched (boring-sys skips its
  post-quantum/RPK patches when given a prebuilt). Fine for the PoC.
