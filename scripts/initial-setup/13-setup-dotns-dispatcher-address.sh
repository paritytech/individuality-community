#!/usr/bin/env bash
# Sets the DotNS gateway dispatcher address on AssetHub.
set -euo pipefail
source ./load-config.sh

echo "-> Set DotNS gateway dispatcher address on AssetHub"
current_address=$(dot asset-hub.query.DotnsGateway.DispatcherAddress | tr -d '"')
if [ "$current_address" == "$DOTNSGATEWAY_DISPATCHER_ADDRESS" ]; then
  echo "Dispatcher address already set to $DOTNSGATEWAY_DISPATCHER_ADDRESS"
else
  echo "Dispatcher address is $current_address, setting to $DOTNSGATEWAY_DISPATCHER_ADDRESS"
  set_call=$(dot --encode asset-hub.tx.DotnsGateway.set_dispatcher_address "$DOTNSGATEWAY_DISPATCHER_ADDRESS")
  dot asset-hub.tx.Sudo.sudo "$set_call" --from "$SIGNER_AH_SUDO"
fi
