import { AccountId } from "polkadot-api";
import { readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { DEV_PHRASE, entropyToMiniSecret, mnemonicToEntropy } from "@polkadot-labs/hdkd-helpers";

export const DEV_ACCOUNTS = ["Alice", "Bob", "Charlie", "Dave", "Eve", "Ferdie"] as const;

interface SavedSigner {
  devAccount?: string;
  payer: string;
  signerSource?: { file: string; name: string };
}

/** Load private keys without putting mnemonic contents in run files or error messages. */
export function selectBotAccount(requested?: string, stored?: SavedSigner, mnemonicFile?: string) {
  const source = mnemonicFile ? resolve(mnemonicFile) : stored?.signerSource?.file;
  if (!source) {
    const account = selectDevAccount(requested, stored);
    return { ...account, signerSource: undefined, signerKey: undefined, lockName: `public-${account.name.toLowerCase()}` };
  }
  const name = requested ?? stored?.signerSource?.name;
  if (!name) throw Error("--account must select an entry in the mnemonic file");
  if (stored && (!stored.signerSource || stored.signerSource.name !== name || stored.signerSource.file !== source))
    throw Error("Cannot change a run's signer source");
  const mode = statSync(source);
  if (!mode.isFile() || (mode.mode & 0o077) !== 0) throw Error("Mnemonic file must have owner-only permissions");
  let entry: any;
  try {
    const entries = JSON.parse(readFileSync(source, "utf8")).accounts;
    const matches = entries.filter((a: any) => a.name === name);
    if (matches.length !== 1) throw Error();
    entry = matches[0];
  } catch { throw Error("Mnemonic file is invalid or the account is not unique"); }
  let pair: ReturnType<ReturnType<typeof sr25519CreateDerive>>;
  try {
    if (entry.crypto !== "sr25519" || typeof entry.derivationPath !== "string") throw Error();
    pair = sr25519CreateDerive(entropyToMiniSecret(mnemonicToEntropy(entry.mnemonic)))(entry.derivationPath);
  } catch { throw Error("Invalid private signer configuration"); }
  const address = AccountId().dec(pair.publicKey);
  if (entry.address !== address || (stored && stored.payer !== address))
    throw Error("Private signer does not match the saved address");
  if (DEV_ACCOUNTS.some((name) => selectDevAccount(name).address === address))
    throw Error("Private signer must not use a well-known dev account");
  const signerKey = Buffer.from(pair.publicKey).toString("hex");
  return { name, address, pair, signerSource: { file: source, name }, signerKey, lockName: `private-${signerKey}` };
}

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
