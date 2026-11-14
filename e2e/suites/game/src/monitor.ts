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

import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { DecodedCall, PeopleApi, PeopleClient, TxFromCallData } from "./chain.ts";
import { toJson, type Untyped } from "./chain.ts";

const SUITE_DIR = path.dirname(fileURLToPath(import.meta.url));
/** Zombienet's native-provider base directory, one subdirectory per node. */
const ZOMBIENET_NET_DIR = path.resolve(SUITE_DIR, "..", "..", "..", "zombienet", ".tools", "net");

/** What the monitor recorded for one finalized block. */
export interface BlockRecord {
  number: number;
  hash: string;
  /** Wall-clock time the block was recorded, for the timeline dump. */
  at: number;
  /** The chain's own time in seconds. Game deadlines are measured against it. */
  chainTime: number;
  gameIndex: number;
  scheduled: number;
  /** The game state as a string. A transition is a string change. `none` means no game. */
  state: string;
  events: string[];
  /** The game steps the block includes. */
  steps: StepLanding[];
}

/** A game step call included in a block. */
export interface StepLanding {
  name: string;
  /** The `discriminator` argument: the block whose offchain worker submitted the step. */
  submittedAt: number;
}

/**
 * The length of each game step call: the pallet and call index, then `game_index: u32` for every
 * step except `start_game`, then `discriminator: u32`.
 */
const STEP_CALL_LENGTHS: Record<string, number> = {
  start_game: 6,
  end_registration: 10,
  advance_shuffle: 10,
  end_reporting: 10,
  process_players: 10,
  advance_cancelling: 10,
};

/**
 * The game steps in `extrinsics`, given as hex.
 *
 * The call is the last field of an extrinsic and a step call has a fixed length. Only the tail of
 * each extrinsic is decoded, which avoids decoding the transaction extensions. A tail that is not a
 * step call fails to decode or decodes to a call of another length.
 */
function findSteps(extrinsics: string[], txFromCallData: TxFromCallData): StepLanding[] {
  const steps: StepLanding[] = [];
  const lengths = [...new Set(Object.values(STEP_CALL_LENGTHS))];
  for (const hex of extrinsics) {
    const bytes = new Uint8Array(Buffer.from(hex.slice(2), "hex"));
    for (const length of lengths) {
      if (bytes.length < length) {
        continue;
      }
      let call: DecodedCall;
      try {
        call = txFromCallData(bytes.slice(-length)).decodedCall;
      } catch (error) {
        // A tail that is not a call fails with `Invalid call data`. A `TypeError` is a defect here.
        if (error instanceof TypeError) {
          throw error;
        }
        continue;
      }
      const name = call.value.type;
      if (call.type === "Game" && STEP_CALL_LENGTHS[name] === length) {
        steps.push({ name, submittedAt: Number(call.value.value.discriminator) });
        break;
      }
    }
  }
  return steps;
}

/**
 * Whether a block whose parent ended in `state` always has a game step due.
 *
 * The shuffle, the player process and the cancellation run one step per block until they finish.
 * Two waits are the exception. The shuffle waits for the score session in `Step4AwaitSession`.
 * The player process waits for credit buffer space, which the state does not show. The tests run
 * one player per game, so the player process does not fill the buffers.
 */
function stepDueAfter(state: string): boolean {
  if (state === "Shuffle/Step4AwaitSession") {
    return false;
  }
  return ["Shuffle/", "PlayerProcess/", "Cancelling/"].some(prefix => state.startsWith(prefix));
}

export const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

/**
 * Renders `GameInfo.state` as a single line.
 *
 * Every offchain-worker step mutates this value. A change between two blocks is the signal that a
 * step was applied. The steps emit no event of their own.
 */
function describeState(game: Untyped): string {
  if (game === undefined || game === null) {
    return "none";
  }
  const state = game.state;
  const step = state?.value?.step ?? state?.value;
  switch (state?.type) {
    case "Registration":
      return `Registration(next_player_index=${state.value?.next_player_index})`;
    case "Shuffle":
      return `Shuffle/${step?.type}`;
    case "Reporting":
      return `Reporting(player_count=${state.value?.player_count})`;
    case "PlayerProcess":
      return `PlayerProcess/${step?.type}`;
    case "Cancelling":
      return `Cancelling/${step?.type}`;
    default:
      return `unknown(${toJson(state)})`;
  }
}

