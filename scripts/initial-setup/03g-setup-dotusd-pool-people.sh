#!/usr/bin/env bash
# Creates PAS/dotUSD liquidity pool on People and adds liquidity.
# Coinage converts its fees through this pool, so it cannot charge a fee in dotUSD without it.
# Must run after 03d (creates dotUSD as foreign asset on People).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

dotusd_foreign=$(people_foreign_location "$DOTUSD_ASSET_ID")
native_max=$(( 1000 * 10**NATIVE_DECIMALS ))
native_min=$(( 500 * 10**NATIVE_DECIMALS ))
dotusd_max=$(( 4000 * 10**DOTUSD_DECIMALS ))
dotusd_min=$(( 2000 * 10**DOTUSD_DECIMALS ))
dotusd_fund=$(( dotusd_max + DOTUSD_MIN_BALANCE ))

echo "-> Create PAS/dotUSD liquidity pool on People"
dotusd_pool=$(dot people.query.AssetConversion.Pools "[$NATIVE_TOKEN, $dotusd_foreign]")
if [ "$dotusd_pool" == "undefined" ]; then
  echo "dotUSD pool is not set, creating pool"
  dot people.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$dotusd_foreign" --from "$SIGNER_ASSET_OWNER"
  dotusd_pool=$(dot people.query.AssetConversion.Pools "[$NATIVE_TOKEN, $dotusd_foreign]")
else
  echo "PAS/dotUSD pool is LPToken: $dotusd_pool"
fi

echo "-> Check PAS/dotUSD pool liquidity on People"
# People exposes no AssetConversionApi, so the LP token supply stands in for the reserves.
lp_asset=$(dot people.query.PoolAssets.Asset "$dotusd_pool" --output json)
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
  owner_balance=$(assets_account_balance people "$dotusd_foreign" "$ACCOUNT_ASSET_OWNER")
  if ! int_ge "$owner_balance" "$dotusd_fund"; then
    shortfall=$(echo "$dotusd_fund - $owner_balance" | bc)
    echo "Minting $shortfall units to asset owner $ACCOUNT_ASSET_OWNER"
    dot people.tx.Assets.mint "$dotusd_foreign" "$ACCOUNT_ASSET_OWNER" "$shortfall" --from "$SIGNER_ASSET_OWNER"
  fi
  echo "Pool has no liquidity, adding liquidity (1:4 ratio)"
  dot people.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$dotusd_foreign" $native_max $dotusd_max $native_min $dotusd_min "$ACCOUNT_ASSET_OWNER" --from "$SIGNER_ASSET_OWNER"
fi
