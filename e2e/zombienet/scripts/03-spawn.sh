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

# Spawn the local relay + People + Asset Hub network. Leave running (foreground).
#
# Binaries come from the suite-local .tools/bin (scripts/00-install-binaries.sh):
#   - polkadot, polkadot-execute-worker, polkadot-prepare-worker  (relay bundle)
#   - polkadot-omni-node                                          (parachain collator)
#   - zombie-cli                                                  (orchestrator)
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
cd "$HERE"
BASE="$HERE/.tools/net"

# Prefer the pinned binaries over any globally-installed/generic versions.
export PATH="$HERE/.tools/bin:$PATH"

for spec in specs/people-local.json specs/people-local.raw.json specs/ah-local.json specs/ah-local.raw.json; do
	[ -f "$spec" ] || { echo "missing $spec — run bash scripts/02-gen-specs.sh first" >&2; exit 1; }
done

for bin in polkadot polkadot-omni-node zombie-cli; do
	command -v "$bin" >/dev/null || { echo "$bin not found on PATH — run bash scripts/00-install-binaries.sh first" >&2; exit 1; }
done

# Assert network.toml's RPC ports agree with config-local.env before spawning (see the script).
bash "$HERE/scripts/helper/check-rpc-ports.sh" "$HERE/network.toml"

# Preflight: this is the first thing zombienet does, and where a node that predates
# the pinned polkadot-sdk release fails with a missing host import (e.g.
# ext_statement_store_remove_by_version_1). Run it up front so the fix is obvious.
if ! err=$(polkadot-omni-node export-genesis-wasm --chain specs/people-local.json 2>&1 >/dev/null); then
	echo "$err" >&2
	if echo "$err" | grep -q "function imports which are not present"; then
		rel=$(grep -E '^POLKADOT_RELEASE_(VERSION|PATCH)=' "$ROOT/.github/env" 2>/dev/null | cut -d= -f2 | tr -d '\n')
		echo >&2
		echo "==> Your node binaries are older than the pinned polkadot-sdk release (${rel:-see .github/env})." >&2
		echo "    Reinstall with scripts/00-install-binaries.sh (collator AND relay bundle)." >&2
	fi
	exit 1
fi

# Fresh base dir each spawn (zombienet-sdk warns but proceeds on a non-empty --dir).
rm -rf "$BASE"; mkdir -p "$BASE"
exec zombie-cli spawn network.toml --provider native --dir "$BASE"
