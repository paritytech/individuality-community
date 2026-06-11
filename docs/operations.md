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
- `set_underlying_asset_id` — set the underlying asset (one-time, permanent)

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
1. Create the asset (`Assets::force_create`) and mint the supply.
2. `set_underlying_asset_id` to bind it. This is permanent.

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
