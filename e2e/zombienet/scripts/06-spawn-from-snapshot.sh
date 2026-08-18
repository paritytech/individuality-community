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

# Re-spawn the network from a captured snapshot (scripts/05-snapshot.sh). Each node boots with the
# post-bootstrap state already in its DB, so NO initial-setup run is needed — just spawn.
#
# Uses the committed `network-snap.toml` (network.toml plus per-node `db_snapshot` lines pointing
# at .tools/snapshot/<node>.tgz, and a relay `chain_spec_path` pinning the captured relay spec at
# .tools/snapshot/rococo-local.raw.json). Relative paths resolve against the spawn CWD, which is
# this directory since we `cd "$HERE"` below.
#
# The pinned relay spec matters: the relay genesis isn't reproducible across build hosts, so without it
# the collators' relay nodes boot a mismatched genesis and the relay stalls at #0. See 05-snapshot.sh 4b.
#
# Leave running (foreground), like 03-spawn.sh.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$HERE"
export PATH="$HERE/.tools/bin:$PATH"

SNAP_DIR="$HERE/.tools/snapshot"
BASE="$HERE/.tools/net"
NODES=(alice-relay-validator bob-relay-validator charlie-relay-validator dave-relay-validator people-collator ah-collator)

[ -f "$HERE/network-snap.toml" ] || { echo "missing $HERE/network-snap.toml" >&2; exit 1; }
for spec in specs/people-local.json specs/people-local.raw.json specs/ah-local.json specs/ah-local.raw.json; do
	[ -f "$spec" ] || { echo "missing $spec — run bash scripts/02-gen-specs.sh first" >&2; exit 1; }
done
for n in "${NODES[@]}"; do
	[ -f "$SNAP_DIR/$n.tgz" ] || {
		echo "missing $SNAP_DIR/$n.tgz — run bash scripts/05-snapshot.sh against a bootstrapped network first" >&2
		exit 1
	}
done
[ -f "$SNAP_DIR/rococo-local.raw.json" ] || {
	echo "missing $SNAP_DIR/rococo-local.raw.json — the pinned relay spec is part of the snapshot." >&2
	echo "Re-capture with bash scripts/05-snapshot.sh, or re-download with bash scripts/07-fetch-snapshot.sh." >&2
	exit 1
}
for bin in polkadot polkadot-omni-node zombie-cli; do
	command -v "$bin" >/dev/null || { echo "$bin not found on PATH — run bash scripts/00-install-binaries.sh first" >&2; exit 1; }
done

# Assert network-snap.toml's RPC ports agree with config-local.env before spawning (see the script).
bash "$HERE/scripts/helper/check-rpc-ports.sh" "$HERE/network-snap.toml"

# Same host-fn preflight as 03-spawn.sh (catches node binaries older than the pinned release).
if ! err=$(polkadot-omni-node export-genesis-wasm --chain specs/people-local.json 2>&1 >/dev/null); then
	echo "$err" >&2
	echo "==> Reinstall the pinned node binaries with scripts/00-install-binaries.sh." >&2
	exit 1
fi

# Fresh base dir each spawn; zombienet-sdk copies each db_snapshot tarball into <BASE>/<node> and unpacks it.
rm -rf "$BASE"; mkdir -p "$BASE"

echo "spawning from snapshot — nodes restore their post-bootstrap DB (db_snapshot from $SNAP_DIR)"
echo "no bootstrap is required; this snapshot already contains the verified post-init state"
exec zombie-cli spawn "$HERE/network-snap.toml" --provider native --dir "$BASE"
