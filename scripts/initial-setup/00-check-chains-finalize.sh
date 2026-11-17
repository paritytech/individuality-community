#!/usr/bin/env bash
# Checks that Relay, People and AssetHub finalize new blocks.
set -euo pipefail
source ./load-config.sh
source ./utils.sh

FINALITY_TIMEOUT_SECONDS=600

check_chain_finalizes() {
  local chain="$1" start_block_number="" block_number deadline=$((SECONDS + FINALITY_TIMEOUT_SECONDS))
  echo "-> Check that $chain finalizes new blocks"
  while true; do
    block_number=$(run_with_timeout 30 dot "$chain.query.System.Number" || true)
    if [[ "$block_number" =~ ^[0-9]+$ ]]; then
      if [ -z "$start_block_number" ]; then
        start_block_number="$block_number"
      elif [ "$block_number" -gt "$start_block_number" ]; then
        echo "$chain finalized block #$block_number"
        return
      fi
    fi

    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "ERROR: $chain did not finalize a new block within ${FINALITY_TIMEOUT_SECONDS}s" >&2
      exit 1
    fi

    sleep 10
  done
}

check_chain_finalizes relay
check_chain_finalizes people
check_chain_finalizes asset-hub
