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
| Another addon or a WeakAura | Runs Lua in the same environment as our addon. It can call our functions or draw a strip. | Partial. Keep all addon functions local. The bridge policy (6.2) limits the damage. |
| A stream or recording | The strip shows the prompt on screen | None. Do not stream while you use the relay. The README says this. |

A hostile addon in the same Lua environment can always act as the user.
No design inside the game can stop this.
So the bridge limits what any message from the game can do.

### 6.2 Bridge policy

1. The folder of a chat must be inside `allowed_roots` from the config. The bridge rejects all other folders.
2. The permission level of each agent comes only from the bridge config. A message from the game cannot raise it.
3. An "allow" answer from the game applies to the current session only. A permanent rule needs a confirmation outside the game (a desktop notification or the terminal).
4. The bridge limits the message rate: at most 10 messages per minute (config key `max_messages_per_minute`).
5. The bridge never runs the agent with `full-auto` unless the config sets it for that agent.
6. The bridge rejects frames with a timestamp more than 5 minutes old or more than 1 minute in the future (S11).
7. The bridge never writes, renames, or deletes through a symbolic link. It opens files with `O_NOFOLLOW` (Unix) or checks the reparse point (Windows).
8. The bridge deletes only the screenshots that it decoded as valid strips. It never deletes other screenshots.
9. The bridge limits sizes: an image before decoding (4096 × 4096 px), a hook message (64 KB), a reply record (32 KB), a slot body (S12), and each chat queue (20 messages).
10. The bridge resolves symbolic links in a chat folder with `canonicalize`, then checks `allowed_roots` again on the result.
11. The bridge never starts a process through a shell. It passes the command as an argument list.
12. The bridge gives each agent process only an allowlist of environment variables (`PATH`, `HOME`, `LANG`, `TERM`, and the variables in the agent config). All others, for example API keys of other tools, stay out.
13. The bridge writes prompt files with mode 0600 in a private folder, and deletes them after the run.
14. Setup writes `config.toml` with mode 0600, because it holds the strip key.
15. The bridge escapes control characters and newlines in `bridge.log`, so a prompt cannot fake a log line.
16. The bridge sends a restore bundle only in answer to a hello with a valid MAC.

### 6.3 Strip authentication

The setup step makes a random 32-byte key.
It writes the key into the addon (as a file-local value) and into the bridge config.
Each strip ends with a truncated HMAC-SHA256 tag (8 bytes) of the header and payload.
The bridge drops each strip with a wrong tag, and logs it.
The bridge compares tags in constant time (`subtle::ConstantTimeEq`).

Open point: the cost of HMAC-SHA256 in WoW Lua (with the `bit` library). The spike measures it.

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
- The strip shows the prompt text on screen for up to 40 seconds.

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
So each strip starts with a calibration row of known colors. The decoder fits the cell width and height to that row as real numbers, then reads the data rows.
The search area is the top-left 800×192 pixels of the image.

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
| `agent=<name>` | The agent for a new chat. |
| `perm=<request>:<option>` | The answer to a permission request (9.3). |
| `read=<id>,<id>` | The final replies that the addon has shown since its last `read` flag. The bridge then takes them out of the slot body (7.3). |
| `restored` | The addon has applied the restore bundle for its token (7.6). |

**Strip lifetime:**
The strip stays up until the bridge acknowledges it, or for 40 seconds.
If no acknowledgment comes, the addon shows the strip again, up to 3 times.
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

The bridge cannot know which slot the addon loads next.
So each publish writes the same body into all 200 slots, each with an atomic rename.
The addon loads the first slot that it has not loaded in this UI session.

Each slot is a folder `GnomishRelay_S001` to `GnomishRelay_S200` with two files:

- `GnomishRelay_SNNN.toc`: `## Interface: 16001`, `## LoadOnDemand: 1`, `## Dependencies: GnomishRelay`, and the Lua file name.
- `Inbox.lua`: the body.

The body sets one global table:

