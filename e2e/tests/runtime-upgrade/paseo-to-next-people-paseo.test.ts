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
import { xxhashAsHex } from "@polkadot/util-crypto";
import { createClient } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";

import { BuildBlockMode, setupWithServer } from "@acala-network/chopsticks";
import { setupContext } from "@acala-network/chopsticks-testing";

import { constants as fsConstants } from "node:fs";
import { access, mkdir, readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { Bytes, Struct, Tuple, u8, Vector } from "scale-ts";
import WebSocket from "ws";
import { createPeopleV1Extrinsic } from "../../packages/shared/src/paseo_v1.ts";

const UPGRADE_TEST_TIMEOUT = 20 * 60_000;
const ENABLE_RUNTIME_UPGRADE_TESTS = process.env.RUN_RUNTIME_UPGRADE_TESTS === "1";
const describeRuntimeUpgrade = ENABLE_RUNTIME_UPGRADE_TESTS ? describe : describe.skip;
const RUNTIME_LOG_LEVEL = Number(process.env.NEXT_PEOPLE_PASEO_UPGRADE_RUNTIME_LOG_LEVEL ?? 0);
type PapiWebSocketClass = NonNullable<Parameters<typeof getWsProvider>[1]>["websocketClass"];

const repoRoot = resolve(import.meta.dirname, "../../..");
const e2eRoot = resolve(import.meta.dirname, "../..");
const SPEC_NAME = "next-people-paseo";
const DEFAULT_ENDPOINT = "wss://paseo-people-next-system-rpc.polkadot.io";
// #120000 predates SeedSubscriptionWhitelist and has spec version 3_000_000. Keep this fixture
// pinned so the migration assertion tests its one-shot precondition rather than a live snapshot.
const PRE_MIGRATION_UPGRADE_FIXTURE_BLOCK = 120_000;

/** r2e9 (3 pages) + r2e10 (5 pages) at the runtime's `ChunkPageSize`. */
const EXPECTED_CHUNK_PAGE_HASHES = 8;

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

/** Reads the hash captured beside the metadata-hash-enabled People release WASM. */
async function metadataHashForRuntime(runtimeWasm: string): Promise<`0x${string}`> {
  const hashPath = runtimeWasm.replace(/\.wasm$/u, "_metadata_hash.txt");
  const hash = (await readFile(hashPath, "utf8")).trim();
  if (!/^0x[0-9a-f]{64}$/u.test(hash)) throw new Error(`Invalid metadata hash in ${hashPath}`);
  return hash as `0x${string}`;
}

function resolveEndpoint(): string {
  return process.env.NEXT_PEOPLE_PASEO_UPGRADE_ENDPOINT ?? DEFAULT_ENDPOINT;
}

function resolveBlockOption() {
  const blockRef = process.env.NEXT_PEOPLE_PASEO_UPGRADE_BLOCK;
  if (blockRef == null) {
    return { blockNumber: PRE_MIGRATION_UPGRADE_FIXTURE_BLOCK };
  }
  return blockRef.startsWith("0x")
    ? { blockHash: blockRef as `0x${string}` }
    : { blockNumber: Number(blockRef) };
}

function resolveServerBlockOption() {
  const blockRef = process.env.NEXT_PEOPLE_PASEO_UPGRADE_BLOCK;
  return blockRef == null
    ? { block: PRE_MIGRATION_UPGRADE_FIXTURE_BLOCK }
    : { block: blockRef.startsWith("0x") ? blockRef : Number(blockRef) };
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

        // This pinned fixture predates the one-shot seed. A populated whitelist here means the
        // fixture no longer tests the migration precondition and must be refreshed deliberately.
        const whitelistKey = subscriptionWhitelistKey(ASSET_HUB_PARA_ID);
        expect(await block.get(whitelistKey)).toBeFalsy();

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

  test(
    "Polkadot.js rejects a pre-upgrade transaction and submits a fresh standard V4 transfer",
    async () => {
      const runtimeWasm = await resolveNextPeoplePaseoRuntimeWasm();
      const dbPath = resolve(
        e2eRoot,
        process.env.NEXT_PEOPLE_PASEO_UPGRADE_DB ?? ".cache/nextPeoplePaseo.pjs.sqlite",
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
        api = await ApiPromise.create({
          noInitWarn: true,
          provider,
        });
        await api.isReady;

        const keyring = new Keyring({ type: "sr25519" });
        const alice = keyring.addFromUri("//Alice");
        const bob = keyring.addFromUri("//Bob");
        // This is a local fork-only endowment. It deliberately uses no custom Individuality
        // extension registration, so Polkadot.js must use the standard V0 metadata pipeline.
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

        // This exercises PAPI's V16 dynamic call encoder with the companion People V1 creator.
        // Chopsticks' RPC pool still uses Polkadot.js' V4-only codec, so inject the client-created
        // General/V1 bytes directly into its local block builder. The upgraded runtime executes
        // them through `BlockBuilder_apply_extrinsic`.
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
        const v1 = createPeopleV1Extrinsic(
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
