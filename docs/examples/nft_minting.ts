/**
 * Usage example — earn NFT claim credits in a game, then mint them on the Asset Hub,
 * printing each credit's stage as it moves between the chains.
 *
 * Needs the dual-chain network with HRMP open both ways (`pnpm run node:assethub`,
 * then `pnpm run bootstrap:assethub`), and no game in progress or scheduled.
 *
 *   pnpm run nft_minting
 *
 * Steps (see `docs/nft-minting.md` for the stages):
 *   1. Asset Hub: //Alice creates a Scarcity collection with one item and registers it
 *      for claims with `Random` selection. Set `COLLECTION=<id>` to reuse one.
 *   2. People: Sudo schedules a one-round game, the players sign up and sudo forces the
 *      shuffle, so the game does not wait for its registration phase.
 *   3. People: Every player reports every co-player as a person. Each `Person` vote awards
 *      a credit to the reported player. Sudo then ends reporting, and the script waits until
 *      the game's credits are final.
 *   4. Both: The script follows each credit from earned to claimable.
 *   5. Asset Hub: Each player claims each of their credits into a fresh purse key.
 *   6. Both: The script waits until the fully claimed trees are removed on the People chain.
 *
 * The players are dev accounts, `//Bob,//Charlie` by default. Override with
 * `PLAYERS=//Bob,//Charlie,//Dave`. At least two are needed for a group to have co-players.
 */
import { AccountId, Enum, FixedSizeBinary, type PolkadotSigner, type SS58String } from "polkadot-api";
import { connectAssetHub, connectPeople, customSignedExtensions, devSigner } from "./lib/client";
import { type CreditProof, creditRows, formatRows } from "./lib/nft-status";
import { signedSubmitter, sudoSubmitter } from "./lib/submit";

const PLAYERS = (process.env.PLAYERS ?? "//Bob,//Charlie").split(",");

// `new_game` drops a schedule whose registration has already started. With the default phase
// durations, registration starts `registration + shuffle + post_shuffle_margin` = 390 s before
// play time, so schedule further ahead than that. The forced shuffle makes the lead time free.
const LEAD_SECS = 450;

// The pallet stores the communication key as-is, so a placeholder works.
const COMMS_KEY = FixedSizeBinary.fromHex("0x" + "2a".repeat(65));

// How long to wait for each cross-chain step before giving up.
const TIMEOUT_MS = 10 * 60 * 1000;
const POLL_MS = 3000;

interface Player {
  uri: string;
  signer: PolkadotSigner;
  address: SS58String;
  /** The proofs of each tree block, kept once the People chain removes its root. */
  proofs: Map<number, CreditProof[]>;
  /** Tree blocks the player held before this run, left out of the tracking. */
  earlier: Set<number>;
}

const toAddress = (signer: PolkadotSigner) => AccountId(42).dec(signer.publicKey);
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function waitFor<T>(label: string, check: () => Promise<T | undefined>): Promise<T> {
  const deadline = Date.now() + TIMEOUT_MS;
  while (Date.now() < deadline) {
    const value = await check();
    if (value !== undefined) return value;
    await sleep(POLL_MS);
  }
  throw new Error(`Timed out waiting for ${label}`);
}

