#!/usr/bin/env bash
# Funds sudo proxy, attestation, attestation proxy, faucet,
# and asset owner accounts on People with native tokens,
# and bootstraps the People sovereign on the Relay so it can
# pay for the upward XCM that establishes the HRMP channels.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

PEOPLE_FUND_PAS=10000
RELAY_FUND_PAS=10

PEOPLE_FUND_TARGETS=(
  "ACCOUNT_PEOPLE_SUDO_PROXY"
  "ACCOUNT_ATTESTATION"
  "ACCOUNT_ATTESTATION_PROXY"
  "ACCOUNT_FAUCET"
  "ACCOUNT_ASSET_OWNER"
)

# The People sovereign only needs to cover WithdrawAsset+BuyExecution.
# RefundSurplus returns ~99% per call, so a one-time bootstrap lasts many cycles.
RELAY_FUND_TARGETS=(
  "ACCOUNT_CHILD_PEOPLE"
)

fund_targets() {
  local chain="$1" pas_amount="$2"
  shift 2
  local targets=("$@")
  local fund_amount=$((pas_amount * 10**NATIVE_DECIMALS))

  echo "=== Funding on $chain - $pas_amount PAS ($fund_amount units) per account ==="

  for var in "${targets[@]}"; do
    local account="${!var}"
    if has_funds "$chain" "$account"; then
      echo "  Skipping $var - already funded"
    else
      echo "  Funding $var ($account)"
      dot "${chain}.tx.Balances.transfer_keep_alive" "$account" "$fund_amount" \
        --from "$SIGNER_PEOPLE_SUDO"
    fi
  done
}

fund_targets people "$PEOPLE_FUND_PAS" "${PEOPLE_FUND_TARGETS[@]}"
fund_targets relay "$RELAY_FUND_PAS" "${RELAY_FUND_TARGETS[@]}"
