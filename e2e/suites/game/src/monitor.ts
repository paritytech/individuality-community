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
import type { PeopleApi, PeopleClient } from "./chain.ts";
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

  const record = async (block: { number: number; hash: string }) => {
    const at = { at: block.hash };
    const [game, gameIndex, schedules, events, millis] = await Promise.all([
      api.query.Game.Game.getValue(at),
      api.query.Game.GameIndex.getValue(at),
      api.query.Game.GameSchedules.getValue(at),
      api.query.System.Events.getValue(at),
      api.query.Timestamp.Now.getValue(at),
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
    };
    records.push(entry);

    const previous = records[records.length - 2];
    if (previous === undefined || previous.state !== entry.state || entry.events.length > 0) {
      const tail = entry.events.length > 0 ? `  ${entry.events.join(" | ")}` : "";
      console.log(`  #${entry.number} ${entry.state} [schedules=${entry.scheduled}]${tail}`);
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
    records
      .map(
        entry =>
          `  #${entry.number} t=${entry.chainTime} ${entry.state} [schedules=${entry.scheduled}]` +
          (entry.events.length > 0 ? ` ${entry.events.join(" | ")}` : ""),
      )
      .join("\n");

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
