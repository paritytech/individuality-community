import { test } from "node:test";
import assert from "node:assert/strict";
import { DEV_ACCOUNTS, selectDevAccount } from "./recycler-account.js";

test("six distinct public signers and legacy Bob address", () => {
  assert.equal(new Set(DEV_ACCOUNTS.map(n => selectDevAccount(n).address)).size, 6);
  assert.equal(selectDevAccount().address, "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty");
});
test("resume uses the saved account and supports old Bob checkpoints", () => {
  const alice = selectDevAccount("Alice");
  assert.equal(selectDevAccount(undefined, { devAccount: "Alice", payer: alice.address }).address, alice.address);
  assert.equal(selectDevAccount(undefined, { payer: selectDevAccount().address }).name, "Bob");
});
test("reject signer changes, mismatched payer and unsupported derivations", () => {
  const bob = { devAccount: "Bob", payer: selectDevAccount().address };
  assert.throws(() => selectDevAccount("Alice", bob), /Cannot change/);
  assert.throws(() => selectDevAccount("Alice", { payer: bob.payer }), /does not match/);
  assert.throws(() => selectDevAccount("Bob", { payer: "wrong" }), /does not match/);
  assert.throws(() => selectDevAccount("Ferdy"), /Unknown/);
  assert.throws(() => selectDevAccount("Alice//other"), /Unknown/);
});

import { mkdtempSync, writeFileSync, chmodSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DEV_PHRASE, entropyToMiniSecret, mnemonicToEntropy } from "@polkadot-labs/hdkd-helpers";
import { sr25519CreateDerive } from "@polkadot-labs/hdkd";
import { AccountId } from "polkadot-api";
import { selectBotAccount } from "./recycler-account.js";

test("private signer resumes by file without persisting the mnemonic and locks by key", () => {
  const dir = mkdtempSync(join(tmpdir(), "recycler-private-"));
  const file = join(dir, "accounts.json");
  const entry = { name: "Demo-1", mnemonic: DEV_PHRASE, address: AccountId().dec(sr25519CreateDerive(entropyToMiniSecret(mnemonicToEntropy(DEV_PHRASE)))("//PrivateTest").publicKey), crypto: "sr25519", derivationPath: "//PrivateTest" };
  try {
    writeFileSync(file, JSON.stringify({ accounts: [entry] }), { mode: 0o600 });
    const a = selectBotAccount("Demo-1", undefined, file);
    const saved = { devAccount: a.name, payer: a.address, signerSource: a.signerSource };
    assert.equal(selectBotAccount(undefined, saved).address, entry.address);
    assert.equal(a.lockName, `private-${Buffer.from(a.pair.publicKey).toString("hex")}`);
    assert.ok(!JSON.stringify(saved).includes(DEV_PHRASE));
    assert.throws(() => selectBotAccount("Demo-2", saved), /Cannot change/);
    assert.throws(() => selectBotAccount(undefined, { ...saved, payer: "wrong" }), /does not match/);
    assert.throws(() => selectBotAccount("Demo-1", { payer: entry.address }, file), /Cannot change/);
    writeFileSync(file, JSON.stringify({ accounts: [{ ...entry, address: selectDevAccount("Alice").address, derivationPath: "//Alice" }] }));
    assert.throws(() => selectBotAccount("Demo-1", undefined, file), /well-known dev account/);
    chmodSync(file, 0o644);
    assert.throws(() => selectBotAccount("Demo-1", undefined, file), /owner-only/);
    chmodSync(file, 0o600);
    writeFileSync(file, JSON.stringify({ accounts: [entry, entry] }));
    assert.throws(() => selectBotAccount("Demo-1", undefined, file), /not unique/);
    writeFileSync(file, JSON.stringify({ accounts: [{ ...entry, mnemonic: "invalid secret phrase" }] }));
    assert.throws(() => selectBotAccount("Demo-1", undefined, file), /^Error: Invalid private signer configuration$/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