```lua
GnomishRelay_SlotData = {
  proto = 1, slots = 200, ack_max = 200, presence_max = 2000, note_max = 2000,
  ts = 1790211079, now = 1790211081,
  cwd = "/home/eitan/Documents/Code",
  replies = {
    { chat = "c1", id = 12, status = "working", text = "...", cwd = "...", session = "...",
      progress = { "edit src/main.rs", "$ cargo test" }, denied = { "Bash(rm:*)" } },
  },
  notes = { { seq = 41, source = "claude", repo = "lighthouse", kind = "done", text = "..." } },
  permissions = { { request = "p7", chat = "c1", tool = "Bash", detail = "cargo test",
                    options = { { id = "o1", kind = "allow_once", label = "Allow" } } } },
  restore = nil,
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
A signal (7.4) makes the addon load a slot at once.

**Slot budget:** there are 200 slots per UI session. Each reply costs about one slot when signals work, and about four when they do not.
When the pool is empty, the addon asks for a `/reload` (7.5).

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

1. The addon writes the message into `outbox` in its saved variables. Text and folder are hex-encoded.
2. The addon asks the user to press a key. `ReloadUI` needs a hardware event, and the key catcher stays off in combat.
3. WoW writes the saved variables file at reload.
4. The bridge watches `WTF/Account/<ACCOUNT>/SavedVariables/GnomishRelay.lua` (checks the modification time every 750 ms).
5. The bridge writes the reply into `GnomishRelay/Inbox.lua`. The main addon reads it at the next reload.

After each `/reload`, the addon shows the strip again for every sent message that has no reply and is not in the outbox.
The saved variables also carry the `read` and `restored` state, so the bridge reads them from the file too.

### 7.6 Restore after a saved-data wipe

The beta client sometimes wipes addon saved data. The addon then makes a new token.
When the bridge sees an unknown token, it adds a `restore` bundle to each publish, addressed to that token.
The bundle stays in each publish until a strip from that token has the `restored` flag.
The addon applies a bundle only one time. It merges the chats by chat id, so a second copy of the bundle changes nothing.
After the `restored` flag, the bridge retires the older tokens and takes their records out of the slot body. Their replies are in the transcripts and in the bundle.
The bundle holds up to 16 chats, 40 messages each, 2000 characters per message.

### 7.7 Versioning

- The strip has a version byte. The bridge drops frames with an unknown version and logs it.
- Each slot body carries `proto` and the pool sizes. The hello (`h` flag) carries the addon version.
- On a mismatch, the addon shows "bridge and addon versions do not match" and stops sending.
- Pool sizes live in one place: the `protocol` crate. The setup step writes them into the addon.

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

- `state.json`: agent session IDs per chat, the folder of each session, handled message IDs, signal counters.
- `transcripts.json`: every prompt and reply, per chat. 200 messages per chat, 4000 characters each.

Rules:

- The bridge writes each state file atomically: write a temp file, then rename.
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

**Live permission flow (ACP):**

1. The agent sends `session/request_permission`. The run waits.
2. The bridge adds the request to `permissions` in the next publish and raises a `note` signal.
3. The addon shows a popup with the options.
4. The user picks an option. The addon sends a record with the `perm=<request>:<option>` flag.
5. The bridge answers the agent.

Rules:

- `allow_always` from the game counts as `allow_once` for the session (6.2 rule 3).
- The run timeout stops while the run waits for a permission answer. A separate `permission_timeout_minutes` applies (default 10). After it, the bridge answers "cancelled".
- If the game closes or reloads, open requests stay in the next publish until they time out.

### 9.4 Agent processes

- ACP: one agent process per agent kind. It serves many sessions.
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

The config file is `config.toml` in the config folder of the OS (from the `directories` crate).

```toml
default_cwd = "~/Documents/Code"
allowed_roots = ["~/Documents/Code"]
max_parallel_runs = 3
max_messages_per_minute = 10
timeout_minutes = 30
permission_timeout_minutes = 10
default_agent = "claude"
strip_key = "…"            # written by setup, 32 random bytes as hex

[wow]
path = "~/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_"
account = "auto"            # the folder name under WTF/Account, or "auto"

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

The first version starts from the `wow-claude` addon. These changes are necessary:

