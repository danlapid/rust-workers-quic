# quiche + BoringSSL on `wasm32-unknown-emscripten`

The quiche demo uses Cloudflare's `quiche` crate and its normal `boring` dependency.
Cargo builds `boring-sys` and its bundled BoringSSL; there is no manual or setup-time
BoringSSL build.

`scripts/run-quiche.sh` provides the two pieces of target configuration that upstream
build scripts do not infer:

- `cmake/boringssl-emscripten.cmake` sets `OPENSSL_NO_ASM` and loads Emscripten's CMake
  toolchain. BoringSSL has no WebAssembly assembly implementation.
- `BINDGEN_EXTRA_CLANG_ARGS` supplies the Emscripten target, sysroot, and
  `-fvisibility=default` to `boring-sys` bindgen.

## Why the visibility flag is needed

Emscripten's clang defaults to hidden symbol visibility. BoringSSL's `OPENSSL_EXPORT`
macro expands to empty, so libclang reports its functions with hidden visibility and
bindgen omits them. `-fvisibility=default` makes bindgen emit the functions. No bindgen
or `boring-sys` source patch is needed.

## Upstream quiche fix

quiche previously declared the C `void` functions `AES_ecb_encrypt` and
`CRYPTO_chacha_20` as returning Rust `c_void`. Native ABIs tolerated the mismatch, but
WebAssembly includes the result in the function type and trapped at runtime. Upstream
[PR #2535](https://github.com/cloudflare/quiche/pull/2535) corrected the declarations.
The demo pins commit `68c23b9dd16068c5b77fbb4d232c92e8bd23505e`, which includes that fix, so no local
quiche patch is needed.

## Remaining limitations

- The demo uses single-datagram UDP without GSO/GRO.
- Certificate verification is disabled with `verify_peer(false)` for this transport PoC.
