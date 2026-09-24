# Gnomish Relay: Specification

Status: draft 3, 2026-09-23. Nothing is built yet.
Draft 2 applies a review against the `wow-claude` source code.
Draft 3 applies the spike results in `spikes/README.md`: the strip goes out through `Screenshot()`, and `.wav` signals do not work.

## 1. Summary

Gnomish Relay connects AI coding agents to World of Warcraft: Forever.
You send a task from a chat window in the game. The agent does the work on your computer.
The reply comes back into the game with a whisper sound.

Gnomish Relay also shows pings from agent sessions that you run in a normal terminal.
When a terminal session ends a turn or needs input, a message appears in the game.

Gnomish Relay has two parts:

- **The addon**: a WoW addon, written in Lua. It shows the chat window.
- **The bridge**: a program on your computer, written in Rust. It moves messages between the game and the agents.

## 2. Goals

1. Drive an AI coding agent from inside WoW, with no alt-tab and no `/reload` per message.
2. Get a ping in the game when a terminal agent session ends a turn or needs input.
3. Work with any coding agent: Claude Code, Codex, Gemini CLI, and others.
4. Work on Linux (WoW in Wine), Windows, and macOS.
5. Use only documented addon APIs. Never inject code, read game memory, or send keystrokes to the game.
6. Prove the protocol core correct with Aeneas and Lean. Test everything else.

## 3. Non-goals

- Automation of gameplay. The addon never acts in the game for the player.
- Compatibility with `wow-claude` on the wire. Gnomish Relay has its own magic bytes and version byte.
- Other WoW versions. The first target is WoW: Forever only.
- A hosted service. Everything runs on the local machine.

## 4. Prior work and credits

Gnomish Relay uses the design of two earlier projects:

