#!/usr/bin/env bash
# Creates the Coinage instance wrapping dotUSD on People.
# Must run after 03d (creates dotUSD as foreign asset on People).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

dotusd_foreign=$(people_foreign_location "$DOTUSD_ASSET_ID")
coinage_account=$(dot account inspect --pallet-id "coinage " --output json | jq -r ".ss58")

echo "-> Fund Coinage pallet account with dotUSD on People"
# Instance creation requires this balance and mints nothing itself, so provision it here.
coinage_balance=$(assets_account_balance people "$dotusd_foreign" "$coinage_account")
if int_ge "$coinage_balance" "$DOTUSD_MIN_BALANCE"; then
  echo "Pallet account $coinage_account already holds $coinage_balance units"
else
  echo "Minting $DOTUSD_MIN_BALANCE units to pallet account $coinage_account"
  dot people.tx.Assets.mint "$dotusd_foreign" "$coinage_account" "$DOTUSD_MIN_BALANCE" --from "$SIGNER_ASSET_OWNER"
fi

echo "-> Create Coinage instance for dotUSD on People"
# An instance is never exclusive over its asset, so look for one that already wraps dotUSD at
# the unit this setup wants.
coinage_instance=""
instance_ids=$(dot people.query.Coinage.AssetToInstance "$dotusd_foreign" | jq -r ".[].keys[1]")
for id in $instance_ids; do
  instance=$(dot people.query.Coinage.Instances "$id" --output json)
  asset_unit=$(echo "$instance" | jq -r ".asset_unit")
  mode=$(echo "$instance" | jq -r ".mode.type")
  if [ "$asset_unit" == "$COINAGE_ASSET_UNIT" ] && [ "$mode" == "Sufficient" ]; then
    coinage_instance="$id"
    break
  fi
done
if [ -n "$coinage_instance" ]; then
  echo "dotUSD is already wrapped at asset unit $COINAGE_ASSET_UNIT by instance $coinage_instance"
else
  echo "No instance wraps dotUSD at asset unit $COINAGE_ASSET_UNIT, creating one"
  create_call=$(dot --encode people.tx.Coinage.create_sufficient_instance "$dotusd_foreign" "$COINAGE_ASSET_UNIT")
  dot people.tx.Sudo.sudo "$create_call" --from "$SIGNER_PEOPLE_SUDO"
fi
