/**
 * Usage example — print the stage of every NFT claim credit of one claimant, and
 * keep printing as it changes.
 *
 *   CLAIMANT=<ss58 address> pnpm run nft_status
 *   ALIAS=<0x-prefixed 32-byte alias> pnpm run nft_status
 *
 * Reads the People chain and the Asset Hub, so both RPCs must be reachable. The
 * claimant defaults to the account //Bob. `ALIAS` tracks the credits a person earned
 * under that alias instead of an account. Set `ONCE=1` to print once and exit.
 *
 * The People chain serves a block's proofs only while its root is recorded. A
 * block removed before this script first saw it is listed with its leaves unknown.
 */
import { AccountId, Enum, FixedSizeBinary } from "polkadot-api";
import { connectAssetHub, connectPeople, devSigner } from "./lib/client";
import { type Claimant, type CreditProof, creditRows, formatRows } from "./lib/nft-status";

const CLAIMANT: Claimant = process.env.ALIAS
  ? Enum("Person", FixedSizeBinary.fromHex(process.env.ALIAS))
  : Enum("Account", process.env.CLAIMANT ?? AccountId(42).dec(devSigner("//Bob").publicKey));
const POLL_MS = 6000;

async function main() {
  const people = connectPeople();
  const assetHub = connectAssetHub();
  const proofs = new Map<number, CreditProof[]>();

  try {
    console.log(`Credits of ${CLAIMANT.type} ${CLAIMANT.type === "Person" ? CLAIMANT.value.asHex() : CLAIMANT.value}:`);
    let lastPrinted = "";
    for (;;) {
      const text = formatRows(await creditRows(people.api, assetHub.api, CLAIMANT, proofs));
      if (text !== lastPrinted) {
        console.log(`[${new Date().toISOString()}]\n${text}`);
        lastPrinted = text;
      }
      if (process.env.ONCE) return;
      await new Promise((resolve) => setTimeout(resolve, POLL_MS));
    }
  } finally {
    people.client.destroy();
    assetHub.client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
