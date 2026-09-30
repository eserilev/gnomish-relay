#!/usr/bin/env bash
# Makes the folder of the relay addon that CurseForge ships (SPEC.md 11.3):
#   scripts/package-addon.sh <out-dir>    makes <out-dir>/GnomishRelay
# It holds the files of addon/GnomishRelay and the shared transport as real files. It never
# holds a key addon or a slot: the desktop app writes those next to it on each computer.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
out=${1:?usage: package-addon.sh <out-dir>}
dir=$out/GnomishRelay

rm -rf "$dir"
mkdir -p "$dir"
# -L copies the file behind a link, so the transport links of scripts/dev-link.sh come out
# as real files. The transport goes last, so a stale link never wins.
for file in "$root"/addon/GnomishRelay/* "$root"/addon/transport/*.lua; do
  name=$(basename "$file")
  # A developer checkout can still hold the key file of an older setup.
  [ "$name" = Key.lua ] && continue
  cp -L "$file" "$dir/$name"
done
cp "$root/addon/GnomishRelay/.pkgmeta" "$dir/.pkgmeta"
echo "$dir"
