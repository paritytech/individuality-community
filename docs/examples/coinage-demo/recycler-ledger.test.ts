import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyResolution,
  decodeState,
  encodeState,
  eraBirth,
  newState,
  planAction,
  resolvePending,
  type FinalizedChain,
  type HeldVoucher,
  type LedgerState,
  type PendingOp,
} from "./recycler-ledger.js";
import type { RecyclerConfig } from "./recycler-plan.js";

const config: RecyclerConfig = {
  durationSeconds: 96 * 3600,
  intervalSeconds: 90,
  denominations: [0, 1],
  maxHeldRaw: 60000n,
  assetFloorRaw: 500000n,
  nativeBudgetRaw: 10n ** 12n,
  nativeFloorRaw: 10n ** 13n,
  maxUnloadFeeRaw: 5n * 10n ** 8n,
  minHoldSeconds: 3600,
  maxUnloadsPerTx: 4,
  ringCeiling: 700,
};
const amounts = [10000n, 20000n];
const rich = { liquidAsset: 1620000n, nativeFree: 5n * 10n ** 13n };
const fee = 5n * 10n ** 8n;
const hour = 3600000;

/** Canonical chain with blocks listed by number; `loaded` is finalized membership. */
function fakeChain(opts: {
  head: number;
  blocks?: Record<number, string[]>;
  failed?: string[];
  loaded?: boolean;
}): FinalizedChain & { reads: number[] } {
  const reads: number[] = [];
  return {
    reads,
    finalized: async () => ({ number: opts.head, hash: `0x${opts.head}` }),
    blockHash: async (n) => {
      if (n > opts.head) throw Error("not finalized");
      reads.push(n);
      return `0x${n}`;
    },
    extrinsicHashes: async (hash) => opts.blocks?.[Number(hash.slice(2))] ?? [],
    outcome: async (hash, index) => {
      const tx = opts.blocks![Number(hash.slice(2))][index];
      const ok = !opts.failed?.includes(tx);
      return { ok, fee: 7n, error: ok ? undefined : "Module" };
    },
    memberLoaded: async () => opts.loaded ?? false,
  };
}
function pending(overrides: Partial<PendingOp> = {}): PendingOp {
  return {
    id: 3,
    kind: "load",
    load: { member: "0xnew", denomination: 1, amount: 20000n },
    unloads: [],
    txHash: "0xtx",
    extrinsic: "0x00",
    nonce: 10,
    birth: { number: 100, hash: "0x100" },
    period: 64,
    signedAt: 0,
    scannedThrough: 100,
    ...overrides,
  };
}
const held = (member: string, loadedAt: number, amount = 20000n, ringIndex: number | undefined = 0): HeldVoucher => ({
  member,
  denomination: amount === 10000n ? 0 : 1,
  amount,
  loadedAt,
  block: { number: 1, hash: "0x1", index: 2 },
  ringIndex,
});
function withPending(op: PendingOp): LedgerState {
  return { ...newState(0, 96 * 3600), pending: op, nextOp: op.id + 1 };
}

