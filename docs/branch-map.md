# Dependency branch / commit map

Every non-crates.io dependency is pinned to a specific fork + branch + commit. These are
the versions this POC was proven against; `scripts/setup.sh` clones them.

## Toolchain

| Component | Source | Branch | Commit (pinned) | Notes |
| --- | --- | --- | --- | --- |
| emscripten | `guybedford/emscripten` | `cf` | `e06aa29a8` (`6.0.3-git`) | epoll (`libepoll.js`, `emscripten_epoll_set_callback`), async DNS (`emscripten_dns_lookup_async`), UDP/TCP over `-sNODERAWSOCKETS`, `-sWASM_BINDGEN`. |
| emsdk | `emscripten-core/emsdk` | default | latest | Provides the LLVM/binaryen/node backend. This POC reused a Homebrew-installed LLVM 6.0.1 backend instead of a full emsdk install (see `scripts/setup.sh`). |
| Rust | rustup stable | — | `1.95.0` | `rust-toolchain.toml` in workers-rs pins this; `wasm32-unknown-emscripten` target. |

## Rust dependency forks (pinned via `[patch.crates-io]`)

| Crate | Source | Branch | Commit | Why |
| --- | --- | --- | --- | --- |
| workers-rs (workspace) | `guybedford/workers-rs` | `emscripten` | — | Monorepo that pins the rest via submodules + patch table; hosts the examples. |
| tokio | `guybedford/tokio` | `emscripten` | `4060fe16508df0503518b5aefd525a2becc79ff2` | Emscripten reactor (JSPI-parked `block_on`, host event loop), `TcpStream`, async `lookup_host`. **We add `UdpSocket`** (see `patches/`). |
| ring | `guybedford/ring` | `emscripten` | `6671f7cfbb13f249b571ffa6326275a8596e0ca2` | getrandom-backed `SystemRandom`; C core via no-asm fallback. rustls crypto backend. |
| libc | `guybedford/libc` | `libc-0.2-emscripten` | `016d45207b895a6032366815f92586570ceae917` | Emscripten decls. |
| wasm-bindgen | `guybedford/wasm-bindgen` | `emscripten-non-identifier-names` | `4b69f3b3ba4212c857be6854f77fa5aec8b62871` | `#[wasm_bindgen(tokio)]` (drives async exports on tokio's emscripten event loop) + emscripten descriptor-interpreter fixes. Provides the `wasm-bindgen` CLI (`0.2.126`) used by `-sWASM_BINDGEN=auto`. |
| ts-gen | `guybedford/ts-gen` | `main` | `b698be710dd2f96e813796d9bac6ebb089c90f0f` | TypeScript binding generation. |
| socket2 | `rust-lang/socket2` | `master` (upstream) | — | Emscripten support (unreleased upstream); patched in via git. |
| quinn-udp | crates.io `0.5.15` | — | — | **Patched** (`patches/quinn-udp-emscripten-fallback.patch`): route `target_os=emscripten` to its `fallback.rs` stub (its `unix.rs` needs `pktinfo` cmsg structs emscripten's libc lacks). Never actually used — we pass quinn an abstract socket. |

## Not needed for the QUIC POC

`arboard`, `fs2-rs`, `sys-info-rs`, `tree-sitter` submodules are only pulled in by the
unrelated `emscripten-goose` example; they are excluded from the workspace here (see
`patches/workers-rs-workspace-Cargo.toml.patch`).
