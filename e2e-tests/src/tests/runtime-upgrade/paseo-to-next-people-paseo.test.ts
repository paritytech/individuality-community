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
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);

const repoRoot = resolve(import.meta.dirname, "../../../..");
const e2eRoot = resolve(import.meta.dirname, "../../..");
const allowedSourceSpecNames = new Set(["next-people-paseo"]);

const sourceEndpointPresets = {
  "next-paseo": "wss://paseo-people-next-system-rpc.polkadot.io",
  "paseo-review": "wss://paseo-people-review-rpc.polkadot.io",
} as const;
type SourceEndpointPreset = keyof typeof sourceEndpointPresets;

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

  const readableCandidates: string[] = [];
  for (const candidate of candidates) {
    if (await isReadable(candidate)) {
      readableCandidates.push(candidate);
    }
  }

  if (readableCandidates.length === 1) {
    return readableCandidates[0]!;
  }

  if (readableCandidates.length > 1) {
    throw new Error(
      [
        "Found multiple built next-people-paseo runtime WASM candidates.",
        ...readableCandidates.map(path => `- ${path}`),
        "Set NEXT_PEOPLE_PASEO_RUNTIME_WASM to choose the exact file to test.",
      ].join("\n"),
    );
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
    "builds a block after injecting local next-people-paseo runtime from $label",
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

        // This is the simulated runtime upgrade: inject the local
        // next-people-paseo runtime WASM into the block.
        const wasm = await readFile(runtimeWasm);
        block.setWasm(`0x${wasm.toString("hex")}`);

        // Build a block with the injected runtime.
        const upgradedBlock = await ctx.chain.newBlock();
        const newVersion = await upgradedBlock.runtimeVersion;
        expect(newVersion.specName).toBe("next-people-paseo");
        expect(upgradedBlock.number).toBeGreaterThan(block.number);
      } finally {
        await ctx.teardown();
      }
    },
    UPGRADE_TEST_TIMEOUT,
  );
});
