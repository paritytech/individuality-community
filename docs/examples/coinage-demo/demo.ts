import {
  mkdirSync,
  writeFileSync,
  appendFileSync,
  readFileSync,
  existsSync,
  unlinkSync,
  renameSync,
} from "node:fs";
import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import { parseArgs } from "node:util";
import { setTimeout as sleep } from "node:timers/promises";
import {
  AccountId,
  createClient,
  type PolkadotSigner,
  type Transaction,
  type TxFinalizedPayload,
} from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";
import { getPolkadotSigner } from "polkadot-api/signer";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import {
  blake2b256,
  DEV_PHRASE,
  entropyToMiniSecret,
  mnemonicToEntropy,
} from "@polkadot-labs/hdkd-helpers";
import {
  member_from_entropy,
  sign,
  alias_in_context,
  type RingExponent,
} from "verifiablejs/nodejs";
import { demoApi } from "./chain.js";
import {
  assetAmount,
  bytes,
  collectionId,
  hex,
  json,
  paidUnloadSigner,
  recyclerContext,
  type Ring,
} from "./protocol.js";

const { values } = parseArgs({
  options: {
    run: { type: "boolean", default: false },
    resume: { type: "boolean", default: false },
    endpoint: {
      type: "string",
      default: "wss://previewnet.substrate.dev/people",
    },
    instance: { type: "string", default: "0" },
    denomination: { type: "string", default: "8" },
    holders: { type: "string", default: "10" },
    rounds: { type: "string", default: "1" },
    "interval-ms": { type: "string", default: "2000" },
    "timeout-ms": { type: "string", default: "240000" },
    output: {
      type: "string",
      default: `runs/${new Date().toISOString().replaceAll(":", "-")}`,
    },
    help: { type: "boolean", default: false },
  },
});
if (values.help) {
  console.log(
    "pnpm demo [--run] [--resume] [--endpoint ws(s)://…] [--instance 0] [--denomination 8] [--holders 10] [--rounds 1] [--interval-ms 2000] [--timeout-ms 240000] [--output runs/name]\nWithout --run, only reads chain state. --run loads holders, unloads a coin, passes it through five dev accounts and recycles it each round.",
  );
  process.exit(0);
}
function integer(
  name:
    | "instance"
    | "denomination"
    | "holders"
    | "rounds"
    | "interval-ms"
    | "timeout-ms",
  min: number,
  max: number,
) {
  const n = Number(values[name]);
  if (!Number.isSafeInteger(n) || n < min || n > max)
    throw new Error(`--${name} must be an integer in [${min}, ${max}]`);
  return n;
}
const instanceId = integer("instance", 0, 0xffffffff);
const denomination = integer("denomination", -128, 127);
const holders = integer("holders", 1, 100);
const rounds = integer("rounds", 1, 1000);
const interval = integer("interval-ms", 0, 3600000);
const timeout = integer("timeout-ms", 1000, 3600000);
const endpoint = new URL(values.endpoint);
if (
  endpoint.href !== "wss://previewnet.substrate.dev/people" &&
  !["localhost", "127.0.0.1", "[::1]"].includes(endpoint.hostname)
) {
  throw new Error(
    "Use PreviewNet or a loopback PreviewNet engine endpoint with these public dev keys",
  );
}
const output = resolve(values.output);
mkdirSync(output, { recursive: true, mode: 0o700 });
const receiptFile = resolve(output, "receipts.jsonl");
const runFile = resolve(output, "run.json");
if (values.resume) {
  const previous = JSON.parse(readFileSync(runFile, "utf8"));
  for (const key of [
    "endpoint",
    "instance",
    "denomination",
    "holders",
    "rounds",
  ] as const) {
    if (previous[key] !== values[key])
      throw new Error(`Resume requires the original --${key}`);
  }
} else writeFileSync(runFile, json(values), { flag: "wx", mode: 0o600 });
const lockFile = resolve(output, "running.lock");
writeFileSync(lockFile, String(process.pid), { flag: "wx", mode: 0o600 });
const history: Array<Record<string, any>> =
  values.resume && existsSync(receiptFile)
    ? readFileSync(receiptFile, "utf8")
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line))
    : [];
