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
- `tests/runtime-upgrade` forks Paseo People with Chopsticks and injects a local runtime build.

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

Optional environment variables:

- `NEXT_PEOPLE_PASEO_UPGRADE_TARGETS=next-paseo` (or a `wss://` endpoint running next-people-paseo)
- `NEXT_PEOPLE_PASEO_UPGRADE_BLOCK=<block-number-or-hash>`
- `NEXT_PEOPLE_PASEO_UPGRADE_DB=.cache/nextPeoplePaseo.runtime-upgrade.sqlite`
- `NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL=0`
- `NEXT_PEOPLE_PASEO_RUNTIME_WASM=target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm`

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
