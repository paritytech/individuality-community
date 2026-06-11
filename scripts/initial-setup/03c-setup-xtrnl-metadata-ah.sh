#!/usr/bin/env bash
# Sets XTRNL asset metadata (name, symbol, decimals) on AssetHub.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Set XTRNL metadata on AssetHub"
xtrnl_metadata=$(dot asset-hub.query.Assets.Metadata "$XTRNL_ASSET_ID")
if [ "$(echo "$xtrnl_metadata" | jq .decimals)" == "0" ]; then
  echo "XTRNL metadata is not set, setting metadata"
  dot asset-hub.tx.Assets.set_metadata "$XTRNL_ASSET_ID" "$XTRNL_NAME" "$XTRNL_SYMBOL" "$XTRNL_DECIMALS" --from "$SIGNER_ASSET_OWNER"
else
  echo "XTRNL metadata on AssetHub:"
  echo "$xtrnl_metadata" | jq
fi
