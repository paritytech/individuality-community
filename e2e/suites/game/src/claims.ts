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

import { AccountId, Enum, type PolkadotSigner, type SS58String } from "polkadot-api";

import {
  ASSET_HUB_SIGN_OPTIONS,
  type AssetHubApi,
  addressOf,
  devSigner,
  type PeopleApi,
  type Submit,
  toJson,
  type Untyped,
} from "./chain.ts";
import { sleep } from "./monitor.ts";

/** One credit's inclusion proof, as `NftCreditsApi.nft_claim_credit_proofs` returns it. */
export interface CreditProof {
  /** The People chain block whose tree commits to the credit. */
  block: number;
  credit: Untyped;
  leaf_index: number;
  proof: Untyped[];
}

/** The `AccountOrPerson::Account` identity of `address`, as the game chain records an account player. */
export const accountClaimant = (address: SS58String) => Enum("Account", address);

const RUN_ID = Date.now();

/** Whether two SS58 addresses name the same account, whatever their network prefixes. */
export const sameAccount = (a: SS58String, b: SS58String) =>
  AccountId().dec(AccountId().enc(a)) === AccountId().dec(AccountId().enc(b));

/**
 * A purse key no one has used.
 *
 * A Scarcity purse key holds one NFT, so each mint needs an empty one. `label` keeps the keys of one
 * run apart. The run's start time keeps them apart from earlier runs.
 */
export function freshPurseKey(label: string): SS58String {
  return addressOf(devSigner(`//nft-claim/${RUN_ID}/${label}`));
}

/**
 * Resolves with `read()` once `done` holds for it. Throws with `what` when `timeoutMs` elapses first.
 */
export async function pollUntil<T>(
  read: () => Promise<T>,
  done: (value: T) => boolean,
  what: string,
  timeoutMs: number,
  intervalMs = 3_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = await read();
    if (done(value)) {
      return value;
    }
    if (Date.now() >= deadline) {
      throw new Error(`timed out after ${timeoutMs}ms waiting for ${what}; last value ${toJson(value)}`);
    }
    await sleep(intervalMs);
  }
}

/**
 * The tree blocks that commit to a credit of `claimant` in game `gameIndex`, in ascending order.
 *
 * A tree's root is recorded in the block after its credits are awarded, so a caller polls this.
 */
export async function creditTreeBlocks(
  api: PeopleApi,
  claimant: Untyped,
  gameIndex: number,
): Promise<number[]> {
  const roots = (await api.apis.NftCreditsApi.nft_claim_credit_roots(claimant)) as [number, Untyped][];
  return roots.filter(([, tree]) => Number(tree.game_index) === gameIndex).map(([block]) => Number(block));
}

/** The inclusion proofs of every credit `claimant` holds in the tree of `block`. */
export async function creditProofs(api: PeopleApi, claimant: Untyped, block: number): Promise<CreditProof[]> {
  const result = await api.apis.NftCreditsApi.nft_claim_credit_proofs(block, claimant);
  if (!result.success) {
    throw new Error(`nft_claim_credit_proofs(${block}) failed: ${toJson(result.value)}`);
  }
  return (result.value as Untyped[]).map(proof => ({
    block,
    credit: proof.credit,
    leaf_index: Number(proof.leaf_index),
    proof: proof.proof,
  }));
}

/** Reads the payloads of `event` in a transaction result. The unsafe API wraps them as `{ original, payload }`. */
export function eventsOf(result: Untyped, event: Untyped): Untyped[] {
  return event.filter(result.events).map((match: Untyped) => match?.payload ?? match);
}

/**
 * Creates a Scarcity collection with one item and registers it for claims with random item
 * selection.
 *
 * `Random` maps a credit to an item modulo the collection's next item index. One item makes the
 * mapping total.
 */
export async function createClaimCollection(api: AssetHubApi, submit: Submit): Promise<number> {
  const created = await submit(api.tx.Scarcity.create_collection(), "Scarcity.create_collection");
  const [event] = eventsOf(created, api.event.Scarcity.CollectionCreated);
  if (event === undefined) {
    throw new Error(`no Scarcity.CollectionCreated event in ${toJson(created.events)}`);
  }
  const collection = Number(event.collection);

  await submit(
    api.tx.Scarcity.define_item({ collection, transferability: Enum("Transferable"), metadata: [] }),
    `Scarcity.define_item(${collection})`,
  );
  await submit(
    api.tx.NftClaims.set_collection_minter({ collection, selection: Enum("Random") }),
    `NftClaims.set_collection_minter(${collection}, Random)`,
  );
  return collection;
}

/** The `NftClaims.claim` transaction of an account claimant. */
export function claimTx(api: AssetHubApi, proof: CreditProof, collection: number, mintTo: SS58String) {
  return api.tx.NftClaims.claim({
    claimant: Enum("Account"),
    block: proof.block,
    credit: proof.credit,
    leaf_index: proof.leaf_index,
    proof: proof.proof,
    collection,
    mint_to: mintTo,
  });
}

/**
 * Submits `tx` and expects it to fail with the `NftClaims` error `expected`.
 *
 * The transaction is included and pays its fee, so the failure is a dispatch error, not a pool
 * rejection.
 */
export async function expectClaimError(tx: Untyped, signer: PolkadotSigner, expected: string): Promise<void> {
  const result = await tx.signAndSubmit(signer, ASSET_HUB_SIGN_OPTIONS);
  if (result.ok) {
    throw new Error(`the claim succeeded, expected NftClaims.${expected}`);
  }
  const error = result.dispatchError;
  const module = error?.type === "Module" ? error.value : undefined;
  if (module?.type !== "NftClaims" || module.value?.type !== expected) {
    throw new Error(`the claim failed with ${toJson(error)}, expected NftClaims.${expected}`);
  }
  console.log(`  tx NftClaims.claim: failed with ${expected} as expected (block #${result.block.number})`);
}