const finalized = new Map<string, TxFinalizedPayload>(
  history
    .filter((r) => r.type === "finalized")
    .map((r) => [r.label, r as unknown as TxFinalizedPayload]),
);

const client = createClient(getWsProvider(endpoint.href));
const api = demoApi(client); // Resolve call and storage codecs from the connected runtime.
const derive = sr25519CreateDerive(
  entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)),
);
const accounts = ["Alice", "Bob", "Charlie", "Dave", "Ferdie"].map((name) => {
  const pair = derive(`//${name}`);
  return {
    name,
    publicKey: pair.publicKey,
    address: AccountId().dec(pair.publicKey),
    signer: getPolkadotSigner(pair.publicKey, "Sr25519", pair.sign),
  };
});
const alice = accounts[0];
const extensions = {
  VerifyMultiSignature: { value: { type: "Disabled", value: undefined } },
};
let stopped = false;
process.on("SIGINT", () => {
  stopped = true;
  console.log(
    "Stopping after the current operation; receipts and vouchers are saved.",
  );
});
function checkStop() {
  if (stopped) throw new Error("Stopped by SIGINT");
}
async function bounded<T>(label: string, operation: Promise<T>): Promise<T> {
  let timer: NodeJS.Timeout;
  try {
    return await Promise.race([
      operation,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error(
                `${label} timed out. A submitted transaction may still finalize; inspect receipts and chain state before another run.`,
              ),
            ),
          timeout,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer!);
  }
}
function record(value: unknown) {
  appendFileSync(
    receiptFile,
    JSON.stringify(value, (_, v) =>
      typeof v === "bigint" ? v.toString() : v,
    ) + "\n",
    { mode: 0o600 },
  );
}
async function submit(
  label: string,
  tx: Transaction<any, any>,
  signer: PolkadotSigner,
  extra = {},
) {
  checkStop();
  const previous = finalized.get(label);
  if (previous) {
    if (!previous.ok)
      throw new Error(`Previous dispatch failed: ${label}; inspect receipts`);
    console.log(`Already finalized: ${label}`);
    return previous;
  }
  if (
    history.findLast(
      (r) => r.label === label && ["submitted", "rejected"].includes(r.type),
    )?.type === "submitted"
  ) {
    throw new Error(
      `Unresolved submission for ${label}; verify its hash on-chain before resuming`,
    );
  }
  console.log(`Submitting ${label}`);
  const encoded = await bounded(
    "sign",
    tx.sign(signer, {
      mortality: { mortal: true, period: 8 },
      customSignedExtensions: { ...extensions, ...extra },
    }),
  );
  record({
    type: "submitted",
    label,
    txHash: hex(blake2b256(encoded)),
    extrinsic: hex(encoded),
    time: new Date().toISOString(),
  });
  let result: TxFinalizedPayload;
  try {
    result = await bounded(label, client.submit(encoded));
  } catch (error) {
    if (error instanceof Error && error.name === "InvalidTxError")
      record({ type: "rejected", label, error: String(error) });
    throw error;
  }
  record({ type: "finalized", label, ...result });
  console.log(
    `${result.ok ? "Finalized" : "FAILED"} ${label}: #${result.block.number}-${result.block.index} ${result.txHash}`,
  );
  if (!result.ok) throw new Error(json(result.dispatchError));
  return result;
}
const secretsFile = resolve(output, "vouchers.json");
const secrets: Array<{ label: string; entropy: string; member: string }> =
  values.resume && existsSync(secretsFile)
    ? JSON.parse(readFileSync(secretsFile, "utf8"))
    : [];
