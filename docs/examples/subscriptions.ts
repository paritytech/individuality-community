/**
 * Operations example — subscribe a parachain to ring-root updates.
 *
 * Call: MembersNotifier.subscribe(...), wrapped in Sudo.sudo because it
 * requires a manager origin (backed by sudo on this chain).
 *
 *   pnpm run subscriptions                                  # people-lite -> para 1000
 *   COLLECTION=people SUBSCRIBER_PARA_ID=1500 pnpm run subscribe   # full people -> AH
 *
 * After this call the chain does the rest on its own: an offchain worker sends paged
 * initial ring roots to the subscriber over XCM, then keeps sending
 * updates as rings change.
 *
 * Tunables (env):
 *   COLLECTION            `people-lite` (default) or `people` (the full collection).
 *   SUBSCRIBER_PARA_ID    subscriber para id (default 1000; the soak uses 1500 for AH).
 *   SUBSCRIBER_PALLET_INDEX  members-subscriber pallet index on the subscriber (default 97).
 *   RING_EXPONENT         override the ring exponent (default depends on COLLECTION).
 */
import { Enum, FixedSizeBinary } from "polkadot-api";
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { sudoSubmitter } from "./lib/submit";
import { PEOPLE_IDENTIFIER, PEOPLE_RING_EXPONENT } from "./lib/constants";

// The companion Asset Hub's para id — `AssetHubParaId` in the People runtime
// (runtimes/next-people-paseo/src/lib.rs). Default 1000 keeps the original
// people-lite example; the soak overrides it to 1500.
const SUBSCRIBER_PARA_ID = Number(process.env.SUBSCRIBER_PARA_ID ?? 1000);
// Index of `members-subscriber` in the subscriber's construct_runtime
// (`MembersSubscriber: indiv_pallet_members_subscriber = 97`).
const SUBSCRIBER_PALLET_INDEX = Number(process.env.SUBSCRIBER_PALLET_INDEX ?? 97);

// Which collection to share. `people-lite` (2^10 rings) is the documented
// default; `people` is the full collection the soak drives (2^9 rings).
const COLLECTION = process.env.COLLECTION ?? "people-lite";
const { identifier, ringExponent } =
  COLLECTION === "people"
    ? { identifier: PEOPLE_IDENTIFIER, ringExponent: PEOPLE_RING_EXPONENT }
    : {
        identifier: FixedSizeBinary.fromText("pop:polkadot.network/people-lite"),
        ringExponent: Enum("R2e10"),
      };
const RING_EXPONENT = process.env.RING_EXPONENT ? Enum(process.env.RING_EXPONENT as any) : ringExponent;

async function main() {
  const { client, api } = connectPeople();
  const signer = devSigner(); // //Alice — the dev sudo key on a local chain
  const sudo = sudoSubmitter(api, signer, { customSignedExtensions });

  try {
    // Parachain already subscribed — nothing to do.
    const existing = await api.query.MembersNotifier.Subscribers.getValue(SUBSCRIBER_PARA_ID);
    if (existing) {
      console.log(`Para ${SUBSCRIBER_PARA_ID} is already subscribed.`);
      return;
    }

    // Governance subscribes the parachain.
    const subscribeTx = api.tx.MembersNotifier.subscribe({
      subscriber_parachain_id: SUBSCRIBER_PARA_ID,
      members_collections: [[identifier, RING_EXPONENT]],
      pallet_index: SUBSCRIBER_PALLET_INDEX,
    });
    console.log(`Subscribing para ${SUBSCRIBER_PARA_ID} to ${COLLECTION} ring-root updates ...`);
    const result = await sudo(subscribeTx.decodedCall, "MembersNotifier.subscribe");
    console.log(`Subscribed in block ${result.block.hash}`);

    // The initial state transfer is now pending — the offchain worker delivers
    // it page by page.
    const pending = await api.query.MembersNotifier.PendingInit.getValue(SUBSCRIBER_PARA_ID);
    if (pending) {
      console.log("Initial ring roots queued for delivery to the subscriber.");
    }
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
