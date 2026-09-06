// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

import { blake2AsU8a } from "@polkadot/util-crypto";

/** A signer accepted by the Asset Hub `VerifyMultiSignature` extension. */
export type Sr25519Signer = {
  publicKey: Uint8Array;
  sign: (payload: Uint8Array) => Uint8Array;
};

/** Runtime data required by the V1 inherited implication. */
export type PaseoV1SigningContext = {
  nonce: number;
  specVersion: number;
  transactionVersion: number;
  genesisHash: `0x${string}`;
};

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const bytes = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.length;
  }
  return bytes;
}

function fromHex(value: `0x${string}`): Uint8Array {
  if (!/^0x(?:[0-9a-f]{2})+$/iu.test(value)) {
    throw new Error("Expected an even-length hexadecimal value");
  }
  return Uint8Array.from(
    value
      .slice(2)
      .match(/.{2}/gu)
      ?.map(byte => Number.parseInt(byte, 16)) ?? [],
  );
}

function compactU32(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) {
    throw new Error("The transaction nonce must be a u32");
  }
  if (value < 1 << 6) return Uint8Array.of(value << 2);
  if (value < 1 << 14) return Uint8Array.of((value << 2) | 1, value >>> 6);
  if (value < 1 << 30) {
    return Uint8Array.of((value << 2) | 2, value >>> 6, value >>> 14, value >>> 22);
  }
  return Uint8Array.of(3, value & 0xff, value >>> 8, value >>> 16, value >>> 24);
}

function u32(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff) {
    throw new Error("The runtime version must be a u32");
  }
  return Uint8Array.of(value & 0xff, value >>> 8, value >>> 16, value >>> 24);
}

/**
 * Builds the shared signed General/V1 framing used by the Paseo Individuality pipelines.
 *
 * PAPI currently exposes V16 metadata but its bundled signer only creates V4/V0 extrinsics. This
 * narrow adapters deliberately leave call encoding to PAPI and own only the stable V1 extension
 * layout. They create the same disabled Individuality authorizations, normal payment wrapper, and
 * immortal era as the runtimes' V1 transaction factories.
 */
function createPaseoV1Extrinsic(
  call: Uint8Array,
  signer: Sr25519Signer,
  context: PaseoV1SigningContext,
  disabledAuthorizationExtensions: number,
): Uint8Array {
  if (signer.publicKey.length !== 32) throw new Error("Paseo V1 requires a 32-byte account ID");

  const genesisHash = fromHex(context.genesisHash);
  if (genesisHash.length !== 32) throw new Error("Paseo V1 requires a 32-byte genesis hash");

  // The explicit data after VerifyMultiSignature. Empty extensions contribute no bytes; each
  // disabled Individuality authorization is an `Option::None` byte. In the default case,
  // People's SkipCheckIfFeeless<ChargeAssetTxPayment> and Asset Hub's
  // ChargePGAS<ChargeAssetTxPayment> both encode `tip: 0, asset_id: None` as two zero bytes.
  // That shared layout is why this helper serves both chains.
  const afterVerify = concat([
    new Uint8Array(disabledAuthorizationExtensions),
    Uint8Array.of(1), // RestrictOrigins(true)
    Uint8Array.of(0), // CheckEra(Immortal)
    compactU32(context.nonce), // CheckNonce
    Uint8Array.of(0, 0), // Default ChargeAssetTxPayment wrapper fields
    Uint8Array.of(0), // CheckMetadataHash(Disabled)
  ]);

  // The implicit data after VerifyMultiSignature. For an immortal era CheckEra signs the genesis
  // hash. All surrounding authorization and weight extensions have unit implicit data.
  const implicitAfterVerify = concat([
    u32(context.specVersion),
    u32(context.transactionVersion),
    genesisHash,
    genesisHash,
    Uint8Array.of(0), // CheckMetadataHash implicit Option<Hash>::None
  ]);
  const implication = concat([Uint8Array.of(1), call, afterVerify, implicitAfterVerify]);
  const signature = signer.sign(blake2AsU8a(implication, 256));
  if (signature.length !== 64) throw new Error("Paseo V1 requires an sr25519 signature");

  // Version 5 General preamble, then extension version 1. `1, 1` encodes
  // VerifyMultiSignature::Signed(MultiSignature::Sr25519(...)).
  const body = concat([
    Uint8Array.of(0x45, 1),
    Uint8Array.of(1, 1),
    signature,
    signer.publicKey,
    afterVerify,
    call,
  ]);
  return concat([compactU32(body.length), body]);
}

/** Builds the signed General/V1 form of Asset Hub's Individuality pipeline. */
export function createAssetHubV1Extrinsic(
  call: Uint8Array,
  signer: Sr25519Signer,
  context: PaseoV1SigningContext,
): Uint8Array {
  // AsScarcity, AsPgas and AsDotnsGateway are all disabled `Option` values.
  return createPaseoV1Extrinsic(call, signer, context, 3);
}

/** Builds the signed General/V1 form of People's Individuality pipeline. */
export function createPeopleV1Extrinsic(
  call: Uint8Array,
  signer: Sr25519Signer,
  context: PaseoV1SigningContext,
): Uint8Array {
  // People has nine disabled `Option` authorization extensions after VerifyMultiSignature.
  return createPaseoV1Extrinsic(call, signer, context, 9);
}
