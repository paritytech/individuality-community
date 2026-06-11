#!/usr/bin/env bash
# Grants DIM1 (ProofOfInk) and DIM2 (Game) invites to the attestation account.
set -euo pipefail
source ./load-config.sh

setup_invites() {
  local pallet="$1" count="$2"

  local available
  available=$(dot people.query."${pallet}".AvailableInvites "$ACCOUNT_ATTESTATION")
  if [ "$available" -ge "$count" ]; then
    echo "$ACCOUNT_ATTESTATION has $available $pallet invites, already >= $count, skipping grant"
    return
  fi

  echo "Granting $count $pallet invites to $ACCOUNT_ATTESTATION"
  local grant_call
  grant_call=$(dot --encode people.tx."${pallet}".grant_invites "$ACCOUNT_ATTESTATION" "$count")
  dot people.tx.Sudo.sudo "$grant_call" --from "$SIGNER_PEOPLE_SUDO"
}

setup_invites ProofOfInk "$DIM1_INVITES"
setup_invites Game "$DIM2_INVITES"
