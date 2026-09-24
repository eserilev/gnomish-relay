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
The input to the bridge comes from pixels on the screen.
So the bridge treats every decoded strip as untrusted input.

### 6.1 Attackers

| Attacker | How | Defense |
|---|---|---|
| Another window over the game (browser, video, overlay) | Shows a fake strip | Capture reads the WoW window content, not a screen region. Each strip carries a MAC. |
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

### 6.3 Strip authentication

The setup step makes a random 32-byte key.
It writes the key into the addon (as a file-local value) and into the bridge config.
Each strip ends with a truncated HMAC-SHA256 tag (8 bytes) of the header and payload.
The bridge drops each strip with a wrong tag, and logs it.

Open point: the cost of HMAC-SHA256 in WoW Lua (with the `bit` library). The spike measures it.

### 6.4 Known leaks

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
[0x6E 0x52] [version] [frame id hi, lo] [len hi, lo] [payload: len bytes] [fletcher16 s1, s2] [mac: 8 bytes]
```

- Magic bytes `0x6E 0x52` differ from `wow-claude` (`0xC7 0x1A`). A wrong magic means "not a strip".
- `version` is the protocol version, 1 for this spec.
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
| `h` | Hello only. It announces the token and the addon version. It has no prompt. |
| `d` | The chat is deleted. The bridge drops its transcript and session. The addon keeps the id in `db.forget` and sends it with each hello until the bridge acknowledges it. |
| `agent=<name>` | The agent for a new chat. |
| `perm=<request>:<option>` | The answer to a permission request (9.3). |

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
- `replies` holds the last 30 records. Each `text` is at most 32 KB. The bridge cuts longer text and adds a note with the full length.
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

### 7.6 Restore after a saved-data wipe

The beta client sometimes wipes addon saved data. The addon then makes a new token.
When the bridge sees an unknown token, it adds a `restore` bundle to the next 3 publishes, addressed to that token.
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

**Correctness theorems:**

| # | Theorem |
|---|---|
| C1 | **Cell round trip:** bytes → 3-bit cells → bytes gives the same bytes. |
| C2 | **Frame round trip:** for every payload of at most 3200 bytes, `decode_frame(encode_frame(m)) = m`. |
| C3 | **Record round trip:** for records with no RS in any field and no US before `text`, `parse(serialize(r)) = r`. |

**Order:** C1 first, because it is the smallest. Then S1, S3, S8, and S5, because those inputs come from outside. Then the rest.

**Proof hygiene:**

- Every theorem ends with `#print axioms`. CI fails if the list contains `sorryAx` or anything other than `propext`, `Classical.choice`, and `Quot.sound`.
- The Aeneas standard library has 4 `sorry` placeholders (in `Slice` and `StringIter`, checked 2026-09-23). The axiom check catches every proof that depends on them.
- `native_decide` is not allowed. It adds an extra axiom and trusts compiled code.

**What the proofs do not cover:**

- **The crypto.** HMAC-SHA256 comes from the `hmac` and `sha2` crates. The proofs treat `verify_tag` as opaque. The bridge compares tags in constant time.
- **The file system.** S5 is about path text. A symbolic link inside a root can still point outside. The bridge resolves links with `canonicalize` and runs the S5 check again on the result.
- **What the agent does on the host.** A malicious or confused agent can do damage inside its folder, within its permission level. Only the permission level (S6), the folder policy (S5), and the agent sandbox limit that. No proof in this project can make an agent safe.
- **Hostile addons in the same Lua environment.** See 6.1.

**Design rule from the first proof:** do not cast `bool` to an integer in the core. The Bool casts made the bit proof hard. Integer bit operations (`(v >> 2) & 1`) are easier to prove.

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

## 15. Build order

0. **Start WoW once.** This makes `Interface/` and `WTF/Account/`.
1. **Done: `Screenshot()` spike.** A test addon draws a strip and calls `Screenshot()` from an event, with no key press. If a PNG appears, WoW writes the strip image itself, and the capture layer (section 11) becomes a fallback. The addon hides the "Screen captured" text through the `ActionStatus` frame.
2. **Skipped: capture spike.** Step 1 passed. Capture the top-left 800×192 pixels of the WoW window content 4 times per second. Save one frame as PNG. Test the portal and X11 paths.
3. **Done: Wine rules spike.** Test the five rules in 7.2 under Wine: the `ctl` self-test, a fresh read of a load-on-demand file, and "a new file is not found". Results in `spikes/README.md`. The HMAC-SHA256 cost in WoW Lua is not measured yet.
4. **`protocol` crate with Aeneas.** Frame, cells, records, slot body, escapes. Set up Charon, Aeneas, and the Lean project. Prove theorems 1 and 2 first.
5. **Slot writer.** Publish a fixed reply. Make sure that it shows in the game.
6. **Addon port** with the stub harness and the differential tests.
7. **Quint model** of the transport. Then the bridge state machine, queue, and publisher.
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
- Aeneas and Charon source trees are in `~/verif`. They are not built yet.

## 17. Open questions

1. Does X11 capture of the WoW window work under XWayland? (Only for the fallback.)
2. Can font files replace the `.wav` signals?
3. How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
4. What are the ACP mode IDs of `claude-agent-acp` and `codex-acp`?
5. Does Gemini CLI have hooks for pings?
6. How large is the hitch at a higher window size? (The "Screen captured" hide works.)
7. Does the Aeneas standard library model cover the `Vec` and slice functions that the core needs?
