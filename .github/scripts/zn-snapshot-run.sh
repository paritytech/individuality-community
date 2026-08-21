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

# Full zombienet snapshot run, driven by .github/workflows/zombienet-snapshot.yml
#   1. spawn the network in the background
#   2. wait for all three chains to start producing blocks (fail if they never do)
#   3. run the initial-setup bootstrap
#   4. verify with the init-state test
#   5. capture the snapshot (which stops the network)
#
# The `::group::`/`::endgroup::` lines are GitHub Actions log-fold markers; they print
# harmlessly as plain text when run locally. Logs go to $RUNNER_TEMP (so the workflow's
# later log-tail + diagnostics steps find them), falling back to a temp dir locally.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)" # repo root (.github/scripts -> up two)
cd "$ROOT/e2e"

TMP="${RUNNER_TEMP:-$(mktemp -d)}"
LOG="$TMP/zn-spawn.log"
# shellcheck source=/dev/null
. "$ROOT/scripts/initial-setup/config-local.env"

export PEOPLE_RPC="$RPC_PEOPLE"
export ASSET_HUB_RPC="$RPC_ASSET_HUB"

# Best block number (hex, e.g. "0x1a3") of the node at HTTP RPC url $1, or "" if it's down or
# doesn't reply within 4s. (`|| true` so a failed query doesn't abort the script under `set -e`.)
hdr() {
	curl -s -m 4 -H 'Content-Type: application/json' \
		-d '{"jsonrpc":"2.0","id":1,"method":"chain_getHeader","params":[]}' \
		"$1" 2>/dev/null | sed -n 's/.*"number":"\(0x[0-9a-f]*\)".*/\1/p' || true
}

# ws://→http://, wss://→https:// so curl can POST to the node.
RELAY_HTTP_RPC=$(printf '%s' "$RPC_RELAY" | sed 's|^ws|http|')
PEOPLE_HTTP_RPC=$(printf '%s' "$RPC_PEOPLE" | sed 's|^ws|http|')
ASSET_HUB_HTTP_RPC=$(printf '%s' "$RPC_ASSET_HUB" | sed 's|^ws|http|')

echo "::group::spawn network (background)"
just spawn > "$LOG" 2>&1 &
SPAWN_PID=$!

# Wait for the network to come up: poll all three chains until each reports a block past genesis
# (non-empty and not 0x0). A healthy network gets here in 1-2 min; give up after 75 tries
# (~5 min) or if the spawn process dies, and fail.
ok=""
for _ in $(seq 1 75); do
	r=$(hdr "$RELAY_HTTP_RPC"); p=$(hdr "$PEOPLE_HTTP_RPC"); a=$(hdr "$ASSET_HUB_HTTP_RPC")
	if [ -n "$r" ] && [ "$r" != "0x0" ] && [ -n "$p" ] && [ "$p" != "0x0" ] && [ -n "$a" ] && [ "$a" != "0x0" ]; then
		echo "network up: relay=$r people=$p ah=$a"; ok=1; break
	fi
	kill -0 "$SPAWN_PID" 2>/dev/null || { echo "spawn exited early"; break; }
	sleep 4
done
echo "::endgroup::"
[ -n "$ok" ] || { echo "network did not come up"; tail -200 "$LOG" || true; exit 1; }

echo "::group::bootstrap (initial-setup, 29 scripts)"
just bootstrap 2>&1 | tee "$TMP/zn-bootstrap.log"
echo "::endgroup::"

echo "::group::verify (init-state test)"
pnpm run test:init-state
echo "::endgroup::"

echo "::group::capture snapshot (stops the network)"
just snapshot
echo "::endgroup::"
