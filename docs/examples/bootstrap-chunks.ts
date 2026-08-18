/**
 * Upload the R2e9 ring-verifier chunk pages to `ChunksManager` so the People
 * chain can actually build ring roots.
 *
 *   pnpm run chunks
 *
 * Without this, `ChunksManager.Chunks` is empty (genesis stores only page
 * hashes), every `Members::build_ring` aborts with `CouldNotPush`, no ring root
 * is produced, and the Asset Hub members-subscriber stays empty.
 *
 * `add_chunks` is a permissionless `#[pallet::authorize]` call (Authorized
 * origin — not sudo-able), so each page is submitted as a v5 general
 * transaction. Idempotent: pages already present on-chain are skipped.
 *
 * Pages are read from `CHUNKS_DIR` (default `./chunks/r2e9/`), produced by the
 * soak harness's `chunks-gen` tool — see
 * `internal/members-notifier-soak/scripts/gen-chunks.sh`, which writes them here.
 */
import { readFileSync } from "node:fs";
import { Binary, Enum } from "polkadot-api";
import { connectPeople, customSignedExtensions } from "./lib/client";
import { signedSubmitter } from "./lib/submit";
import { createGeneralSigner } from "./lib/general-signer";

const RING_EXPONENT = "R2e9";
const dir = new URL(process.env.CHUNKS_DIR ?? "./chunks/r2e9/", import.meta.url);

async function main() {
  const manifest = JSON.parse(readFileSync(new URL("manifest.json", dir), "utf8")) as {
    total_pages: number;
  };

  const { client, api } = connectPeople();
  // `add_chunks` takes an Authorized origin — submit as a general transaction.
  const authorized = signedSubmitter(createGeneralSigner(), { customSignedExtensions });

  console.log(`Uploading ${manifest.total_pages} ${RING_EXPONENT} chunk page(s) ...`);

  try {
    for (let page = 0; page < manifest.total_pages; page++) {
      const existing = await api.query.ChunksManager.Chunks.getValue(Enum(RING_EXPONENT), page);
      if (existing !== undefined) {
        console.log(`  page ${page}: already present, skipping.`);
        continue;
      }
      const bytes = new Uint8Array(readFileSync(new URL(`page-${page}.bin`, dir)));
      const tx = api.tx.ChunksManager.add_chunks({
        ring_exponent: Enum(RING_EXPONENT),
        page_index: page,
        encoded_chunks: Binary.fromBytes(bytes),
      });
      await authorized(tx, `add_chunks page ${page} (${bytes.length} bytes)`);
    }
    console.log("All chunk pages present. Ring building can now proceed.");
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
