#!/usr/bin/env bash
# Acquires USDT/USDC on AssetHub for ACCOUNT_ASSET_OWNER by swapping
# PAS via AssetConversion pools. Re-run guard: only swaps the shortfall
# needed so that 05b can move $target units per asset to the faucet on
# People.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

# (symbol, asset_id, decimals, whole_tokens, slippage_pct)
# whole_tokens MUST match the per-asset row in 05b
ASSETS=(
  "USDT  $USDT_ASSET_ID  $USDT_DECIMALS  10000  10"
  "USDC  $USDC_ASSET_ID  $USDC_DECIMALS  10000  10"
)

swap_native_for_exact() {
  local symbol="$1" asset_id="$2" amount_out="$3" slippage_pct="$4"
  local quote amount_in_max path

  if ! quote=$(quote_native_for_exact "$asset_id" "$amount_out"); then
    echo "ERROR: No quote for $symbol - pool missing or insufficient liquidity" >&2
    exit 1
  fi
  amount_in_max=$(( quote + quote * slippage_pct / 100 ))
  echo "   quote=$quote PAS-planck, amount_in_max=$amount_in_max (+${slippage_pct}% slippage)"

  path='['"$NATIVE_TOKEN"','"$(ah_local_location "$asset_id")"']'
  dot asset-hub.tx.AssetConversion.swap_tokens_for_exact_tokens \
    "$path" "$amount_out" "$amount_in_max" "$ACCOUNT_ASSET_OWNER" true \
    --from "$SIGNER_ASSET_OWNER"
}

echo "Acquiring stablecoin balances for $ACCOUNT_ASSET_OWNER on AssetHub by swapping PAS"

for row in "${ASSETS[@]}"; do
  read -r symbol asset_id decimals whole slippage <<<"$row"
  target=$((whole * 10**decimals))

  echo ""
  echo "== $symbol (asset $asset_id), target=$target =="

  if ! asset_exists asset-hub "$asset_id" >/dev/null; then
    echo "not set up on AssetHub - skipping"
    continue
  fi

  on_people=$(assets_account_balance people "$(people_foreign_location "$asset_id")" "$ACCOUNT_FAUCET")
  if int_ge "$on_people" "$target"; then
    echo "Faucet on People already holds $on_people (>= $target) - skipping swap"
    continue
  fi

  on_ah=$(assets_account_balance asset-hub "$asset_id" "$ACCOUNT_ASSET_OWNER")
  if int_ge "$on_ah" "$target"; then
    echo "Owner on AssetHub already holds $on_ah (>= $target) - skipping swap"
    continue
  fi

  shortfall=$(( target - on_ah ))
  echo "-> Swap PAS for $shortfall units of $symbol (owner has $on_ah, target $target)"
  swap_native_for_exact "$symbol" "$asset_id" "$shortfall" "$slippage"
done
