# E2E Tests

End-to-end tests for Individuality using [Chopsticks](https://github.com/AcalaNetwork/chopsticks) for chain forking and [Vitest](https://vitest.dev/) as the test runner.

## Setup

```bash
pnpm install
cargo build --release -p next-people-paseo-runtime --locked
pnpm run papi:update:runtimes
pnpm exec papi generate
```

`paseoPeople.scale` is generated from the local runtime build and ignored by git, so rerun the metadata refresh after runtime changes.

## Running tests

```bash
pnpm test
pnpm run test:runtime-upgrade
pnpm run test:runtime-upgrade:paseo-to-next-people-paseo
pnpm run test:watch
pnpm run test:verbose
```

## Runtime Upgrade Test

The remaining suite forks the live Paseo People endpoint, swaps in the local `next-people-paseo` WASM from this repository, then builds a block with the injected runtime.

Use `pnpm run test:runtime-upgrade` to test the runtime upgrade. It currently targets the `paseo-to-next-people-paseo` suite.

Optional environment variables:

- `NEXT_PEOPLE_PASEO_UPGRADE_TARGETS=next-paseo,paseo-review`
- `NEXT_PEOPLE_PASEO_UPGRADE_BLOCK=<block-number-or-hash>`
- `NEXT_PEOPLE_PASEO_UPGRADE_DB=.cache/nextPeoplePaseo.runtime-upgrade.sqlite`
- `NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL=0`
- `NEXT_PEOPLE_PASEO_RUNTIME_WASM=target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm`

## Before pushing

```bash
pnpm run format
pnpm run lint:fix
```

## License

Copyright (C) Parity Technologies (UK) Ltd.

This file is part of Individuality and is licensed under the Apache License, Version 2.0.
You may obtain a copy of the License at [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0).

Unless required by applicable law or agreed to in writing, software distributed under the License is distributed on an "AS IS" BASIS, without warranties or conditions of any kind, either express or implied. See the License for the specific language governing permissions and limitations under the License.
