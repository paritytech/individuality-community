#!/usr/bin/env bash
# Creates the people collection on People.
set -euo pipefail
source ./load-config.sh

echo "-> Create people collection on People"
people_collection_created=$(dot people.query.People.PeopleCollectionCreated)
if [ "$people_collection_created" == "true" ]; then
  echo "People collection already created, skipping"
else
  echo "People collection not created, creating"
  dot people.tx.People.create_people_collection --unsigned
fi