test("a best-block inclusion that was reorganised away is dropped only after its era is finalized", async () => {
  // The transaction was seen in a non-canonical best block; canonical blocks never contain it.
  const op = pending();
  assert.deepEqual(await resolvePending(op, fakeChain({ head: 150 })), { type: "pending" });
  assert.equal(op.scannedThrough, 150);
  assert.deepEqual(await resolvePending(op, fakeChain({ head: 163 })), { type: "dropped", finalized: 163 });
});
test("scanning resumes after the last searched block instead of rereading the era", async () => {
  const op = pending({ scannedThrough: 140 });
  const chain = fakeChain({ head: 145 });
  await resolvePending(op, chain);
  assert.deepEqual(chain.reads, [141, 142, 143, 144, 145]);
});
test("a watch timeout does not lose a transaction that finalized", async () => {
  const op = pending();
  const r = await resolvePending(op, fakeChain({ head: 200, blocks: { 104: ["0xother", "0xa", "0xtx"] }, loaded: true }));
  assert.deepEqual(r, { type: "success", block: { number: 104, hash: "0x104", index: 2 }, fee: 7n });
});
test("a finalized dispatch failure is reported as failed, not retried as dropped", async () => {
  const r = await resolvePending(pending(), fakeChain({ head: 200, blocks: { 101: ["0xtx"] }, failed: ["0xtx"] }));
  assert.equal(r.type, "failed");
});
test("finalized membership that disagrees with the transaction search stops the run", async () => {
  // Key loaded, but not by this transaction: unknown accounting, so no retry.
  const missing = await resolvePending(pending(), fakeChain({ head: 200, loaded: true }));
  assert.equal(missing.type, "inconsistent");
  const unloaded = await resolvePending(pending(), fakeChain({ head: 200, blocks: { 101: ["0xtx"] }, loaded: false }));
  assert.equal(unloaded.type, "inconsistent");
});
test("era birth is recovered from any block inside the era", () => {
  assert.equal(eraBirth(7178363, 7178362 % 64, 64), 7178362);
  assert.equal(eraBirth(7178425, 7178362 % 64, 64), 7178362);
  assert.equal(eraBirth(7178426, 7178362 % 64, 64), 7178426);
});

test("applying a resolution twice counts it once", () => {
  const s0 = withPending(pending());
  const r = { type: "success" as const, block: { number: 104, hash: "0x104", index: 2 }, fee: 7n };
  const s1 = applyResolution(s0, 3, r, 1000);
  const s2 = applyResolution(s1, 3, r, 2000);
  assert.equal(s2, s1);
  assert.equal(s1.counters.loads, 1);
  assert.equal(s1.counters.assetLoaded, 20000n);
  assert.equal(s1.held.length, 1);
  assert.equal(s1.pending, undefined);
  assert.equal(s1.nextLoad, 1);
});
test("a stale resolution for an earlier operation is ignored", () => {
  const s0 = withPending(pending({ id: 4 }));
  assert.equal(applyResolution(s0, 3, { type: "dropped", finalized: 200 }, 0), s0);
});
test("a dropped recycle keeps its vouchers held and retries the same denomination", () => {
  const old = held("0xold", 0);
  const s0 = { ...withPending(pending({ kind: "recycle", unloads: [old] })), held: [old] };
  const s1 = applyResolution(s0, 3, { type: "dropped", finalized: 200 }, 0);
  assert.deepEqual(s1.held, [old]);
  assert.equal(s1.nextLoad, 0);
  assert.equal(s1.counters.dropped, 1);
  assert.equal(s1.consecutiveFailures, 1);
  assert.equal(s1.counters.assetLoaded, 0n);
});
test("a successful recycle replaces withdrawn vouchers and accounts both directions", () => {
  const old = held("0xold", 0), keep = held("0xkeep", 1);
  const s0 = { ...withPending(pending({ kind: "recycle", unloads: [old] })), held: [old, keep], consecutiveFailures: 2 };
  const s1 = applyResolution(s0, 3, { type: "success", block: { number: 104, hash: "0x104", index: 2 }, fee: 9n }, 5);
  assert.deepEqual(s1.held.map((v) => v.member), ["0xkeep", "0xnew"]);
  assert.equal(s1.counters.recycles, 1);
  assert.equal(s1.counters.assetWithdrawn, 20000n);
  assert.equal(s1.counters.nativeFees, 9n);
  assert.equal(s1.consecutiveFailures, 0);
});
test("state survives a checkpoint round trip including bigint amounts", () => {
  const s = { ...withPending(pending()), held: [held("0xa", 1)] };
  assert.deepEqual(decodeState(encodeState(s)), s);
});

