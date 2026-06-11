#!/usr/bin/env bash
# Adds attestation proxy account on People and AssetHub.
set -euo pipefail
source ./load-config.sh

setup_proxy() {
  local chain="$1"
  # SS58 encoding differs across chains (different prefixes for the same
  # pubkey), so normalize the target to the chain's prefix before comparing.
  local prefix target
  prefix=$(dot "${chain}".const.System.SS58Prefix)
  target=$(dot account inspect "$ACCOUNT_ATTESTATION_PROXY" --prefix "$prefix" --output json | jq -r '.ss58')

  local delegates
  delegates=$(dot "${chain}".query.Proxy.Proxies "$SIGNER_ATTESTATION" | jq -r '.[0][]?.delegate // empty' 2>/dev/null || echo "")
  echo "Delegates for $SIGNER_ATTESTATION on $chain:"
  echo "$delegates"
  if echo "$delegates" | grep -qx "$target"; then
    echo "Proxy $ACCOUNT_ATTESTATION_PROXY is already set on $chain"
    return
  fi
  echo "Adding proxy $ACCOUNT_ATTESTATION_PROXY on $chain"
  dot "${chain}".tx.Proxy.add_proxy "$ACCOUNT_ATTESTATION_PROXY" any 0 --from "$SIGNER_ATTESTATION"
}

setup_proxy people
setup_proxy asset-hub
