import { test } from "node:test";
import assert from "node:assert/strict";
import { recyclerPlan } from "./recycler-plan.js";

test("two-hour plan fits eighty denomination-1 loads without exceeding 1.60 units", () => {
  assert.deepEqual(recyclerPlan(7200, 90, [1], 10000n, 1600000n), {
    maxLoads: 80,
    total: 1600000n,
    amounts: [20000n],
  });
  assert.throws(() => recyclerPlan(7200, 90, [1], 10000n, 1599999n), /budget/);
});
test("mixed denominations budget every scheduled load including the partial last interval", () => {
  assert.deepEqual(recyclerPlan(181, 90, [1, 5], 10000n, 360000n), {
    maxLoads: 3,
    total: 360000n,
    amounts: [20000n, 320000n],
  });
});
test("unsafe duration, rate and denomination inputs fail before connecting", () => {
  for (const duration of [0, 10801, NaN, 1.5])
    assert.throws(
      () => recyclerPlan(duration, 90, [1], 10000n, 1600000n),
      /duration/,
    );
  for (const interval of [0, 29, 3601, Infinity])
    assert.throws(
      () => recyclerPlan(7200, interval, [1], 10000n, 1600000n),
      /interval/,
    );
  assert.throws(() => recyclerPlan(1, 90, [], 10000n, 10000n), /denominations/);
  assert.throws(
    () => recyclerPlan(1, 90, [128], 10000n, 10000n),
    /denomination/,
  );
  assert.throws(() => recyclerPlan(1, 90, [-5], 10000n, 10000n), /precision/);
});
