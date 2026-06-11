#!/bin/sh
# Apply formatters in place: rustfmt, taplo, zepter.

set -e

# Check required tools
for cmd in taplo zepter rg cargo; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "error: '$cmd' not found. Please install it first." >&2; exit 1; }
done

# Load same toolchain versions as in CI
. .github/env
FMT_TOOLCHAIN="+nightly-${RUST_NIGHTLY_VERSION}"

cargo $FMT_TOOLCHAIN fmt --all &
taplo format --config ./.config/taplo.toml &
zepter run &
wait
