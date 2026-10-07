#!/usr/bin/env bash
# Sets the alias fee on AssetHub.
set -euo pipefail
source ./load-config.sh

echo "-> Set alias fee on AssetHub"
alias_fee_key='{"type":"AliasAccounts","value":{"type":"AliasFee"}}'
alias_fee_param=$(dot asset-hub.query.Parameters.Parameters "$alias_fee_key")
if [ "$alias_fee_param" == "undefined" ]; then
  current_fee="unset"
else
  current_fee=$(echo "$alias_fee_param" | jq -r ".value.value")
fi
if [ "$current_fee" == "$PGAS_ALIAS_FEE" ]; then
  echo "Alias fee already set to $PGAS_ALIAS_FEE"
else
  echo "Alias fee is $current_fee, setting to $PGAS_ALIAS_FEE"
  set_call=$(dot --encode asset-hub.tx.Parameters.set_parameter \
    "{\"type\":\"AliasAccounts\",\"value\":{\"type\":\"AliasFee\",\"value\":[$PGAS_ALIAS_FEE]}}")
  dot asset-hub.tx.Sudo.sudo "$set_call" --from "$SIGNER_AH_SUDO"
fi
