#!/usr/bin/env bash
# Creates the PGAS asset on AssetHub.
set -euo pipefail
source ./load-config.sh

echo "-> Create PGAS asset($PGAS_ASSET_ID) on AssetHub if it does not exist"
pgas_asset=$(dot asset-hub.query.Assets.Asset "$PGAS_ASSET_ID")
if [ "$pgas_asset" == "undefined" ]; then
  echo "Asset $PGAS_ASSET_ID does not exist, creating..."
  dot asset-hub.tx.Pgas.create_pgas_asset --unsigned
else
  echo "Asset $PGAS_ASSET_ID already exists"
fi
