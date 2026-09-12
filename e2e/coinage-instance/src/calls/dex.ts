import { Enum } from "polkadot-api";

import { peopleApi } from "../helper/api.ts";
import {
  EXAMPLE_ASSET_OWNER_LOCAL,
  POOL_EXAMPLE_ASSET_LIQUIDITY,
  POOL_NATIVE_ASSET_LIQUIDITY,
} from "../helper/config.ts";
import { ORIGINS } from "../helper/origins.ts";
import { submitPeople } from "../helper/submit.ts";

type PoolAsset = Parameters<typeof peopleApi.tx.AssetConversion.create_pool>[0]["asset1"];

/** Create and seed the pool Coinage uses to convert asset fees to native currency.
 * Validation rejects asset-denominated fees with `CannotConvertAssetToNative` without this pool.
 * The batch prevents a failed setup from leaving an empty pool. */
export async function createDexPool(nativeAsset: PoolAsset, asset: PoolAsset): Promise<void> {
  const provider = EXAMPLE_ASSET_OWNER_LOCAL;
  const call = peopleApi.tx.Utility.batch_all({
    calls: [
      // Mint test liquidity locally. A production provider uses reserve-backed supply.
      peopleApi.tx.Assets.mint({
        id: asset,
        beneficiary: Enum("Id", provider),
        amount: POOL_EXAMPLE_ASSET_LIQUIDITY,
      }).decodedCall,
      peopleApi.tx.AssetConversion.create_pool({ asset1: nativeAsset, asset2: asset }).decodedCall,
      // An empty pool accepts the desired amounts exactly.
      peopleApi.tx.AssetConversion.add_liquidity({
        asset1: nativeAsset,
        asset2: asset,
        amount1_desired: POOL_NATIVE_ASSET_LIQUIDITY,
        amount2_desired: POOL_EXAMPLE_ASSET_LIQUIDITY,
        amount1_min: POOL_NATIVE_ASSET_LIQUIDITY,
        amount2_min: POOL_EXAMPLE_ASSET_LIQUIDITY,
        mint_to: provider,
      }).decodedCall,
    ],
  });
  await submitPeople("Dex pool", ORIGINS.liquidityProvider, call);
}
