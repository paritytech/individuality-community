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

## What it prints

One line per state change and per `Game.*` event, then the full timeline at the end:

```
  #412 Registration(next_player_index=0) [schedules=0]  Game.NewGame {...}
  #418 Registration(next_player_index=1) [schedules=0]  Game.SignedUp {...}
  #480 Shuffle/Step1Insert [schedules=0]
  #481 Shuffle/Step2Retrieve [schedules=0]
  ...
```

A block that includes a game step names it with the block whose offchain worker submitted it, for
example `advance_shuffle(submitted at #480)`. The run ends with a step landing summary:

```
--- game step landing ---
  steps landed: <n>, by blocks after the submitting block: 1: <n>, 2: <n>
  blocks with a step due: <n>, with a step landed: <n> (<share>%)
```

The first line counts the blocks from submission to inclusion. A lag of 1 is the next block. The
second line counts the blocks after a shuffle, player process or cancellation state, where a step is
always due. Its share is the fraction of blocks that run a step. Block time matters: the offchain
worker of block N must submit before block N+1 is built.

## Notes

- **Phase durations.** Defaults are 60/120/24/60/60 seconds against the runtime's 300/60/30/600/60.
  Override per phase with `GAME_PHASE_REGISTRATION`, `GAME_PHASE_SHUFFLE`, `GAME_PHASE_MARGIN`,
  `GAME_PHASE_REPORTING`, `GAME_PHASE_PLAYER_PROCESS`.
- ``offchain worker: `<step>` rejected by the transaction pool`` in that log is **normal**. A step
  is tagged `(step, game_index)`, so once a submission sits in the pool every later block's
  resubmission is rejected as a duplicate. Only a phase that stops advancing is a failure, and the
  state-transition timeouts catch that.
- **Steps emit no events.** `advance_shuffle` and `process_players` are recognised by the change in
  `Game.Game`, not by an event, which is why the monitor fills in the blocks that finalization
  skips.
- **One player is enough.** The People runtime sets `TESTNET`, so a single registered player is an
  acceptable player count and the game reaches the shuffle instead of being cancelled.
- **The floor is checked here.** "two games at the floor run back to back" sets the shuffle and the
  player process to `Game.MinStepPhaseDuration` and places the games one clock tick apart. The
  pallet can only reject a floor that can never work. Whether the floor the runtime ships holds on
  a real chain is what this test shows. A failure here means the runtime value is too low.
