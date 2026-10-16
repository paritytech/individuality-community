import { Enum, type Transaction, type TxFinalizedPayload } from "polkadot-api";
import { getTxCreator } from "polkadot-api/tx-creator";

import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { DEV_PHRASE, entropyToMiniSecret, mnemonicToEntropy } from "@polkadot-labs/hdkd-helpers";
import { assetHubApi, type peopleApi, relayApi } from "./api.ts";
import { generalTransaction } from "./general-transaction.ts";
import { log } from "./log.ts";
import type { Origin, PeopleOrigin } from "./origins.ts";

const derive = sr25519CreateDerive(entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)));

function devAccount(uri: string) {
  const pair = derive(uri);
  return getTxCreator(pair.publicKey, "Sr25519", input => pair.sign(input));
}

const alice = devAccount("//Alice");
const eve = devAccount("//Eve");

const peopleOptions = {
  customSignedExtensions: {
    VerifyMultiSignature: { value: Enum("Disabled") },
    RestrictOrigins: { value: false },
  },
};

const authorizedOptions = {
  customSignedExtensions: {
    VerifyMultiSignature: { value: Enum("Disabled") },
    RestrictOrigins: { value: false },
    CheckNonce: { value: 0 },
  },
};

const assetHubOptions = {
  customSignedExtensions: {
    RestrictOrigins: { value: false },
  },
};

type PeopleCall = Parameters<typeof peopleApi.tx.Utility.batch_all>[0]["calls"][number];
type AssetHubCall = Parameters<typeof assetHubApi.tx.Sudo.sudo>[0]["call"];
type RelaySudo = NonNullable<NonNullable<typeof relayApi.tx.Sudo>["sudo"]>;
type RelayCall = Parameters<RelaySudo>[0]["call"];

const NOISE_EVENTS = new Set([
  "System",
  "Balances",
  "TransactionPayment",
  "ParachainSystem",
  "Sudo",
  "Utility",
]);

function stringify(value: unknown): string {
  return (
    JSON.stringify(value, (_key, item) => (typeof item === "bigint" ? item.toString() : item)) ??
    String(value)
  );
}

function describeError(error: unknown): string {
  const module = error as { type?: string; value?: { type?: string; value?: { type?: string } } } | undefined;
  return module?.type === "Module" ? `${module.value?.type}.${module.value?.value?.type}` : stringify(error);
}

function describeEvent(result: TxFinalizedPayload): string {
  const event = result.events.find(event => !NOISE_EVENTS.has(event.type));
  if (event === undefined) {
    return "";
  }

  const detail = stringify(event.value.value);
  return `${event.type}.${event.value.type} ${detail.length > 120 ? `${detail.slice(0, 117)}...` : detail}`;
}

function sudoError(result: TxFinalizedPayload): string | undefined {
  const event = result.events.find(event => event.type === "Sudo" && event.value.type === "Sudid");
  const sudoResult = (
    event?.value.value as { sudo_result?: { success: boolean; value: unknown } } | undefined
  )?.sudo_result;
  return sudoResult !== undefined && !sudoResult.success ? describeError(sudoResult.value) : undefined;
}

function describeOrigin(chain: string, origin: Origin): string {
  return `${chain} as ${origin.requires} [${origin.track}]`;
}

async function submit(label: string, via: string, pending: Promise<TxFinalizedPayload>): Promise<void> {
  log(label, "submitting", via);
  const chain = via.split(" as ")[0];

  try {
    const result = await pending;
    const failure = result.ok ? sudoError(result) : describeError(result.dispatchError);
    if (failure !== undefined) {
      throw new Error(`${chain} #${result.block.number}: ${failure}`);
    }

    const event = describeEvent(result);
    log(
      label,
      "finalized",
      `${chain} #${result.block.number} tx ${result.txHash}${event === "" ? "" : ` ${event}`}`,
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : describeError(error);
    log(label, "failed", message.replace(/\s+/g, " "));
    throw error;
  }
}

/** Local network plumbing with no production counterpart: relay sudo. */
export async function submitRelayRoot(label: string, call: { decodedCall: RelayCall }): Promise<void> {
  const tx = relayApi.tx.Sudo!.sudo!({ call: call.decodedCall });
  await submit(label, "Relay", tx.createAndSubmit(alice));
}

export async function submitAssetHub(
  label: string,
  origin: Origin,
  call: Transaction & { decodedCall: AssetHubCall },
): Promise<void> {
  const pending = {
    sudo: () => assetHubApi.tx.Sudo.sudo({ call: call.decodedCall }).createAndSubmit(alice, assetHubOptions),
    eve: () => call.createAndSubmit(eve, assetHubOptions),
    authorized: () => call.createAndSubmit(generalTransaction, authorizedOptions),
  }[origin.local]();
  await submit(label, describeOrigin("Asset Hub", origin), pending);
}

export async function submitPeople(
  label: string,
  origin: PeopleOrigin,
  call: Transaction & { decodedCall: PeopleCall },
): Promise<void> {
  const pending = {
    eve: () => call.createAndSubmit(eve, peopleOptions),
    authorized: () => call.createAndSubmit(generalTransaction, authorizedOptions),
  }[origin.local]();
  await submit(label, describeOrigin("People", origin), pending);
}
