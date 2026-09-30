import { assetAmount } from "./protocol.js";

export function recyclerPlan(
  durationSeconds: number,
  intervalSeconds: number,
  denominations: number[],
  unit: bigint,
  budget: bigint,
) {
  if (
    !Number.isSafeInteger(durationSeconds) ||
    durationSeconds < 1 ||
    durationSeconds > 10800
  )
    throw Error("duration-seconds must be in [1, 10800]");
  if (
    !Number.isSafeInteger(intervalSeconds) ||
    intervalSeconds < 30 ||
    intervalSeconds > 3600
  )
    throw Error("interval-seconds must be in [30, 3600]");
  if (!denominations.length || denominations.length > 32)
    throw Error("Choose between 1 and 32 denominations");
  const amounts = denominations.map((d) => assetAmount(unit, d));
  if (amounts.some((a) => a <= 0n) || budget <= 0n)
    throw Error("Amounts and budget must be positive");
  const maxLoads = Math.ceil(durationSeconds / intervalSeconds);
  const total = Array.from(
    { length: maxLoads },
    (_, i) => amounts[i % amounts.length],
  ).reduce((a, b) => a + b, 0n);
  if (total > budget)
    throw Error(`Plan needs ${total} raw asset units; budget is ${budget}`);
  return { maxLoads, total, amounts };
}
