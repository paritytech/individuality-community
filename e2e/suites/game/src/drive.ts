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

import { Binary } from "polkadot-api";

import type { PeopleApi, Submit, Sudo } from "./chain.ts";

/**
 * The granularity of the chain's clock, in seconds.
 *
 * Every game deadline is compared against `UnixTime::now()`, the block timestamp. On this network
 * that timestamp advances in 12-second jumps. Two parachain blocks share a value, then it steps. A
 * phase is measured in ticks of this size, not in blocks.
 */
export const CLOCK_TICK = 12;

/**
 * Game phase durations in seconds, expressed in whole clock ticks.
 *
 * A phase must fit the offchain worker's pace. The worker submits at most one step per block. Each
 * step lands a block or two later. The deadline it races moves one tick at a time.
 *
 * The shuffle is the tight phase. `end_registration` alone costs two blocks after
 * `registration_ends`. `advance_shuffle` must then finish before
 * `game_play_time - post_shuffle_margin`, or the game is cancelled. The shuffle gets the most
 * ticks.
 *
 * The production values are 300/60/30/600/60, set by `GamePhaseDurations` in the People runtime.
 * They make one game take 17.5 minutes. A `GAME_PHASE_*` override trades run time against headroom.
 */
export const PHASES = {
  registration: Number(process.env.GAME_PHASE_REGISTRATION ?? 5 * CLOCK_TICK),
  shuffle: Number(process.env.GAME_PHASE_SHUFFLE ?? 10 * CLOCK_TICK),
  post_shuffle_margin: Number(process.env.GAME_PHASE_MARGIN ?? 2 * CLOCK_TICK),
  reporting: Number(process.env.GAME_PHASE_REPORTING ?? 5 * CLOCK_TICK),
  player_process: Number(process.env.GAME_PHASE_PLAYER_PROCESS ?? 5 * CLOCK_TICK),
};

/** The play deposit charged to a new player, in plancks. */
export const PLAY_DEPOSIT = 10_000_000_000n;

/**
 * How far before `game_play_time` a game's registration may open.
 */
export const LEAD_TIME = PHASES.registration + PHASES.shuffle + PHASES.post_shuffle_margin;

/**
 * The shortest distance `schedule_games` accepts between two consecutive play times.
 */
export const MIN_SPACING =
  PHASES.registration +
  PHASES.shuffle +
  PHASES.post_shuffle_margin +
  PHASES.reporting +
  PHASES.player_process;

/**
 * Extra spacing on top of `MIN_SPACING`.
 *
 * `schedule_games` accepts games that touch exactly. `new_game` then rejects the second one. It
 * requires the registration start to be strictly in the future. At the minimum spacing the previous
 * game ends exactly on it. The slack also absorbs the player-process steps, which run a few blocks
 * past `game_play_time + reporting`.
 */
export const SPACING_SLACK = 90;

/** The distance between consecutive play times used by this suite. */
export const SPACING = MIN_SPACING + SPACING_SLACK;

/**
 * Headroom between reading the clock and `start_game` running, for the first game of a batch.
 *
 * `new_game` drops a schedule whose registration start has passed, with `OutdatedGameSetup`. Only
 * the first game is exposed to that. The clock is read. The schedule transaction is submitted. It
 * must then finalize. The offchain worker starts the game only after that. The chain starts later
 * games itself as soon as the previous one ends. They need only `SPACING_SLACK`.
 */
export const FIRST_GAME_MARGIN = 120;

/** How far ahead of `now` the first game of a batch is placed. */
export const FIRST_OFFSET = LEAD_TIME + FIRST_GAME_MARGIN;

/** Any 65 bytes. The game only stores this identifier for the players to find each other. */
const IDENTIFIER_KEY = Binary.toHex(new Uint8Array(65).fill(0x42));

/**
 * Sets the phase durations.
 */
export async function setPhases(api: PeopleApi, sudo: Sudo): Promise<void> {
  await sudo(api.tx.Game.set_game_phases({ phases: PHASES }).decodedCall, "Game.set_game_phases");
}

/** Sets the deposit a new player pays at sign-up. */
export async function setPlayDeposit(api: PeopleApi, sudo: Sudo): Promise<void> {
  await sudo(api.tx.Game.set_play_deposit({ amount: PLAY_DEPOSIT }).decodedCall, "Game.set_play_deposit");
}

/** Play times for `count` games. The first sits `FIRST_OFFSET` ahead of `from`, the rest `SPACING` apart. */
export function playTimes(from: number, count: number): number[] {
  return Array.from({ length: count }, (_value, index) => from + FIRST_OFFSET + index * SPACING);
}

/** Queues one game per play time. One player per group is enough on a `TESTNET` runtime. */
export async function scheduleGames(api: PeopleApi, sudo: Sudo, times: number[]): Promise<void> {
  const games_schedules = times.map(game_play_time => ({
    game_play_time,
    rounds: 1,
    max_group_size: 3,
    airdrops: [],
  }));
  await sudo(
    api.tx.Game.schedule_games({ games_schedules }).decodedCall,
    `Game.schedule_games(${times.join(", ")})`,
  );
}

/** Signs the sudo account up for the game that is currently in its registration phase. */
export async function signUp(api: PeopleApi, submit: Submit): Promise<void> {
  await submit(
    api.tx.Game.sign_up_with_account({ identifier_key: IDENTIFIER_KEY, airdrops: undefined }),
    "Game.sign_up_with_account",
  );
}
