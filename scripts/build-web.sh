#!/usr/bin/env bash
set -euo pipefail
# Legacy entry point; the build implementation is Rust (`cargo xtask build-web`).
exec cargo run --locked --manifest-path "$(dirname "${BASH_SOURCE[0]}")/../Cargo.toml" --package xtask -- build-web "$@"
