#!/usr/bin/env bash
# Creates USDT and USDC as foreign assets on People with metadata.
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
}

setup_foreign_asset "$USDT_ASSET_ID" "$USDT_DECIMALS" "$USDT_NAME" "$USDT_SYMBOL" "$USDT_MIN_BALANCE"
setup_foreign_asset "$USDC_ASSET_ID" "$USDC_DECIMALS" "$USDC_NAME" "$USDC_SYMBOL" "$USDC_MIN_BALANCE"
