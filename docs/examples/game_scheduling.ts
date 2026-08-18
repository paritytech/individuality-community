/**
 * Operations example — schedule a game.
 *
 * Call: Game.schedule_games(Vec<GameSchedule>), wrapped in Sudo.sudo because it
 * requires a manager origin (backed by sudo on this chain).
 *
 *   pnpm run game_scheduling
 *
 * Every field is typed from the chain metadata, so a wrong field name or type
 * is a compile error rather than a runtime decode failure.
 */
import { connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { sudoSubmitter } from "./lib/submit";

// Game parameters (same shape the web app uses).
const ROUNDS = 2;
const MAX_GROUP_SIZE = 6;

async function main() {
  const { client, api } = connectPeople();
  const signer = devSigner(); // //Alice — the dev sudo key on a local chain
  const sudo = sudoSubmitter(api, signer, { customSignedExtensions });

  try {
    const [existing, maxSchedules] = await Promise.all([
      api.query.Game.GameSchedules.getValue(),
      api.constants.Game.MaxGameSchedules(),
    ]);
    if (existing.length >= maxSchedules) {
      console.log(`Schedule full (${existing.length}/${maxSchedules}); nothing to do.`);
      return;
    }

    const playTime = Math.floor(Date.now() / 1000) + 7200; // 2h from now, unix seconds

    const gamesSchedules = [
      {
        game_play_time: playTime,
        rounds: ROUNDS,
        max_group_size: MAX_GROUP_SIZE,
        airdrops: [],
      },
    ];

    const scheduleTx = api.tx.Game.schedule_games({ games_schedules: gamesSchedules });

    console.log(`Scheduling game at ${playTime} (${new Date(playTime * 1000).toISOString()}) ...`);
    // schedule_games is gated by ManagerOrigin; dispatch it through root.
    const result = await sudo(scheduleTx.decodedCall, "Game.schedule_games");
    console.log(`Finalized in block ${result.block.hash}`);
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
