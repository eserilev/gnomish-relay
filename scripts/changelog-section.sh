#!/usr/bin/env bash
# Prints the notes of one version from CHANGELOG.md, for the GitHub release and CurseForge:
#   scripts/changelog-section.sh <version> [changelog]
# The notes are the lines under `## <version>` up to the next `## ` heading. A version with
# no such heading, or with no text under it, fails, so a release never goes out without notes.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=${1:?usage: changelog-section.sh <version> [changelog]}
file=${2:-$root/CHANGELOG.md}
heading="## $version"

if [ ! -f "$file" ]; then
  echo "error: $file not found" >&2
  exit 1
fi
if ! grep -qxF -- "$heading" "$file"; then
  echo "error: CHANGELOG.md has no \"$heading\" section. Add one with the release notes." >&2
  exit 1
fi

# Leading blank lines go here, and the command substitution drops the trailing ones.
notes=$(awk -v heading="$heading" '
  /^## / { if (found) exit; found = ($0 == heading); next }
  found { print }
' "$file" | sed '/[^[:space:]]/,$!d')

if [ -z "$notes" ]; then
  echo "error: the \"$heading\" section of CHANGELOG.md is empty. Write the release notes." >&2
  exit 1
fi
printf '%s\n' "$notes"
