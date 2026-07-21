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
`cloudflare-quic.com` — `200 OK`, ~126 KB of HTML, ~40 ms RTT. This is proven on **two**
QUIC stacks: **quinn** (rustls + ring, `demos/quinn_h3/src/main.rs`) and **quiche** (Cloudflare's own
QUIC/H3 on BoringSSL, `demos/quiche/`).

**Scope: Node.js only.** Cloudflare Workers is the eventual home for the same crate, but
needs `node:dgram` in the Workers runtime (a separate effort). Nothing here depends on it.

## The stack (mental model)

```
  h3 (HTTP/3)  ──►  quinn (QUIC / TLS 1.3 via rustls + ring)
     │   custom AsyncUdpSocket / UdpPoller adapter (demos/quinn_h3/src/main.rs)
     ▼
  tokio::net::UdpSocket        ← standard Tokio API over Guy's mio backend
     │   mio over the epoll reactor
     ▼
  emscripten libc + JS FS      ← epoll + async DNS + -sNODERAWSOCKETS (guybedford fork)
     ▼
  node:net / node:dgram  →  the internet  →  QUIC endpoint
```

The async entry point is `#[wasm_bindgen(tokio)]`, which schedules each exported
`async fn` on a persistent Tokio `HostedRuntime` driven cooperatively by the host event
loop. That attribute lives in the patched wasm-bindgen fork.

## Repo layout

