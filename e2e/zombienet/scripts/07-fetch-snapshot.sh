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

# Download the CI-published initialised-state snapshot into .tools/snapshot/, verify integrity and
# (if local specs are present) spec validity, so you can spawn it locally without bootstrapping.
#
# The snapshot is produced by .github/workflows/zombienet-snapshot.yml and published as a rolling
# GitHub prerelease (default tag `e2e-zombienet-snapshot`). It is SPEC/VERSION-LOCKED: it only boots
# against the same runtime WASM it was built from, so this script compares the published parachain
# spec sha256s against your local specs/*.json and warns on drift. It also downloads the captured relay
# spec (rococo-local.raw.json), which network-snap.toml pins because the relay genesis isn't reproducible
# across build hosts (see 05/06).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$HERE"
SNAP_DIR="$HERE/.tools/snapshot"
TAG="${SNAPSHOT_TAG:-e2e-zombienet-snapshot}"
REPO="${SNAPSHOT_REPO:-paritytech/individuality}"

command -v gh >/dev/null 2>&1 || {
  echo "gh (GitHub CLI) is required to download from the private repo: https://cli.github.com" >&2
  echo "After installing, run 'gh auth login'. (Override the source with SNAPSHOT_REPO / SNAPSHOT_TAG.)" >&2
  exit 1
}

mkdir -p "$SNAP_DIR"
echo "downloading snapshot '$TAG' from $REPO -> $SNAP_DIR"
gh release download "$TAG" --repo "$REPO" --dir "$SNAP_DIR" --clobber \
  --pattern '*.tgz' --pattern 'rococo-local.raw.json' --pattern 'manifest.json' --pattern 'SHA256SUMS'

# Integrity: verify the tarball checksums.
if [ -f "$SNAP_DIR/SHA256SUMS" ]; then
  ( cd "$SNAP_DIR" && shasum -a 256 -c SHA256SUMS ) || { echo "checksum verification FAILED" >&2; exit 1; }
else
  echo "WARNING: no SHA256SUMS in the release — skipping integrity check." >&2
fi

# Validity: the snapshot only boots against the runtime it was built from. Compare to local specs.
manifest="$SNAP_DIR/manifest.json"
if [ -f "$manifest" ] && [ -f specs/people-local.json ] && [ -f specs/ah-local.json ]; then
  lp=$(shasum -a 256 specs/people-local.json | awk '{print $1}')
  la=$(shasum -a 256 specs/ah-local.json | awk '{print $1}')
  mp=$(jq -r '.specs_sha256.people' "$manifest")
  ma=$(jq -r '.specs_sha256.ah' "$manifest")
  if [ "$lp" = "$mp" ] && [ "$la" = "$ma" ]; then
    echo "OK: snapshot matches your local specs (commit $(jq -r .commit "$manifest"), release $(jq -r .polkadot_release "$manifest"))."
  else
    echo "WARNING: snapshot was built from DIFFERENT runtime specs than your local ones." >&2
    echo "  spawn:snap will likely be rejected (genesis-hash mismatch)." >&2
    echo "  Rebuild your runtime to match the snapshot's commit ($(jq -r .commit "$manifest")), or" >&2
    echo "  regenerate locally with bash scripts/03-spawn.sh -> bash scripts/04-bootstrap.sh -> bash scripts/05-snapshot.sh." >&2
  fi
else
  echo "NOTE: no local specs/ to verify against — run 'bash scripts/02-gen-specs.sh' first to confirm the snapshot"
  echo "      matches your runtime. (snapshot commit: $(jq -r '.commit // "?"' "$manifest" 2>/dev/null))"
fi

echo
echo "fetched into $SNAP_DIR. Boot it with: bash scripts/06-spawn-from-snapshot.sh"
