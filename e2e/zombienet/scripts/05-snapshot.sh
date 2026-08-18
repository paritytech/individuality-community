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

# Capture the post-initialisation chain state of a RUNNING, bootstrapped network into per-node DB
# snapshots under .tools/snapshot/, so it can be re-spawned (06-spawn-from-snapshot.sh) WITHOUT
# re-running the ~29-script initial-setup bootstrap. Boot then drops from minutes to seconds.
#
# WHAT IS CAPTURED, AND WHY ALL OF IT:
#   We snapshot all 6 nodes (4 relay validators + People & AH collators) at the SAME stop point.
#   A parachain DB is coupled to its relay's block history (a People DB at relay-parent #N won't
#   resume against a fresh relay at #0), and the People<->AH HRMP channels live in RELAY state
#   (opened at runtime by initial-setup scripts) — so the whole network must be captured together.
#   The collators' embedded relay full-node (the `relay-data/` sibling of `data/`) is intentionally
#   NOT captured: it re-syncs in seconds from the restored relay validators, and we only snapshot
#   each node's `data/` anyway.
#   We also capture the relay raw chain spec (rococo-local.raw.json); it is pinned on restore (step 4b)
#   so every relay node shares the snapshot's genesis regardless of build host.
#
# HOW THE TARBALL IS SHAPED:
#   zombienet-sdk's db_snapshot restore unpacks the tarball into the node's base dir (<NS>/<node>),
#   so the tarball must contain `data/...` entries to land at <NS>/<node>/data. We create them with
#   `tar -C <node-dir> data`.
#
# SPEC/VERSION-LOCKED: a snapshot is valid only while the parachain specs/*.json and the node binaries
#   are unchanged. The restored parachain DBs' genesis must match the freshly-generated parachain spec
#   or the node rejects them as the wrong chain. The relay is instead locked to the captured
#   rococo-local.raw.json (step 4b), which ships with the snapshot. Re-snapshot after any runtime /
#   spec / binary change.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$HERE"
CONFIG_LOCAL_ENV="$HERE/../../scripts/initial-setup/config-local.env"
SNAP_DIR="$HERE/.tools/snapshot"
NODES=(alice-relay-validator bob-relay-validator charlie-relay-validator dave-relay-validator people-collator ah-collator)

. "$CONFIG_LOCAL_ENV"

rpc_http_url() {
  local ws_url="$1"
  case "$ws_url" in
    ws://*) printf '%s\n' "http://${ws_url#ws://}" ;;
    wss://*) printf '%s\n' "https://${ws_url#wss://}" ;;
    *) printf '%s\n' "$ws_url" ;;
  esac
}

RELAY_HTTP_RPC="$(rpc_http_url "$RPC_RELAY")"
PEOPLE_HTTP_RPC="$(rpc_http_url "$RPC_PEOPLE")"
ASSET_HUB_HTTP_RPC="$(rpc_http_url "$RPC_ASSET_HUB")"

# 1. The network was spawned into a FIXED base dir by 03-spawn.sh (zombie-cli --dir .tools/net), so each
#    node's DB is at <NS>/<node>/data. Verify it's actually up before capturing.
NS="$HERE/.tools/net"
pgrep -x polkadot >/dev/null 2>&1 || {
  echo "No running zombienet network found (no live 'polkadot' relay process)." >&2
  echo "Start it (bash scripts/03-spawn.sh) and bootstrap it (bash scripts/04-bootstrap.sh) before snapshotting." >&2
  exit 1
}
echo "namespace: $NS"
for n in "${NODES[@]}"; do
  [ -d "$NS/$n/data" ] || { echo "missing $NS/$n/data — wrong or partial network (spawned with a different --dir)?" >&2; exit 1; }
done

