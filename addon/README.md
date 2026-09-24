# GnomishRelay addon

The addon side of Gnomish Relay. It is at step 5 of the build order (SPEC.md 15):
it loads a slot and shows the replies in it.

## Try step 5

1. Close WoW.
2. Run `scripts/dev-link.sh`. It links this folder into the game and prints the
   `GNOMISH_ADDONS` line for the next step.
3. Run `export GNOMISH_ADDONS=...` with the printed folder.
4. Run `cargo run -q --bin gnomish-relay -- install`. It makes the 200 slot addons.
5. Start WoW. On the character screen, open AddOns and make sure that "Gnomish Relay" is on.
6. Log in, and type `/relay poll`. The chat shows `no replies yet`.
7. In a terminal, run `cargo run -q --bin gnomish-relay -- say "hello from the bridge"`.
8. Type `/relay poll` again. The chat shows the reply.
9. Type `/relay status`. It shows `2 of 200 slots used`.

Each poll uses one slot. `/reload` frees all of them.
