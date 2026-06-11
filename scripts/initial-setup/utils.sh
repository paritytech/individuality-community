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

# Echoes the native units needed to buy a given amount of an asset; returns non-zero on failure.
quote_native_for_exact() {
  local asset_id="$1" amount_out="$2" quote
  quote=$(dot asset-hub.apis.AssetConversionApi.quote_price_tokens_for_exact_tokens \
    "$NATIVE_TOKEN" "$(ah_local_location "$asset_id")" "$amount_out" true --json 2>/dev/null || true)
  quote=$(echo "$quote" | tr -d '"')
  [ -z "$quote" ] || [ "$quote" = "null" ] || [ "$quote" = "undefined" ] && return 1
  echo "$quote"
}
