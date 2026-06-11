#!/usr/bin/env bash
# Sets Coinage's underlying asset to XTRNL on People.
# Must run after 03d (creates XTRNL as foreign asset on People).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Set Coinage underlying asset id on People"
underlying_asset_id=$(dot people.query.Coinage.UnderlyingAssetId)
if [ "$underlying_asset_id" == "undefined" ]; then
  echo "Underlying asset id not set, setting to XTRNL"
  xtrnl_foreign=$(people_foreign_location "$XTRNL_ASSET_ID")
  set_call=$(dot --encode people.tx.Coinage.set_underlying_asset_id "$xtrnl_foreign")
  dot people.tx.Sudo.sudo "$set_call" --from "$SIGNER_PEOPLE_SUDO"
else
  echo "Underlying asset id already set:"
  echo "$underlying_asset_id" | jq
fi
