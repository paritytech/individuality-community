# Coinage benchmark proof cache

`proof_cache.rs` stores cached alias proofs used by coinage benchmarks.

A ring-VRF proof takes over a second to create in WASM, and the unload
benchmarks need up to `MaxConsolidation` of them per run. Without a warm
cache a `frame-omni-bencher` run of the coinage pallet takes hours instead of
minutes.

A miss is logged at warn level as `alias proof cache miss`, so a run under
`RUNTIME_LOG=warn` shows directly whether the cache still matches. A run that
is slower than expected is the same symptom.

## What is cached

Each cache entry stores:

- the hash of `(member, all_members, msg)`
- the encoded proof bytes
- the alias

The lookup in [proof_cache.rs](./proof_cache.rs) uses binary search, so entries
must stay sorted by the first hash key.

## Current ring exponent

A cache key hashes the ring's whole member list, so entries only ever match one
`RecyclerRingExponent`. `next-people-paseo-runtime`, the only runtime with
coinage, uses `R2e10`, and `CACHE_ENTRIES_R2E10` is the single table. A runtime
on another exponent misses every lookup and logs a warning.

## When to regenerate

Regenerate whenever a benchmark setup changes what a proof commits to: the ring
member set, the proven message, or the accounts and values feeding into it.
Those all feed the cache key, so a stale entry does not go wrong, it simply
never matches, and the run pays full ring-VRF proof generation instead.

The unload benchmarks sample the alias count at the fixed values 1, 2, 4, 8 and
the maximum (`unload_recycler_into_coin_1`, ..., `unload_recycler_into_coin_max`)
rather than sweeping a `Linear` component, and the pallet interpolates between
them (see `weight_interpolation.rs`). The proofs a run needs are therefore the
same at any `--steps` and `--repeat`, and one harvest is warm for every run.
The remaining `Linear` components (split outputs, ring cleaning, ...) do not
feed a proof.

## Regeneration feature flags

Two feature flags toggle the regeneration mode. The harvest commands below
already enable them; you only need to know what they do.

- pallet: `benchmark-proof-cache-regenerate`
- runtime shim: `coinage-benchmark-proof-cache-regenerate`

With either flag enabled, `generate_alias_proof(...)` skips the cache lookup
and emits each proof as:

```rust
CACHE_ENTRY: (hex!("..."), &hex!("..."), hex!("...")),
```

Drop the flags after you've updated `proof_cache.rs` so the normal cache
lookup path is active again.

## Regenerating the cache

There are two paths: a scripted one that wraps steps 2–4 below, and the
underlying manual commands. Step 1 (smoke test) and step 5 (verification)
must still be run by hand in either case.

### Scripted regeneration

```bash
python3 pallets/coinage/src/benchmarking/scripts/regen_proof_cache.py
```

The script builds the R2e10 runtime with the regeneration feature (using the
stable toolchain pinned in `rust-toolchain.toml`), runs `frame-omni-bencher`,
deduplicates and sorts the captured `CACHE_ENTRY:` lines, and splices the
result into `CACHE_ENTRIES_R2E10` in `proof_cache.rs`. A harvest creates every
proof cold and takes a while.

Flags:

- `--no-build` — skip the cargo build and reuse existing WASM.
- `--no-write` — run the full harvest and print the entry count without
  modifying `proof_cache.rs`. Useful for dry runs.
- `--profile <profile>` — cargo profile for the runtime build, `production` by
  default. The cached proofs are the same either way, so `release` trades a
  slower harvest for a much shorter build.

After the script finishes, still run the step 5 verification below.

### Manual regeneration

If you want to run the underlying commands yourself, the rest of this
document walks through them step by step.

The commands below use plain `cargo`, which picks up the stable toolchain
pinned in `rust-toolchain.toml`. Run them from the repo root.

### 1. Smoke test first

Before a full run, verify the logging path with a small run.

Pallet:

```bash
cargo test -p indiv-pallet-coinage \
  --features runtime-benchmarks,benchmark-proof-cache-regenerate \
  bench_unload_recycler_into_external_asset_prepaid_1 -- --nocapture
```

Runtime:

```bash
cargo build --profile production -p next-people-paseo-runtime \
  --features runtime-benchmarks,coinage-benchmark-proof-cache-regenerate \
  --locked

RUNTIME_LOG=error frame-omni-bencher v1 benchmark pallet \
  --runtime ./target/production/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm \
  --pallet indiv_pallet_coinage \
  --extrinsic unload_recycler_into_external_asset_prepaid_1 \
  --steps 2 \
  --repeat 1 \
  --min-duration 0 \
  --genesis-builder runtime \
  --quiet 2>&1 | tee /tmp/coinage-small-runtime.log
```

`RUNTIME_LOG=error` is used to include the `CACHE_ENTRY:` lines. `RUNTIME_LOG=off` hides them.

### 2. Harvest full runtime entries

```bash
cargo build --profile production -p next-people-paseo-runtime \
  --features runtime-benchmarks,coinage-benchmark-proof-cache-regenerate \
  --locked

RUNTIME_LOG=error frame-omni-bencher v1 benchmark pallet \
  --runtime ./target/production/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm \
  --pallet indiv_pallet_coinage \
  --extrinsic '*' \
  --steps 2 \
  --repeat 1 \
  --min-duration 0 \
  --genesis-builder runtime \
  --quiet 2>&1 \
  | tee /tmp/coinage-paseo-proof-cache.log
```

### 3. Extract, sort, dedup

```bash
rg 'CACHE_ENTRY:' /tmp/coinage-paseo-proof-cache.log \
  | sed -E 's/.*CACHE_ENTRY: //; s/[[:space:]]+$//' \
  | LC_ALL=C sort -u \
  > /tmp/coinage-r2e10-cache-entries.txt
```

`s/[[:space:]]+$//` strips trailing whitespace — `frame-omni-bencher`'s
log lines end with 4 padding spaces, and without this the spliced entries
carry them.

`LC_ALL=C sort -u` sorts by the first `hex!("...")` key and removes exact duplicates
with deterministic byte-order.

### 4. Update `proof_cache.rs`

Paste the entries from `/tmp/coinage-r2e10-cache-entries.txt` into
`CACHE_ENTRIES_R2E10`. The block must stay sorted and deduplicated.

### 5. Verify

```bash
cargo test -p indiv-pallet-coinage --features runtime-benchmarks benchmarking::benches
```

Then rerun the runtime benchmark under `RUNTIME_LOG=warn` and check that no
`alias proof cache miss` line appears.
