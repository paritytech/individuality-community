import type { RecyclerConfig } from "./recycler-plan.js";

// A run has at most one unresolved transaction. Its outcome is decided only from
// canonical finalized blocks, so a best-block inclusion that is later reorganised
// away, a dropped subscription or a timeout never changes the accounting.

export interface BlockRef {
  number: number;
  hash: string;
}
export interface IncludedRef extends BlockRef {
  index: number;
}
export interface Voucher {
  member: string;
  denomination: number;
  amount: bigint;
}
export interface HeldVoucher extends Voucher {
  /** Wall-clock time of the finalized load receipt, in milliseconds. */
  loadedAt: number;
  block: IncludedRef;
  /** Ring index once finalized state shows the key as included. */
  ringIndex?: number;
}
export interface PendingOp {
  id: number;
  kind: "load" | "recycle";
  load: Voucher;
  unloads: HeldVoucher[];
  txHash: string;
  extrinsic: string;
  nonce: number;
  /** Finalized block the mortal era starts at. */
  birth: BlockRef;
  period: number;
  signedAt: number;
  /** Highest canonical block already searched for `txHash`. */
  scannedThrough: number;
}
export interface Counters {
  loads: number;
  recycles: number;
  dropped: number;
  failed: number;
  assetLoaded: bigint;
  assetWithdrawn: bigint;
  nativeFees: bigint;
}
export interface LedgerState {
  version: 1;
  startedAt: number;
  deadline: number;
  nextOp: number;
  /** Index into the configured denominations for the next load. */
  nextLoad: number;
  lastActionAt: number;
  held: HeldVoucher[];
  pending?: PendingOp;
  counters: Counters;
  consecutiveFailures: number;
}

export function newState(startedAt: number, durationSeconds: number) {
  return {
    version: 1,
    startedAt,
    deadline: startedAt + durationSeconds * 1000,
    nextOp: 0,
    nextLoad: 0,
    lastActionAt: 0,
    held: [],
    counters: {
      loads: 0,
      recycles: 0,
      dropped: 0,
      failed: 0,
      assetLoaded: 0n,
      assetWithdrawn: 0n,
      nativeFees: 0n,
    },
    consecutiveFailures: 0,
  } satisfies LedgerState as LedgerState;
}

const bigintKeys = new Set([
  "amount",
  "assetLoaded",
  "assetWithdrawn",
  "nativeFees",
]);
export const encodeState = (state: LedgerState) =>
  JSON.stringify(
    state,
    (_, v) => (typeof v === "bigint" ? v.toString() : v),
    2,
  ) + "\n";
export const decodeState = (text: string): LedgerState =>
  JSON.parse(text, (k, v) =>
    bigintKeys.has(k) && typeof v === "string" ? BigInt(v) : v,
  );

/** Last block number at which a mortal transaction can still be included. */
export const lastValidBlock = (op: Pick<PendingOp, "birth" | "period">) =>
  op.birth.number + op.period - 1;

/** Era start of a mortal transaction seen at block `seen`: the latest block not after it with the era's phase. */
export const eraBirth = (seen: number, phase: number, period: number) =>
  seen - ((((seen - phase) % period) + period) % period);

export type Resolution =
  | { type: "pending" }
  | { type: "success"; block: IncludedRef; fee: bigint }
  | { type: "failed"; block: IncludedRef; fee: bigint; error: unknown }
  | { type: "dropped"; finalized: number }
  | { type: "inconsistent"; reason: string };

/** Read-only view of canonical finalized chain state. */
export interface FinalizedChain {
  finalized(): Promise<BlockRef>;
  /** Canonical block hash at `number`; only called for finalized numbers. */
  blockHash(number: number): Promise<string>;
  extrinsicHashes(hash: string): Promise<string[]>;
  outcome(
    hash: string,
    index: number,
  ): Promise<{ ok: boolean; fee: bigint; error?: unknown }>;
  /** Whether finalized state records `member` in the denomination's recycler. */
  memberLoaded(
    denomination: number,
    member: string,
    at: string,
  ): Promise<boolean>;
}

