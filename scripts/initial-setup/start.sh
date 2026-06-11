#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV="${1:-${ENV:-local}}"
export ENV

# Comment out entries to skip steps.
# Keep only one entry here if you want to run a single script.
SCRIPT_NAMES=(
  "00-requirements.sh"
  "01a-fund-accounts-relay-people.sh"
  "01b-setup-hrmp-channel-people-ah.sh"
  "01c-setup-hrmp-channel-people-bulletin.sh"
  "01d-fund-accounts-ah.sh"
  "02-setup-sudo-proxy.sh"
  "03a-setup-xtrnl-ah.sh"
  "03b-setup-xtrnl-pool-ah.sh"
  "03c-setup-xtrnl-metadata-ah.sh"
  "03d-setup-xtrnl-metadata-people.sh"
  "03e-setup-xtrnl-rate-people.sh"
  "03f-setup-coinage-underlying-asset.sh"
  "04a-setup-usdt-usdc-ah.sh"
  "04b-setup-usdt-usdc-people.sh"
  "05a-swap-stablecoins-ah.sh"
  "05b-fund-faucet-stablecoins-people.sh"
  "05c-fund-faucet-stablecoins-ah.sh"
  "06a-setup-pgas.sh"
  "06b-setup-pgas-pool.sh"
  "06c-setup-alias-fee.sh"
  "07-add-zk-chunks.sh"
  "08-setup-people-collection.sh"
  "09-override-onboarding-sizes.sh"
  "10-subscribe-ah-ring-root-updates.sh"
  "11-setup-poi-design-families.sh"
  "12a-setup-attestation-invites.sh"
  "12b-setup-attestation-allowances.sh"
  "12c-setup-attestation-proxy.sh"
  "13-setup-dotns-dispatcher-address.sh"
)

if [ "${#SCRIPT_NAMES[@]}" -eq 0 ]; then
  echo "ERROR: No scripts configured in start.sh" >&2
  exit 1
fi

cd "$SCRIPT_DIR"

echo "=== Initial Setup (ENV=$ENV) ==="
echo "Running ${#SCRIPT_NAMES[@]} script(s) from $SCRIPT_DIR"

for script_name in "${SCRIPT_NAMES[@]}"; do
  if [ ! -f "$script_name" ]; then
    echo "ERROR: Configured script not found: $SCRIPT_DIR/$script_name" >&2
    exit 1
  fi

  echo ""
  echo "==> Running $script_name"

  # Run each script in a clean subprocess so exports do not leak between scripts.
  env -i \
    ENV="$ENV" \
    PATH="$PATH" \
    HOME="$HOME" \
    USER="${USER:-}" \
    SHELL="${SHELL:-/bin/bash}" \
    TERM="${TERM:-xterm-256color}" \
    PEOPLE_SUDO_MNEMONIC="${PEOPLE_SUDO_MNEMONIC:-}" \
    AH_SUDO_MNEMONIC="${AH_SUDO_MNEMONIC:-}" \
    ATTESTATION_MNEMONIC="${ATTESTATION_MNEMONIC:-}" \
    ASSET_OWNER_MNEMONIC="${ASSET_OWNER_MNEMONIC:-}" \
    bash "$script_name"
done

echo ""
echo "=== Summary ==="
echo "Environment: $ENV"
echo "Scripts run: ${#SCRIPT_NAMES[@]}"
echo "All scripts completed successfully."
