#!/usr/bin/env bash
# Creates USDT and USDC on AssetHub with metadata, pool, and liquidity.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

setup_stablecoin() {
  local asset_id="$1"
  local decimals="$2"
  local name="$3"
  local symbol="$4"
  local total_supply="$5"
  local min_balance="$6"
  local asset_local='{"parents":0,"interior":{"type":"X2","value":[{"type":"PalletInstance","value":50},{"type":"GeneralIndex","value":"'"$asset_id"'"}]}}'

  echo ""
  echo "== Setting up $symbol (asset $asset_id) =="

  echo "-> Create new $symbol asset if $asset_id does not exist"
  local asset
  asset=$(dot asset-hub.query.Assets.Asset "$asset_id")
  if [ "$asset" == "undefined" ]; then
    echo "$symbol asset does not exist, creating"
    local create_call
    create_call=$(dot --encode asset-hub.tx.Assets.force_create "$asset_id" "$ACCOUNT_ASSET_OWNER" true "$min_balance")
    dot asset-hub.tx.Sudo.sudo "$create_call" --from "$SIGNER_AH_SUDO"
  else
    echo "$symbol asset already exists"
  fi

  echo "-> Mint $symbol total supply on AssetHub"
  local balance
  balance=$(dot asset-hub.query.Assets.Account "$asset_id" "$ACCOUNT_ASSET_OWNER")
  if [ "$balance" == "undefined" ]; then
    echo "$symbol balance is undefined, minting"
    dot asset-hub.tx.Assets.mint "$asset_id" "$ACCOUNT_ASSET_OWNER" "$total_supply" --from "$SIGNER_ASSET_OWNER"
  else
    echo "$symbol balance is $(echo "$balance" | jq .balance) ($ACCOUNT_ASSET_OWNER)"
  fi

  echo "-> Set $symbol metadata on AssetHub"
  local metadata
  metadata=$(dot asset-hub.query.Assets.Metadata "$asset_id")
  if [ "$(echo "$metadata" | jq .decimals)" != "$decimals" ]; then
    echo "$symbol metadata is not set, setting metadata"
    dot asset-hub.tx.Assets.set_metadata "$asset_id" "$name" "$symbol" "$decimals" --from "$SIGNER_ASSET_OWNER"
  else
    echo "$symbol metadata already set on AssetHub:"
    echo "$metadata" | jq
  fi

  echo "-> Create PAS/$symbol liquidity pool on AssetHub"
  local pool
  pool=$(dot asset-hub.query.AssetConversion.Pools "[$NATIVE_TOKEN, $asset_local]")
  if [ "$pool" == "undefined" ]; then
    echo "$symbol pool is not set, creating pool"
    dot asset-hub.tx.AssetConversion.create_pool "$NATIVE_TOKEN" "$asset_local" --from "$SIGNER_ASSET_OWNER"
  else
    echo "PAS/$symbol pool is LPToken: $pool"
  fi

  echo "-> Check PAS/$symbol pool liquidity on AssetHub"
  local reserves
  reserves=$(dot asset-hub.apis.AssetConversionApi.get_reserves "$NATIVE_TOKEN" "$asset_local" --json)
  local native_reserve
  if [ "$reserves" == "undefined" ] || [ "$reserves" == "null" ]; then
    native_reserve="0"
  else
    native_reserve=$(echo "$reserves" | jq -r '.[0] // "0"')
  fi
  local native_max=$(( 10000 * 10**NATIVE_DECIMALS ))
  local native_min=$(( 5000 * 10**NATIVE_DECIMALS ))
  local asset_max=$(( 40000 * 10**decimals ))
  local asset_min=$(( 20000 * 10**decimals ))
  if [ "$native_reserve" == "null" ] || [ "$native_reserve" -lt "$native_max" ]; then
    echo "Pool liquidity insufficient (native=$native_reserve), adding liquidity (1:4 ratio)"
    dot asset-hub.tx.AssetConversion.add_liquidity "$NATIVE_TOKEN" "$asset_local" $native_max $asset_max $native_min $asset_min "$ACCOUNT_ASSET_OWNER" --from "$SIGNER_ASSET_OWNER"
  else
    echo "Pool already has sufficient liquidity: native=$native_reserve"
  fi
}

setup_stablecoin "$USDT_ASSET_ID" "$USDT_DECIMALS" "$USDT_NAME" "$USDT_SYMBOL" "$USDT_TOTAL_SUPPLY" "$USDT_MIN_BALANCE"
setup_stablecoin "$USDC_ASSET_ID" "$USDC_DECIMALS" "$USDC_NAME" "$USDC_SYMBOL" "$USDC_TOTAL_SUPPLY" "$USDC_MIN_BALANCE"
