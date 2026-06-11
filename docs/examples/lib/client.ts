/**
 * Shared connection + signer helpers used by every example.
 *
 * `people` comes from the generated PAPI descriptors. Run the codegen first
 * (see README): `pnpm run papi:add`, which creates `@polkadot-api/descriptors`.
 */
import { createClient, Enum } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws-provider";
import { withPolkadotSdkCompat } from "polkadot-api/polkadot-sdk-compat";
import { getPolkadotSigner } from "polkadot-api/signer";
import { assethub, people } from "@polkadot-api/descriptors";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import {
  DEV_PHRASE,
  entropyToMiniSecret,
  mnemonicToEntropy,
} from "@polkadot-labs/hdkd-helpers";

/** People chain RPC endpoint. Override with `PEOPLE_RPC=wss://… pnpm run …`. */
export const PEOPLE_RPC = process.env.PEOPLE_RPC ?? "ws://127.0.0.1:10010";

/**
 * This chain carries a custom `VerifyMultiSignature` signed extension, so PAPI
 * requires an explicit value for it when signing. `Disabled` means a normal
 * single signature. Pass these as `signAndSubmit(signer, { customSignedExtensions })`.
 */
export const customSignedExtensions = {
  VerifyMultiSignature: { value: Enum("Disabled") },
};

/** Asset Hub RPC endpoint. Override with `ASSET_HUB_RPC=wss://… pnpm run …`. */
export const ASSET_HUB_RPC = process.env.ASSET_HUB_RPC ?? "ws://127.0.0.1:10020";

/** Connect to the People chain and return the typed API plus the raw client. */
export function connectPeople() {
  const client = createClient(withPolkadotSdkCompat(getWsProvider(PEOPLE_RPC)));
  return { client, api: client.getTypedApi(people) };
}

/** Connect to the Asset Hub and return the typed API plus the raw client. */
export function connectAssetHub() {
  const client = createClient(withPolkadotSdkCompat(getWsProvider(ASSET_HUB_RPC)));
  return { client, api: client.getTypedApi(assethub) };
}

/**
 * A dev signer derived from the well-known dev phrase (`//Alice` by default).
 * In real operations this would be your manager-origin / sudo key instead.
 */
export function devSigner(uri = "//Alice") {
  const miniSecret = entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE));
  const keypair = sr25519CreateDerive(miniSecret)(uri);
  return getPolkadotSigner(keypair.publicKey, "Sr25519", keypair.sign);
}
