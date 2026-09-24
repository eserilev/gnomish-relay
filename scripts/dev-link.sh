#!/usr/bin/env bash
# Links the addon into the game, so an edit plus /reload loads the new code (SPEC.md 16).
# It also makes the strip key once, for the addon and the bridge (SPEC.md 6.3).
# WoW finds addons only at launch, so run this with the game closed.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
wow=${WOW_DIR:-$HOME/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_}
addons="$wow/Interface/AddOns"
config=${XDG_CONFIG_HOME:-$HOME/.config}/gnomish-relay
key_file="$config/strip.key"
key_lua="$root/addon/GnomishRelay/Key.lua"

if [ ! -d "$addons" ]; then
  echo "error: $addons not found. Set WOW_DIR to the _classic_beta_ folder." >&2
  exit 1
fi

if [ ! -f "$key_file" ]; then
  mkdir -p "$config"
  (umask 077 && od -An -tx1 -N32 /dev/urandom | tr -d ' \n' > "$key_file")
fi
key=$(cat "$key_file")
(umask 077 && cat > "$key_lua" <<LUA
local _, ns = ...
ns.key = ("$key"):gsub("%x%x", function(h)
	return string.char(tonumber(h, 16))
end)
LUA
)

ln -sfn "$root/addon/GnomishRelay" "$addons/GnomishRelay"
echo "linked $addons/GnomishRelay"
if [ ! -f "$config/config.toml" ]; then
  cargo run -q --bin gnomish-relay -- setup "$wow"
fi
