# Extrinsics

- [Pays::Yes vs Pays::No](pays-yes-vs-pays-no.md) — fee refunds, weight refunds on error, and how to handle error paths
- Avoid `ValidateUnsigned`: use authorized calls (`#[pallet::authorize]`) for unsigned validation, or a `TransactionExtension` for custom validation logic
- For `authorize` error reporting, prefer a `CustomInvalidity` enum (`#[repr(u8)]`) to give callers accurate diagnostics. The exception is `Stale` and `Future`: these have distinct tx-pool semantics, keeping the transaction in the pool to be revalidated later, whereas `Custom` (and the other generic variants) drop it. Use `Stale`/`Future` when the transaction may become valid in a later view and should be retained, and a `CustomInvalidity` variant otherwise.
- `authorize` returns *unspent* weight, subtracted from `weight_of_authorize`, not the weight consumed. Return `Weight::zero()` unless a success exited early having done less than the declared weight covers: returning `WeightInfo::authorize_foo(..)` refunds the whole check.
- Consider that malicious node operators can craft arbitrary OCW transactions or spoof `TransactionSource::Local`/`InBlock`
  - Use `TransactionSource` only as a tx-pool filter, not a security gate
  - Validate on-chain in `authorize`; `dispatch` runs immediately after on the same state and need not repeat those checks
  - Enforce every precondition in `authorize`, not the offchain worker. OCW pre-filtering can be bypassed or spoofed.
- Offchain workers must submit general transactions, not bare/inherent ones. Use `CreateAuthorizedTransaction` instead of the now-unneeded `CreateInherent` to bound the config trait.
- Avoid real work in `on_poll`/`on_idle`; they run unconditionally as part of the STF every block. Defer deferrable work to OCW-submitted authorized calls with low per-call limits.
- Add `fn integrity_test() { ... }` for configuration invariants (e.g. interval must not be zero, or that a `Linear`-bounded extrinsic's worst-case weight fits the block budget
- Document that a `Local`/`InBlock`-only authorized call cannot be submitted externally, once per pallet, e.g. in the module doc
- FRAME storage: for the length or emptiness of a `Vec`/`BoundedVec` entry, use `decode_len()` not `get().len()` (and `decode_len().is_none_or(|n| n == 0)` not `get().is_empty()`). When the value comes from persistent state (prior blocks), both pull the whole value into the PoV, so `decode_len()` is only a CPU/allocation win. It returns `None` if never written, so use `.unwrap_or(0)` when that means "empty". Fall back to `get()` only when you need the values themselves.
