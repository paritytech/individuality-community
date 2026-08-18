#!/usr/bin/env bash

# Copyright (C) Parity Technologies (UK) Ltd.
# This file is part of Individuality.
# SPDX-License-Identifier: Apache-2.0
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# Install the node binaries into the suite-local `.tools/bin` by delegating to the vendored
# scripts/get_polkadot_binaries.sh:
#   * polkadot relay bundle + polkadot-omni-node — BUILT from source at the polkadot-sdk release tag
#     pinned in `.github/env` (POLKADOT_RELEASE_VERSION + POLKADOT_RELEASE_PATCH). Built, not
#     downloaded, because the `*-local` relay chains need `fast_runtime_binary`, which prebuilt release
#     binaries omit; the vendored script builds polkadot `--features fast-runtime`.
#   * zombie-cli — DOWNLOADED + sha256-verified at ZOMBIENET_SDK_VERSION
#     pinned in `.github/env`.
# The `.github/env` pins are the single source of truth — no separate lock file.
#
# First run clones polkadot-sdk + builds (~30-60 min); get_polkadot_binaries.sh keeps a per-ref cache
# under `.tools/cache`, so subsequent runs are instant. We symlink the resolved binaries into the
# stable `.tools/bin` path that 02-gen-specs.sh / 03-spawn.sh put on PATH.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TOOLS="$HERE/.tools"
BIN="$TOOLS/bin"
GET="$HERE/scripts/get_polkadot_binaries.sh"

mkdir -p "$BIN"

# Load the pinned versions (POLKADOT_RELEASE_*, ZOMBIENET_SDK_*) so get_polkadot_binaries.sh sees them.
[ -f "$ROOT/.github/env" ] || { echo "missing $ROOT/.github/env" >&2; exit 1; }
set -a; . "$ROOT/.github/env"; set +a
: "${POLKADOT_RELEASE_VERSION:?not set in .github/env}"
TAG="${POLKADOT_RELEASE_VERSION}${POLKADOT_RELEASE_PATCH:-}"

# macOS: help bindgen (librocksdb-sys) find libclang during the polkadot build.
if [ "$(uname -s)" = "Darwin" ] && [ -z "${LIBCLANG_PATH:-}" ]; then
	for cand in "$(brew --prefix llvm 2>/dev/null)/lib" /opt/homebrew/opt/llvm/lib /Library/Developer/CommandLineTools/usr/lib; do
		if [ -e "$cand/libclang.dylib" ]; then export LIBCLANG_PATH="$cand"; break; fi
	done
fi

export POLKADOT_BINARIES_DIR="$TOOLS/cache"

# polkadot bundle + omni-node, built from the .github/env release TAG. FORCE_SOURCE_BUILD=1 makes the
# vendored script build the tag from source (a non-commit ref would otherwise be downloaded — and the
# prebuilt lacks fast_runtime_binary). POLKADOT_NODE_VERSION carries the tag — it IS the .github/env pin.
echo "polkadot bundle from polkadot-sdk@$TAG (built from source; first run clones polkadot-sdk + ~30-60 min, cached after)"
node_dir="$(POLKADOT_NODE_VERSION="$TAG" FORCE_SOURCE_BUILD=1 bash "$GET" polkadot-node)"

# zombie-cli, downloaded + sha256-verified at ZOMBIENET_SDK_VERSION (pinned in .github/env).
echo "zombie-cli ${ZOMBIENET_SDK_VERSION:-<unset>} (downloaded, sha256-verified)"
zn_dir="$(bash "$GET" zombienet-sdk)"

# Expose everything at the stable .tools/bin path (gen-specs/spawn prepend it to PATH).
for src in "$node_dir/polkadot" "$node_dir/polkadot-prepare-worker" "$node_dir/polkadot-execute-worker" \
	"$node_dir/polkadot-omni-node" "$zn_dir/zombie-cli"; do
	[ -x "$src" ] || { echo "expected binary missing: $src" >&2; exit 1; }
	ln -sf "$src" "$BIN/$(basename "$src")"
done

echo
echo "Installed in $BIN:"
for b in polkadot polkadot-execute-worker polkadot-prepare-worker polkadot-omni-node zombie-cli; do
	[ -x "$BIN/$b" ] && printf '  %-26s %s\n' "$b" "$("$BIN/$b" --version 2>/dev/null | head -1)"
done
echo
echo "spawn/gen-specs put $BIN on PATH automatically."