| Path | What |
| --- | --- |
| `demos/quinn_h3/` (`src/main.rs`, `Cargo.toml`, `.cargo/`, `run_quic.mjs`) | **The quinn demo.** quinn + the custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`. |
| `demos/quiche/` | **The quiche demo.** `tokio-quiche` + BoringSSL over the Emscripten `tokio::net::UdpSocket`. |
| `patches/` | Reproducible Emscripten compatibility patches for wasm-bindgen, quanta, and tokio-quiche dependencies. |
| `cmake/boringssl-emscripten.cmake` | Lets Cargo's normal `boring-sys` build select portable BoringSSL C code and the Emscripten toolchain. |
| `scripts/` | `setup.sh` (clone dependencies + apply patches + toolchain), `run-quinn_h3.sh` (quinn demo), `run-quiche.sh` (quiche demo). No user-supplied configuration is required. |
| `.github/workflows/ci.yml` | CI (macOS): provisions the toolchain via `setup.sh` and runs **both** demos, asserting the success sentinels. |
| `docs/branch-map.md` | Exact revisions and provisioning methods for nonstandard dependencies. |

## How code flows here (important)

`setup.sh` clones pinned emscripten, wasm-bindgen, Tokio, libc, ring, quanta, and quiche
revisions into git-ignored `.work/` directories. Cargo fetches pinned mio and socket2;
`tokio-quiche`, `boring`, and `boring-sys` come from crates.io. See `patches/README.md`
for the three local dependency changes.

If you modify a patched `.work` checkout, regenerate its patch so the change survives a
fresh setup. Keep unpatched checkouts unmodified; update their pins instead. Do not
reintroduce local Tokio/libc UDP patches unless upstream support demonstrably regresses.

## Build & run

```sh
bash scripts/setup.sh                        # one-time: clone, patch, configure toolchain
bash scripts/run-quinn_h3.sh                 # quinn demo: build + run under Node
bash scripts/run-quiche.sh                   # quiche demo: build + run under Node
```

`setup.sh` clones the five pinned Guy Bedford forks plus quanta and quiche, then builds the
wasm-bindgen CLI. Cargo fetches mio and socket2; quiche's published `boring` dependency causes
`boring-sys` to compile its bundled BoringSSL for wasm.
`.github/workflows/ci.yml` runs this whole flow + both demos on
macOS.

- The root Cargo workspace references the fork sources under `.work`; run `setup.sh` first.
  `scripts/run-quinn_h3.sh` / `scripts/run-quiche.sh` set the target environment, build the
  corresponding workspace package, and run it. `run-quiche.sh` also sets the BoringSSL CMake and bindgen environment
  (see `docs/quiche-boringssl.md`).
- Toolchain: Guy's `emscripten @ cf` (`emcc 6.0.3-git`) frontend driving an LLVM/binaryen/node
  backend borrowed from a system emscripten — `setup.sh` auto-detects Homebrew's (proven:
  the 6.0.1 backend). emsdk is NOT used: it has no installable prebuilt backend for these
  versions on Apple Silicon. Rust nightly 2026-07-20 with the
  `wasm32-unknown-emscripten` target.
- **Node ≥ 24 (26 recommended) to run:** needs JSPI + `node:dgram` `bindSync`/`connectSync`.

## Key technical knowledge (hard-won — don't relearn these)

### Upstream networking architecture
Guy's `tokio @ emscripten-layering` uses `guybedford/mio @ emscripten`, so Tokio's
standard TCP and UDP implementations run over the Emscripten epoll reactor. The current
`libc-0.2-emscripten` branch includes `in6_pktinfo`. This supersedes this repository's
former custom `udp_emscripten.rs` and libc patches.

### QUIC library choice
**quinn** (`demos/quinn_h3/src/main.rs`) — the de-facto pure-Rust QUIC impl, tokio-native, uses **rustls**.
We bypass `quinn-udp`'s platform GSO/GRO code by passing a custom `AsyncUdpSocket` to
`Endpoint::new_with_abstract_socket`. TLS is **rustls + ring** (ring builds via its no-asm
fallback + a getrandom `SystemRandom`). It was the fastest first result: no C crypto library.

**quiche** (`demos/quiche/`) — Cloudflare's own QUIC/H3, TLS via **BoringSSL** (the `boring`
crate). Initially assumed intractable, but BoringSSL *does* build to wasm (`OPENSSL_NO_ASM`)
and run on Node. `tokio-quiche` drives quiche's sans-I/O state machines over the same
Emscripten `tokio::net::UdpSocket`. The build needs
`-fvisibility=default` so bindgen sees BoringSSL functions. quiche's former wasm-trapping
`-> c_void` declarations were corrected upstream in PR #2535, and the demo pins a commit
containing that fix. The local quiche manifest patch suppresses unused C ABI crate types;
there is no quiche protocol or boring source patch. **s2n-quic** (aws-lc-rs) remains
intractable on Emscripten.

### Build gotchas (encoded in `.cargo/config.toml` + `scripts/`, but know why)
- The **patched wasm-bindgen CLI must be on `PATH`** for `-sWASM_BINDGEN=auto` (built from
  the wasm-bindgen fork's `crates/cli`; the schema version must match the linked lib).
- **Drop `-sSOURCE_PHASE_IMPORTS`** for Node — it trips emscripten's acorn DCE walker on the
  wasm-bindgen ESM output (it's a browser-only/experimental flag).
- **Don't name a wasm-bindgen export `run`** — it collides with emscripten's internal
  runtime `run()` (`AssertionError: runtimeElements contains library symbol: $run`).
- **Stock `quinn-udp` builds unmodified** because Guy's current libc includes
  `in6_pktinfo`. We still pass quinn an abstract socket, so quinn-udp is unused at runtime.
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
  auto-run `main`. `demos/quinn_h3/run_quic.mjs` imports the instance and calls the export.

### Certificate verification behind a TLS-inspecting proxy
If you run behind a TLS-inspecting VPN/proxy with a private CA, **strict** cert verification
fails (`UnknownIssuer`, or `UnsupportedSignatureAlgorithm` if the private CA uses a key ring
doesn't implement, e.g. ECDSA P-521). **The QUIC transport is unaffected.** `demos/quinn_h3/src/main.rs`
has a `STRICT_VERIFY` constant:
- `true` — full `webpki` verification against the Mozilla roots (use on a clean network path).
- `false` (default) — a verifier that still ring-checks the server leaf's handshake
  signature but accepts the chain path, so the transport is demonstrable through an
  inspecting proxy. **Not for production.**

## Dependency pins

See `docs/branch-map.md` for every nonstandard dependency source and pin.

## Status / next steps

- ✅ tokio async DNS + `TcpStream` on Node (emscripten reactor).
- ✅ standard `tokio::net::UdpSocket` over Guy's Emscripten mio backend.
- ✅ quinn QUIC handshake to `cloudflare-quic.com` over the internet, on Node.
- ✅ HTTP/3 GET over that connection (`h3` + `h3-quinn`) — 200 OK, real HTML body.
- ✅ BoringSSL compiled to wasm; `tokio-quiche` GET on Node — 200 OK (second demo).
- ✅ Cargo builds quiche's published `boring` dependency and bundled BoringSSL for wasm.
- ✅ Tokio UDP and libc `in6_pktinfo` are present in the pinned Guy Bedford branches.
- ⬜ Upstream the remaining BoringSSL Emscripten build configuration. Also report the latent `quinn-udp`
  `fallback.rs` `send()` return-type bug upstream; it is unrelated to Emscripten.
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.
- ⬜ Single-datagram UDP only (no GSO/GRO) — a perf follow-up.

## Conventions for changes

- **Never commit** `.work/`, `target/`, `*.pem`, or generated
  `*.js`/`*.wasm` (all git-ignored).
- Changes to patched dependency checkouts must be reflected in the corresponding file under
  `patches/`; do not make unreproducible edits under `.work`.
- **Keep everything public-safe**: no secrets, no private CA material, no internal
  infrastructure names, no non-public URLs, no account IDs.

## Git safety

- **Never** `git commit`, `push`, `rebase`, `reset --hard`, or force-push **without explicit
  approval.** Show the diff / proposed commands and wait for confirmation.
- Prefer additive changes and keep generated dependency checkouts reproducible from pins.
