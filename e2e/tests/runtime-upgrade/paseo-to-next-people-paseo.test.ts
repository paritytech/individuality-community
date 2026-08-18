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

import { describe, expect, test } from "vitest";

import { xxhashAsHex } from "@polkadot/util-crypto";

import { setupContext } from "@acala-network/chopsticks-testing";

import { constants as fsConstants } from "node:fs";
import { access, mkdir, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { Bytes, Struct, Tuple, u8, Vector } from "scale-ts";

const UPGRADE_TEST_TIMEOUT = 20 * 60_000;
const ENABLE_RUNTIME_UPGRADE_TESTS = process.env.RUN_RUNTIME_UPGRADE_TESTS === "1";
const describeRuntimeUpgrade = ENABLE_RUNTIME_UPGRADE_TESTS ? describe : describe.skip;
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);

const repoRoot = resolve(import.meta.dirname, "../../..");
const e2eRoot = resolve(import.meta.dirname, "../..");
const allowedSourceSpecNames = new Set(["next-people-paseo"]);

/** r2e9 (3 pages) + r2e10 (5 pages) at the runtime's `ChunkPageSize`. */
const EXPECTED_CHUNK_PAGE_HASHES = 8;

const sourceEndpointPresets = {
  "next-paseo": "wss://paseo-people-next-system-rpc.polkadot.io",
} as const;
type SourceEndpointPreset = keyof typeof sourceEndpointPresets;

// The expected outcome of the SeedSubscriptionWhitelist migration, mirroring
// people::asset_hub_subscription_whitelist() in runtimes/next-people-paseo.
const ASSET_HUB_PARA_ID = 1500;
const SUBSCRIBER_PALLET_INDEX = 97;
const PEOPLE_IDENTIFIER = "pop:polkadot.network/people     ";
const PEOPLE_LITE_IDENTIFIER = "pop:polkadot.network/people-lite";
// RingExponent::R2e9 encodes as its explicit discriminant.
const PEOPLE_RING_EXPONENT = 9;

// SCALE layout of indiv_pallet_members_notifier::WhitelistedSubscription.
const whitelistedSubscriptionCodec = Struct({
  collections: Vector(Tuple(Bytes(32), u8)),
  palletIndex: u8,
});

/**
 * `twox128(pallet) ++ twox128(item)` — the full key of a plain storage value and the key prefix
 * of a map. `pallet` is the name as declared in `construct_runtime!`.
 */
function storagePrefix(pallet: string, item: string) {
  return `${xxhashAsHex(pallet, 128)}${xxhashAsHex(item, 128).slice(2)}` as `0x${string}`;
}

function subscriptionWhitelistKey(paraId: number): string {
  const paraIdLe = Buffer.alloc(4);
  paraIdLe.writeUInt32LE(paraId, 0);
  // Identity-hashed map key: the storage prefix followed by the ParaId as u32 LE.
  return `${storagePrefix("MembersNotifier", "SubscriptionWhitelist")}${paraIdLe.toString("hex")}`;
}

async function isReadable(path: string) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

async function resolveNextPeoplePaseoRuntimeWasm(): Promise<string> {
  const configuredPath = process.env.NEXT_PEOPLE_PASEO_RUNTIME_WASM;
  if (configuredPath) {
    const runtimePath = resolve(repoRoot, configuredPath);
    if (!(await isReadable(runtimePath))) {
      throw new Error(
        `Configured NEXT_PEOPLE_PASEO_RUNTIME_WASM does not exist or is unreadable: ${runtimePath}`,
      );
    }
    return runtimePath;
  }

  const candidates = [
    resolve(repoRoot, "target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm"),
    resolve(
      repoRoot,
      "target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.wasm",
    ),
  ];

  for (const candidate of candidates) {
    if (await isReadable(candidate)) {
      return candidate;
    }
  }

  throw new Error(
    `Unable to find a built next-people-paseo runtime WASM. Checked:\n${candidates.map(path => `- ${path}`).join("\n")}`,
  );
}

function resolveSourceEndpoints(): Array<{ label: string; endpoint: string }> {
  const raw = process.env.NEXT_PEOPLE_PASEO_UPGRADE_TARGETS ?? "next-paseo";
  return raw
    .split(",")
    .map(value => value.trim())
    .filter(Boolean)
    .map(value => {
      const preset = value as SourceEndpointPreset;
      return {
        label: value,
        endpoint: sourceEndpointPresets[preset] ?? value,
      };
    });
}

