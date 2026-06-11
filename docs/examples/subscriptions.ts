/**
 * Operations example — subscribe a parachain to ring-root updates.
 *
 * Call: MembersNotifier.subscribe(...), wrapped in Sudo.sudo because it
 * requires a manager origin (backed by sudo on this chain).
 *
 *   pnpm run subscriptions
 *
 * After this call the chain does the rest on its own: an offchain worker sends paged
 * initial ring roots to the subscriber over XCM, then keeps sending
 * updates as rings change.
 */
import { Enum, FixedSizeBinary } from "polkadot-api";
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { sudoSubmitter } from "./lib/submit";

// The companion Asset Hub's para id — `AssetHubParaId` in the People runtime
// (runtimes/next-people-paseo/src/lib.rs:1510).
const SUBSCRIBER_PARA_ID = 1000;
// Index of `members-subscriber` in the Asset Hub runtime's construct_runtime
// (`MembersSubscriber: indiv_pallet_members_subscriber = 97`,
// runtimes/next-asset-hub-paseo/src/lib.rs:2298).
const SUBSCRIBER_PALLET_INDEX = 97;

// The collection to share: people-lite, with its ring exponent as configured
// on this chain (2^10 rings).
const PEOPLE_LITE = FixedSizeBinary.fromText("pop:polkadot.network/people-lite");
const RING_EXPONENT = Enum("R2e10");

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
      members_collections: [[PEOPLE_LITE, RING_EXPONENT]],
      pallet_index: SUBSCRIBER_PALLET_INDEX,
    });
    console.log(`Subscribing para ${SUBSCRIBER_PARA_ID} to ring-root updates ...`);
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
