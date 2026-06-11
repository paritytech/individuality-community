#!/usr/bin/env bash
# Creates PAS/XTRNL liquidity pool on AssetHub and adds liquidity.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create PAS/XTRNL liquidity pool on AssetHub"
xtrnl_local=$(ah_local_location "$XTRNL_ASSET_ID")
xtrnl_pool=$(dot asset-hub.query.AssetConversion.Pools "[$NATIVE_TOKEN, $xtrnl_local]")
if [ "$xtrnl_pool" == "undefined" ]; then
  echo "XTRNL pool is not set, creating pool"
  dot asset-hub.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$xtrnl_local" --from "$SIGNER_ASSET_OWNER"
else
  echo "PAS/XTRNL pool is LPToken: $xtrnl_pool"
fi

echo "-> Check PAS/XTRNL pool liquidity on AssetHub"
xtrnl_reserves=$(dot asset-hub.apis.AssetConversionApi.get_reserves "$NATIVE_TOKEN" "$xtrnl_local" --json)
if [ "$xtrnl_reserves" == "undefined" ] || [ "$xtrnl_reserves" == "null" ]; then
  native_reserve="0"
else
  native_reserve=$(echo "$xtrnl_reserves" | jq -r '.[0] // "0"')
fi
native_max=$(( 1000 * 10**NATIVE_DECIMALS ))
native_min=$(( 500 * 10**NATIVE_DECIMALS ))
xtrnl_max=$(( 4000 * 10**XTRNL_DECIMALS ))
xtrnl_min=$(( 2000 * 10**XTRNL_DECIMALS ))
if [ "$native_reserve" == "null" ] || [ "$native_reserve" -lt "$native_max" ]; then
  echo "Pool liquidity insufficient (native=$native_reserve), adding liquidity (1:4 ratio)"
  dot asset-hub.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$xtrnl_local" $native_max $xtrnl_max $native_min $xtrnl_min "$ACCOUNT_ASSET_OWNER" --from "$SIGNER_ASSET_OWNER"
else
  echo "Pool already has sufficient liquidity: native=$native_reserve"
fi