/** The `Game.*` and failed-extrinsic events of one block, as readable lines. */
function describeEvents(events: Untyped[]): string[] {
  const out: string[] = [];
  for (const record of events) {
    const event = record?.event;
    if (event?.type === "Game") {
      out.push(`Game.${event.value?.type} ${toJson(event.value?.value)}`);
    } else if (event?.type === "System" && event.value?.type === "ExtrinsicFailed") {
      out.push(`System.ExtrinsicFailed ${toJson(event.value?.value?.dispatch_error)}`);
    }
  }
  return out;
}

/** The state, the landed steps and the events of a recorded block, as one line. */
function describeRecord(entry: BlockRecord): string {
  const steps = entry.steps.map(step => `${step.name}(submitted at #${step.submittedAt})`);
  const tail = [...steps, ...entry.events];
  return `${entry.state} [schedules=${entry.scheduled}]${tail.length > 0 ? `  ${tail.join(" | ")}` : ""}`;
}

/** The People collator's log file, or `undefined` when the network was spawned elsewhere. */
function findCollatorLog(): string | undefined {
  let nodeDirs: string[];
  try {
    nodeDirs = readdirSync(ZOMBIENET_NET_DIR);
  } catch {
    return undefined;
  }
  for (const dir of nodeDirs.filter(name => name.includes("people"))) {
    const base = path.join(ZOMBIENET_NET_DIR, dir);
    const stack = [base];
    while (stack.length > 0) {
      const current = stack.pop() as string;
      let entries: string[];
      try {
        entries = readdirSync(current);
      } catch {
        continue;
      }
      for (const entry of entries) {
        const full = path.join(current, entry);
        if (entry.endsWith(".log")) {
          return full;
        }
        // Only the node directory's own tree is searched. The databases below it are deep.
        if (current === base && statSync(full).isDirectory() && entry !== "data") {
          stack.push(full);
        }
      }
    }
  }
  return undefined;
}

/**
 * Watches the People chain. Records one entry per finalized block.
 *
 * The recorded entries are consumed in order. [`Monitor.waitFor`] matches only blocks after the one
 * that satisfied the previous wait. A sequence of waits asserts a sequence of transitions, never
 * the same block twice.
 */
