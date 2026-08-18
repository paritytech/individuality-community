# Architecture

Individuality is a Cargo workspace of [Polkadot SDK](https://github.com/paritytech/polkadot-sdk) (FRAME) crates — pallets that implement Proof-of-Personhood, and reference runtimes that combine them into Substrate-flavored chains.

## Workspace layout

| Directory | Contents |
|---|---|
| `pallets/` | The custom FRAME pallets that implement personhood, games, rewards, and asset integration. |
| `runtimes/` | The parachain runtimes that wire the pallets together (see below). |
| `support/` | `indiv-support` — shared traits, types, and genesis helpers used across the SDK. |
| `precompiles/` | `personhood` — a `pallet-revive` precompile exposing personhood logic to contracts; `scarcity` — precompiles exposing `pallet-scarcity` collections as ERC-721 contracts; `nft-claims` — the claim-minter precompile. |
| `integration-tests/` | End-to-end tests spanning multiple pallets. |

### Precompile address indices

Address indices allocated in `next-asset-hub-paseo`; check new precompiles against this table:

| Index | Kind | Precompile |
|---|---|---|
| `0x120` | prefix | `ERC20`, trust-backed assets (upstream) |
| `0x220` | prefix | reserved for `ERC20`, foreign assets (upstream) |
| `0x320` | prefix | `ERC20`, pool assets (upstream) |
| `0x0A01` | fixed | `personhood` |
| `0x0520` | prefix | `scarcity` collections |
| `0x0521` | fixed | `scarcity` factory |
| `0x0522` | fixed | `nft-claims` minter |

`XcmPrecompile` is also wired, at its upstream-fixed address.

## Runtimes

The runtimes in `runtimes/` are **reference implementations** — prototypes that show how the Individuality pallets integrate into a Substrate-flavored (FRAME) chain. Treat them as working examples to learn from and adapt into your own runtime, not as turnkey production chains.

| Runtime | Role |
|---|---|
| `next-people-paseo` | The People chain — where personhood lives (people, games, attestations, coinage, airdrops). Shows how the Individuality pallets combine into a complete People chain runtime. |
| `next-asset-hub-paseo` | A companion Asset Hub. Shows how a non-People chain wires in the integration pallets to interoperate with the People chain. |

Together they prototype both sides of an integration: the chain that hosts personhood, and another chain that consumes it.

## Pallets

Core pallets on the People chain include:

- `people-lite`, `people-multi` — personhood registration and attestations.
- `game`, `score` — the personhood game and its scoring. The game also ships the Merkle roots of its NFT claim credits to `nft-claims` on Asset Hub.
- `coinage` — coinage asset mechanics.
- `airdrop` — airdrop events.
- `members` — ring VRF manager.
- `members-notifier` — publishes ring roots to subscriber chains over XCM (paired with `members-subscriber` on the consuming chain).

On the companion Asset Hub:

- `members-subscriber` — receives and stores ring roots from the People chain.
- `nft-claims` — holds the game's NFT claim credit Merkle roots, received from the game pallet over XCM, and is where a claimant mints their NFT against one.
- `dotns-gateway` — gateway to dotNS smart contracts to include username/domain registration in personhood flow.

The workspace contains further pallets — `alias-accounts`, `chunks-manager`, `honour`, `mob-rule`,
`pgas`, `resources`, and others.

For per-pallet details, build the API docs (rustdoc) locally:

```bash
cargo doc --no-deps -p 'indiv-pallet-*' --open
```

This generates rustdoc for every pallet under `target/doc/` (git-ignored).
