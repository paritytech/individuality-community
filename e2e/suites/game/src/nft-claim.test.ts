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

import { afterAll, beforeAll, describe, expect, test } from "vitest";

import type { PolkadotSigner } from "polkadot-api";

import {
  ASSET_HUB_SIGN_OPTIONS,
  addressOf,
  chainNow,
  connect,
  connectAssetHub,
  devSigner,
  type Submit,
  type Sudo,
  submitters,
  type Untyped,
} from "./chain.ts";
import {
  accountClaimant,
  type CreditProof,
  claimTx,
  createClaimCollection,
  creditProofs,
  creditTreeBlocks,
  eventsOf,
  expectClaimError,
  freshPurseKey,
  pollUntil,
  sameAccount,
} from "./claims.ts";
import {
  claimTenureCredits,
  FIRST_OFFSET,
  PHASES,
  playTimes,
  readMinStepPhaseDuration,
  reportAlone,
  scheduleGames,
  setPhases,
  setPlayDeposit,
  setTenureThresholds,
  signUp,
} from "./drive.ts";
import { RUN_ZOMBIENET_TESTS } from "./env.ts";
import { inPhase, type Monitor, sawEvent, startMonitor } from "./monitor.ts";

const SECOND = 1_000;

/** A game runs from its registration start until the reporting deadline, plus the tail steps. */
const GAME_MS = (FIRST_OFFSET + PHASES.reporting + PHASES.player_process) * SECOND;
/** Every wait is given the phase it covers plus a generous tail for inclusion latency. */
const PHASE_SLACK_MS = 90 * SECOND;
const ONE_GAME_TIMEOUT = GAME_MS + PHASE_SLACK_MS;

/**
 * How long a credit tree may take from its root on the People chain to Asset Hub.
 *
 * The offchain worker sends the queued trees once a block. The XCM then crosses the HRMP channel,
 * which takes a few relay blocks.
 */
const DELIVERY_TIMEOUT = 300 * SECOND;

/** Every player earns one tenure credit, whatever their tenure. */
const TENURE_THRESHOLDS = [0];

