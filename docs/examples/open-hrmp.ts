/**
 * Force-open the People <-> Asset Hub HRMP channels on the relay, then wait until
 * both parachains actually see them.
 *
 *   pnpm run hrmp
 *
 * We can't declare `[[hrmp_channels]]` in the zombienet spec: that preopens them
 * at genesis, which on this polkadot-sdk yields an inconsistent relay DMQ head
 * and panics the parachain ("DMQ head mismatch" → missing `set_validation_data`
 * inherent, polkadot-sdk#1616), so it never produces a block. Instead we open
 * both directions at runtime via relay sudo (`Hrmp.force_open_hrmp_channel`),
 * after genesis — which sidesteps that computation. Without an open channel the
 * members-notifier's init XCM never reaches Asset Hub and `PendingInit` stays
 * stuck (the subscriber never initializes).
 *
 * `force_open_hrmp_channel` takes effect at the next relay session, and the
 * parachains only learn about their egress channel a session or two after that
 * (via their relay-parent state). This script blocks until People sees its
 * egress channel to AH (and vice-versa) — so by the time it returns it is safe
 * to `pnpm run subscribe`. Subscribing earlier would make the first
 * `send_init_page` hit "No channel", no-op, and get banned by the tx pool
 * (it has no retry-window discriminator), wedging the init permanently.
 *
 * Idempotent: channels already open are skipped.
 */
import { connectRelay, connectPeople, connectAssetHub, devSigner } from "./lib/client";
import { sudoSubmitter } from "./lib/submit";
import { ASSET_HUB_PARA_ID } from "./lib/constants";

const PEOPLE_PARA_ID = 1502;
const MAX_CAPACITY = 8;
const MAX_MESSAGE_SIZE = 102_400;

const CHANNELS = [
  { sender: PEOPLE_PARA_ID, recipient: ASSET_HUB_PARA_ID },
  { sender: ASSET_HUB_PARA_ID, recipient: PEOPLE_PARA_ID },
];

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** True once `chain`'s relevant messaging state lists an egress channel to `recipient`. */
async function seesEgressTo(api: any, recipient: number): Promise<boolean> {
  const rms = await api.query.ParachainSystem.RelevantMessagingState.getValue();
  const channels: Array<[number, unknown]> = rms?.egress_channels ?? [];
  return channels.some(([id]) => id === recipient);
}

async function main() {
  const relay = connectRelay();
  const signer = devSigner(); // //Alice — relay dev sudo key
  // The relay uses the untyped (unsafe) API — see `connectRelay` — so we cast to
  // satisfy `sudoSubmitter`'s structural `SudoApi`. The relay uses stock signed
  // extensions, so no custom overrides are needed.
  const relayApi = relay.api as any;
  const sudo = sudoSubmitter(relayApi, signer);

  try {
    for (const { sender, recipient } of CHANNELS) {
      const existing = await relayApi.query.Hrmp.HrmpChannels.getValue({ sender, recipient });
      if (existing !== undefined) {
        console.log(`HRMP ${sender} -> ${recipient}: already open, skipping.`);
        continue;
      }
      const call = relayApi.tx.Hrmp.force_open_hrmp_channel({
        sender,
        recipient,
        max_capacity: MAX_CAPACITY,
        max_message_size: MAX_MESSAGE_SIZE,
      });
      await sudo(call.decodedCall, `force_open_hrmp_channel ${sender} -> ${recipient}`);
    }
  } finally {
    relay.client.destroy();
  }

  // Wait until both parachains actually see their egress channels — this is what
  // makes it safe to subscribe right after.
  const people = connectPeople();
  const ah = connectAssetHub();
  try {
    console.log("Waiting for the channels to become visible to both parachains (a few sessions) ...");
    for (let i = 0; i < 60; i++) {
      const peopleReady = await seesEgressTo(people.api, ASSET_HUB_PARA_ID);
      const ahReady = await seesEgressTo(ah.api, PEOPLE_PARA_ID);
      if (peopleReady && ahReady) {
        console.log("HRMP channels visible on both sides. Safe to `pnpm run subscribe`.");
        return;
      }
      if (i % 5 === 0) {
        console.log(`  waiting… People->AH visible=${peopleReady}, AH->People visible=${ahReady}`);
      }
      await sleep(6000);
    }
    throw new Error("Timed out waiting for HRMP egress channels to become visible (~6 min).");
  } finally {
    people.client.destroy();
    ah.client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
