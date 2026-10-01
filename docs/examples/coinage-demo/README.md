# Coinage demo bots

A standalone TypeScript/PAPI example for PreviewNet. It creates real recycler
holdings, withdraws a coin with ring-VRF proofs, transfers it through **Alice →
Bob → Charlie → Dave → Ferdie → Alice** and loads it back into the recycler.
Each round buys a paid unload token, so no identity backend or personhood
registration is needed. It uses `verifiablejs` for cryptography.

[Verified live run: ten finalized transactions](evidence/README.md).

## Run

Use Node.js 22+ and pnpm. This package has its own lockfile and uses live metadata;
it does not need the parent examples' generated descriptors.

```sh
cd docs/examples/coinage-demo
pnpm install --frozen-lockfile
pnpm check
pnpm test

# Read configuration, balances and existing coins without submitting transactions.
pnpm demo

# Submit a finite smoke run and save its receipts and voucher inventory.
pnpm demo --run --output runs/demo

# Keep exchanging during a demo. There are five transfers per round.
pnpm demo --run --rounds 100 --interval-ms 3000 --output runs/live-demo
```

The default endpoint is `wss://previewnet.substrate.dev/people`. Only that endpoint
and loopback endpoints are accepted: the dev account keys are public. Alice pays
for initial holdings and unload tokens. Coin transfers use `AsCoinage.AsCoin`
and do not require funding Bob, Charlie, Dave or Ferdie with native tokens.
Do not run multiple copies using these same accounts concurrently.

Defaults: instance `0`, denomination exponent `8`, ten holdings, one round.
Override with `--instance`, `--denomination`, `--holders` and `--rounds`.
The script reads the instance's asset ID/unit and the supported exponent range.
Value in raw asset units is `asset_unit * 2^denomination`, matching the iOS and
Android denomination libraries. This is not a decimal currency amount.

For example, at the tested PreviewNet configuration, unit `10000` and exponent
`8` mean `2560000` raw asset units per holding. Ten holdings load `25600000`
raw units in total. `preflight.json` records the actual asset location and unit.
Native token fees are additional. No sudo calls or shared configuration changes
are made.

## What this tests

- Two atomic batches load ten distinct recycler keys by default.
- A paid token and recycler membership are read at finalized blocks. RingKeys
  are truncated to the committed member count; queued keys cannot enter a proof.
- Both proofs are verified locally against the fetched commitments before signing.
- A v5 general transaction unloads the recycler into Alice's coin. Proof payloads
  are encoded with live metadata, including the length of each bounded proof.
- Five `Coinage.transfer` calls circulate that coin. Each destination's instance,
  denomination and incremented age are checked at the inclusion block.
- Alice reloads the coin, and the script waits for its new finalized ring membership.
  Ten holdings remain in the recycler when the default run finishes.

