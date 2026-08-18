# Using Individuality components

How an end user interacts with components (pallets, calls, etc.) of Individuality project.

These are the **open** calls — anyone with a signed account can make them, no
privileged origin required. They are the counterpart to the privileged calls in
[operations.md](./operations.md): operators set things up (schedule a game, run
an airdrop, grant invites), and users take part with the calls below.

This is a non-exhaustive overview. For full signatures and parameters, read the
pallet docs (see [Reference](#reference)).

## Play a game

Pallet: `pallets/game`.

- `sign_up_with_invite` / `sign_up_with_account` / `sign_up_with_account_lite_invite` - sign up
  for the next game as an account-based player for the first time or after being archived by using
  an invite / paying a deposit / using a lite person alias.
- `sign_up_with_account` - sign up for the next game as an active account-based player (for a first
  time signup and for an archived player the deposit will be charged, and the user may prefer using
  `sign_up_with_invite` and `sign_up_with_account_lite_invite` to avoid paying a deposit).
- `sign_up_with_alias` - sign up for the next game using full personhood provided from another DIM.
- `report` — report your result during the reporting phase.
- `claim_airdrop` — claim a prize from a game-linked airdrop.
- `offboard` — leave and reclaim any deposit once you are done.

## Verify someone (attest)

Pallet: `pallets/people-lite`.

- `attest` — vouch for another account's personhood. You need an attestation
  allowance first, which an operator grants via `increase_attestation_allowance`
  (see [operations.md](./operations.md#what-you-can-do)).

## Manage your alias account

Pallet: `pallets/people-lite`.

- `set_alias_account` — bind an account to your personhood alias.
- `unset_alias_account` — remove that binding.

## Distribute invites

Pallet: `pallets/game`.

Once an operator grants you invites (`grant_invites`), you hand them out:

- `set_invite_ticket` — issue an invite ticket to someone.
- `cancel_invite_ticket` — revoke a ticket you issued.

## Coins

Pallet: `pallets/coinage`.

Coinage is user-driven once an operator has created an instance for an asset.
The common actions:

- `split` — split a coin into smaller coins.
- `transfer` — transfer a coin to another account.
- `load_recycler_*` — move value into the coinage system.
- `unload_recycler_*` — take value back out.

The pallet exposes several `load_*` / `unload_*` variants for different payment
and anonymity options — see the pallet docs for the full set.

## Register a dotNS username

Pallet: `pallets/dotns-gateway` (on the Asset Hub).

- `reserve_name` — an attester with allowance reserves a lite-person label
  (e.g. `alice.42`) for the user; the user signs the reservation to consent.
- `register_name` — registers a full-person label (e.g. `alice`), authenticated
  by a ring membership proof via the pallet's transaction extension.

## Reference

For the full set of calls and parameters, build the API docs (rustdoc) locally:

```bash
cargo doc --no-deps -p 'indiv-pallet-*' --open
```

This generates docs for all pallets under `target/doc/` and opens them in your
browser — e.g. `indiv_pallet_game`, `indiv_pallet_people_lite`,
`indiv_pallet_coinage`.
