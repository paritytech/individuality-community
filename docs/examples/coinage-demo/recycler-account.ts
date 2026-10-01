import { AccountId } from "polkadot-api";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { DEV_PHRASE, entropyToMiniSecret, mnemonicToEntropy } from "@polkadot-labs/hdkd-helpers";

export const DEV_ACCOUNTS = ["Alice", "Bob", "Charlie", "Dave", "Eve", "Ferdie"] as const;

/** A checkpoint belongs to one signer for its entire lifetime. */
export function selectDevAccount(requested?: string, stored?: { devAccount?: string; payer: string }) {
  const name = requested ?? stored?.devAccount ?? "Bob";
  if (!DEV_ACCOUNTS.some((account) => account === name))
    throw Error(`Unknown dev account ${name}; choose ${DEV_ACCOUNTS.join(", ")}`);
  if (stored?.devAccount && name !== stored.devAccount)
    throw Error("Cannot change a run's signer; create a separate run directory");
  const pair = sr25519CreateDerive(entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)))(`//${name}`);
  const address = AccountId().dec(pair.publicKey);
  if (stored && stored.payer !== address)
    throw Error("Selected signer does not match the saved payer");
  return { name, address, pair };
}
