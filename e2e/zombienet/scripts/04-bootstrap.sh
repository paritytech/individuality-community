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

# Initialise the running network with the shared bash suite `scripts/initial-setup`
# (ENV=local). This is the SAME flow used for real environments. We run it as-is:
# there is no Bulletin node, but the only Bulletin-touching step (01c) is a harmless
# no-op without one (it only submits a People sudo->XCM; the relay-side channel-open
# to the unregistered 1501 fails/dangles asynchronously and does not affect People<->AH).
#
# Run ONCE per fresh spawn: start.sh is all-or-nothing (set -euo pipefail) and not
# fully idempotent — re-running on an already-initialised chain may fail a guard.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

exec env ENV=local bash "$ROOT/scripts/initial-setup/start.sh" local
