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

import {
  ASSET_HUB_PARA_ID,
  connectAssetHub,
  connectPeople,
  PEOPLE_IDENTIFIER,
  PEOPLE_PARA_ID,
} from "@individuality-e2e/shared";
import { RUN_ZOMBIENET_TESTS } from "./env.ts";
import { pollUntil } from "./poll.ts";

// Connects to an already-running, already-bootstrapped network (orchestration lives in
// scripts/03-spawn.sh + scripts/04-bootstrap.sh). Gated behind RUN_ZOMBIENET_TESTS so a
// bare `pnpm -r run test` skips it.
const BLOCK_MS = 6_000;
const POLL_TIMEOUT_MS = 6 * BLOCK_MS;
const TEST_TIMEOUT_MS = 8 * BLOCK_MS;
type MessagingState = {
  egress_channels?: Array<readonly [number, unknown]>;
};

type SubscriptionStatus = {
  type: string;
};

type RingExponent = {
  type: string;
};

function requireQueryEntry(
  module: Record<string, { getValue: (...args: readonly unknown[]) => Promise<unknown> }> | undefined,
  entryName: string,
) {
  const entry = module?.[entryName];
  if (!entry) {
    throw new Error(`Missing query entry: ${entryName}`);
  }
  return entry;
}

describe.skipIf(!RUN_ZOMBIENET_TESTS)("zombienet network health", () => {
  let people: ReturnType<typeof connectPeople>;
  let ah: ReturnType<typeof connectAssetHub>;

  beforeAll(() => {
    people = connectPeople();
    ah = connectAssetHub();
  });

  afterAll(() => {
    people?.client.destroy();
    ah?.client.destroy();
  });

  test(
    "both parachains advance finalized blocks",
    async () => {
      const [p0, a0] = await Promise.all([people.client.getFinalizedBlock(), ah.client.getFinalizedBlock()]);
      const [p1, a1] = await pollUntil(
        () => Promise.all([people.client.getFinalizedBlock(), ah.client.getFinalizedBlock()]),
        ([p, a]) => p.number > p0.number && a.number > a0.number,
        { timeoutMs: POLL_TIMEOUT_MS, intervalMs: BLOCK_MS },
      );
      expect(p1.number).toBeGreaterThan(p0.number);
      expect(a1.number).toBeGreaterThan(a0.number);
    },
    TEST_TIMEOUT_MS,
  );

  test(
    "People sees an HRMP egress channel to Asset Hub",
    async () => {
      const egress = await pollUntil(
        () => peopleEgressIds(),
        ids => ids.includes(ASSET_HUB_PARA_ID),
        { timeoutMs: POLL_TIMEOUT_MS, intervalMs: BLOCK_MS },
      );
      expect(egress).toContain(ASSET_HUB_PARA_ID);
    },
    TEST_TIMEOUT_MS,
  );

  test(
    "Asset Hub sees an HRMP egress channel to People",
    async () => {
      const egress = await pollUntil(
        () => ahEgressIds(),
        ids => ids.includes(PEOPLE_PARA_ID),
        { timeoutMs: POLL_TIMEOUT_MS, intervalMs: BLOCK_MS },
      );
      expect(egress).toContain(PEOPLE_PARA_ID);
    },
    TEST_TIMEOUT_MS,
  );

  test(
    "People notifier lists Asset Hub as a subscriber",
    async () => {
      const subscribers = requireQueryEntry(people.api.query.MembersNotifier, "Subscribers");
      const sub = await pollUntil(
        () => subscribers.getValue(ASSET_HUB_PARA_ID),
        (value: unknown) => value !== undefined,
        { timeoutMs: POLL_TIMEOUT_MS, intervalMs: BLOCK_MS },
      );
      expect(sub).toBeDefined();
    },
    TEST_TIMEOUT_MS,
  );

  // The strongest end-to-end signal: AH only flips to `Active` after the People->AH init XCM is
  // delivered (which needs HRMP open + chunks uploaded + subscribe, in order). Async, so poll long.
  test(
    "Asset Hub subscription becomes Active (init complete)",
    async () => {
      const membersSubscriber = ah.api.query.MembersSubscriber;
      const subscription = requireQueryEntry(membersSubscriber, "Subscription");
      const status = await pollUntil(
        () => subscription.getValue() as Promise<SubscriptionStatus | undefined>,
        (value): value is SubscriptionStatus => value?.type === "Active",
        { timeoutMs: POLL_TIMEOUT_MS, intervalMs: BLOCK_MS },
      );
      if (!status) {
        throw new Error("Expected Asset Hub subscription status to be defined");
      }
      expect(status.type).toBe("Active");

      // Soft secondary: the people ring exponent propagated to AH.
      const ringCollectionExponents = requireQueryEntry(membersSubscriber, "RingCollectionExponents");
      const exponent = (await ringCollectionExponents.getValue(PEOPLE_IDENTIFIER)) as
        | RingExponent
        | undefined;
      expect(exponent?.type).toBe("R2e9");
    },
    TEST_TIMEOUT_MS,
  );

  async function peopleEgressIds(): Promise<number[]> {
    const relevantMessagingState = requireQueryEntry(
      people.api.query.ParachainSystem,
      "RelevantMessagingState",
    );
    const state = (await relevantMessagingState.getValue()) as MessagingState | undefined;
    return (state?.egress_channels ?? []).map(([id]: readonly [number, unknown]) => id);
  }

  async function ahEgressIds(): Promise<number[]> {
    const relevantMessagingState = requireQueryEntry(ah.api.query.ParachainSystem, "RelevantMessagingState");
    const state = (await relevantMessagingState.getValue()) as MessagingState | undefined;
    return (state?.egress_channels ?? []).map(([id]: readonly [number, unknown]) => id);
  }
});
