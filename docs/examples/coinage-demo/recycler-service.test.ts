import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("service status uses separate account labels and preserves legacy Bob", () => {
  const dir = mkdtempSync(join(tmpdir(), "coinage-service-"));
  try {
    const bin = join(dir, "bin");
    mkdirSync(bin);
    for (const account of [undefined, "Bob", "Alice", "Charlie", "Dave", "Eve", "Ferdie", "Demo-1"]) {
      const run = join(dir, account ?? "legacy");
      mkdirSync(run);
      writeFileSync(join(run, "run.json"), JSON.stringify({ devAccount: account, ...(account === "Demo-1" ? { signerSource: { file: "/private/accounts.json", name: account }, signerKey: "ab".repeat(32) } : {}) }));
      writeFileSync(join(run, "status.json"), JSON.stringify({ phase: "running", heartbeatAt: new Date().toISOString() }));
      // The wrapper redirects launchctl stderr, so emit a matching status field with its argument.
      writeFileSync(join(bin, "launchctl"), '#!/bin/sh\necho "  state = $2"\n', { mode: 0o755 });
      const result = spawnSync("sh", ["recycler-service.sh", "status", run], {
        encoding: "utf8", env: { ...process.env, NODE: process.execPath, PATH: `${bin}:${process.env.PATH}` },
      });
      assert.equal(result.status, 0, result.stderr);
      const suffix = account === "Demo-1" ? `.private-${"ab".repeat(32)}` : !account || account === "Bob" ? "" : `.${account.toLowerCase()}`;
      assert.match(result.stdout, new RegExp(`io\\.parity\\.coinage-demo\\.recycler-bot${suffix.replaceAll(".", "\\.")}\\n`));
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
