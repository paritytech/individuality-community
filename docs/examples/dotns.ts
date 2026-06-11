/**
 * Operations example — enable dotNS registrations on the Asset Hub.
 *
 * Calls: DotnsGateway.set_dispatcher_address(...) and
 * DotnsGateway.increase_attestation_allowance(...), each wrapped in Sudo.sudo
 * because they require manager origins (backed by sudo on this chain).
 *
 *   pnpm run dotns
 *
 * After this, attesters can reserve lite-person labels (`reserve_name`) and
 * then proven persons can register full-person labels (`register_name`).
 */
import { AccountId, FixedSizeBinary } from "polkadot-api";
import { connectAssetHub, devSigner } from "./lib/client";
import { sudoSubmitter } from "./lib/submit";

// The deployed RootGatewayDispatcher contract (H160). The placeholder lets the
// example run on a fresh local chain with no contracts deployed.
const DISPATCHER = FixedSizeBinary.fromHex(
  process.env.DOTNS_DISPATCHER ?? "0x" + "2a".repeat(20),
);

// How many reservations the attester may make.
const ALLOWANCE = 10;

async function main() {
  const { client, api } = connectAssetHub();
  const signer = devSigner(); // //Alice — the dev sudo key on a local chain
  const attester = AccountId(42).dec(signer.publicKey); // here also the attester

  const sudo = sudoSubmitter(api, signer);

  try {
    // Dispatcher already set — only topping up the allowance below.
    const existing = await api.query.DotnsGateway.DispatcherAddress.getValue();
    if (existing) {
      console.log(`Dispatcher already set: ${existing.asHex()}`);
    } else {
      // Governance points the gateway at the dispatcher contract.
      const setTx = api.tx.DotnsGateway.set_dispatcher_address({ address: DISPATCHER });
      console.log(`Setting dispatcher address to ${DISPATCHER.asHex()} ...`);
      const r = await sudo(setTx.decodedCall, "set_dispatcher_address");
      console.log(`Dispatcher set in block ${r.block.hash}`);
    }

    // Governance grants the attester a reservation quota.
    const allowTx = api.tx.DotnsGateway.increase_attestation_allowance({
      account: attester,
      count: ALLOWANCE,
    });
    console.log(`Granting ${ALLOWANCE} attestations to ${attester} ...`);
    await sudo(allowTx.decodedCall, "increase_attestation_allowance");

    const allowance = await api.query.DotnsGateway.AttestationAllowance.getValue(attester);
    console.log(`Attester allowance is now ${allowance}.`);
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
