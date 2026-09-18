# game — offchain worker verification on a live network

Drives `indiv-pallet-game` on the spawned zombienet network and asserts that the People collator's
offchain worker applies every step of the game state machine: `start_game`, `end_registration`,
`advance_shuffle`, `end_reporting`, `process_players` and `advance_cancelling`.

## Scenario

| step | what runs |
|---|---|
| idle | nothing scheduled, nothing due |
| 1 game | Alice signs up, the game runs registration → shuffle → reporting → player process |
| idle | queue empty again |
| 3 games | scheduled in one call, run back to back, Alice signs up for each |
| 1 game | nobody signs up, so it takes the cancel path |
| idle | queue empty, no game |

About 37 minutes at the default phase durations (measured).

## Prerequisites

The runtime changed, so its WASM and the chain specs must be rebuilt even if they already exist.

```bash
cd e2e
just build-runtimes
just gen-specs
just spawn        # terminal A, stays in the foreground
just bootstrap    # terminal B, once the relay and both parachains produce blocks
```

Wait for the readiness probe in `e2e/zombienet/README.md` before running the suite. `just
descriptors` is **not** needed: the suite uses PAPI's unsafe API and no generated descriptors.

## Run

```bash
cd e2e
pnpm run test:game
```

Or from this directory: `RUN_ZOMBIENET_TESTS=1 ../../node_modules/.bin/vitest run`. Without
`RUN_ZOMBIENET_TESTS=1` every test is skipped.

`PEOPLE_RPC` overrides the endpoint, which otherwise comes from
`scripts/initial-setup/config-local.env` (`ws://localhost:10010`).

`pnpm run lint`, `pnpm run format:check` and `pnpm run typecheck` from `e2e` cover this directory.
No CI job runs the suite, because it needs a live network and about 37 minutes. The suite has no
`package.json` on purpose: `suites/*` in `pnpm-workspace.yaml` matches directories that have one,
and a new workspace project adds an importer to `pnpm-lock.yaml`, which fails
`pnpm install --frozen-lockfile` in CI until the lockfile is regenerated.

## What it prints

One line per state change and per `Game.*` event, then the full timeline at the end:

```
  #412 Registration(next_player_index=0) [schedules=0]  Game.NewGame {...}
  #418 Registration(next_player_index=1) [schedules=0]  Game.SignedUp {...}
  #480 Shuffle/Step1Insert [schedules=0]
  #481 Shuffle/Step2Retrieve [schedules=0]
  ...
```

On a failure the error carries the timeline and the collator's log lines, so a stall names the
phase it stopped in.

## Notes

- **Phase durations.** Defaults are 60/120/24/60/60 seconds against the runtime's 300/60/30/600/60.
  Override per phase with `GAME_PHASE_REGISTRATION`, `GAME_PHASE_SHUFFLE`, `GAME_PHASE_MARGIN`,
  `GAME_PHASE_REPORTING`, `GAME_PHASE_PLAYER_PROCESS`.
- **Do not shorten the shuffle.** Game deadlines are compared against the block timestamp, which on
  this network advances in **12-second jumps**, and the offchain worker submits at most one step per
  block. After `registration_ends`, `end_registration` alone costs about two blocks, and
  `advance_shuffle` must still finish before `game_play_time - post_shuffle_margin` or the game is
  cancelled. A 30-second shuffle reliably loses that race; the default gives it ten clock ticks.
- **Node log.** The suite tails the People collator's log under `e2e/zombienet/.tools/net/` and
  reports the pallet's lines alongside a failure. It first tries to raise the log level over RPC,
  which the pinned `polkadot-omni-node` refuses (`No reload handle present`, logged as a warning
  and otherwise ignored). The pallet's `warn` lines still reach the log at the default level. For
  its `debug`/`trace` lines, add the targets at spawn time to the People collator in
  `e2e/zombienet/network.toml` and do not commit the edit:

  ```toml
  args = [
  	# existing args...
  	"-lruntime::indiv-pallet-game=debug",
  	"-loffchain=debug",
  ]
  ```
- ``offchain worker: `<step>` rejected by the transaction pool`` in that log is **normal**. A step
  is tagged `(step, game_index)`, so once a submission sits in the pool every later block's
  resubmission is rejected as a duplicate. Only a phase that stops advancing is a failure, and the
  state-transition timeouts catch that.
- **Steps emit no events.** `advance_shuffle` and `process_players` are recognised by the change in
  `Game.Game`, not by an event, which is why the monitor fills in the blocks that finalization
  skips.
- **One player is enough.** The People runtime sets `TESTNET`, so a single registered player is an
  acceptable player count and the game reaches the shuffle instead of being cancelled.
- **`set_play_deposit` rejects zero** (`Error::InvalidPlayDeposit`, confirmed against the live
  chain), so the suite sets a real deposit and Alice pays it on her first sign-up.
- **Leave the chain idle at the start.** The suite refuses to run while a game is already in
  progress.
