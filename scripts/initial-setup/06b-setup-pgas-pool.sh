#!/usr/bin/env bash
# Creates PAS/PGAS liquidity pool on AssetHub and adds liquidity.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create PAS/PGAS liquidity pool on AssetHub"
pgas_local=$(ah_local_location "$PGAS_ASSET_ID")
pgas_pool=$(dot asset-hub.query.AssetConversion.Pools "[$NATIVE_TOKEN, $pgas_local]")
if [ "$pgas_pool" == "undefined" ]; then
  echo "PGAS pool is not set, creating pool"
  dot asset-hub.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$pgas_local" --from "$SIGNER_AH_SUDO"
else
  echo "PAS/PGAS pool is LPToken: $pgas_pool"
fi

echo "-> Check PAS/PGAS pool liquidity on AssetHub"
pgas_reserves=$(dot asset-hub.apis.AssetConversionApi.get_reserves "$NATIVE_TOKEN" "$pgas_local" --json)
if [ "$pgas_reserves" == "undefined" ] || [ "$pgas_reserves" == "null" ]; then
  native_reserve="0"
else
  native_reserve=$(echo "$pgas_reserves" | jq -r '.[0] // "0"')
fi
native_max=$(( 1 * 10**NATIVE_DECIMALS ))
native_min=$(( native_max / 2 ))
pgas_max=100000000000
pgas_min=$(( pgas_max / 2 ))
if [ "$native_reserve" == "null" ] || [ "$native_reserve" -lt "$native_max" ]; then
  # Only claims and the asset admin can mint PGAS, so mint it as the admin.
  pgas_asset=$(dot asset-hub.query.Assets.Asset "$PGAS_ASSET_ID")
  pgas_admin=$(echo "$pgas_asset" | jq -r '.admin')
  pgas_min_balance=$(echo "$pgas_asset" | jq -r '.min_balance')
  # PGAS pays the AssetHub transaction fees, including this call's own, and the fee is taken
  # from the balance being supplied as liquidity. Keep a buffer well above one tx fee.
  pgas_fee_buffer=1000000000 # 1 PGAS
  pgas_mint=$(( pgas_max + pgas_min_balance + pgas_fee_buffer ))
  echo "Minting $pgas_mint PGAS to $ACCOUNT_AH_SUDO via the PGAS admin"
  mint_call=$(dot --encode asset-hub.tx.Assets.mint "$PGAS_ASSET_ID" "$ACCOUNT_AH_SUDO" "$pgas_mint")
  dot asset-hub.tx.Sudo.sudo_as "$pgas_admin" "$mint_call" --from "$SIGNER_AH_SUDO"
  echo "Pool liquidity insufficient (native=$native_reserve), adding liquidity (1:1e11 ratio)"
  dot asset-hub.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$pgas_local" $native_max $pgas_max $native_min $pgas_min "$ACCOUNT_AH_SUDO" --from "$SIGNER_AH_SUDO"
else
  echo "Pool already has sufficient liquidity: native=$native_reserve"
fi
