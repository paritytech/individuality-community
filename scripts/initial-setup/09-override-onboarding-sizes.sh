#!/usr/bin/env bash
# Overrides onboarding size for the people and people-lite collections on People.
# Must run after 08 (creates the people collection).
set -euo pipefail
source ./load-config.sh

override_onboarding_size() {
  local collection="$1" identifier="$2" current_size
  current_size=$(dot people.query.Members.OnboardingSize "$identifier")
  if [ "$current_size" == "$ONBOARDING_SIZE" ]; then
    echo "$collection onboarding size already $ONBOARDING_SIZE, skipping"
    return
  fi
  echo "$collection onboarding size is $current_size, setting to $ONBOARDING_SIZE"
  local set_call
  set_call=$(dot --encode people.tx.Members.set_onboarding_size "$identifier" "$ONBOARDING_SIZE")
  dot people.tx.Sudo.sudo "$set_call" --from "$SIGNER_PEOPLE_SUDO"
}

echo "-> Override onboarding sizes on People"
override_onboarding_size "people" "$PEOPLE_IDENTIFIER"
override_onboarding_size "people-lite" "$PEOPLE_LITE_IDENTIFIER"
