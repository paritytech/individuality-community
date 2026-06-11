#!/usr/bin/env bash
# Sets the alias fee on AssetHub.
set -euo pipefail
source ./load-config.sh

echo "-> Set alias fee on AssetHub"
current_fee=$(dot asset-hub.query.AliasAccounts.AliasFee | tr -d '"')
if [ "$current_fee" == "$PGAS_ALIAS_FEE" ]; then
  echo "Alias fee already set to $PGAS_ALIAS_FEE"
else
  echo "Alias fee is $current_fee, setting to $PGAS_ALIAS_FEE"
  set_call=$(dot --encode asset-hub.tx.AliasAccounts.set_alias_fee "$PGAS_ALIAS_FEE")
  dot asset-hub.tx.Sudo.sudo "$set_call" --from "$SIGNER_AH_SUDO"
fi
