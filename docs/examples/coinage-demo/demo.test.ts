import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function cli(...args: string[]) {
  return spawnSync(process.execPath, ["--import", "tsx", "demo.ts", ...args], {
    cwd: import.meta.dirname,
    encoding: "utf8",
    timeout: 10000,
  });
}

test("help exits without connecting", () => {
  const result = cli("--help");
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Without --run, only reads chain state/);
});

test("public dev keys cannot target a production endpoint", () => {
  const result = cli("--run", "--endpoint", "wss://rpc.polkadot.io");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Use PreviewNet or a loopback/);
});

test("invalid traffic bounds fail before creating a run or connecting", () => {
  for (const args of [
    ["--holders", "101"],
    ["--denomination", "1.5"],
    ["--rounds", "0"],
  ]) {
    const result = cli(...args);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /must be an integer/);
  }
});

test("a new run cannot overwrite an existing voucher inventory", () => {
  const directory = mkdtempSync(join(tmpdir(), "coinage-demo-test-"));
  try {
    writeFileSync(join(directory, "run.json"), "{}");
    const result = cli("--run", "--output", directory);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /EEXIST/);
  } finally {
    rmSync(directory, { recursive: true });
  }
});
