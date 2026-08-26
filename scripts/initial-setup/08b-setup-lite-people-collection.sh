#!/usr/bin/env bash
# Creates the lite people collection on People.
set -euo pipefail
source ./load-config.sh

echo "-> Create lite people collection on People"
lite_people_collection_created=$(dot people.query.PeopleLite.LitePeopleCollectionCreated)
if [ "$lite_people_collection_created" == "true" ]; then
  echo "Lite people collection already created, skipping"
else
  echo "Lite people collection not created, creating"
  dot people.tx.PeopleLite.create_lite_people_collection --unsigned
fi
