import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import { AccountId, createClient, type Transaction } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";
import { getPolkadotSigner } from "polkadot-api/signer";
import { mergeUint8 } from "polkadot-api/utils";
import {
  decAnyMetadata,
  Tuple,
  unifyMetadata,
  u32,
} from "@polkadot-api/substrate-bindings";
import {
  getDynamicBuilder,
  getLookupFn,
} from "@polkadot-api/metadata-builders";
import { selectDevAccount } from "./recycler-account.js";
import { blake2b256 } from "@polkadot-labs/hdkd-helpers";
import {
  alias_in_context,
  member_from_entropy,
  one_shot,
  sign,
  validate_with_commitment,
  type RingExponent,
} from "verifiablejs/nodejs";
import { demoApi } from "./chain.js";
import {
  bytes,
  collectionId,
  encodeMembers,
  hex,
  json,
  recyclerContext,
} from "./protocol.js";
import { validateConfig, type RecyclerConfig } from "./recycler-plan.js";
import {
  applyResolution,
  decodeState,
  encodeState,
  eraBirth,
  lastValidBlock,
  newState,
  planAction,
  resolvePending,
  type FinalizedChain,
  type HeldVoucher,
  type LedgerState,
  type PendingOp,
  type Resolution,
  type Voucher,
} from "./recycler-ledger.js";

const planOptions = {
  "duration-seconds": String(96 * 3600),
  "interval-seconds": "90",
  denominations: "0,1",
  "max-held-raw": "1000000",
  "asset-floor-raw": "500000",
  "native-budget-raw": "2500000000000",
  "native-floor-raw": "45000000000000",
  "max-unload-fee-raw": "500000000",
  "min-hold-seconds": "3600",
  "max-unloads-per-tx": "4",
  "ring-ceiling": "700",
} as const;
type PlanKey = keyof typeof planOptions;
const { values } = parseArgs({
  options: {
    run: { type: "boolean", default: false },
    output: { type: "string" },
    account: { type: "string" },
    adopt: { type: "string" },
    reconfigure: { type: "boolean", default: false },
    init: { type: "boolean", default: false },
    "max-failures": { type: "string", default: "6" },
    "max-crashes": { type: "string", default: "20" },
    help: { type: "boolean", default: false },
    ...(Object.fromEntries(
      Object.keys(planOptions).map((k) => [k, { type: "string" }]),
    ) as Record<PlanKey, { type: "string" }>),
  },
});
if (values.help || !values.output) {
  console.log(`pnpm recycler-bot --output runs/name [--run | --init [--adopt runs/old-run] [--reconfigure]]
  [--duration-seconds ${planOptions["duration-seconds"]}] [--interval-seconds 90] [--denominations 0,1]
  [--max-held-raw 1000000] [--asset-floor-raw 500000] [--min-hold-seconds 3600] [--max-unloads-per-tx 4]
  [--native-budget-raw 2500000000000] [--native-floor-raw 45000000000000] [--max-unload-fee-raw 500000000]
  [--account Bob|Alice|Charlie|Dave|Eve|Ferdie]
  [--ring-ceiling 700] [--max-failures 6] [--max-crashes 20]
Without --run or --init, writes a read-only preflight. --run loads recycler vouchers from the selected public test account on
public Paseo People and, once max-held-raw is reached, withdraws the oldest vouchers back to that account in
the same transaction as each new load. An existing output directory resumes from its checkpoint;
--init creates or reconfigures a run without submitting. A STOP file in the output directory,
SIGINT or SIGTERM stops once any in-flight transaction settles.`);
  process.exit(values.help ? 0 : 2);
}

const endpoints = [
  "wss://people-paseo.rotko.net",
  "wss://people-paseo.gatotech.network",
  "wss://rpc.interweb-it.com/people-paseo",
];
const genesis =
  "0xe6c30d6e148f250b887105237bcaa5cb9f16dd203bf7b5b9d4f1da7387cb86ec";
const instanceId = 0;
const unit = 10000n;
const period = 64;
// Covers one transaction's weight fee; unload fees are bounded separately.
const worstFee = 500000000n;
const maxFailures = Number(values["max-failures"]);
const maxCrashes = Number(values["max-crashes"]);
const output = resolve(values.output);
const file = (name: string) => resolve(output, name);
const runFile = file("run.json");
const resuming = existsSync(runFile);
if (resuming && values.adopt)
  throw Error("--adopt only applies when a run is created");
