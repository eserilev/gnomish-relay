#!/usr/bin/env bash
# Tells CI which slow jobs a change needs, as GitHub step outputs: `proofs=true` and
# `model=true`. With no usable BASE commit, every job runs.
set -euo pipefail
base=${1:-}

if [ -z "$base" ] || [ "$base" = "0000000000000000000000000000000000000000" ] ||
  ! git cat-file -e "$base^{commit}" 2>/dev/null; then
  echo "proofs=true"
  echo "model=true"
  exit 0
fi

changed=$(git diff --name-only "$base" HEAD)
touches() {
  if grep -qE "$1" <<< "$changed"; then echo true; else echo false; fi
}
echo "proofs=$(touches '^(crates/protocol/|proofs/|scripts/(extract|check-proofs)\.sh|\.github/workflows/ci\.yml)')"
echo "model=$(touches '^(models/|scripts/check-model\.sh|\.github/workflows/ci\.yml)')"
