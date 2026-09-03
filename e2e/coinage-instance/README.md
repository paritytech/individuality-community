# Coinage instance example

This package initializes a Coinage instance backed by an example asset on a fresh local Relay,
Asset Hub and People network. It uses local sudo to execute calls that model Asset Hub governance
proposals.

```bash
cd e2e
pnpm install
pnpm --filter @individuality-e2e/coinage-instance init-coinage
```

## Initialization flow

The script performs these steps in order:

1. Force-open the HRMP channels between People and Asset Hub.
2. Upload the R2e9 and R2e10 chunk pages to People as authorized transactions.
3. Wait until People sees its egress channel to Asset Hub, then subscribe Asset Hub to ring root
   updates with an authorized `MembersNotifier.subscribe_whitelisted` call.
4. Execute a simulated Asset Hub technical maintenance proposal. Its batch creates the example
   asset and metadata on its reserve chain, then sends XCM to register its foreign representation
   and metadata on People.
5. Wait for People to process the XCM, then create and seed the native/example-asset conversion
   pool used for asset-denominated unload fees.
6. Provision the Coinage pallet account with the asset's minimum balance. The example asset is
   sufficient, so minting this balance also creates its account. A non-sufficient asset would
   require an `Assets.touch` call first.
7. Create the Coinage instance. By default Eve creates a sponsored instance and a second simulated
   proposal makes it sufficient through XCM. Set `COINAGE_PERMISSIONLESS` to `false` in `src/main.ts`
   to create a sufficient instance directly through the proposal path.

This package does not submit on-chain referenda. The current runtimes require Root for forced asset
and Coinage admin calls, so Alice's sudo dispatch models successful enactment on the intended
`technical_maintenance` track.

The local asset issuer mints the pool's example-asset liquidity directly on People. A production
provider must instead use reserve-backed supply.

## Configuration

| Variable | Default |
| --- | --- |
| `RPC_RELAY` | `ws://localhost:10000` |
| `RPC_PEOPLE` | `ws://localhost:10010` |
| `RPC_ASSET_HUB` | `ws://localhost:10020` |

## Local assumptions

The script targets a fresh network and does not resume a partial initialization. Alice is the local
sudo account. Eve owns and issues the example asset locally and creates the sponsored Coinage
instance.
