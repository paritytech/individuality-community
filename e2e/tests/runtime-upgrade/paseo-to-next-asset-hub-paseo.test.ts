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

import { ApiPromise, WsProvider } from "@polkadot/api";
import { Keyring } from "@polkadot/keyring";
import { createClient } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";

import { BuildBlockMode, setupWithServer } from "@acala-network/chopsticks";
import { setupContext } from "@acala-network/chopsticks-testing";

import { constants as fsConstants } from "node:fs";
import { access, mkdir, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import WebSocket from "ws";
import { createAssetHubV1Extrinsic } from "../../packages/shared/src/paseo_v1.ts";

const UPGRADE_TEST_TIMEOUT = 20 * 60_000;
const ENABLE_RUNTIME_UPGRADE_TESTS = process.env.RUN_RUNTIME_UPGRADE_TESTS === "1";
const describeRuntimeUpgrade = ENABLE_RUNTIME_UPGRADE_TESTS ? describe : describe.skip;
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);
type PapiWebSocketClass = NonNullable<Parameters<typeof getWsProvider>[1]>["websocketClass"];

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

async function resolveNextAssetHubPaseoMetadataHashRuntimeWasm(): Promise<string> {
  const configuredPath = process.env.NEXT_ASSET_HUB_PASEO_METADATA_HASH_RUNTIME_WASM;
  const runtimePath = configuredPath
    ? resolve(repoRoot, configuredPath)
    : resolve(
        repoRoot,
        "target/release/wbuild/next-asset-hub-paseo-runtime/next_asset_hub_paseo_runtime_metadata_hash.wasm",
      );
  if (!(await isReadable(runtimePath))) {
    throw new Error(`Metadata-hash-enabled Asset Hub test WASM is unreadable: ${runtimePath}`);
  }
  return runtimePath;
}

/** Reads the hash captured beside the exact WASM that the test injects. */
async function metadataHashForRuntime(runtimeWasm: string): Promise<`0x${string}`> {
  const hashPath = runtimeWasm.replace(/\.wasm$/u, ".txt");
  const hash = (await readFile(hashPath, "utf8")).trim();
  if (!/^0x[0-9a-f]{64}$/u.test(hash)) throw new Error(`Invalid metadata hash in ${hashPath}`);
  return hash as `0x${string}`;
}

function resolveEndpoint(): string {
  return process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_ENDPOINT ?? DEFAULT_ENDPOINT;
}

function resolveBlockOption() {
  const blockRef = process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_BLOCK;
  if (blockRef == null) return {};
  return blockRef.startsWith("0x")
    ? { blockHash: blockRef as `0x${string}` }
    : { blockNumber: Number(blockRef) };
}

function resolveServerBlockOption() {
  const blockRef = process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_BLOCK;
  return blockRef == null ? {} : { block: blockRef.startsWith("0x") ? blockRef : Number(blockRef) };
}

