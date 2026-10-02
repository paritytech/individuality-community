import { Binary, Enum } from "polkadot-api";

import { assetHubApi, peopleApi } from "../helper/api.ts";
import {
  EXAMPLE_ASSET_DECIMALS,
  EXAMPLE_ASSET_ID,
  EXAMPLE_ASSET_LOCATION,
  EXAMPLE_ASSET_MIN_BALANCE,
  EXAMPLE_ASSET_NAME,
  EXAMPLE_ASSET_OWNER_LOCAL,
  EXAMPLE_ASSET_SYMBOL,
} from "../helper/config.ts";
import { waitFor } from "../helper/wait.ts";

/** Build the calls that create the example asset on its reserve chain and set its metadata. */
export function buildExampleReserveAssetRegistration() {
  return [
    assetHubApi.tx.Assets.force_create({
      id: EXAMPLE_ASSET_ID,
      owner: Enum("Id", EXAMPLE_ASSET_OWNER_LOCAL),
      is_sufficient: true,
      min_balance: EXAMPLE_ASSET_MIN_BALANCE,
    }).decodedCall,
    assetHubApi.tx.Assets.force_set_metadata({
      id: EXAMPLE_ASSET_ID,
      name: Binary.fromText(EXAMPLE_ASSET_NAME),
      symbol: Binary.fromText(EXAMPLE_ASSET_SYMBOL),
      decimals: EXAMPLE_ASSET_DECIMALS,
      is_frozen: false,
    }).decodedCall,
  ];
}

/** Build the calls that register Asset Hub's example asset on People by its location. */
export function buildExampleForeignAssetRegistration() {
  return [
    peopleApi.tx.Assets.force_create({
      id: EXAMPLE_ASSET_LOCATION,
      owner: Enum("Id", EXAMPLE_ASSET_OWNER_LOCAL),
      is_sufficient: true,
      min_balance: EXAMPLE_ASSET_MIN_BALANCE,
    }).decodedCall,
    peopleApi.tx.Assets.force_set_metadata({
      id: EXAMPLE_ASSET_LOCATION,
      name: Binary.fromText(EXAMPLE_ASSET_NAME),
      symbol: Binary.fromText(EXAMPLE_ASSET_SYMBOL),
      decimals: EXAMPLE_ASSET_DECIMALS,
      is_frozen: false,
    }).decodedCall,
  ];
}

/** Wait until People has processed the registration sent from Asset Hub over XCM. */
export async function waitForExampleForeignAsset(): Promise<void> {
  await waitFor(
    "Example foreign asset on People",
    async () => (await peopleApi.query.Assets.Asset.getValue(EXAMPLE_ASSET_LOCATION)) !== undefined,
  );
}
