import {
  appendFileSync,
  existsSync,
  mkdirSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import { AccountId, createClient, type TxFinalizedPayload } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";
import { getPolkadotSigner } from "polkadot-api/signer";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import {
  blake2b256,
  DEV_PHRASE,
  entropyToMiniSecret,
  mnemonicToEntropy,
} from "@polkadot-labs/hdkd-helpers";
import { member_from_entropy, sign } from "verifiablejs/nodejs";
import { demoApi } from "./chain.js";
import { collectionId, hex, json } from "./protocol.js";
import { recyclerPlan } from "./recycler-plan.js";

const { values } = parseArgs({
  options: {
    run: { type: "boolean", default: false },
    "duration-seconds": { type: "string", default: "7200" },
    "interval-seconds": { type: "string", default: "90" },
    denominations: { type: "string", default: "1" },
    "max-asset-raw": { type: "string", default: "1600000" },
    "max-native-fee-raw": { type: "string", default: "10000000000" },
    output: { type: "string", default: `runs/recycler-bot-${Date.now()}` },
    help: { type: "boolean", default: false },
  },
});
if (values.help) {
  console.log(
    "pnpm recycler-bot [--run] [--duration-seconds 7200] [--interval-seconds 90] [--denominations 1] [--max-asset-raw 1600000] [--max-native-fee-raw 10000000000] [--output runs/name]\nDefaults to read-only preflight on public Paseo People. --run loads and holds recycler vouchers using //Bob. Stops on a deadline, a budget limit, an error, SIGINT/SIGTERM or a STOP file in its output directory. No automatic resume or resubmission.",
  );
  process.exit(0);
}
const duration = Number(values["duration-seconds"]);
const interval = Number(values["interval-seconds"]);
const denominations = values.denominations
  .split(",")
  .map((s) => (s.trim() === "" ? NaN : Number(s)));
const assetBudget = BigInt(values["max-asset-raw"]),
  feeBudget = BigInt(values["max-native-fee-raw"]);
if (feeBudget <= 0n) throw Error("Native fee budget must be positive");
// The pinned instance uses this unit; validate all bounds before opening a run.
const plan = recyclerPlan(
  duration,
  interval,
  denominations,
  10000n,
  assetBudget,
);
const endpoint = "wss://people-paseo.rotko.net";
const genesis =
  "0xe6c30d6e148f250b887105237bcaa5cb9f16dd203bf7b5b9d4f1da7387cb86ec";
const output = resolve(values.output);
mkdirSync(output, { recursive: true, mode: 0o700 });
writeFileSync(
  resolve(output, "run.json"),
  json({ ...values, endpoint, genesis, instance: 0, plan }),
  { flag: "wx", mode: 0o600 },
);
const receipts = resolve(output, "receipts.jsonl");
const record = (value: Record<string, unknown>) =>
  appendFileSync(
    receipts,
    JSON.stringify({ time: new Date().toISOString(), ...value }, (_, v) =>
      typeof v === "bigint" ? String(v) : v,
    ) + "\n",
    { mode: 0o600 },
  );
const client = createClient(getWsProvider(endpoint)),
  api = demoApi(client);
const pair = sr25519CreateDerive(
  entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)),
)("//Bob");
const address = AccountId().dec(pair.publicKey),
  signer = getPolkadotSigner(pair.publicKey, "Sr25519", pair.sign);
const options = {
  mortality: { mortal: true as const, period: 64 },
  customSignedExtensions: {
    VerifyMultiSignature: { value: { type: "Disabled", value: undefined } },
  },
};
let stopping = false,
  startedAt = 0,
  deadline = 0,
  loaded = 0,
  spent = 0n,
  fees = 0n,
  locked = false;
const lock = fileURLToPath(
  new URL("runs/.recycler-bot-public-bob.lock", import.meta.url),
);
const secrets: Array<{
  label: string;
  denomination: number;
  entropy: string;
  member: string;
}> = [];
for (const signal of ["SIGINT", "SIGTERM"] as const)
  process.on(signal, () => {
    stopping = true;
  });