- Rename to `GnomishRelay`: folder, `.toc`, saved variables, slot names.
- Keep all functions and state local to the addon files. Expose only what the slot files need.
- Add the MAC (6.3), the new frame header (7.1), and the version check (7.7).
- Add an agent name and a permission level to each chat.
- Add the permission popup (9.3).
- Add pings (section 10) to the chat window and the game chat.
- Add the `note` signal family (7.4).

Slash commands:

| Command | Action |
|---|---|
| `/relay` | Open or close the window. |
| `/ai <text>` | Send a message to the current chat. |
| `/relay new <agent> [name]` | Start a new chat with an agent. |
| `/relay cd <folder>` | Set the folder of the current chat. It must be inside `allowed_roots`. |
| `/relay diag` | Show transport diagnostics. |

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
| S6 | **No privilege from the game:** the effective permission level is at most the level in the config, for every flag list. `allow_always` from the game never becomes a permanent rule. | A message from the game raises its own permissions. |
| S7 | **Replay protection:** a `(token, id)` pair is accepted at most one time while it is in the window. | A replayed strip runs a task two times. |
| S8 | **Lua escape:** for every string, the escape function gives a Lua string literal that reads back as the same string. The output never ends the literal early. | A reply from a malicious agent injects Lua code into the game. |
| S9 | **Slot body shape:** the slot file writer only puts escaped strings and numbers into a fixed table shape. | A malicious agent changes `proto`, adds fields, or runs code in the slot file. |
| S10 | **UI escape:** the display sanitizer doubles every `\|` in agent text. | A malicious agent fakes a WoW chat link (`\|H...\|h`), a texture, or a color that imitates a system message. |
| S11 | **Freshness:** the bridge accepts a frame only if its time is at most 5 minutes old and at most 1 minute in the future. | An old screenshot of a strip is replayed. The MAC is still valid, so S2 does not stop it. |
| S12 | **Size bounds:** for every input, a slot body is at most 1 MB, and each reply record in it is at most 32 KB. | A malicious agent writes a huge reply, and the bridge writes 200 huge slot files. |
| S13 | **ID charset:** the id validator accepts only `[a-z0-9_-]`, 1 to 32 characters. | A chat id like `../../x` reaches a file path or a state key. |
| S14 | **Rate limit and queue cap:** the limiter never admits more than N messages in any window. A chat queue never holds more than 20 messages. | Strip spam fills memory or starts many runs. |
| S15 | **Honest popup:** the popup text contains the full raw command, or its start and end with a cut mark. It contains no raw control, bidi, or zero-width characters. | A malicious agent asks for permission with a false label, or hides the dangerous part of a command. |

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
7. **Done: Quint model** of the transport. Next: the bridge state machine, queue, and publisher, which follow the model.
8. **Threat model in code:** `allowed_roots`, the policy, and the MAC check.
9. **ACP backend.** Test with one agent first.
10. **`note` signal and pings:** the hook CLI and the socket.
11. **`native-*` and `command` backends.**
12. **Windows and macOS capture backends.** Mark them experimental until a tester on each OS makes sure that they work.

Steps 1 to 5 prove the channels. After those, the rest is normal Rust work.

## 16. Development environment

- `dev gnomish-relay` opens tmux with nvim, the agent, and a terminal in this folder.
- Link `addon/GnomishRelay` into `_classic_beta_/Interface/AddOns`. Then an edit plus `/reload` loads the new code, with no copy step.
- Run the bridge in the bottom-right pane.
- Aeneas and Charon are built in `~/verif`. `proofs/TOOLS` pins their commits, and CI builds the same commits with Nix.

## 17. Open questions

1. Does X11 capture of the WoW window work under XWayland? (Only for the fallback.)
2. Can font files replace the `.wav` signals?
3. How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
4. What are the ACP mode IDs of `claude-agent-acp` and `codex-acp`?
5. Does Gemini CLI have hooks for pings?
6. How large is the hitch at a higher window size? (The "Screen captured" hide works.)
7. Does the Aeneas standard library model cover the `Vec` and slice functions that the core needs?
