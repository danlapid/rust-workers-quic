# Patches

This POC has one local dependency source patch. `scripts/setup.sh` applies it to the pinned
wasm-bindgen checkout described in [`../docs/branch-map.md`](../docs/branch-map.md).

| File | Target | What it does |
| --- | --- | --- |
| `wasm-bindgen-tokio-hosted-runtime.patch` | `git apply` in the wasm-bindgen checkout | Updates `#[wasm_bindgen(tokio)]` from Tokio's removed global Emscripten event-loop API to the successor branch's public `HostedRuntime`. |

Direct clone revisions are pinned in `scripts/setup.sh`, Cargo-managed sources in the root
`Cargo.toml` and `Cargo.lock`, and Rust in `rust-toolchain.toml`.
