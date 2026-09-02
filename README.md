<div align="center">

![Individuality](./docs/images/logo-symbol-wordmark_light.svg#gh-dark-mode-only)
![Individuality](./docs/images/logo-symbol-wordmark_dark.svg#gh-light-mode-only)

# Individuality SDK

![GitHub stars](https://img.shields.io/github/stars/paritytech/individuality-community)&nbsp;&nbsp;![GitHub forks](https://img.shields.io/github/forks/paritytech/individuality-community)

<!-- markdownlint-disable-next-line MD013 -->
![GitHub contributors](https://img.shields.io/github/contributors/paritytech/individuality-community)&nbsp;&nbsp;![GitHub commit activity](https://img.shields.io/github/commit-activity/m/paritytech/individuality-community)&nbsp;&nbsp;![GitHub last commit](https://img.shields.io/github/last-commit/paritytech/individuality-community)

*Pallets and a runtime to manage Proof-of-Personhood (PoP) and related on-chain logic.*

</div>

> [!WARNING]
> This code has not been fully audited and is experimental — it may contain bugs, vulnerabilities, or incomplete features. The runtimes under `runtimes/` are reference implementations, not turnkey production chains. Use at your own risk.

Individuality is a standalone project built with the [Polkadot SDK](https://github.com/paritytech/polkadot-sdk) (FRAME): an in-development upgrade of the People chain, plus integration pallets that let other chains (e.g. Asset Hub) interoperate with it. Runtime releases follow the process in [RELEASE.md](./RELEASE.md).

It is an infrastructure layer under active development (see [Security](#-security)), and is **not part of the official People chain**. We develop this layer only; anything built on top is owned by others and not associated in any form with Parity.

## ⚡ Getting started

Install a recent Rust toolchain — follow the [Polkadot SDK install guide](https://docs.polkadot.com/develop/parachains/install-polkadot-sdk).

Some crates depend on `pallet-revive`, which compiles Solidity fixtures at build time and needs `solc` and `resolc` on your `PATH`. For local dev you can skip them with `SKIP_PALLET_REVIVE_FIXTURES=1` — see the [Launch guide](./docs/launch.md) for the full install.

Build a runtime to WASM:

```bash
cargo build --release -p next-people-paseo-runtime
# -> ./target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm
```

To spin up a local network, see the [Launch guide](./docs/launch.md).

## 🚀 Releases

Tagged releases ship the runtime WASM as release assets, built reproducibly with [srtool](https://github.com/paritytech/srtool). Each release publishes the `rustc` version, srtool version, and the **Blake2-256 hash** of every runtime — to verify a download, reproduce the build with srtool and confirm the hash matches the release notes.

> Releases are not guaranteed to be present on every clone of this repository.

## 🛠️ Operating a chain

Running a deployed People chain? See the [Operations guide](./docs/operations.md) for the privileged tasks an operator performs — scheduling games, running airdrops, enabling coinage, and managing attestations and invites.

## 📚 Documentation

1. [Launch](./docs/launch.md) — build the runtime and run a chain.
2. [Operations](./docs/operations.md) — privileged calls for the operator.
3. [Usage](./docs/usage.md) — open calls for end users.

For the framework these crates are built on, see the [Polkadot SDK docs](https://docs.polkadot.com).

## 🔐 Security

The security policy and procedures can be found in Parity's [SECURITY.md](https://github.com/paritytech/.github/blob/main/SECURITY.md).

## 🤍 Contributing & Code of Conduct

This repository **does not accept external pull requests at this time** — it is a public mirror that receives releases in waves. Bug reports, questions, and suggestions are welcome as [issues](https://github.com/paritytech/individuality-community/issues). See the [contribution guidelines](./docs/contributor/CONTRIBUTING.md) for details.

Comments in the code cite open work as `paritytech/individuality#NNN`. That is Parity's internal tracker for this project, and those issues are not yet publicly readable. We will gradually port these issues out of the private repo and update the references.

In every interaction and contribution, this project adheres to the [Contributor Covenant Code of Conduct](./docs/contributor/CODE_OF_CONDUCT.md).

## 📝 License

Licensed under the Apache License, Version 2.0 ([Apache-2.0](./LICENSE)).
