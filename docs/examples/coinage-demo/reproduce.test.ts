import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function cli(...args: string[]) {
  return spawnSync(
    process.execPath,
    ["--import", "tsx", "reproduce.ts", ...args],
    {
      cwd: import.meta.dirname,
      encoding: "utf8",
      timeout: 10000,
    },
  );
}

test("reproduction help exits without opening a run", () => {
  const result = cli("--help");
  assert.equal(result.status, 0);
  assert.match(result.stdout, /Reads Devnet by default/);
});

test("invalid wait bounds cannot start a live reproduction", () => {
  const directory = mkdtempSync(join(tmpdir(), "coinage-reproduction-"));
  try {
    for (const limit of ["0", "900001", "NaN"]) {
      const result = cli("--run", "--timeout-ms", limit, "--output", directory);
      assert.equal(result.status, 1);
      assert.match(result.stderr, /timeout-ms must be between/);
      assert.deepEqual(readdirSync(directory), []);
    }
  } finally {
    rmSync(directory, { recursive: true });
  }
});
