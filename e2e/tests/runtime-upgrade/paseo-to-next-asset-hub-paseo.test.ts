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

import { setupContext } from "@acala-network/chopsticks-testing";

import { constants as fsConstants } from "node:fs";
import { access, mkdir, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const UPGRADE_TEST_TIMEOUT = 20 * 60_000;
const ENABLE_RUNTIME_UPGRADE_TESTS = process.env.RUN_RUNTIME_UPGRADE_TESTS === "1";
const describeRuntimeUpgrade = ENABLE_RUNTIME_UPGRADE_TESTS ? describe : describe.skip;
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);

const repoRoot = resolve(import.meta.dirname, "../../..");
const e2eRoot = resolve(import.meta.dirname, "../..");
const SPEC_NAME = "next-asset-hub-paseo";
const DEFAULT_ENDPOINT = "wss://paseo-asset-hub-next-rpc.polkadot.io";

async function isReadable(path: string) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

async function resolveNextAssetHubPaseoRuntimeWasm(): Promise<string> {
  const configuredPath = process.env.NEXT_ASSET_HUB_PASEO_RUNTIME_WASM;
  if (configuredPath) {
    const runtimePath = resolve(repoRoot, configuredPath);
    if (!(await isReadable(runtimePath))) {
      throw new Error(
        `Configured NEXT_ASSET_HUB_PASEO_RUNTIME_WASM does not exist or is unreadable: ${runtimePath}`,
      );
    }
    return runtimePath;
  }

  const candidates = [
    resolve(repoRoot, "target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime.wasm"),
    resolve(
      repoRoot,
      "target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime.compact.wasm",
    ),
  ];

  for (const candidate of candidates) {
    if (await isReadable(candidate)) {
      return candidate;
    }
  }

  throw new Error(
    `Unable to find a built next-asset-hub-paseo runtime WASM. Checked:\n${candidates.map(path => `- ${path}`).join("\n")}`,
  );
}

function resolveEndpoint(): string {
  return process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_ENDPOINT ?? DEFAULT_ENDPOINT;
}

function resolveBlockOption() {
  const blockRef = process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_BLOCK;
  if (blockRef == null) {
    return {};
  }
  return blockRef.startsWith("0x")
    ? { blockHash: blockRef as `0x${string}` }
    : { blockNumber: Number(blockRef) };
}

describeRuntimeUpgrade("Paseo next-asset-hub-paseo -> local next-asset-hub-paseo runtime upgrade", () => {
  test(
    "builds a block after injecting local next-asset-hub-paseo runtime",
    async () => {
      const runtimeWasm = await resolveNextAssetHubPaseoRuntimeWasm();
      const dbPath = resolve(
        e2eRoot,
        process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_DB ?? ".cache/nextAssetHubPaseo.runtime-upgrade.sqlite",
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
        // next-asset-hub-paseo runtime WASM into the block.
        const wasm = await readFile(runtimeWasm);
        block.setWasm(`0x${wasm.toString("hex")}`);

        // Build a block with the injected runtime. Executive runs the migrations
        // only when the spec version changed.
        const upgradedBlock = await ctx.chain.newBlock();
        const newVersion = await upgradedBlock.runtimeVersion;
        expect(newVersion.specName).toBe(SPEC_NAME);
        expect(newVersion.specVersion).toBeGreaterThan(oldVersion.specVersion);
        expect(upgradedBlock.number).toBeGreaterThan(block.number);
      } finally {
        await ctx.teardown();
      }
    },
    UPGRADE_TEST_TIMEOUT,
  );
});