- [chelinho139/wow-claude](https://github.com/chelinho139/wow-claude) (MIT).
  It is a Windows-only Node bridge for Claude Code. Gnomish Relay starts from its Lua addon and its transport design.
- [0xInuarashi/wow-forever-codex](https://github.com/0xinuarashi/wow-forever-codex).
  It measured the file-load rules of the Forever client and invented the pixel-out channel.

The README credits both projects. The addon code from `wow-claude` keeps its MIT license notice.
References in this spec to `wow-claude` files use the path in that repo, for example `bridge/protocol.js`.

## 5. Terms

| Term | Meaning |
|---|---|
| Strip | The block of colored cells that the addon draws to send data out. |
| Record | One message inside a strip, or one reply inside a slot file. |
| Slot | One of the 200 load-on-demand reply addons. |
| Publish | One write of the current reply records into all slots. |
| Signal | A `.wav` file that is empty (off) or valid (on). |
| Run | One agent process that works on one message. |
| Token | The random ID of one copy of the addon saved data. |

## 6. Threat model

The bridge runs agents that edit code and run commands.
Input reaches the bridge from four places: screenshots, the hook socket, the config file, and agent output.
The bridge treats all four as untrusted input.

### 6.1 Attackers

| Attacker | How | Defense |
|---|---|---|
| Another window over the game (browser, video, overlay) | Shows a fake strip | WoW takes the screenshot itself, so other windows are not in it. With the capture fallback, capture reads the window content. Each strip carries a MAC. |
| A local program | Drops a crafted PNG into the Screenshots folder, or replaces a slot folder with a symbolic link | MAC (6.3) and freshness check (S11). Image size limit before decoding. No writes or deletes through symbolic links (6.2). |
| A local program | Connects to the hook socket and sends fake pings | Socket mode 0600. Size limit and rate limit. Ping text goes through the same escapes as agent text. |
| A malicious or prompt-injected agent | Writes a reply that injects Lua or fakes WoW chat links. Asks for permission with a false label. Writes a huge reply. | Lua escape and UI escape (S8 to S10). Honest permission popup (6.4). Size limits (S12). |
| An old screenshot | A strip is replayed from an old file, for example after `state.json` is lost | Freshness check (S11). |
| Another addon or a WeakAura | Runs Lua in the same environment as our addon. It can call our handlers, fill our input box, click our buttons, read and change `GnomishRelayDB`, and replace a slot body during a load. | Signed state (6.6.1) stops changes to stored messages. The taint warning is designed to catch naive calls. The ceiling, the classifier, and the sandbox (6.6.2 to 6.6.4) bound every game message, whoever sent it. |
| A prompt injection in a file | The agent reads a README, an issue, or a web page with hidden instructions | The action classifier (6.6.3) and the sandbox (6.6.4). Layer 1 does not help: the prompt came from the user. |
| A stream or recording | The strip shows the prompt on screen | None. Do not stream while you use the relay. The README says this. |

A hostile addon in the same Lua environment can call every entry point of our addon.
No WoW mechanism proves that the user typed a message (6.6.1).
So the bridge bounds what any message from the game can do (6.6).

### 6.2 Bridge policy

1. The folder of a chat must be inside `allowed_roots` from the config. The bridge rejects all other folders.
2. The permission level of each agent comes only from the bridge config. A message from the game cannot raise it.
3. A permanent "always allow" rule from the game follows 6.6.5.
4. The bridge limits the message rate: at most 10 messages per minute (config key `max_messages_per_minute`).
5. The bridge never runs the agent with `full-auto` unless the config sets it for that agent. The classifier and the sandbox still apply (6.6.2).
6. The bridge rejects frames with a timestamp more than 5 minutes old or more than 1 minute in the future (S11).
7. The bridge never writes, renames, or deletes through a symbolic link. It opens files with `O_NOFOLLOW` (Unix) or checks the reparse point (Windows).
8. The bridge deletes only the screenshots that it decoded as valid strips. It never deletes other screenshots.
9. The bridge limits sizes: an image before decoding (4096 × 4096 px), a hook message (64 KB), a reply record (32 KB), a slot body (S12), and each chat queue (20 messages).
10. The bridge resolves symbolic links in a chat folder with `canonicalize`, then checks `allowed_roots` again on the result.
11. The bridge never starts a process through a shell. It passes the command as an argument list.
12. The bridge gives each agent process only an allowlist of environment variables (`PATH`, `HOME`, `LANG`, `TERM`, and the variables in the agent config). All others, for example API keys of other tools, stay out.
13. The bridge writes prompt files with mode 0600 in a private folder, and deletes them after the run.
14. Setup writes `config.toml` with mode 0600. The bridge refuses a config that other users can write, because the config sets the ceiling of every game message. The strip key is in its own file, `strip.key`, with mode 0600.
15. The bridge escapes control characters and newlines in `bridge.log`, so a prompt cannot fake a log line.
16. The bridge sends a restore bundle only in answer to a hello with a valid MAC.

### 6.3 Strip authentication

The setup step makes a random 32-byte key.
It writes the key into the addon (as a file-local value) and into the bridge config.
Each strip ends with a truncated HMAC-SHA256 tag (8 bytes) of the header and payload.
The bridge drops each strip with a wrong tag, and logs it.
The bridge compares tags in constant time (`subtle::ConstantTimeEq`).

The cost of HMAC-SHA256 in Lua: about 0.1 ms for a full 3221-byte strip under LuaJIT with the JIT off. The plain Lua 5.1 of WoW is a few times slower, still well under 1 ms.

### 6.4 Honest permission popup

A malicious agent can ask for permission with a false label, for example "run tests" for `rm -rf ~`.
So the popup never shows the label of the agent as the main text. Rules:

- The popup text comes from the raw tool input: the real command line, or the real file path.
- If the text is too long, the popup shows the start and the end, with a visible "cut" mark in the middle.
- Control characters, Unicode bidi characters, and zero-width characters show as visible escapes, for example `<U+202E>`.
- The label of the agent shows below the raw command, marked as "the agent says".

Theorem S15 covers these rules.

### 6.5 Known leaks

- Reply text sits in a global table after a slot loads. Any addon can read it.
- `GnomishRelayDB` is a global table. Any addon can read the chats in it.
- The strip is signed, not encrypted. The prompt is in the pixels of each strip screenshot until the bridge deletes it. If the bridge does not run, these files stay. A cloud sync of the Screenshots folder (for example OneDrive on Windows) copies them.
- Code in the sandbox can still send data to the allowed API host, for example with an upload under another account key. A proxy that ends TLS and pins the account closes this. It is not in v1.

### 6.6 Four layers of defense

Each layer covers a hole in the layer before it. No layer depends on a model that judges another model.

| Layer | Question | Where |
|---|---|---|
| 1. Signed state and the taint warning | Did our own code make this message, and did anything change it? | Addon |
| 2. Game ceiling | What can a message from the game do at most? | Bridge config |
| 3. Action classifier | Does this tool call run, ask in the game, ask on the desktop, or never run? | Bridge, proved in `protocol` |
| 4. Sandbox | What can happen when layers 1 to 3 fail? | Operating system |

The trust of "always allow" (6.6.5) rests on layers 2 to 4. It never rests on layer 1.

#### 6.6.1 Signed state and the taint warning

**Signed state.** Another addon can change `GnomishRelayDB` without a call to our code. So the addon signs messages from private state:

- The addon keeps the text of each open message in its private table (`ns`), not only in `GnomishRelayDB`.
- When the user sends a message, the addon signs it at once. It stores the signed frame and its time in `GnomishRelayDB`, next to the text.
- After a `/reload`, the addon sends only frames with a valid tag. It never signs text that it reads back from `GnomishRelayDB`.
- A stored frame or an outbox frame older than 270 seconds is too old for the bridge (S11 allows 300). The message then ends with "Not sent. Send it again.", and the user decides.
- An outbox entry (7.5) is the same signed frame. The bridge checks the tag, the time, and the replay store for it (S2, S11, S7), as for a strip.
- A permission answer carries a hash of the exact text that the popup showed: `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256, in hex. The bridge refuses an answer whose hash does not match its own text of the request.

**Taint warning.** WoW tracks which addon tainted each variable, and `issecurevariable(table, key)` returns its name.
At each entry point, the addon writes a probe value and reads its taint. If the taint names another addon, the addon refuses the action and shows one line: "Blocked: <addon> tried to send as you."
This catches a naive attack only. It is not a trust decision:

- Taint probably moves to the last data that the code read. Our handler reads our own tables before the probe, so the probe can name "GnomishRelay" for any caller. The spike in 15 tests this.
- Some attacks need no call to our code. A secure macro button can run `/ai …` on the user's own click. Another addon can fill the chat box with `/ai …` and wait for the user to press Enter.
- A hostile addon that loads first can replace `issecurevariable`.

No WoW mechanism lets an addon prove that the user typed a message. So layers 2 to 4 assume that any game message can come from another addon.

#### 6.6.2 Game ceiling

Every message from the game (a strip or the reload outbox) runs under one ceiling from `config.toml`. No message from the game can raise it (S6).

| Setting | Default |
|---|---|
| Write | The chat folder only (the `auto-edit` level of 9.3) |
| Read | `allowed_roots` |
| Commands | The allow table of the config. All others ask. |
| Network | The agent's own API host only (6.6.4). Allowing a network tool does not widen the proxy. |

- The `full-auto` level (6.2 rule 5) skips the questions only. The classifier denials and the sandbox still apply.
- The game never answers a permission request of a terminal session, and never sends a task to one. Terminal sessions only send pings (section 10). The bridge does not run them, so this spec gives them no rules.
- The bridge shows a desktop notice for each game message: "New task from WoW: <first line>". The config can turn this off.

#### 6.6.3 Action classifier

The bridge classifies each tool call before it runs. It works on the structured tool input, never on the prompt text. The same input always gives the same answer.

There are four answers, in this order from strict to open:

| Answer | Meaning |
|---|---|
| `deny` | Never runs. Only for the files that guard the relay itself. |
| `desktop` | The user approves on the desktop. The game popup says "Approve on your desktop". No addon can click a desktop prompt. |
| `ask` | The user approves in the game popup (6.4). |
| `allow` | Runs with no question. |

**How tool calls reach it:**

- ACP: through `session/request_permission`.
- Claude: through a `PreToolUse` hook, `gnomish-relay-hook pretool`. This subcommand ignores `GNOMISH_RELAY_JOB`, and it fails closed: if the bridge does not answer, the answer is `deny`.
- `native-codex exec` sends no tool calls to the bridge. So a game message runs Codex through ACP (`codex-acp`) only.

**Rules:**

- **Unknown tools are `desktop`.** The classifier knows file reads, file writes, and shell commands. Every other tool is `desktop` in `game`: web fetch, web search, MCP tools, and subagents.
- **Paths:** each path in the tool input, and each redirect target of a command, is resolved with `canonicalize` at check time. A write outside the chat folder is `desktop`. A read outside `allowed_roots` is `desktop`. Both checks use `resolve_folder` (S5).
- **`deny` paths:** the strip key, `config.toml`, and everything else in `~/.config/gnomish-relay`. An approved access would let the agent sign fake strips or raise its own ceiling.
- **`desktop` paths, for reads and writes:** `~/.ssh`, `~/.aws`, `~/.gnupg`, `.env` files, keychains, and browser profiles.
- **`desktop` paths, for writes:** files that code on the host runs later, outside the sandbox. They are `.claude/`, `.git/hooks/`, `.git/config`, `.envrc`, `.vscode/`, and `.github/workflows/`.
- **Commands:** a real shell parser splits each command. A command that does not parse is `desktop`. The popup shows its raw text (6.4).
- **`desktop` commands:** `eval`, command substitution (`$(...)`, backticks), a pipe into a shell, `sudo`, `cmd.exe`, and PowerShell. PowerShell stays `desktop` until the classifier has a PowerShell parser.
- **Commands that run other commands** (`find -exec`, `xargs`, `env`, `git -c`, `sh -c`, `bash -c`, `python -c`, `node -e`, `perl -e`) always ask.
- **Network tools** (`curl`, `wget`, `nc`, `ssh`, `scp`, and more) always ask.
- **Never "always":** `rm -r`, `chmod`, `chown`, `git push --force`, `git reset --hard`, the commands that run other commands, and every `desktop` answer. They get "Allow once" at most.
- A prompt keyword (for example `.ssh` or `token`) is only a signal. It moves the whole run to `ask`. It is never the wall.

The classifier core is pure and lives in `protocol`. Theorems S16 and S17 cover it.

#### 6.6.4 Sandbox

The bridge starts every agent process for a game message inside a sandbox. The user does nothing.

| Rule | Value |
|---|---|
| Write | The chat folder, and a private temp folder |
| Read | The system, except the `desktop` and `deny` paths of 6.6.3, which are hidden |
| Network | Only through a bridge proxy that allows the agent's own API host |
| Children | Every child process, for example `cargo test`, is inside the same sandbox |

The sandbox closes the hole that a classifier cannot close: an allowed command such as `cargo test` runs code that the agent can edit first.
It covers shell commands. The file tools of Claude run outside it, so the classifier (6.6.3) guards them.

What each backend enforces:

| Backend | Linux | macOS | Windows |
|---|---|---|---|
| Claude | Its own sandbox (bubblewrap) | Its own sandbox (Seatbelt) | None: fallback. Under WSL2, as Linux. |
| Codex through ACP | Its own sandbox, `workspace-write` | Its own sandbox, `workspace-write` | Its own Windows sandbox |
| Other agents | `@anthropic-ai/sandbox-runtime` around the command | `@anthropic-ai/sandbox-runtime` around the command | None: fallback |

**Claude settings.** The bridge starts Claude with a `--settings` value that sets:

- `sandbox.enabled` to true
- `allowUnsandboxedCommands` to false, so Claude cannot retry a command outside the sandbox
- `failIfUnavailable` to true, so a sandbox that does not start stops the run
- the credentials and network settings of the table above, with a strict allowlist

Settings from the project and the user do not apply to these runs, because array keys such as `excludedCommands` merge from every scope. The flag for this is an open question (17).

**Codex.** Its `workspace-write` blocks the network for commands. Its model traffic does not go through the bridge proxy. Its read scope is an open question (17).

**Processes.** For game messages, the bridge starts one ACP process per chat folder, so the write rule applies per chat (9.4).

**Fallback, when a backend has no sandbox:**

- The game ceiling drops to `ask` for every command.
- "Always allow" for a command that runs code (build, test, run, install) needs a second step in the game. The popup then says: "No sandbox on this computer. This rule lets the agent run any code that it writes, with your full access. Allow always anyway?"
- File edits inside the chat folder still work.
- The chat header shows "No sandbox".

On Windows, the setup recommends Codex, or Claude under WSL2. Both have a sandbox there. A Windows sandbox for other agents comes later (17).

#### 6.6.5 "Always allow"

The goal is one click for the common case, with a bounded worst case.
Any game message can come from another addon (6.6.1). So a rule is safe to add with one click only when the sandbox bounds what the rule allows.

1. The popup (6.4) shows the exact rule, for example "Always allow `cargo test *` in lighthouse".
2. A rule covers one command pattern in one project. It never covers a whole tool, for example "all Bash".
3. If the backend of the chat has a sandbox (6.6.4), one click in the game adds the rule. The game and the desktop both show "Rule added: cargo test * (lighthouse)", each with **Undo**. Neither blocks.
4. With the fallback of 6.6.4, a rule for a command that runs code needs the second warning step of 6.6.4.
5. The "never always" commands of 6.6.3 get "Allow once" at most.
6. A rule expires after 30 days. The Settings tab of the window lists every rule and its expiry, and removes one with a click.

## 7. Transport

WoW addons run in a sandbox. An addon cannot open a network socket.
An addon cannot read a file while the game runs, with one exception (7.3).
Gnomish Relay uses three side channels: the strip in a screenshot (out), slots (in), and a reload fallback.
Signals (7.4) do not work on the tested client.

### 7.1 Strip: game to bridge

The addon draws a strip in the top-left corner of the screen.
Then the addon calls `Screenshot()` from a timer. WoW saves a PNG in `_classic_beta_/Screenshots`.
The bridge watches that folder, decodes the strip, and deletes the file.

The spike proved this path (2026-09-23): the call takes under 1 ms, the file arrives after about 0.4 s, and every color is exact.

- The addon sets the `screenshotFormat` CVar to `png` at login.
- The addon hides the "Screen captured" text for its own screenshots through the `ActionStatus` frame. Normal screenshots still show it.
- The bridge ignores screenshots with no valid strip. Those are the screenshots of the user.
- Screen capture of the window (section 11) is a fallback for a client that blocks `Screenshot()`.

**Frame layout (bytes):**

```
[0x6E 0x52] [version] [time: 4 bytes] [frame id hi, lo] [len hi, lo] [payload: len bytes] [fletcher16 s1, s2] [mac: 8 bytes]
```

- Magic bytes `0x6E 0x52` differ from `wow-claude` (`0xC7 0x1A`). A wrong magic means "not a strip".
- `version` is the protocol version, 1 for this spec.
- `time` is the Unix time from `time()` in the game, big-endian. The bridge uses it for the freshness check (S11).
- `frame id` is the message id modulo 65536. It only tells frames apart. The record ids are the real keys.
- Fletcher-16 covers version to payload. It catches capture errors.
- The MAC covers magic to checksum. It stops fake strips (6.3).
- `len` is at most 3200. The addon refuses longer text and tells the user.

**Cells:**

- Each cell carries 3 bits, most significant bit first.
- Bit 2 is red, bit 1 is green, bit 0 is blue. Each channel is fully on or fully off, so there are 8 colors.
- The decoder reads each channel at the cell center and compares it to 128.
- One row has 200 cells. A strip has at most 48 rows.
- The addon sizes a cell to 4 physical pixels. It uses `GetPhysicalScreenSize()`, `SetIgnoreParentScale`, and strata `TOOLTIP`.

**The decoder finds the grid itself.** UI scale makes the cell size fractional. The spike measured 3.875 px wide and 4 px high at 1280×720.
So each strip starts with two calibration rows of known colors: row 1 counts 0 to 7, and row 2 counts 7 to 0.
The decoder tries every cell size from 3 to 8 pixels, and keeps a size that matches both rows exactly. Row 2 runs backwards, so a grid one cell off fails.
The two rows fix the cell width but not the row height. So the decoder reads the data rows with each size that matches, and keeps the one whose bytes decode as a frame with a valid checksum.
The search starts at the top-left corner of the image. With 8-pixel cells, a strip is 1600×384 pixels.

**Records in the payload:**

- One frame holds one or more records, divided by `\x1E` (RS).
- The fields of a record are divided by `\x1F` (US):

```
token \x1F chat \x1F id \x1F cwd \x1F flags \x1F name \x1F text
```

| Field | Meaning |
|---|---|
| `token` | The random token of the addon saved data. |
| `chat` | The chat id. |
| `id` | The message id. The bridge drops a duplicate `(token, id)`. |
| `cwd` | The folder that the user asked for. Empty means the default. The bridge applies 6.2 rule 1. |
| `flags` | A `;`-divided list (7.1.1). |
| `name` | The chat name. The addon replaces RS and US with spaces. |
| `text` | The message. It is the last field, so it can contain `\x1F`. |

#### 7.1.1 Flags

| Flag | Meaning |
|---|---|
| `n` | Start a new agent session for this chat. |
| `h` | Hello only. It announces the token and the addon version. It has no prompt. The addon sends one at login and after it applies a restore bundle (7.6). |
| `d` | The chat is deleted. The bridge drops its transcript and session. The addon keeps the id in `db.forget` and sends it with each hello until the bridge acknowledges it. |
| `agent=<name>` | The agent for a new chat. The config must have an `[agents.<name>]` entry, or the message ends with "Agent not set up." |
| `level=<level>` | The mode of the chat: `ask`, `auto-edit`, or `full-auto`. The run gets the lower of this level and the level of the agent in the config (S6). An unknown word counts as `ask`. |
| `perm=<request>:<option>` | The answer to a permission request (9.3). |
| `read=<id>,<id>` | The final replies in the last body that the addon has shown. The bridge then takes them out of the slot body (7.3). A lost strip loses nothing: the next strip names them again. |
| `restored` | The addon has applied the restore bundle for its token (7.6). |
| `next=<n>` | The next slot that the addon loads (7.3). |

**Strip lifetime:**
The strip shows only while its screenshot is taken, about half a second.
If no acknowledgment comes in 40 seconds, the addon shows the strip again, up to 3 times in all.
Then the addon uses the reload fallback (7.5).

### 7.2 Why each channel works

The WoW client has these rules. `wow-forever-codex` measured them on Windows:

1. The client finds addon files only at launch. A file that does not exist at launch is never found.
2. The client reads a load-on-demand addon from disk when it loads, not at launch.
3. Each addon loads one time per UI session. `/reload` starts a new UI session.
4. `PlaySoundFile` fails for an empty `.wav` file and works for a valid one.
5. After a `.wav` file plays one time, the client treats it as valid until the client process stops. A `/reload` does not reset this.

The spike tested the rules under Wine (2026-09-23). Rules 1, 2, and 3 hold. Rule 4 fails: `PlaySoundFile` reports "will play" for an empty file. So rule 5 does not matter.

### 7.3 Slots: bridge to game

There are 1000 slots. The addon loads them in order, from the first slot that it has not loaded in this UI session.

The bridge writes each body only into a window of 30 slots, with an atomic rename per file.
The window starts at the next slot that the addon reported (`next` flag, 7.1.1).
Writing all 1000 slots at every publish costs too much disk: a 20 KB body every 3 seconds is 20 MB per publish.

- The addon reports `next` in every strip. When it nears the end of the window without a strip to send, it sends a hello with `next`.
- At a hello, or when the saved variables file changes (a `/reload`), the bridge starts the window at the reported slot, or at slot 1.
- A slot outside the window holds an older body. A read of an older body is harmless: every record stays in the body until a `read` flag names it, so a later poll gets it. The model (14.2) checks this.

Each slot is a folder `GnomishRelay_S0001` to `GnomishRelay_S1000` with three files:

- `GnomishRelay_SNNNN.toc`: `## Interface: 16001`, `## LoadOnDemand: 1`, `## Dependencies: GnomishRelay`, and the two Lua file names.
- `Inbox.lua`: the body.
- `Restore.lua`: the restore bundle (7.6). With no restore, its token is empty, and no addon takes it.

The body sets one global table:

```lua
GnomishRelay_SlotData = {
  proto = 1, slots = 1000, ack_max = 200, presence_max = 2000, note_max = 2000,
  ts = 1790211079, now = 1790211081,
  cwd = "/home/eitan/Documents/Code",
  replies = {
    { chat = "c1", id = 12, status = "working", text = "...", cwd = "...", session = "...",
      progress = { "edit src/main.rs", "$ cargo test" }, denied = { "Bash(rm:*)" } },
  },
  notes = { { seq = 41, source = "claude", repo = "lighthouse", kind = "done", text = "..." } },
  permissions = { { request = "p7", chat = "c1", tool = "Bash", detail = "cargo test",
                    options = { { id = "o1", kind = "allow_once", label = "Allow" } } } },
}
```

- `proto` and the pool sizes let the addon detect a mismatch (7.7).
- `replies` holds every record that the addon has not read, at most 30. Each `text` is at most 32 KB. The bridge cuts longer text and adds a note with the full length.
- A final reply stays in the body until a `read` flag names it. Then the bridge takes it out.
- When the body holds 30 records, the bridge refuses new messages. It does not mark a refused message as seen, so the addon sends it again: the strip shows again, and the outbox (7.5) keeps it. The bridge takes new messages again after the next `read` flag.
- The `transport.qnt` model (14.2) checks these rules. Without them, a reply can drop out of the body before the addon reads it.
- `notes` holds terminal pings (section 10). `permissions` holds open permission requests (9.3).
- String escapes follow one function in the `protocol` crate. The addon reads the file as Lua source, so the escape rules are part of the protocol.
- The bridge writes progress at most every 3 seconds. It writes final replies at once.
- If `LoadAddOn` returns `MISSING` or `DISABLED`, the addon reports "slots not installed".

**Poll schedule after a send:** the addon loads a slot at 5, 10, 16, 24, 34, 46, 60, 80, 100, 130, 160, 200, 240, and 300 seconds.
Then it loads one every 60 seconds until the reply is done.
With no message pending, it loads one slot every 10 minutes, for terminal pings and the status light.
A signal (7.4) makes the addon load a slot at once.

**Slot budget:** there are 1000 slots per UI session. Each reply costs about one slot when signals work, and about four when they do not.
The window never shows the slot count. `/relay diag` shows it.
Below 20 free slots, the window shows "Reload soon" with a **Reload** button, and the next click on **Send** or on the window does the `/reload` first.
`ReloadUI` needs a hardware event, and a click is one. The addon never reloads in combat, and never on a key press that the user did not aim at the window.
The chat history is in the saved variables, so a `/reload` keeps it.

### 7.4 Signals

**Status: signals do not work on the tested client (rule 4 fails).** The addon polls slots on the schedule in 7.3. This section stays for clients where the self-test passes. A replacement signal through font files (as in `wow-forever-codex`) is an open question.

Each signal is a `.wav` file. The bridge makes it empty (off) or writes a valid silent sound (on).
The valid sound is 8 kHz, 8-bit, mono, 80 samples.
The addon checks a signal every 2 seconds with `PlaySoundFile` on a muted channel.

| Family | Files | Meaning |
|---|---|---|
| `ack/NNN` | 200 | The bridge decoded message NNN. The addon takes it off the strip. |
| `sig/NNN` | 200 | The reply for message NNN is ready. |
| `act/NNN/kk` | 200 × 60 | Heartbeat kk for message NNN. The agent is still working. |
| `presence/kkkk` | 2000 | The bridge is alive. One every 30 seconds. |
| `note/kkkk` | 2000 | A ping or a permission request is waiting (section 10, 9.3). |
| `ctl/empty`, `ctl/valid` | 2 | The self-test at login. |

- `NNN = ((id − 1) mod 200) + 1`.
- Rule 5 in 7.2 makes each signal one-shot until the game restarts. After the message ids wrap past 200, a signal can already be valid. The addon treats an unexpected "valid" as unreliable and uses the poll schedule.
- `presence` and `note` are counters. The bridge keeps 50 files ahead of the counter empty. The counters live in `state.json`. `presence` wraps after about 16 hours.
- **Self-test:** at login, the addon plays `ctl/empty` and `ctl/valid`. If `ctl/empty` plays, or `ctl/valid` does not, signals are off for this session. The addon then uses slot polls only.
- **Status light:** with presence signals, 90 seconds of silence means "stale" and 300 seconds means "down". Without them, the addon spends one slot every 10 minutes, and the limits are 12 and 22 minutes.

Total file count for slots and signals: about 17,000.

### 7.5 Reload fallback

The addon uses the reload fallback when the strip gets no acknowledgment, the pool is empty, or the slots are missing.

1. The addon writes the signed frame of the message into `outbox` in its saved variables (6.6.1). The bridge checks it as a strip: tag, time, and replay store.
2. The addon asks the user to press a key. `ReloadUI` needs a hardware event, and the key catcher stays off in combat.
3. WoW writes the saved variables file at reload.
4. The bridge watches `WTF/Account/<ACCOUNT>/SavedVariables/GnomishRelay.lua` (checks the modification time every 250 ms).
5. The bridge writes the reply into `GnomishRelay/Inbox.lua`. The main addon reads it at the next reload.

After each `/reload`, the addon shows the strip again for every sent message that has no reply and is not in the outbox.
The saved variables also carry the `read` and `restored` state, so the bridge reads them from the file too.

### 7.6 Restore after a saved-data wipe

The beta client sometimes wipes addon saved data. The addon then makes a new token.
When a hello comes from an unknown token, and the bridge already knows another token, the bridge writes a restore bundle for the new token.
The bundle goes into `Restore.lua` in each slot of the window, next to the body. So the body keeps its own 1 MiB bound (S12).
The bundle stays in each publish until a strip from that token has the `restored` flag.
The addon applies a bundle only one time. It merges the chats by chat id, so a second copy of the bundle changes nothing.
After the `restored` flag, the bridge retires the older tokens and takes their records out of the slot body. A run of a retired token that ends later goes only into the history.

The bundle holds the 16 chats with the latest activity, and the last 10 messages of each (S18).
Each message is cut to 500 bytes, at a character boundary. The file is at most 512 KiB (S19).
The bridge keeps this history in `state.json`. The full transcripts come later (8.3).

### 7.7 Versioning

- The strip has a version byte. The bridge drops frames with an unknown version and logs it.
- Each slot body carries `proto` and the pool sizes. The hello (`h` flag) carries the addon version.
- On a mismatch, the addon shows "bridge and addon versions do not match" and stops sending.
- Pool sizes live in one place: the `protocol` crate. The setup step writes them into the addon.

### 7.8 Design for breakage

The transport rests on client behaviors that Blizzard never promised: an addon can call `Screenshot()`, and a load-on-demand addon reads its files fresh.
A client patch can break either one. So a patch costs a day of work, not the project.

**One interface per direction.** The core never knows which channel carries a message.

| Direction | Interface | Channels, in order |
|---|---|---|
| Out (game to bridge) | Addon `Out.Send(frame)`, bridge `trait FrameSource` | Strip by `Screenshot()`, strip by screen capture (section 11), reload outbox (7.5) |
| In (bridge to game) | Addon `In.Poll()`, bridge `trait Publisher` | Slots (7.3), fonts (a spike in 15), reload inbox (7.5) |

- The protocol core, the model, and the proofs work on frames and records. They do not change when a channel changes.
- A new channel is one new module on each side, with its own tests. Nothing else changes.

**Self-test and health report.**

- At login, the addon tests each channel. It takes one screenshot of a test strip, and it loads one slot and checks that the body is fresh.
- The hello carries the result and the client build from `GetBuildInfo()`: `out=shot`, `in=slots`, and `build=<number>`.
- If a channel fails, the addon moves to the next one of the table and shows one line: "Screenshots are blocked. Using screen capture."
- `/relay diag` shows each channel and its last success. `gnomish-relay doctor` does the same on the desktop.

**Builds.**

- The bridge keeps the last client build that passed the self-test.
- On a new build, the bridge logs it. The window shows "New game version: checking the relay" until the self-test passes.
- A known break goes into a table in the bridge, so the setup can name the channel that works on each build.

## 8. Architecture

```
 ┌──────────── WoW: Forever ────────────┐
 │  GnomishRelay addon (Lua)            │
 │   chat window, strip, slot loader,   │
 │   signal checks, reload fallback     │
 └──────┬──────────────────▲────────────┘
        │ pixels           │ slot files, signals
 ┌──────▼──────────────────┴────────────┐
 │  gnomish-relay bridge (Rust)         │
 │   capture → decode → policy → queue  │
 │   agent runner → publisher           │
 │   hook socket ◄── terminal sessions  │
 └──────┬───────────────────────────────┘
        │ ACP / native CLI / command
 ┌──────▼──────────┐
 │  Coding agents  │  Claude Code, Codex, Gemini CLI, ...
 └─────────────────┘
```

### 8.1 Repository layout

```
gnomish-relay/
  addon/GnomishRelay/   Lua addon
  crates/
    protocol/           frames, records, slot body, escapes, dedup, counters. No I/O. Verified with Aeneas.
    capture/            trait Capture + one backend per platform
    agents/             trait Agent + ACP, native, and command backends
    bridge/             the daemon: capture loop, policy, queue, publisher, state
    hook/               small CLI that terminal agent hooks call
  proofs/               Lean project with the Aeneas output and the proofs
  models/               Quint model of the transport
  tests/vectors/        golden strip images
  SPEC.md
```

### 8.2 Bridge main loop

1. Watch the Screenshots folder for a new file.
2. If a new strip is visible, decode it, check the MAC, and drop duplicates.
3. Apply the policy (6.2). If a record fails the policy, publish an error reply for it.
4. Put the record in the FIFO queue of its chat.
5. Start a run when fewer than `max_parallel_runs` runs are active.
6. Publish progress and the final reply. Raise the signals.

Each chat has a FIFO queue. A second message to a busy chat waits. It never replaces the first.
(`wow-claude` keeps one queued job per chat, so a second message replaces the first. Do not copy this.)

### 8.3 State

The bridge keeps its state in JSON files in the data folder of the OS:

- `state.json`: the replay store, the unread records, the waiting messages, the slot window, the tokens, and the restore history (7.6). Later also agent session IDs per chat, the folder of each session, and signal counters.
- `transcripts.json`: every prompt and reply, per chat. 200 messages per chat, 4000 characters each.

Rules:

- The bridge writes each state file atomically: write a temp file, then rename.
- A run starts only after `state.json` marks its message as seen. So a crash cannot run a message twice.
- A run that is in progress when the bridge stops ends as an error after the restart. It never runs again, because it can have changed files already.
- A damaged `state.json` stops the bridge at start. A fresh start forgets which messages ran.
- The rate limiter is not in the state. A restart gives a fresh minute.
- Waiting jobs are keyed by chat and message id. A second token with the same pair waits until the first job leaves the queue.
- Sessions are keyed by chat id alone, so they survive a saved-data wipe.
- Dedup keeps the last 1000 ids per token. The bridge prunes tokens that it has not seen for 30 days.
- The bridge compares folders by exact path. Linux paths are case sensitive. (`wow-claude` lowercases paths. Do not copy this.)

### 8.4 Only one bridge

Two bridges fight over the screen and the slot files.
The bridge takes an OS advisory lock (`fd-lock` crate) at start. The OS releases the lock when the process stops, also after a crash.
If the lock is taken, the bridge stops with an error.

## 9. Agents

### 9.1 The Agent trait

```rust
trait Agent {
    async fn open_session(&self, cwd: &Path, resume: Option<SessionId>) -> Result<SessionId>;
    async fn prompt(&self, session: &SessionId, text: &str) -> Result<EventStream>;
    async fn answer_permission(&self, request: RequestId, choice: Option<OptionId>) -> Result<()>;
    async fn cancel(&self, session: &SessionId) -> Result<()>;
}

enum Event {
    SessionStarted(SessionId),
    Text(String),
    ToolCall { name: String, detail: String },
    PermissionRequest(PermissionRequest),
    Denied(Vec<Rule>),
    Done { text: String, stop: StopReason },
    Error(String),
}

struct PermissionRequest {
    id: RequestId,
    session: SessionId,
    tool: String,
    detail: String,
    options: Vec<PermissionOption>,
}

struct PermissionOption { id: OptionId, kind: OptionKind, label: String }
enum OptionKind { AllowOnce, AllowAlways, RejectOnce, RejectAlways }
enum StopReason { EndTurn, MaxTokens, Refusal, Cancelled }
```

- `resume` restores a session after a bridge restart. If the agent cannot resume, the bridge opens a new session and tells the user in the game.
- `answer_permission(id, None)` means "cancelled".
- `Denied` carries the rules that the agent refused. The **Allow & retry** button needs them.

### 9.2 Backends

| Backend | How it works | Progress | Live permissions | Allow & retry |
|---|---|---|---|---|
| `acp` (main) | Agent Client Protocol: JSON-RPC over stdin and stdout. The bridge is the client. | Yes | Yes | Not necessary |
| `native-claude` | `claude -p --output-format stream-json --verbose --resume <id>` | Yes | No | Yes, from `permission_denials` |
| `native-codex` | `codex exec --json` | Yes | No | No. Fixed level from config. |
| `command` | A command template. The prompt goes in, plain text comes out. | No | No | No. Fixed level from config. |

**Support levels.** Any agent with a command line runs. How well the relay protects it depends on what the bridge can see:

| Level | Connection | What the classifier sees | Examples |
|---|---|---|---|
| Full | ACP, or a tool-call hook | Every tool call, before it runs | Gemini CLI, Claude, Codex through `codex-acp`, any ACP agent |
| Sandbox only | `command` | Nothing | Aider, `llm`, a script |
| Trusted | `command` with no sandbox | Nothing | The same agents on Windows |

- A Full agent runs in its "ask for everything" mode. The classifier then answers most questions itself. In a looser mode the agent acts without asking, and the classifier never sees the action.
- `command` needs the sandbox. With no sandbox, the level is Trusted: it is off by default, and the config turns it on after a warning.
- The chat header shows the level next to the agent name, for example "Aider · trusted".
- An ACP agent needs one line in the config. An agent with a hook system needs a small hook command. Every other CLI agent uses `command`.

ACP agents (checked 2026-09-23):

- Gemini CLI: `gemini --acp`. It supports new sessions, `loadSession`, and `setSessionMode`.
- Claude Code: the `claude-agent-acp` adapter (formerly `claude-code-acp`).
- Codex: the `codex-acp` adapter, now in the `agentclientprotocol` organization.

The Rust crate is `agent-client-protocol` 2.2.x. Its API uses builders and roles.
Pin 2.2.x. Do not turn on the unstable v2 features in v1.
Put a thin adapter between the crate and the `Agent` trait, so that API changes stay in one file.

### 9.3 Permissions

Each agent in the config has one permission level:

| Level | Meaning |
|---|---|
| `ask` | Every command outside the allowlist needs an answer. |
| `auto-edit` | File edits inside the chat folder need no answer. |
| `full-auto` | Nothing needs an answer. |

Each backend maps the level differently:

- `acp`: the bridge sets the session mode. Mode IDs differ per agent, so the config has a `modes` table per agent.
- `native-claude`: `--permission-mode` and `--allowedTools`.
- `native-codex` and `command`: the level is fixed by the command in the config. The addon shows the level in the chat header. If the level is `full-auto`, the addon shows a warning.
- For game messages, Codex runs through ACP only, so the classifier sees its tool calls (6.6.3).

**Live permission flow (ACP):**

1. The agent sends `session/request_permission`. The run waits.
2. The bridge adds the request to `permissions` in the next publish and raises a `note` signal.
3. The addon shows a popup with the options.
4. The user picks an option. The addon sends a record with the `perm=<request>:<option>` flag.
5. The bridge answers the agent.

Rules:

- `allow_always` from the game follows 6.6.5.
- The run timeout stops while the run waits for a permission answer. A separate `permission_timeout_minutes` applies (default 10). After it, the bridge answers "cancelled".
- If the game closes or reloads, open requests stay in the next publish until they time out.

### 9.4 Agent processes

- ACP: one agent process per agent kind. It serves many sessions. For game messages: one process per chat folder, inside the sandbox (6.6.4).
- `native-*` and `command`: one process per run.
- `max_parallel_runs` counts active runs, not processes.
- If an ACP process stops, the bridge starts it again and resumes the open sessions. If a session cannot resume, the bridge reports an error for that chat.
- `cancel` for `native-*` and `command` stops the whole process tree.
- The bridge declares ACP client capabilities `fs` and `terminal` as false in v1. The agent uses its own tools.
- If an agent needs a login, the bridge reports "agent needs login" in the game. The bridge never handles credentials.
- The bridge removes `CLAUDECODE` from the environment of each child process. It sets `GNOMISH_RELAY_JOB=1` (section 10).

### 9.5 Sessions and folders

Claude stores sessions per project folder.
If a chat changes folder, the bridge starts a new session for it.
The bridge stores the folder of each session in `state.json`.

## 10. Pings from terminal sessions

The `gnomish-relay-hook` CLI sends one event to the bridge:

```json
{ "source": "claude", "session": "…", "cwd": "/path/to/repo", "kind": "turn-done", "message": "…" }
```

`kind` is `turn-done` or `needs-input`.
If `GNOMISH_RELAY_JOB` is set, the CLI exits at once and sends nothing. This stops the bridge's own runs from pinging the game.

**Socket:** a Unix socket with mode 0600 in `$XDG_RUNTIME_DIR` (Linux, macOS), or a named pipe with a current-user ACL (Windows).
The `interprocess` crate gives one API for both.

**Hook points:**

| Tool | Hook | Input |
|---|---|---|
| Claude Code | `Stop` hook → `turn-done`. It fires at the end of each turn. | JSON on stdin: `session_id`, `cwd`, `last_assistant_message` |
| Claude Code | `Notification` hook with matcher `permission_prompt\|idle_prompt\|agent_needs_input` → `needs-input` | JSON on stdin |
| Codex | `Stop` event in Codex hooks (`hooks.json`). Or the `notify` program (`agent-turn-complete`). | `notify` gets the JSON as one command-line argument, not on stdin. |
| Gemini CLI | Not checked yet. | |
| Other tools | A wrapper script that sends `turn-done` when the command exits. | |

Codex `notify` accepts only one program. If the user already has one, the hook CLI calls it after it sends the event.

**In the game:**

- The bridge adds the ping to `notes` in the next publish and raises a `note` signal.
- Pings share the slot budget. The bridge merges pings that arrive within 30 seconds into one publish.
- A ping shows the repo name and the message, with a sound.
- Custom sounds must exist when the game starts (7.2 rule 1). The setup step installs them. The default uses built-in sound kit IDs.

## 11. Platforms

`Screenshot()` makes the capture layer a fallback, so only a few paths change per platform. All other code is shared.
The capture rows in this table apply only to the fallback.

| Part | Linux | Windows | macOS |
|---|---|---|---|
| Capture | Wayland portal with PipeWire (`ashpd`), or X11 (`x11rb`) | Windows.Graphics.Capture | ScreenCaptureKit |
| Capture permission | One portal prompt. The bridge stores the restore token (`persist_mode = 2`), so the prompt does not repeat. | None | One "Screen Recording" prompt |
| Find the WoW window | Process `WowB.exe` (Wine PID through `_NET_WM_PID`) or `WM_CLASS` | Process `WowB.exe` | Window owner name |
| WoW folder | Inside the Wine prefix | `Program Files (x86)\World of Warcraft\_classic_beta_` | `/Applications/World of Warcraft/_classic_beta_` |
| Hook socket | Unix socket | Named pipe | Unix socket |
| Replace a file that the game has open | Rename always works | Rename can fail. Retry with backoff, then log. | Rename always works |

Do not match the window by the title "World of Warcraft". That title also matches "World of Warcraft Launcher".

### 11.1 Linux notes (the first target)

The development machine runs Wayland with XWayland. The home file system is ext4.

- WoW can run on D3D12 through vkd3d-proton, or on D3D11 through DXVK.
- If Wine uses its native Wayland driver, X11 capture cannot see the window. Then only the portal works. The bridge detects this case.
- X11 capture of a Vulkan window under XWayland can return a black image. The spike tests this.
- The bridge finds `Interface/AddOns` and `WTF/Account/<ACCOUNT>` without regard to case. It never makes a second folder that differs only in case, for example `Addons` next to `AddOns`.
- The game makes `Interface/` and `WTF/` only after its first start. The setup step makes `Interface/AddOns` if it is missing.

### 11.2 Other platform notes

- **File system:** any file system works except FAT32 and exFAT. (`wow-claude` says NTFS. That line comes from `wow-forever-codex`, which stores 65,535 font files. It has no reason in `wow-claude`.)
- **Exclusive fullscreen** blocks capture. WoW must run windowed or borderless.
- **HDR** is not tested.
- **The `claude` command on Windows** is `claude.cmd` in some installs. The bridge finds the path with the `which` crate.

## 12. Config

The config file is `config.toml` in the config folder of the OS:

| OS | Config folder | Data folder (`state.json`) |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/gnomish-relay`, or `~/.config/gnomish-relay` | `$XDG_DATA_HOME/gnomish-relay`, or `~/.local/share/gnomish-relay` |
| macOS | `~/Library/Application Support/gnomish-relay` | the same |
| Windows | `%APPDATA%\gnomish-relay` | `%LOCALAPPDATA%\gnomish-relay` |

`gnomish-relay setup <wow folder>` writes the first config. It never replaces a config.

The bridge accepts only the keys that it implements. Any other key is an error, so a typo never leaves a wider default in place.
Today these keys work: `allowed_roots`, `default_cwd`, `default_agent`, `[wow] path`, and `[agents.<name>] permission`.
The other keys below come with their features.
Each root must exist. The bridge resolves links in it at start. `default_cwd` must be inside a root.

```toml
default_cwd = "~/Documents/Code"
allowed_roots = ["~/Documents/Code"]
max_parallel_runs = 3
max_messages_per_minute = 10
timeout_minutes = 30
permission_timeout_minutes = 10
default_agent = "claude"

[wow]
path = "~/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_"

[capture]
backend = "auto"            # auto | portal | x11 | windows | macos
process = "WowB.exe"

[agents.claude]
kind = "acp"
command = ["claude-agent-acp"]
permission = "auto-edit"
modes = { ask = "default", auto-edit = "acceptEdits", full-auto = "bypassPermissions" }

[agents.codex]
kind = "acp"
command = ["codex-acp"]
permission = "ask"

[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"

[agents.aider]
kind = "command"
command = ["aider", "--yes", "--message-file", "{prompt_file}"]
permission = "full-auto"    # --yes approves everything
```

The Claude mode IDs are not checked yet.

## 13. The addon

### 13.1 Look

The window follows the classic Guild & Communities frame, and uses the built-in game textures and fonts.
The mockup is the reference for the layout.

- **Frame:** the dark metal frame, a black title bar with the gold title "Gnomish Relay", and gold-framed red minimize and close buttons.
- **Portrait:** a round emblem at the top-left corner: a red pipe wrench on a brass cog. It is our own drawing, shipped as a texture.
- **Left column:** one tile per chat, with the agent as the shield icon. The selected tile glows green. A gold "!" marks a new reply. The last tile is "Start a New Chat".
- **Center:** a dropdown for the agent and the permission mode, the folder, and the bridge light. Below them, the transcript on a black background in classic lines: `[You]: text` and `[Claude]: text`. The text is white. Only the name has a color: the user in blue, each agent in its own color. Code shows in black boxes in a shipped mono font.
- **Input:** one empty line, with no label and no hint text. Enter sends. The limit is 3200 characters.
- **Right column, Activity:** a cast bar while the agent works, and one row per step. A tooltip on each row shows the details.
- **Side tabs:** Chats, Terminal pings, Settings, and Diagnostics.
- **Bottom bar:** a red **Stop** button, only while an agent works. It stops the run.
- **Game chat:** a finished reply or a ping shows one line, `[Claude] whispers: [chat] …`, in its own color (copper by default, a setting). A click on it opens the chat. It plays the whisper sound.
- **Permission requests** use the separate popup of 6.4, never the window.

### 13.2 Code

The addon is our own code. It uses the design of `wow-claude`, not its files.
All state is local to the addon files, which share one table. The files load in this order:

| File | Job |
|---|---|
| `Key.lua` | The strip key. `scripts/dev-link.sh` writes it, and git ignores it. |
| `Sha256.lua` | SHA-256 and HMAC-SHA256 for the strip tag. |
| `Codec.lua` | Records, frames, and cells: the Lua side of `crates/protocol`. |
| `Store.lua` | The saved data: token, chats, and the outbox. |
| `Strip.lua` | Draws a frame and takes one screenshot of it. |
| `Transport.lua` | The strip retries, the poll schedule, the slots, and the flags. It follows `models/transport.qnt`. |
| `Window.lua` | The window of 13.1. |
| `Core.lua` | Startup, slash commands, and the whisper line. |

Message ids start from the clock, so the ids after a saved-data wipe never repeat the ids in an older body.

The tests run the addon in a real Lua 5.1 with a fake WoW API (`addon/tests/wow.lua`), from `crates/bridge/tests`.
They decode each strip with the proved Rust decoder and check its tag against the Rust HMAC.
They also check the SHA code against both kinds of `bit` results: unsigned as in WoW, and signed as in LuaJIT.

Still to come: the permission popup (9.3), pings (section 10), the side tabs, the agent dropdown, and the emblem texture.

Slash commands:

| Command | Action |
|---|---|
| `/relay` | Open or close the window. |
| `/ai <text>` | Send a message to the current chat. |
| `/relay diag` | Show transport diagnostics. |
| `/relay poll` | Load the next slot now. |

## 14. Verification and tests

The two priorities are readability and test coverage. `CLAUDE.md` has the rules for code and tests.

### 14.1 Aeneas proofs of the protocol core

`crates/protocol` is pure Rust inside the subset that Aeneas supports (see `CLAUDE.md`).
Charon translates it to LLBC. Aeneas translates LLBC to Lean. The proofs live in `proofs/`.

The core is the place where untrusted input enters the system: pixels from a screenshot in one direction, agent replies in the other.
So most theorems are security properties. Each one closes a named attack.

**Security theorems (untrusted input):**

| # | Theorem | Attack that it closes |
|---|---|---|
| S1 | **Decoder totality:** for every image grid, `decode_frame` returns a frame or a defined error. It never panics and never reads out of bounds. | A crafted strip crashes the bridge. |
| S2 | **Checksum and tag gate:** `decode_frame` returns a frame only if the checksum matches and `verify_tag(key, bytes, tag)` is true. `verify_tag` is an opaque function in the proof. | A fake strip from another addon passes as real. |
| S3 | **Record parser totality:** for every byte string, `parse_records` returns records or a defined error. | A crafted payload crashes the bridge. |
| S4 | **Field isolation:** no byte of one field ends up in another field. | Text bleeds into the `cwd` or `flags` field and changes the folder or the permissions. |
| S5 | **Folder policy:** if `resolve_folder(roots, request)` accepts, the result is inside one of the roots. This holds for every request, also with `..`, `.`, repeated `/`, and trailing `/`. | A message escapes `allowed_roots`, for example `../../.ssh`. |
| S6 | **No privilege from the game:** the effective permission level is at most the level in the config, for every flag list. A rule from the game is bounded by S17, and never covers a "never always" command. | A message from the game raises its own permissions. |
| S7 | **Replay protection:** a `(token, id)` pair is accepted at most one time while it is in the window. | A replayed strip runs a task two times. |
| S8 | **Lua escape:** for every string, the escape function gives a Lua string literal that reads back as the same string. The output never ends the literal early. | A reply from a malicious agent injects Lua code into the game. |
| S9 | **Slot body shape:** the slot file writer only puts escaped strings and numbers into a fixed table shape. | A malicious agent changes `proto`, adds fields, or runs code in the slot file. |
| S18 | **Restore file shape:** the restore writer only puts escaped strings and numbers into a fixed table shape. Its prepare step keeps the last 16 chats and the last 10 messages of each, and cuts only the ends of strings. | A chat name or a message from a malicious agent runs code in the restore file. |
| S19 | **Restore size bound:** a restore file that fits is at most 512 KiB. | A long chat history makes a restore file that the game cannot load. |
| S10 | **UI escape:** the display sanitizer doubles every `\|` in agent text. | A malicious agent fakes a WoW chat link (`\|H...\|h`), a texture, or a color that imitates a system message. |
| S11 | **Freshness:** the bridge accepts a frame only if its time is at most 5 minutes old and at most 1 minute in the future. | An old screenshot of a strip is replayed. The MAC is still valid, so S2 does not stop it. |
| S12 | **Size bounds:** for every input, a slot body is at most 1 MB, and each reply record in it is at most 32 KB. | A malicious agent writes a huge reply, and the bridge writes 200 huge slot files. |
| S13 | **ID charset:** the id validator accepts only `[a-z0-9_-]`, 1 to 32 characters. | A chat id like `../../x` reaches a file path or a state key. |
| S14 | **Rate limit and queue cap:** the limiter never admits more than N messages in any window. A chat queue never holds more than 20 messages. | Strip spam fills memory or starts many runs. |
| S15 | **Honest popup:** the popup text contains the full raw command, or its start and end with a cut mark. It contains no raw control, bidi, or zero-width characters. | A malicious agent asks for permission with a false label, or hides the dangerous part of a command. |
| S16 | **Classifier paths:** let `paths(call)` be the path fields of a file tool, plus the redirect targets and the working folder of a command. Command arguments are out of scope. If `classify(call) ≥ ask`, then every write path is inside the chat folder, every read path is inside `allowed_roots`, and no path is a `desktop` or `deny` path. If a path is inside `~/.config/gnomish-relay`, then `classify(call) = deny`. | An approved tool call in the game reads `~/.ssh`, writes outside the project, or reads the strip key. |
| S17 | **Classifier ceiling:** with the order `deny < desktop < ask < allow`, for every tool call and every rule list from the game, `classify(call, rules) ≤ classify(call, config)`. A "never always" command and an unknown tool never get `allow` from a rule. | A rule from the game, or a crafted command, gets more than the config allows. |

**Correctness theorems:**

| # | Theorem |
|---|---|
| C1 | **Cell round trip:** bytes → 3-bit cells → bytes gives the same bytes, followed by the zero padding of the last group. **Proved** (`Protocol.Cell.cells_round_trip`, 2026-09-23), for inputs up to 65536 bytes. |
| C2 | **Frame round trip:** for every payload of at most 3200 bytes, `decode_frame(encode_frame(m)) = m`. |
| C3 | **Record round trip:** for records with no RS in any field and no US before `text`, `parse(serialize(r)) = r`. |

**Order:** C1 first, because it is the smallest. Then S15 and S11, because they close the most real risks. Then S1, S3, S8, and S5, because those inputs come from outside. Then the rest.

**Proof hygiene:**

- `proofs/Axioms.lean` prints the axioms of every top theorem. `scripts/check-proofs.sh` fails if the list contains anything other than `propext`, `Classical.choice`, and `Quot.sound`.
- `bv_decide` and Aeneas's `bv_tac` add a native-code axiom, so they are not allowed. Bit facts are proved bit by bit (`ext`, then `simp`), which the kernel checks.
- The generated Lean in `proofs/Protocol/Code` is in the repo. `scripts/check-proofs.sh` regenerates it and fails if it differs.
- The Aeneas standard library has 4 `sorry` placeholders (in `Slice` and `StringIter`, checked 2026-09-23). The axiom check catches every proof that depends on them.
- `native_decide` is not allowed. It adds an extra axiom and trusts compiled code.

**What the proofs do not cover:**

- **The crypto.** HMAC-SHA256 comes from the `hmac` and `sha2` crates. The proofs treat `verify_tag` as opaque. The bridge compares tags in constant time.
- **The file system.** S5 is about path text. A symbolic link inside a root can still point outside. The bridge resolves links with `canonicalize` and runs the S5 check again on the result.
- **What the agent does on the host.** A malicious or confused agent can do damage inside its folder, within its permission level. Only the permission level (S6), the folder policy (S5), and the agent sandbox limit that. No proof in this project can make an agent safe.
- **Hostile addons in the same Lua environment.** See 6.1.

**Design rules from the first proofs:**

- Do not cast `bool` to an integer in the core. Integer bit operations (`(v >> 2) & 1`) are easier to prove.
- Put bit arithmetic in tiny helpers that take and return integers (`cell_at`, `append_cell`). One large function with 30 bit operations timed out in the proof. The same code split into helpers proves in seconds.
- The cell codec works in groups: 3 bytes (24 bits) are exactly 8 cells. The bit stream is the same as in 7.1. The encoder pads the last group with zero bytes, and the frame header carries the real length.

### 14.2 Quint model of the transport

`models/transport.qnt` models the addon, the bridge, the slots, the signals, `/reload`, and a saved-data wipe.
The model checker checks these properties:

- The agent never runs one message twice.
- A publish never loses a reply that the addon has not read.
- After a saved-data wipe, the restore never duplicates or drops a chat.
- Each sent message ends with a reply or an error, also across `/reload`.

Write the model before the bridge state machine. The Rust state machine follows the model.

### 14.3 Tests

- **Property tests** (`proptest`) for the codec, with pixel noise, color shift, and a cell pitch of 3 to 8 pixels.
- **Golden vectors:** run the addon `Codec.lua` under `mlua` to make strip images with noise and gamma. Commit them in `tests/vectors/`. The Rust decoder must decode all of them. (`wow-claude` makes its images at test time and tests them only on Windows.)
- **Differential tests:** the Lua encoder and the Rust decoder agree on every vector. The Rust slot writer and a Lua reader agree on every body.
- **Addon harness:** run the addon in a Lua VM against a stub of the WoW API, as `wow-claude` does with `tests/wow_stub.lua`.
- **Fuzzing:** `cargo-fuzz` on the frame decoder and the record parser. No panic and no hang on any input.
- **Fake agent and fake capture** for the bridge loop. No test needs the game or a real LLM, except live tests marked `#[ignore]`.
- **Coverage gates:** `protocol` 95% of lines, `bridge` and `agents` 80%.
- **CI** on Linux, Windows, and macOS. CI runs everything except live capture.

### 14.4 Fuzz targets

Each target runs in CI for a short time and nightly for a long time. Every crash becomes a regression test.

| Target | Why |
|---|---|
| Frame decoder and record parser | Backs up S1 and S3 on the compiled code. |
| PNG decoding with the size limit | Any local program can write to the Screenshots folder. We did not write the PNG decoder. |
| Hook socket messages | Any process of the same user can connect. |
| `resolve_folder` with Unix and Windows path forms | Windows has `\\?\`, UNC paths, `C:foo`, `file:stream`, and reserved names such as `CON`. S5 must hold for all of them. |
| Lua escape, with the output loaded in a real Lua 5.1 VM | Backs up S8 against the real Lua parser. Inputs include NUL bytes, invalid UTF-8, and `]]`. |
| UI escape and popup text | Backs up S10 and S15. |
| `config.toml` parser | A broken or hostile config gives an error, never a wider permission. |

### 14.5 Security tests

Each rule in 6.2 has at least one named test. These are the ones that need a real file system or a real process:

- A slot folder replaced by a symbolic link: the bridge refuses to write.
- A symbolic link in the Screenshots folder: the bridge does not follow it and does not delete its target.
- A normal screenshot with no strip: the bridge leaves it alone.
- A prompt such as `; rm -rf ~`: the agent gets it as one argument.
- A prompt file: it has mode 0600 in a private folder, and it is gone after the run.
- `config.toml` after setup: it has mode 0600.
- The tag check: it uses `subtle::ConstantTimeEq` (a test on the code, not on timing).
- A hello with a new token and a bad MAC: no restore bundle.
- The environment of an agent process: it contains only the allowlist.
- A prompt with newlines: `bridge.log` has one line for it.
- On macOS and Windows: `allowed_roots` works with a root in a different letter case.

### 14.6 Supply chain

- `cargo deny` (licenses, sources, duplicate versions) and `cargo audit` (known CVEs) run in CI.
- `Cargo.lock` is in the repo.
- The verified core (`crates/protocol`) has no dependencies. Every dependency there is code that we trust but do not prove.
- New dependencies in the bridge need a reason in the commit or PR.

## 15. Build order

0. **Start WoW once.** This makes `Interface/` and `WTF/Account/`.
1. **Done: `Screenshot()` spike.** A test addon draws a strip and calls `Screenshot()` from an event, with no key press. If a PNG appears, WoW writes the strip image itself, and the capture layer (section 11) becomes a fallback. The addon hides the "Screen captured" text through the `ActionStatus` frame.
2. **Skipped: capture spike.** Step 1 passed. Capture the top-left 800×192 pixels of the WoW window content 4 times per second. Save one frame as PNG. Test the portal and X11 paths.
3. **Done: Wine rules spike.** Test the five rules in 7.2 under Wine: the `ctl` self-test, a fresh read of a load-on-demand file, and "a new file is not found". Results in `spikes/README.md`. The HMAC-SHA256 cost in WoW Lua is not measured yet.
4. **Done: `protocol` crate with Aeneas.** Frame, cells, records, slot body, escapes, and every theorem in 14.1. `VERIFICATION.md` has the status.
5. **Done: slot writer.** Publish a fixed reply. Make sure that it shows in the game. Passed in the game on 2026-09-24: `install`, then `say`, then `/relay poll` showed the reply. The steps are in `addon/README.md`.
6. **Addon port** with the stub harness and the differential tests.
7. **Done: Quint model** of the transport. **Done (7a):** the bridge reads strips from screenshots, checks the tag and the time, queues per chat, runs an echo agent, and publishes. Tests run one message around the whole loop. **Done (7b, part):** the addon signs each message at send, and the bridge reads the signed outbox frames from the saved variables. **Done (7b):** `state.json` and the restore bundle in `Restore.lua`.
8. **Threat model in code:** `allowed_roots`, the policy, and the MAC check. **Done (8a):** `config.toml`, the `level` flag under the ceiling of the config (S6), and "Agent not set up." **Next:** the classifier (6.6.3) needs the tool calls of step 9.
9. **ACP backend.** Test with one agent first.
10. **`note` signal and pings:** the hook CLI and the socket.
11. **`native-*` and `command` backends.**
12. **Windows and macOS capture backends.** Mark them experimental until a tester on each OS makes sure that they work.

Steps 1 to 5 prove the channels. After those, the rest is normal Rust work.

**Taint spike (before the taint warning ships):** a second test addon calls the send handler of our addon, calls a closure that reads our tables before the probe, fills our input box, and clicks our buttons. The spike records what the probe names in each case, on the Forever client under Wine, Windows, and macOS. Some cases will likely name "GnomishRelay". Nothing in 6.6.5 depends on this spike.

## 16. Development environment

- `dev gnomish-relay` opens tmux with nvim, the agent, and a terminal in this folder.
- Link `addon/GnomishRelay` into `_classic_beta_/Interface/AddOns`. Then an edit plus `/reload` loads the new code, with no copy step.
- Run the bridge in the bottom-right pane.
- Aeneas and Charon are built in `~/verif`. `proofs/TOOLS` pins their commits, and CI builds the same commits with Nix.

## 17. Open questions

- Can an AppContainer or a restricted token give Claude and other agents a sandbox on native Windows?
- Which `claude` flag keeps the project and user settings out of a run (6.6.4)?
- What can Codex read inside `workspace-write`?
- Two WoW accounts on one computer have two tokens. A hello from the second account starts a restore, and its `restored` flag retires the first token. How does the bridge tell two accounts from a saved-data wipe?

1. Does X11 capture of the WoW window work under XWayland? (Only for the fallback.)
2. Can font files replace the `.wav` signals?
3. How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
4. What are the ACP mode IDs of `claude-agent-acp` and `codex-acp`?
5. Does Gemini CLI have hooks for pings?
6. How large is the hitch at a higher window size? (The "Screen captured" hide works.)
7. Does the Aeneas standard library model cover the `Vec` and slice functions that the core needs?
