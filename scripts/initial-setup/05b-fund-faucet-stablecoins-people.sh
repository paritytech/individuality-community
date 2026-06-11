#!/usr/bin/env bash
# Transfers USDT, USDC from AssetHub to the faucet account on People via XCM.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

target_id32=$(dot account inspect "$ACCOUNT_FAUCET" --output json | jq -r '.publicKey')
reserve_topup=$((FAUCET_RESERVE_TOPUP_PAS * 10**NATIVE_DECIMALS))

# (symbol, asset_id, decimals, whole_tokens, transfer_type)
ASSETS=(
  "USDT  $USDT_ASSET_ID  $USDT_DECIMALS  10000    LocalReserve"
  "USDC  $USDC_ASSET_ID  $USDC_DECIMALS  10000    LocalReserve"
)

ensure_sibling_reserve_funded() {
  local free
  free=$(dot asset-hub.query.System.Account "$ACCOUNT_SIBLING_PEOPLE" --output json | jq -r '.data.free // 0')
  if [ "$free" = "0" ]; then
    echo "Funding sibling reserve $ACCOUNT_SIBLING_PEOPLE with $reserve_topup"
    dot asset-hub.tx.Balances.transfer_keep_alive "$ACCOUNT_SIBLING_PEOPLE" "$reserve_topup" --from "$SIGNER_ASSET_OWNER"
  else
    echo "Sibling reserve $ACCOUNT_SIBLING_PEOPLE has $free units (>0), skipping top-up"
  fi
}

xcm_transfer_asset_to_people() {
  local symbol="$1" asset_id="$2" decimals="$3" whole="$4" transfer_type="$5"
  local fund_units=$((whole * 10**decimals))

  if ! asset_exists asset-hub "$asset_id" >/dev/null; then
    echo "$symbol (asset $asset_id) not set up on AssetHub - skipping"
    return
  fi

  local on_people on_ah
  on_people=$(assets_account_balance people "$(people_foreign_location "$asset_id")" "$ACCOUNT_FAUCET")
  on_ah=$(assets_account_balance asset-hub "$asset_id" "$ACCOUNT_ASSET_OWNER")
  echo "== $symbol (asset $asset_id): faucet=$on_people on People, owner=$on_ah on AssetHub, target=$fund_units =="

  if int_ge "$on_people" "$fund_units"; then
    echo "$symbol already funded - skipping"
    return
  fi
  if ! int_ge "$on_ah" "$fund_units"; then
    echo "ERROR: Source short of $symbol on AssetHub: need $fund_units, have $on_ah" >&2
    exit 1
  fi

  local asset_local dest assets remote_fees_id custom_xcm
  asset_local=$(ah_local_location "$asset_id")
  dest='{"type":"V4","value":{"parents":1,"interior":{"type":"X1","value":[{"type":"Parachain","value":'"$PARACHAIN_ID_PEOPLE"'}]}}}'
  assets='{"type":"V4","value":[{"id":'"$asset_local"',"fun":{"type":"Fungible","value":"'"$fund_units"'"}}]}'
  remote_fees_id='{"type":"V4","value":'"$asset_local"'}'
  custom_xcm='{"type":"V4","value":[{"type":"DepositAsset","value":{"assets":{"type":"Wild","value":{"type":"All"}},"beneficiary":{"parents":0,"interior":{"type":"X1","value":{"type":"AccountId32","value":{"network":null,"id":"'"$target_id32"'"}}}}}}]}'

  dot asset-hub.tx.PolkadotXcm.transfer_assets_using_type_and_then \
    "$dest" "$assets" "$transfer_type" "$remote_fees_id" "$transfer_type" "$custom_xcm" Unlimited \
    --from "$SIGNER_ASSET_OWNER"
}

echo "Funding faucet $ACCOUNT_FAUCET (id32=$target_id32) via XCM from AssetHub"
ensure_sibling_reserve_funded

for row in "${ASSETS[@]}"; do
  read -r symbol asset_id decimals whole transfer_type <<<"$row"
  xcm_transfer_asset_to_people "$symbol" "$asset_id" "$decimals" "$whole" "$transfer_type"
done
