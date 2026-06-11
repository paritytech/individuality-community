#!/usr/bin/env bash
# Acquires USDT/USDC for the faucet on AssetHub by swapping PAS via
# AssetConversion pools. Must run after 04a (creates the pools).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

# (symbol, asset_id, decimals, whole_tokens, slippage_pct)
STABLECOINS=(
  "USDT  $USDT_ASSET_ID  $USDT_DECIMALS  10000  10"
  "USDC  $USDC_ASSET_ID  $USDC_DECIMALS  10000  10"
)

echo "=== Fund faucet stablecoins on AssetHub by swapping PAS ==="
for row in "${STABLECOINS[@]}"; do
  read -r symbol asset_id decimals whole slippage <<<"$row"
  target=$((whole * 10**decimals))

  echo ""
  echo "== $symbol (asset $asset_id), target=$target =="
  if ! asset_exists asset-hub "$asset_id" >/dev/null; then
    echo "not set up on AssetHub - skipping"
    continue
  fi

  on_faucet=$(assets_account_balance asset-hub "$asset_id" "$ACCOUNT_FAUCET")
  if int_ge "$on_faucet" "$target"; then
    echo "Faucet already holds $on_faucet (>= $target) - skipping swap"
    continue
  fi

  shortfall=$(( target - on_faucet ))
  if ! quote=$(quote_native_for_exact "$asset_id" "$shortfall"); then
    echo "ERROR: No quote for $symbol - pool missing or insufficient liquidity" >&2
    exit 1
  fi
  amount_in_max=$(( quote + quote * slippage / 100 ))
  echo "-> Swap PAS for $shortfall units of $symbol, send_to=$ACCOUNT_FAUCET"
  echo "   quote=$quote PAS-planck, amount_in_max=$amount_in_max (+${slippage}% slippage)"

  path='['"$NATIVE_TOKEN"','"$(ah_local_location "$asset_id")"']'
  dot asset-hub.tx.AssetConversion.swap_tokens_for_exact_tokens \
    "$path" "$shortfall" "$amount_in_max" "$ACCOUNT_FAUCET" true \
    --from "$SIGNER_ASSET_OWNER"
done
