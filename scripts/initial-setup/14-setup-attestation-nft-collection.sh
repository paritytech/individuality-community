#!/usr/bin/env bash
# Creates the attestation NFT collection on People.
set -euo pipefail
source ./load-config.sh

echo "-> Create attestation NFT collection on People"
attestation_nft_collection=$(dot people.query.Game.AttestationNftCollection)
if [ "$attestation_nft_collection" != "undefined" ]; then
  echo "Attestation NFT collection already created (id=$attestation_nft_collection), skipping"
else
  echo "Attestation NFT collection not created, creating"
  dot people.tx.Game.create_attestation_collection --unsigned
fi
