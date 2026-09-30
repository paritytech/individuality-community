import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import { decAnyMetadata, unifyMetadata } from "@polkadot-api/substrate-bindings";
import {
  getDynamicBuilder,
  getLookupFn,
} from "@polkadot-api/metadata-builders";
import { blake2b256 } from "@polkadot-labs/hdkd-helpers";
import { bytes, collectionId, hex, json } from "./protocol.js";

// Re-verifies a recycler-bot run through an independent HTTP RPC provider and
// writes public data only: no voucher entropy leaves the run directory.
const { values } = parseArgs({
  options: {
    run: { type: "string" },
    endpoint: {
      type: "string",
      default: "https://rpc.interweb-it.com/people-paseo",
    },
    output: { type: "string" },
  },
});
if (!values.run || !values.output)
  throw Error("Pass --run runs/<run> --output evidence/<file>.json");
const genesis =
  "0xe6c30d6e148f250b887105237bcaa5cb9f16dd203bf7b5b9d4f1da7387cb86ec";
const explorerEndpoint = "wss://people-paseo.rotko.net";
async function rpc<T = any>(method: string, params: unknown[] = []): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      const response = await fetch(values.endpoint!, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
        signal: AbortSignal.timeout(30000),
      });
      const result = await response.json();
      if (result.error) throw Error(json(result.error));
      return result.result;
    } catch (error) {
      if (attempt >= 3) throw error;
      await new Promise((r) => setTimeout(r, 2000 * (attempt + 1)));
    }
  }
}
const rows = readFileSync(resolve(values.run, "receipts.jsonl"), "utf8")
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));
if ((await rpc("chain_getBlockHash", [0])) !== genesis)
  throw Error("Endpoint is not public Paseo People");
const finalizedHash = await rpc<string>("chain_getFinalizedHead");
const finalized = Number((await rpc("chain_getHeader", [finalizedHash])).number);
const meta = unifyMetadata(
  decAnyMetadata(bytes(await rpc<string>("state_getMetadata", [finalizedHash]))),
);
const builder = getDynamicBuilder(getLookupFn(meta));
const events = builder.buildStorage("System", "Events");
const members = builder.buildStorage("Members", "Members");
const ringStatus = builder.buildStorage("Members", "RingKeysStatus");
const storage = async (codec: typeof members, at: string, ...keys: unknown[]) => {
  const raw = await rpc<string | null>("state_getStorage", [codec.keys.enc(...keys), at]);
  return raw ? codec.value.dec(raw) : undefined;
};
const explorer = (hash: string, index: number) =>
  `https://dev.papi.how/explorer/${hash}#networkId=custom&endpoint=${encodeURIComponent(explorerEndpoint)}&tx=${index}`;

