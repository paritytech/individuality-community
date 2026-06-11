#!/usr/bin/env bash
# Installs dot CLI and jq, imports signer accounts.
# Run this once before using other scripts. Chain aliases are registered by load-config.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV="${ENV:-local}"

# Source static config (no dot CLI needed yet)
source "$SCRIPT_DIR/config-base.env"
source "$SCRIPT_DIR/config-${ENV}.env"

ensure_dot_installed() {
  if ! command -v dot >/dev/null 2>&1; then
    echo "dot not found. Installing polkadot-cli@$REQUIRED_DOT_VERSION globally..."
    npm install -g "polkadot-cli@$REQUIRED_DOT_VERSION"
  fi

  if ! command -v dot >/dev/null 2>&1; then
    echo "ERROR: dot is still not available after install." >&2
    exit 1
  fi

  echo "dot is available: $(dot --version || echo unknown version)"
}

ensure_jq_installed() {
  if command -v jq >/dev/null 2>&1; then
    echo "jq is available: $(jq --version || echo unknown version)"
    return
  fi

  echo "jq not found. Attempting to install jq..."
  if command -v brew >/dev/null 2>&1; then
    brew install jq
  elif command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y jq
  else
    echo "ERROR: jq is not installed and no supported package manager was found." >&2
    echo "Install jq manually and rerun this script." >&2
    exit 1
  fi

  if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: jq is still not available after install." >&2
    exit 1
  fi

  echo "jq is available: $(jq --version || echo unknown version)"
}

ensure_bc_installed() {
  if command -v bc >/dev/null 2>&1; then
    echo "bc is available: $(bc --version 2>/dev/null | head -1 || echo unknown version)"
    return
  fi

  echo "ERROR: bc is not installed." >&2
  echo "Install bc and rerun this script." >&2
  exit 1
}

ensure_npm_installed() {
  if command -v npm >/dev/null 2>&1; then
    echo "npm is available: $(npm --version || echo unknown version)"
    return
  fi

  echo "ERROR: npm is not installed (required to install the dot CLI)." >&2
  echo "Install Node.js (which bundles npm) and rerun this script." >&2
  exit 1
}

ensure_people_sudo_account() {
  if ! dot account inspect $SIGNER_PEOPLE_SUDO >/dev/null 2>&1; then
    echo "'$SIGNER_PEOPLE_SUDO' account is not available, trying to add it..."
    if [[ -z "${PEOPLE_SUDO_MNEMONIC:-}" ]]; then
      echo "ERROR: PEOPLE_SUDO_MNEMONIC is not set" >&2
      exit 1
    fi
    dot account add $SIGNER_PEOPLE_SUDO --env PEOPLE_SUDO_MNEMONIC
    echo "Added $SIGNER_PEOPLE_SUDO account using PEOPLE_SUDO_MNEMONIC"
  fi

  echo "SIGNER_PEOPLE_SUDO is available"
}

ensure_ah_sudo_account() {
  if ! dot account inspect $SIGNER_AH_SUDO >/dev/null 2>&1; then
    echo "'$SIGNER_AH_SUDO' account is not available, trying to add it..."
    if [[ -z "${AH_SUDO_MNEMONIC:-}" ]]; then
      echo "ERROR: AH_SUDO_MNEMONIC is not set" >&2
      exit 1
    fi
    dot account add $SIGNER_AH_SUDO --env AH_SUDO_MNEMONIC
    echo "Added $SIGNER_AH_SUDO account using AH_SUDO_MNEMONIC"
  fi

  echo "SIGNER_AH_SUDO is available"
}

ensure_attestation_account() {
  if ! dot account inspect $SIGNER_ATTESTATION >/dev/null 2>&1; then
    echo "'$SIGNER_ATTESTATION' account is not available, trying to add it..."
    if [[ -z "${ATTESTATION_MNEMONIC:-}" ]]; then
      echo "ERROR: ATTESTATION_MNEMONIC is not set" >&2
      exit 1
    fi
    dot account add $SIGNER_ATTESTATION --env ATTESTATION_MNEMONIC
    echo "Added $SIGNER_ATTESTATION account using ATTESTATION_MNEMONIC"
  fi
  echo "SIGNER_ATTESTATION is available"
}

ensure_asset_owner_account() {
  if ! dot account inspect $SIGNER_ASSET_OWNER >/dev/null 2>&1; then
    echo "'$SIGNER_ASSET_OWNER' account is not available, trying to add it..."
    if [[ -z "${ASSET_OWNER_MNEMONIC:-}" ]]; then
      echo "ERROR: ASSET_OWNER_MNEMONIC is not set" >&2
      exit 1
    fi
    dot account add $SIGNER_ASSET_OWNER --env ASSET_OWNER_MNEMONIC
    echo "Added $SIGNER_ASSET_OWNER account using ASSET_OWNER_MNEMONIC"
  fi

  echo "SIGNER_ASSET_OWNER is available"
}

ensure_npm_installed
ensure_dot_installed
ensure_jq_installed
ensure_bc_installed
ensure_people_sudo_account
ensure_ah_sudo_account
ensure_attestation_account
ensure_asset_owner_account
