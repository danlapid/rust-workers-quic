# Dependency Sources And Pins

This lists the nonstandard dependency sources and noteworthy registry versions used by
the PoC. Direct checkouts are pinned in `scripts/setup.sh`, Cargo sources in `Cargo.toml`
and `Cargo.lock`, and Rust in `rust-toolchain.toml`.

## Toolchain

| Component | Source | Revision | Provisioning | Notes |
| --- | --- | --- | --- | --- |
| emscripten frontend | `guybedford/emscripten` branch `cf` | `08a1fc28569c93a86f9e737bac508332a298615e` (`6.0.3-git`) | Direct clone to `.work/emscripten` | Integrated epoll, async DNS, UDP/TCP over `-sNODERAWSOCKETS`, wasm-bindgen, and blocking socket operations. |
| emscripten backend | Homebrew `emscripten` | Auto-detected (proven: `6.0.1`) | `brew --prefix emscripten` | Supplies LLVM, binaryen, and Node support to the fork frontend; it is an external host prerequisite rather than a source pin. |
| Rust | rustup `nightly-2026-07-20` | `1.99.0-nightly` (`9f36de775`) | `rust-toolchain.toml` | Includes the Emscripten fd-cloning support needed by unmodified mio. |

## Direct Checkouts

| Crate | Source branch | Commit | Local change |
| --- | --- | --- | --- |
| wasm-bindgen | `guybedford/wasm-bindgen@emscripten-non-identifier-names` | `4b69f3b3ba4212c857be6854f77fa5aec8b62871` | `patches/wasm-bindgen-tokio-hosted-runtime.patch` updates the async export bridge to Tokio's public `HostedRuntime` API. |
| tokio | `guybedford/tokio@emscripten-layering` | `7c1d4977c510866775ed6164b58b2218a6a2955b` | None. Provides the hosted runtime and standard networking over Emscripten mio, including `UdpSocket`. |
| libc | `guybedford/libc@libc-0.2-emscripten` | `29d4451facf22c9dcc25e3e3f3bc2ba827b278f8` | None. Includes epoll and `in6_pktinfo`. |
| ring | `guybedford/ring@emscripten` | `6671f7cfbb13f249b571ffa6326275a8596e0ca2` | None. Provides getrandom-backed `SystemRandom` and the no-assembly C fallback. |
| quanta | `metrics-rs/quanta@v0.12.6` | `0f81349c223854136113e634cf8dd6cd85569880` | `patches/quanta-emscripten-clock.patch` selects the existing POSIX monotonic clock on Emscripten. |
| quiche workspace | `cloudflare/quiche@0.29.3` | `55886df3be579579207104c8e645825b6347a209` | `patches/tokio-quiche-emscripten.patch` excludes unavailable Unix-domain datagrams and unused C ABI crate types. Cargo uses its `quiche`, `qlog`, `octets`, and `datagram-socket` crates. |

## Cargo Git Sources

| Crate | Source branch | Commit | Why |
| --- | --- | --- | --- |
| mio | `guybedford/mio@emscripten` | `4c43fd5522f5b13a5bc2d4b54bd7f50e698084d1` | Emscripten epoll backend used by Tokio's standard TCP and UDP types. |
| socket2 | `rust-lang/socket2@master` | `239dd83a4ced08e514d2c38942aab99791119f0d` | Contains unreleased Emscripten support. |

## Noteworthy Registry Dependencies

| Crate | Version | Notes |
| --- | --- | --- |
| `boring` / `boring-sys` | `4.22.0` | quiche's published dependency. `boring-sys` compiles bundled BoringSSL using `cmake/boringssl-emscripten.cmake`; no source patch or prebuild is used. |
| `tokio-quiche` | `0.19.1` | Official Tokio socket and HTTP/3 driver, with default qlog gzip/zstd features enabled. |
| `quinn-udp` | `0.5.15` | Stock and unmodified. It compiles with Guy's libc but is unused at runtime because the demo passes quinn an abstract single-datagram socket. |
