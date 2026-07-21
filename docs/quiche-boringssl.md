# quiche + BoringSSL on `wasm32-unknown-emscripten`

The quiche demo uses Cloudflare's `tokio-quiche` crate, which drives `quiche` over a Tokio
`UdpSocket`, and its normal `boring` dependency. Cargo builds `boring-sys` and its bundled
BoringSSL; there is no manual or setup-time BoringSSL build.

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
The demo pins quiche `0.29.3`, which includes that fix.

## Tokio adapter compatibility

`tokio-quiche 0.19.1` is used with all default features, including qlog gzip and zstd.
Two small target adaptations are applied to its pinned quiche workspace dependencies:

- `datagram-socket` excludes `tokio::net::UnixDatagram` on Emscripten, where Tokio does
  not provide Unix-domain datagram sockets. Its UDP support remains unchanged.
- quiche builds only its Rust library. The unused `staticlib` and `cdylib` C ABI outputs
  otherwise perform intermediate side-module links that require every qlog compression
  dependency to be PIC.

Quanta, pulled in by Foundations telemetry, uses its existing POSIX monotonic clock on
Emscripten through a separate small patch.

## Remaining limitations

- The demo uses single-datagram UDP without GSO/GRO.
- Certificate verification is disabled with `verify_peer(false)` for this transport PoC.
