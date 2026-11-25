#!/usr/bin/env bash
# Sets dotUSD-to-native conversion rate on People via AssetRate pallet.
# Rate aligns with the 1:4 ratio used in AssetHub liquidity pools.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

echo "-> Set dotUSD conversion rate on People"

# 1 PAS = 4 dotUSD -> 1 dotUSD = 0.25 PAS
dotusd_rate=$(echo "10^18 * 10^$NATIVE_DECIMALS / (4 * 10^$DOTUSD_DECIMALS)" | bc)
echo "Computed rate: $dotusd_rate (native_dec=$NATIVE_DECIMALS, dotusd_dec=$DOTUSD_DECIMALS)"

dotusd_foreign=$(people_foreign_location "$DOTUSD_ASSET_ID")
current_rate=$(dot people.query.AssetRate.ConversionRateToNative "$dotusd_foreign" | tr -d '"')
if [ "$current_rate" == "undefined" ]; then
  echo "No rate set, creating"
  create_call=$(dot --encode people.tx.AssetRate.create "$dotusd_foreign" "$dotusd_rate")
  dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
elif [ "$current_rate" != "$dotusd_rate" ]; then
  echo "Rate mismatch (current=$current_rate, expected=$dotusd_rate), updating"
  update_call=$(dot --encode people.tx.AssetRate.update "$dotusd_foreign" "$dotusd_rate")
  dot people.tx.Sudo.sudo "$update_call" --from "$SIGNER_PEOPLE_SUDO"
else
  echo "Rate already correct: $current_rate"
fi
