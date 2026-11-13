/**
 * Shared NFT claim credit tracker: combines the People chain's view of a
 * claimant's tree blocks with the Asset Hub's view of each credit, and maps the
 * pair to a stage of `docs/nft-minting.md`.
 *
 * Reads runtime APIs only, so it needs no event history:
 *   - People: `NftCreditsApi.nft_claim_credit_blocks` and `nft_claim_credit_proofs`
 *   - Asset Hub: `NftClaimsApi.credit_statuses`
 */
import type { FixedSizeBinary } from "polkadot-api";
import type { connectAssetHub, connectPeople } from "./client";

type PeopleApi = ReturnType<typeof connectPeople>["api"];
type AssetHubApi = ReturnType<typeof connectAssetHub>["api"];

/** An `AccountOrPerson`: `Enum("Account", address)` or `Enum("Person", alias)`. */
export type Claimant = Parameters<PeopleApi["apis"]["NftCreditsApi"]["nft_claim_credit_blocks"]>[0];

/** The People chain's state of a tree block, from `nft_claim_credit_blocks`. */
export type BlockState = "Pending" | "Committed" | "Removed";

/**
 * How long after `tree.timestamp` a committed tree may stay off the Asset Hub before it shows
 * as delayed. A tree normally arrives within a minute of its first award.
 */
const DELAY_SECS = 5n * 60n;

/** The maximum number of credits in one `credit_statuses` call (`MAX_STATUS_QUERIES`). */
const MAX_STATUS_QUERIES = 256;

/** The stage of one credit. `docs/nft-minting.md` describes the stages. */
export type Stage =
  | "earned"
  | "in transit"
  | "delayed"
  | "expired"
  | "claimable"
  | "claimed"
  | "removed"
  | "leaf mismatch";

/** One inclusion proof from `nft_claim_credit_proofs`, which is what `claim` takes. */
export interface CreditProof {
  credit: FixedSizeBinary<32>;
  leaf_index: number;
  proof: FixedSizeBinary<32>[];
}

/** The stage of one credit, or of a whole block while its leaves are not known. */
export interface CreditRow {
  block: number;
  blockState: BlockState;
  /**
   * Absent while the block is pending, when it was removed before its proofs were read, or when
   * the People chain pruned its awards before its proofs were read.
   */
  proof?: CreditProof;
  stage: Stage;
  /** The claim deadline in seconds since the UNIX epoch, once the tree is committed. */
  expiresAt?: bigint;
}

/**
 * Reads the stage of every credit of `claimant`.
 *
 * `proofs` caches the proofs of each block. People serves proofs only while the block's root is
 * recorded, so the cache keeps a removed block's leaves known and its Asset Hub status readable.
 * Pass the same map on every call.
 */
export async function creditRows(
  people: PeopleApi,
  assetHub: AssetHubApi,
  claimant: Claimant,
  proofs: Map<number, CreditProof[]>,
): Promise<CreditRow[]> {
  const [blocks, ttl] = await Promise.all([
    people.apis.NftCreditsApi.nft_claim_credit_blocks(claimant),
    people.constants.NftCredits.ClaimsChainTreeTtl(),
  ]);

  // The People chain prunes a block's awards at the claim deadline (`AwardRetentionTtl`) but
  // keeps its root. A pruned block has therefore expired, and its leaves stay unknown.
  const pruned = new Set<number>();
  await Promise.all(
    blocks
      .filter(([block, state]) => state.type === "Committed" && !proofs.has(block))
      .map(async ([block]) => {
        const result = await people.apis.NftCreditsApi.nft_claim_credit_proofs(block, claimant);
        if (result.success) proofs.set(block, result.value);
        else if (result.value.type === "AwardsPruned") pruned.add(block);
      }),
  );

  // Every row of a committed or removed block with known leaves needs an Asset Hub status.
  const rows: CreditRow[] = [];
  const pending: { row: CreditRow; delayedAt?: bigint }[] = [];
  for (const [block, state] of blocks) {
    const blockState = state.type;
    if (state.type === "Pending") {
      rows.push({ block, blockState, stage: "earned" });
      continue;
    }
    const timestamp = state.type === "Committed" ? BigInt(state.value.timestamp) : undefined;
    const expiresAt = timestamp === undefined ? undefined : timestamp + ttl;
    const delayedAt = timestamp === undefined ? undefined : timestamp + DELAY_SECS;
    const known = proofs.get(block);
    if (!known) {
      const stage = blockState === "Removed" ? "removed" : pruned.has(block) ? "expired" : "in transit";
      rows.push({ block, blockState, stage, expiresAt });
      continue;
    }
    for (const proof of known) {
      const row: CreditRow = { block, blockState, proof, stage: "in transit", expiresAt };
      rows.push(row);
      pending.push({ row, delayedAt });
    }
  }

  const now = BigInt(Math.floor(Date.now() / 1000));
  for (let start = 0; start < pending.length; start += MAX_STATUS_QUERIES) {
    const batch = pending.slice(start, start + MAX_STATUS_QUERIES);
    const statuses = await assetHub.apis.NftClaimsApi.credit_statuses(
      batch.map(({ row }) => ({ block: row.block, leaf_index: row.proof!.leaf_index })),
    );
    batch.forEach(({ row, delayedAt }, i) => {
      const status = statuses[i];
      switch (status.type) {
        case "Claimable":
          row.stage = "claimable";
          row.expiresAt = status.value.expires_at;
          break;
        case "Claimed":
          row.stage = "claimed";
          break;
        case "LeafIndexOutOfBounds":
          row.stage = "leaf mismatch";
          break;
        case "NotStored":
          // Asset Hub holds no tree: still on its way, or turned away or swept after its deadline.
          if (row.blockState === "Removed") row.stage = "removed";
          else if (row.expiresAt !== undefined && now >= row.expiresAt) row.stage = "expired";
          else if (delayedAt !== undefined && now >= delayedAt) row.stage = "delayed";
          else row.stage = "in transit";
          break;
      }
    });
  }
  return rows;
}

/** One line per row, for a log that prints only when something changed. */
export function formatRows(rows: CreditRow[]): string {
  if (rows.length === 0) return "    (no credits)";
  return rows
    .map(({ block, proof, stage, expiresAt }) => {
      const leaf = proof ? `leaf ${proof.leaf_index}` : "leaves unknown";
      const deadline =
        expiresAt === undefined ? "" : `, deadline ${new Date(Number(expiresAt) * 1000).toISOString()}`;
      return `    block ${block}, ${leaf}: ${stage}${deadline}`;
    })
    .join("\n");
}
