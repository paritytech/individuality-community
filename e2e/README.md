# e2e

The single home for end-to-end testing in this repo.

```
e2e/
  zombienet/   local network harness for Relay + People (1502) + Asset Hub (1500)
  packages/
    shared/        @individuality-e2e/shared
    descriptors/   @individuality-e2e/descriptors-src -> generates @polkadot-api/descriptors
  suites/
    initialization-tests/   live init-state checks against the zombienet network
  tests/
    runtime-upgrade/        Chopsticks-based runtime-upgrade suite
  justfile      task entry points — `just <recipe>` wraps the shell scripts and pnpm commands
```

`e2e/` is a pnpm workspace. The zombienet harness lives beside the TypeScript packages and suites so
there is only one E2E home to discover and maintain. Everything below is driven via `just`, run from
the `e2e/` directory; `just --list` shows every recipe with its one-line description.

## What lives here

- `zombienet/` is infrastructure, not a pnpm package. It spawns and bootstraps the local network.
- `packages/shared` contains the connection helpers used by the live suites.
- `packages/descriptors` generates `@polkadot-api/descriptors` from locally built runtime WASM.
- `suites/initialization-tests` validates the bootstrapped local network state.
- `tests/runtime-upgrade` forks Paseo People and Paseo Asset Hub with Chopsticks and injects a local
  runtime build.

## Prerequisites

- Rust via the repo's pinned toolchain in `rust-toolchain.toml`
- Node from `.nvmrc`
- `pnpm` via corepack
- `dot` (`polkadot-cli`) for the zombienet bootstrap flow

Install the workspace once:

```bash
cd e2e
just install
```

## Runtime-upgrade suite

This is the fast suite that does not need the local zombienet harness.

```bash
cd e2e
just test-runtime-upgrade
```

Both chains are covered; run one on its own with `pnpm run test:runtime-upgrade:paseo-to-next-people-paseo`
or `pnpm run test:runtime-upgrade:paseo-to-next-asset-hub-paseo`. Each needs the matching runtime built
first. The release workflow enables metadata hashing for People. Asset Hub's production release build
keeps its existing feature selection; the E2E build action creates a separate metadata-hash-enabled
Asset Hub artifact for the enabled-mode compatibility case.
The suite also starts a local fork RPC for each chain and uses `@polkadot/api` to prove that a
pre-upgrade transaction is rejected while a freshly signed standard V4 transfer succeeds without
registering any Individuality extensions. It obtains V16 call bytes from PAPI, constructs a General/V1
extrinsic with the repository adapter and executes those exact bytes through the upgraded runtime.

After a transaction-extension pipeline upgrade, regenerate the descriptors before running either
upgrade suite:

```bash
pnpm --dir e2e descriptors:update
pnpm --dir e2e typecheck
pnpm --dir e2e lint
pnpm --dir e2e test:runtime-upgrade
```

## Transaction client transition

The upgraded Paseo runtimes expose two transaction-extension pipelines. Pipeline version, extrinsic
format version, metadata version and runtime transaction version are separate values.

- Standard wallet transfers remain legacy V4 signed transactions, but now use the frozen standard
  V0 extension pipeline. Clients must fetch the post-upgrade metadata and rebuild and re-sign every
  transaction. A pre-upgrade signature is not valid after the runtime upgrade.
- Individuality transactions use General/V1 transactions. Rebuild their proof and signature data
  against V1; do not submit the old bespoke V0 bytes or register them as a fallback extension.
- People V0 and V1 include `CheckMetadataHash`. Encode its mode and, when enabled, its metadata-hash
  signing data. Ordinary test transactions use disabled mode.
- Refresh PAPI descriptors and metadata before submission. Tooling that previously constructed the
  bespoke Individuality V0 pipeline must select V1 after activation.
- PAPI 3.1.0 is the minimum pinned version because the descriptor workflow must consume V16 metadata.
  Its stock transaction creator selects V0. Use the repository's `createAssetHubV1Extrinsic` or
  `createPeopleV1Extrinsic` adapter for the
  corresponding V1 General preamble and `VerifyMultiSignature` implication; do not use PAPI's default
  signer and assume it produced V1. The adapters are deliberately runtime-specific: Individuality
  clients must still supply their own authorization proofs and downstream extension values.

This repository only provides the runtime and compatibility coverage. Updating downstream wallets,
applications and public networks remains a rollout dependency.

Optional environment variables:

- `NEXT_PEOPLE_PASEO_UPGRADE_ENDPOINT=wss://paseo-people-next-system-rpc.polkadot.io`
- `NEXT_PEOPLE_PASEO_UPGRADE_BLOCK=<block-number-or-hash>`
- `NEXT_PEOPLE_PASEO_UPGRADE_DB=.cache/nextPeoplePaseo.runtime-upgrade.sqlite`
- `NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL=0`
- `NEXT_PEOPLE_PASEO_RUNTIME_WASM=target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm`
- `NEXT_ASSET_HUB_PASEO_UPGRADE_ENDPOINT=wss://paseo-asset-hub-next-rpc.polkadot.io`
- `NEXT_ASSET_HUB_PASEO_UPGRADE_BLOCK=<block-number-or-hash>`
- `NEXT_ASSET_HUB_PASEO_UPGRADE_DB=.cache/nextAssetHubPaseo.runtime-upgrade.sqlite`
- `NEXT_ASSET_HUB_PASEO_UPGRADE_RUNTIME_LOG_LEVEL=0`
- `NEXT_ASSET_HUB_PASEO_RUNTIME_WASM=target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime.wasm`
- `NEXT_ASSET_HUB_PASEO_METADATA_HASH_RUNTIME_WASM=target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime_metadata_hash.wasm`

## Zombienet + init-state suite

This is the realistic local-network path. The detailed runbook, readiness probe, and snapshot flow
live in [zombienet/README.md](./zombienet/README.md).

Build the network inputs (from `e2e/`):

```bash
just install-binaries
just build-runtimes
just gen-specs
just descriptors
```

Then use separate terminals:

```bash
# Terminal A — keep running
just spawn
```

```bash
# Terminal B — one-time bootstrap for a fresh spawn
just bootstrap
```

After the readiness probe in `zombienet/README.md` reports the network as ready:

```bash
# Terminal C
just test-init
```

Faster boot from a captured snapshot (no bootstrap needed):

```bash
just fetch-snapshot   # `just snapshot` saves a freshly bootstrapped network
just spawn-snap
```

## Workspace checks

```bash
cd e2e
pnpm run lint
pnpm run typecheck
pnpm run format:check
```

## Notes

- `packages/descriptors/.papi/descriptors` is a workspace member on purpose; consumers should use
  `workspace:*`, not `file:`.
- After runtime changes, rerun `just build-runtimes` + `just descriptors` before the live zombienet
  suite so the typed APIs match the spawned chain.
- The live init-state suite is intentionally separate from the default `pnpm test` flow because it
  depends on a running, bootstrapped local network.
- `justfile` is available as an optional wrapper around the same script and pnpm commands.

## License

Copyright (C) Parity Technologies (UK) Ltd.

This file is part of Individuality and is licensed under the Apache License, Version 2.0.
You may obtain a copy of the License at [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0).

Unless required by applicable law or agreed to in writing, software distributed under the License is
distributed on an "AS IS" BASIS, without warranties or conditions of any kind, either express or
implied. See the License for the specific language governing permissions and limitations under the
License.
