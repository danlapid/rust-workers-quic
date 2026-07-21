# Patches

`scripts/setup.sh` applies these local dependency changes to pinned checkouts described in
[`../docs/branch-map.md`](../docs/branch-map.md).

| File | Target | What it does |
| --- | --- | --- |
| `emscripten-recvmmsg.patch` | `git apply` in the Emscripten checkout | Implements `recvmmsg` over the existing `recvmsg` socket backend and completes the `msghdr` output fields required by quinn-udp. |
| `wasm-bindgen-tokio-hosted-runtime.patch` | `git apply` in the wasm-bindgen checkout | Updates `#[wasm_bindgen(tokio)]` from Tokio's removed global Emscripten event-loop API to the successor branch's public `HostedRuntime`. |
| `quanta-emscripten-clock.patch` | `git apply` in the quanta checkout | Routes Emscripten through quanta's existing POSIX `clock_gettime` monotonic clock implementation. |
| `tokio-quiche-emscripten.patch` | `git apply` in the quiche checkout | Excludes Tokio's unavailable Unix-domain datagram type on Emscripten and builds quiche as an `rlib` only, avoiding unused C ABI side-module links. |

Direct clone revisions are pinned in `scripts/setup.sh`, Cargo-managed sources in the root
`Cargo.toml` and `Cargo.lock`, and Rust in `rust-toolchain.toml`.
