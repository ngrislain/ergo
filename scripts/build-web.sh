#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root. Cargo.lock pins the matching wasm-bindgen version.
cargo build --locked --release --target wasm32-unknown-unknown -p ergo-viewer
wasm-bindgen --target web --out-dir web/pkg --no-typescript target/wasm32-unknown-unknown/release/ergo_viewer.wasm
cargo build --locked --release --target wasm32-unknown-unknown -p ergo-client --features mcp-entry
wasm-bindgen --target web --out-dir web/mcp-pkg --no-typescript target/wasm32-unknown-unknown/release/ergo_client.wasm