const stored = resuming ? JSON.parse(readFileSync(runFile, "utf8")) : {};
const { name: devAccount, address, pair } = selectDevAccount(values.account, resuming ? stored : undefined);
const plan = {} as Record<PlanKey, string>;
for (const key of Object.keys(planOptions) as PlanKey[]) {
  const given = values[key];
  if (resuming && !values.reconfigure && given !== undefined && given !== stored.plan[key])
    throw Error(`--${key} differs from the stored run; pass --reconfigure to change it`);
  // Options added after a run was created take their default.
  plan[key] = given ?? (resuming ? (stored.plan[key] ?? planOptions[key]) : planOptions[key]);
}
const config: RecyclerConfig = {
  durationSeconds: Number(plan["duration-seconds"]),
  intervalSeconds: Number(plan["interval-seconds"]),
  denominations: plan.denominations
    .split(",")
    .map((s) => (s.trim() === "" ? NaN : Number(s))),
  maxHeldRaw: BigInt(plan["max-held-raw"]),
  assetFloorRaw: BigInt(plan["asset-floor-raw"]),
  nativeBudgetRaw: BigInt(plan["native-budget-raw"]),
  nativeFloorRaw: BigInt(plan["native-floor-raw"]),
  maxUnloadFeeRaw: BigInt(plan["max-unload-fee-raw"]),
  minHoldSeconds: Number(plan["min-hold-seconds"]),
  maxUnloadsPerTx: Number(plan["max-unloads-per-tx"]),
  ringCeiling: Number(plan["ring-ceiling"]),
};
const { amounts, maxActions } = validateConfig(config, unit);
mkdirSync(output, { recursive: true, mode: 0o700 });

function atomicWrite(path: string, text: string) {
  writeFileSync(`${path}.tmp`, text, { mode: 0o600 });
  renameSync(`${path}.tmp`, path);
}
const receiptFile = file("receipts.jsonl");
function record(value: Record<string, unknown>) {
  appendFileSync(
    receiptFile,
    JSON.stringify({ time: new Date().toISOString(), ...value }, (_, v) =>
      typeof v === "bigint" ? String(v) : v,
    ) + "\n",
    { mode: 0o600 },
  );
}
const log = (message: string) =>
  console.log(`${new Date().toISOString()} ${message}`);

let connection = "connecting";
const client = createClient(
  getWsProvider(endpoints, {
    heartbeatTimeout: 60000,
    onStatusChanged: (s) => {
      connection = `${s.type}${"uri" in s ? ` ${s.uri}` : ""}`;
    },
  }),
);
const api = demoApi(client);
const unsafe = client.getUnsafeApi() as any;
const signer = getPolkadotSigner(pair.publicKey, "Sr25519", pair.sign);
const extensions = {
  VerifyMultiSignature: { value: { type: "Disabled", value: undefined } },
};

async function bounded<T>(
  label: string,
  promise: Promise<T>,
  milliseconds = 60000,
): Promise<T> {
  let timer: NodeJS.Timeout;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(Error(`${label} timed out after ${milliseconds} ms`)),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer!);
  }
}

let stopping = false;
for (const signal of ["SIGINT", "SIGTERM"] as const)
  process.on(signal, () => {
    stopping = true;
  });
const stopRequested = () => stopping || existsSync(file("STOP"));

let state: LedgerState | undefined;
let lastStatus: Record<string, unknown> = {};
let asset: unknown;
const saveState = () => state && atomicWrite(file("state.json"), encodeState(state));
function status(phase: string, extra: Record<string, unknown> = {}) {
  lastStatus = {
    ...lastStatus,
    ...extra,
    phase,
    pid: process.pid,
    heartbeatAt: new Date().toISOString(),
    connection,
    genesis,
    payer: address,
    instance: instanceId,
    plan,
    maxActions,
  };
  if (state)
    Object.assign(lastStatus, {
      startedAt: new Date(state.startedAt).toISOString(),
      endsAt: new Date(state.deadline).toISOString(),
      held: state.held.length,
      heldValue: state.held.reduce((a, v) => a + v.amount, 0n),
      heldByDenomination: Object.fromEntries(
        config.denominations.map((d) => [
          d,
          state!.held.filter((v) => v.denomination === d).length,
        ]),
      ),
      pending: state.pending
        ? { id: state.pending.id, kind: state.pending.kind, txHash: state.pending.txHash }
        : null,
      counters: state.counters,
      consecutiveFailures: state.consecutiveFailures,
    });
  atomicWrite(file("status.json"), json(lastStatus) + "\n");
}

// Voucher secrets: written before a key is ever submitted, never committed.
type Secret = Voucher & { label: string; entropy: string; source: string };
const secretFile = file("vouchers.json");
const secrets: Secret[] = existsSync(secretFile)
  ? JSON.parse(readFileSync(secretFile, "utf8"), (k, v) =>
      k === "amount" ? BigInt(v) : v,
    )
  : [];
function saveSecret(secret: Secret) {
  secrets.push(secret);
  atomicWrite(secretFile, json(secrets) + "\n");
}
const secretOf = (member: string) => {
  const s = secrets.find((v) => v.member === member);
  if (!s) throw Error(`No saved secret for ${member}`);
  return bytes(s.entropy);
};

