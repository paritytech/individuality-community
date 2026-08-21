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

# Best-effort diagnostics for a failed zombienet spawn/bootstrap, meant to run from a
# CI `if: failure()` step (but also runnable locally after a failed `just spawn`/`just
# bootstrap`). Collects into $1 (default: $RUNNER_TEMP/zombienet-diagnostics):
#   - copies of the spawn + bootstrap logs, if present
#   - the running zombie-cli/polkadot processes
#   - per chain: best header, finalized head, system health, and pending extrinsics

set +e # collect everything; a single failed query must not abort the dump

# Repo root, resolved from this script's location (.github/scripts) so it runs from anywhere.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

OUT="${1:-${RUNNER_TEMP:-/tmp}/zombienet-diagnostics}"
mkdir -p "$OUT"

# Spawn + bootstrap logs, if the CI step left any behind.
cp "${RUNNER_TEMP:-/tmp}"/zn-*.log "$OUT/" 2>/dev/null

# RPC endpoints (ws://) come from the local config, the single source of truth for the ports.
CONFIG="$ROOT/scripts/initial-setup/config-local.env"
# shellcheck source=/dev/null
. "$CONFIG" 2>/dev/null || echo "warning: could not source $CONFIG; RPC endpoints will be empty" >&2

{
	date -u
	echo
	ps -ef | grep -E 'zombie-cli|polkadot' | grep -v grep

	for entry in "relay:$RPC_RELAY" "people:$RPC_PEOPLE" "asset-hub:$RPC_ASSET_HUB"; do
		# ws://→http://, wss://→https:// so curl can POST to the node.
		url=$(printf '%s' "${entry#*:}" | sed 's|^ws|http|')
		echo
		echo "== ${entry%%:*} ($url) =="
		for method in chain_getHeader chain_getFinalizedHead system_health author_pendingExtrinsics; do
			printf '%s: ' "$method"
			curl -sS -m 5 -H 'content-type: application/json' \
				-d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":[]}" "$url" 2>&1
			echo
		done
	done
} > "$OUT/rpc-diagnostics.txt" 2>&1

echo "wrote diagnostics to $OUT"
