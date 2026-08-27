# Private NFT claims

A claim path that breaks the link between the player who earned an NFT claim credit and the
account that mints the NFT. A game opts into private claims at scheduling time.

On the public path a claim proves a Merkle leaf built from the claimant, so the mint is tied to the
player. On the private path a claim instead proves ring VRF membership of the set of claimants that
registered for one game: the proof shows that some registrant made it, but not which.

Status: experimental.

## How a private game runs

1. **Schedule.** An operator sets `private_claims` on the `GameSchedule` entry, with the number of
   `slots` a registrant gets. A game that opts in is private only.
2. **Register.** Each claimant calls `register_private_claim_key` on the People chain, spending
   `PrivateClaimEntryCredits` and handing over a one-time ring VRF public key their wallet made for
   this game. Every registrant gets the same slots, so a registration says only that they took
   part. The registration is public, and has to be: the ring is built from the registered keys, so
   the set a proof names is public too. A registered key discloses none of the aliases a proof of
   it yields. What the ring hides is which mint is whose.
3. **Build the ring.** Once registration closes, the offchain worker drives `build_private_ring`
   over the registered keys in bounded chunks.
4. **Deliver.** `send_private_ring` ships the game's outcome to the claims chain in one XCM
   message. The claims chain keeps the first outcome it receives for a game, and fixes the window
   its claims are taken in.
5. **Claim.** A claimant submits `claim_private` on the claims chain, one per slot, inside the
   game's claim window.
6. **Clean up.** `clean_up_private_game` removes the game's keys, registrations and unspent credits
   on the game chain in bounded steps. `close_private_ring` does the same for the ring and its
   spent aliases on the claims chain, once the window is closed.

## Calls

**NftCredits** (`pallets/nft-credits`, People chain)

- `register_private_claim_key(game_index, key)` — open call, made by the claimant. Costs
  `PrivateClaimEntryCredits`, once per game, and refuses a key already registered for it.
- `build_private_ring` / `send_private_ring` / `clean_up_private_game` — authorized calls submitted
  by the pallet's offchain worker. Local or in-block source only, so they cannot be submitted
  externally.

**NftClaims** (`pallets/nft-claims`, Asset Hub)

- `claim_private(game_index, slot, alias, proof, collection, mint_to)` — authorized call with no
  signer and no fee. `authorize` verifies the ring proof, checks it yields the `alias` the call
  names, that the alias is unspent and that the game's claim window is open; the dispatch spends
  the alias and mints.
- `receive_private_rings` — receives a game's outcome over XCM: a ring, or an abandonment.
- `close_private_ring(game_index)` — open call, made by anyone, once the game's claim window is
  closed. Removes up to 32 spent aliases per call and the ring with the last of them, recording
  the game in `ClosedPrivateGames`. It only reclaims space: a closed window takes no claim whether
  the ring is still there or not.

A claim carries no signed origin on purpose: the fee payer would be the strongest link a claim
leaks, since two claims from one account are two claims by one member. What bounds the call instead
is the proof, which nobody outside the ring can make, plus the alias as the pool's `provides` tag
and `MaxPrivateClaimsPerBlock` as a per-block cap. A claim past the cap is
`InvalidTransaction::Future` and stays in the pool.

## Slots and nullifiers

Each proof runs under a context derived from `(game_index, slot)`, so a claimant holds one distinct,
unlinkable alias per slot. `SpentPrivateClaims: (GameIdx, Alias) -> ()` records spent aliases, which
is the whole spend bound: a member has `slots` aliases in a game and each mints once. `slot` is a
public call argument; it names a context, not a person, and every ring member may name any of them.

Every slot proves against the same ring, so the set does not shrink as a claimant spends it and a
claimant's claims cannot be intersected into a smaller set.

The proof message binds `collection` and `mint_to`, so an observed proof cannot be replayed into
another purse or spent on another collection's item.

## The claim window

A ring's claims run in one window. It opens `PrivateClaimDelay` after the ring arrives and closes
`PrivateClaimWindow` later, both recorded on the ring when it is stored, and both named by
`PrivateRingReceived`. A claim before the window opens is `InvalidTransaction::Future` and waits in
the pool; a claim after it closes is a custom invalidity and is dropped. A redelivery of the same
ring keeps the window the first one set.

The delay opens every member's claims in the same block, so the wallets that watch the chain
closest are not the ones that claim first. The close is what bounds the interval a game's claims
fall in: without one they trail off indefinitely, and a late claim has only the members who had not
claimed yet as its anonymity set, however large the ring is.

The cost is forfeiture. A member who does not claim inside the window mints nothing, and the
credits their registration spent are gone: the ring is the only path a private game's credits mint
on, and abandonment is decided long before the window opens. The reference window is a month for
that reason.

A closed game is recorded in `ClosedPrivateGames`, and any later outcome for it is a
`PrivateOutcomeConflict`. The aliases that stopped a slot minting twice were dropped with the ring,
so a second ring over the same keys would mint every slot of the game again, and an abandonment
would reopen the public path for credits that already minted privately.

## Abandoned games

A game is abandoned when its registration stays below its anonymity floor, or when the build fails
`PRIVATE_RING_BUILD_RETRIES` (8) times in a row — a push fails on trusted-setup chunks the
ring cannot be built from, which no retry repairs. Without the limit the game would hold its keys,
registrations and credits forever with nothing able to claim them.

