# Gnomish Relay

Chat with your coding agents from inside World of Warcraft: Forever.
Type in a window in the game. An agent such as Claude, Codex, or Gemini works in your
project folder on the same computer, and its reply comes back as a whisper.

Gnomish Relay has two parts: an addon in the game, and a small desktop app,
`gnomish-relay`, that runs the agents. It works with Claude Code, Codex, and any agent
that speaks the Agent Client Protocol (ACP), on Windows, macOS, and Linux. No Node needed.
`SPEC.md` has the design, and `VERIFICATION.md` the proofs.

## Install

1. Close WoW: Forever. The game only finds new addons when it starts.
2. Run the installer:
   - Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh`
   - Windows (PowerShell): `irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex`
3. Answer setup's questions. You only see the ones that apply to you:
   - **WoW folder**: when setup finds no game, or more than one. Type a number from the list, or a path.
   - **Also set up Gnomish Relay?**: when you already have the Timeways addon but not Gnomish Relay. The default is no.
   - **Folders the agents can work in**: separate them with commas. Setup suggests the code folders it finds.
     It never suggests your home folder, because that holds `~/.ssh` and your browser profiles.
   - **Found aider. Add it as an agent?** (also for `llm`): only for a tool with no ACP mode. The default is no.
     Such a tool runs its own commands without asking, inside the sandbox.
4. Start WoW and type `/relay`.

Setup uses the agents you already have: `claude`, `codex`, `gemini`, `qwen`,
`opencode`, `goose`, and the other ACP agents in `SPEC.md` 9.2.
If you have none, replies just repeat your message until you add one (below).

The installer downloads the desktop app, checks its SHA-256 sum, and runs
`gnomish-relay setup --autostart`. Setup finds the game, installs the addon with a key
that only this computer has, writes `config.toml`, and starts the desktop app each time
you log in. Running it again is safe: it leaves alone whatever already works.

## Timeways

Timeways is a story addon that also uses the desktop app.
If you have the `Timeways` addon, setup finds it: it writes the Timeways key, makes its
addon files, and adds a `[story]` section to `config.toml` with the model it finds
(`claude`, Ollama, or LM Studio). With only Timeways, setup skips the folder question and
sets up no coding agent. To add coding agents later, run `gnomish-relay setup --relay`.

## Add an agent

Claude Code needs only the `claude` program, and Codex only the `codex` program:

```toml
[agents.claude]
kind = "claude"
command = ["claude"]
permission = "auto-edit"

