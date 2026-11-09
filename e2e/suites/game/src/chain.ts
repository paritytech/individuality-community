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

import { createClient, type PolkadotSigner } from "polkadot-api";
import { getPolkadotSigner } from "polkadot-api/signer";
import { getWsProvider } from "polkadot-api/ws";

import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { DEV_PHRASE, entropyToMiniSecret, mnemonicToEntropy } from "@polkadot-labs/hdkd-helpers";
import { customSignedExtensions, PEOPLE_RPC } from "../../../packages/shared/src/client.ts";

export { PEOPLE_RPC };

/** Opens the People chain connection. The untyped API needs no PAPI descriptors. */
export function connect() {
  const client = createClient(getWsProvider(PEOPLE_RPC));
  return { client, api: client.getUnsafeApi() as unknown as PeopleApi };
}

export type PeopleClient = ReturnType<typeof connect>["client"];

/**
 * A value the unsafe API resolves by name at runtime. It carries no generated type.
 *
 * Descriptors would type these values. They would also cost a `just descriptors` run before every
 * verification. They would break compilation whenever the metadata moves. The chain rejects a wrong
 * shape at submission.
 */
// biome-ignore lint/suspicious/noExplicitAny: the unsafe API is untyped by construction
export type Untyped = any;

/** The People chain API. PAPI's unsafe interface reaches it without descriptors. */
export interface PeopleApi {
  query: Untyped;
  tx: Untyped;
  event: Untyped;
  constants: Untyped;
}

/** Submits a call wrapped in `Sudo.sudo`. Reports the inner call's result. */
export type Sudo = (call: Untyped, label: string) => Promise<Untyped>;

/** Submits a plain signed transaction. */
export type Submit = (tx: Untyped, label: string) => Promise<Untyped>;

/** Derives a signer from the well-known dev phrase. `//Alice` is sudo on the local network. */
export function devSigner(uri = "//Alice"): PolkadotSigner {
  const miniSecret = entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE));
  const keypair = sr25519CreateDerive(miniSecret)(uri);
  return getPolkadotSigner(keypair.publicKey, "Sr25519", keypair.sign);
}

/** Serialises a value to JSON. `JSON.stringify` refuses the bigints chain values carry. */
export function toJson(value: unknown): string {
  return JSON.stringify(value, (_key, v) => (typeof v === "bigint" ? v.toString() : v));
}

/**
 * Transaction submitters bound to `signer`.
 *
 * Both resolve at finalization. Both throw a labelled error on failure. `sudo` also reports a
 * failure of the wrapped call. The outer transaction reports that failure as success.
 */
export function submitters(api: PeopleApi, signer: PolkadotSigner) {
  const signOptions = { customSignedExtensions };

  const submit: Submit = async (tx, label) => {
    const result = await tx.signAndSubmit(signer, signOptions);
    if (!result.ok) {
      throw new Error(`${label} failed: ${toJson(result.dispatchError)}`);
    }
    console.log(`  tx ${label}: ok (block #${result.block.number})`);
    return result;
  };

  const sudo: Sudo = async (call, label) => {
    const result = await api.tx.Sudo.sudo({ call }).signAndSubmit(signer, signOptions);
    if (!result.ok) {
      throw new Error(`${label}: outer sudo transaction failed: ${toJson(result.dispatchError)}`);
    }
    // The unsafe API returns matches as `{ original, payload }`. The typed API returns the payload
    // alone. Accepting either. A missing field would report every failed inner call as a success.
    const [sudid] = api.event.Sudo.Sudid.filter(result.events);
    const outcome = (sudid?.payload ?? sudid)?.sudo_result;
    if (outcome === undefined) {
      throw new Error(`${label}: no Sudo.Sudid event in ${toJson(result.events)}`);
    }
    if (!outcome.success) {
      throw new Error(`${label}: inner call failed: ${toJson(outcome.value)}`);
    }
    console.log(`  sudo ${label}: ok (block #${result.block.number})`);
    return result;
  };

  return { submit, sudo };
}

/**
 * Reads the chain's own time in seconds. Every game timestamp is measured against it.
 *
 * The read is at the best block, not the finalized one. Finality lags authoring by about 12
 * seconds. A play time computed from a stale clock loses that lag from a schedule's margin.
 */
export async function chainNow(api: PeopleApi): Promise<number> {
  const millis = (await api.query.Timestamp.Now.getValue({ at: "best" })) as bigint;
  return Number(millis / 1000n);
}
