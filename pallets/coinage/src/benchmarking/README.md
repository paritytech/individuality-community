# Coinage benchmark proof cache

`proof_cache.rs` stores cached alias proofs used by coinage benchmarks.

A ring-VRF proof takes over a second to create in WASM, and the unload
benchmarks need up to `MaxConsolidation` of them per run. A warm cache avoids
repeating that proof generation during benchmark setup.

With a warm cache, the production-WASM smoke test at `--steps 2 --repeat 1
--min-duration 0` took about three minutes on a local macOS host. This is not
the duration of a full weight-generation run. The [successful CI command job
on September 14, 2026](https://github.com/paritytech/individuality-community/actions/runs/34807856617)
took about 7 hours 49 minutes, including setup and building, for
`next-people-paseo` / `indiv_pallet_coinage` at `--steps 50 --repeat 20`.
These are observed durations, not limits; the machine, build cache and proof
cache coverage affect subsequent runs.

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

The unload benchmarks sample the alias count at the fixed values 1, 2, 4, 8, 16, 32 and
the maximum (`unload_recycler_into_coin_1`, ..., `unload_recycler_into_coin_max`)
rather than sweeping a `Linear` component, and the pallet interpolates between
them (see `weight_interpolation.rs`). The proofs a run needs are therefore the
same at any `--steps` and `--repeat`, and one harvest is warm for every run.
The remaining `Linear` components (split outputs, ring cleaning, ...) do not
feed a proof.

The `_16` and `_32` benchmarks measure exactly 16 and 32 aliases. They return
`BenchmarkError::Skip` before setup if the count exceeds the call's configured
maximum. The mock maximum is 16, so its `_32` cases skip; Paseo supports both
counts and measures all seven samples. A skipped benchmark produces no weight
measurement and must not be treated as coverage for that sample.

The output-sweep benchmarks build a fixed member list large enough for
`max_aliases_per_unload()`, rather than filling every slot in the ring. They
seal the ring to retain the expiration check and next-ring state. Proof
verification still uses the configured ring exponent, and cleaning benchmarks
still fill the ring because their measured work depends on its members.
The output-fee extension signs a fixed maximum denomination and fee limit, so
regenerating weights does not change its cached proof's message.

## Mixed-output dispatch components

Only `unload_recycler_into_external_asset_and_loaded_coins` groups loaded outputs by
denomination. Its fourteen dispatch benchmarks use `g` distinct denominations and `e`
additional coins at an existing denomination, so the output count is `g + e`.
`validate_unload_calls(r, d)` retains its output-count component and uses one group.
Coin-output calls retain their existing setup and weights.

The mixed-output setup constructs one coin at each exponent from `MinimumExponent`
through `MinimumExponent + g - 1`, followed by `e` coins at `MinimumExponent`.
The input denomination is `MaximumExponent`; the remaining value becomes the external
asset output. This costs `2^g - 1 + e` base units. A base unit is the asset amount of
`MinimumExponent`, so the same arithmetic applies to negative minimum exponents.
`FromOutput` reserves `ceil(quoted_asset_fee / base_unit) + 1` units; `Prepaid` reserves
zero. The fee quote uses the benchmark asset and conversion pool.

For each alias bucket `a`, the helper chooses the greatest `G` satisfying
`2^G - 1 + (MaxSplitOutputs - G) + reserve <= a * 2^(MaximumExponent - MinimumExponent)`.
The independent component ranges are `g = 1..G` and `e = 0..MaxSplitOutputs-G`.
Metadata obtains the fee quote inside a rolled-back storage transaction, without
initializing proof chunks. Every point inside this rectangle must construct a valid
scenario; infeasible setup is an error, never a skipped measurement.

At Paseo's exponent range `0..14` and 32 outputs, `G` is 13, 14 and 15 for one, two
and at least four aliases when the reserve is at most 8,174 units. The production
benchmark pool holds raw native and external assets in a 10,000:1 ratio; denomination
zero is 10,000 raw external units. Recheck the quote after regenerating lifecycle
weights because those weights determine the fee.

The rectangle does not cover the entire valid call domain. Calls with one or two
aliases can have one extra denomination with at most one repeat. `FromOutput` also
requires the remaining value to cover its actual fee, which can be less than the
benchmark reserve. Calls with
fewer groups can have up to 31 repeats. The linear formula extrapolates into both
regions. The setup tests verify their value and output construction.

Every target collection starts with a full onboarding tail. The first output in each
group reads that full page and writes a new page. A group of at most `MaxSplitOutputs`
keys touches at most two pages, and this state reaches both pages for every group
independent of repeats. `OnboardingQueue` and `QueuePageIndices` accesses therefore
scale with `g`.

The source ring is sealed before filler members are inserted. Source members still
use seed 60,000 and the proven message is still `[0; 32]`, so filling target tails
does not change the proof cache key. Filler members decode the hash of their index
and group into the member encoding. They are not validated or onboarded. The measured
call decodes and stores these bytes and validates its actual output keys normally.

Regenerate both weight files through `/cmd bench` for `next-people-paseo` and
`indiv_pallet_coinage`. Check that all fourteen mixed-output benchmarks run, queue
accesses scale with `g` and per-coin accesses scale with `g + e`. Replace the temporary
output-count formulas with the generated `(g, e)` formulas before merging.

## Regeneration feature flags

Two feature flags toggle the regeneration mode. The harvest commands below
already enable them; you only need to know what they do.

- pallet: `benchmark-proof-cache-regenerate`
- runtime shim: `coinage-benchmark-proof-cache-regenerate`

With either flag enabled, `generate_alias_proof(...)` emits each proof it uses,
including matching cached proofs. Only missing entries need proof generation:

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
result into `CACHE_ENTRIES_R2E10` in `proof_cache.rs`. Existing matching proofs
are reused, but missing proofs are generated and can make a harvest slow.

Flags:

- `--no-build` — skip the cargo build and reuse existing WASM.
- `--no-write` — run the full harvest and print the entry count without
  modifying `proof_cache.rs`. Useful for dry runs.
- `--profile <profile>` — cargo profile for the runtime build, `production` by
  default. The cached proofs are the same either way. `dev` uses the debug WASM
  artefact and gives the shortest build, while `release` uses the compact
  compressed WASM artefact and gives the fastest harvest.

After the script finishes, still run the step 5 verification below.

The `/cmd bench` command generates weights, not the proof cache. Regenerate
and commit the cache before requesting new weights after proof-input changes.
That command uses `RUNTIME_LOG=off`, which hides cache-miss warnings.

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
