#!/usr/bin/env bash

# Integer >= comparison that handles arbitrary-precision positive integers.
# Needed because bash's [ -ge ] silently breaks past int64 (~9.2e18), and on-chain
# balances routinely exceed that (e.g. a 21M supply of a 18-decimal asset is ~2.1e25).
int_ge() {
  [ "$(echo "$1 >= $2" | bc)" = "1" ]
}

# Returns 0 and echoes the value if the on-chain query result is not "undefined".
chain_value_exists() {
  local result
  result=$(dot "$@")
  if [ "$result" != "undefined" ]; then
    echo "$result"
    return 0
  fi
  return 1
}

# Checks if an asset exists on a given chain.
asset_exists() {
  local chain="$1"
  local asset_id="$2"
  chain_value_exists "${chain}.query.Assets.Asset" "$asset_id"
}

has_funds() {
  local chain="$1"
  local account="$2"
  local funds

  funds=$(dot "${chain}.query.System.Account" "$account" --output json | jq -r '.data.free // 0')
  if [ "$funds" -eq 0 ]; then
    return 1
  else
    return 0
  fi
}

has_hrmp_channel() {
  local src="$1"
  local dest="$2"
  local channel

  channel=$(dot relay.query.Hrmp.HrmpChannels "$src" "$dest")
  if [ "$channel" != "undefined" ]; then
    return 0
  else
    return 1
  fi
}

# Echoes an asset's local location on AssetHub.
ah_local_location() {
  local asset_id="$1"
  echo '{"parents":0,"interior":{"type":"X2","value":[{"type":"PalletInstance","value":50},{"type":"GeneralIndex","value":"'"$asset_id"'"}]}}'
}

# Echoes an asset's foreign location on People.
people_foreign_location() {
  local asset_id="$1"
  echo '{"parents":1,"interior":{"type":"X3","value":[{"type":"Parachain","value":'"$PARACHAIN_ID_ASSET_HUB"'},{"type":"PalletInstance","value":50},{"type":"GeneralIndex","value":"'"$asset_id"'"}]}}'
}

# Echoes an account's balance of an asset on a chain, or 0 if it holds none.
assets_account_balance() {
  local chain="$1" asset="$2" account="$3" result
  result=$(dot "$chain.query.Assets.Account" "$asset" "$account")
  [ "$result" = "undefined" ] && echo 0 || echo "$result" | jq -r '.balance // 0'
}

# Runs a command with a portable timeout. Returns 124 on timeout.
run_with_timeout() {
  local timeout_seconds="$1"
  shift

  if [ "$timeout_seconds" -le 0 ]; then
    "$@" || return $?
    return 0
  fi

  "$@" &
  local pid=$!
  local deadline=$((SECONDS + timeout_seconds))
  local status=0

  while kill -0 "$pid" 2>/dev/null; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "ERROR: command timed out after ${timeout_seconds}s: $1 ..." >&2
      kill "$pid" 2>/dev/null || true
      kill -KILL "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      return 124
    fi
    sleep 1
  done

  wait "$pid" || status=$?
  return "$status"
}
