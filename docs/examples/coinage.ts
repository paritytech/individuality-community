/**
 * Usage example — coin operations.
 *
 * Calls: Coinage.split / transfer / load_recycler_with_external_asset /
 * unload_recycler_into_external_asset.
 *
 *   pnpm run coinage
 *
 * Unlike the other usage calls, these are NOT plain signed transactions, so
 * this example builds and encodes them against the live metadata to show their
 * shapes — it does not submit:
 *   - `split` / `transfer` are authenticated by a Coin origin: a transaction
 *     extension consumes the coin being spent, so there is no ordinary signed
 *     account to sign with.
 *   - `load` / `unload` require real ring membership and an ownership-proof
 *     signature, which can't be fabricated.
 *
 * The `value` fields below take a `Denomination`: an `i8` exponent, not a raw
 * token balance.
 */
import { Enum, FixedSizeBinary } from "polkadot-api";
import { connectPeople } from "./lib/client";

const ALICE = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
const DENOMINATION = 3; // i8
// Coinage wraps each underlying asset in an instance.
const INSTANCE_ID = 0;

// Placeholder 32-byte ring member key.
const MEMBER_KEY = FixedSizeBinary.fromHex("0x" + "00".repeat(32));
// Placeholder 32-byte ring alias to consolidate on unload.
const ALIAS = FixedSizeBinary.fromHex("0x" + "00".repeat(32));
// Placeholder 64-byte ownership-proof signature.
const PROOF_OF_OWNERSHIP = FixedSizeBinary.fromHex("0x" + "00".repeat(64));

async function main() {
  const { client, api } = connectPeople();

  try {
    // Coinage only works once an operator has created an instance
    // (see operations.md → create_sufficient_instance).
    const instance = await api.query.Coinage.Instances.getValue(INSTANCE_ID);
    console.log("Coinage instance created:", instance ? "yes" : "no");

    // Divide the origin coin into new coins assigned to accounts.
    const splitTx = api.tx.Coinage.split({
      split_into: [[DENOMINATION, [ALICE]]], // one coin of denomination 3 -> ALICE
    });

    // Move the origin coin to `to`.
    const transferTx = api.tx.Coinage.transfer({ to: ALICE });

    // Load external-asset value into the recycler as a coin. `member_key` is the
    // ring member, `proof_of_ownership` a signature over that membership.
    const loadTx = api.tx.Coinage.load_recycler_with_external_asset({
      preservation: Enum("Preserve"),
      instance_id: INSTANCE_ID,
      value: DENOMINATION,
      member_key: MEMBER_KEY,
      proof_of_ownership: PROOF_OF_OWNERSHIP,
    });

    // Take value back out to `to`, consolidating `aliases` from ring `index`/`revision`.
    // `max_fee` bounds how much of the unloaded asset the network fee may consume. It is ignored
    // here because this unload is paid for with a prepaid unload token, which takes no fee out of
    // the output.
    const unloadTx = api.tx.Coinage.unload_recycler_into_external_asset({
      aliases: [ALIAS],
      instance_id: INSTANCE_ID,
      value: DENOMINATION,
      index: 0,
      revision: 0,
      to: ALICE,
      max_fee: 0n,
    });

    const calls = [
      ["split", splitTx],
      ["transfer", transferTx],
      ["load_recycler_with_external_asset", loadTx],
      ["unload_recycler_into_external_asset", unloadTx],
    ] as const;
    for (const [label, tx] of calls) {
      console.log(`${label}:`, (await tx.getEncodedData()).asHex());
    }

    console.log(
      "\nBuilt and encoded against live metadata, not submitted — see the header comment.",
    );
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