// Canonical finalized chain access through the legacy RPC methods, which serve
// any historical block rather than only those pinned by this client.
let eventsCodec: { keys: { enc(): string }; value: { dec(v: string): any } };
let inputsCodec: { enc(v: unknown): Uint8Array };
async function loadMetadata(at: string) {
  const meta = unifyMetadata(
    decAnyMetadata(await bounded("metadata", client.getMetadata(at))),
  );
  const builder = getDynamicBuilder(getLookupFn(meta));
  eventsCodec = builder.buildStorage("System", "Events") as any;
  const pallet = meta.pallets.find((p) => p.name === "Coinage")!;
  const calls = meta.lookup[pallet.calls!.type];
  if (calls.def.tag !== "variant") throw Error("Unexpected Coinage call type");
  const field = calls.def.value
    .find((v) => v.name === "unload_recyclers_into_external_asset_non_anonymous")
    ?.fields.find((f) => f.name === "inputs");
  if (!field) throw Error("Runtime lacks non-anonymous batch unload");
  inputsCodec = builder.buildDefinition(field.type) as any;
}
const rpc = <T>(method: string, params: unknown[]) =>
  bounded(method, client._request<T>(method, params));
const chain: FinalizedChain = {
  async finalized() {
    const head = await bounded("finalized", client.getFinalizedBlock());
    return { number: head.number, hash: head.hash };
  },
  blockHash: (n) => rpc<string>("chain_getBlockHash", [n]),
  async extrinsicHashes(hash) {
    const block = await rpc<{ block: { extrinsics: string[] } }>(
      "chain_getBlock",
      [hash],
    );
    return block.block.extrinsics.map((x) => hex(blake2b256(bytes(x))));
  },
  async outcome(hash, index) {
    const events = (await eventsForExtrinsic(hash, index)).map((e) => e.event);
    const ok = events.some(
      (e) => e.type === "System" && e.value.type === "ExtrinsicSuccess",
    );
    const failed = events.find(
      (e) => e.type === "System" && e.value.type === "ExtrinsicFailed",
    );
    if (ok === !!failed) throw Error(`Ambiguous dispatch result at ${hash}-${index}`);
    let fee = 0n;
    for (const e of events) {
      if (
        e.type === "TransactionPayment" &&
        e.value.type === "TransactionFeePaid" &&
        e.value.value.who === address
      )
        fee += BigInt(e.value.value.actual_fee);
      // Unload fees are charged by transfer from the signer.
      if (
        e.type === "Balances" &&
        e.value.type === "Transfer" &&
        e.value.value.from === address
      )
        fee += BigInt(e.value.value.amount);
    }
    return { ok, fee, error: failed?.value.value.dispatch_error };
  },
  async memberLoaded(denomination, member, at) {
    return !!(await bounded(
      "member",
      api.query.Members.Members.getValue(
        collectionId("recycler", instanceId, denomination),
        member,
        { at },
      ),
    ));
  },
};
async function eventsForExtrinsic(hash: string, index: number) {
  const raw = await rpc<string | null>("state_getStorage", [
    eventsCodec.keys.enc(),
    hash,
  ]);
  if (!raw) throw Error(`No events at ${hash}`);
  return (eventsCodec.value.dec(raw) as any[]).filter(
    (e) => e.phase.type === "ApplyExtrinsic" && e.phase.value === index,
  );
}
const explorer = (hash: string, index: number) =>
  `https://dev.papi.how/explorer/${hash}#networkId=custom&endpoint=${encodeURIComponent(endpoints[0])}&tx=${index}`;

interface Ring {
  index: number;
  revision: number;
  commitment: string;
  members: string[];
}
async function ring(denomination: number, index: number, at: string): Promise<Ring> {
  const collection = collectionId("recycler", instanceId, denomination);
  const [root, keysStatus] = await bounded(
    "ring",
    Promise.all([
      api.query.Members.Root.getValue(collection, index, { at }),
      api.query.Members.RingKeysStatus.getValue(collection, index, { at }),
    ]),
  );
  if (!root) throw Error(`No root for denomination ${denomination} ring ${index}`);
  const members: string[] = [];
  for (let page = 0; members.length < keysStatus.included; page++) {
    const keys = await bounded(
      "ring keys",
      api.query.Members.RingKeys.getValue(collection, index, page, { at }),
    );
    if (!keys.length) throw Error("Ring pages ended before the included count");
    members.push(...keys);
  }
  // RingKeys also holds queued keys that no proof can use yet.
  members.length = keysStatus.included;
  return { index, revision: root.revision, commitment: root.root, members };
}

async function aliasState(v: HeldVoucher, ringIndex: number, at: string) {
  const alias = hex(alias_in_context(secretOf(v.member), recyclerContext));
  const state = await bounded(
    "alias state",
    unsafe.query.Coinage.RecyclerAliasStates.getValue(instanceId, v.denomination, ringIndex, alias, { at }),
  );
  return (state as { type: string } | undefined)?.type;
}

