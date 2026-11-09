#!/usr/bin/env bash
# Creates the PSM that mints dotUSD against USDT on AssetHub.
# Must run after 03c and 04a (set the dotUSD and USDT metadata).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

ROOT_ORIGIN='{"type":"system","value":{"type":"Root"}}'
dotusd_local=$(ah_local_location "$DOTUSD_ASSET_ID")
usdt_local=$(ah_local_location "$USDT_ASSET_ID")

# Echoes the value a PSM storage map holds for the dotUSD/USDT pair, or nothing if it is unset.
# dot cannot query a single entry of these maps, so this function reads the whole map.
psm_pair_value() {
  local map="$1"
  dot asset-hub.query.Psm."$map" --dump --output json \
    | jq -c --argjson internal "$dotusd_local" --argjson external "$usdt_local" \
      '.[] | select(.keys == [$internal, $external]) | .value'
}

set_psm_pair_value() {
  local map="$1" call="$2" value="$3" label="$4" current
  current=$(psm_pair_value "$map")
  if [ "$current" == "$value" ]; then
    echo "$label already $value, skipping"
    return
  fi
  echo "$label is ${current:-unset}, setting to $value"
  local set_call
  set_call=$(dot --encode asset-hub.tx.Psm."$call" "$dotusd_local" "$usdt_local" "$value")
  dot asset-hub.tx.Sudo.sudo "$set_call" --from "$SIGNER_AH_SUDO"
}

echo "-> Create PSM on AssetHub"
psm=$(dot asset-hub.query.Psm.Psm "$dotusd_local")
if [ "$psm" == "undefined" ]; then
  echo "PSM does not exist, creating"
  create_call=$(dot --encode asset-hub.tx.Psm.create_psm "$dotusd_local" "$ROOT_ORIGIN" "$ROOT_ORIGIN" \
    "$PSM_FEE_DESTINATION" "$PSM_MAX_DEBT" "$PSM_MIN_SWAP_AMOUNT")
  dot asset-hub.tx.Sudo.sudo "$create_call" --from "$SIGNER_AH_SUDO"
else
  echo "PSM already exists, skipping"
fi

echo "-> Add USDT to the PSM on AssetHub"
usdt=$(psm_pair_value ExternalAssets)
if [ -n "$usdt" ]; then
  echo "USDT already added, skipping"
else
  echo "USDT not added, adding"
  add_call=$(dot --encode asset-hub.tx.Psm.add_external_asset "$dotusd_local" "$usdt_local")
  dot asset-hub.tx.Sudo.sudo "$add_call" --from "$SIGNER_AH_SUDO"
fi

echo "-> Set PSM fees and ceiling weight for USDT on AssetHub"
set_psm_pair_value MintingFee set_minting_fee "$PSM_MINTING_FEE" "Minting fee"
set_psm_pair_value RedemptionFee set_redemption_fee "$PSM_REDEMPTION_FEE" "Redemption fee"
set_psm_pair_value AssetCeilingWeight set_asset_ceiling_weight "$PSM_ASSET_CEILING_WEIGHT" "Ceiling weight"
