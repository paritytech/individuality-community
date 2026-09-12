# Operating the Individuality system

How to operate the Individuality system once it's deployed and configured.

Every privileged call in the Individuality pallets is gated by a manager origin
that the pallet defines itself (`ManagerOrigin`, `InviteIssuer`,
`AttestationAllowanceManager`, and others). These origins can be the same
account or several different ones — it is up to the runtime.

How those origins are satisfied is a deployment choice: whoever deploys the
runtime decides how to back each one — sudo (`EnsureRoot`), a proxy, governance,
or a dedicated account per role. So who can make a given call below depends on
how the deployed chain wired its manager origins.

Initial chain setup — HRMP channels, operational accounts, assets, etc. — is
handled before this point by [a set of scripts](../scripts/initial-setup/README.md).
This guide covers what you do against a chain that is already initialized.

## What you can do

A non-exhaustive list of the privileged calls, grouped by pallet. For full
signatures, parameters, and behavior, read the pallet docs (see [Reference](#reference)).

**Game** (`pallets/game`)
- `schedule_games` — schedule one or more future games
- `set_play_deposit` — set the signup deposit for account-based players
- `kill_current_game` — emergency stop the running game
- `grant_invites` / `remove_available_and_pending_invites` — manage invites

**Airdrop** (`pallets/airdrop`)
- `enable_asset` / `disable_asset` — make an asset available for airdrops
- `schedule_event` / `remove_scheduled_event` — manage airdrop events

**Coinage** (`pallets/coinage`)
- `create_sufficient_instance` — wrap an underlying asset in a coinage instance. (Without requiring
  the funding of a pot to cover loads until unloads).
- `create_sponsored_instance` — wrap an underlying asset in a coinage instance whose loads requires
  a deposit held from a pot until unloads. Permissionless. The pot is also funded permissionlessly.
- `funt_pot` / `withdraw_pot_funds` — manage the pot that covers the load-side costs of a sponsored
  instance.

**PeopleLite** (`pallets/people-lite`)
- `increase_attestation_allowance` / `clear_attestation_allowance` — manage a
  verifier's attestation quota

**MembersNotifier** (`pallets/members-notifier`)
- `subscribe` — register a parachain to receive ring-root updates
- `unsubscribe` — remove a subscriber (also notifies the subscriber chain)

**DotnsGateway** (`pallets/dotns-gateway`, on the companion Asset Hub)
- `set_dispatcher_address` — sets the dotNS dispatcher contract address
- `increase_attestation_allowance` / `clear_attestation_allowance` — manages an
  attester's reservation quota

## Common sequences

These are sensible starting points, not the only valid flows.

### Run a game
1. `set_play_deposit` to configure the signup deposit (optional).
2. `schedule_games` with your schedule.
3. Players sign up and the game runs automatically.

## Run an airdrop
1. `enable_asset` for the prize asset (once per asset).
2. Fund the airdrop pot with the prize amount. The pot is a derived account —
   `Airdrop::airdrop_pot_id()` (`PalletId` turned into an account) — not a
   configurable address. Transfer the prize asset to it with a normal asset
   transfer; `schedule_event` assumes the pot already holds the prize.
3. `schedule_event`. The event lifecycle then runs automatically.

## Enable coinage

### A sufficient instance
The chain absorbs the load-side costs, so creating one takes the admin origin.

1. Create an asset (`Assets::force_create`) and mint the supply.
2. Prepare `Coinage::pallet_account()` for the underlying asset. For a non-sufficient asset, call
   `Assets::touch` for the pallet account first. Fund the account with the asset's minimum balance in
   either case.
3. `create_sufficient_instance` with the asset and the amount of a coin of denomination zero.

### A sponsored instance
The creator and the sponsors pay instead, permissionless.

1. Create an asset (`Assets::force_create`) and mint the supply.
2. The funded creator calls `create_sponsored_instance` with the asset, the coin unit and an
   optional initial pot funding.
3. Sponsors keep the instance's pot funded if needed with `fund_pot`; loads are refused while the
   pot cannot cover the deposit.

### Fees in the underlying asset
For either kind of instance, to let users pay unload fees in the underlying asset instead of the
native currency, create an `AssetConversion` pool for the native/asset pair and add liquidity to
it.

## Connect a subscriber chain
1. Open HRMP channels between the chains in both directions (part of initial
   chain setup).
2. `subscribe` with the subscriber's para ID, the collections to share, and the
   pallet index of `members-subscriber` on that chain.
3. The initial ring roots and all later updates flow automatically.

To disconnect: `terminate_subscription` on the subscriber chain (its own
governance), or `unsubscribe` here.

## Enable dotNS registrations (on the Asset Hub)
1. `set_dispatcher_address` with the deployed `RootGatewayDispatcher` contract
   address.
2. `increase_attestation_allowance` for each attester.
3. Users reserve and register names (see [usage.md](./usage.md)).

## Reference

For the full set of calls, parameters, and origins, build the API docs (rustdoc)
locally:

```bash
cargo doc --no-deps -p 'indiv-pallet-*' --open
```