The floor is `MinPrivateRingKeys`, raised to `MinPrivateRingParticipation` of the claimants that
earned the entry price, and capped at `MaxPrivateRingKeys` so a full registration always reaches
it. A claimant is counted the moment their credits reach the price, which is the population that
can register at all; every award lands before registration opens, so the count is final by then.
`PrivateRingAbandoned` names both the keys registered and the floor they fell short of.

An absolute floor on its own is a fixed number of keys to buy. A group that registers with keys it
never claims with fills the anonymity set of one target, and sixteen keys in a game of hundreds
buys the whole set. The share ties that price to the size of the game.

The abandonment is delivered like a ring and recorded in `AbandonedPrivateGames`, which reopens
`claim` for the game's credit trees, so every player mints publicly as they would have without the
opt-in. Registrants lose only what the registration cost. No credit mints twice: a game is
abandoned only when no ring exists, so no `claim_private` of it can have been made, and the claims
chain refuses an abandonment for a game holding a ring, and a ring for one already abandoned
(`PrivateOutcomeConflict`).

## Configuration

| Item | Where | Reference value |
|---|---|---|
| `MaxPrivateClaimSlots` | `pallet-game` | 5 — upper bound on the slots a game may schedule |
| `PrivateClaimEntryCredits` | `pallet-nft-credits` | 5 — flat registration price, in credits |
| `MinPrivateRingKeys` | `pallet-nft-credits` | 16 — absolute floor, below which the game is abandoned |
| `MinPrivateRingParticipation` | `pallet-nft-credits` | 25% — share of the claimants that can pay the entry price which has to register |
| `MaxPrivateRingKeys` | `pallet-nft-credits` | 767 — registrants one game takes |
| `PrivateRingExponent` | both | `R2e10` — ring capacity, 767 keys |
| `PrivateKeysPerBuild` | `pallet-nft-credits` | 8 — keys pushed per offchain-worker call |
| `MaxPrivateClaimsPerBlock` | `pallet-nft-claims` | 8 — ring verifications per block |
| `PrivateClaimDelay` | `pallet-nft-claims` | 1 hour — from a ring arriving to its claims opening |
| `PrivateClaimWindow` | `pallet-nft-claims` | 30 days — how long a game's claims are taken |

`PrivateRingExponent` must match on both chains, and the trusted-setup chunks of that exponent must
be in `chunks-manager` before any ring builds. The initial-setup scripts upload `R2e9` and `R2e10`;
a larger exponent takes a chunk set of its own.

The entry price is in credits, not PGAS, because a person claimant has no account on the People
chain to burn PGAS from. It is flat rather than scaled with what a claimant earned, so that one
ring serves the whole game: a price ladder would put claimants in rings of their own tier, and
intersecting the tiers a payer proved against narrows them down. The cost is that a player who
earned 15 credits mints as many NFTs as one who earned 5.

Integrity tests assert that the entry price stays within what one game awards, that the claim
window is at least one block, that ring construction fits the offchain-worker budget and that a
private claim and a `close_private_ring` step each fit the block's extrinsic budget.

## Privacy limits

- **The set is the game's registration, capped at `MaxPrivateRingKeys`.** Registration is
  first-come, first-served, and a game with more registrants than the ring holds turns the surplus
  away, leaving those claimants no private path. Splitting a game over several rings would raise
  the cap, but a claim names the ring it proves against, so each ring is a smaller set to hide in;
  raising `PrivateRingExponent` is the way instead.
- **The set is keys, not people.** Registrants who collude, or who register a key they never claim
  with, count towards the anonymity floor without hiding anyone. Duplicate keys are refused, but an
  unused key is indistinguishable from a used one. Registration costs credits the claimant earned
  in that game, so padding the set takes real players, and `MinPrivateRingParticipation` makes the
  number of them scale with the game. Neither turns keys into people.
- **Timing is a side channel.** The window bounds it but does not close it: a claim in the block
  the window opens, or one made at a fixed interval after registering, narrows the set whatever the
  cryptography does. What a wallet does inside the window is what decides this.
- **`mint_to` reuse is refused, its aftermath is not.** A Scarcity purse key holds one NFT, so a
  second claim into the same key fails and a fresh key per claim is forced. Moving the NFTs out of
  those keys afterwards, or deriving the keys so that an observer can enumerate them, links the
  claims again. The ring's guarantee ends at the mint.
- **The submitting node sees the claim.** A `claim_private` transaction is gossiped from some node,
  which sees the proof next to whatever it knows about its submitter. That link is outside the
  runtime.

## What a wallet has to do

None of these are choices the runtime can make for a claimant: each one is a public call argument
or a submission time that only the claimant controls.

- **Pick a moment inside the window at random.** The window exists so that a game's claims cover
  each other, which they do only if they are spread over it. Claiming as soon as it opens, or a
  fixed interval after registering, gives the timing away.
- **Randomise the order the slots are spent in.** `slot` is public and every member may name any of
  them. A claimant who walks 0, 1, 2 at their own pace leaves a pattern that ties their own claims
  together, even though the aliases behind them are unlinkable by construction.
- **Mint each claim into a fresh purse key.** The chain refuses a key that holds an NFT already, so
  a new key per claim is forced, but the keys also have to be underivable from one another and left
  alone afterwards.
- **Vary nothing else that is public.** `collection` is a call argument: minting every slot into
  one niche collection links those claims as surely as a shared purse key would.
- **Claim before the window closes.** A missed window mints nothing and the registration price is
  not returned.
