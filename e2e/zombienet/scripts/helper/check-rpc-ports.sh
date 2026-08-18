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

# Assert the RPC ports in a zombienet network TOML ($1, default network.toml) agree with the
# RPC_* URLs in scripts/initial-setup/config-local.env. The two are duplicated because
# zombienet TOML can't read shell env, so a port changed in one but not the other would only
# surface as confusing connection errors during bootstrap. Called by the spawn scripts
# (03-spawn.sh, 06-spawn-from-snapshot.sh) to fail fast instead. Exits non-zero on any mismatch.
set -euo pipefail

ZN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)" # e2e/zombienet (this script is in scripts/helper)
ROOT="$(cd "$ZN_DIR/../.." && pwd)"                          # repo root
TOML="${1:-$ZN_DIR/network.toml}"

[ -f "$TOML" ] || { echo "check-rpc-ports: missing TOML $TOML" >&2; exit 1; }
# shellcheck source=/dev/null
. "$ROOT/scripts/initial-setup/config-local.env"

toml_name="$(basename "$TOML")"
mismatch=0
# network.toml node name : the config-local.env URL the bootstrap uses to reach that node
for pair in "alice-relay-validator:$RPC_RELAY" "people-collator:$RPC_PEOPLE" "ah-collator:$RPC_ASSET_HUB"; do
	node="${pair%%:*}"
	port_env="${pair##*:}" # ws://localhost:10000 -> 10000
	port_toml=$(awk -v n="$node" '$0 ~ "name = \"" n "\"" {f=1} f && /rpc_port/ {gsub(/[^0-9]/, ""); print; exit}' "$TOML")
	if [ "$port_env" != "$port_toml" ]; then
		echo "ERROR: RPC port mismatch for $node: $toml_name=$port_toml config-local.env=$port_env" >&2
		mismatch=1
	fi
done
[ "$mismatch" -eq 0 ] || { echo "Update the port so $toml_name and config-local.env agree." >&2; exit 1; }
