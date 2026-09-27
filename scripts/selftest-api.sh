#!/usr/bin/env bash
# The API gate of scripts/wow-api.sh for the self-test addon (SPEC.md 14.3). The self-test
# calls functions that the relay must never call, so it has its own lint list and its own
# API files. Arguments go to wow-api.sh, for example --latest.
# GnomishRelaySelfTest_Old is left out on purpose: its old Interface number is the test.
set -euo pipefail
scripts=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec "$scripts/wow-api.sh" "$@" \
  --addon addon/GnomishRelaySelfTest \
  --addon addon/GnomishRelaySelfTest_Slot \
  --addon addon/GnomishRelaySelfTest_Off \
  --addon addon/transport \
  --lint selftest.yml \
  --api addon/tests/selftest-api.lua \
  --signatures addon/tests/selftest-api-signatures.lua
