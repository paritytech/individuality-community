import { test } from "node:test";
import assert from "node:assert/strict";
import {
  assetAmount,
  bytes,
  collectionId,
  encodeMembers,
  hex,
  recyclerContext,
} from "./protocol.js";
import {
  alias_in_context,
  member_from_entropy,
  one_shot,
  members_root,
  validate_with_commitment,
} from "verifiablejs/nodejs";

test("denominations use powers of two and reject lossy or invalid amounts", () => {
  assert.equal(assetAmount(10000n, 8), 2560000n);
  assert.equal(assetAmount(10000n, -4), 625n);
  assert.throws(() => assetAmount(10000n, -5), /precision/);
  for (const value of [128, -129, 1.5, NaN])
    assert.throws(() => assetAmount(10000n, value));
});
test("recycler identifiers distinguish instances, signed denominations and token periods", () => {
  const id = bytes(collectionId("recycler", 0x12345678, -1));
  assert.equal(
    new TextDecoder().decode(id.subarray(0, 16)),
    "coinage/recycler",
  );
  assert.deepEqual([...id.subarray(16, 21)], [0x78, 0x56, 0x34, 0x12, 255]);
  assert.notEqual(
    collectionId("recycler", 0, 8),
    collectionId("recycler", 1, 8),
  );
  assert.notEqual(collectionId("paid", 1), collectionId("recycler", 1));
});
test("proof library binds recycler membership to the call message and commitment", () => {
  const entropy = new Uint8Array(32).fill(7);
  const members = encodeMembers([hex(member_from_entropy(entropy))]);
  const message = new Uint8Array(32).fill(1);
  const proof = one_shot(10, entropy, members, recyclerContext, message);
  const root = members_root(10, members);
  assert.equal(
    hex(
      validate_with_commitment(10, proof.proof, root, recyclerContext, message),
    ),
    hex(alias_in_context(entropy, recyclerContext)),
  );
  assert.throws(() =>
    validate_with_commitment(
      10,
      proof.proof,
      root,
      recyclerContext,
      new Uint8Array(32).fill(2),
    ),
  );
});
