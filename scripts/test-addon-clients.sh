#!/usr/bin/env bash
# Runs the addon tests once more in the fake game of each client other than Forever
# (SPEC.md 7.9). Each run takes the fixture and the API file of its client.
# The strip and line suites stay on Forever: they check drawing math against the
# screen size of the fixture, and the screen size is no part of the client.
set -euo pipefail
suites=(--test addon_flow --test addon_notices --test addon_chat_tools --test addon_git --test addon_update)
for client in anniversary; do
  echo "addon tests in the fake game of $client"
  GNOMISH_TEST_CLIENT=$client cargo test -q -p bridge "${suites[@]}"
done