test("no new transaction is planned while one is unresolved", () => {
  const s = withPending(pending());
  assert.deepEqual(planAction(s, config, amounts, rich, fee, 10 * hour, 6), { type: "reconcile" });
});
test("loads follow the denomination order and respect the interval", () => {
  const s = { ...newState(0, 96 * 3600), lastActionAt: 1000, nextLoad: 1 };
  assert.equal(planAction(s, config, amounts, rich, fee, 1000 + 89999, 6).type, "wait");
  assert.deepEqual(planAction(s, config, amounts, rich, fee, 1000 + 90000, 6), {
    type: "load",
    slot: 1,
    load: { denomination: 1, amount: 20000n },
  });
});
test("a denomination whose current ring reached the ceiling is skipped, and all full means wait", () => {
  const s = { ...newState(0, 96 * 3600), nextLoad: 0 };
  const a = planAction(s, config, amounts, rich, fee, hour, 6, new Set([0]));
  assert.equal(a.type === "load" && a.load.denomination, 1);
  assert.equal(planAction(s, config, amounts, rich, fee, hour, 6, new Set([0, 1])).type, "wait");
});
test("at the held cap the oldest confirmed voucher past its hold age is withdrawn with the next load", () => {
  const vouchers = [held("0xyoung", 2 * hour), held("0xold", 0), held("0xunconfirmed", 0, 20000n, undefined)];
  const s = { ...newState(0, 96 * 3600), held: vouchers, nextLoad: 1 };
  const a = planAction(s, config, amounts, rich, fee, 1.5 * hour, 6);
  assert.equal(a.type, "recycle");
  assert.deepEqual(a.type === "recycle" && a.unloads.map((v) => v.member), ["0xold"]);
});
test("vouchers younger than the hold age are not withdrawn early", () => {
  const s = { ...newState(0, 96 * 3600), held: [held("0xa", hour), held("0xb", hour), held("0xc", hour)] };
  const a = planAction(s, config, amounts, rich, fee, 1.5 * hour, 6);
  assert.equal(a.type, "wait");
  assert.equal(a.type === "wait" && a.until, 2 * hour);
});
test("the asset floor is never crossed", () => {
  const s = newState(0, 96 * 3600);
  const a = planAction(s, config, amounts, { ...rich, liquidAsset: 509999n }, fee, hour, 6);
  assert.equal(a.type, "stop");
  assert.equal(a.type === "stop" && a.reason, "budget");
});
test("native budget and floor stop the run before signing", () => {
  const spent = { ...newState(0, 96 * 3600), counters: { ...newState(0, 1).counters, nativeFees: 10n ** 12n - fee + 1n } };
  assert.equal(planAction(spent, config, amounts, rich, fee, hour, 6).type, "stop");
  const poor = newState(0, 96 * 3600);
  assert.equal(planAction(poor, config, amounts, { ...rich, nativeFree: 10n ** 13n + fee - 1n }, fee, hour, 6).type, "stop");
  assert.equal(planAction(poor, config, amounts, { ...rich, nativeFree: 10n ** 13n + fee }, fee, hour, 6).type, "load");
  // A recycle also needs the per-voucher unload fee.
  const full = { ...newState(0, 96 * 3600), held: [held("0xa", 0), held("0xb", 0), held("0xc", 0)], nextLoad: 1 };
  const a = planAction(full, config, amounts, { ...rich, nativeFree: 10n ** 13n + fee + config.maxUnloadFeeRaw - 1n }, fee, 2 * hour, 6);
  assert.equal(a.type, "stop");
});
test("repeated failures and the deadline stop the run", () => {
  const failing = { ...newState(0, 96 * 3600), consecutiveFailures: 6 };
  assert.equal(planAction(failing, config, amounts, rich, fee, 0, 6).type, "stop");
  const late = newState(0, 60);
  const a = planAction(late, config, amounts, rich, fee, 60000, 6);
  assert.equal(a.type === "stop" && a.reason, "deadline");
});