[agents.codex]
kind = "codex"
command = ["codex"]
permission = "auto-edit"
```

Any ACP agent is one entry in `config.toml`:

```toml
[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "auto-edit"
```

Then run `gnomish-relay check-agent gemini` to test it, and `gnomish-relay restart` to load it.

`permission` is the most a chat from the game can do. At `auto-edit`, the agent edits
files in the chat folder without asking, and asks in the game before each command.
At `ask`, it also asks before each edit.

## What an agent can do from the game

Every tool call from the game gets one of four answers: it runs, it asks in the game,
it asks on your desktop, or it never runs (`SPEC.md` 6.6.3). Reading `~/.ssh` or
writing outside the chat folder asks on your desktop: a dialog with Approve and Deny
opens. If your computer has no dialog tool, answer in a terminal:

```sh
gnomish-relay approve          # list the requests that wait for you
gnomish-relay approve <id>     # approve one
gnomish-relay deny <id>        # deny one
```

At `auto-edit` and `full-auto`, commands in the allow table run without asking:

```toml
[allow]
commands = ["cargo test *"]
```

### The protection on each OS

The sandbox for commands depends on your OS (`SPEC.md` 6.6.4):

| OS | Commands of Claude | Commands of Codex |
|---|---|---|
| Linux | Each command runs in a `bwrap` sandbox. Install `bubblewrap`. | The sandbox of Codex |
| macOS | Each command runs in a Seatbelt sandbox (`sandbox-exec`). | The sandbox of Codex |
| Windows | No sandbox. Every command asks in the game, even one in the allow table. | The Windows sandbox of Codex |

- The desktop app's sandbox lets a command write only to the chat folder and a temp folder.
  It hides `~/.ssh`, the app's own keys, and the other credential folders.
  It lets a command reach only the allowed package hosts.
- On Linux without a working `bwrap`, it acts as on Windows: every command asks.
- The sandbox of Codex lets a command read the whole disk, `~/.ssh` too. It gives a command no network.
- Other ACP agents run their commands themselves, with no sandbox. Each of their tool calls asks at most.
- On Windows, use Codex for commands that run without asking, or run Claude and the desktop app under WSL2.
  Under WSL2, the desktop app uses `bwrap`, as on Linux.

## Notifications

Get a notification in the game when Claude Code or Codex in a normal terminal needs
you or finishes a long task. Run this once:

```
gnomish-relay hooks install
```

It adds a hook to `~/.claude/settings.json` and `~/.codex/hooks.json`, for each of
`claude` and `codex` on your `PATH`. It keeps your own hooks and backs up each file first.
Then restart any sessions that are open. The next time Codex starts, it asks you to
trust the new hooks: trust them to get notifications.

While a notification waits, a bell shows at the edge of the minimap, with a line in
chat and a sound. A notification never runs anything: you answer in the terminal.
To turn the chat lines, sounds, or banners on or off, open Settings in the game.
`gnomish-relay hooks status` shows whether notifications are on, and
`gnomish-relay hooks remove` turns them off.

## Update

`gnomish-relay update` installs the latest version and restarts the desktop app.
Then type `/reload` in WoW.

## Troubleshooting

Start with `gnomish-relay status`. It checks each part and says what to do next.

| You see | Do this |
|---|---|
| "Desktop app offline" in the window, or "the desktop app isn't running" | Run `gnomish-relay restart`. |
| "Your game and the desktop app don't match" | Run `gnomish-relay setup`, then type `/reload` in WoW. |
| "Some addon files are missing" | Close the game, then run `gnomish-relay install`. |
| "Can't take screenshots" | Free up disk space and check the `Screenshots` folder of WoW, then type `/reload`. |
| "Another addon is in the way of the colored bar" | Turn off other addons that take screenshots, then type `/reload`. |
| A colored bar flashes at the top left | That's normal: it's how your messages reach the desktop app. |
| "1 message is waiting. Reload to send it." | Click **Reload**. |
| "Claude needs you to log in again" | On your desktop, run `claude` and log in. |
| A reply says "That agent isn't in config.toml" | Pick another agent in Settings, or add it to `config.toml` and run `gnomish-relay restart`. |

The desktop app writes its log to `bridge.log` in its data folder
(on macOS, `~/Library/Logs/gnomish-relay.log`; with the Linux service, `journalctl --user -u gnomish-relay`).

## From source

`cargo run -q --bin gnomish-relay -- setup`. For addon work, `scripts/dev-link.sh`
links `addon/GnomishRelay` into the game first. `CLAUDE.md` has the rules of the code.

The tools of the checks:

| Tool | For | Script |
|---|---|---|
| Rust stable, with `rustfmt` and `clippy` | the build, the lints, and the tests | `check-fast.sh`, `check-all.sh` |
| `stylua` and `selene` | the format and the lints of the addon | `check-fast.sh`, `check-all.sh` |
| `python3` and `git` | the WoW API gate | `wow-api.sh`, `selftest-api.sh`, `check-all.sh` |
| `cargo-deny` | the licenses and advisories of the dependencies | `check-all.sh` |
| Charon and Aeneas, at the commits in `proofs/TOOLS`, in `~/verif` or in `CHARON_DIR` and `AENEAS_DIR` | the translation of `protocol` to Lean | `extract.sh`, `check-proofs.sh` |
| Lean through `elan`, at the version in `proofs/lean-toolchain` | the proofs | `check-proofs.sh` |
| Quint (`npm install -g @informalsystems/quint`) | the models of the transport | `check-model.sh` |
| Rust nightly and `cargo-fuzz` | the fuzz targets | `fuzz.sh` |
| `cargo-llvm-cov` | the coverage gates | `check-coverage.sh` |

For a quick loop, run `scripts/check-fast.sh`. Before each commit, run `scripts/check-all.sh`.

### After a game patch

The tests run the addon in a fake game. A self-test addon measures the real game, so
the fake game acts as the real one (SPEC.md 14.3). Run it after each client patch:

1. Close the game. Run `scripts/selftest-link.sh`.
2. Start the game and log in. Stay out of combat. When the chat says "done", type `/reload`.
3. Run `cargo run -q --bin gnomish-relay -- selftest collect` in this folder. Run
   `cargo test`, and commit `tests/fixtures` and `tests/vectors`.

The first time, collect asks for one more `/reload`: the first session has no saved
file, so it cannot see the load order. `scripts/selftest-link.sh --remove` takes the
self-test out of the game.

## Credits

Gnomish Relay uses the design of two earlier projects (`SPEC.md` 4):

- [chelinho139/wow-claude](https://github.com/chelinho139/wow-ai), now named `wow-ai`, by chelinho139, under the MIT license.
  It inspired the design of the addon and the transport.
  Gnomish Relay has no code from it.
- [0xInuarashi/wow-forever-codex](https://github.com/0xInuarashi/wow-forever-codex), by 0xInuarashi.
  It measured the file-load rules of the Forever client and invented the pixel-out channel.
  Gnomish Relay uses its findings, not its code.

Code in the game window uses the font JetBrains Mono, under the SIL Open Font License 1.1.
Its license is in `addon/GnomishRelay/JetBrainsMono-OFL.txt`.

Gnomish Relay is under the MIT license. See `LICENSE`.
