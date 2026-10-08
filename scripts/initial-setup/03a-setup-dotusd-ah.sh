#!/usr/bin/env bash
# Creates dotUSD asset on AssetHub and mints total supply.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create dotUSD asset($DOTUSD_ASSET_ID) on AssetHub if it does not exist"
dotusd_asset=$(dot asset-hub.query.Assets.Asset "$DOTUSD_ASSET_ID")
if [ "$dotusd_asset" == "undefined" ]; then
  echo "Asset $DOTUSD_ASSET_ID does not exist, creating..."
  create_call=$(dot --encode asset-hub.tx.Assets.force_create "$DOTUSD_ASSET_ID" "$ACCOUNT_ASSET_OWNER" true "$DOTUSD_MIN_BALANCE")
  dot asset-hub.tx.Sudo.sudo "$create_call" --from "$SIGNER_AH_SUDO"
else
  echo "Asset $DOTUSD_ASSET_ID already exists"
fi

echo "-> Mint dotUSD total supply on AssetHub"
dotusd_balance=$(dot asset-hub.query.Assets.Account "$DOTUSD_ASSET_ID" "$ACCOUNT_ASSET_OWNER")
if [ "$dotusd_balance" == "undefined" ]; then
  echo "dotUSD balance is undefined, minting"
  dot asset-hub.tx.Assets.mint "$DOTUSD_ASSET_ID" "$ACCOUNT_ASSET_OWNER" "$DOTUSD_TOTAL_SUPPLY" --from "$SIGNER_ASSET_OWNER"
else
  echo "dotUSD balance is $(echo "$dotusd_balance" | jq .balance) ($ACCOUNT_ASSET_OWNER)"
fi