/**
 * Searches finalized blocks for the pending transaction.
 *
 * A transaction absent from every canonical block of its era, once the whole era
 * is finalized, can never be included. The recycler membership of the new key
 * must agree with the result; disagreement stops the run.
 */
export async function resolvePending(
  op: PendingOp,
  chain: FinalizedChain,
  onProgress: (scannedThrough: number) => void = () => {},
): Promise<Resolution> {
  const head = await chain.finalized();
  const last = Math.min(head.number, lastValidBlock(op));
  let found: IncludedRef | undefined;
  for (let n = Math.max(op.scannedThrough + 1, op.birth.number); n <= last; n++) {
    const hash = await chain.blockHash(n);
    const index = (await chain.extrinsicHashes(hash)).indexOf(op.txHash);
    if (index >= 0) {
      found = { number: n, hash, index };
      break;
    }
    op.scannedThrough = n;
    onProgress(n);
  }
  const loaded = await chain.memberLoaded(
    op.load.denomination,
    op.load.member,
    head.hash,
  );
  if (found) {
    const result = await chain.outcome(found.hash, found.index);
    if (result.ok !== loaded)
      return {
        type: "inconsistent",
        reason: `${op.txHash} dispatch ok=${result.ok} but new member loaded=${loaded}`,
      };
    return result.ok
      ? { type: "success", block: found, fee: result.fee }
      : { type: "failed", block: found, fee: result.fee, error: result.error };
  }
  if (loaded)
    return {
      type: "inconsistent",
      reason: `${op.load.member} is loaded but ${op.txHash} is not in blocks ${op.birth.number}..${last}`,
    };
  if (head.number >= lastValidBlock(op))
    return { type: "dropped", finalized: head.number };
  return { type: "pending" };
}

/**
 * Applies a final resolution of `state.pending`. A resolution for another
 * operation, or one already applied, leaves the state unchanged.
 */
export function applyResolution(
  state: LedgerState,
  opId: number,
  resolution: Exclude<Resolution, { type: "pending" | "inconsistent" }>,
  now: number,
): LedgerState {
  const op = state.pending;
  if (!op || op.id !== opId) return state;
  const counters = { ...state.counters };
  let held = state.held;
  let consecutiveFailures = state.consecutiveFailures;
  if (resolution.type === "success") {
    const unloaded = new Set(op.unloads.map((v) => v.member));
    held = [
      ...held.filter((v) => !unloaded.has(v.member)),
      { ...op.load, loadedAt: now, block: resolution.block },
    ];
    counters.loads++;
    counters.assetLoaded += op.load.amount;
    if (op.kind === "recycle") {
      counters.recycles++;
      counters.assetWithdrawn += op.unloads.reduce((a, v) => a + v.amount, 0n);
    }
    counters.nativeFees += resolution.fee;
    consecutiveFailures = 0;
  } else if (resolution.type === "failed") {
    counters.failed++;
    counters.nativeFees += resolution.fee;
    consecutiveFailures++;
  } else {
    counters.dropped++;
    consecutiveFailures++;
  }
  return {
    ...state,
    held,
    pending: undefined,
    counters,
    consecutiveFailures,
    // A failed or dropped load is retried with a new key in the same denomination.
    nextLoad: resolution.type === "success" ? state.nextLoad + 1 : state.nextLoad,
  };
}

export type Action =
  | { type: "reconcile" }
  | { type: "wait"; until: number; reason: string }
  | { type: "stop"; reason: "deadline" | "budget" | "failures"; detail: string }
  | { type: "load"; slot: number; load: { denomination: number; amount: bigint } }
  | {
      type: "recycle";
      slot: number;
      load: { denomination: number; amount: bigint };
      unloads: HeldVoucher[];
    };

export interface Balances {
  liquidAsset: bigint;
  nativeFree: bigint;
}

