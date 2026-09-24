# GnomishRelay addon

The addon side of Gnomish Relay. SPEC.md section 13 describes it.

## Try it

1. Close WoW.
2. Run `scripts/dev-link.sh`. It links this folder into the game, makes the strip key,
   and prints the `GNOMISH_ADDONS` line for the next step.
3. Run `export GNOMISH_ADDONS=...` with the printed folder.
4. Run `cargo run -q --bin gnomish-relay -- install`. It makes the 1000 slot addons.
5. Start WoW, make sure that "Gnomish Relay" is on in the AddOns list, and log in.
6. Type `/relay` to open the window, or `/ai <message>` to send from the chat line.

7. In a terminal, run `cargo run -q --bin gnomish-relay -- run`. It reads the strips,
   answers each message with the echo agent, and publishes the reply.
8. Type a message in the window. The reply "echo: <your message>" comes back as a
   whisper at the next poll, 5 to 10 seconds later.

`say <chat> <id> <text>` publishes a reply by hand. `/relay diag` shows the ids.

Older versions made 200 slots named `GnomishRelay_S001` to `S200`. With the game closed, delete them:
`rm -r "$GNOMISH_ADDONS"/GnomishRelay_S[0-9][0-9][0-9]`

## Tests

`cargo test -p bridge` runs the addon in Lua 5.1 with a fake WoW API (`tests/wow.lua`).
