import { setTimeout as delay } from "node:timers/promises";
import { log } from "./log.ts";

const ATTEMPTS = 30;
const INTERVAL_MS = 6_000;

/** Poll until a condition is true. Cross-chain calls finalize before their destination effects
 * become observable. */
export async function waitFor(label: string, ready: () => Promise<boolean>): Promise<void> {
  log(label, "waiting");
  for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
    if (await ready()) {
      log(label, "ready");
      return;
    }
    await delay(INTERVAL_MS);
  }
  throw new Error(`${label}: not ready after ${(ATTEMPTS * INTERVAL_MS) / 60_000} minutes`);
}
