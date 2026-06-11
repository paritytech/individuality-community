#!/usr/bin/env bash
# Subscribes AssetHub to ring root updates from People
# for the people and people-lite collections.
set -euo pipefail
source ./load-config.sh

# MembersSubscriber pallet index on AssetHub.
SUBSCRIBER_PALLET_INDEX=97

echo "-> Subscribe AssetHub to ring root updates from People for the people and people-lite collections"
subscriber=$(dot people.query.MembersNotifier.Subscribers "$PARACHAIN_ID_ASSET_HUB")
if [ "$subscriber" != "undefined" ]; then
  echo "AssetHub($PARACHAIN_ID_ASSET_HUB) already subscribed, skipping"
else
  echo "AssetHub($PARACHAIN_ID_ASSET_HUB) not subscribed, subscribing"
  collections="[[\"$PEOPLE_IDENTIFIER\", {\"type\": \"R2e9\"}], [\"$PEOPLE_LITE_IDENTIFIER\", {\"type\": \"R2e9\"}]]"
  subscribe_call=$(dot --encode people.tx.MembersNotifier.subscribe "$PARACHAIN_ID_ASSET_HUB" "$collections" "$SUBSCRIBER_PALLET_INDEX")
  dot people.tx.Sudo.sudo "$subscribe_call" --from "$SIGNER_PEOPLE_SUDO"
fi
