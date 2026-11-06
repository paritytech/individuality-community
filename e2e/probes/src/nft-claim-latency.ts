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

/**
 * Measures how long an NFT claim takes on a live People + Asset Hub pair.
 *
 * Read-only. Every figure is a sample set printed as a table with the sample count, minimum,
 * median, average and maximum. A count or unit shared by every row of a table moves into the
 * table's heading. Four sections:
 *
 * 1. State summary: delivered trees, sequence gaps, registered collections, minted instances.
 * 2. Delivery latency of the newest trees, from the award on People to the tree being stored on
 *    Asset Hub. The stages on People chain are counted in blocks. The transfer to
 *    Asset Hub is counted in relay-chain blocks. The chains' own timestamps are not
 *    comparable: each stamps on a 12-second grid, and the two grids are offset from each other.
 * 3. What a wallet waits for in the claim flow: the two People chain runtime API calls that produce
 *    the proof and the Asset Hub preview of the item, each repeated `ROUNDS` times per claimant.
 * 4. Block interval and finality lag of both chains, sampled over `SAMPLE_SECONDS`.
 *
 * Defaults to Paseo Next v2. `PEOPLE_RPC` and `ASSET_HUB_RPC` point it elsewhere.
 * `TREES`, `CLAIMANTS`, `ROUNDS` and `SAMPLE_SECONDS` size the samples. `VERBOSE=1` prints
 * every sample under its table.
 */

import { createClient, type PolkadotClient } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";

const PEOPLE_RPC = process.env.PEOPLE_RPC ?? "wss://paseo-people-next-system-rpc.polkadot.io";
const ASSET_HUB_RPC = process.env.ASSET_HUB_RPC ?? "wss://paseo-asset-hub-next-rpc.polkadot.io";
const TREES = Number(process.env.TREES ?? 8);
const CLAIMANTS = Number(process.env.CLAIMANTS ?? 3);
const ROUNDS = Number(process.env.ROUNDS ?? 5);
const SAMPLE_SECONDS = Number(process.env.SAMPLE_SECONDS ?? 60);
const VERBOSE = process.env.VERBOSE === "1";

/** Seconds per People block, exact for the stages that stay on that chain. */
const PEOPLE_BLOCK_SECONDS = 2;
/** Seconds per relay-chain block, the resolution of the cross-chain stage of section 2. */
const RELAY_BLOCK_SECONDS = 6;
/** How many blocks after the tree block the `CreditTreesSent` event is looked for. */
const SENT_SCAN_BLOCKS = 60;
/** How many Asset Hub blocks after the send time the stored tree is looked for. */
const RECEIVED_SCAN_BLOCKS = 300;
/** Asset Hub stamps the block before a People send up to this far back, on the 12-second grid. */
const RECEIVED_SCAN_SLACK_MS = 12_000;
/** How often the block heads are polled in section 4. */
const HEAD_POLL_MS = 250;

interface CreditTree {
  game_index: number;
  root: string;
  leaf_count: number;
  timestamp: number;
}

interface Claimant {
  type: "Account" | "Person";
  value: string;
}

interface ChainEvent {
  event: { type: string; value: { type: string; value: Record<string, unknown> } };
}

interface StorageEntry<V> {
  keyArgs: unknown[];
  value: V;
}

interface StorageItem<V> {
  getValue(...args: unknown[]): Promise<V>;
  getEntries(): Promise<StorageEntry<V>[]>;
}

/** The subset of PAPI's unsafe API this probe reads, named per pallet item. */
interface UnsafeApi {
  query: Record<string, Record<string, StorageItem<unknown>>>;
  apis: Record<string, Record<string, (...args: unknown[]) => Promise<unknown>>>;
}

interface RuntimeResult<T> {
  success: boolean;
  value: T;
}

interface CreditProof {
  credit: string;
  leaf_index: number;
  proof: string[];
}

/** A named set of samples in one unit, summarised as a table row. */
class Samples {
  readonly values: number[] = [];

  constructor(
    readonly name: string,
    readonly unit: string,
  ) {}

  add(value: number) {
    this.values.push(value);
  }

