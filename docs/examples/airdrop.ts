/**
 * Operations example — enable an asset and schedule an airdrop event.
 *
 * Calls (gated by ManagerOrigin, dispatched through Sudo.sudo):
 *   - Airdrop.enable_asset({ asset_id, source })
 *   - Airdrop.schedule_event({ event_id, info })
 *
 *   pnpm run airdrop
 *
 * `schedule_event` needs the airdrop pot pre-funded with the prize
 * (`max_winners × asset_amount`). On a real chain that funding is an operator
 * step; here we scaffold it ourselves (create a test asset, mint, fund the pot)
 * so the example runs end to end.
 */
import { AccountId, Enum, FixedSizeBinary } from "polkadot-api";
import { XcmV5Junctions } from "@polkadot-api/descriptors";
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { signedSubmitter, sudoSubmitter } from "./lib/submit";

// A dedicated stable asset (XCM v5 Location, parents: 1, X3) — a fresh
// GeneralIndex so we don't collide with real on-chain assets.
const AH_PARA_ID = 1500;
const STABLE_ASSET = {
  parents: 1,
  interior: XcmV5Junctions.X3([
    { type: "Parachain", value: AH_PARA_ID },
    { type: "PalletInstance", value: 50 },
    { type: "GeneralIndex", value: 50_000_999n },
  ]),
};

const MIN_BALANCE = 1n;
const ASSET_AMOUNT = 1_000_000n; // prize per winner
const MAX_WINNERS = 10;
const PRIZE_TOTAL = ASSET_AMOUNT * BigInt(MAX_WINNERS);

// Permill is parts-per-million; 10% -> 100_000.
const percentToPermill = (pct: number) => Math.round(pct * 10_000);

// Derive a PalletId sovereign account the way `into_account_truncating` does:
// b"modl" ++ <8-byte PalletId> ++ zero padding, to 32 bytes.
function palletIdAccount(id: string): string {
  const bytes = new Uint8Array(32);
  bytes.set(new TextEncoder().encode("modl"), 0);
  bytes.set(new TextEncoder().encode(id), 4);
  return AccountId(42).dec(bytes);
}
const AIRDROP_POT = palletIdAccount("pop/adrp");

async function main() {
  const { client, api } = connectPeople();
  const signer = devSigner(); // //Alice — sudo key + asset owner/issuer we create below
  const me = AccountId(42).dec(signer.publicKey);
  // People chain carries a custom signed extension, so every submission needs it.
  const submitSigned = signedSubmitter(signer, { customSignedExtensions });
  const submitAsSudo = sudoSubmitter(api, signer, { customSignedExtensions });

  try {
    const now = Math.floor(Date.now() / 1000);
    const eventId = FixedSizeBinary.fromHex("0x" + "11".repeat(32));

    const enableTx = api.tx.Airdrop.enable_asset({ asset_id: STABLE_ASSET, source: me });
    const scheduleTx = api.tx.Airdrop.schedule_event({
      event_id: eventId,
      info: {
        prize: {
          asset_id: STABLE_ASSET,
          asset_amount: ASSET_AMOUNT, // u128
          max_winners: MAX_WINNERS, // u32
          winner_cap: percentToPermill(10), // 10% — max share a single winner can take
        },
        registration_starts: BigInt(now + 600), // u64 unix seconds
        draw_time: BigInt(now + 3600),
        end_time: BigInt(now + 7200),
      },
    });

    // --- test-only scaffolding (a provisioned chain already has this) ------
    console.log("Preparing test asset + pot funding ...");

    const assetExists = await api.query.Assets.Asset.getValue(STABLE_ASSET);
    if (!assetExists) {
      // force_create is root-gated; owner = us so we can mint as the issuer.
      await submitAsSudo(
        api.tx.Assets.force_create({
          id: STABLE_ASSET,
          owner: Enum("Id", me),
          is_sufficient: true,
          min_balance: MIN_BALANCE,
        }).decodedCall,
        "Assets.force_create",
      );
    }

    // Mint the existential deposit to ourselves (the enable_asset `source`)
    // and the full prize straight into the pot.
    await submitSigned(
      api.tx.Assets.mint({ id: STABLE_ASSET, beneficiary: Enum("Id", me), amount: MIN_BALANCE }),
      "Assets.mint -> source",
    );
    await submitSigned(
      api.tx.Assets.mint({ id: STABLE_ASSET, beneficiary: Enum("Id", AIRDROP_POT), amount: PRIZE_TOTAL }),
      "Assets.mint -> pot",
    );

    // --- the actual airdrop operations -------------------------------------
    const alreadyEnabled = await api.query.Airdrop.SupportedAssets.getValue(STABLE_ASSET);
    if (!alreadyEnabled) {
      await submitAsSudo(enableTx.decodedCall, "Airdrop.enable_asset");
    } else {
      console.log("  Airdrop.enable_asset: already enabled, skipping");
    }

    await submitAsSudo(scheduleTx.decodedCall, "Airdrop.schedule_event");

    const event = await api.query.Airdrop.Events.getValue(eventId);
    console.log("\nScheduled event on chain:", event ? "yes" : "no");
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
