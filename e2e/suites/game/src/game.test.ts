// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { afterAll, beforeAll, describe, expect, test } from "vitest";

import { chainNow, connect, devSigner, type Submit, type Sudo, submitters } from "./chain.ts";
import {
  CLOCK_TICK,
  firstOffset,
  minShuffleDuration,
  PHASES,
  playTimes,
  readStepLatency,
  scheduleGames,
  setPhases,
  setPlayDeposit,
  signUp,
  spacing,
} from "./drive.ts";
import { RUN_ZOMBIENET_TESTS } from "./env.ts";
import { type BlockRecord, type Monitor, startMonitor } from "./monitor.ts";

const SECOND = 1_000;

/**
 * The step latency the timeouts below are sized for.
 */
const TIMEOUT_STEP_LATENCY = 2 * CLOCK_TICK;

/** A game runs from its registration start until the reporting deadline, plus the tail steps. */
const GAME_MS = (firstOffset(TIMEOUT_STEP_LATENCY) + PHASES.reporting + PHASES.player_process) * SECOND;
/** Every wait is given the phase it covers plus a generous tail for inclusion latency. */
const PHASE_SLACK_MS = 90 * SECOND;
/** Finalized blocks each idle window must observe without any game activity. */
const IDLE_BLOCKS = 10;

const ONE_GAME_TIMEOUT = GAME_MS + PHASE_SLACK_MS;
const THREE_GAMES_TIMEOUT = GAME_MS + 2 * spacing(TIMEOUT_STEP_LATENCY) * SECOND + 3 * PHASE_SLACK_MS;

/** An idle window plus room for the monitor to catch up before it starts. */
const IDLE_TIMEOUT = IDLE_BLOCKS * 15 * SECOND + 120 * SECOND;

/** Recognises a state string produced by `describeState`, ignoring its payload. */
const inPhase = (phase: string) => (entry: BlockRecord) =>
  entry.state === phase || entry.state.startsWith(`${phase}(`) || entry.state.startsWith(`${phase}/`);

const sawEvent = (entry: BlockRecord, name: string) =>
  entry.events.some(line => line.startsWith(`Game.${name} `));

/**
 * Follows one game from registration until it finishes. Asserts the offchain worker applies every
 * step. `index` is the game index the game is expected to take.
 */
async function expectFullGame(monitor: Monitor, index: number, signUpPlayer: () => Promise<void>) {
  const registration = await monitor.waitFor(
    entry => inPhase("Registration")(entry) && entry.gameIndex === index,
    `game ${index} to enter Registration (start_game)`,
    ONE_GAME_TIMEOUT,
  );
  expect(sawEvent(registration, "NewGame")).toBe(true);

  await signUpPlayer();

  // A game that cancels never reaches the next phase. Naming that branch beats waiting out the
  // timeout.
  const notCancelled = {
    predicate: inPhase("Cancelling"),
    reason: `game ${index} was cancelled`,
  };

  // One registered player is enough. The People runtime sets `TESTNET`, which makes that player
  // count acceptable. `end_registration` then moves the game to the shuffle instead of cancelling.
  await monitor.waitFor(
    inPhase("Shuffle"),
    `game ${index} to enter Shuffle (end_registration)`,
    ONE_GAME_TIMEOUT,
    notCancelled,
  );
  await monitor.waitFor(
    inPhase("Reporting"),
    `game ${index} to enter Reporting (advance_shuffle)`,
    ONE_GAME_TIMEOUT,
    notCancelled,
  );
  await monitor.waitFor(
    inPhase("PlayerProcess"),
    `game ${index} to enter PlayerProcess (end_reporting)`,
    ONE_GAME_TIMEOUT,
    notCancelled,
  );
  const ended = await monitor.waitFor(
    entry => entry.state === "none",
    `game ${index} to be killed (process_players)`,
    ONE_GAME_TIMEOUT,
  );
  expect(sawEvent(ended, "GameEnded")).toBe(true);
}

