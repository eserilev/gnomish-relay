#!/usr/bin/env bash
# Writes addon/tests/api.lua, the API of the WoW Forever client (SPEC.md 7.8).
# The tests and the lint check use it, so a call to a function that the client
# does not have fails here, not in the game.
#
# With no argument it uses the pinned commits below. `--latest` uses the newest
# commit of each `forever` branch: the nightly job does this to find client patches.
set -euo pipefail

UI_REPO=https://github.com/Gethe/wow-ui-source
UI_COMMIT=bd2470aed543f72697a044e989285b6c83e63f73 # forever, 1.60.1.70009
BIR_REPO=https://github.com/Ketho/BlizzardInterfaceResources
BIR_COMMIT=659e8042049df854c114714f8ecd640823a1cd5c # forever, 1.60.1.69913

root=$(git rev-parse --show-toplevel)
work=$root/target/wow-api

if [ "${1:-}" = "--latest" ]; then
  UI_COMMIT=forever
  BIR_COMMIT=forever
fi

# A shallow fetch of one commit. It skips the fetch when the folder has it already.
fetch() {
  local dir=$1 repo=$2 commit=$3
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" = "$commit" ]; then
    return
  fi
  rm -rf "$dir"
  git init -q "$dir"
  git -C "$dir" fetch -q --depth 1 "$repo" "$commit"
  git -C "$dir" checkout -q FETCH_HEAD
}

fetch "$work/ui" "$UI_REPO" "$UI_COMMIT"
fetch "$work/bir" "$BIR_REPO" "$BIR_COMMIT"
build=$(tr -d '[:space:]' < "$work/ui/version.txt")

# The TOC Interface number of 1.60.1 is 16001: major * 10000 + minor * 100 + patch.
IFS=. read -r major minor patch _ <<< "$build"
interface=$((major * 10000 + minor * 100 + patch))
toc=$(sed -n 's/^## Interface: *\([0-9]*\).*/\1/p' "$root/addon/GnomishRelay/GnomishRelay.toc")
if [ "$toc" != "$interface" ]; then
  echo "error: the client is $build, so the TOC needs ## Interface: $interface (it has $toc)" >&2
  exit 1
fi
# A failed check leaves the old file in place.
python3 "$root/scripts/wow-api.py" "$work/ui" "$work/bir" "$root/addon/GnomishRelay" "$build" \
  > "$work/api.lua"
mv "$work/api.lua" "$root/addon/tests/api.lua"
echo "wrote addon/tests/api.lua for $build"