async function main() {
  const people = connectPeople();
  const assetHub = connectAssetHub();
  const sudo = devSigner(); // //Alice — sudo on People, collection owner on Asset Hub
  const sudoPeople = sudoSubmitter(people.api, sudo, { customSignedExtensions });
  const submitAssetHub = signedSubmitter(sudo);

  const players: Player[] = PLAYERS.map((uri) => {
    const signer = devSigner(uri);
    return { uri, signer, address: toAddress(signer), proofs: new Map(), earlier: new Set() };
  });
  if (players.length < 2) throw new Error("PLAYERS needs at least two accounts");

  // Rows of this run's credits only, so a rerun does not count the trees of an earlier one.
  const rowsOf = async (player: Player) =>
    (await creditRows(people.api, assetHub.api, Enum("Account", player.address), player.proofs)).filter(
      (row) => !player.earlier.has(row.block),
    );

  // Prints every player's rows whenever they change, and returns them.
  let lastPrinted = "";
  const track = async () => {
    const all = await Promise.all(players.map(async (player) => [player, await rowsOf(player)] as const));
    const text = all.map(([player, rows]) => `  ${player.uri}\n${formatRows(rows)}`).join("\n");
    if (text !== lastPrinted) {
      console.log(`[${new Date().toISOString()}]\n${text}`);
      lastPrinted = text;
    }
    return all;
  };

  try {
    // --- 1. collection on the Asset Hub --------------------------------------
    let collection = process.env.COLLECTION === undefined ? undefined : Number(process.env.COLLECTION);
    if (collection === undefined) {
      console.log("Creating a collection on the Asset Hub ...");
      const created = await submitAssetHub(assetHub.api.tx.Scarcity.create_collection(), "Scarcity.create_collection");
      collection = assetHub.api.event.Scarcity.CollectionCreated.filter(created.events)[0].collection;
      await submitAssetHub(
        assetHub.api.tx.Scarcity.define_item({ collection, transferability: Enum("Transferable"), metadata: [] }),
        "Scarcity.define_item",
      );
      // Only the collection owner can register it, so this is signed rather than sudo.
      await submitAssetHub(
        assetHub.api.tx.NftClaims.set_collection_minter({ collection, selection: Enum("Random") }),
        "NftClaims.set_collection_minter",
      );
    }
    console.log(`Claims mint into collection ${collection}.`);

    // --- 2. game on the People chain -----------------------------------------
    const [game, schedules] = await Promise.all([
      people.api.query.Game.Game.getValue(),
      people.api.query.Game.GameSchedules.getValue(),
    ]);
    if (game || schedules.length > 0) {
      throw new Error("A game is in progress or scheduled; run this on a chain with neither.");
    }
    for (const player of players) {
      const blocks = await people.api.apis.NftCreditsApi.nft_claim_credit_blocks(Enum("Account", player.address));
      player.earlier = new Set(blocks.map(([block]) => block));
    }

    // Chain time rather than local time, which a local network can drift from.
    const now = Number((await people.api.query.Timestamp.Now.getValue()) / 1000n);
    const playTime = now + LEAD_SECS;
    console.log(`Scheduling a game at ${new Date(playTime * 1000).toISOString()} ...`);
    await sudoPeople(
      people.api.tx.Game.schedule_games({
        games_schedules: [{ game_play_time: playTime, rounds: 1, max_group_size: players.length, airdrops: [] }],
      }).decodedCall,
      "Game.schedule_games",
    );
    const { index } = await waitFor("the game to open registration", async () => {
      const current = await people.api.query.Game.Game.getValue();
      return current?.state.type === "Registration" ? current : undefined;
    });
    console.log(`Game #${index} is in registration.`);

    for (const player of players) {
      await signedSubmitter(player.signer, { customSignedExtensions })(
        people.api.tx.Game.sign_up_with_account({ identifier_key: COMMS_KEY, airdrops: undefined }),
        `${player.uri} Game.sign_up_with_account`,
      );
    }
    await sudoPeople(people.api.tx.Game.testnet_force_start_shuffle().decodedCall, "Game.testnet_force_start_shuffle");

    // --- 3. reports ------------------------------------------------------------
    const { playerCount, groupSize } = await waitFor("the shuffle to finish", async () => {
      const current = await people.api.query.Game.Game.getValue();
      return current?.state.type === "Reporting"
        ? { playerCount: current.state.value.player_count, groupSize: current.max_group_size }
        : undefined;
    });
    // Groups follow `GroupsSetting` in the game pallet: Player `i` is in group `i % groups`,
    // whose members are `group, group + groups, …` below the player count.
    const groups = Math.ceil(playerCount / groupSize);
    console.log(`Reporting: ${playerCount} players in ${groups} group(s).`);

    for (const player of players) {
      const indices = await people.api.query.Game.PlayerToIndex.getValue(Enum("Account", player.address));
      if (!indices) throw new Error(`${player.uri} is not in the game`);
      // One inner vec per round, one entry per co-player in group order. All `Person`.
      const fullReport = indices.map((playerIndex) => {
        const group = playerIndex % groups;
        let members = 0;
        for (let i = 0; i < groupSize; i++) if (group + i * groups < playerCount) members++;
        return Array.from({ length: members - 1 }, () => Enum("Person"));
      });
      await signedSubmitter(player.signer, { customSignedExtensions })(
        people.api.tx.Game.report({ full_report: fullReport }),
        `${player.uri} Game.report`,
      );
    }

    // Attendance processing can award more credits after reporting. See "Award phases" in
    // `docs/nft-minting.md` for when a game's credits are final.
    await sudoPeople(people.api.tx.Game.testnet_force_end_reporting().decodedCall, "Game.testnet_force_end_reporting");
    await waitFor("the game's credits to be final", async () => {
      const current = await people.api.query.Game.Game.getValue();
      const final =
        current === undefined ||
        current.index !== index ||
        current.state.type === "Cancelling" ||
        (current.state.type === "PlayerProcess" && current.state.value.step.type === "Step2ClearIndices");
      return final ? true : undefined;
    });
    console.log(`The credits of game #${index} are final.`);

    // --- 4. earned -> claimable --------------------------------------------------
    console.log("Following the credits to the Asset Hub ...");
    await waitFor("every credit to become claimable", async () => {
      const all = await track();
      const rows = all.flatMap(([, rows]) => rows);
      const stuck = rows.find((row) => ["expired", "leaf mismatch"].includes(row.stage));
      if (stuck) throw new Error(`block ${stuck.block} cannot be claimed: ${stuck.stage}`);
      return rows.length > 0 && rows.every((row) => row.stage === "claimable") ? true : undefined;
    });

    // --- 5. claims ---------------------------------------------------------------
    for (const [player, rows] of await track()) {
      for (const row of rows) {
        const { credit, leaf_index, proof } = row.proof!;
        // A purse key holds one NFT, so each claim mints to its own derived key. The player can
        // sign for it later, e.g. to transfer the NFT.
        const purse = toAddress(devSigner(`${player.uri}//nft//${row.block}//${leaf_index}`));
        const claimed = await signedSubmitter(player.signer)(
          assetHub.api.tx.NftClaims.claim({
            claimant: Enum("Account"),
            block: row.block,
            credit,
            leaf_index,
            proof,
            collection,
            mint_to: purse,
          }),
          `${player.uri} NftClaims.claim (block ${row.block}, leaf ${leaf_index})`,
        );
        const [event] = assetHub.api.event.NftClaims.CreditClaimed.filter(claimed.events);
        const nft = await assetHub.api.query.Scarcity.NftsByOwner.getValue(purse);
        console.log(
          `    minted instance ${event.instance} of item ${event.item} to ${purse}` +
            (nft ? ` (owner query: instance ${nft.instance})` : " (owner query: none)"),
        );
      }
    }

    // --- 6. removal ----------------------------------------------------------------
    // A fully claimed tree is removed on the Asset Hub, which then tells the People chain.
    // A tree that also holds credits of accounts outside `PLAYERS` stays until those are claimed.
    console.log("Waiting for the People chain to remove the claimed trees ...");
    await waitFor("the trees to be removed", async () => {
      const rows = (await track()).flatMap(([, rows]) => rows);
      return rows.every((row) => row.blockState === "Removed") ? true : undefined;
    });
    console.log("Every credit is claimed and its tree removed.");
  } finally {
    people.client.destroy();
    assetHub.client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
