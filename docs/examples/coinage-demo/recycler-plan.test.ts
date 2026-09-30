import { test } from "node:test";
import assert from "node:assert/strict";
import { validateConfig, type RecyclerConfig } from "./recycler-plan.js";

const base: RecyclerConfig = {
  durationSeconds: 96 * 3600,
  intervalSeconds: 90,
  denominations: [0, 1],
  maxHeldRaw: 1000000n,
  assetFloorRaw: 500000n,
  nativeBudgetRaw: 2500000000000n,
  nativeFloorRaw: 45000000000000n,
  maxUnloadFeeRaw: 500000000n,
  minHoldSeconds: 3600,
  maxUnloadsPerTx: 4,
  ringCeiling: 700,
};

test("a 96-hour plan is valid because capital is recycled, not accumulated", () => {
  assert.deepEqual(validateConfig(base, 10000n), {
    amounts: [10000n, 20000n],
    maxActions: 3840,
  });
});
test("the held-value cap must fit one voucher of every denomination", () => {
  assert.throws(
    () => validateConfig({ ...base, denominations: [8], maxHeldRaw: 2559999n }, 10000n),
    /cannot hold one 2560000/,
  );
});
test("unsafe duration, rate, denomination and bound inputs fail before connecting", () => {
  for (const durationSeconds of [0, 100 * 3600 + 1, NaN, 1.5])
    assert.throws(() => validateConfig({ ...base, durationSeconds }, 10000n), /duration/);
  for (const intervalSeconds of [0, 29, 3601, Infinity])
    assert.throws(() => validateConfig({ ...base, intervalSeconds }, 10000n), /interval/);
  assert.throws(() => validateConfig({ ...base, denominations: [] }, 10000n), /denominations/);
  assert.throws(() => validateConfig({ ...base, denominations: [1, 1] }, 10000n), /distinct/);
  assert.throws(() => validateConfig({ ...base, denominations: [128] }, 10000n), /denomination/);
  assert.throws(() => validateConfig({ ...base, denominations: [-5] }, 10000n), /precision/);
  assert.throws(() => validateConfig({ ...base, nativeBudgetRaw: 0n }, 10000n), /budget/);
  assert.throws(() => validateConfig({ ...base, assetFloorRaw: -1n }, 10000n), /negative/);
  assert.throws(() => validateConfig({ ...base, minHoldSeconds: 96 * 3600 }, 10000n), /min-hold/);
  assert.throws(() => validateConfig({ ...base, maxUnloadsPerTx: 0 }, 10000n), /unloads/);
  assert.throws(() => validateConfig({ ...base, ringCeiling: 0 }, 10000n), /ring-ceiling/);
});