/**
 * Chooses the next step. Loads while the held value and asset floor allow it;
 * otherwise withdraws the oldest confirmed vouchers old enough to release in the
 * same transaction as the next load. If the oldest inputs cannot fund it within
 * the input limit, the largest eligible vouchers are used instead.
 * `worstFee` bounds one transaction's native
 * cost excluding unload fees. Denominations in `full` are skipped: their current
 * ring has reached the configured ceiling.
 */
export function planAction(
  state: LedgerState,
  config: RecyclerConfig,
  amounts: bigint[],
  balances: Balances,
  worstFee: bigint,
  now: number,
  maxFailures: number,
  full: ReadonlySet<number> = new Set(),
): Action {
  if (state.pending) return { type: "reconcile" };
  if (now >= state.deadline)
    return { type: "stop", reason: "deadline", detail: "deadline reached" };
  if (state.consecutiveFailures >= maxFailures)
    return {
      type: "stop",
      reason: "failures",
      detail: `${state.consecutiveFailures} consecutive failed or dropped transactions`,
    };
  const next = state.lastActionAt + config.intervalSeconds * 1000;
  if (now < next) return { type: "wait", until: next, reason: "interval" };
  const count = config.denominations.length;
  const slot = Array.from({ length: count }, (_, k) => (state.nextLoad + k) % count).find(
    (i) => !full.has(config.denominations[i]),
  );
  if (slot === undefined)
    return { type: "wait", until: now + 600000, reason: "every configured ring is at the ceiling" };
  const load = { denomination: config.denominations[slot], amount: amounts[slot] };
  const heldValue = state.held.reduce((a, v) => a + v.amount, 0n);
  const nativeOk = (unloads: number) => {
    const cost = worstFee + BigInt(unloads) * config.maxUnloadFeeRaw;
    return (
      state.counters.nativeFees + cost <= config.nativeBudgetRaw &&
      balances.nativeFree - cost >= config.nativeFloorRaw
    );
  };
  const fits = (released: bigint) =>
    heldValue - released + load.amount <= config.maxHeldRaw &&
    balances.liquidAsset + released - load.amount >= config.assetFloorRaw;
  if (fits(0n)) {
    if (!nativeOk(0))
      return { type: "stop", reason: "budget", detail: "native fee budget or floor" };
    return { type: "load", slot, load };
  }
  const eligible = state.held
    .filter(
      (v) =>
        v.ringIndex !== undefined &&
        v.loadedAt + config.minHoldSeconds * 1000 <= now,
    )
    .sort((a, b) => a.loadedAt - b.loadedAt);
  let unloads: HeldVoucher[] = [];
  let released = 0n;
  for (const v of eligible) {
    if (fits(released) || unloads.length >= config.maxUnloadsPerTx) break;
    unloads.push(v);
    released += v.amount;
  }
  if (!fits(released)) {
    // Larger vouchers can fund the load when the oldest ones exceed the input limit.
    unloads = [];
    released = 0n;
    const byValue = [...eligible].sort((a, b) =>
      a.amount === b.amount ? a.loadedAt - b.loadedAt : a.amount > b.amount ? -1 : 1,
    );
    for (const v of byValue) {
      if (fits(released) || unloads.length >= config.maxUnloadsPerTx) break;
      unloads.push(v);
      released += v.amount;
    }
  }
  if (unloads.length && fits(released)) {
    if (!nativeOk(unloads.length))
      return { type: "stop", reason: "budget", detail: "native fee budget or floor" };
    return { type: "recycle", slot, load, unloads };
  }
  const waiting = state.held.filter((v) => !eligible.includes(v));
  if (!waiting.length)
    return {
      type: "stop",
      reason: "budget",
      detail: `cannot fund a ${load.amount} raw load: liquid ${balances.liquidAsset}, floor ${config.assetFloorRaw}, held ${heldValue}, max held ${config.maxHeldRaw}`,
    };
  const soonest = Math.min(
    ...waiting.map((v) => v.loadedAt + config.minHoldSeconds * 1000),
  );
  return {
    type: "wait",
    until: Math.max(soonest, now + 15000),
    reason: "capital held until vouchers reach the minimum hold age",
  };
}
