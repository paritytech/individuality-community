# Examples

Runnable TypeScript examples for performing calls against a People chain and Asset Hub (when applies), using
[pnpm](https://pnpm.io), [tsx](https://tsx.is) and [PAPI](https://papi.how) (polkadot-api).

Each example is a small script that performs one task end to end. The calls are
typed from the chain's own metadata.

| Script | Side | What it does |
|---|---|---|
| `game_scheduling.ts` | operations | Schedule a game (`Game.schedule_games`). |
| `airdrop.ts` | operations | Enable an asset and schedule an airdrop event (`Airdrop.enable_asset` / `schedule_event`). |
| `subscriptions.ts` | operations | Subscribe a parachain to ring-root updates (`MembersNotifier.subscribe`). |
| `dotns.ts` | operations | Set the dotNS dispatcher and grant attestation allowance (`DotnsGateway.*`). Runs against the Asset Hub. |
| `coinage.ts` | usage | Build coin calls — split / transfer / load / unload (`Coinage.*`, encode-only). |
| `game_participation.ts` | usage | Sign up for a game and build a report (`Game.sign_up_with_account` / `report`). |

## Setup

1. **Install pnpm** — see <https://pnpm.io/installation>.

2. **Bootstrap** — installs dependencies and generates the typed descriptors
   from the committed chain metadata (offline, no node needed):
   ```bash
   cd docs/examples
   pnpm run setup
   ```

The generated `@polkadot-api/descriptors` package is git-ignored; the `.papi/`
config and `metadata/*.scale` are committed, so `pnpm run setup` reproduces
the types offline.

The `generated/` directory is for local generated artifacts: keep the relay
bundle under `generated/relaychain-bin/`, and expect `pnpm run spec:people` to
write `generated/people-chain-spec.json`.

To **refresh the types from a live chain** after a runtime change:
```bash
PEOPLE_RPC=wss://<people-rpc> pnpm run papi:add
ASSET_HUB_RPC=wss://<asset-hub-rpc> pnpm run papi:add:assethub
```

## Running an example

Install the local relay-chain binary bundle once. `zombienet setup polkadot`
downloads `polkadot`, `polkadot-execute-worker`, and
`polkadot-prepare-worker` together into the current directory:

```bash
mkdir -p generated/relaychain-bin
cd generated/relaychain-bin
zombienet setup polkadot -y
cd ../..
```

On macOS, if the build cannot find `libclang`, set `LIBCLANG_PATH` to your LLVM
library directory before running the install command.

Check the installed bundle before starting the network:

```bash
export PATH="$(pwd)/generated/relaychain-bin:$PATH"
which -a polkadot polkadot-execute-worker polkadot-prepare-worker
polkadot --version
```

If `polkadot` resolves to an older binary before the newly installed bundle,
or the three commands resolve from different directories, move the new bundle
earlier in `PATH`; otherwise the relay validators can start and then
immediately exit with `Worker binaries could not be found`. A common symptom is
repeated Zombienet metrics errors while the People collator stays at block 0.

Start the local People chain in one terminal:

```bash
pnpm run node:people
```

This builds `next-people-paseo-runtime`, creates a local chain spec, and starts
the local network from `people-network.toml`. The People collator command is
`polkadot-omni-node`; the relay validators use the `polkadot` bundle above.
The People RPC is `ws://127.0.0.1:10010`.

Wait until the People chain starts producing blocks before running live
examples. It may sit at block 0 until the relay reaches the next session.

Then run examples from another terminal:

```bash
# Typecheck the examples without connecting to a node.
pnpm run typecheck

# Run a People example.
pnpm run game_scheduling

# Override the People endpoint.
PEOPLE_RPC=wss://<people-rpc> pnpm run game_scheduling

# Run the Asset Hub example against a separately running Asset Hub endpoint.
ASSET_HUB_RPC=wss://<asset-hub-rpc> pnpm run dotns
```

The examples are live scripts: they connect to the configured RPC endpoints,
read chain state, and submit the configured extrinsics. The local People network
uses these ports by default:

| Node | RPC |
|---|---|
| People collator | `ws://127.0.0.1:10010` |
| relay validator Alice | `ws://127.0.0.1:10000` |
| relay validator Bob | `ws://127.0.0.1:9955` |
| relay validator Charlie | `ws://127.0.0.1:9956` |
| relay validator Dave | `ws://127.0.0.1:9957` |

The local relay chain uses `rococo-local` because the current `polkadot` binary
exposes that local relay chain alias; the People runtime and package remain the
Paseo People runtime. `pnpm run spec:people` writes the generated People chain
spec to `generated/people-chain-spec.json`, which is ignored by git.

`coinage.ts` is encode-only for the extrinsics by design: Coinage
split/transfer require a custom coin origin, and load/unload require real ring
membership proof material. It still connects to the People chain to read
`Coinage.UnderlyingAssetId`.

`game_participation.ts` only signs up if there is an active game in
`Registration`; it always keeps `report` encode-only because a real report must
match the player's shuffled group and reporting phase.

## Notes

- **Signing.** The examples sign with the well-known dev key `//Alice` (see
  `lib/client.ts`). Replace it with the real manager-origin / sudo key for
  operations calls. Never commit a real mnemonic — pass it via the environment.
- **pnpm + tsx is the documented path**, but the scripts are plain TypeScript;
  any TypeScript runner (Bun, `ts-node`, native Node ≥ 23.6) runs them too.