function voucher(label: string) {
  const saved = secrets.find((v) => v.label === label);
  if (saved) return { entropy: bytes(saved.entropy), member: saved.member };
  const entropy = new Uint8Array(randomBytes(32));
  const member = hex(member_from_entropy(entropy));
  secrets.push({ label, entropy: hex(entropy), member });
  writeFileSync(`${secretsFile}.tmp`, json(secrets), { mode: 0o600 });
  renameSync(`${secretsFile}.tmp`, secretsFile);
  return { entropy, member };
}
async function ring(
  collection: string,
  member: string,
  exponent: RingExponent,
  minimum = 1,
): Promise<Ring> {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    checkStop();
    const at = (await bounded("finalized block", client.getFinalizedBlock()))
      .hash;
    const location = await bounded(
      "member status",
      api.query.Members.Members.getValue(collection, member, { at }),
    );
    if (location?.type === "Included") {
      const index = location.value.ring_index;
      const [root, status] = await bounded(
        "ring root",
        Promise.all([
          api.query.Members.Root.getValue(collection, index, { at }),
          api.query.Members.RingKeysStatus.getValue(collection, index, { at }),
        ]),
      );
      if (root && status.included >= minimum) {
        const members: string[] = [];
        for (let page = 0; members.length < status.included; page++) {
          const keys = await bounded(
            "ring page",
            api.query.Members.RingKeys.getValue(collection, index, page, {
              at,
            }),
          );
          if (!keys.length)
            throw new Error("Ring pages ended before included member count");
          members.push(...keys);
        }
        members.length = status.included; // RingKeys also contains keys not yet committed.
        if (members.includes(member))
          return {
            index,
            revision: root.revision,
            members,
            commitment: root.root,
            exponent,
            at,
          };
      }
    }
    await sleep(3000);
  }
  throw new Error(`Ring membership timed out for ${member} in ${collection}`);
}
async function paidRing(member: string, exponent: RingExponent) {
  // PaidTokenCollectionsCreated uses big endian period keys. Discover the period
  // from membership rather than assuming the runtime's period duration.
  const collections =
    await api.query.Coinage.PaidTokenCollectionsCreated.getEntries();
  for (const entry of collections.reverse()) {
    const period = new DataView(bytes(entry.keyArgs[0]).buffer).getUint32(
      0,
      false,
    );
    const id = collectionId("paid", period);
    if (await api.query.Members.Members.getValue(id, member))
      return { period, ring: await ring(id, member, exponent) };
  }
  throw new Error("Paid token has no membership after finalized registration");
}
async function main() {
  const block = await bounded("connect", client.getFinalizedBlock());
  const chain = await bounded("chain identity", client.getChainSpecData());
  const identityFile = resolve(output, "chain.json");
  if (existsSync(identityFile)) {
    const previous = JSON.parse(readFileSync(identityFile, "utf8"));
    if (previous.genesisHash !== chain.genesisHash)
      throw new Error(
        "The chain genesis changed; do not reuse this run's inventory",
      );
  } else writeFileSync(identityFile, json(chain), { flag: "wx" });
  const [instance, min, max, recyclerExponent, paidExponent] = await bounded(
    "configuration",
    Promise.all([
      api.query.Coinage.Instances.getValue(instanceId),
      api.constants.Coinage.MinimumExponent(),
      api.constants.Coinage.MaximumExponent(),
      api.constants.Coinage.RecyclerRingExponent(),
      api.constants.Coinage.PaidUnloadTokenRingExponent(),
    ]),
  );
  if (!instance)
    throw new Error(`Coinage instance ${instanceId} does not exist`);
  if (denomination < min || denomination > max)
    throw new Error(`Denomination must be in [${min}, ${max}]`);
  const exponent = (value: { type: string }): RingExponent => {
    const n = Number(value.type.slice(3));
    if (n !== 9 && n !== 10 && n !== 14)
      throw new Error(`Unsupported ring exponent ${value.type}`);
    return n;
  };
  const amount = assetAmount(instance.asset_unit, denomination);
  const balance = await api.query.Assets.Account.getValue(
    instance.asset_id,
    alice.address,
  );
  const accountState = await Promise.all(
    accounts.map(async (a) => ({
      name: a.name,
      address: a.address,
      system: await api.query.System.Account.getValue(a.address),
      coin: await api.query.Coinage.CoinsByOwner.getValue(a.address),
    })),
  );
  const report = {
    endpoint: endpoint.href,
    block,
    instanceId,
    instance,
    denomination,
    amount,
    totalLoad: amount * BigInt(holders),
    balance,
    accounts: accountState,
  };
  writeFileSync(resolve(output, "preflight.json"), json(report));
  console.log(json(report));
  if (!values.run) return;
  if (history.some((r) => r.type === "recycled" && r.round === rounds - 1)) {
    console.log(`Run already completed. Receipts: ${receiptFile}`);
    return;
  }
  if ((balance?.balance ?? 0n) < amount * BigInt(holders))
    throw new Error("Alice needs more of the backing asset");
  if (!values.resume && accountState.some((a) => a.coin))
    throw new Error(
      "A dev account already owns a coin; use its existing run/state before starting another run",
    );
  const recyclerId = collectionId("recycler", instanceId, denomination);
  const inventory = Array.from({ length: holders }, (_, i) =>
    voucher(`holder-${i}`),
  );
  // Small atomic batches provide background holdings without flooding the pool.
  for (let i = 0; i < inventory.length; i += 5) {
    const calls = inventory.slice(i, i + 5).map(
      (v) =>
        api.tx.Coinage.load_recycler_with_external_asset({
          instance_id: instanceId,
          preservation: { type: "Preserve" },
          value: denomination,
          member_key: v.member,
          proof_of_ownership: hex(sign(v.entropy, alice.publicKey)),
        }).decodedCall,
    );
    await submit(
      `load holders ${i}..${Math.min(i + 5, holders) - 1}`,
      api.tx.Utility.batch_all({ calls }),
      alice.signer,
    );
  }
  let current = inventory[0];
  for (let round = 0; round < rounds; round++) {
    const token = voucher(`paid-token-${round}`);
    await submit(
      `buy unload token ${round}`,
      api.tx.Coinage.pay_for_recycler_unload_fee_token_with_native({
        member_key: token.member,
        proof_of_ownership: hex(sign(token.entropy, alice.publicKey)),
      }),
      alice.signer,
    );
    console.log("Waiting for finalized recycler and paid-token rings");
    const paid = await paidRing(token.member, exponent(paidExponent));
    const recycler = await ring(
      recyclerId,
      current.member,
      exponent(recyclerExponent),
      Math.min(holders, 10),
    );
    record({ type: "rings", round, recycler, paid });
    await submit(
      `unload into Alice ${round}`,
      api.tx.Coinage.unload_recycler_into_coin({
        instance_id: instanceId,
        aliases: [hex(alias_in_context(current.entropy, recyclerContext))],
        value: denomination,
        index: recycler.index,
        revision: recycler.revision,
        to: alice.address,
      }),
      paidUnloadSigner(
        recycler,
        current.entropy,
        paid.ring,
        token.entropy,
        paid.period,
      ),
    );
    for (let i = 0; i < accounts.length; i++) {
      const from = accounts[i],
        to = accounts[(i + 1) % accounts.length];
      const result = await submit(
        `${from.name} -> ${to.name} (round ${round})`,
        api.tx.Coinage.transfer({ to: to.address }),
        from.signer,
        { AsCoinage: { value: { type: "AsCoin" } } },
      );
      const coin = await api.query.Coinage.CoinsByOwner.getValue(to.address, {
        at: result.block.hash,
      });
      if (
        !coin ||
        coin.instance_id !== instanceId ||
        coin.value !== denomination ||
        coin.age !== i + 1
      )
        throw new Error("Coin transfer state verification failed");
      record({
        type: "coin-state",
        owner: to.address,
        coin,
        block: result.block,
      });
      await sleep(interval);
    }
    current = voucher(`recycled-${round}`);
    await submit(
      `recycle Alice ${round}`,
      api.tx.Coinage.load_recycler_with_coin({
        member_key: current.member,
        proof_of_ownership: hex(sign(current.entropy, alice.publicKey)),
      }),
      alice.signer,
      { AsCoinage: { value: { type: "AsCoin" } } },
    );
    const finalRing = await ring(
      recyclerId,
      current.member,
      exponent(recyclerExponent),
    );
    record({
      type: "recycled",
      round,
      member: current.member,
      ring: finalRing,
    });
  }
  console.log(
    `Completed ${rounds} round(s). ${holders} vouchers remain held in the recycler. Receipts: ${receiptFile}`,
  );
}
main()
  .catch((error) => {
    record({ type: "error", message: String(error) });
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => {
    client.destroy();
    unlinkSync(lockFile);
  });
