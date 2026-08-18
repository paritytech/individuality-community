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

# Generate local_testnet chain specs for both parachains from the built WASM.
# These specs carry the //Alice sudo key and the Alice+Bob collator set — unlike
# the live `paseo-next-*.chainspec.json` snapshots, which cannot be spawned locally.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SPECS="$HERE/specs"
PEOPLE_COLLATOR="5D2Qh7L3QdD8e6tFxLMdQNmGfibTt4FuYPwp1rSkg2Lptfip"
AH_COLLATOR="5G8nzbRev2qjY3kqCHgqQBY7hC738wrPwsHzU68ov2VLCKhH"
GENESIS_BALANCE=10000000000000000

# Use the pinned chain-spec-builder (shipped with the pinned polkadot-omni-node).
export PATH="$HERE/.tools/bin:$PATH"

mkdir -p "$SPECS"

PEOPLE_WASM="$ROOT/target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm"
AH_WASM="$ROOT/target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime.compact.compressed.wasm"

for w in "$PEOPLE_WASM" "$AH_WASM"; do
	[ -f "$w" ] || { echo "missing $w — run 01-build-runtimes.sh first" >&2; exit 1; }
done

# `--relay-chain rococo` matches the `rococo-local` relay the local `polkadot`
# binary exposes (see network.toml). Para IDs are the canonical paseo values.
polkadot-omni-node chain-spec-builder -c "$SPECS/people-local.json" create \
	--relay-chain rococo --para-id 1502 \
	--runtime "$PEOPLE_WASM" named-preset local_testnet

polkadot-omni-node chain-spec-builder -c "$SPECS/ah-local.json" create \
	--relay-chain rococo --para-id 1500 \
	--runtime "$AH_WASM" named-preset local_testnet

# Give each custom spec a distinct identity so zombienet stages separate files
# instead of collapsing both parachains into the same generic `custom` spec.
jq '.name = "People Local" | .id = "people-local" | .protocolId = "people-local"' \
	"$SPECS/people-local.json" > "$SPECS/people-local.json.tmp"
mv "$SPECS/people-local.json.tmp" "$SPECS/people-local.json"

jq '.name = "Asset Hub Local" | .id = "asset-hub-local" | .protocolId = "asset-hub-local"' \
	"$SPECS/ah-local.json" > "$SPECS/ah-local.json.tmp"
mv "$SPECS/ah-local.json.tmp" "$SPECS/ah-local.json"

# Match the genesis invulnerables/session authorities to the collator names that
# zombienet actually boots (`//People-collator` and `//Ah-collator`), otherwise
# the nodes stay at block 0 because the generated local_testnet specs still
# expect Alice/Bob to author parachain blocks.
jq \
	--arg collator "$PEOPLE_COLLATOR" \
	--argjson balance "$GENESIS_BALANCE" \
	'.chainType = "Local"
	| .genesis.runtimeGenesis.patch.collatorSelection.invulnerables = [$collator]
	| .genesis.runtimeGenesis.patch.session.keys = [
		[$collator, $collator, {"aura": $collator}]
	]
	| .genesis.runtimeGenesis.patch.balances.balances += [[$collator, $balance]]' \
	"$SPECS/people-local.json" > "$SPECS/people-local.json.tmp"
mv "$SPECS/people-local.json.tmp" "$SPECS/people-local.json"

jq \
	--arg collator "$AH_COLLATOR" \
	--argjson balance "$GENESIS_BALANCE" \
	'.chainType = "Local"
	| .genesis.runtimeGenesis.patch.collatorSelection.invulnerables = [$collator]
	| .genesis.runtimeGenesis.patch.session.keys = [
		[$collator, $collator, {"aura": $collator}]
	]
	| .genesis.runtimeGenesis.patch.balances.balances += [[$collator, $balance]]' \
	"$SPECS/ah-local.json" > "$SPECS/ah-local.json.tmp"
mv "$SPECS/ah-local.json.tmp" "$SPECS/ah-local.json"

# Prebuild raw specs so zombienet doesn't try to derive both parachains from the
# same generic `custom` staging path.
polkadot-omni-node build-spec \
	--chain "$SPECS/people-local.json" \
	--disable-default-bootnode \
	--raw > "$SPECS/people-local.raw.json"

polkadot-omni-node build-spec \
	--chain "$SPECS/ah-local.json" \
	--disable-default-bootnode \
	--raw > "$SPECS/ah-local.raw.json"

echo "wrote $SPECS/people-local.json, $SPECS/people-local.raw.json, $SPECS/ah-local.json, and $SPECS/ah-local.raw.json"
