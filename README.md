# rust-workers-quic

**Proof that Rust compiled to `wasm32-unknown-emscripten` can make a real HTTP/3
(QUIC) request to the public internet using `tokio` sockets — running on Node.js.**

```
HTTP/3 GET OK: cloudflare-quic.com (104.18.26.14:443) alpn=h3 rtt=40ms
status=200 OK body=125959B first_line="<!DOCTYPE html>"
```

This repository is a reproducible proof-of-concept. A Rust program is compiled to
`wasm32-unknown-emscripten`, and — via a small set of patches to `tokio`, `quinn-udp`,
and the emscripten toolchain — it uses **async `tokio` UDP sockets** to complete a
**QUIC handshake and an HTTP/3 (ALPN `h3`) GET** against `cloudflare-quic.com` over the
real internet (200 OK, ~126 KB of HTML), executed under Node.js.

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
`quinn-udp`'s platform-specific GSO/GRO code. (`quiche` and `s2n-quic` were considered
but pull BoringSSL / aws-lc-rs, which don't build cleanly on emscripten.)

## What's in here

| Path | What |
| --- | --- |
| `src/main.rs`, `Cargo.toml`, `.cargo/`, `run_quic.mjs` | **The demo.** `h3` + `quinn` + a custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`, doing an HTTP/3 GET to `cloudflare-quic.com`. |
| `patches/` | The changes to `tokio`, `quinn-udp`, and the `workers-rs` workspace (as `.patch` files + the one new tokio source file, `udp_emscripten.rs`). |
| `scripts/` | `env.sh` (paths), `setup.sh` (clone forks + apply patches + toolchain), `run.sh` (build + run the demo). |
| `docs/branch-map.md` | Exact upstream fork + branch + commit each dependency is pinned to. |
| `AGENTS.md` | Orientation + hard-won build knowledge for contributors and coding agents. |

> The demo builds inside the patched `workers-rs` workspace (it needs the wasm-bindgen
> fork + `[patch.crates-io]`), so `cargo build` won't work directly from this directory —
> use `scripts/run.sh`, which copies it into the workspace and builds it there.

## Quickstart

Prereqs: `git`, a recent Rust toolchain with `rustup`, **Node.js ≥ 24** (JSPI +
`node:dgram` `bindSync`/`connectSync`; Node 26 recommended), Python 3, and a C toolchain.

```sh
# 1. Configure paths (edit to taste; defaults put everything under ./.work).
cp scripts/env.sh.example scripts/env.sh    # then edit, or just use defaults
source scripts/env.sh

# 2. One-time setup: clones Guy's forks at the pinned branches, applies the
#    patches, builds the patched wasm-bindgen CLI, and configures emcc.
./scripts/setup.sh

# 3. Build + run the QUIC demo under Node.
./scripts/run.sh
```

Expected tail:

```
QUIC handshake OK: alpn=h3; rtt=40ms
sending HTTP/3 GET https://cloudflare-quic.com/ ...
HTTP/3 response: status=200 OK server="cloudflare"
HTTP/3 GET OK: cloudflare-quic.com (...) alpn=h3 status=200 OK body=125959B first_line="<!DOCTYPE html>"
QUIC-H3-ON-EMSCRIPTEN-OK
```

## A note on certificate verification

If you run behind a TLS-inspecting VPN/proxy with a private CA (e.g. corporate
Zero-Trust), strict certificate verification will fail (`UnknownIssuer`, or an
`UnsupportedSignatureAlgorithm` if the private CA uses a key `ring` doesn't implement,
such as ECDSA P-521). The QUIC transport is unaffected.

`src/main.rs` has a `STRICT_VERIFY` constant:

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
- ⬜ Upstream the `tokio` `UdpSocket` support.
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.

## Credits

The heavy lifting — epoll, async DNS, raw sockets, and the tokio reactor on
`wasm32-unknown-emscripten` — is [Guy Bedford](https://github.com/guybedford)'s work in
his forks of emscripten, tokio, wasm-bindgen, ring, and workers-rs. See
[`docs/branch-map.md`](docs/branch-map.md).

## License

MIT. See [`LICENSE`](LICENSE).
