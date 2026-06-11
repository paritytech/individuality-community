#!/usr/bin/env bash
# Adds Proof of Ink design families from snapshot on People.
set -euo pipefail
source ./load-config.sh

SNAPSHOT_FILE="./poi-design-families.json"

entry_count="$(jq 'length' "$SNAPSHOT_FILE")"
echo "-> Adding design families from snapshot: $SNAPSHOT_FILE"
echo "-> Entries: $entry_count"

# Collect the encoded calls so they can be submitted in one batch.
add_calls=()
while IFS= read -r entry; do
  index="$(echo "$entry" | jq -r '.index')"
  id="$(echo "$entry" | jq -r '.value.id')"
  kind_type="$(echo "$entry" | jq -r '.value.kind')"
  if echo "$entry" | jq -e '.value | has("value")' >/dev/null; then
    kind="$(echo "$entry" | jq -c '.value | { type: .kind, value: .value }')"
  else
    kind="$kind_type"
  fi

  existing_value="$(dot people.query.ProofOfInk.DesignFamilies "$index")"
  if [ "$existing_value" == "undefined" ]; then
    echo "-> index=$index is undefined, queuing add (kind=$kind id=$id)"
    add_calls+=("$(dot --encode people.tx.ProofOfInk.add_design_family "$index" "$kind" "$id")")
  else
    echo "-> index=$index already set, skipping"
  fi
done < <(jq -c '.[]' "$SNAPSHOT_FILE")

call_count=${#add_calls[@]}
if [ "$call_count" -eq 0 ]; then
  echo "-> All design families already added"
  exit 0
fi

# dot can't build a Vec<Call>, so assemble Utility.batch_all by hand:
# the prefix (from encoding it with no calls), then a SCALE-compact
# count, then the concatenated encoded calls.
batch_call=$(dot --encode people.tx.Utility.batch_all '[]')
batch_call=${batch_call%00}
batch_call+=$(printf '%02x' $((call_count << 2)))
for add_call in "${add_calls[@]}"; do
  batch_call+="${add_call#0x}"
done

echo "-> Adding $call_count design families in one batch"
dot people.tx.Sudo.sudo "$batch_call" --from "$SIGNER_PEOPLE_SUDO"