  /** The row's cells in `COLUMNS` order. */
  cells(): Record<Column, string> {
    const sorted = [...this.values].sort((a, b) => a - b);
    const n = sorted.length;
    const fmt = (v: number | undefined) =>
      v === undefined ? "n/a" : Number.isInteger(v) ? String(v) : v.toFixed(1);
    const median = n % 2 === 1 ? sorted[(n - 1) / 2] : ((sorted[n / 2 - 1] ?? 0) + (sorted[n / 2] ?? 0)) / 2;
    const average = n === 0 ? undefined : sorted.reduce((sum, v) => sum + v, 0) / n;
    return {
      action: this.name,
      n: String(n),
      min: fmt(sorted[0]),
      median: n === 0 ? "n/a" : fmt(median),
      avg: fmt(average),
      max: fmt(sorted[n - 1]),
      unit: this.unit,
    };
  }
}

type Column = "action" | "n" | "min" | "median" | "avg" | "max" | "unit";
const COLUMNS: Column[] = ["action", "n", "min", "median", "avg", "max", "unit"];

/** The value every set shares in `column`, or `null` when they differ and each row keeps its own. */
function shared(sets: Samples[], column: Column): string | null {
  const values = new Set(sets.map(s => s.cells()[column]));
  return values.size === 1 ? (sets[0]?.cells()[column] ?? null) : null;
}

/** Prints a heading, any notes, then the table. A count or unit shared by every row goes in the heading. */
function section(title: string, sets: Samples[], notes: string[] = []) {
  const n = shared(sets, "n");
  const unit = shared(sets, "unit");
  const suffix = [n === null ? null : `n = ${n}`, unit].filter(s => s !== null).join(", ");
  console.log(`\n## ${title}${suffix ? ` (${suffix})` : ""}`);
  for (const note of notes) console.log(`  ${note}`);

  const columns = COLUMNS.filter(c => !((c === "n" && n !== null) || (c === "unit" && unit !== null)));
  const rows = [columns.map(String), ...sets.map(s => columns.map(c => s.cells()[c]))];
  const widths = columns.map((_, i) => Math.max(...rows.map(r => r[i]?.length ?? 0)));
  const line = (r: string[]) =>
    `  | ${r.map((c, i) => (i === 0 ? c.padEnd(widths[i] ?? 0) : c.padStart(widths[i] ?? 0))).join(" | ")} |`;
  console.log(line(rows[0] ?? []));
  console.log(`  |${widths.map(w => "-".repeat(w + 2)).join("|")}|`);
  for (const r of rows.slice(1)) console.log(line(r));
  if (VERBOSE) for (const s of sets) console.log(`  ${s.name}: ${s.values.join(", ")}`);
}

class Chain {
  readonly client: PolkadotClient;
  readonly api: UnsafeApi;

  constructor(
    readonly name: string,
    rpc: string,
  ) {
    this.client = createClient(getWsProvider(rpc));
    this.api = this.client.getUnsafeApi() as unknown as UnsafeApi;
  }

  query<V>(pallet: string, item: string): StorageItem<V> {
    const entry = this.api.query[pallet]?.[item];
    if (!entry) throw new Error(`${this.name}: no storage item ${pallet}.${item}`);
    return entry as StorageItem<V>;
  }

  runtimeApi<R>(trait: string, method: string): (...args: unknown[]) => Promise<R> {
    const entry = this.api.apis[trait]?.[method];
    if (!entry) throw new Error(`${this.name}: no runtime API ${trait}.${method}`);
    return entry as (...args: unknown[]) => Promise<R>;
  }

  hashAt(block: number): Promise<string | null> {
    return this.client._request<string | null>("chain_getBlockHash", [block]);
  }

  async headNumber(finalized: boolean): Promise<number> {
    const params = finalized ? [await this.client._request<string>("chain_getFinalizedHead", [])] : [];
    const header = await this.client._request<{ number: string }>("chain_getHeader", params);
    return Number.parseInt(header.number, 16);
  }

  /** Unix seconds the block was stamped with. */
  async timestampAt(block: number): Promise<number> {
    const hash = await this.hashAt(block);
    if (!hash) throw new Error(`${this.name}: no block ${block}`);
    const millis = await this.query<bigint>("Timestamp", "Now").getValue({ at: hash });
    return Number(millis) / 1000;
  }

  /** The relay-chain block the parachain block was built on. */
  async relayBlockAt(block: number): Promise<number> {
    const hash = await this.hashAt(block);
    if (!hash) throw new Error(`${this.name}: no block ${block}`);
    return this.query<number>("ParachainSystem", "LastRelayChainBlockNumber").getValue({ at: hash });
  }

  async eventsAt(block: number): Promise<ChainEvent[]> {
    const hash = await this.hashAt(block);
    if (!hash) return [];
    return this.query<ChainEvent[]>("System", "Events").getValue({ at: hash });
  }

