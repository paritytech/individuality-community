import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
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
  alias_in_context,
  member_from_entropy,
  sign,
  type RingExponent,
} from "verifiablejs/nodejs";
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
import { demoApi } from "./chain.js";

const { values } = parseArgs({
  options: {
    run: { type: "boolean", default: false },
    resume: { type: "boolean", default: false },
    output: { type: "string", default: `runs/reproduction-${Date.now()}` },
    "timeout-ms": { type: "string", default: "360000" },
    help: { type: "boolean", default: false },
  },
});
if (values.help) {
  console.log(
    "pnpm reproduce [--run] [--resume] [--output runs/name] [--timeout-ms 360000]\nReads Devnet by default. --run spends 320000 raw backing-asset units from //Bob, buys one unload token, then measures direct transfer, exact handoff, split/claim and recycling. This models chain operations, not phone messaging or UI.",
  );
  process.exit(0);
}
const timeout = Number(values["timeout-ms"]);
if (!Number.isSafeInteger(timeout) || timeout < 1000 || timeout > 900000)
  throw Error("timeout-ms must be between 1000 and 900000");
const endpoint = "wss://people-paseo.rotko.net";
const genesis =
  "0xe6c30d6e148f250b887105237bcaa5cb9f16dd203bf7b5b9d4f1da7387cb86ec";
const instanceId = 0,
  denomination = 5;
const output = resolve(values.output);
mkdirSync(output, { recursive: true, mode: 0o700 });
const runFile = resolve(output, "run.json"),
  receipts = resolve(output, "receipts.jsonl");
if (values.resume) {
  const previous = JSON.parse(readFileSync(runFile, "utf8"));
  if (previous.genesis !== genesis || previous.endpoint !== endpoint)
    throw Error("Run identity mismatch");
} else
  writeFileSync(
    runFile,
    json({ endpoint, genesis, instanceId, denomination }),
    { flag: "wx", mode: 0o600 },
  );
const lock = resolve(output, "running.lock");
writeFileSync(lock, String(process.pid), { flag: "wx", mode: 0o600 });
const history: any[] = existsSync(receipts)
  ? readFileSync(receipts, "utf8")
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((x) => JSON.parse(x))
  : [];
const record = (value: Record<string, unknown>) => {
  const entry = { time: new Date().toISOString(), ...value };
  appendFileSync(
    receipts,
    JSON.stringify(entry, (_, v) => (typeof v === "bigint" ? String(v) : v)) +
      "\n",
    { mode: 0o600 },
  );
  history.push(entry);
};
const client = createClient(getWsProvider(endpoint));
const api = demoApi(client);
const derive = sr25519CreateDerive(
  entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)),
);
const accounts = ["Alice", "Bob", "Charlie", "Dave", "Eve", "Ferdie"].map(
  (name) => {
    const pair = derive(`//${name}`);
    return {
      name,
      address: AccountId().dec(pair.publicKey),
      publicKey: pair.publicKey,
      signer: getPolkadotSigner(pair.publicKey, "Sr25519", pair.sign),
    };
  },
);
const [alice, bob, charlie, dave, eve, ferdie] = accounts;
let stopping = false;
process.on("SIGINT", () => {
  stopping = true;
  console.log("Stopping after the current operation");
});
const bounded = async <T>(label: string, promise: Promise<T>): Promise<T> => {
  let timer: NodeJS.Timeout;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () =>
            reject(
              Error(`${label} timed out; inspect receipts before retrying`),
            ),
          timeout,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer!);
  }
};
const coin = (address: string, at?: string) =>
  at
    ? api.query.Coinage.CoinsByOwner.getValue(address, { at })
    : api.query.Coinage.CoinsByOwner.getValue(address);
const explorer = (hash: string, index: number) =>
  `https://dev.papi.how/explorer/${hash}#networkId=custom&endpoint=${encodeURIComponent(endpoint)}&tx=${index}`;

