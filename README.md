# Gnomish Relay

Talk to your coding agents from inside World of Warcraft: Forever.
You type in a chat window in the game. An agent such as Claude, Codex, or Gemini
works in your project folder on the same computer, and its reply comes back as a whisper.

It works with Claude Code, Codex, and any agent that speaks the Agent Client Protocol
(ACP), on Windows, macOS, and Linux. It needs no Node.
`SPEC.md` has the design, and `VERIFICATION.md` the proofs.

## Install

1. Close WoW: Forever. WoW finds a new addon only when it starts, so setup must run first.
2. Run the installer:
   - Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh`
   - Windows (PowerShell): `irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex`
3. Answer the questions of setup. Setup asks only the questions that apply to your computer:
   - **WoW folder**: only when setup finds no game, or more than one. Type the number of the list, or a path.
   - **Also set up Gnomish Relay?**: only when the Timeways addon is installed and Gnomish Relay is not. The default is no.
   - **Folders the agents can work in**: a list divided by commas. Setup suggests the code folders that it finds.
     It never suggests the home folder, because that folder holds `~/.ssh` and the browser profiles.
   - **Found aider. Add it as an agent?** (also for `llm`): only for a tool with no ACP mode. The default is no.
     Such a tool runs its own commands with no question, inside the sandbox.
4. Start WoW and type `/relay`.

Setup uses the agents that you already have: `claude`, `codex`, `gemini`, `qwen`,
`opencode`, `goose`, and the other ACP agents in `SPEC.md` 9.2.
With none, replies repeat your message until you add one (below).

The installer downloads the program, checks its SHA-256 sum, and runs `gnomish-relay setup --autostart`.
Setup finds the game, makes its `Interface/AddOns` folder if WoW has not made it yet,
installs the addon with a key that only this computer has, writes `config.toml`,
and starts the bridge at each login. A second run changes nothing that works.

## Timeways

Timeways is a story addon that uses this program as its desktop half.
When the `Timeways` addon is installed, setup finds it: it writes the Timeways key,
makes its slot addons, and adds a `[story]` section with the model that it finds
(`claude`, Ollama, or LM Studio) to `config.toml`. With only Timeways, setup asks no
folder question and sets up no coding agent. `gnomish-relay setup --relay` adds them later.

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

Then run `gnomish-relay check-agent gemini`, and `gnomish-relay restart` to load the change.

`permission` is the most that a chat from the game can do. At `auto-edit`, the agent
edits files in the chat folder with no question, and asks in the game before each
command. At `ask`, it also asks before each edit.

## What an agent can do from the game

Every tool call of a message from the game gets one answer: it runs, it asks in the
game, it asks on your desktop, or it never runs (`SPEC.md` 6.6.3). A read of `~/.ssh`
or a write outside the chat folder asks on the desktop: a dialog with Approve and Deny
opens. With no dialog tool, answer it in a terminal:

```sh
gnomish-relay approve          # list the calls that wait
gnomish-relay approve <id>     # allow one
gnomish-relay deny <id>        # refuse one
```

Commands in the allow table run with no question at `auto-edit` and `full-auto`:

```toml
[allow]
commands = ["cargo test *"]
```

### The protection on each OS

The sandbox of commands is different on each OS (`SPEC.md` 6.6.4):

| OS | Commands of Claude | Commands of Codex |
|---|---|---|
| Linux | Each command runs in a `bwrap` sandbox. Install `bubblewrap`. | The sandbox of Codex |
| macOS | Each command runs in a Seatbelt sandbox (`sandbox-exec`). | The sandbox of Codex |
| Windows | No sandbox. Every command asks in the game, also a command of the allow table. | The Windows sandbox of Codex |

- The sandbox of the bridge lets a command write only the chat folder and a temp folder.
  It hides `~/.ssh`, the keys of the bridge, and the other credential folders.
  It lets a command reach only the allowed package hosts.
- On a Linux with no working `bwrap`, the bridge acts as on Windows: every command asks.
- The sandbox of Codex lets a command read the whole disk, also `~/.ssh`. It gives a command no network.
- Other ACP agents run their commands themselves, with no sandbox. Each of their tool calls asks at most.
- On Windows, use Codex for commands that run with no question, or run Claude and the bridge under WSL2.
  Under WSL2, the bridge uses `bwrap`, as on Linux.

## Update

`gnomish-relay update` installs the latest release and restarts the bridge.

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
  The addon starts from its Lua addon and its transport design.
  Its license notice is in `addon/GnomishRelay/LICENSE-wow-claude.txt`.
- [0xInuarashi/wow-forever-codex](https://github.com/0xInuarashi/wow-forever-codex), by 0xInuarashi.
  It measured the file-load rules of the Forever client and invented the pixel-out channel.
  Gnomish Relay uses its findings, not its code.

Code in the game window uses the font JetBrains Mono, under the SIL Open Font License 1.1.
Its license is in `addon/GnomishRelay/JetBrainsMono-OFL.txt`.

Gnomish Relay is under the MIT license. See `LICENSE`.