async function confirmMemberships(at: string) {
  for (const v of state!.held.filter((h) => h.ringIndex === undefined)) {
    const location = await bounded(
      "member",
      api.query.Members.Members.getValue(
        collectionId("recycler", instanceId, v.denomination),
        v.member,
        { at },
      ),
    );
    if (location?.type !== "Included") continue;
    const r = await ring(v.denomination, location.value.ring_index, at);
    if (!r.members.includes(v.member)) continue;
    const alias = await aliasState(v, r.index, at);
    if (alias === "Unloaded") throw new Halt(`${v.member} is held in the checkpoint but already unloaded`);
    v.ringIndex = r.index;
    record({
      type: "membership-confirmed",
      member: v.member,
      denomination: v.denomination,
      at,
      ringIndex: r.index,
      revision: r.revision,
      included: r.members.length,
      afterLoadMs: Date.now() - v.loadedAt,
    });
    log(`member confirmed d${v.denomination} ring ${r.index}: ${r.members.length} included`);
  }
  saveState();
}

async function balances(at: string) {
  const [account, holding] = await bounded(
    "balances",
    Promise.all([
      api.query.System.Account.getValue(address, { at }),
      api.query.Assets.Account.getValue(asset as any, address, { at }),
    ]),
  );
  return {
    nonce: account.nonce,
    nativeFree: account.data.free,
    liquidAsset: holding?.balance ?? 0n,
  };
}

// Current ring size per denomination, refreshed at most once a minute.
let ringSizes: { at: number; sizes: Record<number, { ring: number; total: number; included: number }> } = { at: 0, sizes: {} };
async function fullRings(at: string) {
  if (Date.now() - ringSizes.at > 60000) {
    const sizes: typeof ringSizes.sizes = {};
    for (const d of config.denominations) {
      const entries: Array<{ keyArgs: [string, number]; value: { total: number; included: number } }> = await bounded(
        "ring sizes",
        unsafe.query.Members.RingKeysStatus.getEntries(collectionId("recycler", instanceId, d), { at }),
      );
      // New keys join the highest ring index.
      const current = entries.sort((a, b) => b.keyArgs[1] - a.keyArgs[1])[0];
      sizes[d] = current
        ? { ring: current.keyArgs[1], total: current.value.total, included: current.value.included }
        : { ring: 0, total: 0, included: 0 };
    }
    ringSizes = { at: Date.now(), sizes };
    status("running", { rings: sizes });
  }
  return new Set(config.denominations.filter((d) => ringSizes.sizes[d].total >= config.ringCeiling));
}

function newVoucher(denomination: number, amount: bigint, opId: number) {
  const entropy = new Uint8Array(randomBytes(32));
  const member = hex(member_from_entropy(entropy));
  // Saved before the key appears in any signed transaction.
  saveSecret({
    label: `op-${opId}-d${denomination}`,
    denomination,
    amount,
    member,
    entropy: hex(entropy),
    source: "recycler-bot",
  });
  return { entropy, voucher: { member, denomination, amount } };
}

async function unloadCall(unloads: HeldVoucher[], at: string, exponent: RingExponent) {
  const groups = new Map<string, HeldVoucher[]>();
  for (const v of unloads) {
    const key = `${v.denomination}/${v.ringIndex}`;
    groups.set(key, [...(groups.get(key) ?? []), v]);
  }
  const inputs: Array<{ value: number; index: number; revision: number; aliases: string[] }> = [];
  const rings: Array<{ ring: Ring; vouchers: HeldVoucher[] }> = [];
  for (const vouchers of groups.values()) {
    const r = await ring(vouchers[0].denomination, vouchers[0].ringIndex!, at);
    for (const v of vouchers) {
      if (!r.members.includes(v.member)) throw new Halt(`${v.member} not in its ring`);
      const alias = await aliasState(v, r.index, at);
      if (alias === "Unloaded") throw new Halt(`${v.member} is already unloaded`);
      if (alias) throw Error(`${v.member} alias is ${alias}; retrying later`);
    }
    rings.push({ ring: r, vouchers });
    inputs.push({
      value: vouchers[0].denomination,
      index: r.index,
      revision: r.revision,
      aliases: vouchers.map((v) =>
        hex(alias_in_context(secretOf(v.member), recyclerContext)),
      ),
    });
  }
  // The runtime proves blake2_256((instance_id, inputs, to, signer).encode()).
  const message = blake2b256(
    mergeUint8([
      u32.enc(instanceId),
      inputsCodec.enc(inputs),
      pair.publicKey,
      pair.publicKey,
    ]),
  );
  // Proofs are `Vec<u8>`, which PAPI encodes from bytes rather than hex.
  const proofs: Uint8Array[] = [];
  for (const { ring: r, vouchers } of rings) {
    const encoded = encodeMembers(r.members);
    for (const v of vouchers) {
      const entropy = secretOf(v.member);
      const proof = one_shot(exponent, entropy, encoded, recyclerContext, message);
      const alias = validate_with_commitment(
        exponent,
        proof.proof,
        bytes(r.commitment),
        recyclerContext,
        message,
      );
      if (hex(alias) !== hex(alias_in_context(entropy, recyclerContext)))
        throw Error("Unload proof does not verify against the finalized root");
      proofs.push(proof.proof);
    }
  }
  return api.tx.Coinage.unload_recyclers_into_external_asset_non_anonymous({
    instance_id: instanceId,
    inputs,
    alias_proofs: proofs,
    to: address,
    fee_currency: { type: "Native", value: undefined },
    max_fee: config.maxUnloadFeeRaw * BigInt(unloads.length),
  });
}

