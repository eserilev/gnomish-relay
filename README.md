# Gnomish Relay

Talk to your coding agents from inside World of Warcraft: Forever.
You type in a chat window in the game. An agent such as Claude, Codex, or Gemini
works in your project folder on the same computer, and its reply comes back as a whisper.

It works with any agent that speaks the Agent Client Protocol (ACP), on Windows,
macOS, and Linux. `SPEC.md` has the design, and `VERIFICATION.md` the proofs.

## Install

1. Start WoW: Forever once, so that it makes its `Interface/AddOns` folder. Then close it.
2. Run the installer:
   - Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh`
   - Windows (PowerShell): `irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex`
3. Answer one question: the folders that the agents can work in.

Setup uses the agents that you already have: `claude-agent-acp`, `codex-acp`, or `gemini`.
With none, replies repeat your message until you add one (below).
4. Start WoW and type `/relay`.

The installer downloads the program, checks its SHA-256 sum, and runs `gnomish-relay setup --autostart`.
Setup finds the game, installs the addon with a key that only this computer has, writes
`config.toml`, and starts the bridge at each login. A second run changes nothing that works.

## Add an agent

Any ACP agent is one entry in `config.toml`:

```toml
[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"
```

Then run `gnomish-relay check-agent gemini`, and `gnomish-relay restart` to load the change.

## Update

`gnomish-relay update` installs the latest release and restarts the bridge.

## From source

`cargo run -q --bin gnomish-relay -- setup`. For addon work, `scripts/dev-link.sh`
links `addon/GnomishRelay` into the game first. `CLAUDE.md` has the rules of the code.

## Credits

Code in the game window uses the font JetBrains Mono, under the SIL Open Font License 1.1.
Its license is in `addon/GnomishRelay/JetBrainsMono-OFL.txt`.
