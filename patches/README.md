# Patches

The changes this POC applies on top of the pinned forks (see
[`../docs/branch-map.md`](../docs/branch-map.md)). `scripts/setup.sh` applies them all.

| File | Target | What it does |
| --- | --- | --- |
| `tokio/src/net/udp_emscripten.rs` | copied into `tokio/tokio/src/net/` | **New file.** A reactor-backed `tokio::net::UdpSocket` for emscripten, mirroring the existing `UnixDatagram` (`ReactorStream` + `with_std::<std::net::UdpSocket>` under `async_io`/`poll_read_io`/`poll_write_io`). Exposes `bind`/`connect`/`send_to`/`recv_from`/`try_*`/`poll_*` — the surface the quinn adapter drives. |
| `tokio-emscripten-udp.patch` | `git apply` in the tokio checkout | Wires the new module into `net/mod.rs` for `target_os = "emscripten"`, and adds `poll_write_io` to the shared `ReactorStream`. |
| `quinn-udp-emscripten-fallback.patch` | `git apply`/`patch -p1` in the quinn-udp crate | Routes `target_os = "emscripten"` to quinn-udp's `fallback.rs` stub (its `unix.rs` needs `pktinfo` cmsg structs emscripten lacks) and fixes a latent `send()` return-type bug. Never actually used — the demo passes quinn an abstract socket. |
| `workers-rs-workspace-Cargo.toml.patch` | `git apply` in the workers-rs checkout | Adds the `quinn-udp` patch entry and drops the goose-only patches/example so the workspace resolves minimally. |

### The one code gap this closes

Guy's `tokio@emscripten` branch ships `TcpStream` and `UnixDatagram` over the emscripten
epoll reactor, but gates `UdpSocket` **off** (`cfg_net_not_emscripten!`). The emscripten
C/JS layer already has UDP (`node:dgram` via `-sNODERAWSOCKETS`); the new
`udp_emscripten.rs` simply surfaces it as an async `tokio::net::UdpSocket`. QUIC then
rides on top with no further platform work.
