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

const UPGRADE_TEST_TIMEOUT = 20 * 60_000;
const ENABLE_RUNTIME_UPGRADE_TESTS = process.env.RUN_RUNTIME_UPGRADE_TESTS === "1";
const describeRuntimeUpgrade = ENABLE_RUNTIME_UPGRADE_TESTS ? describe : describe.skip;
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);

const repoRoot = resolve(import.meta.dirname, "../../..");
const e2eRoot = resolve(import.meta.dirname, "../..");
const SPEC_NAME = "next-people-paseo";
const DEFAULT_ENDPOINT = "wss://paseo-people-next-system-rpc.polkadot.io";

/** r2e9 (3 pages) + r2e10 (5 pages) at the runtime's `ChunkPageSize`. */
const EXPECTED_CHUNK_PAGE_HASHES = 8;

/**
 * `twox128(pallet) ++ twox128(item)` — the full key of a plain storage value and the key prefix
 * of a map. `pallet` is the name as declared in `construct_runtime!`.
 */
function storagePrefix(pallet: string, item: string) {
  return `${xxhashAsHex(pallet, 128)}${xxhashAsHex(item, 128).slice(2)}` as `0x${string}`;
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

function resolveEndpoint(): string {
  return process.env.NEXT_PEOPLE_PASEO_UPGRADE_ENDPOINT ?? DEFAULT_ENDPOINT;
}

function resolveBlockOption() {
  const blockRef = process.env.NEXT_PEOPLE_PASEO_UPGRADE_BLOCK;
  if (blockRef == null) {
    return {};
  }
  return blockRef.startsWith("0x")
    ? { blockHash: blockRef as `0x${string}` }
    : { blockNumber: Number(blockRef) };
}

describeRuntimeUpgrade("Paseo next-people-paseo -> local next-people-paseo runtime upgrade", () => {
  test(
    "builds a block after injecting local next-people-paseo runtime",
    async () => {
      const runtimeWasm = await resolveNextPeoplePaseoRuntimeWasm();
      const dbPath = resolve(
        e2eRoot,
        process.env.NEXT_PEOPLE_PASEO_UPGRADE_DB ?? ".cache/nextPeoplePaseo.runtime-upgrade.sqlite",
      );
      await mkdir(dirname(dbPath), { recursive: true });

      const ctx = await setupContext({
        endpoint: resolveEndpoint(),
        db: dbPath,
        allowUnresolvedImports: true,
        processQueuedMessages: false,
        runtimeLogLevel: RUNTIME_LOG_LEVEL,
        saveBlock: false,
        timeout: 60_000,
        ...resolveBlockOption(),
      });

      try {
        const block = ctx.chain.head;

        // The fork starts from the live Paseo chain, so this version check tells us
        // which runtime is currently deployed before we inject our local build.
        const oldVersion = await block.runtimeVersion;
        expect(oldVersion.specName).toBe(SPEC_NAME);

        // This is the simulated runtime upgrade: inject the local
        // next-people-paseo runtime WASM into the block.
        const wasm = await readFile(runtimeWasm);
        block.setWasm(`0x${wasm.toString("hex")}`);

        // Build a block with the injected runtime. Executive runs the migrations
        // only when the spec version changed.
        const upgradedBlock = await ctx.chain.newBlock();
        const newVersion = await upgradedBlock.runtimeVersion;
        expect(newVersion.specName).toBe(SPEC_NAME);
        expect(newVersion.specVersion).toBeGreaterThan(oldVersion.specVersion);
        expect(upgradedBlock.number).toBeGreaterThan(block.number);

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
