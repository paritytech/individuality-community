#!/usr/bin/env bash
# Creates USDT and USDC as foreign assets on People with metadata and conversion rates.
# Rates align with the 1:4 ratio used in AssetHub liquidity pools.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

setup_foreign_asset() {
  local asset_id="$1"
  local decimals="$2"
  local name="$3"
  local symbol="$4"
  local min_balance="$5"
  local asset_location='{"parents":1,"interior":{"type":"X3","value":[{"type":"Parachain","value":'"$PARACHAIN_ID_ASSET_HUB"'},{"type":"PalletInstance","value":50},{"type":"GeneralIndex","value":"'"$asset_id"'"}]}}'

  echo ""
  echo "== Setting up $symbol (asset $asset_id) =="

  echo "-> Create foreign $symbol asset on People if it does not exist"
  local asset
  asset=$(dot people.query.Assets.Asset "$asset_location")
  if [ "$asset" == "undefined" ]; then
    echo "$symbol asset does not exist, creating"
    local create_call
    create_call=$(dot --encode people.tx.Assets.force_create "$asset_location" "$ACCOUNT_ASSET_OWNER" true "$min_balance")
    dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
  else
    echo "$symbol asset already exists"
  fi

  echo "-> Set $symbol metadata on People"
  local metadata
  metadata=$(dot people.query.Assets.Metadata "$asset_location")
  if [ "$(echo "$metadata" | jq .decimals)" != "$decimals" ]; then
    echo "$symbol metadata is not set, setting metadata"
    dot people.tx.Assets.set_metadata "$asset_location" "$name" "$symbol" "$decimals" --from "$SIGNER_ASSET_OWNER"
  else
    echo "$symbol metadata already set on People:"
    echo "$metadata" | jq
  fi

  echo "-> Set $symbol conversion rate on People"
  # 1 PAS = 4 USDT or USDC
  local rate current_rate
  rate=$(echo "10^18 * 10^$NATIVE_DECIMALS / (4 * 10^$decimals)" | bc)
  current_rate=$(dot people.query.AssetRate.ConversionRateToNative "$asset_location" | tr -d '"')
  if [ "$current_rate" == "undefined" ]; then
    echo "No $symbol rate set, creating"
    local create_rate_call
    create_rate_call=$(dot --encode people.tx.AssetRate.create "$asset_location" "$rate")
    dot people.tx.Sudo.sudo "$create_rate_call" --from "$SIGNER_PEOPLE_SUDO"
  elif [ "$current_rate" != "$rate" ]; then
    echo "$symbol rate mismatch (current=$current_rate, expected=$rate), updating"
    local update_rate_call
    update_rate_call=$(dot --encode people.tx.AssetRate.update "$asset_location" "$rate")
    dot people.tx.Sudo.sudo "$update_rate_call" --from "$SIGNER_PEOPLE_SUDO"
  else
    echo "$symbol rate already correct: $current_rate"
  fi
}

setup_foreign_asset "$USDT_ASSET_ID" "$USDT_DECIMALS" "$USDT_NAME" "$USDT_SYMBOL" "$USDT_MIN_BALANCE"
setup_foreign_asset "$USDC_ASSET_ID" "$USDC_DECIMALS" "$USDC_NAME" "$USDC_SYMBOL" "$USDC_MIN_BALANCE"
