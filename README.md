# Gnomish Relay

Talk to your coding agents from inside World of Warcraft: Forever.
You type in a chat window in the game. An agent such as Claude, Codex, or Gemini
works in your project folder on the same computer, and its reply comes back as a whisper.

It works with Claude Code, Codex, and any agent that speaks the Agent Client Protocol
(ACP), on Windows, macOS, and Linux. It needs no Node.
`SPEC.md` has the design, and `VERIFICATION.md` the proofs.

## Install

1. Start WoW: Forever once, so that it makes its `Interface/AddOns` folder. Then close it.
2. Run the installer:
   - Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh`
   - Windows (PowerShell): `irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex`
3. Answer one question: the folders that the agents can work in.

Setup uses the agents that you already have: `claude`, `codex`, `gemini`, `qwen`,
`opencode`, `goose`, and the other ACP agents in `SPEC.md` 9.2.
With none, replies repeat your message until you add one (below).
4. Start WoW and type `/relay`.

The installer downloads the program, checks its SHA-256 sum, and runs `gnomish-relay setup --autostart`.
Setup finds the game, installs the addon with a key that only this computer has, writes
`config.toml`, and starts the bridge at each login. A second run changes nothing that works.

## Add an agent

Claude Code needs only the `claude` program, and Codex only the `codex` program:

```toml
[agents.claude]
kind = "claude"
command = ["claude"]
permission = "ask"

[agents.codex]
kind = "codex"
command = ["codex"]
permission = "ask"
```

Any ACP agent is one entry in `config.toml`:

```toml
[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"
```

Then run `gnomish-relay check-agent gemini`, and `gnomish-relay restart` to load the change.

## What an agent can do from the game

Every tool call of a message from the game gets one answer: it runs, it asks in the
game, it asks on your desktop, or it never runs (`SPEC.md` 6.6.3). A read of `~/.ssh`
or a write outside the chat folder asks on the desktop. Answer it in a terminal:

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

## Update

`gnomish-relay update` installs the latest release and restarts the bridge.

## From source

`cargo run -q --bin gnomish-relay -- setup`. For addon work, `scripts/dev-link.sh`
links `addon/GnomishRelay` into the game first. `CLAUDE.md` has the rules of the code.

## Credits

Code in the game window uses the font JetBrains Mono, under the SIL Open Font License 1.1.
Its license is in `addon/GnomishRelay/JetBrainsMono-OFL.txt`.
