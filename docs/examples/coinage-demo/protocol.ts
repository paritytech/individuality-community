import { Binary, type PolkadotSigner } from "polkadot-api";
import {
  compact,
  decAnyMetadata,
  extrinsicFormat,
  unifyMetadata,
} from "@polkadot-api/substrate-bindings";
import {
  getDynamicBuilder,
  getLookupFn,
} from "@polkadot-api/metadata-builders";
import { blake2b256 } from "@polkadot-labs/hdkd-helpers";
import { mergeUint8 } from "polkadot-api/utils";
import {
  encode_members,
  one_shot,
  validate_with_commitment,
  type RingExponent,
} from "verifiablejs/nodejs";

export const recyclerContext = new TextEncoder().encode(
  "pop:polkadot.network/coinrecyclr",
);
export const hex = Binary.toHex;
export const bytes = Binary.fromHex;
export const json = (value: unknown) =>
  JSON.stringify(
    value,
    (_, v) => (typeof v === "bigint" ? v.toString() : v),
    2,
  );
export const encodeMembers = (members: string[]) =>
  encode_members(members.map(bytes));

export function collectionId(
  kind: "recycler" | "paid",
  id: number,
  denomination = 0,
): string {
  const result = new Uint8Array(32);
  result.set(
    new TextEncoder().encode(
      kind === "recycler" ? "coinage/recycler" : "coinage/paidtkn!",
    ),
  );
  new DataView(result.buffer).setUint32(16, id, true);
  if (kind === "recycler") result[20] = denomination;
  return hex(result);
}

export function assetAmount(unit: bigint, denomination: number): bigint {
  if (
    !Number.isInteger(denomination) ||
    denomination < -128 ||
    denomination > 127
  )
    throw new Error("Invalid i8 denomination");
  if (denomination >= 0) return unit << BigInt(denomination);
  const divisor = 1n << BigInt(-denomination);
  if (unit % divisor) throw new Error("Denomination loses asset precision");
  return unit / divisor;
}

export interface Ring {
  index: number;
  revision: number;
  members: string[];
  commitment: string;
  exponent: RingExponent;
  at: string;
}

/** Proofs bind to the actual call and trailing extensions supplied by PAPI. */
export function paidUnloadSigner(
  recycler: Ring,
  entropy: Uint8Array,
  paid: Ring,
  paidEntropy: Uint8Array,
  period: number,
): PolkadotSigner {
  return {
    publicKey: new Uint8Array(32),
    signBytes() {
      throw new Error("Only transaction signing is supported");
    },
    async signTx(callData, extensions, metadata) {
      const meta = unifyMetadata(decAnyMetadata(metadata));
      const entries = meta.extrinsic.signedExtensions[0];
      const index = entries.findIndex((e) => e.identifier === "AsCoinage");
      if (index < 0) throw new Error("Missing AsCoinage extension");
      const values = entries.map((e) => {
        const ext = extensions[e.identifier];
        if (!ext) throw new Error(`Missing extension ${e.identifier}`);
        return ext;
      });
      const trailing = values.slice(index + 1);
      const implication = mergeUint8([
        new Uint8Array([0]),
        callData,
        ...trailing.map((e) => e.value),
        ...trailing.map((e) => e.additionalSigned),
      ]);
      const recyclerMessage = blake2b256(implication);
      const proof = one_shot(
        recycler.exponent,
        entropy,
        encodeMembers(recycler.members),
        recyclerContext,
        recyclerMessage,
      );
      const alias = validate_with_commitment(
        recycler.exponent,
        proof.proof,
        bytes(recycler.commitment),
        recyclerContext,
        recyclerMessage,
      );
      if (hex(alias) !== hex(proof.alias))
        throw new Error("Recycler proof does not match finalized root");
      const paidContext = new Uint8Array(32);
      paidContext.set(new TextEncoder().encode("pop:polkadot.net/coinpaidtok"));
      new DataView(paidContext.buffer).setUint32(28, period, true);
      const builder = getDynamicBuilder(getLookupFn(meta));
      const info = meta.lookup.find(
        (entry) => entry.path.at(-1) === "AsCoinageInfo",
      );
      if (info?.def.tag !== "variant")
        throw new Error("Missing AsCoinageInfo metadata");
      const aliasField = info.def.value
        .find((v) => v.name === "AsUnloadTokenPaid")
        ?.fields.find((f) => f.name === "alias_proofs");
      if (!aliasField) throw new Error("Missing paid-token alias proof field");
      // Current ProofOf is bounded bytes, so each proof has its own SCALE length.
      const encodedProofs = builder
        .buildDefinition(aliasField.type)
        .enc([proof.proof]);
      const paidMessage = blake2b256(mergeUint8([encodedProofs, implication]));
      const token = one_shot(
        paid.exponent,
        paidEntropy,
        encodeMembers(paid.members),
        paidContext,
        paidMessage,
      );
      validate_with_commitment(
        paid.exponent,
        token.proof,
        bytes(paid.commitment),
        paidContext,
        paidMessage,
      );
      const codec = builder.buildDefinition(entries[index].type);
      const value = codec.enc({
        type: "AsUnloadTokenPaid",
        value: {
          proof: token.proof,
          period,
          paid_token_ring_index: paid.index,
          paid_token_ring_revision: paid.revision,
          alias_proofs: [proof.proof],
        },
      });
      const body = mergeUint8([
        extrinsicFormat.enc({ version: 5, type: "general" }),
        new Uint8Array([0]),
        ...values.map((e, i) => (i === index ? value : e.value)),
        callData,
      ]);
      return mergeUint8([compact.enc(body.length), body]);
    },
  };
}
