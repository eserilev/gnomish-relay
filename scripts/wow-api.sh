#!/usr/bin/env bash
# Writes addon/tests/api.lua, the API of the WoW Forever client that the addon uses
# (SPEC.md 7.8). The tests and the lint check use it, so a call to a function that
# the client does not have fails here, not in the game.
#
# With no argument it uses the pinned commits below. `--latest` uses the newest
# commit of each `forever` branch: the nightly job does this to find client patches.
#
# Another addon repo runs this script from its own root with its own paths:
#   wow-api.sh [--latest] --addon <folder> [--addon <folder>]... [--lint <wow.yml>]
#              [--fake <wow.lua>] --api <api.lua>
# Each `--addon` folder with a TOC is an addon. A folder without one is shared code.
# `--lint` and `--fake` are optional there. Paths are relative to the repo root.
set -euo pipefail

UI_REPO=https://github.com/Gethe/wow-ui-source
UI_COMMIT=bd2470aed543f72697a044e989285b6c83e63f73 # forever, 1.60.1.70009
BIR_REPO=https://github.com/Ketho/BlizzardInterfaceResources
BIR_COMMIT=659e8042049df854c114714f8ecd640823a1cd5c # forever, 1.60.1.69913

scripts=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(git rev-parse --show-toplevel)
cd "$root"
work=$root/target/wow-api

addons=()
lint=""
fake=""
api=""
custom=no
while [ $# -gt 0 ]; do
  case $1 in
    --latest) UI_COMMIT=forever BIR_COMMIT=forever ;;
    --addon) addons+=("$2"); custom=yes; shift ;;
    --lint) lint=$2; custom=yes; shift ;;
    --fake) fake=$2; custom=yes; shift ;;
    --api) api=$2; custom=yes; shift ;;
    *) echo "error: unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done
if [ "$custom" = no ]; then
  addons=(addon/GnomishRelay addon/transport)
  lint=wow.yml
  fake=addon/tests/wow.lua
  api=addon/tests/api.lua
fi
if [ ${#addons[@]} -eq 0 ] || [ -z "$api" ]; then
  echo "error: give --addon and --api, or no path at all" >&2
  exit 2
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
for folder in "${addons[@]}"; do
  for toc_file in "$folder"/*.toc; do
    [ -e "$toc_file" ] || continue
    toc=$(sed -n 's/^## Interface: *\([0-9]*\).*/\1/p' "$toc_file")
    if [ "$toc" != "$interface" ]; then
      echo "error: the client is $build, so $toc_file needs ## Interface: $interface (it has $toc)" >&2
      exit 1
    fi
  done
done

options=()
for folder in "${addons[@]}"; do
  options+=(--addon "$folder")
done
[ -z "$lint" ] || options+=(--lint "$lint")
[ -z "$fake" ] || options+=(--fake "$fake")
# The script writes the file only when every check passes.
python3 "$scripts/wow-api.py" --ui "$work/ui" --bir "$work/bir" --build "$build" \
  "${options[@]}" --api "$api"
echo "wrote $api for $build"
