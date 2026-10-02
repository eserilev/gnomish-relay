#!/usr/bin/env bash
# Writes addon/tests/api.lua and addon/tests/api-signatures.lua, the API of the WoW
# Forever client that the addon uses (SPEC.md 7.8). The tests and the lint check use
# them, so a call to a function that the client does not have fails here, not in the game.
#
# `--client anniversary` checks TBC Anniversary instead (SPEC.md 7.9), and writes
# api-anniversary.lua and api-signatures-anniversary.lua. With no `--latest` it uses
# the pinned commits below. `--latest` uses the newest commit of the branch of the
# client: the nightly job does this to find client patches.
#
# Another addon repo runs this script from its own root with its own paths:
#   wow-api.sh [--latest] --addon <folder> [--addon <folder>]... [--lint <wow.yml>]
#              [--fake <wow.lua>] --api <api.lua> --signatures <api-signatures.lua>
# Each `--addon` folder with a TOC is an addon. A folder without one is shared code.
# `--lint` and `--fake` are optional there. Paths are relative to the repo root.
set -euo pipefail

UI_REPO=https://github.com/Gethe/wow-ui-source
BIR_REPO=https://github.com/Ketho/BlizzardInterfaceResources

scripts=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(git rev-parse --show-toplevel)
cd "$root"

client=forever
latest=no
addons=()
lint=""
fake=""
api=""
signatures=""
custom=no
while [ $# -gt 0 ]; do
  case $1 in
    --latest) latest=yes ;;
    --client) client=$2; shift ;;
    --addon) addons+=("$2"); custom=yes; shift ;;
    --lint) lint=$2; custom=yes; shift ;;
    --fake) fake=$2; custom=yes; shift ;;
    --api) api=$2; custom=yes; shift ;;
    --signatures) signatures=$2; custom=yes; shift ;;
    *) echo "error: unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done
case $client in
  forever)
    UI_COMMIT=966519cf0ad2c10301ea011a88c14b25697c9687 # forever, 1.60.1.70124
    BIR_COMMIT=4149af6437af8631d045f3c6add51555fba3d784 # forever, 1.60.1.70009
    branch=forever title="WoW Forever" suffix=""
    ;;
  anniversary)
    UI_COMMIT=1463c686270b6c64e2c5c228f447c4597c0f8ba6 # classic_anniversary, 2.5.6.69795
    BIR_COMMIT=d6d4a8f445f198c5c73ff5f8f5002ad8f04451e4 # classic_anniversary, 2.5.6.68575
    branch=classic_anniversary title="WoW Classic: TBC Anniversary" suffix="-anniversary"
    ;;
  *) echo "error: unknown client $client: give forever or anniversary" >&2; exit 2 ;;
esac
if [ "$latest" = yes ]; then
  UI_COMMIT=$branch BIR_COMMIT=$branch
fi
work=$root/target/wow-api$suffix

if [ "$custom" = no ]; then
  addons=(addon/GnomishRelay addon/transport)
  lint=wow.yml
  fake=addon/tests/wow.lua
  api=addon/tests/api$suffix.lua
  signatures=addon/tests/api-signatures$suffix.lua
fi
if [ ${#addons[@]} -eq 0 ] || [ -z "$api" ] || [ -z "$signatures" ]; then
  echo "error: give --addon, --api, and --signatures, or no path at all" >&2
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
# A TOC lists the number of every client, so the number of this one must be in the list.
IFS=. read -r major minor patch _ <<< "$build"
interface=$((major * 10000 + minor * 100 + patch))
for folder in "${addons[@]}"; do
  for toc_file in "$folder"/*.toc; do
    [ -e "$toc_file" ] || continue
    toc=$(sed -n 's/^## Interface: *//p' "$toc_file" | tr -d ' \r')
    case ",$toc," in
      *",$interface,"*) ;;
      *)
        echo "error: the client is $build, so $toc_file needs $interface in ## Interface (it has $toc)" >&2
        exit 1
        ;;
    esac
  done
done

options=()
for folder in "${addons[@]}"; do
  options+=(--addon "$folder")
done
[ -z "$lint" ] || options+=(--lint "$lint")
[ -z "$fake" ] || options+=(--fake "$fake")
# The script writes both files only when every check passes.
python3 "$scripts/wow-api.py" --ui "$work/ui" --bir "$work/bir" --build "$build" \
  --client "$title" "${options[@]}" --api "$api" --signatures "$signatures"
echo "wrote $api and $signatures for $build"
