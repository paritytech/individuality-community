# Testing

- Use mock helpers (e.g. `setup_person_with_alias`, `with_mock_context_disabled`) instead of manually constructing storage entries
- Use `parameter_types! { pub storage ... }` for values that tests need to override
- Prefer `assert_noop!` over manual `is_err()` + `unwrap_err()` for dispatch error assertions
- Place `use` imports at the module level, not inside closures or test function bodies
- `fn Executive::apply_extrinsic` returns a `Result<Result<(), DispatchError>, TransactionInvalidityError>` do not use `assert_ok(...)` as it ignores the inner result.
- Cover every new branch or call filter with tests, including negative cases (e.g. right target/wrong selector and wrong target/right selector)
- When an extrinsic refunds a worst-case charge (e.g. `a.max(b)`) down to the branch taken via `PostDispatchInfo`, test each cheap branch: assert `post.actual_weight` equals that branch's `WeightInfo` weight and is `all_lt` the pre-dispatch `Call::<Test>::the_call { .. }.get_dispatch_info().call_weight`. Give the branch weights distinct non-zero values in the mock's `WeightInfo`, otherwise the assertion is tautological.