function extrinsicPreambleOffset(encoded: `0x${string}`): number {
  const first = Number.parseInt(encoded.slice(2, 4), 16);
  switch (first & 0b11) {
    case 0:
      return 1;
    case 1:
      return 2;
    case 2:
      return 4;
    default:
      return (first >> 2) + 5;
  }
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

        // The shared development spec version intentionally remains at 3_002_000;
        // the compatibility upgrade is signalled by transaction version instead.
        const upgradedBlock = await ctx.chain.newBlock();
        const newVersion = await upgradedBlock.runtimeVersion;
        expect(newVersion.specName).toBe(SPEC_NAME);
        expect(newVersion.transactionVersion).toBeGreaterThan(oldVersion.transactionVersion);
        expect(upgradedBlock.number).toBeGreaterThan(block.number);
      } finally {
        await ctx.teardown();
      }
    },
    UPGRADE_TEST_TIMEOUT,
  );

  test(
    "Polkadot.js rejects a pre-upgrade transaction and submits a fresh standard V4 transfer",
    async () => {
      const runtimeWasm = await resolveNextAssetHubPaseoMetadataHashRuntimeWasm();
      const dbPath = resolve(
        e2eRoot,
        process.env.NEXT_ASSET_HUB_PASEO_UPGRADE_DB ?? ".cache/nextAssetHubPaseo.pjs.sqlite",
      );
      await mkdir(dirname(dbPath), { recursive: true });

      const fork = await setupWithServer({
        endpoint: resolveEndpoint(),
        db: dbPath,
        "allow-unresolved-imports": true,
        "build-block-mode": BuildBlockMode.Manual,
        "process-queued-messages": false,
        "runtime-log-level": RUNTIME_LOG_LEVEL,
        "save-blocks": false,
        "rpc-timeout": 60_000,
        host: "127.0.0.1",
        port: 0,
        ...resolveServerBlockOption(),
      });
      let api: ApiPromise | undefined;
      let provider: WsProvider | undefined;
      let papiClient: ReturnType<typeof createClient> | undefined;

      try {
        provider = new WsProvider(`ws://${fork.addr}`);
        api = await ApiPromise.create({ noInitWarn: true, provider });
        await api.isReady;

        const keyring = new Keyring({ type: "sr25519" });
        const alice = keyring.addFromUri("//Alice");
        const bob = keyring.addFromUri("//Bob");
        await provider.send("dev_setStorage", [
          {
            System: {
              Account: [
                [
                  [alice.address],
                  {
                    consumers: 0,
                    data: { free: 1_000_000_000_000_000n },
                    nonce: 0,
                    providers: 1,
                    sufficients: 0,
                  },
                ],
              ],
            },
          },
        ]);

        const nonce = (await api.query.system.account(alice.address)).nonce.toNumber();
        const preUpgrade = await api.tx.balances
          .transferKeepAlive(bob.address, 1_000_000_000n)
          .signAsync(alice, { nonce });
        const preUpgradeEncoded = preUpgrade.toHex() as `0x${string}`;

        // Reconnect after the runtime code changes so polkadot.js reads the new V15/V16 metadata.
        await api.disconnect();
        await provider.disconnect();
        api = undefined;
        provider = undefined;

        const wasm = await readFile(runtimeWasm);
        fork.chain.head.setWasm(`0x${wasm.toString("hex")}`);
        const upgradedBlock = await fork.chain.newBlock();

        provider = new WsProvider(`ws://${fork.addr}`);
        api = await ApiPromise.create({ noInitWarn: true, provider });
        await api.isReady;

        await expect(api.rpc.author.submitExtrinsic(preUpgradeEncoded)).rejects.toThrow();

        const metadataHash = await metadataHashForRuntime(runtimeWasm);
        const mismatchedMetadata = await api.tx.balances
          .transferKeepAlive(bob.address, 1_000_000_000n)
          .signAsync(alice, { nonce, metadataHash: `0x${"00".repeat(32)}`, mode: 1 });
        await expect(api.rpc.author.submitExtrinsic(mismatchedMetadata.toHex())).rejects.toThrow();

        const matchingMetadata = await api.tx.balances
          .transferKeepAlive(bob.address, 1_000_000_000n)
          .signAsync(alice, { nonce, metadataHash, mode: 1 });
        await api.rpc.author.submitExtrinsic(matchingMetadata.toHex());
        const metadataBlock = await fork.chain.newBlock();
        expect(await metadataBlock.extrinsics).toContain(matchingMetadata.toHex());
        const nonceAfterMetadata = (
          await api.query.system.account.at(metadataBlock.hash, alice.address)
        ).nonce.toNumber();
        expect(nonceAfterMetadata).toBe(nonce + 1);

        // PAPI's stock signer only selects V0, but its dynamic call encoder works with the
        // post-upgrade V16 metadata. Feed that call into the narrow version-aware V1 creator.
        // The Chopsticks RPC pool still parses submissions with Polkadot.js' V4-only codec, so
        // inject the client-created bytes directly into the local block builder. It invokes the
        // upgraded runtime's `BlockBuilder_apply_extrinsic` and proves the V1 transaction executes.
        papiClient = createClient(
          getWsProvider(`ws://${fork.addr}`, { websocketClass: WebSocket as unknown as PapiWebSocketClass }),
        );
        const papi = papiClient.getUnsafeApi() as unknown as {
          tx: {
            System: {
              remark: (args: { remark: Uint8Array }) => { getEncodedData: () => Promise<Uint8Array> };
            };
          };
        };
        const call = await papi.tx.System.remark({ remark: Uint8Array.of(87) }).getEncodedData();
        const runtimeVersion = await api.rpc.state.getRuntimeVersion();
        const v1 = createAssetHubV1Extrinsic(
          call,
          { publicKey: alice.publicKey, sign: payload => alice.sign(payload) },
          {
            nonce: nonceAfterMetadata,
            specVersion: runtimeVersion.specVersion.toNumber(),
            transactionVersion: runtimeVersion.transactionVersion.toNumber(),
            genesisHash: (await api.rpc.chain.getBlockHash(0)).toHex() as `0x${string}`,
          },
        );
        const encodedV1 = `0x${Buffer.from(v1).toString("hex")}` as `0x${string}`;
        const v1PreambleOffset = extrinsicPreambleOffset(encodedV1);
        expect(Number.parseInt(encodedV1.slice(2 + v1PreambleOffset * 2, 4 + v1PreambleOffset * 2), 16)).toBe(
          0x45,
        );
        expect(Number.parseInt(encodedV1.slice(4 + v1PreambleOffset * 2, 6 + v1PreambleOffset * 2), 16)).toBe(
          1,
        );
        const v1Block = await fork.chain.newBlock({ transactions: [encodedV1] });
        expect(await v1Block.extrinsics).toContain(encodedV1);
        expect((await api.query.system.account.at(v1Block.hash, alice.address)).nonce.toNumber()).toBe(
          nonceAfterMetadata + 1,
        );
        const signed = await api.tx.balances
          .transferKeepAlive(bob.address, 1_000_000_000n)
          .signAsync(alice, { nonce: nonceAfterMetadata + 1 });
        const encoded = signed.toHex() as `0x${string}`;
        const preambleOffset = extrinsicPreambleOffset(encoded);
        expect(Number.parseInt(encoded.slice(2 + preambleOffset * 2, 4 + preambleOffset * 2), 16)).toBe(0x84);

        await api.rpc.author.submitExtrinsic(encoded);
        const appliedBlock = await fork.chain.newBlock();
        expect(await appliedBlock.extrinsics).toContain(encoded);
        expect((await api.query.system.account.at(appliedBlock.hash, alice.address)).nonce.toNumber()).toBe(
          nonceAfterMetadata + 2,
        );
        expect(upgradedBlock.number).toBeLessThan(appliedBlock.number);
      } finally {
        papiClient?.destroy();
        await api?.disconnect();
        await provider?.disconnect();
        await fork.close();
      }
    },
    UPGRADE_TEST_TIMEOUT,
  );
});
