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
