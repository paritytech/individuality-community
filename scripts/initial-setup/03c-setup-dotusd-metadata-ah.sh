#!/usr/bin/env bash
# Sets dotUSD asset metadata (name, symbol, decimals) on AssetHub.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Set dotUSD metadata on AssetHub"
dotusd_metadata=$(dot asset-hub.query.Assets.Metadata "$DOTUSD_ASSET_ID")
if [ "$(echo "$dotusd_metadata" | jq .decimals)" == "0" ]; then
  echo "dotUSD metadata is not set, setting metadata"
  dot asset-hub.tx.Assets.set_metadata "$DOTUSD_ASSET_ID" "$DOTUSD_NAME" "$DOTUSD_SYMBOL" "$DOTUSD_DECIMALS" --from "$SIGNER_ASSET_OWNER"
else
  echo "dotUSD metadata on AssetHub:"
  echo "$dotusd_metadata" | jq
fi