const shouldStop = () => stopping || existsSync(resolve(output, "STOP"));
async function bounded<T>(
  label: string,
  promise: Promise<T>,
  milliseconds = 180000,
): Promise<T> {
  let timer: NodeJS.Timeout;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              Error(`${label} timed out; inspect receipts before any retry`),
            ),
          milliseconds,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer!);
  }
}
function status(state: string, reason?: string) {
  const data = {
    state,
    reason,
    pid: process.pid,
    endpoint,
    payer: address,
    instance: 0,
    denominations,
    startedAt: startedAt ? new Date(startedAt).toISOString() : null,
    endsAt: deadline ? new Date(deadline).toISOString() : null,
    loaded,
    assetSpent: spent,
    nativeFees: fees,
    maxLoads: plan.maxLoads,
    assetBudget,
    feeBudget,
  };
  const temp = resolve(output, "status.tmp");
  writeFileSync(temp, json(data));
  renameSync(temp, resolve(output, "status.json"));
}
async function waitUntil(time: number) {
  while (Date.now() < time && !shouldStop())
    await sleep(Math.min(1000, time - Date.now()));
}
async function observeMembership(member: string, denomination: number) {
  const collection = collectionId("recycler", 0, denomination),
    start = Date.now();
  while (Date.now() - start < 30000 && Date.now() < deadline && !shouldStop()) {
    const at = (await client.getFinalizedBlock()).hash;
    const location = await api.query.Members.Members.getValue(
      collection,
      member,
      { at },
    );
    if (location?.type === "Included") {
      const index = location.value.ring_index;
      const [root, state] = await Promise.all([
        api.query.Members.Root.getValue(collection, index, { at }),
        api.query.Members.RingKeysStatus.getValue(collection, index, { at }),
      ]);
      if (root) {
        const members: string[] = [];
        for (let page = 0; members.length < state.included; page++) {
          const keys = await api.query.Members.RingKeys.getValue(
            collection,
            index,
            page,
            { at },
          );
          if (!keys.length) break;
          members.push(...keys);
        }
        if (members.slice(0, state.included).includes(member)) {
          record({
            type: "membership-confirmed",
            member,
            denomination,
            at,
            index,
            revision: root.revision,
            included: state.included,
            elapsedMs: Date.now() - start,
          });
          return;
        }
      }
    }
    await sleep(3000);
  }
  record({
    type: "membership-pending",
    member,
    denomination,
    note: "Not confirmed during this observation window; subsequent loads may continue.",
  });
}
async function main() {
  const chain = await client.getChainSpecData();
  if (chain.genesisHash !== genesis) throw Error("Wrong chain genesis");
  const at = (await client.getFinalizedBlock()).hash;
  const [instance, minimum, maximum, native] = await Promise.all([
    api.query.Coinage.Instances.getValue(0, { at }),
    api.constants.Coinage.MinimumExponent(),
    api.constants.Coinage.MaximumExponent(),
    api.query.System.Account.getValue(address, { at }),
  ]);
  if (!instance || instance.asset_unit !== 10000n)
    throw Error("Unexpected Coinage instance/unit");
  if (denominations.some((d) => d < minimum || d > maximum))
    throw Error(`Denomination outside chain range ${minimum}..${maximum}`);
  const balance = await api.query.Assets.Account.getValue(
    instance.asset_id,
    address,
    { at },
  );
  const report = {
    chain,
    at,
    instance,
    payer: address,
    native: native.data.free,
    balance,
    plan,
    feeBudget,
  };
  writeFileSync(resolve(output, "preflight.json"), json(report));
  console.log(json(report));
  if (!values.run) {
    status("preflight");
    return;
  }
  if ((balance?.balance ?? 0n) < plan.total)
    throw Error("Insufficient backing asset for the complete plan");
  if (native.data.free < feeBudget)
    throw Error("Native balance is below the selected fee budget");
  mkdirSync(fileURLToPath(new URL("runs/", import.meta.url)), {
    recursive: true,
  });
  writeFileSync(lock, json({ pid: process.pid, output }), {
    flag: "wx",
    mode: 0o600,
  });
  locked = true;
  startedAt = Date.now();
  deadline = startedAt + duration * 1000;
  let nextAt = startedAt;
  record({ type: "started", startedAt, deadline, payer: address, plan });
  status("running");
  while (Date.now() < deadline && loaded < plan.maxLoads && !shouldStop()) {
    await waitUntil(Math.min(nextAt, deadline));
    if (Date.now() >= deadline || shouldStop()) break;
    const denomination = denominations[loaded % denominations.length],
      amount = plan.amounts[loaded % plan.amounts.length];
    const entropy = new Uint8Array(randomBytes(32)),
      member = hex(member_from_entropy(entropy)),
      label = `load-${loaded}-denomination-${denomination}`;
    const tx = api.tx.Coinage.load_recycler_with_external_asset({
      instance_id: 0,
      preservation: { type: "Preserve" },
      value: denomination,
      member_key: member,
      proof_of_ownership: hex(sign(entropy, pair.publicKey)),
    });
    const estimatedFee = await bounded(
      "fee estimate",
      tx.getEstimatedFees(pair.publicKey, options),
    );
    if (spent + amount > assetBudget || fees + estimatedFee * 2n > feeBudget) {
      record({ type: "budget-stop", estimatedFee, spent, fees });
      break;
    }
    if (Date.now() >= deadline || shouldStop()) break;
    secrets.push({ label, denomination, entropy: hex(entropy), member });
    const temp = resolve(output, "vouchers.tmp");
    writeFileSync(temp, json(secrets), { mode: 0o600 });
    renameSync(temp, resolve(output, "vouchers.json"));
    const encoded = await bounded("sign", tx.sign(signer, options));
    if (Date.now() >= deadline || shouldStop()) break;
    const submittedAt = Date.now(),
      txHash = hex(blake2b256(encoded));
    nextAt = submittedAt + interval * 1000;
    record({
      type: "submitted",
      label,
      denomination,
      amount,
      member,
      estimatedFee,
      txHash,
      extrinsic: hex(encoded),
    });
    let subscription: { unsubscribe(): void } | undefined;
    let result: TxFinalizedPayload;
    try {
      result = await bounded(
        label,
        new Promise<TxFinalizedPayload>((resolve, reject) => {
          subscription = client.submitAndWatch(encoded).subscribe({
            next(event) {
              record({
                type: "progress",
                label,
                elapsedMs: Date.now() - submittedAt,
                event,
              });
              if (event.type === "finalized") resolve(event);
            },
            error: reject,
          });
        }),
      );
    } finally {
      subscription?.unsubscribe();
    }
    const explorer = `https://dev.papi.how/explorer/${result.block.hash}#networkId=custom&endpoint=${encodeURIComponent(endpoint)}&tx=${result.block.index}`;
    record({
      type: "finalized",
      label,
      ...result,
      denomination,
      amount,
      member,
      elapsedMs: Date.now() - submittedAt,
      explorer,
    });
    if (!result.ok)
      throw Error(`Dispatch failed: ${json(result.dispatchError)}`);
    loaded++;
    spent += amount;
    const paid = result.events.find(
      (e: any) =>
        e.type === "TransactionPayment" &&
        e.value.type === "TransactionFeePaid",
    ) as any;
    if (!paid) throw Error("Cannot account for transaction fee; stopping");
    fees += BigInt(paid.value.value.actual_fee);
    console.log(
      `${label}: finalized #${result.block.number}-${result.block.index} ${txHash} ${explorer}`,
    );
    status("running");
    await bounded(
      "membership observation",
      observeMembership(member, denomination),
      60000,
    );
    if (fees > feeBudget)
      throw Error("Actual fees exceeded the configured budget");
  }
  // A full schedule remains alive until its stated end time, unless stopped or budget-limited.
  if (loaded === plan.maxLoads && !shouldStop()) await waitUntil(deadline);
  const reason = shouldStop()
    ? "requested-stop"
    : Date.now() >= deadline
      ? "deadline"
      : "budget";
  record({ type: "complete", reason, loaded, spent, fees });
  status("complete", reason);
}
bounded("bot", main(), (duration + 240) * 1000)
  .catch((error) => {
    stopping = true;
    record({ type: "error", message: String(error) });
    status("error", String(error));
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => {
    client.destroy();
    if (locked) unlinkSync(lock);
  });