export function startMonitor(client: PeopleClient, api: PeopleApi) {
  const records: BlockRecord[] = [];
  const waiters: Array<() => void> = [];
  let cursor = 0;
  let lastNumber: number | undefined;
  let failure: Error | undefined;
  let queue: Promise<void> = Promise.resolve();
  let logOffset = 0;
  const logPath = findCollatorLog();

  if (logPath === undefined) {
    console.warn(`  monitor: no collator log under ${ZOMBIENET_NET_DIR}, log checks skipped`);
  } else {
    logOffset = statSync(logPath).size;
    console.log(`  monitor: tailing ${logPath}`);
  }

  const staticApis = api.getStaticApis();

  const record = async (block: { number: number; hash: string }) => {
    const at = { at: block.hash };
    // The legacy RPC serves any block. `getBlockBody` serves only the blocks the client pins, and
    // the gap blocks `recordFrom` fills in are not pinned.
    const [game, gameIndex, schedules, events, millis, body, { txFromCallData }] = await Promise.all([
      api.query.Game.Game.getValue(at),
      api.query.Game.GameIndex.getValue(at),
      api.query.Game.GameSchedules.getValue(at),
      api.query.System.Events.getValue(at),
      api.query.Timestamp.Now.getValue(at),
      client._request<{ block: { extrinsics: string[] } }>("chain_getBlock", [block.hash]),
      staticApis,
    ]);

    const entry: BlockRecord = {
      number: block.number,
      hash: block.hash,
      at: Date.now(),
      chainTime: Number((millis as bigint) / 1000n),
      gameIndex: Number(gameIndex ?? 0),
      scheduled: (schedules as unknown[] | undefined)?.length ?? 0,
      state: describeState(game),
      events: describeEvents((events as Untyped[]) ?? []),
      steps: findSteps(body.block.extrinsics, txFromCallData),
    };
    records.push(entry);

    const previous = records[records.length - 2];
    if (
      previous === undefined ||
      previous.state !== entry.state ||
      entry.events.length > 0 ||
      entry.steps.length > 0
    ) {
      console.log(`  #${entry.number} ${describeRecord(entry)}`);
    }

    const failed = entry.events.find(line => line.startsWith("System.ExtrinsicFailed"));
    if (failed !== undefined) {
      failure ??= new Error(`extrinsic failed in block #${entry.number}: ${failed}`);
    }

    // A dropped schedule never becomes a game. Every later wait would time out on a state that can
    // no longer arrive. Reporting the cause instead.
    const dropped = entry.events.find(line => line.startsWith("Game.GameScheduleDropped"));
    if (dropped !== undefined) {
      failure ??= new Error(`a schedule was dropped in block #${entry.number}: ${dropped}`);
    }

    for (const wake of waiters.splice(0)) {
      wake();
    }
  };

  /**
   * Records `block` together with every block between it and the last recorded one.
   *
   * Finalization advances in jumps. `finalizedBlock$` therefore skips blocks. A phase can be
   * shorter than one jump. A shuffle over one player takes four blocks. A skipped block hides the
   * transition that proves the step ran.
   */
  const recordFrom = async (block: { number: number; hash: string }) => {
    if (lastNumber !== undefined && block.number <= lastNumber) {
      return;
    }
    if (lastNumber !== undefined) {
      for (let number = lastNumber + 1; number < block.number; number += 1) {
        const hash = await client._request<string>("chain_getBlockHash", [number]);
        await record({ number, hash });
      }
    }
    await record(block);
    lastNumber = block.number;
  };

  const subscription = client.finalizedBlock$.subscribe({
    next: block => {
      queue = queue
        .then(() => recordFrom(block))
        .catch((error: Error) => {
          failure ??= error;
        });
    },
    error: (error: Error) => {
      failure ??= error;
    },
  });

  /** The pallet's own log lines written since the last call. */
  const newLogLines = (): string[] => {
    if (logPath === undefined) {
      return [];
    }
    const size = statSync(logPath).size;
    if (size <= logOffset) {
      return [];
    }
    const buffer = Buffer.alloc(size - logOffset);
    const handle = readFileSync(logPath);
    handle.copy(buffer, 0, logOffset, size);
    logOffset = size;
    return buffer
      .toString("utf8")
      .split("\n")
      .filter(line => line.includes("indiv-pallet-game"));
  };

  const timeline = (): string =>
    records.map(entry => `  #${entry.number} t=${entry.chainTime} ${describeRecord(entry)}`).join("\n");

  /**
   * When the game steps landed, over every block recorded so far.
   *
   * The lag of a step is the number of blocks from the block whose offchain worker submitted it to
   * the block that includes it. A lag of 1 is the next block. A due block is a block whose parent
   * left the game in a state with a step due, see `stepDueAfter`.
   */
  const stepReport = (): string => {
    const lags = new Map<number, number>();
    let landed = 0;
    let due = 0;
    let dueWithStep = 0;
    for (const [index, entry] of records.entries()) {
      for (const step of entry.steps) {
        const lag = entry.number - step.submittedAt;
        lags.set(lag, (lags.get(lag) ?? 0) + 1);
        landed += 1;
      }
      const parent = records[index - 1];
      if (parent !== undefined && parent.number + 1 === entry.number && stepDueAfter(parent.state)) {
        due += 1;
        if (entry.steps.length > 0) {
          dueWithStep += 1;
        }
      }
    }
    const lagCounts = [...lags.entries()]
      .sort(([a], [b]) => a - b)
      .map(([lag, count]) => `${lag}: ${count}`)
      .join(", ");
    const share = due > 0 ? ` (${Math.round((100 * dueWithStep) / due)}%)` : "";
    return [
      `  steps landed: ${landed}, by blocks after the submitting block: ${lagCounts || "none"}`,
      `  blocks with a step due: ${due}, with a step landed: ${dueWithStep}${share}`,
    ].join("\n");
  };

  const fail = (message: string): never => {
    throw new Error(
      `${message}\n--- timeline ---\n${timeline()}\n--- collator log ---\n${newLogLines().join("\n")}`,
    );
  };

  const throwIfFailed = () => {
    if (failure !== undefined) {
      const error = failure;
      failure = undefined;
      fail(error.message);
    }
  };

  return {
    records,
    timeline,
    stepReport,
    newLogLines,

    /**
     * Resolves with the first block after the previous wait whose record satisfies `predicate`.
     * Moves the cursor past that block. Throws with the timeline when `timeoutMs` elapses first.
     *
     * `forbid` names states the game must not reach while waiting. A game that takes the wrong
     * branch never reaches the awaited state. Without `forbid` the wait ends only at its timeout,
     * minutes later and pointing at the wrong thing.
     */
    async waitFor(
      predicate: (entry: BlockRecord) => boolean,
      what: string,
      timeoutMs: number,
      forbid?: { predicate: (entry: BlockRecord) => boolean; reason: string },
    ): Promise<BlockRecord> {
      const deadline = Date.now() + timeoutMs;
      for (;;) {
        throwIfFailed();
        while (cursor < records.length) {
          const entry = records[cursor] as BlockRecord;
          cursor += 1;
          if (predicate(entry)) {
            return entry;
          }
          if (forbid?.predicate(entry)) {
            return fail(`while waiting for ${what}: ${forbid.reason} in block #${entry.number}`);
          }
        }
        if (Date.now() >= deadline) {
          const last = records[records.length - 1];
          return fail(
            `timed out after ${timeoutMs}ms waiting for ${what}; last state ${last?.state ?? "none recorded"}`,
          );
        }
        await Promise.race([
          new Promise<void>(resolve => waiters.push(resolve)),
          sleep(Math.min(2_000, Math.max(0, deadline - Date.now()))),
        ]);
      }
    },

    /**
     * Resolves once every block finalized so far has been recorded.
     *
     * Recording lags the chain. A caller that just saw a transaction finalize may be ahead of the
     * monitor. Without this the blocks carrying that transaction land inside the next idle window.
     * They are read there as unexpected activity.
     */
    async sync(timeoutMs = 60_000): Promise<void> {
      const target = (await client.getFinalizedBlock()).number;
      const deadline = Date.now() + timeoutMs;
      while ((records[records.length - 1]?.number ?? -1) < target) {
        throwIfFailed();
        if (Date.now() >= deadline) {
          return fail(`timed out waiting for the monitor to reach block #${target}`);
        }
        await Promise.race([new Promise<void>(resolve => waiters.push(resolve)), sleep(500)]);
      }
      cursor = records.length;
    },

    /**
     * Asserts the chain stays quiet over the next `minBlocks` finalized blocks. The game state does
     * not change. No `Game.*` event is emitted.
     *
     * The window is counted in blocks, not in wall-clock time. Finality arrives in bursts. A fixed
     * duration can cover a single block and assert almost nothing.
     */
    async expectIdle(minBlocks: number, what: string): Promise<void> {
      throwIfFailed();
      await this.sync();
      const from = records.length;
      const baseline = records[from - 1]?.state ?? "none";
      // Finality lags block authoring. The budget is well over the nominal block time per block.
      const deadline = Date.now() + minBlocks * 15_000 + 30_000;

      while (records.length - from < minBlocks) {
        throwIfFailed();
        if (Date.now() >= deadline) {
          return fail(`${what}: only ${records.length - from} of ${minBlocks} blocks were finalized in time`);
        }
        await Promise.race([new Promise<void>(resolve => waiters.push(resolve)), sleep(500)]);
      }

      for (const entry of records.slice(from, from + minBlocks)) {
        if (entry.state !== baseline) {
          return fail(`${what}: state changed from ${baseline} to ${entry.state} in block #${entry.number}`);
        }
        if (entry.events.length > 0) {
          return fail(`${what}: unexpected events in block #${entry.number}: ${entry.events.join(" | ")}`);
        }
      }
      cursor = records.length;
      console.log(`  idle confirmed over ${minBlocks} blocks (${what})`);
    },

    stop() {
      subscription.unsubscribe();
    },
  };
}

export type Monitor = ReturnType<typeof startMonitor>;
