#!/usr/bin/env bash
# Sets XTRNL-to-native conversion rate on People via AssetRate pallet.
# Rate aligns with the 1:4 ratio used in AssetHub liquidity pools.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Set XTRNL conversion rate on People"

# 1 PAS = 4 XTRNL -> 1 XTRNL = 0.25 PAS
xtrnl_rate=$(echo "10^18 * 10^$NATIVE_DECIMALS / (4 * 10^$XTRNL_DECIMALS)" | bc)
echo "Computed rate: $xtrnl_rate (native_dec=$NATIVE_DECIMALS, xtrnl_dec=$XTRNL_DECIMALS)"

xtrnl_foreign=$(people_foreign_location "$XTRNL_ASSET_ID")
current_rate=$(dot people.query.AssetRate.ConversionRateToNative "$xtrnl_foreign" | tr -d '"')
if [ "$current_rate" == "undefined" ]; then
  echo "No rate set, creating"
  create_call=$(dot --encode people.tx.AssetRate.create "$xtrnl_foreign" "$xtrnl_rate")
  dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
elif [ "$current_rate" != "$xtrnl_rate" ]; then
  echo "Rate mismatch (current=$current_rate, expected=$xtrnl_rate), updating"
  update_call=$(dot --encode people.tx.AssetRate.update "$xtrnl_foreign" "$xtrnl_rate")
  dot people.tx.Sudo.sudo "$update_call" --from "$SIGNER_PEOPLE_SUDO"
else
  echo "Rate already correct: $current_rate"
fi
