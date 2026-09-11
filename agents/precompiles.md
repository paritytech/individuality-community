# Precompiles

- Use `indiv-precompile-support` for the shared guards. Do not re-implement them per crate.
- Order the guards at the top of `call`, before any charge or state read: `ensure_not_delegate`, then `ensure_no_value`, then the read-only check.
- `ensure_not_delegate` is unconditional. Under a delegate call the precompile runs with the delegator's address and storage, and `env.caller()` is the delegator's own caller (msg.sender is preserved), so identity and any address-derived id are attacker-controlled. Refuse even when no current selector derives identity from them, so a later one cannot reintroduce the risk.
- `ensure_no_value` is unconditional for a non-payable precompile. The address has no owner, no code and no withdrawal path, so attached value is unrecoverable.
- Both guards revert rather than trap, so the caller keeps its forwarded gas and can catch the failure.
- Gate the read-only check on the selector, not on the whole precompile: `if env.is_read_only() && is_mutating(input)`, as in `precompiles/scarcity/src/collection.rs:51` and `precompiles/nft-claims/src/lib.rs:134`. A view selector must stay callable from a read-only frame. A blanket `env.is_read_only()` denial is correct only when every selector mutates, as in `precompiles/scarcity/src/factory.rs:45`.
- Resolve the caller with `caller_account`, not by hand. It reverts with `ERR_INVALID_CALLER` when the EVM address has no mapped account id.
- Charge before the work, never after. Use `charge_reads(env, n)` for plain storage reads and `env.charge(WeightInfo::..)` for a dispatch. When the real cost is known only after the call, charge the worst case then settle with `env.adjust_gas(charged, actual_weight)`.
- Report caller-correctable failures as string reverts through `revert`. `pallet-revive` offers no path for a typed Solidity custom error, so a typed `error` in the ABI is not available.
- Document the frame guards in the crate's module doc under a `# Frame guards` heading, including why a guard is absent. See `precompiles/personhood/src/lib.rs`.
- Cover each guard with a negative test: delegate call, attached value and, for a mutating selector, a read-only frame.
