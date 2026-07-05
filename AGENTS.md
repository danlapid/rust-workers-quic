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
| `demos/quinn_h3/` (`src/main.rs`, `Cargo.toml`, `.cargo/`, `run_quic.mjs`) | **The quinn demo.** quinn + the custom `AsyncUdpSocket` over the emscripten `tokio::net::UdpSocket`. |
| `demos/quiche/` | **The quiche demo.** quiche + `quiche::h3` (BoringSSL) driving its sans-I/O loop over the same emscripten `tokio::net::UdpSocket`. Builds inside the workspace, like the quinn demo, plus the boring-sys env. |
| `patches/` | Durable changes to `tokio` (adds the emscripten `UdpSocket` module + wiring), `libc` (adds emscripten `in6_pktinfo`), and the `workers-rs` workspace, as git-apply-able diffs. |
| `patches/quiche-boringssl/` | Making quiche + BoringSSL build/run on emscripten (BoringSSL wasm recipe, the `-fvisibility=default` bindgen fix, the `-> c_void` FFI trap fix, quiche/boring-sys patches). |
| `scripts/` | `setup.sh` (clone forks + apply patches + toolchain + BoringSSL-for-wasm), `run-quinn_h3.sh` (quinn demo), `run-quiche.sh` (quiche demo). No env/config — each script derives its own paths (repo root + `.work`). |
| `.github/workflows/ci.yml` | CI (macOS): provisions the toolchain via `setup.sh` and runs **both** demos, asserting the success sentinels. |
| `docs/branch-map.md` | Exact upstream fork + branch + commit each dependency is pinned to. |

## How code flows here (important)

The actual dependency source lives in **git-ignored** clones under `vendor/` / `.work/`
(Guy's forks of emscripten, tokio, ring, wasm-bindgen, libc, workers-rs; plus
cloudflare/boring + cloudflare/quiche for the quiche demo). Those are **never committed**.
The durable, reviewable changes are captured as `patches/`.

**If you modify a vendored dependency, regenerate the corresponding patch** (e.g.
`git -C <checkout> diff <files>` → `patches/…`; for a new file, `git add -N <file>` first
so it appears in the diff as a creation hunk). The tokio patch
(`patches/tokio-emscripten-udp.patch`) both **creates** the new `udp_emscripten.rs` module
(as a `new file` hunk) and wires it into `net/mod.rs` + `reactor_stream.rs`, so a single
`git apply` does everything.

## Build & run

```sh
bash scripts/setup.sh                        # one-time: clone forks, apply patches, toolchain, BoringSSL-for-wasm
bash scripts/run-quinn_h3.sh                 # quinn demo: build + run under Node
bash scripts/run-quiche.sh                   # quiche demo: build + run under Node
```

`setup.sh` provisions **both** stacks: it clones the guybedford forks and builds the
wasm-bindgen CLI (quinn), and clones `cloudflare/boring` + `cloudflare/quiche` at the pinned
commits, applies the quiche patches, and compiles BoringSSL to wasm (quiche).
`.github/workflows/ci.yml` runs this whole flow + both demos on
macOS.

- **Neither demo `cargo build`s standalone from its directory.** Each must compile inside
  the patched `workers-rs` workspace (for the wasm-bindgen fork + `[patch.crates-io]`);
  `scripts/run-quinn_h3.sh` / `scripts/run-quiche.sh` copy the crate in as a workspace member and
  build it there. `run-quiche.sh` also sets the boring-sys bindgen env (libclang +
  `-fvisibility=default` + sysroot; see `patches/quiche-boringssl/README.md`).
- Toolchain: Guy's `emscripten @ cf` (`emcc 6.0.3-git`) frontend driving an LLVM/binaryen/node
  backend borrowed from a system emscripten — `setup.sh` auto-detects Homebrew's (proven:
  the 6.0.1 backend). emsdk is NOT used: it has no installable prebuilt backend for these
  versions on Apple Silicon. Rust 1.95 with the `wasm32-unknown-emscripten` target.
- **Node ≥ 24 (26 recommended) to run:** needs JSPI + `node:dgram` `bindSync`/`connectSync`.

## Key technical knowledge (hard-won — don't relearn these)

