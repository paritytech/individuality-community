// A PolkadotSigner that produces v5 *general* (unsigned-with-extensions)
// transactions, for submitting permissionless `#[pallet::authorize]` calls such
// as `People.create_people_collection` — which take an `Authorized` origin and
// cannot be wrapped in sudo.
//
// Adapted from paritytech/product-preview-net#120 (`e2e/src/general-signer.ts`),
// itself from polkadot-bulletin-chain #194 / polkadot-api#760.
//
// PAPI's submit path calls `signer.signTx(callData, signedExtensions, metadata)`
// to get the bytes it broadcasts. Instead of signing, we assemble the v5 general
// extrinsic by hand (RFC-0084 / RFC-0099):
//
//   compact(payload_len)
//   ‖ extrinsicFormat({ version: 5, type: "general" })
//   ‖ extension_version: u8
//   ‖ extension_values_in_metadata_order
//   ‖ call
//
// PAPI fills each extension `value` from metadata defaults plus any
// `TxOptions.customSignedExtensions` overrides (we pass `VerifyMultiSignature:
// Disabled`, the same override the signed scripts use), so we just read them
// back by identifier in metadata order.

import { compact, decAnyMetadata, extrinsicFormat, unifyMetadata } from "@polkadot-api/substrate-bindings";
import { mergeUint8 } from "polkadot-api/utils";
import type { PolkadotSigner } from "polkadot-api/signer";

const EXTENSION_VERSION = 0;

export function createGeneralSigner(): PolkadotSigner {
  return {
    publicKey: new Uint8Array(32),
    signBytes() {
      throw new Error("createGeneralSigner: signBytes is unsupported");
    },
    async signTx(callData, signedExtensions, metadata) {
      const decMeta = unifyMetadata(decAnyMetadata(metadata));

      const extra = decMeta.extrinsic.signedExtensions[EXTENSION_VERSION].map(({ identifier }) => {
        const ext = signedExtensions[identifier];
        if (!ext) throw new Error(`Missing ${identifier} signed extension`);
        return ext.value;
      });

      const body = mergeUint8([
        extrinsicFormat.enc({ version: 5, type: "general" }),
        new Uint8Array([EXTENSION_VERSION]),
        ...extra,
        callData,
      ]);

      return mergeUint8([compact.enc(body.length), body]);
    },
  };
}
