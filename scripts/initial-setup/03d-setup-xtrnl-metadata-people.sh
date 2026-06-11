#!/usr/bin/env bash
# Creates XTRNL as a foreign asset on People and sets metadata.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create foreign XTRNL asset on People"
xtrnl_foreign=$(people_foreign_location "$XTRNL_ASSET_ID")
xtrnl_foreign_asset=$(dot people.query.Assets.Asset "$xtrnl_foreign")
if [ "$xtrnl_foreign_asset" == "undefined" ]; then
  echo "Foreign asset is not set, creating foreign asset"
  create_call=$(dot --encode people.tx.Assets.force_create "$xtrnl_foreign" "$ACCOUNT_ASSET_OWNER" true "$XTRNL_MIN_BALANCE")
  dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
else
  echo "Foreign asset on People:"
  echo "$xtrnl_foreign_asset" | jq
fi

echo "-> Set XTRNL metadata on People"
xtrnl_metadata=$(dot people.query.Assets.Metadata "$xtrnl_foreign")
if [ "$(echo "$xtrnl_metadata" | jq .decimals)" == "0" ]; then
  echo "XTRNL metadata is not set, setting metadata"
  dot people.tx.Assets.set_metadata "$xtrnl_foreign" "$XTRNL_NAME" "$XTRNL_SYMBOL" "$XTRNL_DECIMALS" --from "$SIGNER_ASSET_OWNER"
else
  echo "XTRNL metadata on People:"
  echo "$xtrnl_metadata" | jq
fi
