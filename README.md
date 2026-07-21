# rust-workers-quic

[![demos](https://github.com/danlapid/rust-workers-quic/actions/workflows/ci.yml/badge.svg)](https://github.com/danlapid/rust-workers-quic/actions/workflows/ci.yml)

**Proof that Rust compiled to `wasm32-unknown-emscripten` can make a real HTTP/3
(QUIC) request to the public internet using `tokio` sockets — running on Node.js.**

```
HTTP/3 GET OK: cloudflare-quic.com (104.18.26.14:443) alpn=h3 rtt=40ms
status=200 OK body=125959B first_line="<!DOCTYPE html>"
```

This repository is a reproducible proof-of-concept. A Rust program is compiled to
`wasm32-unknown-emscripten`, and, via Guy Bedford's Emscripten, mio, and Tokio forks,
it uses **async `tokio` UDP sockets** to complete a
**QUIC handshake and an HTTP/3 (ALPN `h3`) GET** against `cloudflare-quic.com` over the
real internet (200 OK, ~126 KB of HTML), executed under Node.js.

There are **two demos**, proving the same result on the two major Rust QUIC stacks:

- **`quinn`** (TLS via **rustls + ring**) — the original PoC (`demos/quinn_h3/src/main.rs`).
- **`quiche`** (Cloudflare's own QUIC/H3, TLS via **BoringSSL** through the `boring`
  crate) — a second demo (`demos/quiche/`) that also does a full HTTP/3 GET. Cargo compiles
  the bundled BoringSSL to wasm. See [`docs/quiche-boringssl.md`](docs/quiche-boringssl.md).

It builds on [Guy Bedford](https://github.com/guybedford)'s work bringing full POSIX
networking (epoll, async DNS, raw TCP/UDP sockets over `node:net`/`node:dgram`, and a
`tokio` reactor) to the `wasm32-unknown-emscripten` target.

> **Scope:** the POC targets **Node.js**. Cloudflare Workers is the eventual home for the
> same crate, but is out of scope here — it needs `node:dgram` in the Workers runtime,
> which is being added separately. Nothing in this POC depends on it.

---

## The stack

```
  h3 (HTTP/3)  ──►  quinn (QUIC/TLS 1.3)
     │   custom AsyncUdpSocket / UdpPoller adapter
     ▼
  tokio::net::UdpSocket        ← standard Tokio API over Guy's mio backend
     │   mio over the epoll reactor
     ▼
  emscripten libc + JS FS      ← epoll + async DNS + -sNODERAWSOCKETS (Guy's fork)
     ▼
  node:net / node:dgram  →  the internet  →  QUIC endpoint
```

TLS 1.3 is provided by **rustls + ring** in the quinn demo; the quiche demo uses
**BoringSSL**. Both build on Emscripten without assembly.

## Why quinn?

`quinn` is the de-facto pure-Rust QUIC implementation, is `tokio`-native, and uses
`rustls` — the exact TLS stack that already works on emscripten. It also exposes an
`AsyncUdpSocket` trait, letting us plug in an emscripten UDP backend and bypass
`quinn-udp`'s platform-specific GSO/GRO code. It was the fastest path to a first result
because it needs no C crypto library.

## Also quiche (BoringSSL)

The second demo (`demos/quiche/`) uses **quiche**, Cloudflare's own QUIC/HTTP-3 library,
which does TLS via **BoringSSL** (through the `boring` crate). BoringSSL was initially
assumed intractable on emscripten, but Cargo can compile it to wasm (`OPENSSL_NO_ASM`) and
run it on Node. The target configuration and upstream wasm FFI fix are documented in
[`docs/quiche-boringssl.md`](docs/quiche-boringssl.md). The demo uses Cloudflare's
`tokio-quiche` adapter to drive quiche's sans-I/O state machines over the same Emscripten
`tokio::net::UdpSocket`. (`s2n-quic` pulls aws-lc-rs, which remains intractable on
Emscripten.)

## What's in here

| Path | What |
| --- | --- |
| `demos/quinn_h3/` | **The quinn demo.** `h3` + `quinn` + a custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`, doing an HTTP/3 GET to `cloudflare-quic.com`. |
| `demos/quiche/` | **The quiche demo.** `tokio-quiche` + BoringSSL over the Emscripten `tokio::net::UdpSocket`. |
| `patches/` | Reproducible Emscripten compatibility patches for wasm-bindgen, quanta, and tokio-quiche dependencies. |
| `cmake/boringssl-emscripten.cmake` | Selects BoringSSL's portable C implementation and loads the Emscripten CMake toolchain for Cargo's `boring-sys` build. |
| `scripts/` | `setup.sh` (clone dependencies + apply patches + toolchain), `run-quinn_h3.sh` (quinn demo), `run-quiche.sh` (quiche demo). No user-supplied configuration is required. |
| `.github/workflows/ci.yml` | CI that provisions the toolchain and runs **both** demos on macOS, asserting the success sentinels. |
| `docs/branch-map.md` | Exact revisions and provisioning methods for nonstandard dependencies. |
| `AGENTS.md` | Orientation + hard-won build knowledge for contributors and coding agents. |

> Run `scripts/setup.sh` before building. It provisions five pinned Guy Bedford forks plus
> pinned quanta and quiche checkouts under `.work`; Cargo fetches mio, socket2, and
> registry dependencies.
> The run scripts set the target environment and execute the generated modules under Node.

## Quickstart

Prereqs: `git`, a recent Rust toolchain with `rustup`, **Node.js ≥ 24** (JSPI +
`node:dgram` `bindSync`/`connectSync`; Node 26 recommended), Python 3, `cmake` (for
BoringSSL), a C toolchain, and a system **emscripten** for its LLVM/binaryen backend plus a
`libclang` for bindgen — on macOS: `brew install emscripten llvm cmake` (`setup.sh`
auto-detects the Homebrew emscripten backend; the `cf` fork frontend is cloned for you).

```sh
# 1. One-time setup: clones pinned dependencies, applies the compatibility
#    patches, builds the patched wasm-bindgen CLI, and configures emcc.
#    Everything lands under ./.work (git-ignored).
./scripts/setup.sh

# 2. Build + run the quinn QUIC demo under Node.
./scripts/run-quinn_h3.sh
```

Expected tail:

```
QUIC handshake OK: alpn=h3; rtt=40ms
sending HTTP/3 GET https://cloudflare-quic.com/ ...
HTTP/3 response: status=200 OK server="cloudflare"
HTTP/3 GET OK: cloudflare-quic.com (...) alpn=h3 status=200 OK body=125959B first_line="<!DOCTYPE html>"
QUIC-H3-ON-EMSCRIPTEN-OK
```

To build BoringSSL through Cargo and run the **quiche** demo instead:

```sh
./scripts/run-quiche.sh
```

Expected tail:

```
QUIC handshake and HTTP/3 connection established
HTTP/3 GET OK (tokio-quiche): cloudflare-quic.com (...) status=200 server="cloudflare" body=125959B first_line="<!DOCTYPE html>"
QUIC-H3-QUICHE-ON-EMSCRIPTEN-OK
```

## A note on certificate verification

If you run behind a TLS-inspecting VPN/proxy with a private CA (e.g. corporate
Zero-Trust), strict certificate verification will fail (`UnknownIssuer`, or an
`UnsupportedSignatureAlgorithm` if the private CA uses a key `ring` doesn't implement,
such as ECDSA P-521). The QUIC transport is unaffected.

`demos/quinn_h3/src/main.rs` has a `STRICT_VERIFY` constant:

- `STRICT_VERIFY = true` — full `webpki` verification against the Mozilla roots (use with
  no interception in the path).
- `STRICT_VERIFY = false` (default) — a verifier that still checks the server leaf's
  handshake signature via `ring` but accepts the chain path, so the transport is
  demonstrable through an inspecting proxy.

## Status & next steps

- ✅ tokio async DNS + `TcpStream` on Node (emscripten reactor).
- ✅ standard `tokio::net::UdpSocket` over Guy's Emscripten mio backend.
- ✅ `quinn` QUIC handshake to `cloudflare-quic.com` over the internet, on Node.
- ✅ HTTP/3 GET over that connection (`h3` + `h3-quinn`) — 200 OK, real HTML body.
- ✅ **BoringSSL compiled to wasm; `tokio-quiche` GET on Node** — 200 OK, same
  body (second demo).
- ✅ **CI** (`.github/workflows/ci.yml`) provisions the toolchain and runs both demos on macOS.
- ✅ Tokio UDP and libc `in6_pktinfo` are supplied by the pinned Guy Bedford branches.
- ⬜ Upstream the remaining BoringSSL Emscripten build configuration.
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.

## Credits

The heavy lifting — epoll, async DNS, raw sockets, and the tokio reactor on
`wasm32-unknown-emscripten` — is [Guy Bedford](https://github.com/guybedford)'s work in
his forks of emscripten, mio, tokio, wasm-bindgen, libc, and ring. See
[`docs/branch-map.md`](docs/branch-map.md).

## License

MIT. See [`LICENSE`](LICENSE).
