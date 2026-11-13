# Tracking NFT minting

This guide describes how an NFT claim credit becomes an NFT, and how a wallet or product follows that progress. The process spans two chains: the People chain (`next-people-paseo`) awards the credits and Asset Hub (`next-asset-hub-paseo`) mints the NFTs.

## Stages

| Stage | Chain | What happens | Trigger |
|---|---|---|---|
| 1. Earned | People | The `game` pallet awards a credit and `nft-credits` adds it to the buffer of a tree block. | A `report` by another player, or the attendance processing after the reporting phase. |
| 2. Committed | People | The block after the tree block builds the Merkle tree over the buffer and records its root. | `on_initialize` |
| 3. Sent | People | The offchain worker sends the tree to Asset Hub over XCM. | Offchain worker, with retries |
| 4. Claimable | Asset Hub | `nft-claims` stores the tree. The claimant can now mint. | `receive_credit_trees` |
| 5. Claimed | Asset Hub | The claimant submits `claim` with an inclusion proof. The claim spends the credit and mints the NFT in the same extrinsic. | A signed `claim` by the claimant |
| 6. Removed | Both | Asset Hub removes a tree when all its credits are claimed or when it expires. The People chain then removes the root. | `claim`, expiry sweep, `receive_tree_deletions` |

## Identifying a credit

A credit is identified by `(block, leaf_index)`: the tree block whose tree commits to it and its position in that tree. A `claim` specifies the same pair.

The tree block can be later than the award block. A tree holds at most 2048 credits of one game, so a credit goes to a later block when the current buffer is full or holds another game. Key a credit by the `block` in `NftClaimCreditAwarded`, not by the block that emitted the event.

The claimant is an `AccountOrPerson`: `Account(account)` with `ClaimantKind::Account`, or `Person(alias)` with `ClaimantKind::Person`. For a person, Asset Hub resolves the alias from the signer's `AliasAccounts::AccountToAlias` binding, which must match the alias of the award. Query the runtime APIs with the same claimant.

## Award phases

A game awards credits in two phases:

1. During reporting, each `report` awards a credit to every co-member that the reporter votes `Person` for.
2. After reporting, the game processes each player's attendance. An attendee then gets the rest of their credits: one credit for every co-member in every round they played, whether or not that co-member reported. A player who did not attend keeps only the credits from reports.

Attendance processing can take many blocks. Read `Game::Game` on the People chain to tell whether credits of the current game can still arrive:

| `Game::Game` | Credits of the game |
|---|---|
| `Registration`, `Shuffle` or `Reporting` | Reports can award more credits. |
| `PlayerProcess { step: Step1ProcessPlayers }` | Attendance processing can award more credits. |
| `PlayerProcess { step: Step2ClearIndices }` | Final. |
| `Cancelling`, or no game with this index | Final. |

`Game::GameEnded { index }` marks the end of a game, and `Game::GameCancelled { game_index }` marks a cancellation. After either event, no credit of that game is awarded. The game's last trees follow the stages above. Each tree carries `game_index`, so a wallet groups the trees of a game by it.

An attendee's final count is the sum, over the rounds they played, of their group's size minus one.

## Expiry

A tree expires 90 days after its first award (`CreditTreeTtl` on Asset Hub, mirrored as `ClaimsChainTreeTtl` on the People chain). A claim stays possible after that time until the expiry sweep removes the tree.

## Tracking with runtime APIs

The runtime APIs give the full status without event history.

1. On the People chain, call `NftCreditsApi::nft_claim_credit_blocks(claimant)`. It returns every tree block that holds credits of the claimant, with its state:
   - `Pending`: The credits wait for their tree. This is stage 1.
   - `Committed(tree)`: The root is recorded. This is stage 2 or later. `tree.timestamp + ClaimsChainTreeTtl` is the deadline.
   - `Removed`: The root is removed. This is stage 6.

   The People chain keeps only the 256 newest tree blocks of each claimant (`MaxCreditBlocksPerClaimant`). Find older blocks from the `NftClaimCreditAwarded` events.
