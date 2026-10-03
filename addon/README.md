# GnomishRelay addon

The addon side of Gnomish Relay. SPEC.md section 13 has its design.

## Try it

1. Start WoW once to make the `Interface/AddOns` folder. Then close it.
2. Run `cargo run -q --bin gnomish-relay -- setup`. It finds the game, makes the strip key,
   installs this addon, writes `~/.config/gnomish-relay/config.toml` with the agents it
   finds, and makes the 1000 slot addons. `--autostart` also starts the bridge at each login.
3. Setup puts your code folders into `allowed_roots` in `config.toml`. To add another
   folder, pick it in the game and approve it on your desktop.
4. Start WoW and log in. Type `/relay` to open the window, or `/ai <message>` to send.
5. If you did not use `--autostart`, run `cargo run -q --bin gnomish-relay -- run`.

For development, use `scripts/dev-link.sh`. It links this folder into the game, so an edit
plus `/reload` loads the new code. It also links each file of `addon/transport` into
`addon/GnomishRelay`. Other apps share those files (SPEC.md 9.7). Install copies them
into this addon. Then the script runs `setup`, which writes only `Key.lua` here.

`say <chat> <id> <text>` publishes a reply by hand. `/relay diag` shows the ids.

## Tests

`cargo test -p bridge` runs the addon in Lua 5.1 with a fake WoW API (`tests/wow.lua`).
