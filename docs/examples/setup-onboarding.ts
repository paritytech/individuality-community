/**
 * Prepare the people collection for ring-root delivery:
 *
 *   1. Create the collection if it doesn't exist yet. On a fresh local chain it
 *      isn't created at genesis; `People.create_people_collection` is a
 *      permissionless `#[pallet::authorize]` call (Authorized origin — not
 *      sudo-able), so we submit it as a v5 general transaction.
 *   2. Optionally shrink its onboarding (cohort) size for soak-style churn. This
 *      is only done when `ONBOARDING_SIZE` is set explicitly.
 *
 *   pnpm run onboarding              # create the collection if needed; leave size unchanged
 *   ONBOARDING_SIZE=2 pnpm run onboarding
 *
 * A small cohort means each handful of registrations triggers a ring build and
 * therefore a new ring root, keeping `enqueue_updates` busy block after block.
 */
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { signedSubmitter, sudoSubmitter } from "./lib/submit";
import { createGeneralSigner } from "./lib/general-signer";
import { PEOPLE_IDENTIFIER } from "./lib/constants";

const ONBOARDING_SIZE =
  process.env.ONBOARDING_SIZE == null ? null : Number(process.env.ONBOARDING_SIZE);

async function main() {
  const { client, api } = connectPeople();
  const signer = devSigner();
  const sudo = sudoSubmitter(api, signer, { customSignedExtensions });
  // An authorized (general-transaction) submitter for permissionless `authorize`
  // calls. `VerifyMultiSignature: Disabled` is the same override the signed
  // scripts use — it lets PAPI emit a structurally valid v5 general extrinsic.
  const authorized = signedSubmitter(createGeneralSigner(), { customSignedExtensions });

  try {
    if (await api.query.People.PeopleCollectionCreated.getValue()) {
      console.log("People collection already exists.");
    } else {
      console.log("Creating the people collection (authorized tx) ...");
      await authorized(api.tx.People.create_people_collection(), "People.create_people_collection");
    }

    if (ONBOARDING_SIZE == null) {
      console.log("Leaving people onboarding size unchanged (set ONBOARDING_SIZE to override).");
    } else {
      const tx = api.tx.Members.set_onboarding_size({
        identifier: PEOPLE_IDENTIFIER,
        onboarding_size: ONBOARDING_SIZE,
      });
      console.log(`Setting people onboarding size to ${ONBOARDING_SIZE} ...`);
      await sudo(tx.decodedCall, "Members.set_onboarding_size");
    }
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
