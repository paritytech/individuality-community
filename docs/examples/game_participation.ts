/**
 * Usage example — take part in a game (the open / signed end-user flow).
 *
 * Calls (plain signed origin, no sudo):
 *   - Game.sign_up_with_account({ identifier_key, airdrop })  — join the game
 *   - Game.report({ full_report })                            — report after playing
 *
 *   pnpm run game_participation
 *
 * Sign-up runs for real when a game is in its registration phase. `report` can
 * only be built/encoded here: a valid report needs your shuffled group (runtime
 * data) and must be sent during the reporting phase.
 */
import { AccountId, Enum, FixedSizeBinary } from "polkadot-api";
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { signedSubmitter } from "./lib/submit";

// The player's communication public key ([u8; 65]). The pallet stores it as-is
// (no on-chain validation), so a placeholder is fine for the example.
const COMMS_KEY = FixedSizeBinary.fromHex("0x" + "2a".repeat(65));

async function main() {
  const { client, api } = connectPeople();
  const signer = devSigner(); // //Alice — here acting as an ordinary player
  const me = AccountId(42).dec(signer.publicKey);
  const submitSigned = signedSubmitter(signer, { customSignedExtensions });

  try {
    const game = await api.query.Game.Game.getValue();
    if (!game) {
      console.log("No game in progress — schedule one first with `pnpm run game_scheduling`.");
      return;
    }
    console.log(`Game #${game.index} is in "${game.state.type}".`);

    // --- sign up -----------------------------------------------------------
    const alreadyIn = await api.query.Game.CommunicationIdentifiers.getValue(me);
    if (alreadyIn) {
      console.log("Already signed up for the current game, skipping.");
    } else if (game.state.type !== "Registration") {
      console.log(`Can't sign up: game is "${game.state.type}", not "Registration".`);
    } else {
      const signUpTx = api.tx.Game.sign_up_with_account({
        identifier_key: COMMS_KEY,
        // Option<AirdropVrfs>: for airdrops, undefined to skip them.
        airdrops: undefined,
      });
      console.log("Signing up ...");
      const r = await submitSigned(signUpTx, "sign_up_with_account");
      console.log(`Signed up in block ${r.block.hash}`);
    }

    // --- report ------------------------------------------------------------
    // full_report is Vec<Vec<Report>>: one inner array per round, one entry per
    // co-player in your group, each Person or NotPerson. The shape below is
    // illustrative — a real report mirrors your actual shuffled group.
    const fullReport = [
      [Enum("Person"), Enum("NotPerson")], // round 1
      [Enum("Person"), Enum("Person")], // round 2
    ];
    const reportTx = api.tx.Game.report({ full_report: fullReport });
    console.log("report call (encoded):", (await reportTx.getEncodedData()).asHex());
    console.log(
      "Submit `report` during the reporting phase, with one entry per co-player per round.",
    );
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