### The one code gap this repo closes
Guy's `tokio @ emscripten` ships `TcpStream` and `UnixDatagram` over the epoll reactor but
gates `UdpSocket` **off** (`cfg_net_not_emscripten!`). The emscripten C/JS layer already has
UDP (`node:dgram` via `-sNODERAWSOCKETS`). The tokio patch's new `udp_emscripten.rs`
surfaces it as an async `tokio::net::UdpSocket`, mirroring the `UnixDatagram` pattern
(`ReactorStream` + `with_std::<std::net::UdpSocket>` under `async_io`/`poll_read_io`/
`poll_write_io`). QUIC then rides on top with no further platform work.

### QUIC library choice
**quinn** (`demos/quinn_h3/src/main.rs`) — the de-facto pure-Rust QUIC impl, tokio-native, uses **rustls**.
We bypass `quinn-udp`'s platform GSO/GRO code by passing a custom `AsyncUdpSocket` to
`Endpoint::new_with_abstract_socket`. TLS is **rustls + ring** (ring builds via its no-asm
fallback + a getrandom `SystemRandom`). It was the fastest first result: no C crypto library.

**quiche** (`demos/quiche/`) — Cloudflare's own QUIC/H3, TLS via **BoringSSL** (the `boring`
crate). Initially assumed intractable, but BoringSSL *does* build to wasm (`OPENSSL_NO_ASM`)
and run on Node. quiche is sans-I/O, so there's no platform UDP code to port — we drive its
`send()`/`recv()` loop over the same emscripten `tokio::net::UdpSocket`. Two emscripten-only
fixes were needed, both in `patches/quiche-boringssl/`: (1) bindgen dropped every BoringSSL
function because emscripten clang defaults to hidden visibility — fixed with one clang flag,
`-fvisibility=default`; (2) quiche declared `AES_ecb_encrypt`/`CRYPTO_chacha_20` as
`-> c_void`, which traps on wasm (the result type is part of the call signature) — fixed by
declaring them `-> ()`. **s2n-quic** (aws-lc-rs) remains intractable on emscripten.

### Build gotchas (encoded in `.cargo/config.toml` + `scripts/`, but know why)
- The **patched wasm-bindgen CLI must be on `PATH`** for `-sWASM_BINDGEN=auto` (built from
  the wasm-bindgen fork's `crates/cli`; the schema version must match the linked lib).
- **Drop `-sSOURCE_PHASE_IMPORTS`** for Node — it trips emscripten's acorn DCE walker on the
  wasm-bindgen ESM output (it's a browser-only/experimental flag).
- **Don't name a wasm-bindgen export `run`** — it collides with emscripten's internal
  runtime `run()` (`AssertionError: runtimeElements contains library symbol: $run`).
- **Stock `quinn-udp` builds unmodified** once the emscripten `libc` module gains
  `in6_pktinfo` (`patches/libc-emscripten-in6_pktinfo.patch`). Its `unix.rs` needs that one
  struct (`in_pktinfo` is already inherited from `linux_like`); its GSO/GRO code is gated to
  `target_os = "linux"`, so nothing else is missing on emscripten. We don't patch quinn-udp
  or route it to `fallback.rs` — this was a real gap in Guy's emscripten libc, not a
  quinn-udp problem. (We still pass quinn an abstract socket, so quinn-udp is unused at
  runtime; the fix is just so it *compiles*.)
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

See `docs/branch-map.md` for the exact fork + branch + commit of every dependency.

## Status / next steps

- ✅ tokio async DNS + `TcpStream` on Node (emscripten reactor).
- ✅ new reactor-backed `tokio::net::UdpSocket` for emscripten.
- ✅ quinn QUIC handshake to `cloudflare-quic.com` over the internet, on Node.
- ✅ HTTP/3 GET over that connection (`h3` + `h3-quinn`) — 200 OK, real HTML body.
- ✅ BoringSSL compiled to wasm; quiche + `quiche::h3` GET on Node — 200 OK (second demo).
- ✅ Fold the quiche/boring build (BoringSSL-for-wasm + clones) into `scripts/setup.sh` + CI.
- ⬜ Upstream to Guy: the `tokio` `UdpSocket` support, the `libc` `in6_pktinfo` addition, and
  the quiche/boring emscripten fixes. (Also report the latent `quinn-udp` `fallback.rs`
  `send()` return-type bug upstream — unrelated to emscripten.)
- ⬜ Same crate on Cloudflare Workers, once the runtime exposes `node:dgram`.
- ⬜ Single-datagram UDP only (no GSO/GRO) — a perf follow-up.

## Conventions for changes

- **Never commit** `vendor/`, `.work/`, `target/`, `*.pem`, or generated
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
