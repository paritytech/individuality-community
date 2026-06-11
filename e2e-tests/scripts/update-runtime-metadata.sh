#!/bin/sh

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

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
E2E_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(CDPATH= cd -- "$E2E_ROOT/.." && pwd)
PAPI_BIN="$E2E_ROOT/node_modules/.bin/papi"

resolve_repo_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$REPO_ROOT" "$1" ;;
  esac
}

# CI can point these at downloaded artifacts; local runs fall back to the usual release build outputs.
NEXT_PEOPLE_PASEO_RUNTIME_WASM=$(resolve_repo_path "${NEXT_PEOPLE_PASEO_RUNTIME_WASM:-target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm}")

require_readable() {
  if [ ! -r "$1" ]; then
    printf 'Required file is missing or unreadable: %s\n' "$1" >&2
    exit 1
  fi
}

require_readable "$PAPI_BIN"
require_readable "$NEXT_PEOPLE_PASEO_RUNTIME_WASM"

TMPDIR_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/individuality-papi-XXXXXX")
trap 'rm -rf "$TMPDIR_ROOT"' EXIT INT TERM HUP

# `papi add --wasm` writes metadata relative to the current working directory,
# so we give it a disposable config rooted in a temp directory.
cat > "$TMPDIR_ROOT/polkadot-api.json" <<'EOF'
{
  "version": 0,
  "descriptorPath": ".papi/descriptors",
  "entries": {}
}
EOF

(
  cd "$TMPDIR_ROOT"

  # We derive metadata from the newly built runtime WASM so `papi generate` reflects
  # the runtime we upgrade into, not whatever the source chain happened to run before.
  "$PAPI_BIN" add paseoPeople \
    --config "$TMPDIR_ROOT/polkadot-api.json" \
    --wasm "$NEXT_PEOPLE_PASEO_RUNTIME_WASM" \
    --skip-codegen
)

require_readable "$TMPDIR_ROOT/.papi/metadata/paseoPeople.scale"

# Copy the freshly extracted metadata into the local PAPI metadata directory for
# the current run; this file is generated locally/CI and not tracked in git.
mkdir -p "$E2E_ROOT/.papi/metadata"
cp "$TMPDIR_ROOT/.papi/metadata/paseoPeople.scale" "$E2E_ROOT/.papi/metadata/paseoPeople.scale"
printf 'Updated paseoPeople metadata from %s\n' "$NEXT_PEOPLE_PASEO_RUNTIME_WASM"
