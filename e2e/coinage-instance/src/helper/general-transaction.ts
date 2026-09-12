import { withCommonExtensions } from "@polkadot-api/signers-common";
import { compact, decAnyMetadata, extrinsicFormat, unifyMetadata } from "@polkadot-api/substrate-bindings";
import type { TxCreator } from "polkadot-api";
import { fromHex, mergeUint8, toHex } from "polkadot-api/utils";

const createGeneralTransaction: TxCreator = async payload => {
  const metadata = unifyMetadata(decAnyMetadata(payload.context.metadata));
  const extensionVersion = payload.txExtVersion ?? 0;
  const extensions = metadata.extrinsic.extensionsByVersion[extensionVersion]!.map(({ identifier }) => {
    const extension = payload.extensions.find(({ id }) => id === identifier);
    if (extension === undefined) {
      throw new Error(`Missing ${identifier} transaction extension`);
    }
    return fromHex(extension.extra);
  });

  const body = mergeUint8([
    extrinsicFormat.enc({ version: 5, type: "general" }),
    new Uint8Array([extensionVersion]),
    ...extensions,
    fromHex(payload.callData),
  ]);

  return toHex(mergeUint8([compact.enc(body.length), body]));
};

export const generalTransaction = withCommonExtensions(createGeneralTransaction);