const verified = [];
for (const r of rows.filter((r) => r.type === "finalized" && r.opId !== undefined)) {
  const { number, hash, index } = r.block;
  if (number > finalized) throw Error(`${r.label} is beyond the finalized head`);
  if ((await rpc("chain_getBlockHash", [number])) !== hash)
    throw Error(`${r.label}: block ${number} is not canonical`);
  const block = await rpc("chain_getBlock", [hash]);
  const extrinsic: string = block.block.extrinsics[index];
  if (hex(blake2b256(bytes(extrinsic))) !== r.txHash)
    throw Error(`${r.label}: extrinsic hash mismatch`);
  const own = (events.value.dec(await rpc("state_getStorage", [events.keys.enc(), hash])) as any[])
    .filter((e) => e.phase.type === "ApplyExtrinsic" && e.phase.value === index)
    .map((e) => e.event);
  if (!own.some((e) => e.type === "System" && e.value.type === "ExtrinsicSuccess"))
    throw Error(`${r.label}: no ExtrinsicSuccess`);
  const loaded = own.find((e) => e.type === "Coinage" && e.value.type === "RecyclerLoadedWithExternalAsset")?.value.value;
  const unloaded = own.find((e) => e.type === "Coinage" && e.value.type === "RecyclersUnloadedIntoExternalAssetNonAnonymous")?.value.value;
  const aliases = own.filter((e) => e.type === "Coinage" && e.value.type === "RecyclerAliasUnloaded").map((e) => e.value.value.alias);
  if (r.kind === "recycle" && aliases.length !== r.unloaded.length)
    throw Error(`${r.label}: ${aliases.length} aliases unloaded for ${r.unloaded.length} vouchers`);
  const location = await storage(members, finalizedHash, collectionId("recycler", 0, r.denomination), r.member);
  if (location?.type !== "Included") throw Error(`${r.label}: member not included at the finalized head`);
  const ring = await storage(ringStatus, finalizedHash, collectionId("recycler", 0, r.denomination), location.value.ring_index);
  // Timing from this bot's own watch events: best-block sightings on forks that were later abandoned.
  const progress = rows.filter((p) => p.type === "progress" && p.opId === r.opId);
  const submitted = rows.find((p) => p.type === "submitted" && p.opId === r.opId);
  const bestBlocks = [...new Set(progress.filter((p) => p.event.type === "txBestBlocksState" && p.event.found).map((p) => p.event.block.hash))];
  const firstBest = progress.find((p) => p.event.type === "txBestBlocksState" && p.event.found)?.time;
  const timing = {
    signedAt: submitted?.time,
    firstBestBlockAt: firstBest,
    finalizedAt: r.time,
    signedToFinalizedSeconds: submitted ? (Date.parse(r.time) - Date.parse(submitted.time)) / 1000 : undefined,
    bestBlocksSeen: bestBlocks.length,
    abandonedBestBlocks: bestBlocks.filter((h) => h !== hash).length,
  };
  verified.push({
    timing,
    label: r.label,
    kind: r.kind,
    txHash: r.txHash,
    block: r.block,
    explorer: explorer(hash, index),
    finalizedAt: r.time,
    load: { denomination: loaded.value, amount: loaded.amount, member: r.member, ringIndex: location.value.ring_index, ringIncludedNow: ring?.included },
    unload: unloaded ? { inputs: unloaded.input_count, amount: unloaded.amount, members: r.unloaded, aliases } : undefined,
    fee: r.fee,
    events: own.map((e) => `${e.type}.${e.value.type}`),
  });
}
const dropped = rows
  .filter((r) => r.type === "dropped")
  .map(({ label, txHash, lastValid, finalized: f, note }) => ({ label, txHash, lastValid, finalizedAtDecision: f, note }));
const rings: Record<string, unknown> = {};
for (const d of [...new Set(verified.map((v) => v.load.denomination))].sort()) {
  const c = collectionId("recycler", 0, d);
  const out = [];
  for (let i = 0; ; i++) {
    const s = await storage(ringStatus, finalizedHash, c, i);
    if (!s) break;
    out.push({ ring: i, included: s.included, total: s.total });
  }
  rings[d] = out;
}
const adoption = rows.find((r) => r.type === "adopted");
const result = {
  endpoint: values.endpoint,
  genesis,
  checkedAt: new Date().toISOString(),
  finalizedHead: { number: finalized, hash: finalizedHash },
  runtime: await rpc("state_getRuntimeVersion", [finalizedHash]).then((v) => ({ spec: v.specName, version: v.specVersion })),
  summary: {
    successful: verified.length,
    loads: verified.filter((v) => v.kind === "load").length,
    recycles: verified.filter((v) => v.kind === "recycle").length,
    dropped: dropped.length,
    assetLoaded: verified.reduce((a, v) => a + BigInt(v.load.amount), 0n),
    assetWithdrawn: verified.reduce((a, v) => a + BigInt(v.unload?.amount ?? 0), 0n),
    nativeFees: verified.reduce((a, v) => a + BigInt(v.fee), 0n),
  },
  ringsAtFinalizedHead: rings,
  adoption: adoption && { source: "runs/recycler-bot-devnet-2026-09-30-2h", finalized: adoption.finalized, payerAssetLiquid: adoption.payerAssetLiquid, vouchers: adoption.vouchers },
  dropped,
  verified,
};
writeFileSync(resolve(values.output), json(result) + "\n");
console.log(json(result.summary));
console.log(`${verified.length} transactions verified at finalized #${finalized}: ${resolve(values.output)}`);
