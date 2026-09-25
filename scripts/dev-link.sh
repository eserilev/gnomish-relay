#!/usr/bin/env bash
# Links the addon into the game, so an edit plus /reload loads the new code (SPEC.md 16).
# Then `setup` makes the strip key once, writes Key.lua here, and makes the slots.
# WoW finds addons only at launch, so run this with the game closed.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
wow=${WOW_DIR:-$HOME/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_}
addons="$wow/Interface/AddOns"

if [ ! -d "$addons" ]; then
  echo "error: $addons not found. Set WOW_DIR to the _classic_beta_ folder." >&2
  exit 1
fi

ln -sfn "$root/addon/GnomishRelay" "$addons/GnomishRelay"
echo "linked $addons/GnomishRelay"
# The link first: setup then writes only the key into this checkout.
cargo run -q --bin gnomish-relay -- setup "$wow"
