#!/usr/bin/env bash
# Tells CI which jobs a change needs, as GitHub step outputs: `code=true`, `proofs=true`,
# and `model=true`. With no usable BASE commit, every job runs.
set -euo pipefail
base=${1:-}

if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ] ||
  ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "code=true"
  echo "proofs=true"
  echo "model=true"
  exit 0
fi

changed=$(git diff --name-only "$base" HEAD)
touches() {
  if grep -qE "$1" <<< "$changed"; then echo true; else echo false; fi
}
# A change of only docs, images, or the license needs no build and no test.
if grep -qvE '(\.md|^images/.*|^LICENSE.*)$' <<< "$changed"; then echo "code=true"; else echo "code=false"; fi
echo "proofs=$(touches '^(crates/protocol/|proofs/|scripts/(extract|check-proofs)\.sh|\.github/workflows/ci\.yml)')"
echo "model=$(touches '^(models/|scripts/check-model\.sh|\.github/workflows/ci\.yml)')"