# 2. Record block heights now, while the RPCs are still up (best-effort, for the manifest).
declare -a HEIGHTS
for rpc in "$RELAY_HTTP_RPC" "$PEOPLE_HTTP_RPC" "$ASSET_HUB_HTTP_RPC"; do
  h="$(curl -s -m 3 -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"chain_getHeader","params":[]}' \
    "$rpc" 2>/dev/null | sed -n 's/.*"number":"\(0x[0-9a-f]*\)".*/\1/p')"
  HEIGHTS+=("$rpc=${h:-?}")
done
echo "heights: ${HEIGHTS[*]}"

# 3. Graceful stop so RocksDB flushes: stop the orchestrator, then SIGTERM the nodes and wait for exit.
echo "stopping network (graceful, for a clean RocksDB flush)..."
pkill -TERM -f 'zombie-cli' 2>/dev/null || true
pkill -TERM -x polkadot 2>/dev/null || true
pkill -TERM -f 'polkadot-omni-node' 2>/dev/null || true
for _ in $(seq 1 40); do
  if ! pgrep -x polkadot >/dev/null 2>&1 && ! pgrep -f 'polkadot-omni-node' >/dev/null 2>&1; then break; fi
  sleep 1
done
# Reap any stragglers (PVF workers, or a node that ignored SIGTERM) so the DB files are quiescent.
pkill -KILL -f 'polkadot-execute-worker|polkadot-prepare-worker' 2>/dev/null || true
pkill -KILL -x polkadot 2>/dev/null || true
pkill -KILL -f 'polkadot-omni-node' 2>/dev/null || true
sleep 2

# 4. Tar each node's `data/` (para DB + keystore + network identity; NOT the collators' relay-data/).
mkdir -p "$SNAP_DIR"
rm -f "$SNAP_DIR"/*.tgz
for n in "${NODES[@]}"; do
  echo "  taring $n/data -> $SNAP_DIR/$n.tgz"
  tar -czf "$SNAP_DIR/$n.tgz" -C "$NS/$n" data
done

# 4b. Capture the relay raw chain spec too. Zombienet regenerates rococo-local from the polkadot binary
#     each spawn, and its fast-runtime WASM is not byte-identical across build hosts (verified: a CI
#     Linux build and a local macOS build differ), so a fetched snapshot's relay genesis only matches on
#     the host that built it. Pin it (relay chain_spec_path in network-snap.toml) so the collators' relay
#     nodes don't boot a mismatched genesis and strand the relay at #0. bootNodes cleared (outside genesis).
RELAY_SPEC_SRC="$NS/rococo-local.json"
[ -f "$RELAY_SPEC_SRC" ] || { echo "missing $RELAY_SPEC_SRC — cannot capture the relay chain spec" >&2; exit 1; }
echo "  capturing relay spec -> $SNAP_DIR/rococo-local.raw.json"
jq '.bootNodes = []' "$RELAY_SPEC_SRC" > "$SNAP_DIR/rococo-local.raw.json"

# 5. Manifest (provenance + the spec checksums this snapshot is locked to).
{
  echo "# DB snapshot captured by 05-snapshot.sh"
  echo "namespace=$NS"
  echo "heights: ${HEIGHTS[*]}"
  echo "nodes: ${NODES[*]}"
  echo "specs_sha256:"
  shasum -a 256 specs/people-local.json specs/ah-local.json 2>/dev/null | sed 's/^/  /' || true
  echo "relay_spec_sha256:"
  shasum -a 256 "$SNAP_DIR/rococo-local.raw.json" 2>/dev/null | sed 's/^/  /' || true
  echo "tarball_sizes:"
  du -sh "$SNAP_DIR"/*.tgz 2>/dev/null | sed 's/^/  /' || true
} > "$SNAP_DIR/manifest.txt"

echo
echo "snapshot written to $SNAP_DIR:"
ls -la "$SNAP_DIR"
echo
echo "Re-spawn from it (no bootstrap needed): bash scripts/06-spawn-from-snapshot.sh"