describe.skipIf(!RUN_ZOMBIENET_TESTS)("a credit earned in a game mints an NFT on Asset Hub", () => {
  let people: ReturnType<typeof connect>;
  let assetHub: ReturnType<typeof connectAssetHub>;
  let monitor: Monitor;
  let sudo: Sudo;
  let submit: Submit;
  /** Alice's Asset Hub submitter. Alice plays, claims and owns the collection. */
  let submitOnAssetHub: Submit;
  let alice: PolkadotSigner;
  /** A funded Asset Hub account that holds no credit. */
  let charlie: PolkadotSigner;
  /** The tenure thresholds before the suite, restored afterwards. */
  let previousThresholds: number[] | undefined;

  beforeAll(async () => {
    people = connect();
    assetHub = connectAssetHub();
    alice = devSigner("//Alice");
    charlie = devSigner("//Charlie");
    ({ sudo, submit } = submitters(people.api, alice));
    ({ submit: submitOnAssetHub } = submitters(assetHub.api, alice, ASSET_HUB_SIGN_OPTIONS));

    monitor = startMonitor(people.client, people.api);

    const existing = await people.api.query.Game.Game.getValue();
    expect(existing, "a game is already running; wait for it to finish or cancel it").toBe(undefined);

    await setPhases(people.api, sudo, await readMinStepPhaseDuration(people.api));
    await setPlayDeposit(people.api, sudo);

    previousThresholds = ((await people.api.query.Game.TenureThresholds.getValue()) as number[]).map(Number);
    await setTenureThresholds(people.api, sudo, TENURE_THRESHOLDS);
  }, 120 * SECOND);

  afterAll(async () => {
    monitor?.stop();
    if (previousThresholds !== undefined) {
      await setTenureThresholds(people.api, sudo, previousThresholds);
    }
    console.log(`--- timeline ---\n${monitor?.timeline() ?? ""}`);
    people?.client.destroy();
    assetHub?.client.destroy();
  }, 60 * SECOND);

  test(
    "an attending account claims its tenure credit as an NFT",
    async () => {
      const gameIndex = Number(await people.api.query.Game.GameIndex.getValue()) + 1;
      const now = await chainNow(people.api);
      const [playTime] = playTimes(now, 1) as [number];
      // The game opens its registration as soon as it is scheduled, `FIRST_GAME_MARGIN` before
      // `playTime - LEAD_TIME`. The window opens at `now`, so a player claims right after signing
      // up.
      await scheduleGames(people.api, sudo, [playTime], playTime - now);

      await monitor.waitFor(
        entry => inPhase("Registration")(entry) && entry.gameIndex === gameIndex,
        `game ${gameIndex} to enter Registration (start_game)`,
        ONE_GAME_TIMEOUT,
      );
      await signUp(people.api, submit);

      // Alice is an account, not a person. The claim records her credits. The attendance
      // backfill pays them after the game, once she attends.
      const claimed = await claimTenureCredits(people.api, submit);
      const [tenureClaim] = eventsOf(claimed, people.api.event.Game.TenureCreditsClaimed);
      expect(tenureClaim?.credits, "one threshold of zero earns one credit").toBe(TENURE_THRESHOLDS.length);

      const notCancelled = { predicate: inPhase("Cancelling"), reason: `game ${gameIndex} was cancelled` };
      // `report` opens at `game_date`, which the game enters after the shuffle.
      await monitor.waitFor(
        entry => inPhase("Reporting")(entry) && entry.chainTime >= playTime,
        `game ${gameIndex} to reach its play time in Reporting`,
        ONE_GAME_TIMEOUT,
        notCancelled,
      );
      await reportAlone(people.api, submit);

      const ended = await monitor.waitFor(
        entry => entry.state === "none",
        `game ${gameIndex} to end (process_players)`,
        ONE_GAME_TIMEOUT,
        notCancelled,
      );
      expect(sawEvent(ended, "GameEnded")).toBe(true);

      // The credit is mintable once its tree block has a root.
      const claimant = accountClaimant(addressOf(alice));
      const blocks = await pollUntil(
        () => creditTreeBlocks(people.api, claimant, gameIndex),
        found => found.length > 0,
        `a credit root of game ${gameIndex} for Alice`,
        ONE_GAME_TIMEOUT,
      );
      const proofs: CreditProof[] = [];
      for (const block of blocks) {
        proofs.push(...(await creditProofs(people.api, claimant, block)));
      }
      expect(proofs, "one proof per credit the tenure claim earned").toHaveLength(tenureClaim.credits);

      // Asset Hub verifies a claim against the root it received from the People chain.
      const leafCounts = new Map<number, number>();
      for (const block of blocks) {
        const tree = await pollUntil<Untyped>(
          () => assetHub.api.query.NftClaims.CreditTrees.getValue(block),
          found => found !== undefined,
          `the credit tree of People block #${block} on Asset Hub`,
          DELIVERY_TIMEOUT,
        );
        leafCounts.set(block, Number(tree.leaf_count));
      }

      const collection = await createClaimCollection(assetHub.api, submitOnAssetHub);
      const [first] = proofs as [CreditProof];

      // The claims chain builds the leaf from the signer, so another signer's leaf is in no tree.
      await expectClaimError(
        claimTx(assetHub.api, first, collection, freshPurseKey("charlie")),
        charlie,
        "InvalidProof",
      );

      const fullyClaimedTrees = new Set<number>();
      for (const [index, proof] of proofs.entries()) {
        const mintTo = freshPurseKey(`alice-${index}`);
        const result = await submitOnAssetHub(
          claimTx(assetHub.api, proof, collection, mintTo),
          `NftClaims.claim(block #${proof.block}, leaf ${proof.leaf_index})`,
        );
        const [minted] = eventsOf(result, assetHub.api.event.NftClaims.CreditClaimed);
        expect(minted, "the claim emits CreditClaimed").not.toBe(undefined);
        expect(Number(minted.block)).toBe(proof.block);
        expect(Number(minted.collection)).toBe(collection);
        expect(sameAccount(minted.owner, mintTo), "the NFT is minted to the named purse key").toBe(true);
        for (const removed of eventsOf(result, assetHub.api.event.NftClaims.TreeFullyClaimed)) {
          fullyClaimedTrees.add(Number(removed.block));
        }
      }

      // Another claimant can hold a credit in the same tree. Asset Hub removes a tree once every
      // leaf is claimed, so a replay then finds no tree instead of a spent leaf.
      const fullyClaimed =
        proofs.filter(proof => proof.block === first.block).length === leafCounts.get(first.block);
      expect(fullyClaimedTrees.has(first.block), "TreeFullyClaimed follows the last leaf").toBe(fullyClaimed);
      await expectClaimError(
        claimTx(assetHub.api, first, collection, freshPurseKey("replay")),
        alice,
        fullyClaimed ? "UnknownCreditTree" : "AlreadyClaimed",
      );
    },
    3 * ONE_GAME_TIMEOUT + DELIVERY_TIMEOUT,
  );
});
