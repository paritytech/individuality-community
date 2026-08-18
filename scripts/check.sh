#!/bin/sh
# Local checks: fmt → clippy → tests
# The setup mirrors CI, but skips the WASM build for speed.

set -eu

# Check required tools
for cmd in cargo taplo zepter cargo-nextest; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "error: '$cmd' not found. Please install it first." >&2; exit 1; }
done

# Plain `cargo` uses the stable toolchain pinned in rust-toolchain.toml. Only
# the nightly formatter needs an explicit override; its version lives in
# .github/env.
. .github/env
FMT_TOOLCHAIN="+nightly-${RUST_NIGHTLY_VERSION}"

# Step 1: Format checks
cargo $FMT_TOOLCHAIN fmt --all --check
taplo format --check --diff --config ./.config/taplo.toml
zepter run check

# Skip WASM build for local checks — substrate-wasm-builder respects this env
# var and emits a stub instead, avoiding the heavy wasm-opt chain.
export SKIP_WASM_BUILD=1

# Match CI: deny warnings for both clippy and tests
export RUSTFLAGS="-D warnings"

# Step 2: Clippy
cargo clippy --workspace --locked --all-targets --all-features

# Step 3: Tests reuse the Clippy build artefacts.
cargo nextest run --workspace --locked --all-targets --all-features --no-fail-fast
