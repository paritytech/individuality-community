#!/usr/bin/env bash
# Adds sudo proxy delegation on People.
set -euo pipefail
source ./load-config.sh

check() {
  local delegates
  delegates=$(dot people.query.Proxy.Proxies "$ACCOUNT_PEOPLE_SUDO" | jq -r '.[0][]?.delegate // empty' 2>/dev/null || echo "")
  echo "Delegates for $ACCOUNT_PEOPLE_SUDO:"
  echo "$delegates"
  if echo "$delegates" | grep -qx "$ACCOUNT_PEOPLE_SUDO_PROXY"; then
    echo "$ACCOUNT_PEOPLE_SUDO_PROXY is already set"
    exit 0
  fi
}

run() {
  echo "Setting up sudo proxy for $ACCOUNT_PEOPLE_SUDO_PROXY"
  dot people.tx.Proxy.add_proxy "$ACCOUNT_PEOPLE_SUDO_PROXY" any 0 --from "$SIGNER_PEOPLE_SUDO"
}

check
run