async function submit(
  label: string,
  tx: Transaction<any, any>,
  signer: PolkadotSigner,
  asCoin = false,
) {
  if (stopping) throw Error("Stopped");
  const previous = history.findLast(
    (r) => r.label === label && r.type === "finalized",
  );
  if (previous) {
    if (!previous.ok) throw Error(`Previous failure: ${label}`);
    return previous;
  }
  if (history.some((r) => r.label === label && r.type === "submitted"))
    throw Error(`Unresolved submission: ${label}. Verify it before resuming.`);
  const started = Date.now();
  console.log(`Submitting ${label}`);
  const encoded = await bounded(
    "sign",
    tx.sign(signer, {
      mortality: { mortal: true, period: 64 },
      customSignedExtensions: {
        VerifyMultiSignature: { value: { type: "Disabled", value: undefined } },
        ...(asCoin ? { AsCoinage: { value: { type: "AsCoin" } } } : {}),
      },
    }),
  );
  record({
    type: "submitted",
    label,
    txHash: hex(blake2b256(encoded)),
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
              type: "transaction-progress",
              label,
              elapsedMs: Date.now() - started,
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
  const elapsedMs = Date.now() - started;
  record({
    type: "finalized",
    label,
    ...result,
    elapsedMs,
    explorer: explorer(result.block.hash, result.block.index),
  });
  console.log(
    `${label}: ${elapsedMs} ms, #${result.block.number}-${result.block.index} ${result.txHash}`,
  );
  if (!result.ok) throw Error(json(result.dispatchError));
  return result;
}

const secretFile = resolve(output, "vouchers.json");
const secrets: Record<string, { entropy: string; member: string }> = existsSync(
  secretFile,
)
  ? JSON.parse(readFileSync(secretFile, "utf8"))
  : {};
function voucher(label: string) {
  if (!secrets[label]) {
    const entropy = new Uint8Array(randomBytes(32));
    secrets[label] = {
      entropy: hex(entropy),
      member: hex(member_from_entropy(entropy)),
    };
    writeFileSync(secretFile, json(secrets), { mode: 0o600 });
  }
  return {
    entropy: bytes(secrets[label].entropy),
    member: secrets[label].member,
  };
}

async function waitRing(
  label: string,
  collection: string,
  member: string,
  exponent: RingExponent,
): Promise<Ring> {
  const started = Date.now();
  let lastStatus = "";
  while (Date.now() - started < timeout) {
    if (stopping) throw Error("Stopped");
    const at = (await bounded("finalized head", client.getFinalizedBlock()))
      .hash;
    const location = await bounded(
      "membership",
      api.query.Members.Members.getValue(collection, member, { at }),
    );
    let status, root;
    if (location?.type === "Included") {
      const index = location.value.ring_index;
      [status, root] = await bounded(
        "ring status",
        Promise.all([
          api.query.Members.RingKeysStatus.getValue(collection, index, { at }),
          api.query.Members.Root.getValue(collection, index, { at }),
        ]),
      );
      if (root && status.included > 0) {
        const members: string[] = [];
        for (let page = 0; members.length < status.included; page++) {
          const keys = await bounded(
            "ring keys",
            api.query.Members.RingKeys.getValue(collection, index, page, {
              at,
            }),
          );
          if (!keys.length)
            throw Error("Ring keys ended before included count");
          members.push(...keys);
        }
        members.length = status.included;
        if (members.includes(member)) {
          const ring = {
            index,
            revision: root.revision,
            commitment: root.root,
            members,
            exponent,
            at,
          };
          record({
            type: "ready",
            label,
            elapsedMs: Date.now() - started,
            included: status.included,
            ringIndex: index,
            revision: root.revision,
            at,
          });
          console.log(
            `${label}: included and usable in Fastest after ${Date.now() - started} ms`,
          );
          return ring;
        }
      }
    }
    const nextStatus = json({ location, status });
    if (nextStatus !== lastStatus) {
      record({
        type: "waiting",
        label,
        at,
        elapsedMs: Date.now() - started,
        location,
        status,
      });
      lastStatus = nextStatus;
    }
    await sleep(3000);
  }
  throw Error(`${label}: no confirmed recycler inclusion within ${timeout} ms`);
}

async function expectCoin(
  label: string,
  address: string,
  value: number,
  at: string,
) {
  const state = await bounded("coin state", coin(address, at));
  if (!state || state.instance_id !== instanceId || state.value !== value)
    throw Error(`Wrong coin state for ${label}: ${json(state)}`);
  record({ type: "coin-state", label, address, state, at });
}

async function main() {
  const chain = await bounded("connect", client.getChainSpecData());
  if (chain.genesisHash !== genesis)
    throw Error("Devnet genesis does not match the pinned chain");
  const instance = await api.query.Coinage.Instances.getValue(instanceId);
  if (!instance || instance.asset_unit !== 10000n)
    throw Error("Unexpected instance configuration");
  const balance = await api.query.Assets.Account.getValue(
    instance.asset_id,
    bob.address,
  );
  const existingCoins = await Promise.all(
    accounts.map(async (a) => ({ name: a.name, coin: await coin(a.address) })),
  );
  const report = {
    chain,
    instance,
    payer: bob.address,
    balance,
    loadAmount: assetAmount(instance.asset_unit, denomination),
    existingCoins,
  };
  writeFileSync(resolve(output, "preflight.json"), json(report));
  console.log(json(report));
  if (!values.run || history.some((r) => r.type === "complete")) return;
  if (
    !history.some((r) => r.type === "submitted") &&
    existingCoins.some((a) => a.coin)
  )
    throw Error("A test account already holds a coin; refusing to consume it");
  const loaded = history.some(
    (r) => r.type === "finalized" && r.label === "setup-load" && r.ok,
  );
  if (!loaded && (balance?.balance ?? 0n) < report.loadAmount)
    throw Error("Bob needs backing asset");
  const exponent = (value: { type: string }): RingExponent => {
    const n = Number(value.type.slice(3));
    if (n !== 9 && n !== 10 && n !== 14) throw Error("Unsupported ring size");
    return n;
  };
  const [recyclerExp, paidExp] = await Promise.all([
    api.constants.Coinage.RecyclerRingExponent(),
    api.constants.Coinage.PaidUnloadTokenRingExponent(),
  ]);
  const source = voucher("source"),
    token = voucher("paid-token");
  await submit(
    "setup-load",
    api.tx.Coinage.load_recycler_with_external_asset({
      instance_id: instanceId,
      preservation: { type: "Preserve" },
      value: denomination,
      member_key: source.member,
      proof_of_ownership: hex(sign(source.entropy, bob.publicKey)),
    }),
    bob.signer,
  );
  await submit(
    "setup-buy-token",
    api.tx.Coinage.pay_for_recycler_unload_fee_token_with_native({
      member_key: token.member,
      proof_of_ownership: hex(sign(token.entropy, bob.publicKey)),
    }),
    bob.signer,
  );
  if (
    !history.some(
      (r) => r.type === "finalized" && r.label === "setup-unload" && r.ok,
    )
  ) {
    const recycler = await waitRing(
      "setup-recycler",
      collectionId("recycler", instanceId, denomination),
      source.member,
      exponent(recyclerExp),
    );
    let paid: { period: number; ring: Ring } | undefined;
    for (const entry of (
      await api.query.Coinage.PaidTokenCollectionsCreated.getEntries()
    ).reverse()) {
      const period = new DataView(bytes(entry.keyArgs[0]).buffer).getUint32(
        0,
        false,
      );
      const id = collectionId("paid", period);
      if (await api.query.Members.Members.getValue(id, token.member)) {
        paid = {
          period,
          ring: await waitRing(
            "setup-paid-token",
            id,
            token.member,
            exponent(paidExp),
          ),
        };
        break;
      }
    }
    if (!paid) throw Error("Paid token collection not found");
    await submit(
      "setup-unload",
      api.tx.Coinage.unload_recycler_into_coin({
        instance_id: instanceId,
        aliases: [hex(alias_in_context(source.entropy, recyclerContext))],
        value: denomination,
        index: recycler.index,
        revision: recycler.revision,
        to: alice.address,
      }),
      paidUnloadSigner(
        recycler,
        source.entropy,
        paid.ring,
        token.entropy,
        paid.period,
      ),
    );
  }
  const direct = await submit(
    "direct-Alice-to-Bob",
    api.tx.Coinage.transfer({ to: bob.address }),
    alice.signer,
    true,
  );
  await expectCoin(
    "direct-received",
    bob.address,
    denomination,
    direct.block.hash,
  );

  // Bob hands his coin key to Charlie locally. Charlie's claim uses Bob's key.
  record({
    type: "handoff",
    label: "exact-match-Bob-to-Charlie",
    senderExtrinsicCount: 0,
    source: bob.address,
    recipient: charlie.address,
  });
  const exact = await submit(
    "exact-match-recipient-claim",
    api.tx.Coinage.transfer({ to: charlie.address }),
    bob.signer,
    true,
  );
  await expectCoin(
    "exact-match-received",
    charlie.address,
    denomination,
    exact.block.hash,
  );

  const split = await submit(
    "prepare-split-Charlie",
    api.tx.Coinage.split({
      split_into: [[denomination - 1, [dave.address, eve.address]]],
    }),
    charlie.signer,
    true,
  );
  await expectCoin(
    "prepared-payment-coin",
    dave.address,
    denomination - 1,
    split.block.hash,
  );
  await expectCoin(
    "sender-change",
    eve.address,
    denomination - 1,
    split.block.hash,
  );
  record({
    type: "handoff",
    label: "prepared-payment-to-Ferdie",
    source: dave.address,
    recipient: ferdie.address,
  });
  const claim = await submit(
    "prepared-recipient-claim",
    api.tx.Coinage.transfer({ to: ferdie.address }),
    dave.signer,
    true,
  );
  await expectCoin(
    "prepared-payment-received",
    ferdie.address,
    denomination - 1,
    claim.block.hash,
  );

  // Explicit recycling measures inclusion latency; Fastest would not voluntarily recycle this young coin.
  const recycled = voucher("recycled");
  await submit(
    "explicit-recycle-Ferdie",
    api.tx.Coinage.load_recycler_with_coin({
      member_key: recycled.member,
      proof_of_ownership: hex(sign(recycled.entropy, ferdie.publicKey)),
    }),
    ferdie.signer,
    true,
  );
  await waitRing(
    "recycled-Fastest-ready",
    collectionId("recycler", instanceId, denomination - 1),
    recycled.member,
    exponent(recyclerExp),
  );
  record({
    type: "complete",
    note: "One change coin remains at Eve; one voucher remains in the recycler. Phone transport and UI were not exercised.",
  });
}
main()
  .catch((error) => {
    record({ type: "error", message: String(error) });
    console.error(error);
    process.exitCode = 1;
  })
  .finally(() => {
    client.destroy();
    unlinkSync(lock);
  });
