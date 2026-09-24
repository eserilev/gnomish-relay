# Spikes

Small test addons. Their results decide the transport design (SPEC.md section 15, steps 1 and 3).

| Addon | Question |
|---|---|
| `GRSpikeShot` | Can an addon call `Screenshot()` with no key press? Does the strip survive in the image? |
| `GRSpikeFiles` | Do the five client file-load rules (SPEC.md 7.2) hold under Wine? |
| `GRSpikeSize` | How long does the game stall when it loads a big slot body? |

`spike.py` installs the addons, changes the test files, reads the screenshot, and prints the saved results.
If the game is not in the default Lutris prefix, set `WOW_DIR` to the `_classic_beta_` folder.

## Procedure

Do these steps in one game session.

1. Close WoW.
2. Run `spikes/spike.py install`.
3. Start WoW. On the character screen, open AddOns and enable all "GR Spike" addons.
4. Log in with a character.

### Screenshot test

5. Type `/grshot status`. Note the format and the screen size.
6. Type `/grshot png`.
7. Type `/grshot`. This takes a screenshot from a timer, with no key press.
   A strip of colored squares shows in the top-left corner for a moment.
8. Read the chat line. `ok (timer)` means that the test passed.
   If you see `ADDON_ACTION_BLOCKED`, Blizzard protects `Screenshot()`.
9. Look at the screen. The "Screen captured" text must not show.
10. Run `spikes/spike.py shot`. It reads the newest screenshot and looks for the strip.
11. Type `/grshot key` for a control test with a key press.
12. Optional: type `/grshot jpeg`, then `/grshot`, then `spikes/spike.py shot`. This tells if JPEG also works.

### File test

13. Type `/grfiles before`.
14. Run `spikes/spike.py mutate` in a terminal. Keep the game running.
15. Type `/grfiles after`.
16. Type `/reload`.
17. Type `/grfiles reload`.

Each check prints PASS or FAIL in the chat.

### After the tests

18. Type `/reload` one more time. WoW then writes the results to disk.
19. Run `spikes/spike.py results` and give the output to the agent.
20. Run `spikes/spike.py remove` when the tests are finished.

### Size test

This test is separate. Do it in its own game session.

1. Close WoW.
2. Run `spikes/spike.py sizes`. It makes slot bodies of 100 KB, 1 MB, and 5 MB.
   Every text byte in them is an escape, the slowest case for the Lua parser.
3. Start WoW, enable the "GR Spike" addons, and log in.
4. Type `/grsize`. Each line gives the load time in milliseconds.
5. Type `/reload`, then `/grsize` again. Do this 3 times, so that we see the spread.
6. Run `spikes/spike.py results` and give the output to the agent.

A load of more than about 16 ms drops a frame. The results set the size limit
of a slot body (SPEC.md 7.3, S12). The limit is now 1 MB.

## What the checks mean

| Check | If it fails |
|---|---|
| Screenshot from a timer | Use screen capture (SPEC.md section 11). |
| Strip survives in the image | Use PNG, or larger cells. |
| Rule 1: file made after launch is not found | Nothing breaks. The setup can then add files later. |
| Rule 2: slot reads the file at load | The slot channel does not work under Wine. This is serious. |
| Rule 3: second load does not run again | The slot pool design changes. |
| Rule 4: empty wav fails, valid wav plays | Signals do not work. Use slot polls only. |
| Signal: flip_on plays after it became valid | Signals do not work. Use slot polls only. |
| Rule 5: flip_off still plays | Good news if it fails: signals are reusable. |

## Results

Run on 2026-09-23. Arch Linux, Wine 11.17 (Staging), client 1.60.1.69977, window 1280×720.

| Check | Result |
|---|---|
| `Screenshot()` from a timer | Works. No `ADDON_ACTION_BLOCKED`. |
| Time of the `Screenshot()` call | 0.01 to 0.8 ms |
| Time from call to `SCREENSHOT_SUCCEEDED` | 349 to 455 ms |
| Strip correct in PNG | 400 of 400 cells. Every channel is exactly 0 or 255. |
| Cell size in the image | 3.875 px wide, 4 px high. The decoder must fit a fractional size. |
| Strip correct in JPEG | Not tested |
| "Screen captured" text hidden | Yes. The text did not show. |
| Rule 1: file made after launch is not found | PASS, for sounds and for addons |
| Rule 2: slot reads the file from disk at load | PASS |
| Rule 3: second load does not run the slot again | PASS |
| Rule 3: `/reload` frees the slots | PASS. The scripted check failed, because it ran before the reload took effect. A manual check after `/reload` gave `false false nil nil`. |
| Rule 4: empty wav does not play | **FAIL**. `PlaySoundFile` reports "will play" for an empty file. Only "exists at launch" makes a difference. |
| Rule 5: played wav stays valid | PASS, but rule 4 makes it useless |

Decisions:

1. The addon sends with `Screenshot()`. Screen capture is only a fallback.
2. `.wav` signals do not work on this client. The addon polls slots on a schedule.
