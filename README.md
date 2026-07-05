# rust-workers-quic

[![demos](https://github.com/danlapid/rust-workers-quic/actions/workflows/ci.yml/badge.svg)](https://github.com/danlapid/rust-workers-quic/actions/workflows/ci.yml)

**Proof that Rust compiled to `wasm32-unknown-emscripten` can make a real HTTP/3
(QUIC) request to the public internet using `tokio` sockets — running on Node.js.**

```
HTTP/3 GET OK: cloudflare-quic.com (104.18.26.14:443) alpn=h3 rtt=40ms
status=200 OK body=125959B first_line="<!DOCTYPE html>"
```

This repository is a reproducible proof-of-concept. A Rust program is compiled to
`wasm32-unknown-emscripten`, and — via a small set of patches to `tokio`, `libc`,
and the emscripten toolchain — it uses **async `tokio` UDP sockets** to complete a
**QUIC handshake and an HTTP/3 (ALPN `h3`) GET** against `cloudflare-quic.com` over the
real internet (200 OK, ~126 KB of HTML), executed under Node.js.

There are **two demos**, proving the same result on the two major Rust QUIC stacks:

- **`quinn`** (TLS via **rustls + ring**) — the original PoC (`demos/quinn_h3/src/main.rs`).
- **`quiche`** (Cloudflare's own QUIC/H3, TLS via **BoringSSL** through the `boring`
  crate) — a second demo (`demos/quiche/`) that also does a full HTTP/3 GET, once BoringSSL
  is compiled to wasm. See [`patches/quiche-boringssl/`](patches/quiche-boringssl/).

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
  tokio::net::UdpSocket        ← added for emscripten (this repo's patch)
     │   AsyncFd over the epoll reactor
     ▼
  emscripten libc + JS FS      ← epoll + async DNS + -sNODERAWSOCKETS (Guy's fork)
     ▼
  node:net / node:dgram  →  the internet  →  QUIC endpoint
```

TLS 1.3 is provided by **rustls + ring** (aws-lc-rs / BoringSSL are intractable on
emscripten; ring builds via its no-asm fallback + a getrandom `SystemRandom`).

## Why quinn?

`quinn` is the de-facto pure-Rust QUIC implementation, is `tokio`-native, and uses
`rustls` — the exact TLS stack that already works on emscripten. It also exposes an
`AsyncUdpSocket` trait, letting us plug in an emscripten UDP backend and bypass
`quinn-udp`'s platform-specific GSO/GRO code. It was the fastest path to a first result
because it needs no C crypto library.

## Also quiche (BoringSSL)

The second demo (`demos/quiche/`) uses **quiche**, Cloudflare's own QUIC/HTTP-3 library,
which does TLS via **BoringSSL** (through the `boring` crate). BoringSSL was initially
assumed intractable on emscripten, but it does compile to wasm (`OPENSSL_NO_ASM`) and run
on Node — the write-up, the one-flag bindgen fix, and a wasm-only `-> c_void` FFI trap fix
are in [`patches/quiche-boringssl/`](patches/quiche-boringssl/). quiche is *sans-I/O*, so
there's no platform UDP code to port: the demo drives `quiche`'s `send()`/`recv()` loop
directly over the same emscripten `tokio::net::UdpSocket`. (`s2n-quic` pulls aws-lc-rs,
which remains intractable on emscripten.)

## What's in here

| Path | What |
| --- | --- |
| `demos/quinn_h3/` | **The quinn demo.** `h3` + `quinn` + a custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`, doing an HTTP/3 GET to `cloudflare-quic.com`. |
| `demos/quiche/` | **The quiche demo.** `quiche` + `quiche::h3` (BoringSSL) driving its sans-I/O loop over the same emscripten `tokio::net::UdpSocket`. |
| `patches/` | The changes to `tokio` (adds the emscripten `UdpSocket` module + wiring), `libc` (adds emscripten `in6_pktinfo` so stock `quinn-udp` compiles), and the `workers-rs` workspace, as git-apply-able `.patch` files. |
| `patches/quiche-boringssl/` | The quiche + BoringSSL-on-emscripten track (BoringSSL wasm recipe, the `-fvisibility=default` bindgen fix, the `-> c_void` FFI trap fix, quiche/boring-sys patches). |
| `scripts/` | `setup.sh` (clone forks + apply patches + toolchain + BoringSSL-for-wasm), `run-quinn_h3.sh` (quinn demo), `run-quiche.sh` (quiche demo). No config — each script figures out its own paths. |
| `.github/workflows/ci.yml` | CI that provisions the toolchain and runs **both** demos on macOS, asserting the success sentinels. |
| `docs/branch-map.md` | Exact upstream fork + branch + commit each dependency is pinned to. |
| `AGENTS.md` | Orientation + hard-won build knowledge for contributors and coding agents. |

> Both demos build inside the patched `workers-rs` workspace (they need the wasm-bindgen
> fork + `[patch.crates-io]`), so `cargo build` won't work directly from their directories
> — use `scripts/run-quinn_h3.sh` / `scripts/run-quiche.sh`, which copy the crate into the
> workspace and build it there.

## Quickstart

Prereqs: `git`, a recent Rust toolchain with `rustup`, **Node.js ≥ 24** (JSPI +
`node:dgram` `bindSync`/`connectSync`; Node 26 recommended), Python 3, `cmake` (for
BoringSSL), a C toolchain, and a system **emscripten** for its LLVM/binaryen backend plus a
`libclang` for bindgen — on macOS: `brew install emscripten llvm cmake` (`setup.sh`
auto-detects the Homebrew emscripten backend; the `cf` fork frontend is cloned for you).

```sh
# 1. One-time setup: clones Guy's forks at the pinned branches, applies the
#    patches, builds the patched wasm-bindgen CLI, configures emcc, and compiles
#    BoringSSL to wasm. Everything lands under ./.work (git-ignored).
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

To run the **quiche** demo instead, first compile BoringSSL for wasm once (recipe in
[`patches/quiche-boringssl/README.md`](patches/quiche-boringssl/README.md)), then:

```sh
./scripts/run-quiche.sh
```

Expected tail:

```
QUIC handshake OK: alpn="h3"; rtt=30ms
H3 headers on stream 0: status=200 server="cloudflare"
HTTP/3 GET OK (quiche): cloudflare-quic.com (...) status=200 body=125959B first_line="<!DOCTYPE html>"
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
- ✅ new reactor-backed `tokio::net::UdpSocket` for emscripten.
- ✅ `quinn` QUIC handshake to `cloudflare-quic.com` over the internet, on Node.
- ✅ HTTP/3 GET over that connection (`h3` + `h3-quinn`) — 200 OK, real HTML body.
- ✅ **BoringSSL compiled to wasm; `quiche` + `quiche::h3` GET on Node** — 200 OK, same
  body (second demo).
- ✅ **CI** (`.github/workflows/ci.yml`) provisions the toolchain and runs both demos on macOS.
- ⬜ Upstream the `tokio` `UdpSocket` support, the `libc` `in6_pktinfo` addition, and the
  quiche/boring emscripten fixes.
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.

## Credits

The heavy lifting — epoll, async DNS, raw sockets, and the tokio reactor on
`wasm32-unknown-emscripten` — is [Guy Bedford](https://github.com/guybedford)'s work in
his forks of emscripten, tokio, wasm-bindgen, ring, and workers-rs. See
[`docs/branch-map.md`](docs/branch-map.md).

## License

MIT. See [`LICENSE`](LICENSE).