function loadCall(voucher: Voucher, entropy: Uint8Array) {
  return api.tx.Coinage.load_recycler_with_external_asset({
    instance_id: instanceId,
    preservation: { type: "Preserve", value: undefined },
    value: voucher.denomination,
    member_key: voucher.member,
    proof_of_ownership: hex(sign(entropy, pair.publicKey)),
  });
}

let lastBroadcast = 0;
let lastSeenInBest = 0;
let watch: { unsubscribe(): void } | undefined;
function broadcast(op: PendingOp) {
  // One watch at a time: a rebroadcast replaces the previous subscription.
  watch?.unsubscribe();
  lastBroadcast = Date.now();
  const subscription = client.submitAndWatch(bytes(op.extrinsic)).subscribe({
    next(event) {
      record({ type: "progress", opId: op.id, event });
      if (event.type === "txBestBlocksState" && event.found) lastSeenInBest = Date.now();
      // Best-block and finalized events are informational; only reconciliation settles an operation.
      if (event.type === "finalized") subscription.unsubscribe();
    },
    error(error) {
      record({ type: "broadcast-error", opId: op.id, error: String(error) });
    },
  });
  watch = subscription;
}

async function submit(
  kind: "load" | "recycle",
  load: { denomination: number; amount: bigint },
  unloads: HeldVoucher[],
  exponent: RingExponent,
) {
  const s = state!;
  const head = await chain.finalized();
  const { nonce } = await balances(head.hash);
  const opId = s.nextOp;
  const { entropy, voucher } = newVoucher(load.denomination, load.amount, opId);
  const calls = [loadCall(voucher, entropy)];
  if (kind === "recycle") calls.unshift(await unloadCall(unloads, head.hash, exponent));
  const tx: Transaction<any, any> =
    calls.length === 1
      ? calls[0]
      : api.tx.Utility.batch_all({ calls: calls.map((c) => c.decodedCall) });
  const encoded = await bounded(
    "sign",
    tx.sign(signer, {
      at: head.hash,
      nonce,
      mortality: { mortal: true, period },
      customSignedExtensions: extensions,
    }),
  );
  const op: PendingOp = {
    id: opId,
    kind,
    load: voucher,
    unloads,
    txHash: hex(blake2b256(encoded)),
    extrinsic: hex(encoded),
    nonce,
    birth: head,
    period,
    signedAt: Date.now(),
    scannedThrough: head.number,
  };
  // Persist the exact bytes before broadcasting, so a restart reconciles them.
  s.pending = op;
  s.nextOp++;
  s.lastActionAt = op.signedAt;
  saveState();
  record({
    type: "submitted",
    opId,
    label: `op-${opId}-${kind}-d${voucher.denomination}`,
    kind,
    member: voucher.member,
    denomination: voucher.denomination,
    amount: voucher.amount,
    unloads: unloads.map(({ member, denomination, amount, ringIndex }) => ({
      member,
      denomination,
      amount,
      ringIndex,
    })),
    txHash: op.txHash,
    nonce,
    birth: head,
    lastValid: lastValidBlock(op),
    extrinsic: op.extrinsic,
  });
  log(`op ${opId} ${kind} d${voucher.denomination} ${op.txHash} nonce ${nonce} valid through #${lastValidBlock(op)}`);
  broadcast(op);
}

