import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import { createClient } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";
import { blake2b256 } from "@polkadot-labs/hdkd-helpers";
import { bytes, hex, json } from "./protocol.js";

const { values } = parseArgs({
  options: {
    endpoint: {
      type: "string",
      default: "wss://previewnet.substrate.dev/people",
    },
    receipts: { type: "string" },
    output: { type: "string", default: "verification.json" },
  },
});
if (!values.receipts)
  throw new Error("Pass --receipts runs/<run>/receipts.jsonl");
const receipts = readFileSync(values.receipts, "utf8")
  .trim()
  .split("\n")
  .filter(Boolean)
  .map((line) => JSON.parse(line))
  .filter((r) => r.type === "finalized");
if (!receipts.length) throw new Error("No finalized receipts to verify");
const http = values.endpoint
  .replace(/^wss:/, "https:")
  .replace(/^ws:/, "http:");
async function rpc(method: string, params: unknown[] = []) {
  const response = await fetch(http, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(30000),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const result = await response.json();
  if (result.error) throw new Error(json(result.error));
  return result.result;
}
const client = createClient(getWsProvider(values.endpoint));
const api = client.getUnsafeApi();
const watchdog = setTimeout(() => {
  console.error("Verification timed out");
  client.destroy();
  process.exit(1);
}, 240000);
try {
  const finalizedHash = await rpc("chain_getFinalizedHead");
  const finalizedHeader = await rpc("chain_getHeader", [finalizedHash]);
  const verified = [];
  for (const receipt of receipts) {
    const { hash, number, index } = receipt.block;
    if (number > Number(finalizedHeader.number))
      throw new Error("Receipt is beyond finalized head");
    if ((await rpc("chain_getBlockHash", [number])) !== hash)
      throw new Error(`Noncanonical block ${number}`);
    const block = await rpc("chain_getBlock", [hash]);
    const extrinsic = block?.block.extrinsics[index];
    if (!extrinsic || hex(blake2b256(bytes(extrinsic))) !== receipt.txHash)
      throw new Error(`Extrinsic hash mismatch at ${number}-${index}`);
    const events = (await api.query.System.Events.getValue({
      at: hash,
    })) as Array<{
      phase: { type: string; value: number };
      event: { type: string; value: { type: string; value: unknown } };
    }>;
    const actualEvents = events
      .filter(
        (e) => e.phase.type === "ApplyExtrinsic" && e.phase.value === index,
      )
      .map((e) => e.event);
    if (
      !actualEvents.some(
        (e) => e.type === "System" && e.value.type === "ExtrinsicSuccess",
      )
    )
      throw new Error(`No ExtrinsicSuccess at ${number}-${index}`);
    const item = {
      label: receipt.label,
      txHash: receipt.txHash,
      block: receipt.block,
      extrinsic,
      events: actualEvents,
    };
    verified.push(item);
    console.log(
      `Verified #${number}-${index} ${receipt.label}: ${receipt.txHash}`,
    );
  }
  writeFileSync(
    resolve(values.output),
    json({
      endpoint: values.endpoint,
      genesis: await rpc("chain_getBlockHash", [0]),
      runtime: await rpc("state_getRuntimeVersion", [receipts[0].block.hash]),
      checkedAt: new Date().toISOString(),
      finalizedHead: {
        hash: finalizedHash,
        number: Number(finalizedHeader.number),
      },
      verified,
    }),
  );
  console.log(
    `${verified.length} successful transactions independently verified. ${resolve(values.output)}`,
  );
} finally {
  clearTimeout(watchdog);
  client.destroy();
}
