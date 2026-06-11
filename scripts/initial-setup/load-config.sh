#!/usr/bin/env bash
# load-config.sh - sources the right config for the target environment.
# Usage: source ./load-config.sh (from any script in this directory)
# Set ENV to "local" before sourcing.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV="${ENV:-local}"

if [[ ! "$ENV" =~ ^local$ ]]; then
  echo "ERROR: ENV must be 'local'. Got: '$ENV'" >&2
  exit 1
fi

source "$SCRIPT_DIR/config-base.env"
source "$SCRIPT_DIR/config-${ENV}.env"

# Ensure dot CLI is available
if ! command -v dot >/dev/null 2>&1; then
  echo "ERROR: 'dot' (polkadot-cli) is not installed. Run: npm install -g polkadot-cli@$REQUIRED_DOT_VERSION" >&2
  exit 1
fi

# Fail fast if any required per-env variable is missing
REQUIRED_PER_ENV_VARS=(
  RPC_RELAY RPC_ASSET_HUB RPC_BULLETIN RPC_PEOPLE
  PARACHAIN_ID_ASSET_HUB PARACHAIN_ID_BULLETIN PARACHAIN_ID_PEOPLE
  SIGNER_PEOPLE_SUDO SIGNER_AH_SUDO SIGNER_ATTESTATION SIGNER_ASSET_OWNER
  ACCOUNT_PEOPLE_SUDO ACCOUNT_PEOPLE_SUDO_PROXY ACCOUNT_AH_SUDO
  ACCOUNT_ATTESTATION ACCOUNT_ATTESTATION_PROXY ACCOUNT_ASSET_OWNER ACCOUNT_FAUCET
  DOTNSGATEWAY_DISPATCHER_ADDRESS
)
missing_vars=()
for var in "${REQUIRED_PER_ENV_VARS[@]}"; do
  if [ -z "${!var:-}" ]; then
    missing_vars+=("$var")
  fi
done
if [ ${#missing_vars[@]} -gt 0 ]; then
  echo "ERROR: config-${ENV}.env is missing required variables: ${missing_vars[*]}" >&2
  exit 1
fi

# Register chain aliases (failures swallowed: the alias may already exist)
dot chain add relay --rpc "$RPC_RELAY" 2>/dev/null || true
dot chain add asset-hub --rpc "$RPC_ASSET_HUB" 2>/dev/null || true
dot chain add people --rpc "$RPC_PEOPLE" 2>/dev/null || true
dot chain add bulletin --rpc "$RPC_BULLETIN" 2>/dev/null || true

# Derived values (depend on dot CLI + env-specific parachain IDs)
# Child(People) -> People Account on Relay required for HRMP channels
ACCOUNT_CHILD_PEOPLE=$(dot account inspect --parachain "$PARACHAIN_ID_PEOPLE" --parachain-type child --output json | jq -r ".ss58")
# Sibling(People) -> People Reserve Account on AssetHub
ACCOUNT_SIBLING_PEOPLE=$(dot account inspect --parachain "$PARACHAIN_ID_PEOPLE" --parachain-type sibling --output json | jq -r ".ss58")

echo "[env=$ENV] Config loaded"