  /** The first block stamped at or after `seconds`, by binary search over block timestamps. */
  async blockAtOrAfter(seconds: number): Promise<number> {
    let low = 1;
    let high = await this.headNumber(true);
    while (low < high) {
      const mid = Math.floor((low + high) / 2);
      if ((await this.timestampAt(mid)) < seconds) low = mid + 1;
      else high = mid;
    }
    return low;
  }

  /**
   * The first block in `[from, from + span]` at which `holds` is true, by binary search, or
   * `null` when it is still false at the end of the range. `holds` must be monotonic over the
   * range, as a storage entry that is written once is.
   */
  async firstBlockWhere(
    from: number,
    span: number,
    holds: (block: number) => Promise<boolean>,
  ): Promise<number | null> {
    let low = from;
    let high = from + span;
    if (!(await holds(high))) return null;
    while (low < high) {
      const mid = Math.floor((low + high) / 2);
      if (await holds(mid)) high = mid;
      else low = mid + 1;
    }
    return low;
  }

  /** The first block in `[from, from + span]` whose events include `pallet.name` passing `where`. */
  async findEvent(
    from: number,
    span: number,
    pallet: string,
    name: string,
    where: (value: Record<string, unknown>) => boolean = () => true,
  ): Promise<number | null> {
    for (let block = from; block <= from + span; block++) {
      const events = await this.eventsAt(block);
      const hit = events.some(
        e => e.event.type === pallet && e.event.value.type === name && where(e.event.value.value),
      );
      if (hit) return block;
    }
    return null;
  }
}

function describe(claimant: Claimant): string {
  return `${claimant.type}(${claimant.value.slice(0, 8)}…)`;
}

