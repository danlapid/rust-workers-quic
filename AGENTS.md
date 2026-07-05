# AGENTS.md

Context and working notes for anyone (human or AI agent) hacking on this repo. Read this
before making changes.

## What this project is

A proof-of-concept that **Rust compiled to `wasm32-unknown-emscripten` can make a real
HTTP/3 (QUIC) request to the public internet using `tokio` sockets, running on Node.js.**

It builds on [Guy Bedford](https://github.com/guybedford)'s forks, which bring POSIX
networking to the emscripten target: epoll, async DNS, raw TCP/UDP over
`node:net`/`node:dgram` (`-sNODERAWSOCKETS`), and a `tokio` reactor.

Proven result (Node.js): a QUIC/TLS-1.3 handshake **and an HTTP/3 (ALPN `h3`) GET** to
`cloudflare-quic.com` — `200 OK`, ~126 KB of HTML, ~40 ms RTT.

**Scope: Node.js only.** Cloudflare Workers is the eventual home for the same crate, but
needs `node:dgram` in the Workers runtime (a separate effort). Nothing here depends on it.

## The stack (mental model)

```
  h3 (HTTP/3)  ──►  quinn (QUIC / TLS 1.3 via rustls + ring)
     │   custom AsyncUdpSocket / UdpPoller adapter (src/main.rs)
     ▼
  tokio::net::UdpSocket        ← added for emscripten (patches/)
     │   AsyncFd over the epoll reactor
     ▼
  emscripten libc + JS FS      ← epoll + async DNS + -sNODERAWSOCKETS (guybedford fork)
     ▼
  node:net / node:dgram  →  the internet  →  QUIC endpoint
```

The async entry point is `#[wasm_bindgen(tokio)]`, which drives an exported `async fn` on
tokio's emscripten event loop (JSPI-parked `block_on` + a host event loop). That attribute
lives in the patched wasm-bindgen fork.

## Repo layout

| Path | What |
| --- | --- |
| `src/main.rs`, `Cargo.toml`, `.cargo/`, `run_quic.mjs` | **The demo.** quinn + the custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`. |
| `patches/` | Durable changes to `tokio`, `quinn-udp`, and the `workers-rs` workspace (git-apply-able diffs + the one new tokio file, `udp_emscripten.rs`). |
| `scripts/` | `env.sh.example` (paths), `setup.sh` (clone forks + apply patches + toolchain), `run.sh` (build + run). |
| `docs/branch-map.md` | Exact upstream fork + branch + commit each dependency is pinned to. |

## How code flows here (important)

The actual dependency source lives in **git-ignored** clones under `vendor/` / `.work/`
(Guy's forks of emscripten, tokio, ring, wasm-bindgen, libc, workers-rs, plus a patched
quinn-udp). Those are **never committed**. The durable, reviewable changes are captured as
`patches/`.

**If you modify a vendored dependency, regenerate the corresponding patch** (e.g.
`git -C <checkout> diff <files>` → `patches/…`). The new tokio module is shipped as a whole
file (`patches/tokio/src/net/udp_emscripten.rs`) because it's a new file, not a diff.

## Build & run

```sh
cp scripts/env.sh.example scripts/env.sh   # edit or use defaults
source scripts/env.sh
bash scripts/setup.sh                        # one-time: clone forks, apply patches, toolchain
bash scripts/run.sh                          # build the demo + run under Node
```

- **The demo does not `cargo build` standalone from the repo root.** It must compile inside
  the patched `workers-rs` workspace (for the wasm-bindgen fork + `[patch.crates-io]`);
  `scripts/run.sh` copies it in as a workspace member and builds it there.
- Toolchain: Guy's `emscripten @ cf` (`emcc 6.0.3-git`) driving an LLVM/binaryen/node
  backend (emsdk, or a system emscripten's backend via `RWQ_USE_EMSDK=0`). Rust 1.95 with
  the `wasm32-unknown-emscripten` target (the workspace pins it).
- **Node ≥ 24 (26 recommended) to run:** needs JSPI + `node:dgram` `bindSync`/`connectSync`.

## Key technical knowledge (hard-won — don't relearn these)

### The one code gap this repo closes
Guy's `tokio @ emscripten` ships `TcpStream` and `UnixDatagram` over the epoll reactor but
gates `UdpSocket` **off** (`cfg_net_not_emscripten!`). The emscripten C/JS layer already has
UDP (`node:dgram` via `-sNODERAWSOCKETS`). `patches/tokio/src/net/udp_emscripten.rs`
surfaces it as an async `tokio::net::UdpSocket`, mirroring the `UnixDatagram` pattern
(`ReactorStream` + `with_std::<std::net::UdpSocket>` under `async_io`/`poll_read_io`/
`poll_write_io`). QUIC then rides on top with no further platform work.

### QUIC library choice
**quinn** — the de-facto pure-Rust QUIC impl, tokio-native, uses **rustls**. We bypass
`quinn-udp`'s platform GSO/GRO code by passing a custom `AsyncUdpSocket` to
`Endpoint::new_with_abstract_socket`. `quiche` (BoringSSL) and `s2n-quic` (aws-lc-rs) were
rejected: those C crypto libraries don't build on emscripten. TLS is **rustls + ring**
(ring builds via its no-asm fallback + a getrandom `SystemRandom`; aws-lc-rs / BoringSSL are
intractable on emscripten).

### Build gotchas (encoded in `.cargo/config.toml` + `scripts/`, but know why)
- The **patched wasm-bindgen CLI must be on `PATH`** for `-sWASM_BINDGEN=auto` (built from
  the wasm-bindgen fork's `crates/cli`; the schema version must match the linked lib).
- **Drop `-sSOURCE_PHASE_IMPORTS`** for Node — it trips emscripten's acorn DCE walker on the
  wasm-bindgen ESM output (it's a browser-only/experimental flag).
- **Don't name a wasm-bindgen export `run`** — it collides with emscripten's internal
  runtime `run()` (`AssertionError: runtimeElements contains library symbol: $run`).
- **`quinn-udp` doesn't build on emscripten** (its `unix.rs` uses `in_pktinfo`/`in6_pktinfo`
  cmsg structs emscripten's libc lacks). We route `target_os = "emscripten"` to quinn-udp's
  `fallback.rs` stub (never used — we pass an abstract socket) and fix a latent `send()`
  return-type bug. See `patches/quinn-udp-emscripten-fallback.patch`.
- **emcc link flags go through `rustc -Clink-arg=…`**, not `EMCC_CFLAGS` — the latter also
  applies to C compiles (e.g. ring), where a link-only flag is `-Wunused-command-line-argument`
  and `-Werror` makes it fatal.
- `-sASSERTIONS=0` so emscripten hardcodes `ENVIRONMENT_IS_NODE` (else it auto-detects and
  can misfire).
- `-sSTACK_SIZE=8MB` + `-sALLOW_MEMORY_GROWTH=1` (regex-automata's meta-DFA build recurses
  deeply; the QUIC/TLS stack wants heap room).
- `-Crelocation-model=static` (the only model emcc fully supports for the staticlib +
  post-link pipeline) and `--cfg=tokio_unstable` (gates tokio's emscripten runtime).
- The output is an **ESM** (`MODULARIZE=instance` / `EXPORT_ES6`); `node file.js` won't
  auto-run `main`. `run_quic.mjs` imports the instance and calls the export.

### Certificate verification behind a TLS-inspecting proxy
If you run behind a TLS-inspecting VPN/proxy with a private CA, **strict** cert verification
fails (`UnknownIssuer`, or `UnsupportedSignatureAlgorithm` if the private CA uses a key ring
doesn't implement, e.g. ECDSA P-521). **The QUIC transport is unaffected.** `src/main.rs`
has a `STRICT_VERIFY` constant:
- `true` — full `webpki` verification against the Mozilla roots (use on a clean network path).
- `false` (default) — a verifier that still ring-checks the server leaf's handshake
  signature but accepts the chain path, so the transport is demonstrable through an
  inspecting proxy. **Not for production.**

## Dependency pins

See `docs/branch-map.md` for the exact fork + branch + commit of every dependency.

## Status / next steps

- ✅ tokio async DNS + `TcpStream` on Node (emscripten reactor).
- ✅ new reactor-backed `tokio::net::UdpSocket` for emscripten.
- ✅ quinn QUIC handshake to `cloudflare-quic.com` over the internet, on Node.
- ✅ HTTP/3 GET over that connection (`h3` + `h3-quinn`) — 200 OK, real HTML body.
- ⬜ Upstream the `tokio` `UdpSocket` support.
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.
- ⬜ Single-datagram UDP only (no GSO/GRO) — a perf follow-up.

## Conventions for changes

- **Never commit** `vendor/`, `.work/`, `target/`, `*.pem`, `scripts/env.sh`, or generated
  `*.js`/`*.wasm` (all git-ignored).
- When you change a vendored dependency, **update the matching `patches/` file** so the
  change survives a fresh `setup.sh`.
- **Keep everything public-safe**: no secrets, no private CA material, no internal
  infrastructure names, no non-public URLs, no account IDs.

## Git safety

- **Never** `git commit`, `push`, `rebase`, `reset --hard`, or force-push **without explicit
  approval.** Show the diff / proposed commands and wait for confirmation.
- Prefer additive changes; regenerate patches rather than editing vendored trees in place
  where practical.
