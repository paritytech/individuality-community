#!/usr/bin/env bash
# Transfers dotUSD, USDT, USDC from the asset owner to the faucet account on AssetHub.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

# (symbol, asset_id, decimals, whole_tokens)
ASSETS=(
  "dotUSD  $DOTUSD_ASSET_ID  $DOTUSD_DECIMALS  $FAUCET_FUND_STABLECOINS"
  "USDT    $USDT_ASSET_ID    $USDT_DECIMALS    $FAUCET_FUND_STABLECOINS"
  "USDC    $USDC_ASSET_ID    $USDC_DECIMALS    $FAUCET_FUND_STABLECOINS"
)

transfer_asset_to_faucet() {
  local symbol="$1" asset_id="$2" decimals="$3" whole="$4"
  local fund_units=$((whole * 10**decimals))

  if ! asset_exists asset-hub "$asset_id" >/dev/null; then
    echo "$symbol (asset $asset_id) not set up - skipping"
    return
  fi

  local on_faucet on_owner
  on_faucet=$(assets_account_balance asset-hub "$asset_id" "$ACCOUNT_FAUCET")
  on_owner=$(assets_account_balance asset-hub "$asset_id" "$ACCOUNT_ASSET_OWNER")
  echo "== $symbol (asset $asset_id): faucet=$on_faucet, owner=$on_owner, target=$fund_units =="

  if int_ge "$on_faucet" "$fund_units"; then
    echo "$symbol already funded - skipping"
    return
  fi
  local shortfall=$(( fund_units - on_faucet ))
  if ! int_ge "$on_owner" "$shortfall"; then
    echo "ERROR: Source short of $symbol: need $shortfall, have $on_owner" >&2
    exit 1
  fi

  dot asset-hub.tx.Assets.transfer_keep_alive "$asset_id" "$ACCOUNT_FAUCET" "$shortfall" --from "$SIGNER_ASSET_OWNER"
}

echo "Funding faucet $ACCOUNT_FAUCET on AssetHub from the asset owner"

for row in "${ASSETS[@]}"; do
  read -r symbol asset_id decimals whole <<<"$row"
  transfer_asset_to_faucet "$symbol" "$asset_id" "$decimals" "$whole"
done
