# Transaction validity

Extrinsics are identified in the transaction pool by their `provides` tags; when two share the same tags they conflict and the pool keeps only the higher-priority one. Fees bound paid transactions, but feeless ones (authorized calls, custom transaction extensions) are unbounded and can flood the transaction pool if external submission is allowed and their tags let one caller produce many distinct entries.

## Authorize extrinsics

**Tag design.** Verify that the `provides` tags cannot be varied to produce an unbounded number of distinct entries. For example, if an extrinsic accepts a limit parameter `N` in `0..1000` and `N` appears in the tag, a single sender can place 1000 transactions in the pool at once. Use only identity-bearing data (sender key, target account, fixed operation kind) in tags.

**`Stale` and `Future` are nearly equivalent for a not-yet-ready transaction.** Both ban it from the view for the ban duration (~30 min) without removing it from the pool, so an OCW resubmitting the same call won't get it included until the ban expires. To let it be retried sooner, the levers are a per-run discriminator in the call, a short `longevity`, or returning an invalidity other than `Stale`/`Future` (which removes it from the pool — OCW submissions bypass bans).

**A dispatch may fail only if replaying it is costly.** A failing call reverts its own state changes, but state changes made in `authorize` are kept.

- **Free to replay** — `authorize` only checks preconditions and no fee is charged. A transaction that passes `authorize` but fails `dispatch` can be resubmitted indefinitely at no cost: free blockspace spam. Dispatch must not fail for any transaction `authorize` accepts. The risk is highest for `TransactionSource::External`.
- **Costly to replay** — `authorize` persists a replay guard (consumes a nonce or one-time proof, marks the operation done) or a weight-based fee is charged. A dispatch failure is acceptable.

Verify either that dispatch cannot fail for an authorized transaction, or that `authorize` or the fee makes replaying a failed dispatch costly.

## Custom transaction extensions

A transaction extension must provide three properties:

1. **Spam protection** — The sender must pay for the blockspace they consume. The usual mechanism is weight-based fees. An alternative for one-time operations is to allow a single free transaction per identity, but there must always be a bound on how much blockspace a sender can consume without cost.

2. **Authentication** — The sender's intent must be unforgeable and unambiguous. The usual mechanism is a signature over inherent implications. If the extension carries fields beyond the signature that could alter the extrinsic's behaviour, those fields must be included in the signed payload; otherwise an attacker can modify them to change the sender's intent while reusing the signature.

3. **Replay protection** — A transaction represents a one-time intent; the sender must not be forced to accept it being executed again. The usual mechanism is signing and incrementing a nonce. When an operation advances some state irreversibly (e.g. consuming a one-time proof), the state transition itself can serve as the replay guard instead of a nonce.
