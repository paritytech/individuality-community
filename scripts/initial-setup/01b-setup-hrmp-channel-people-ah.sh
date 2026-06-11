#!/usr/bin/env bash
# Establishes HRMP channel between People and AssetHub via system channel.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

run() {
  local establish_call xcm_call
  establish_call=$(dot relay.tx.Hrmp.establish_channel_with_system "$PARACHAIN_ID_ASSET_HUB" --encode)
  xcm_call=$(dot ./xcm-to-parent.yaml --encode --var CALL="$establish_call" --var PARACHAIN_ID="$PARACHAIN_ID_PEOPLE")
  echo "Sending XCM to establish HRMP channel between People($PARACHAIN_ID_PEOPLE) and AssetHub($PARACHAIN_ID_ASSET_HUB)"
  dot people.tx.Sudo.sudo "$xcm_call" --from "$SIGNER_PEOPLE_SUDO"
}

if ! has_hrmp_channel "$PARACHAIN_ID_PEOPLE" "$PARACHAIN_ID_ASSET_HUB" || ! has_hrmp_channel "$PARACHAIN_ID_ASSET_HUB" "$PARACHAIN_ID_PEOPLE"; then
  echo "People<->AssetHub channel not found, establishing channel..."
  run
fi
