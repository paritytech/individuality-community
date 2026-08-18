#!/bin/sh
# Apply formatters in place: rustfmt, zepter, taplo.

set -eu

# Check required tools
for cmd in cargo zepter taplo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "error: '$cmd' not found. Please install it first." >&2; exit 1; }
done

# The nightly formatter needs an explicit override; its version lives in
# .github/env.
. .github/env
FMT_TOOLCHAIN="+nightly-${RUST_NIGHTLY_VERSION}"

cargo $FMT_TOOLCHAIN fmt --all
zepter run
taplo format --config ./.config/taplo.toml