describe.skipIf(!RUN_ZOMBIENET_TESTS)("game offchain worker drives the game state machine", () => {
  let chain: ReturnType<typeof connect>;
  let monitor: Monitor;
  let sudo: Sudo;
  let submit: Submit;
  /** `Game::OcwStepLatency`, read from the chain so the suite follows the runtime's own rules. */
  let stepLatency: number;

  beforeAll(async () => {
    chain = connect();
    const signer = devSigner("//Alice");
    ({ sudo, submit } = submitters(chain.api, signer));

    // Raising the log level over RPC avoids editing the tracked zombienet network.toml
    try {
      await chain.client._request("system_addLogFilter", ["runtime::indiv-pallet-game=debug"]);
    } catch (error) {
      console.warn(`  could not raise the node log level: ${String(error)}`);
    }

    monitor = startMonitor(chain.client, chain.api);

    stepLatency = await readStepLatency(chain.api);
    console.log(`  OcwStepLatency: ${stepLatency}s (minimum shuffle ${minShuffleDuration(stepLatency)}s)`);
    expect(
      stepLatency,
      `OcwStepLatency exceeds the ${TIMEOUT_STEP_LATENCY}s the test timeouts are sized for`,
    ).toBeLessThanOrEqual(TIMEOUT_STEP_LATENCY);

    const existing = await chain.api.query.Game.Game.getValue();
    expect(existing, "a game is already running; wait for it to finish or cancel it").toBe(undefined);

    await setPhases(chain.api, sudo, stepLatency);
    await setPlayDeposit(chain.api, sudo);
  }, 120 * SECOND);

  afterAll(async () => {
    monitor?.stop();
    try {
      await chain?.client._request("system_resetLogFilter", []);
    } catch {
      // The filter was never raised.
    }
    console.log(`--- timeline ---\n${monitor?.timeline() ?? ""}`);
    chain?.client.destroy();
  });

  test(
    "nothing is due while no game is scheduled",
    async () => {
      await monitor.expectIdle(IDLE_BLOCKS, "idle before any schedule");
    },
    IDLE_TIMEOUT,
  );

  test(
    "one scheduled game runs end to end",
    async () => {
      const before = Number(await chain.api.query.Game.GameIndex.getValue());
      const now = await chainNow(chain.api);
      await scheduleGames(chain.api, sudo, playTimes(now, 1, stepLatency));

      await expectFullGame(monitor, before + 1, () => signUp(chain.api, submit));

      const history = await chain.api.query.Game.GameHistory.getValue(before + 1);
      expect(history, "the finished game is recorded in GameHistory").not.toBe(undefined);
      const schedules = await chain.api.query.Game.GameSchedules.getValue();
      expect(schedules).toHaveLength(0);
    },
    ONE_GAME_TIMEOUT + 60 * SECOND,
  );

  test(
    "nothing is due again once the queue is empty",
    async () => {
      await monitor.expectIdle(IDLE_BLOCKS, "idle between batches");
    },
    IDLE_TIMEOUT,
  );

  test(
    "three scheduled games run back to back",
    async () => {
      const before = Number(await chain.api.query.Game.GameIndex.getValue());
      const now = await chainNow(chain.api);
      await scheduleGames(chain.api, sudo, playTimes(now, 3, stepLatency));

      for (let offset = 1; offset <= 3; offset += 1) {
        await expectFullGame(monitor, before + offset, () => signUp(chain.api, submit));
      }

      const schedules = await chain.api.query.Game.GameSchedules.getValue();
      expect(schedules).toHaveLength(0);
    },
    THREE_GAMES_TIMEOUT + 120 * SECOND,
  );

  test(
    "a game nobody signs up for is cancelled",
    async () => {
      const before = Number(await chain.api.query.Game.GameIndex.getValue());
      const now = await chainNow(chain.api);
      await scheduleGames(chain.api, sudo, playTimes(now, 1, stepLatency));

      const registration = await monitor.waitFor(
        entry => inPhase("Registration")(entry) && entry.gameIndex === before + 1,
        "the cancel-path game to enter Registration",
        ONE_GAME_TIMEOUT,
      );
      expect(sawEvent(registration, "NewGame")).toBe(true);

      // Without a sign-up, `end_registration` finds an unacceptable player count and cancels.
      const cancelling = await monitor.waitFor(
        inPhase("Cancelling"),
        "the game to be cancelled (end_registration)",
        ONE_GAME_TIMEOUT,
        { predicate: inPhase("Shuffle"), reason: "the game reached the shuffle, so a player registered" },
      );
      expect(sawEvent(cancelling, "GameCancelled")).toBe(true);

      const killed = await monitor.waitFor(
        entry => entry.state === "none",
        "the cancelled game to be removed (advance_cancelling)",
        ONE_GAME_TIMEOUT,
      );
      // The cancel path kills the game without a `GameEnded` event. It leaves no history entry.
      expect(sawEvent(killed, "GameEnded")).toBe(false);
      const history = await chain.api.query.Game.GameHistory.getValue(before + 1);
      expect(history, "a cancelled game leaves no history entry").toBe(undefined);
    },
    ONE_GAME_TIMEOUT + 60 * SECOND,
  );

  test(
    "nothing is due after the last game",
    async () => {
      await monitor.expectIdle(IDLE_BLOCKS, "idle after the last game");
      const schedules = await chain.api.query.Game.GameSchedules.getValue();
      expect(schedules).toHaveLength(0);
      expect(await chain.api.query.Game.Game.getValue()).toBe(undefined);
    },
    IDLE_TIMEOUT,
  );
});