async function reconcile() {
  const s = state!;
  const op = s.pending!;
  const resolution: Resolution = await resolvePending(op, chain, () => {});
  saveState(); // Keeps scan progress.
  if (resolution.type === "pending") {
    // Rebroadcasting identical bytes cannot apply twice: the nonce is consumed once.
    // Rebroadcast only when the transaction is not in any recent best block,
    // e.g. after a reorganisation, a reconnect or a restart.
    if (Date.now() - lastBroadcast > 30000 && Date.now() - lastSeenInBest > 60000) {
      record({ type: "rebroadcast", opId: op.id, txHash: op.txHash });
      broadcast(op);
    }
    return;
  }
  if (resolution.type === "inconsistent") throw new Halt(resolution.reason);
  const label = `op-${op.id}-${op.kind}-d${op.load.denomination}`;
  const base = {
    opId: op.id,
    label,
    kind: op.kind,
    txHash: op.txHash,
    denomination: op.load.denomination,
    amount: op.load.amount,
    member: op.load.member,
    unloaded: op.unloads.map((v) => v.member),
    elapsedMs: Date.now() - op.signedAt,
  };
  if (resolution.type === "dropped") {
    record({
      type: "dropped",
      ...base,
      finalized: resolution.finalized,
      lastValid: lastValidBlock(op),
      note: "Absent from every canonical finalized block of its era; the key was never loaded and no funds moved.",
    });
    log(`op ${op.id} dropped (era ended #${lastValidBlock(op)}); will retry with a new key`);
  } else {
    if (resolution.fee > worstFee + config.maxUnloadFeeRaw * BigInt(op.unloads.length))
      throw new Halt(`Fee ${resolution.fee} exceeded the per-transaction bound`);
    record({
      type: resolution.type === "success" ? "finalized" : "failed",
      ...base,
      ok: resolution.type === "success",
      block: resolution.block,
      fee: resolution.fee,
      error: resolution.type === "failed" ? resolution.error : undefined,
      explorer: explorer(resolution.block.hash, resolution.block.index),
    });
    log(
      `op ${op.id} ${resolution.type} #${resolution.block.number}-${resolution.block.index} ${explorer(resolution.block.hash, resolution.block.index)}`,
    );
  }
  if (resolution.type === "success")
    for (const v of op.unloads)
      if ((await aliasState(v, v.ringIndex!, resolution.block.hash)) !== "Unloaded")
        throw new Halt(`${v.member} is not unloaded after ${op.txHash}`);
  state = applyResolution(s, op.id, resolution, Date.now());
  saveState();
}

class Halt extends Error {}

/** Imports vouchers from an earlier run after proving each one's finalized state. */
async function adopt(dir: string, at: string) {
  const old: Array<{ label: string; denomination: number; entropy: string; member: string }> =
    JSON.parse(readFileSync(resolve(dir, "vouchers.json"), "utf8"));
  const rows = readFileSync(resolve(dir, "receipts.jsonl"), "utf8")
    .trim()
    .split("\n")
    .map((l) => JSON.parse(l));
  const report: unknown[] = [];
  const account = await balances(at);
  for (const v of old) {
    const amount = unit << BigInt(v.denomination);
    const loaded = await chain.memberLoaded(v.denomination, v.member, at);
    const submitted = rows.find((r) => r.type === "submitted" && r.member === v.member);
    const done = rows.find((r) => r.type === "finalized" && r.member === v.member);
    if (loaded) {
      if (!done) throw new Halt(`Loaded ${v.member} lacks a finalized receipt`);
      const canonical = await chain.blockHash(done.block.number);
      const hashes = await chain.extrinsicHashes(canonical);
      if (canonical !== done.block.hash || hashes[done.block.index] !== done.txHash)
        throw new Halt(`Receipt for ${v.member} is not canonical`);
      state!.held.push({
        member: v.member,
        denomination: v.denomination,
        amount,
        loadedAt: Date.parse(done.time),
        block: done.block,
      });
      saveSecret({ ...v, amount, source: `adopted from ${dir}` });
      report.push({ label: v.label, member: v.member, status: "held", block: done.block, txHash: done.txHash, explorer: explorer(done.block.hash, done.block.index) });
      continue;
    }
    // Not loaded: prove the submission can never be included before forgetting it.
    if (!submitted) {
      report.push({ label: v.label, member: v.member, status: "never-submitted" });
      continue;
    }
    const decoded = decodeSigned(bytes(submitted.extrinsic));
    const seen = rows.find((r) => r.type === "progress" && r.label === v.label && r.event?.found);
    if (!seen) throw new Halt(`${v.label} has no observed block to anchor its era`);
    const birth = eraBirth(seen.event.block.number, decoded.phase, decoded.period);
    const lastValid = birth + decoded.period - 1;
    const head = await chain.finalized();
    if (head.number < lastValid) throw new Halt(`${v.label} era has not ended`);
    let found: unknown;
    for (let n = birth; n <= lastValid; n++) {
      const hash = await chain.blockHash(n);
      const i = (await chain.extrinsicHashes(hash)).indexOf(submitted.txHash);
      if (i >= 0) found = { n, hash, i };
    }
    if (found) throw new Halt(`${v.label} is in a canonical block: ${json(found)}`);
    const bestReports = rows
      .filter((r) => r.type === "progress" && r.label === v.label && r.event?.found)
      .map((r) => ({
        block: r.event.block,
        canonicalHashAtHeight: null as string | null,
      }));
    for (const b of bestReports)
      b.canonicalHashAtHeight = await chain.blockHash(b.block.number);
    report.push({
      label: v.label,
      member: v.member,
      status: "dropped",
      txHash: submitted.txHash,
      nonce: decoded.nonce,
      eraBirth: birth,
      eraPeriod: decoded.period,
      lastValid,
      scannedCanonicalBlocks: [birth, lastValid],
      finalizedAtCheck: head,
      payerNonceAtCheck: account.nonce,
      memberLoaded: false,
      reorganisedBestBlocks: bestReports,
    });
  }
  const result = { checkedAt: new Date().toISOString(), source: dir, finalized: at, payerAssetLiquid: account.liquidAsset, vouchers: report };
  atomicWrite(file("adoption.json"), json(result) + "\n");
  record({ type: "adopted", ...result });
  saveState();
  log(`adopted ${state!.held.length} vouchers from ${dir}`);
}

