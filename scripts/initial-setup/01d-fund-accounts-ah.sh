#!/usr/bin/env bash
# Funds sudo, attestation, attestation proxy, faucet, and asset
# owner accounts on AssetHub with native tokens teleported from the
# People sudo. Must run after 01b (establishes the People<->AssetHub
# HRMP channel).
set -euo pipefail
source ./load-config.sh
source ./utils.sh

AH_FUND_PAS=10000
AH_ASSET_OWNER_FUND_PAS=100000

AH_FUND_TARGETS=(
  "ACCOUNT_AH_SUDO"
  "ACCOUNT_ATTESTATION"
  "ACCOUNT_ATTESTATION_PROXY"
  "ACCOUNT_FAUCET"
)

teleport_to_ah() {
  local pas_amount="$1"
  shift
  local targets=("$@")
  local fund_amount=$((pas_amount * 10**NATIVE_DECIMALS))

  echo "=== Funding on AssetHub via teleport from People - $pas_amount PAS ($fund_amount units) per account ==="

  for var in "${targets[@]}"; do
    local account="${!var}"
    if has_funds asset-hub "$account"; then
      echo "  Skipping $var - already funded"
    else
      echo "  Teleporting to $var ($account)"
      local id32
      id32=$(dot account inspect "$account" --output json | jq -r '.publicKey')
      local dest beneficiary assets
      dest='{"type":"V4","value":{"parents":1,"interior":{"type":"X1","value":[{"type":"Parachain","value":'"$PARACHAIN_ID_ASSET_HUB"'}]}}}'
      beneficiary='{"type":"V4","value":{"parents":0,"interior":{"type":"X1","value":[{"type":"AccountId32","value":{"network":null,"id":"'"$id32"'"}}]}}}'
      assets='{"type":"V4","value":[{"id":{"parents":1,"interior":{"type":"Here"}},"fun":{"type":"Fungible","value":"'"$fund_amount"'"}}]}'
      dot people.tx.PolkadotXcm.limited_teleport_assets \
        "$dest" "$beneficiary" "$assets" 0 Unlimited \
        --from "$SIGNER_PEOPLE_SUDO"
    fi
  done
}

teleport_to_ah "$AH_FUND_PAS" "${AH_FUND_TARGETS[@]}"
teleport_to_ah "$AH_ASSET_OWNER_FUND_PAS" ACCOUNT_ASSET_OWNER
