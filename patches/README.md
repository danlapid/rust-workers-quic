# Patches

The changes this POC applies on top of the pinned forks (see
[`../docs/branch-map.md`](../docs/branch-map.md)). `scripts/setup.sh` applies them all.

| File | Target | What it does |
| --- | --- | --- |
| `tokio-emscripten-udp.patch` | `git apply` in the tokio checkout | **Creates** the new `tokio/src/net/udp_emscripten.rs` module (a `new file` hunk) **and** wires it in: registers the module in `net/mod.rs` for `target_os = "emscripten"` and adds `poll_write_io` to the shared `ReactorStream`. The module is a reactor-backed `tokio::net::UdpSocket` mirroring the existing `UnixDatagram` (`ReactorStream` + `with_std::<std::net::UdpSocket>` under `async_io`/`poll_read_io`/`poll_write_io`), exposing `bind`/`connect`/`send_to`/`recv_from`/`try_*`/`poll_*` — the surface the quinn adapter drives. |
| `libc-emscripten-in6_pktinfo.patch` | `git apply` in the `libc` submodule (`workers-rs/libc`) | Adds the `in6_pktinfo` struct to the emscripten `libc` module (musl defines it; `in_pktinfo` was already inherited from `linux_like`). |
| `workers-rs-workspace-Cargo.toml.patch` | `git apply` in the workers-rs checkout | Detaches the unrelated `emscripten-goose` example (excludes the member + drops its `arboard`/`fs2`/`sys-info`/`tree-sitter` workspace deps). Required, not cosmetic: goose's `tree-sitter` submodule fork (`guybedford/tree-sitter`) no longer exists so it can't be cloned, and cargo eagerly validates workspace-dependency path sources. The QUIC crates don't use goose. |

### The one code gap this closes

Guy's `tokio@emscripten` branch ships `TcpStream` and `UnixDatagram` over the emscripten
epoll reactor, but gates `UdpSocket` **off** (`cfg_net_not_emscripten!`). The emscripten
C/JS layer already has UDP (`node:dgram` via `-sNODERAWSOCKETS`); the new
`udp_emscripten.rs` simply surfaces it as an async `tokio::net::UdpSocket`. QUIC then
rides on top with no further platform work.