The bot creates traffic and holdings for a cash demo. It does **not** exercise the
mobile app's chat delivery or wallet UI. The current community apps' Fastest mode
adds no privacy wait after confirmed recycler inclusion. Balanced and Most private
require 20% and 90% ring fill respectively, or at least 32 members and ten minutes
since confirmed inclusion. See the exact [iOS policy](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Recycling/Strategy/RecyclingStrategyType.swift)
and [Android policy](https://github.com/paritytech/polkadot-android-community/blob/d49c5e6db17bfca48bf4872014f676e9fdb884b3/feature/coinage/api/src/main/java/io/paritytech/polkadotapp/feature_coinage_api/domain/recycling/RecyclingStrategyType.kt).
These publicly known accounts are test identities, not private users.

## Reproduce payment stages on Devnet

**Network correction:** the September 29 community nightly configuration selects
Paseo Next V2 People, not public Paseo People. The script below and its measured
results target public Paseo People and do not reproduce the nightly app end to end.
See the [release and network investigation](evidence/README.md#nightly-network-correction-and-reproduction-limits).
Release selection differs by platform: iOS currently resolves to Paseo Next V2,
Android production to Summit. See the [release-specific evidence](evidence/README.md#release-builds-platform-specific-network-selection).

`reproduce.ts` measures a bounded sequence on public Paseo People, using the
Devnet endpoint and a pinned genesis hash. It defaults to a read-only preflight.
The [verified live run and timing results](evidence/README.md#devnet-payment-reproduction--30-september-2026)
include clickable transaction proofs; the five-minute delay was not reproduced.

```sh
pnpm reproduce --output runs/repro-check
pnpm reproduce --run --output runs/repro-live
# After inspecting any interrupted submission:
pnpm reproduce --run --resume --output runs/repro-live
```

Bob supplies 320000 raw backing-asset units (denomination 5, unit 10000), pays
native fees and buys one paid unload token. The script refuses a fresh run if any
of the six dev accounts already holds a coin. Do not run it alongside other bots
using these accounts.

1. Load one voucher and unload it into Alice, measuring confirmed ring inclusion.
2. Direct transfer: Alice sends the coin to Bob.
3. Exact handoff: Bob's coin key is handed to the simulated Charlie wallet locally;
   Charlie claims into his own address using Bob's key. No sender extrinsic is
   submitted for the handoff, matching [iOS ExactMatchStrategy.swift](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Transfer/Plan/Strategies/ExactMatchStrategy.swift)
   and [Android ExactMatchStrategy.kt](https://github.com/paritytech/polkadot-android-community/blob/d49c5e6db17bfca48bf4872014f676e9fdb884b3/feature/coinage/impl/src/main/java/io/paritytech/polkadotapp/feature_coinage_impl/domain/planner/strategies/ExactMatchStrategy.kt).
4. Prepared payment: split Charlie's coin into a payment coin at Dave and change
   at Eve; Ferdie claims the payment using Dave's key.
5. Explicitly recycle Ferdie's coin and wait for committed membership. Fastest
   would not voluntarily recycle this young coin; this step isolates recycler
   latency from the payment path.

`receipts.jsonl` contains elapsed times, transaction bytes, finalized results,
explorer URLs and coin-state assertions. `vouchers.json` contains private voucher
material and stays in the ignored run directory. The finished run leaves one
change coin at Eve and one voucher in the recycler. Resume skips finalized steps
and refuses unresolved submissions rather than sending them twice.

This is a protocol reproduction: key handoff is local and claims wait for finalized
inputs. It does not run the mobile apps, their message transport, concurrent
submission queues or their five-minute input-wait recovery logic. A successful run
cannot rule out a phone-side delay. It uses a paid unload token instead of the
apps' personhood allowance, so allowance acquisition is also outside its scope.

## Recycler activity bot on documented Devnet

`recycler-bot.ts` keeps recycler rings on public Paseo People (pinned by genesis,
instance 0) busy using `//Bob`. It loads new vouchers and, once its capital is in
use, withdraws its oldest vouchers back to Bob in the same `Utility.batch_all` as
the next load. Withdrawal uses `unload_recyclers_into_external_asset_non_anonymous`,
which pays the unload fee in native PAS. Every load adds a member to a ring, and
withdrawal does not remove it. [Verified run and current status](evidence/README.md#recycler-activity-bot--96-hour-run).

### Which rings the activity can affect

The community apps count `RingKeysStatus.included` of the voucher's own ring. A
Balanced voucher is ready at 20% ring fill (154 of 767 members), a Most private
voucher at 90% (691). Either is also ready 10 minutes after the phone first sees
it included, once that ring has at least 32 members. Fastest waits only for
confirmed inclusion. Unloads do not reduce the count. See iOS
[RecyclingStrategyType.swift#L16-L53](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Recycling/Strategy/RecyclingStrategyType.swift#L16-L53),
[ParametricRecyclingStrategy.swift#L56-L66](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Recycling/Strategy/ParametricRecyclingStrategy.swift#L56-L66),
[RecyclingParams.swift#L40-L50](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Recycling/Strategy/RecyclingParams.swift#L40-L50),
[VoucherLocationService.swift#L409](https://github.com/paritytech/polkadot-ios-community/blob/97f9b5849be097d805140d30e82d75da397a3a7f/Packages/Coinage/Sources/Services/VoucherLocation/VoucherLocationService.swift#L409)
and Android
[RecyclingStrategyType.kt#L59-L65](https://github.com/paritytech/polkadot-android-community/blob/d49c5e6db17bfca48bf4872014f676e9fdb884b3/feature/coinage/api/src/main/java/io/paritytech/polkadotapp/feature_coinage_api/domain/recycling/RecyclingStrategyType.kt#L59-L65),
[ParametricRecyclingStrategy.kt#L56-L66](https://github.com/paritytech/polkadot-android-community/blob/d49c5e6db17bfca48bf4872014f676e9fdb884b3/feature/coinage/impl/src/main/java/io/paritytech/polkadotapp/feature_coinage_impl/domain/recycling/ParametricRecyclingStrategy.kt#L56-L66).

So loads help only a ring below one of these thresholds, and only for a voucher in
that same ring. They cannot shorten Fastest, and they cannot shorten the 10-minute
wait once a ring has 32 members. The bot stops loading a denomination when its
current ring reaches `--ring-ceiling` (700). This leaves room for real users and
avoids opening a new, empty ring.

### Run

```sh
# Read-only preflight: balances, plan and current ring sizes.
pnpm recycler-bot --output runs/bot-check
# Create a run; --adopt imports vouchers from an earlier run after proving their state.
node --import tsx recycler-bot.ts --init --output runs/bot-live --adopt runs/old-run
# Supervise it with launchd, check it and stop it.
./recycler-service.sh start runs/bot-live
./recycler-service.sh status runs/bot-live
./recycler-service.sh stop runs/bot-live
# Change a stopped run's plan, then start it again.
node --import tsx recycler-bot.ts --init --reconfigure --output runs/bot-live --duration-seconds 3600
# Re-verify every finalized transaction through a second RPC provider.
node --import tsx recycler-evidence.ts --run runs/bot-live --output evidence/name.json
```

Defaults: 96 hours, one transaction per 90 seconds, denominations 0 and 1 in turn.
At most 1.00 backing-asset units are held in vouchers (`--max-held-raw 1000000`)
and Bob keeps at least 0.50 liquid (`--asset-floor-raw 500000`). A voucher is
withdrawn at the earliest 3600 seconds after its load (`--min-hold-seconds`), at
most four per transaction. Native spending is capped at 250 PAS
(`--native-budget-raw`), each unload fee at 0.05 PAS (`--max-unload-fee-raw`), and
Bob's free PAS never drops below 4500 (`--native-floor-raw`). Before each signature
the bot checks the worst case against these bounds and stops rather than exceeding
one. It also stops after six consecutive failed or dropped transactions.

### Eight-hour restart on October 1

The existing checkpoint was resumed on documented Devnet with denomination IDs
1, 2, 3 and 4 (0.02, 0.04, 0.08 and 0.16). Its planned end is
2026-10-01 15:53:08 UTC, or 22:53:08 GMT+7. The interval is 90 seconds,
the voucher hold time is 30 minutes, the held-value cap is 2.00 and the liquid
asset floor is 0.50. The native floor is 1500 PAS. These settings supersede the
historical 96-hour run settings above for this local run.

Withdrawal selection prefers the oldest mature vouchers. If four small vouchers
cannot fund the next load, it tries the largest mature vouchers within the same
input limit. It never withdraws young or unconfirmed vouchers for this fallback.
This fixes the earlier stop while larger recoverable vouchers were available.

The run reuses saved vouchers and remains under launchd supervision. Its directory
keeps the historical name `runs/recycler-bot-devnet-96h`; the current deadline is
in `state.json` and `status.json`. The configured duration is measured from the
original checkpoint start, so it includes the time the bot was stopped.

See [independently checked restart transactions](evidence/recycler-bot-2026-10-01-restart.json).
Finalization timing is not the mobile app's clearing time. Activity only affects
the selected denomination pools on this network; it does not establish that
Alex's full-privacy settings will become ready immediately.

### Failure handling

- At most one transaction is unresolved. Its signed bytes, nonce and mortal era
  (64 blocks from a stated finalized block) are saved before broadcasting.
- Only canonical finalized blocks settle it. The bot searches each finalized block
  of the era for the transaction hash and reads its events. Best-block inclusion,
  watch events and timeouts are only logged.
- A transaction absent from every canonical block of its fully finalized era is
  dropped: it can never be included, so the load is retried with a new key.
- Recycler membership of the new key must agree with that result, and withdrawn
  aliases must be `Unloaded`. Any disagreement halts the run for inspection.
- Rebroadcasting the same bytes is safe: the nonce can be consumed once. The bot
  rebroadcasts only when no recent best block contains the transaction.
- Chain calls time out after 60 seconds. The WebSocket client rotates between
  the three documented endpoints.
- launchd restarts the bot after a crash, at most once a minute. After 20
  consecutive crashes, counting deaths such as `SIGKILL`, it halts. A halt, a
  `STOP` file, SIGTERM or the deadline ends the process with exit code 0. Stopping
  waits until any in-flight transaction settles.
- `caffeinate -i -s` prevents idle and system sleep while on AC power. Closing the
  lid can still suspend the Mac. After a pause longer than two minutes, the bot
  records `clock-gap` and reconciles before signing again.

Run files stay in the ignored run directory. `vouchers.json` holds the voucher
secrets and is written before each key is signed. `state.json` is the checkpoint.
`receipts.jsonl` records every submission, watch event, settlement and membership
check. `status.json` is a heartbeat. `bot.log` is the process log. Only
`recycler-evidence.ts` output, which contains public chain data, belongs in
`evidence/`.

The activity comes from well-known dev keys. It raises ring counts without adding
independent users, so it does not create real privacy. It also lowers the app's
fungibility estimate, which subtracts unloaded members. A useful comparison has
Alex test on this network, in a denomination whose ring the bot affects, with the
privacy mode held constant.

## What an outside observer sees

The direct transfers in this example are public. A transaction hash is a lookup
key for the transaction, not an encryption boundary: the signed extrinsic reveals
the sender, the call contains the recipient and `CoinTransferred` exposes the
instance, denomination and new age. With well-known dev addresses, an observer
can identify Alice → Bob without access to this script or its labels. See the
[transfer call and event](https://github.com/paritytech/individuality-community/blob/fce93ef38a15c673a8b0b208362bc46ae755c7d7/pallets/coinage/src/lib.rs#L2440)
and the [verified transaction](evidence/README.md).

The recycler unload uses a ring proof to avoid revealing which loaded member is
being spent. Its output address and denomination are still public. That breaks a
direct cryptographic link to a particular input; it does not conceal subsequent
coin transfers or eliminate timing/amount correlations. Real wallets can use
fresh coin-owner keys, so visible addresses need not identify real people. This
demo deliberately uses recognizable, reused dev accounts and does not demonstrate
end-to-end user anonymity.

## Receipts and recovery

Every run creates a separate directory under `runs/`:

- `preflight.json`: endpoint, finalized block, instance and the most recent preflight balances.
- `receipts.jsonl`: submitted transaction bytes/hashes, finalized block/index,
  success/failure, events and verified coin states.
- `vouchers.json`: recycler and paid-token secrets, saved **before** submission.
  Keep this file to retain control of the held value; it is git-ignored.
- `run.json`: the arguments needed to reproduce or resume the run.

Independently verify finalized receipts by fetching the canonical blocks,
rehashing each extrinsic and checking its `System.ExtrinsicSuccess` event:

```sh
pnpm verify --receipts runs/demo/receipts.jsonl --output runs/demo/verification.json
```

Ctrl-C stops after the current operation. Resume with the same configuration:

```sh
pnpm demo --run --resume --output runs/demo
```

Resume reuses saved secrets and skips finalized calls. A previous dispatch failure
or ambiguous submission stops the run for inspection; the script never assumes
a timeout means a transaction failed. An existing coin prevents a new run from
accidentally overwriting the shared dev accounts' inventory. After a hard process
kill, check that the recorded process has stopped before removing `running.lock`.

Transactions use an eight-block mortal era on PreviewNet (six-second blocks),
shorter than the runtime's 60-second failed-coin lock. Four blocks proved too short
for the observed RPC/finality lag. If an expired transaction is rejected, inspect
the recorded result and resume rather than blindly resubmitting.

## Local engine fallback

If remote PreviewNet is unavailable, use
[previewnet-engine](https://github.com/paritytech/previewnet-engine). Its CLI
requires Node.js 24+. Start a fork to retain the configured Coinage instance and
backing assets:

```sh
# In another terminal, outside this checkout:
npx --yes @parity/ppn start --fork previewnet

# Once People is producing finalized blocks:
pnpm demo --endpoint ws://127.0.0.1:10010
pnpm demo --run --endpoint ws://127.0.0.1:10010 --output runs/local-demo
pnpm verify --endpoint ws://127.0.0.1:10010 \
  --receipts runs/local-demo/receipts.jsonl --output runs/local-demo/verification.json
```

The engine dashboard is at `http://127.0.0.1:8090`. Keep local and remote run
inventories separate. The example does not start a local chain automatically or
switch chains after a partially successful run. A genesis engine may require
Coinage instance/asset setup; the preflight reports missing configuration.

## Sources

The protocol signing pattern follows [`paritytech/triangle-e2e`'s Coinage helpers](https://github.com/paritytech/triangle-e2e/blob/4a7ecb90baf4854c17e95c85d140033b8fd3cda2/packages/chain-tests/src/lib/coinage.ts)
at commit [`4a7ecb9` (`coinage-signer.ts`)](https://github.com/paritytech/triangle-e2e/blob/4a7ecb90baf4854c17e95c85d140033b8fd3cda2/packages/chain-tests/src/lib/coinage-signer.ts), adapted for the deployed multi-instance runtime, bounded proof
bytes and current `verifiablejs`. Runtime references are
[pallets/coinage/src/lib.rs](https://github.com/paritytech/individuality-community/blob/fce93ef38a15c673a8b0b208362bc46ae755c7d7/pallets/coinage/src/lib.rs), [pallets/coinage/src/extension.rs](https://github.com/paritytech/individuality-community/blob/fce93ef38a15c673a8b0b208362bc46ae755c7d7/pallets/coinage/src/extension.rs), [pallets/coinage/src/paid_tkn_manager.rs](https://github.com/paritytech/individuality-community/blob/fce93ef38a15c673a8b0b208362bc46ae755c7d7/pallets/coinage/src/paid_tkn_manager.rs).
Denomination references are iOS [Packages/Coinage/Sources/Denomination/Denomination.swift](https://github.com/paritytech/polkadot-app-ios-v2/blob/88f790aa05bfcf532fa24d94a451611da33bed1b/Packages/Coinage/Sources/Denomination/Denomination.swift)
and Android [RealCoinAmountBreakdownContext.kt](https://github.com/paritytech/polkadot-app-android-v2/blob/ba3e157498e6f89c5feb46272092ae952181f3c2/feature/coinage/impl/src/main/java/io/pcf/polkadotapp/feature_coinage_impl/domain/common/RealCoinAmountBreakdownContext.kt). Brevity's wallet UI delegates
Coinage operations to its [core](https://github.com/paritytech/brevity-dozer/blob/0fb3fa214c8abeb7a33a7db0db60c257ea069c8e/core/crates/brevity-viewmodel/src/coinage.rs); it is not required by this standalone example.


### Multiple public test accounts

`recycler-bot.ts --account Alice|Bob|Charlie|Dave|Eve|Ferdie` selects a public
Devnet signer when creating a run. Use a separate output directory for each account.
Resume reads the saved account; a mismatched signer or payer is rejected. Old run
files without an account name remain Bob runs. Each signer has one nonce lock and
one launchd label (`io.parity.coinage-demo.recycler-bot` for Bob, with `.alice`,
`.charlie`, `.dave`, `.eve` or `.ferdie` for the others).

For example, after funding Alice with at least 3 CASH and sufficient PAS:

```sh
node --import tsx recycler-bot.ts --init --account Alice --output runs/alice-demo \
  --duration-seconds 28800 --interval-seconds 90 --denominations 1,2,3,4 \
  --max-held-raw 2000000 --asset-floor-raw 500000 --min-hold-seconds 1800 \
  --native-floor-raw 100000000000 --native-budget-raw 250000000000
./recycler-service.sh start runs/alice-demo
./recycler-service.sh status runs/alice-demo
```

The four denomination IDs represent 0.02, 0.04, 0.08 and 0.16 CASH. Six bots at
90-second intervals target 240 successful loads per hour and about 18 CASH/hour
loaded, before finality, ring ceilings and funding constraints. Withdrawals reuse
that capital; adding incoming and outgoing amounts measures gross turnover, not
unique funds. These are targets, not measured throughput. Larger CASH balances
alone do not change denomination coverage or fill other denomination rings.

### Private demo accounts

Use `--mnemonic-file /absolute/path/mnemonics.json --account Demo-1` when
creating a run with a private signer. The file must have owner-only permissions
(`chmod 600`). It contains an `accounts` array; each entry has `name`, `address`,
`mnemonic`, `derivationPath` (an empty string for the generated demo accounts)
and `crypto: "sr25519"`. Keep this file outside Git or under ignored `runs/`.

Only the file path, entry name and public signer identity go into `run.json`.
Resume reads the same private file and verifies that the derived address matches.
Signer changes, duplicate entries, mismatched addresses, well-known dev keys and
files readable by other users are rejected. Private accounts have independent
locks and launchd services keyed by their public key. No mnemonic is passed in a
command-line argument or written to receipts.

The short smoke test uses these settings to exercise both loading and withdrawal:

```sh
node --import tsx recycler-bot.ts --init \
  --mnemonic-file /absolute/path/mnemonics.json --account Demo-1 \
  --output runs/private-smoke-demo-1 --duration-seconds 300 \
  --interval-seconds 30 --denominations 1,2,3,4 \
  --max-held-raw 300000 --asset-floor-raw 100000 --min-hold-seconds 30 \
  --native-floor-raw 100000000000 --native-budget-raw 20000000000
./recycler-service.sh start runs/private-smoke-demo-1
```

Fund each account with 0.50 CASH and 20 PAS first. The run stops after its
five-minute deadline, settling any in-flight transaction. Held voucher secrets
remain in the run directory for later withdrawal or reuse. The 30-second hold
is a test setting, not a mobile-app privacy guarantee.
