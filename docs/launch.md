# Launching a People chain

This guide covers building the Individuality runtime and running it as a local network, intended to be used by whoever wishes to deploy the code in this repository.

> **Scope:** this guide sets up the relay chain and the People parachain only. AssetHub and Bulletin (each via their own configs) must be set up separately.

## Prerequisites

You need a working Rust toolchain and the standard Polkadot SDK build dependencies (Clang/LLVM, `cmake`, `protobuf`, OpenSSL, etc.) for your platform. Follow the official [Install Polkadot SDK](https://docs.polkadot.com/develop/parachains/install-polkadot-sdk/) guide first — it covers macOS, Linux, and Windows (WSL). Missing these is the usual cause of build failures such as `librocksdb-sys` not finding `libclang`.

> **macOS note:** if you still hit the `librocksdb-sys`/`libclang` error after the setup above, install Homebrew LLVM and export its path — `brew install llvm && export LIBCLANG_PATH="$(brew --prefix llvm)/lib"` — making sure it's set in the same shell you run `cargo build` from.

## Build dependencies: solc & resolc

Upstream `pallet-revive` (used here by `precompiles/personhood`, `runtimes/next-asset-hub-paseo`, and `paseo-support/system-parachains-constants`) transitively depends on `pallet-revive-fixtures`, which compiles Solidity fixtures at build time. This requires `solc` and `resolc` to be on your `PATH`.

If you see this during a build:

```
Error: Failed to execute solc. Make sure solc is installed or set env variable `SKIP_PALLET_REVIVE_FIXTURES=1` to skip fixtures compilation.
```

you have two options:

1. **Install the binaries** (required for CI / benchmarks) — _Linux example; on other platforms find the matching binaries yourself_:
    ```bash
    # solc
    curl -L --fail -o /usr/local/bin/solc \
      "https://github.com/ethereum/solidity/releases/download/v0.8.30/solc-static-linux"
    chmod +x /usr/local/bin/solc

    # resolc
    curl -L --fail -o /usr/local/bin/resolc \
      "https://github.com/paritytech/revive/releases/download/v1.0.0/resolc-x86_64-unknown-linux-musl"
    chmod +x /usr/local/bin/resolc
    ```
2. **Skip the fixtures** (local dev when you don't need `pallet-revive`) by
   prefixing the build in the next step with `SKIP_PALLET_REVIVE_FIXTURES=1`.

## WASM runtime

Build the runtime to WASM:

```bash
cargo build --release -p next-people-paseo-runtime

# or, without solc/resolc installed (skips the pallet-revive fixtures — see above):
SKIP_PALLET_REVIVE_FIXTURES=1 cargo build --release -p next-people-paseo-runtime
```

This generates:

```
./target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm
```

## Running a local network

To run the People chain locally, you need the following components:

* A local relay chain of choice and its chain-spec, e.g. Paseo relay.
* The relay-chain node binary, [`polkadot`](https://crates.io/crates/polkadot), to run the relay validators. How to get it depends on your platform:
    * **Linux (x86_64):** download the prebuilt binaries from the [polkadot-sdk releases](https://github.com/paritytech/polkadot-sdk/releases) (grab `polkadot`, `polkadot-execute-worker`, and `polkadot-prepare-worker`), or build with `cargo install polkadot --locked`.
    * **macOS:** there is no official release binary — build from source with `cargo install polkadot --locked` (a long build).
    * **Windows:** the node is not supported natively — use [WSL2](https://learn.microsoft.com/windows/wsl/) and follow the Linux steps.

    However you obtain it, the node needs `polkadot`, `polkadot-execute-worker`, and `polkadot-prepare-worker` side-by-side (`cargo install` keeps them together). Make sure `polkadot` is on your `PATH`.
* A People chain chain-spec, which we can obtain from the above WASM file and the [`chain-spec-builder`](https://crates.io/crates/staging-chain-spec-builder) utility (`cargo install staging-chain-spec-builder --locked`).
* A node to run the parachain, e.g. [`polkadot-omni-node`](https://crates.io/crates/polkadot-omni-node) (`cargo install polkadot-omni-node --locked`).
* [`zombienet`](https://github.com/paritytech/zombienet) as a utility to spawn the network, and configure the relay chain and People chain (`npm i @zombienet/cli -g`).

There are two reasonable ways to proceed.

## Option 1: create your own local config

The high-level commands for a generic local setup are as follows.

Prepare the Paseo relay chain-spec. You can download it from the [`paseo-network/paseo-chain-specs`](https://github.com/paseo-network/paseo-chain-specs) repository:

```bash
curl -L --fail -o paseo.raw.json \
  "https://paseo-r2.zondax.ch/chain-specs/paseo.raw.json"
```

Prepare a chain-spec for the People chain, based on its WASM runtime. The People chain uses para id `1502`:

```bash
chain-spec-builder create \
  --relay-chain paseo \
  --para-id 1502 \
  --runtime ./target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm \
  named-preset local_testnet
```

This writes a `chain_spec.json` in the current directory. (Available presets are `development`, `local_testnet`, and `live`.)

Create and modify a `zombienet` config file, for example `network.toml`:

```toml
[relaychain]
chain_spec_path = "./paseo.raw.json"
default_command = "polkadot"

[[relaychain.nodes]]
name = "alice"
validator = true

[[relaychain.nodes]]
name = "bob"
validator = true

[[parachains]]
id = 1502
chain_spec_path = "./chain_spec.json"

[[parachains.collators]]
name = "people-collator"
command = "polkadot-omni-node"
```

Use `zombienet` to spawn the network:

```bash
zombienet spawn network.toml --provider native
```

## Option 2: use the maintained example config

This repository also ships a maintained local-network config at `docs/examples/people-network.toml`, used by the runnable examples under `docs/examples/`.

That config expects:

* to be run from `docs/examples/`,
* a relay-chain bundle available on `PATH` as `polkadot`,
* a generated People chain-spec at `docs/examples/generated/people-chain-spec.json`, and
* `polkadot-omni-node` available on `PATH` for the People collator.

Install the relay-chain bundle once (or provide your own `polkadot` bundle on `PATH`):

```bash
mkdir -p docs/examples/generated/relaychain-bin
cd docs/examples/generated/relaychain-bin
zombienet setup polkadot -y
cd ../..
export PATH="$(pwd)/generated/relaychain-bin:$PATH"
```

Prepare the example chain-spec:

```bash
cd docs/examples
polkadot-omni-node chain-spec-builder -c generated/people-chain-spec.json create \
  --relay-chain rococo \
  --para-id 1502 \
  --runtime ../../target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm \
  named-preset local_testnet
```

This writes `docs/examples/generated/people-chain-spec.json`.

Spawn the network from the maintained TOML:

```bash
zombienet spawn people-network.toml --provider native
```

If you prefer the scripted path, `docs/examples/package.json` wraps the same flow as `pnpm run node:people`.

The current example uses a `rococo-local` relay chain alias in `people-network.toml`, while still running the `next-people-paseo-runtime` parachain runtime from this repository.

See [this](https://docs.polkadot.com/parachains/testing/run-a-parachain-network/) and [this](https://paritytech.github.io/polkadot-sdk/master/polkadot_sdk_docs/guides/your_first_node/index.html) guide in the Polkadot documentation for more details.
