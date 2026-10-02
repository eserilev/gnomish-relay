#!/usr/bin/env bash
# The API gate of scripts/wow-api.sh for the self-test addon (SPEC.md 14.3). The self-test
# calls functions that the relay must never call, so it has its own lint list and its own
# API files. Arguments go to wow-api.sh, for example --latest.
# GnomishRelaySelfTest_Old is left out on purpose: its old Interface number is the test.
# The self-test has API files only for Forever, so `--client` would overwrite them.
set -euo pipefail
scripts=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
for arg in "$@"; do
  if [ "$arg" = --client ]; then
    echo "error: the self-test gate checks only Forever, so it takes no --client" >&2
    exit 2
  fi
done
exec "$scripts/wow-api.sh" "$@" \
  --addon addon/GnomishRelaySelfTest \
  --addon addon/GnomishRelaySelfTest_Slot \
  --addon addon/GnomishRelaySelfTest_Off \
  --addon addon/transport \
  --lint selftest.yml \
  --api addon/tests/selftest-api.lua \
  --signatures addon/tests/selftest-api-signatures.lua
