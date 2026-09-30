import { assetAmount } from "./protocol.js";

/** Longest supported run: 96 hours plus four hours of slack for a late start. */
export const MAX_DURATION_SECONDS = 100 * 3600;

export interface RecyclerConfig {
  durationSeconds: number;
  intervalSeconds: number;
  denominations: number[];
  /** Largest value, in raw asset units, held in vouchers at once. */
  maxHeldRaw: bigint;
  /** Liquid backing asset that the bot never loads. */
  assetFloorRaw: bigint;
  /** Largest total of native fees, in raw units, this run may pay. */
  nativeBudgetRaw: bigint;
  /** Free native balance that the bot never spends below. */
  nativeFloorRaw: bigint;
  /** Largest native unload fee accepted per withdrawn voucher. */
  maxUnloadFeeRaw: bigint;
  /** Youngest voucher age, in seconds, that the bot withdraws. */
  minHoldSeconds: number;
  /** Most vouchers withdrawn in one transaction. */
  maxUnloadsPerTx: number;
  /** Members in a denomination's current ring at which the bot stops loading it. */
  ringCeiling: number;
}

export function validateConfig(config: RecyclerConfig, unit: bigint) {
  const {
    durationSeconds,
    intervalSeconds,
    denominations,
    maxHeldRaw,
    minHoldSeconds,
    maxUnloadsPerTx,
  } = config;
  if (
    !Number.isSafeInteger(durationSeconds) ||
    durationSeconds < 1 ||
    durationSeconds > MAX_DURATION_SECONDS
  )
    throw Error(`duration-seconds must be in [1, ${MAX_DURATION_SECONDS}]`);
  if (
    !Number.isSafeInteger(intervalSeconds) ||
    intervalSeconds < 30 ||
    intervalSeconds > 3600
  )
    throw Error("interval-seconds must be in [30, 3600]");
  if (!denominations.length || denominations.length > 32)
    throw Error("Choose between 1 and 32 denominations");
  if (new Set(denominations).size !== denominations.length)
    throw Error("Denominations must be distinct");
  const amounts = denominations.map((d) => assetAmount(unit, d));
  if (amounts.some((a) => a <= 0n)) throw Error("Amounts must be positive");
  for (const key of [
    "assetFloorRaw",
    "nativeBudgetRaw",
    "nativeFloorRaw",
    "maxUnloadFeeRaw",
  ] as const)
    if (config[key] < 0n) throw Error(`${key} must not be negative`);
  if (config.nativeBudgetRaw === 0n)
    throw Error("Native fee budget must be positive");
  const largest = amounts.reduce((a, b) => (a > b ? a : b));
  if (maxHeldRaw < largest)
    throw Error(
      `max-held-raw ${maxHeldRaw} cannot hold one ${largest} raw voucher`,
    );
  if (
    !Number.isSafeInteger(minHoldSeconds) ||
    minHoldSeconds < 0 ||
    minHoldSeconds >= durationSeconds
  )
    throw Error("min-hold-seconds must be in [0, duration-seconds)");
  if (
    !Number.isSafeInteger(maxUnloadsPerTx) ||
    maxUnloadsPerTx < 1 ||
    maxUnloadsPerTx > 8
  )
    throw Error("max-unloads-per-tx must be in [1, 8]");
  if (
    !Number.isSafeInteger(config.ringCeiling) ||
    config.ringCeiling < 1 ||
    config.ringCeiling > 16127
  )
    throw Error("ring-ceiling must be in [1, 16127]");
  const maxActions = Math.ceil(durationSeconds / intervalSeconds);
  return { amounts, maxActions };
}
