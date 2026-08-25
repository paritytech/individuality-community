#!/usr/bin/env bash
# Creates PAS/XTRNL liquidity pool on People and adds liquidity.
# Coinage converts its fees through this pool, so it cannot charge a fee in XTRNL without it.
# Must run after 03d (creates XTRNL as foreign asset on People).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

xtrnl_foreign=$(people_foreign_location "$XTRNL_ASSET_ID")
native_max=$(( 1000 * 10**NATIVE_DECIMALS ))
native_min=$(( 500 * 10**NATIVE_DECIMALS ))
xtrnl_max=$(( 4000 * 10**XTRNL_DECIMALS ))
xtrnl_min=$(( 2000 * 10**XTRNL_DECIMALS ))
xtrnl_fund=$(( xtrnl_max + XTRNL_MIN_BALANCE ))

echo "-> Create PAS/XTRNL liquidity pool on People"
xtrnl_pool=$(dot people.query.AssetConversion.Pools "[$NATIVE_TOKEN, $xtrnl_foreign]")
if [ "$xtrnl_pool" == "undefined" ]; then
  echo "XTRNL pool is not set, creating pool"
  dot people.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$xtrnl_foreign" --from "$SIGNER_ASSET_OWNER"
  xtrnl_pool=$(dot people.query.AssetConversion.Pools "[$NATIVE_TOKEN, $xtrnl_foreign]")
else
  echo "PAS/XTRNL pool is LPToken: $xtrnl_pool"
fi

echo "-> Check PAS/XTRNL pool liquidity on People"
# People exposes no AssetConversionApi, so the LP token supply stands in for the reserves.
lp_asset=$(dot people.query.PoolAssets.Asset "$xtrnl_pool" --output json)
if [ "$lp_asset" == "undefined" ]; then
  lp_supply="0"
else
  lp_supply=$(echo "$lp_asset" | jq -r ".supply // 0")
fi
if [ "$lp_supply" != "0" ]; then
  echo "Pool already has liquidity: LP supply $lp_supply"
else
  # The supply lives on AssetHub and no script bridges it, so mint the People side. Providing
  # the liquidity spends it, so this has to be driven by the pool rather than by the balance.
  owner_balance=$(assets_account_balance people "$xtrnl_foreign" "$ACCOUNT_ASSET_OWNER")
  if ! int_ge "$owner_balance" "$xtrnl_fund"; then
    shortfall=$(echo "$xtrnl_fund - $owner_balance" | bc)
    echo "Minting $shortfall units to asset owner $ACCOUNT_ASSET_OWNER"
    dot people.tx.Assets.mint "$xtrnl_foreign" "$ACCOUNT_ASSET_OWNER" "$shortfall" --from "$SIGNER_ASSET_OWNER"
  fi
  echo "Pool has no liquidity, adding liquidity (1:4 ratio)"
  dot people.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$xtrnl_foreign" $native_max $xtrnl_max $native_min $xtrnl_min "$ACCOUNT_ASSET_OWNER" --from "$SIGNER_ASSET_OWNER"
fi
