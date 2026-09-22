import { buildAuthorizedChunkCall } from "./calls/chunks.ts";
import {
  coinageCreateSponsoredInstance,
  coinageCreateSufficientInstanceCall,
  coinageFundPalletAccount,
  coinageMakeInstanceSufficientCall,
  coinageNextInstanceId,
  waitForSufficientInstance,
} from "./calls/coinage.ts";
import { createDexPool } from "./calls/dex.ts";
import {
  buildExampleForeignAssetRegistration,
  buildExampleReserveAssetRegistration,
  waitForExampleForeignAsset,
} from "./calls/example-asset.ts";
import { openHrmpChannels, waitForHrmpChannel } from "./calls/hrmp.ts";
import { subscribeAssetHub } from "./calls/notifier.ts";
import { buildPeopleTransactCall, submitReferendum } from "./calls/referendum.ts";
import { disconnect } from "./helper/api.ts";
import { chunks } from "./helper/chunks.ts";
import { EXAMPLE_ASSET_LOCATION, NATIVE_ASSET_LOCATION } from "./helper/config.ts";
import { log } from "./helper/log.ts";
import { ORIGINS } from "./helper/origins.ts";
import { submitPeople } from "./helper/submit.ts";

async function prerequisites() {
  await openHrmpChannels();
  log(
    "Chunks",
    "submitting",
    `${chunks.length} pages: ${chunks.map(chunk => `${chunk.ringExponent}/${chunk.pageIndex}`).join(" ")}`,
  );
  await Promise.all(
    chunks.map(chunk =>
      submitPeople(
        `Chunks ${chunk.ringExponent}/${chunk.pageIndex}`,
        ORIGINS.authorized,
        buildAuthorizedChunkCall(chunk),
      ),
    ),
  );
  log("Chunks", "complete", `${chunks.length} pages finalized`);
  await waitForHrmpChannel();
  await subscribeAssetHub();
}

const COINAGE_PERMISSIONLESS = true;

async function main() {
  log("Initialization", "started");
  await prerequisites();

  // Asset Hub is the reserve chain for the example asset. People identifies the same asset by its
  // cross-chain location and stores it as a foreign asset.
  await submitReferendum("Referendum register example asset", [
    ...buildExampleReserveAssetRegistration(),
    await buildPeopleTransactCall(buildExampleForeignAssetRegistration()),
  ]);
  // Asset Hub finalizes the proposal before People processes its XCM message.
  await waitForExampleForeignAsset();

  // Coinage needs a liquid native/asset market to collect unload fees in the asset.
  await createDexPool(NATIVE_ASSET_LOCATION, EXAMPLE_ASSET_LOCATION);

  let instanceId: number;
  if (COINAGE_PERMISSIONLESS) {
    // Anyone can create a sponsored instance, but its sponsor must maintain the funding pool.
    instanceId = await coinageCreateSponsoredInstance();
    // Governance can remove the sponsor's funding obligation by making the instance sufficient.
    await submitReferendum("Referendum bless instance", [
      await buildPeopleTransactCall([coinageMakeInstanceSufficientCall(instanceId).decodedCall]),
    ]);
  } else {
    await coinageFundPalletAccount();
    instanceId = await coinageNextInstanceId();
    // Governance can also create a sufficient instance directly.
    await submitReferendum("Referendum create instance", [
      await buildPeopleTransactCall([coinageCreateSufficientInstanceCall().decodedCall]),
    ]);
  }

  await waitForSufficientInstance(instanceId);
  log("Initialization", "complete");
}

await main().finally(disconnect);
