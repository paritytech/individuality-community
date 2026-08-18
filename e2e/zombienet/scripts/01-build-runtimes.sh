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

# Build the People and Asset Hub runtimes (release WASM) used by this suite.
set -euo pipefail

# Repo root is four levels up: scripts -> zombienet -> suites -> e2e -> repo root.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

SKIP_PALLET_REVIVE_FIXTURES=1 cargo +1.93.0 build --release --locked \
	-p next-people-paseo-runtime \
	-p next-asset-hub-paseo-runtime

echo "built:"
echo "  $ROOT/target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm"
echo "  $ROOT/target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime.compact.compressed.wasm"
