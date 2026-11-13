#!/usr/bin/env bash
# Adds the attestation account's proxies: the writer on People and AssetHub,
# and the inviter on People.
set -euo pipefail
source ./load-config.sh

setup_proxy() {
  local chain="$1"
  local delegate="$2"
  # SS58 encoding differs across chains (different prefixes for the same
  # pubkey), so normalize the target to the chain's prefix before comparing.
  local prefix target
  prefix=$(dot "${chain}".const.System.SS58Prefix)
  target=$(dot account inspect "$delegate" --prefix "$prefix" --output json | jq -r '.ss58')

  local delegates
  delegates=$(dot "${chain}".query.Proxy.Proxies "$SIGNER_ATTESTATION" | jq -r '.[0][]?.delegate // empty' 2>/dev/null || echo "")
  echo "Delegates for $SIGNER_ATTESTATION on $chain:"
  echo "$delegates"
  if echo "$delegates" | grep -qx "$target"; then
    echo "Proxy $delegate is already set on $chain"
    return
  fi
  echo "Adding proxy $delegate on $chain"
  dot "${chain}".tx.Proxy.add_proxy "$delegate" any 0 --from "$SIGNER_ATTESTATION"
}

setup_proxy people "$ACCOUNT_ATTESTATION_WRITER"
setup_proxy people "$ACCOUNT_ATTESTATION_INVITER"
setup_proxy asset-hub "$ACCOUNT_ATTESTATION_WRITER"
