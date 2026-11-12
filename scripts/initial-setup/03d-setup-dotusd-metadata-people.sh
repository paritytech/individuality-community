#!/usr/bin/env bash
# Creates dotUSD as a foreign asset on People and sets metadata.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Create foreign dotUSD asset on People"
dotusd_foreign=$(people_foreign_location "$DOTUSD_ASSET_ID")
dotusd_foreign_asset=$(dot people.query.Assets.Asset "$dotusd_foreign")
if [ "$dotusd_foreign_asset" == "undefined" ]; then
  echo "Foreign asset is not set, creating foreign asset"
  create_call=$(dot --encode people.tx.Assets.force_create "$dotusd_foreign" "$ACCOUNT_ASSET_OWNER" true "$DOTUSD_MIN_BALANCE")
  dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
else
  echo "Foreign asset on People:"
  echo "$dotusd_foreign_asset" | jq
fi

echo "-> Set dotUSD metadata on People"
dotusd_metadata=$(dot people.query.Assets.Metadata "$dotusd_foreign")
if [ "$(echo "$dotusd_metadata" | jq .decimals)" == "0" ]; then
  echo "dotUSD metadata is not set, setting metadata"
  dot people.tx.Assets.set_metadata "$dotusd_foreign" "$DOTUSD_NAME" "$DOTUSD_SYMBOL" "$DOTUSD_DECIMALS" --from "$SIGNER_ASSET_OWNER"
else
  echo "dotUSD metadata on People:"
  echo "$dotusd_metadata" | jq
fi