// Decodes the era and nonce of a signed v4 extrinsic using live metadata.
let signedCodec: { dec(v: Uint8Array): any[] };
let extensionNames: string[] = [];
function decodeSigned(extrinsic: Uint8Array) {
  // Decoders read from offset 0 of the underlying buffer, so copy instead of subarray.
  const body = extrinsic.slice([1, 2, 4][extrinsic[0] & 3]);
  if (body[0] !== 0x84) throw Error("Expected a signed v4 extrinsic");
  const values = signedCodec.dec(body.slice(1));
  const ext = Object.fromEntries(extensionNames.map((n, i) => [n, values[i + 2]]));
  const era = ext.CheckMortality as { type: string; value: number };
  if (!era.type.startsWith("Mortal")) throw Error("Expected a mortal era");
  // PAPI splits the two era bytes into the variant name and its value.
  const encoded = Number(era.type.slice(6)) | (era.value << 8);
  const period = 2 << (encoded & 0xf);
  const phase = (encoded >> 4) * Math.max(period >> 12, 1);
  return { nonce: Number(ext.CheckNonce), period, phase };
}
async function main() {
  const spec = await bounded("chain", client.getChainSpecData());
  if (spec.genesisHash !== genesis) throw new Halt("Wrong chain genesis");
  const head = await chain.finalized();
  await loadMetadata(head.hash);
  {
    const meta = unifyMetadata(decAnyMetadata(await client.getMetadata(head.hash)));
    const builder = getDynamicBuilder(getLookupFn(meta));
    const exts = meta.extrinsic.signedExtensions[0];
    if (!("address" in meta.extrinsic)) throw Error("Unsupported extrinsic metadata");
    extensionNames = exts.map((e) => e.identifier);
    signedCodec = Tuple(
      builder.buildDefinition(meta.extrinsic.address),
      builder.buildDefinition(meta.extrinsic.signature),
      ...exts.map((e) => builder.buildDefinition(e.type)),
    ) as any;
  }
  const [instance, minimum, maximum, ringExponent] = await bounded(
    "configuration",
    Promise.all([
      api.query.Coinage.Instances.getValue(instanceId, { at: head.hash }),
      api.constants.Coinage.MinimumExponent(),
      api.constants.Coinage.MaximumExponent(),
      api.constants.Coinage.RecyclerRingExponent(),
    ]),
  );
  if (!instance || instance.asset_unit !== unit)
    throw new Halt("Unexpected Coinage instance/unit");
  if (config.denominations.some((d) => d < minimum || d > maximum))
    throw new Halt(`Denomination outside chain range ${minimum}..${maximum}`);
  const exponent = Number(ringExponent.type.slice(3)) as RingExponent;
  asset = instance.asset_id;
  const account = await balances(head.hash);
  const rings = Object.fromEntries(
    await Promise.all(
      config.denominations.map(async (d) => {
        const c = collectionId("recycler", instanceId, d);
        const entries = await unsafe.query.Members.RingKeysStatus.getEntries(c, { at: head.hash });
        return [d, entries.map((e: any) => ({ ring: e.keyArgs[1], included: e.value.included, total: e.value.total }))];
      }),
    ),
  );
  const preflight = {
    at: head,
    endpoints,
    genesis,
    runtime: await unsafe.constants.System.Version(),
    instance,
    payer: address,
    balances: account,
    plan,
    amounts,
    maxActions,
    rings,
  };
  atomicWrite(file(resuming ? "preflight-resume.json" : "preflight.json"), json(preflight) + "\n");
  if (!values.run && !values.init) {
    console.log(json(preflight));
    return;
  }

  // One bot per signer; preserve Bob's original lock for existing processes.
  const lock = fileURLToPath(new URL(`runs/.recycler-bot-public-${devAccount.toLowerCase()}.lock`, import.meta.url));
  if (existsSync(lock)) {
    const holder = JSON.parse(readFileSync(lock, "utf8"));
    let alive = true;
    try {
      process.kill(holder.pid, 0);
    } catch {
      alive = false;
    }
    if (alive && holder.pid !== process.pid)
      throw new Halt(`Another recycler bot (pid ${holder.pid}) holds ${lock}`);
    unlinkSync(lock);
  }
  writeFileSync(lock, json({ pid: process.pid, output }), { flag: "wx", mode: 0o600 });
  process.on("exit", () => {
    try {
      if (JSON.parse(readFileSync(lock, "utf8")).pid === process.pid) unlinkSync(lock);
    } catch {}
  });

  if (resuming) {
    state = decodeState(readFileSync(file("state.json"), "utf8"));
    // A "running" status left behind means the last process died without its handler, e.g. SIGKILL.
    const previous = existsSync(file("status.json")) ? JSON.parse(readFileSync(file("status.json"), "utf8")).phase : undefined;
    if (previous === "running") setCrashCount(crashCount() + 1);
    record({ type: "resumed", previousPhase: previous, pending: state.pending?.id ?? null, crashes: crashCount() });
    if (values.reconfigure && json(plan) !== json(stored.plan)) {
      const deadline = state.startedAt + config.durationSeconds * 1000;
      record({ type: "reconfigured", from: stored.plan, to: plan, deadline });
      atomicWrite(runFile, json({ ...stored, plan, history: [...(stored.history ?? []), { at: new Date().toISOString(), plan: stored.plan }] }) + "\n");
      state.deadline = deadline;
      saveState();
      log(`reconfigured; ends ${new Date(deadline).toISOString()}`);
    }
  } else {
    for (const name of ["state.json", "vouchers.json", "receipts.jsonl"])
      if (existsSync(file(name))) throw new Halt(`${name} exists without run.json; inspect ${output}`);
    state = newState(Date.now(), config.durationSeconds);
    saveState();
    record({ type: "started", plan, deadline: state.deadline, balances: account });
    if (values.adopt) await adopt(resolve(values.adopt), head.hash);
    // Written last: a directory without run.json never resumes a half-created run.
    writeFileSync(
      runFile,
      json({ endpoints, genesis, instance: instanceId, payer: address, devAccount, plan, createdAt: new Date().toISOString() }) + "\n",
      { flag: "wx", mode: 0o600 },
    );
  }
  if (values.init) {
    status("initialised");
    log(`initialised ${output}; start it with recycler-service.sh start`);
    return;
  }
  if (crashCount() >= maxCrashes)
    throw new Halt(`${crashCount()} consecutive crashes; inspect logs before restarting`);

  let lastLoop = Date.now(),
    healthySince = Date.now();
  while (!stopRequested()) {
    const now = Date.now();
    if (now - lastLoop > 120000) {
      record({ type: "clock-gap", gapMs: now - lastLoop, note: "Process was suspended, e.g. by machine sleep; reconciling before any new submission." });
      log(`clock gap ${now - lastLoop} ms`);
    }
    lastLoop = now;
    if (now - healthySince > 1800000 && crashCount() > 0) setCrashCount(0);
    const head = await chain.finalized();
    await confirmMemberships(head.hash);
    const b = await balances(head.hash);
    const full = await fullRings(head.hash);
    const action = planAction(state!, config, amounts, b, worstFee, now, maxFailures, full);
    status("running", { finalized: head, balances: b, action: action.type, reason: "reason" in action ? action.reason : undefined });
    if (action.type === "reconcile") {
      await reconcile();
      await pause(3000);
    } else if (action.type === "wait") {
      await pause(Math.min(5000, action.until - Date.now()));
    } else if (action.type === "stop") {
      record({ type: "complete", reason: action.reason, detail: action.detail, counters: state!.counters });
      status("complete", { reason: action.reason, detail: action.detail });
      log(`complete: ${action.reason} ${action.detail}`);
      return;
    } else {
      state!.nextLoad = action.slot;
      await submit(action.type, action.load, action.type === "recycle" ? action.unloads : [], exponent);
    }
  }
  // Stop requested: settle any in-flight transaction first, so the checkpoint is final.
  while (state!.pending) {
    await reconcile();
    if (state!.pending) await pause(3000, false);
  }
  record({ type: "complete", reason: "requested-stop", counters: state!.counters });
  status("stopped", { reason: "requested-stop" });
  log("stopped on request");
}
async function pause(ms: number, interruptible = true) {
  const end = Date.now() + Math.max(0, ms);
  while (Date.now() < end && !(interruptible && stopRequested()))
    await sleep(Math.min(1000, end - Date.now()));
}
const crashFile = file("crashes.json");
const crashCount = (): number =>
  existsSync(crashFile) ? JSON.parse(readFileSync(crashFile, "utf8")).count : 0;
const setCrashCount = (count: number) =>
  atomicWrite(crashFile, json({ count, at: new Date().toISOString() }) + "\n");

main()
  .then(() => {
    if (state) setCrashCount(0);
  })
  .catch((error) => {
    const halted = error instanceof Halt;
    record({ type: halted ? "halted" : "error", message: String(error), stack: error?.stack });
    if (!halted && state) setCrashCount(crashCount() + 1);
    try {
      status(halted ? "halted" : "error", { reason: String(error) });
    } catch {}
    console.error(error);
    // A halt needs a person; exit 0 so the supervisor does not restart it.
    process.exitCode = halted ? 0 : 1;
  })
  .finally(() => {
    client.destroy();
    setTimeout(() => process.exit(), 2000).unref();
  });
