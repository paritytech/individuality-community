#!/usr/bin/env bash
# Creates XTRNL asset on AssetHub and mints total supply.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create XTRNL asset($XTRNL_ASSET_ID) on AssetHub if it does not exist"
xtrnl_asset=$(dot asset-hub.query.Assets.Asset "$XTRNL_ASSET_ID")
if [ "$xtrnl_asset" == "undefined" ]; then
  echo "Asset $XTRNL_ASSET_ID does not exist, creating..."
  create_call=$(dot --encode asset-hub.tx.Assets.force_create "$XTRNL_ASSET_ID" "$ACCOUNT_ASSET_OWNER" true "$XTRNL_MIN_BALANCE")
  dot asset-hub.tx.Sudo.sudo "$create_call" --from "$SIGNER_AH_SUDO"
else
  echo "Asset $XTRNL_ASSET_ID already exists"
fi

echo "-> Mint XTRNL total supply on AssetHub"
xtrnl_balance=$(dot asset-hub.query.Assets.Account "$XTRNL_ASSET_ID" "$ACCOUNT_ASSET_OWNER")
if [ "$xtrnl_balance" == "undefined" ]; then
  echo "XTRNL balance is undefined, minting"
  dot asset-hub.tx.Assets.mint "$XTRNL_ASSET_ID" "$ACCOUNT_ASSET_OWNER" "$XTRNL_TOTAL_SUPPLY" --from "$SIGNER_ASSET_OWNER"
else
  echo "XTRNL balance is $(echo "$xtrnl_balance" | jq .balance) ($ACCOUNT_ASSET_OWNER)"
fi
