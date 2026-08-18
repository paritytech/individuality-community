## `Pays::Yes` vs `Pays::No`

| Outcome | `Pays` | How |
|---|---|---|
| Success, did useful chain work | `Pays::No` | `Ok(Pays::No.into())` |
| Success, self-interested action | `Pays::Yes` | `Ok(().into())` |
| Any error | `Pays::Yes` | bare `?` (runtime default) |

Do **not** set `Pays::No` on error paths. Errors default to `Pays::Yes` because a failed transaction produces no state change, so it can be replayed to consume block weight without paying any fee.

## Weight refund on error

When an extrinsic refunds weight on success via `actual_weight`, errors must carry the same weight. A bare `?` leaves `actual_weight: None`, charging the full pre-dispatch weight. Compute `actual_weight` early, wrap the body in a `DispatchResult` closure, and map errors:

```rust
let actual_weight = …;
let result: DispatchResult = (|| { /* bare ? */ Ok(()) })();
result.map_err(|e| DispatchErrorWithPostInfo {
    post_info: PostDispatchInfo { actual_weight: Some(actual_weight), pays_fee: Pays::Yes },
    error: e,
})?;
Ok(Some(actual_weight).into())
```

## Testing

Every extrinsic that returns `Pays::No` on success must have a companion test asserting `Pays::Yes` on the error path. This guards against someone actively setting `Pays::No` on errors.

```rust
let result = MyPallet::my_extrinsic(origin, bad_input);
assert!(result.is_err());
assert_eq!(result.unwrap_err().post_info.pays_fee, Pays::Yes);
```

Use `assert_err_ignore_postinfo!` (not `assert_err!`) when testing the error variant of extrinsics that set `actual_weight` on error, since `assert_err!` compares the full `DispatchErrorWithPostInfo` including post info.