async function timed<T>(into: Samples, run: () => Promise<T>): Promise<T> {
  const started = performance.now();
  const result = await run();
  into.add(Math.round(performance.now() - started));
  return result;
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function stateSummary(people: Chain, assetHub: Chain): Promise<StorageEntry<CreditTree>[]> {
  console.log("## State");
  const roots = await people.query<CreditTree>("NftCredits", "NftClaimCreditRoots").getEntries();
  const nextSequence = await people.query<bigint>("NftCredits", "NextCreditTreeSequence").getValue();
  const queue = await people.query<unknown[]>("NftCredits", "CreditTreeDeliveryQueue").getValue();
  console.log(`  people: ${roots.length} roots held, next sequence ${nextSequence}, ${queue.length} queued`);

  const trees = await assetHub.query<CreditTree>("NftClaims", "CreditTrees").getEntries();
  const expected = await assetHub.query<bigint>("NftClaims", "NextExpectedSequence").getValue();
  const minters = await assetHub
    .query<{ selection: { type: string } }>("NftClaims", "CollectionMinters")
    .getEntries();
  const claimed = await assetHub.query<unknown>("NftClaims", "ClaimedLeaves").getEntries();
  const instances = await assetHub.query<bigint>("Scarcity", "NextInstanceId").getValue();
  const kinds = minters.map(m => `${m.keyArgs[0]}:${m.value.selection.type}`).join(" ");
  console.log(
    `  asset hub: ${trees.length} trees held, next expected sequence ${expected}, ` +
      `${claimed.length} trees with claims, ${instances} instances minted`,
  );
  console.log(`  registered collections: ${kinds || "none"}`);
  return roots;
}

async function deliveryLatency(people: Chain, assetHub: Chain, roots: StorageEntry<CreditTree>[]) {
  const stages = {
    awardToRoot: new Samples("award → root recorded on People", "s"),
    rootToSent: new Samples("root → sent over XCM", "s"),
    sentToReceived: new Samples("sent → stored on Asset Hub", "s"),
    awardToClaimable: new Samples("award → claimable (total)", "s"),
  };
  const notes: string[] = [];
  const relaySeconds = (relayBlocks: number) => relayBlocks * RELAY_BLOCK_SECONDS;
  const peopleSeconds = (blocks: number) => blocks * PEOPLE_BLOCK_SECONDS;
  const byBlock = new Map(roots.map(r => [r.keyArgs[0] as number, r.value]));
  const blocks = [...byBlock.keys()].sort((a, b) => a - b).slice(-TREES);
  const storedTree = assetHub.query<CreditTree | undefined>("NftClaims", "CreditTrees");
  for (const treeBlock of blocks) {
    if (!byBlock.has(treeBlock)) continue;
    // The root is built in `on_initialize` of the block after the awards, so this is one block.
    const rootBlock = treeBlock + 1;
    stages.awardToRoot.add(peopleSeconds(rootBlock - treeBlock));
    const sentBlock = await people.findEvent(
      treeBlock + 1,
      SENT_SCAN_BLOCKS,
      "NftCredits",
      "CreditTreesSent",
      value => Array.isArray(value.trees) && value.trees.includes(treeBlock),
    );
    if (sentBlock === null) {
      notes.push(`tree ${treeBlock}: no CreditTreesSent within ${SENT_SCAN_BLOCKS} blocks, skipped`);
      continue;
    }
    // The offchain worker runs after its block is imported, so the send lands a few blocks later.
    stages.rootToSent.add(peopleSeconds(sentBlock - rootBlock));
    const sentRelay = await people.relayBlockAt(sentBlock);
    // The receipt event does not name its trees, so the arrival is the first Asset Hub block
    // whose state holds this tree.
    const from = await assetHub.blockAtOrAfter(
      (await people.timestampAt(sentBlock)) - RECEIVED_SCAN_SLACK_MS / 1000,
    );
    const storedBlock = await assetHub.firstBlockWhere(from, RECEIVED_SCAN_BLOCKS, async block => {
      const hash = await assetHub.hashAt(block);
      return hash !== null && (await storedTree.getValue(treeBlock, { at: hash })) !== undefined;
    });
    if (storedBlock === null) {
      notes.push(`tree ${treeBlock}: not stored on Asset Hub within ${RECEIVED_SCAN_BLOCKS} blocks, skipped`);
      continue;
    }
    const storedRelay = await assetHub.relayBlockAt(storedBlock);
    const hop = relaySeconds(storedRelay - sentRelay);
    stages.sentToReceived.add(hop);
    stages.awardToClaimable.add(peopleSeconds(sentBlock - treeBlock) + hop);
  }
  section(
    `Delivery latency of the ${TREES} newest trees, People blocks at ${PEOPLE_BLOCK_SECONDS} s, cross-chain hop at ${RELAY_BLOCK_SECONDS} s relay resolution`,
    Object.values(stages),
    notes,
  );
}

async function claimFlow(people: Chain, assetHub: Chain): Promise<Samples[]> {
  const calls = {
    roots: new Samples("fetch roots: nft_claim_credit_roots (People RPC)", "ms"),
    proofs: new Samples("fetch proofs: nft_claim_credit_proofs (People RPC)", "ms"),
    preview: new Samples("preview item: preview_mints (Asset Hub RPC)", "ms"),
  };
  console.log(`\n## Claim flow samples: ${CLAIMANTS} claimants × ${ROUNDS} rounds, wall clock`);
  const index = await people.query<number[]>("NftCredits", "NftClaimCreditBlocks").getEntries();
  // Newest credits first, so the sample is served from retained awards rather than pruned ones.
  const claimants = index
    .filter(e => e.value.length > 0)
    .sort((a, b) => (b.value.at(-1) ?? 0) - (a.value.at(-1) ?? 0))
    .slice(0, CLAIMANTS)
    .map(e => e.keyArgs[0] as Claimant);
  const minters = await assetHub
    .query<{ selection: { type: string } }>("NftClaims", "CollectionMinters")
    .getEntries();
  const randomCollection = minters.find(m => m.value.selection.type === "Random")?.keyArgs[0];

  const roots = people.runtimeApi<[number, CreditTree][]>("NftCreditsApi", "nft_claim_credit_roots");
  const proofs = people.runtimeApi<RuntimeResult<CreditProof[] | { type: string }>>(
    "NftCreditsApi",
    "nft_claim_credit_proofs",
  );
  const preview = assetHub.runtimeApi<unknown[]>("NftClaimsApi", "preview_mints");

  // PAPI answers a repeated runtime call from its per-block cache, so each round is pinned to a
  // different recent finalized block and every call goes to the node.
  const finalized = await people.headNumber(true);
  const ahFinalized = await assetHub.headNumber(true);
  for (const claimant of claimants) {
    const outcome: string[] = [];
    for (let round = 0; round < ROUNDS; round++) {
      const at = { at: await people.hashAt(finalized - round) };
      const ahAt = { at: await assetHub.hashAt(ahFinalized - round) };
      const held = await timed(calls.roots, () => roots(claimant, at));
      const latest = held.at(-1);
      if (!latest) {
        outcome.push("holds no root");
        break;
      }
      const [block] = latest;
      const result = await timed(calls.proofs, () => proofs(block, claimant, at));
      if (!result.success || !Array.isArray(result.value)) {
        outcome.push(`proofs for block ${block} unavailable: ${JSON.stringify(result.value)}`);
        break;
      }
      const first = result.value[0];
      if (round === 0)
        outcome.push(
          `${result.value.length} proofs of ${first?.proof.length ?? 0} sibling hashes in block ${block}`,
        );
      if (first && randomCollection !== undefined) {
        await timed(calls.preview, () =>
          preview([{ credit: first.credit, collection: randomCollection }], ahAt),
        );
      }
    }
    console.log(`  ${describe(claimant)}: ${outcome.join("; ") || "ok"}`);
  }
  return Object.values(calls);
}

interface HeadSamples {
  interval: Samples;
  lagBlocks: Samples;
}

/**
 * Block interval and finality lag, sampled by polling the heads.
 */
async function blockRate(chains: Chain[]): Promise<HeadSamples[]> {
  const sets = chains.map(c => ({
    interval: new Samples(`${c.name} block interval`, "s"),
    lagBlocks: new Samples(`${c.name} finality lag`, "blocks"),
  }));
  const lastBest = chains.map(() => -1);
  const lastSeen = chains.map(() => 0);
  const deadline = Date.now() + SAMPLE_SECONDS * 1000;
  while (Date.now() < deadline) {
    await Promise.all(
      chains.map(async (chain, i) => {
        const best = await chain.headNumber(false);
        const now = Date.now();
        if (best > lastBest[i]!) {
          // The first sighting only starts the clock; a change since then is one interval per block.
          if (lastBest[i]! >= 0)
            sets[i]!.interval.add(Number(((now - lastSeen[i]!) / 1000 / (best - lastBest[i]!)).toFixed(2)));
          lastBest[i] = best;
          lastSeen[i] = now;
          sets[i]!.lagBlocks.add(best - (await chain.headNumber(true)));
        }
      }),
    );
    await sleep(HEAD_POLL_MS);
  }
  console.log(`\n## Block timing over ${SAMPLE_SECONDS} seconds, wall clock`);
  chains.forEach((chain, i) => {
    const interval = sets[i]!.interval.cells();
    const lag = sets[i]!.lagBlocks.cells();
    console.log(
      `  ${chain.name}: block interval configured 2 s, observed median ${interval.median} s over ${interval.n} blocks; ` +
        `finality lag observed median ${lag.median} blocks (${lag.min} to ${lag.max}) over ${lag.n} samples`,
    );
  });
  return sets;
}

/** The user-facing wait, one action per row. Inclusion and finality are derived from block timing. */
function userWait(calls: Samples[], heads: HeadSamples[]) {
  const assetHub = heads[1];
  if (!assetHub) return;
  const avg = (s: Samples) => s.values.reduce((a, b) => a + b, 0) / Math.max(1, s.values.length);
  const interval = avg(assetHub.interval);
  const inclusion = new Samples("submit claim → included in a block (0 to 1 block)", "s");
  for (const v of assetHub.interval.values) {
    inclusion.add(0);
    inclusion.add(v);
  }
  const finality = new Samples("included → finalized (lag × interval)", "s");
  for (const lag of assetHub.lagBlocks.values) finality.add(Number((lag * interval).toFixed(1)));
  const rows = [...calls, inclusion, finality];
  const medianSeconds = rows.reduce((sum, r) => {
    const median = Number(r.cells().median);
    return sum + (Number.isNaN(median) ? 0 : r.unit === "ms" ? median / 1000 : median);
  }, 0);
  section("What a wallet user waits for, per claim", rows, [
    `Sum of medians: ${medianSeconds.toFixed(1)} s to finalized, ${(medianSeconds - Number(finality.cells().median)).toFixed(1)} s to included.`,
    "Inclusion assumes the transaction reaches a collator within one block; it is not measured by submitting.",
  ]);
}

async function main() {
  const people = new Chain("people", PEOPLE_RPC);
  const assetHub = new Chain("asset hub", ASSET_HUB_RPC);
  try {
    console.log(`people: ${PEOPLE_RPC}\nasset hub: ${ASSET_HUB_RPC}`);
    const roots = await stateSummary(people, assetHub);
    await deliveryLatency(people, assetHub, roots);
    const calls = await claimFlow(people, assetHub);
    const heads = await blockRate([people, assetHub]);
    userWait(calls, heads);
  } finally {
    people.client.destroy();
    assetHub.client.destroy();
  }
}

// The WebSocket providers keep the event loop alive after `destroy()`, so the process is ended
// explicitly once the report is complete.
main().then(
  () => process.exit(0),
  error => {
    console.error(error);
    process.exit(1);
  },
);
