#!/usr/bin/env bash
# Creates PAS/dotUSD liquidity pool on AssetHub and adds liquidity.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create PAS/dotUSD liquidity pool on AssetHub"
dotusd_local=$(ah_local_location "$DOTUSD_ASSET_ID")
dotusd_pool=$(dot asset-hub.query.AssetConversion.Pools "[$NATIVE_TOKEN, $dotusd_local]")
if [ "$dotusd_pool" == "undefined" ]; then
  echo "dotUSD pool is not set, creating pool"
  dot asset-hub.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$dotusd_local" --from "$SIGNER_ASSET_OWNER"
else
  echo "PAS/dotUSD pool is LPToken: $dotusd_pool"
fi

echo "-> Check PAS/dotUSD pool liquidity on AssetHub"
dotusd_reserves=$(dot asset-hub.apis.AssetConversionApi.get_reserves "$NATIVE_TOKEN" "$dotusd_local" --json)
if [ "$dotusd_reserves" == "undefined" ] || [ "$dotusd_reserves" == "null" ]; then
  native_reserve="0"
else
  native_reserve=$(echo "$dotusd_reserves" | jq -r '.[0] // "0"')
fi
native_max=$(( 1000 * 10**NATIVE_DECIMALS ))
native_min=$(( 500 * 10**NATIVE_DECIMALS ))
dotusd_max=$(( 4000 * 10**DOTUSD_DECIMALS ))
dotusd_min=$(( 2000 * 10**DOTUSD_DECIMALS ))
if [ "$native_reserve" == "null" ] || [ "$native_reserve" -lt "$native_max" ]; then
  echo "Pool liquidity insufficient (native=$native_reserve), adding liquidity (1:4 ratio)"
  dot asset-hub.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$dotusd_local" $native_max $dotusd_max $native_min $dotusd_min "$ACCOUNT_ASSET_OWNER" --from "$SIGNER_ASSET_OWNER"
else
  echo "Pool already has sufficient liquidity: native=$native_reserve"
fi