2. For each committed block, call `NftCreditsApi::nft_claim_credit_proofs(block, claimant)`. Each proof contains the `credit`, its `leaf_index` and the sibling hashes that `claim` needs. The People chain deletes a block's awards at the claim deadline (`AwardRetentionTtl`) but keeps the root. The call then fails with `AwardsPruned`, and the credit is expired. A claim stays possible until the expiry sweep, so in that window rebuild the proof from the block's `NftClaimCreditAwarded` events with `NftCreditsApi::nft_claim_credit_proof_from_awards`, or use proofs that you saved before.
3. On Asset Hub, call `NftClaimsApi::credit_statuses` with the `(block, leaf_index)` pairs, up to 256 in one call. Each result is one of:
   - `NotStored`: Asset Hub holds no tree for the block and no claim of the leaf.
   - `Claimable { expires_at }`: The tree is stored and the credit is not claimed. `expires_at` is in seconds since the UNIX epoch.
   - `Claimed`: The credit is claimed.
   - `LeafIndexOutOfBounds`: The tree has no leaf at `leaf_index`.

Combine the two chains to get the stage:

| People chain state | Asset Hub status | Stage |
|---|---|---|
| `Pending` | | Earned |
| `Committed` | `NotStored` | In transit. The tree is committed or sent, and on its way to Asset Hub. |
| `Committed` | `NotStored`, five minutes after `tree.timestamp` | Delayed. See [Delivery times](#delivery-times). |
| `Committed` | `NotStored` after the deadline, or `AwardsPruned` | Expired |
| `Committed` | `Claimable` | Claimable |
| `Committed` | `LeafIndexOutOfBounds` | Leaf mismatch. The tree that Asset Hub stores for the block has no leaf at `leaf_index`. |
| `Committed` or `Removed` | `Claimed` | Claimed |
| `Removed` | `NotStored` | Removed. The credit expired, or it was claimed and then swept. A saved `CreditClaimed` event tells the two apart. |

`NftClaimsApi::preview_mints` shows which item a claim of a credit mints into a collection.

To show the minted NFT, read `Scarcity::NftsByOwner(mint_to)`. Asset Hub does not store a claim's `mint_to`, so derive it from the credit, as `nft_minting.ts` does with `//nft//<block>//<leaf_index>`, or save it.

## Tracking with events

Use the events to update a display without polling.

| Stage | Chain | Event |
|---|---|---|
| Earned | People | `NftCredits::NftClaimCreditAwarded { claimant, credit, block, leaf_index }` |
| Committed | People | `NftCredits::NftClaimCreditRootRecorded { block, credit_root }` |
| Sent | People | `NftCredits::CreditTreesSent { trees }`. `trees` pairs each sent tree block with its delivery sequence. |
| Claimable | Asset Hub | `NftClaims::CreditTreesReceived { count, stored }`. `stored` lists the blocks whose trees Asset Hub stored. |
| Claimed | Asset Hub | `NftClaims::CreditClaimed { block, leaf, leaf_index, collection, item, owner, instance }`. `instance` is the minted NFT and `owner` is the purse key it was minted to. |
| Removed | Asset Hub | `NftClaims::TreeFullyClaimed { block }` when the last credit is claimed. `NftClaims::TreeDeletionsSent { blocks }` lists every removed tree, after a full claim or after expiry. |

Only `NftClaimCreditAwarded` contains the claimant. Filter it by claimant and keep each credit's `block` and `leaf_index`. Every later event contains the `block`, and `CreditClaimed` also contains the `leaf_index`. A wallet that missed the award event uses the runtime APIs instead.

## Delivery times

On the People Next and Asset Hub Next devnets, a tree reached Asset Hub 12 to 48 seconds after the People chain sent it. The People chain sends a tree one or two blocks after it records the root. A credit is therefore claimable within about a minute of its award.

Show a committed tree that is still `NotStored` on Asset Hub five minutes after `tree.timestamp` as delayed. `tree.timestamp` is the time of the first award in the tree.

The user is the slow step. Nothing mints without a signed `claim`. On the devnets, the first claim of a tree came from one minute to 18 days after the tree arrived. To shorten this wait, remind the user to claim or offer a "claim all" action.

A tree that never arrives stays delayed until an operator delivers it again. See [Recover missing credit trees](operations.md#recover-missing-credit-trees).

## Examples

[`docs/examples`](examples/README.md) has the `nft_minting.ts` and `nft_status.ts` scripts for a local People + Asset Hub network.
