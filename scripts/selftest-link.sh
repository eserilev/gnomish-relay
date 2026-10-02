#!/usr/bin/env bash
# Links the self-test addon and its helper addons into the game (SPEC.md 14.3). It is
# for developers only: install and the release never ship it.
# WoW finds addons only at launch, so run this with the game closed.
# `--remove` takes the links out of the game again.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
wow=${WOW_DIR:-$HOME/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_}
addons="$wow/Interface/AddOns"
folders=(GnomishRelaySelfTest GnomishRelaySelfTest_Slot GnomishRelaySelfTest_Off GnomishRelaySelfTest_Old)
shared=(Sha256.lua Codec.lua Saved.lua Health.lua Strip.lua)

if [ ! -d "$addons" ]; then
  echo "error: $addons not found. Set WOW_DIR to the client folder: _classic_beta_ or _anniversary_." >&2
  exit 1
fi

if [ "${1:-}" = "--remove" ]; then
  for folder in "${folders[@]}"; do
    if [ -L "$addons/$folder" ]; then
      rm "$addons/$folder"
      echo "removed $addons/$folder"
    fi
  done
  exit 0
fi

for folder in "${folders[@]}"; do
  ln -sfn "$root/addon/$folder" "$addons/$folder"
  echo "linked $addons/$folder"
done
# The self-test draws its strips with the real shared transport. The links are ignored files.
for file in "${shared[@]}"; do
  ln -sfn "../transport/$file" "$root/addon/GnomishRelaySelfTest/$file"
done
echo "linked addon/transport into addon/GnomishRelaySelfTest"
echo "Start the game and log in. Wait for \"done\", type /reload, then run: gnomish-relay selftest collect"
