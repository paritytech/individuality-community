#!/usr/bin/env bash
# Increases the attestation account's allowances on People (PeopleLite)
# and AssetHub (DotnsGateway).
set -euo pipefail
source ./load-config.sh

setup_allowance() {
  local chain="$1" pallet="$2" allowance="$3"

  local current
  current=$(dot "${chain}".query."${pallet}".AttestationAllowance "$ACCOUNT_ATTESTATION")
  echo "Current $pallet allowance for $ACCOUNT_ATTESTATION on $chain is $current"
  if [ "$current" -ge "$allowance" ]; then
    echo "Allowance already >= $allowance on $chain, skipping increase"
    return
  fi

  echo "Increasing allowance for $ACCOUNT_ATTESTATION by $allowance on $chain"
  local increase_call
  increase_call=$(dot "${chain}".tx."${pallet}".increase_attestation_allowance "$ACCOUNT_ATTESTATION" "$allowance" --encode)
  local signer="$SIGNER_PEOPLE_SUDO"
  [ "$chain" = "asset-hub" ] && signer="$SIGNER_AH_SUDO"
  dot "${chain}".tx.Sudo.sudo "$increase_call" --from "$signer"
}

setup_allowance people PeopleLite "$PEOPLELITE_ATTESTATION_ALLOWANCE"
setup_allowance asset-hub DotnsGateway "$DOTNSGATEWAY_ATTESTATION_ALLOWANCE"
