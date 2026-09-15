#!/usr/bin/env bash
# Compares the usernames held by the `resources` pallet on People Chain with the `DotnsGateway` pallet
# storage on Asset Hub.
# Every row it prints is a name users see on People Chain today and lose when the app
# switches its reads to Asset Hub, ahead of the release that removes usernames from `Resources`
# (`indiv_pallet_resources::migration::MigrateV0ToV1`). The rows size that impact and, if a
# reconciliation is decided on, are its input.
#
# One-off: delete this script together with the follow-up release that clears the orphaned username
# storage.
#
# Usage:
#   RPC_PEOPLE=wss://... RPC_ASSET_HUB=wss://... scripts/check-username-sync.sh
#
# Environment:
#   RPC_PEOPLE      People chain RPC endpoint (required unless every dump is already in DUMP_DIR)
#   RPC_ASSET_HUB   Asset Hub RPC endpoint (required unless every dump is already in DUMP_DIR)
#   DUMP_DIR        Where the storage dumps are written, one file per map in `DUMPS`. When a dump
#                   exists there for every map, the chains are not contacted and the analysis runs
#                   on the files. Defaults to a temp dir.
#
# Output: one row per finding on stdout, `category<TAB>account<TAB>resources_name<TAB>dotns_name`,
# with the summary on stderr. Exits 1 when any gap is found. Rows in the `reservation_unverified`
# category are informational and never fail the check.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/initial-setup/config-base.env"

PEOPLE_ALIAS=username-check-people
ASSET_HUB_ALIAS=username-check-asset-hub
DUMP_DIR="${DUMP_DIR:-$(mktemp -d)}"
DUMPS=(consumers reservation_of username_reservation_queue account_names lite_label_owner)

for cmd in dot python3; do
  command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: '$cmd' not found" >&2; exit 1; }
done
dot_version=$(dot --version | sed -E 's#^dot/([^ ]+).*#\1#')
if [ "$dot_version" != "$REQUIRED_DOT_VERSION" ]; then
  echo "ERROR: dot $REQUIRED_DOT_VERSION required, found $dot_version" >&2
  exit 1
fi

all_dumps_present() {
  local name
  for name in "${DUMPS[@]}"; do
    [ -s "$DUMP_DIR/$name.json" ] || return 1
  done
}

# Registers a fresh alias so a stale endpoint from an earlier run is never reused.
register_chain() {
  local alias="$1" rpc="$2"
  dot chain remove "$alias" >/dev/null 2>&1 || true
  dot chain add "$alias" --rpc "$rpc" >/dev/null
}

# Dumps one storage map at the chain's finalized head pinned by the caller.
dump_map() {
  local alias="$1" at="$2" path="$3" name="$4"
  echo "Dumping $path at $at" >&2
  dot "$alias.query.$path" --dump --at "$at" > "$DUMP_DIR/$name.json"
}

if all_dumps_present; then
  echo "Reusing dumps in $DUMP_DIR" >&2
else
  : "${RPC_PEOPLE:?RPC_PEOPLE is required}"
  : "${RPC_ASSET_HUB:?RPC_ASSET_HUB is required}"
  mkdir -p "$DUMP_DIR"
  register_chain "$PEOPLE_ALIAS" "$RPC_PEOPLE"
  register_chain "$ASSET_HUB_ALIAS" "$RPC_ASSET_HUB"

  people_head=$(dot "$PEOPLE_ALIAS.rpc.chain_getFinalizedHead" | tr -d '"')
  asset_hub_head=$(dot "$ASSET_HUB_ALIAS.rpc.chain_getFinalizedHead" | tr -d '"')
  dump_map "$PEOPLE_ALIAS" "$people_head" Resources.Consumers consumers
  dump_map "$PEOPLE_ALIAS" "$people_head" Resources.ReservationOf reservation_of
  dump_map "$PEOPLE_ALIAS" "$people_head" Resources.UsernameReservationQueue username_reservation_queue
  dump_map "$ASSET_HUB_ALIAS" "$asset_hub_head" DotnsGateway.AccountNames account_names
  dump_map "$ASSET_HUB_ALIAS" "$asset_hub_head" DotnsGateway.LiteLabelOwner lite_label_owner
  echo "Dumps written to $DUMP_DIR" >&2
fi

# The two chains render accounts with their own SS58 prefixes, so the join happens on public keys.
python3 - "$DUMP_DIR" <<'PY'
import json
import sys
from collections import Counter
from pathlib import Path

dump_dir = Path(sys.argv[1])
GAP_CATEGORIES = {
    "lite_missing",
    "lite_suffix_differs",
    "lite_owner_mismatch",
    "full_missing",
    "full_differs",
}
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def public_key(ss58):
    """Returns the hex public key of an SS58 address, dropping prefix and checksum."""
    n = 0
    for ch in ss58:
        n = n * 58 + B58.index(ch)
    raw = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return raw[-34:-2].hex()


def load(name):
    with open(dump_dir / f"{name}.json") as f:
        return [(entry["keys"][0], entry["value"]) for entry in json.load(f)]


def stem(label):
    return label.split(".", 1)[0]


consumers = load("consumers")
reservation_of = load("reservation_of")
queues = dict(load("username_reservation_queue"))
account_names = {public_key(account): record for account, record in load("account_names")}
lite_label_owner = {label: (public_key(account), account) for label, account in load("lite_label_owner")}

rows = []
for account, info in consumers:
    key = public_key(account)
    record = account_names.get(key, {})
    lite = info["lite_username"]
    dotns_lite = record.get("lite", {}).get("label")
    owner, owner_ss58 = lite_label_owner.get(lite, (None, None))
    if dotns_lite == lite and owner in (None, key):
        category, detail = "lite_ok", dotns_lite
    elif dotns_lite is not None and stem(dotns_lite) == stem(lite):
        category, detail = "lite_suffix_differs", dotns_lite
    elif owner is not None and owner != key:
        category, detail = "lite_owner_mismatch", f"owned_by={owner_ss58}"
    else:
        category, detail = "lite_missing", ""
    rows.append((category, account, lite, detail))

    full = info.get("full_username")
    if full is not None:
        dotns_full = record.get("full", {}).get("label")
        if dotns_full == full:
            category = "full_ok"
        elif dotns_full is not None:
            category = "full_differs"
        else:
            category = "full_missing"
        rows.append((category, account, full, dotns_full or ""))

for account, reserved in reservation_of:
    queue = queues.get(reserved, [])
    position = next((i for i, e in enumerate(queue) if e["account"] == account), None)
    detail = f"queue_position={position}" if position is not None else "not_in_queue"
    rows.append(("reservation_unverified", account, reserved, detail))

rows.sort()
for row in rows:
    print("\t".join(row))

counts = Counter(row[0] for row in rows)
print("Summary:", file=sys.stderr)
for category in sorted(counts):
    print(f"  {category}: {counts[category]}", file=sys.stderr)
gaps = sum(counts[c] for c in GAP_CATEGORIES)
if gaps:
    print(f"FAIL: {gaps} username(s) in Resources are not held in DotnsGateway", file=sys.stderr)
    sys.exit(1)
print("OK: every Resources username is held in DotnsGateway", file=sys.stderr)
PY