describeRuntimeUpgrade("Paseo next-people-paseo -> local next-people-paseo runtime upgrade", () => {
  const sourceEndpoints = resolveSourceEndpoints();

  test.each(sourceEndpoints)(
    "builds a block and seeds the subscription whitelist after injecting local next-people-paseo runtime from $label",
    async ({ endpoint }) => {
      const runtimeWasm = await resolveNextPeoplePaseoRuntimeWasm();
      // Keep a stable filesystem-safe cache name per source endpoint.
      const endpointSlug = endpoint
        .replace(/[^a-z0-9]+/gi, "-")
        .replace(/^-+|-+$/g, "")
        .toLowerCase();
      const dbPath = resolve(
        e2eRoot,
        process.env.NEXT_PEOPLE_PASEO_UPGRADE_DB ??
          `.cache/nextPeoplePaseo.runtime-upgrade.${endpointSlug}.sqlite`,
      );
      const blockRef = process.env.NEXT_PEOPLE_PASEO_UPGRADE_BLOCK;
      const blockOption =
        blockRef == null
          ? {}
          : blockRef.startsWith("0x")
            ? { blockHash: blockRef as `0x${string}` }
            : { blockNumber: Number(blockRef) };

      await mkdir(dirname(dbPath), { recursive: true });

      const ctx = await setupContext({
        endpoint,
        db: dbPath,
        allowUnresolvedImports: true,
        processQueuedMessages: false,
        runtimeLogLevel: RUNTIME_LOG_LEVEL,
        saveBlock: false,
        timeout: 60_000,
        ...blockOption,
      });

      try {
        const block = ctx.chain.head;

        // The fork starts from the live Paseo chain, so this version check tells us
        // which runtime is currently deployed before we inject our local build.
        const oldVersion = await block.runtimeVersion;
        expect(allowedSourceSpecNames.has(oldVersion.specName)).toBe(true);

        // The seeding migration assumes it runs on a chain whose whitelist was never
        // seeded. When this fails, the upgrade carrying SeedSubscriptionWhitelist is
        // live: remove the migration from the Migrations tuple and this section with it.
        const whitelistKey = subscriptionWhitelistKey(ASSET_HUB_PARA_ID);
        expect(await block.get(whitelistKey)).toBeFalsy();

        // This is the simulated runtime upgrade: inject the local
        // next-people-paseo runtime WASM into the block.
        const wasm = await readFile(runtimeWasm);
        block.setWasm(`0x${wasm.toString("hex")}`);

        // Build a block with the injected runtime.
        const upgradedBlock = await ctx.chain.newBlock();
        const newVersion = await upgradedBlock.runtimeVersion;
        expect(newVersion.specName).toBe("next-people-paseo");
        expect(upgradedBlock.number).toBeGreaterThan(block.number);

        // Executive only runs migrations when the spec version changed, so without
        // a bump over the live chain the seeding migration would silently not run.
        expect(newVersion.specVersion).toBeGreaterThan(oldVersion.specVersion);

        // SeedSubscriptionWhitelist ran in the upgrade block and seeded asset hub.
        const rawSubscription = await upgradedBlock.get(whitelistKey);
        expect(rawSubscription, "the migration seeds the asset hub whitelist entry").toBeTruthy();
        const subscription = whitelistedSubscriptionCodec.dec(rawSubscription as string);
        expect(subscription.palletIndex).toBe(SUBSCRIBER_PALLET_INDEX);
        const textDecoder = new TextDecoder();
        expect(
          subscription.collections.map(([identifier, exponent]) => [
            textDecoder.decode(identifier),
            exponent,
          ]),
        ).toEqual([
          [PEOPLE_IDENTIFIER, PEOPLE_RING_EXPONENT],
          [PEOPLE_LITE_IDENTIFIER, PEOPLE_RING_EXPONENT],
        ]);

        // The bootstrap migrations run in the upgrade block. They log errors rather than
        // panicking, so without these assertions a failed migration is invisible here.
        const [peopleCollection, liteCollection] = await upgradedBlock.getMany([
          storagePrefix("People", "PeopleCollectionCreated"),
          storagePrefix("PeopleLite", "LitePeopleCollectionCreated"),
        ]);
        // `bool` encodes as a single byte; ValueQuery means absent == false.
        expect(peopleCollection).toBe("0x01");
        expect(liteCollection).toBe("0x01");

        // Chunk page hashes for the configured ring exponents (r2e9: 3 pages, r2e10: 5).
        // `getKeysPaged` returns a single page, so ask for one more than expected: a short
        // page would truncate the count, a longer one would hide extra entries.
        const chunkPageHashKeys = await upgradedBlock.getKeysPaged({
          prefix: storagePrefix("ChunksManager", "ChunkPageHashes"),
          // `pageSize` truncates entries to its set value.
          // To detect misconfigurations we make room for 9 hashes
          pageSize: EXPECTED_CHUNK_PAGE_HASHES + 1,
          startKey: "0x",
        });
        expect(chunkPageHashKeys.length).toBe(EXPECTED_CHUNK_PAGE_HASHES);
      } finally {
        await ctx.teardown();
      }
    },
    UPGRADE_TEST_TIMEOUT,
  );
});
