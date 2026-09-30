# Gnomish Relay: Specification

Status: draft 4, 2026-09-29. Section 15 shows what is built. "Not planned now" marks a part with no owner and no date. Its text stays as a design note.
Draft 2 applies a review against the `wow-claude` source code.
Draft 3 applies the spike results in `spikes/README.md`: the strip goes out through `Screenshot()`, and `.wav` signals do not work.
Draft 4 makes the spec match the code where they disagreed, and marks the parts that are not built.

## 1. Summary

Gnomish Relay connects AI coding agents to World of Warcraft: Forever.
You send a task from a chat window in the game. The agent does the work on your computer.
The reply comes back into the game with a whisper sound.

Planned (section 10): Gnomish Relay also shows notifications from agent sessions that you run in a normal terminal.
When a terminal session ends a turn or needs input, a message appears in the game.

Gnomish Relay has two parts:

- **The addon**: a WoW addon, written in Lua. It shows the chat window.
- **The bridge**: a program on your computer, written in Rust. It moves messages between the game and the agents.

## 2. Goals

1. Drive an AI coding agent from inside WoW, with no alt-tab and no `/reload` per message.
2. Get a notification in the game when a terminal agent session finishes long work or waits for you.
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

- [chelinho139/wow-claude](https://github.com/chelinho139/wow-claude) (MIT), now named `chelinho139/wow-ai`.
  It is a Windows-only Node bridge for Claude Code. Gnomish Relay starts from its Lua addon and its transport design.
- [0xInuarashi/wow-forever-codex](https://github.com/0xinuarashi/wow-forever-codex).
  It measured the file-load rules of the Forever client and invented the pixel-out channel.

The README credits both projects. The addon code from `wow-claude` keeps its MIT license notice, in `addon/GnomishRelay/LICENSE-wow-claude.txt`. `wow-forever-codex` has no license file, so Gnomish Relay takes no code from it.
References in this spec to `wow-claude` files use the path in that repo, for example `bridge/protocol.js`.

## 5. Terms

| Term | Meaning |
|---|---|
| Strip | The block of colored cells that the addon draws to send data out. |
| Record | One message inside a strip, or one reply inside a slot file. |
| Slot | One of the 1000 load-on-demand reply addons (7.3). |
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
| Another window over the game (browser, video, overlay) | Shows a fake strip | WoW takes the screenshot itself, so other windows are not in it. Each strip carries a MAC. |
| A local program | Drops a crafted PNG into the Screenshots folder, or replaces a slot folder with a symbolic link | MAC (6.3) and freshness check (S11). Image size limit before decoding. No writes or deletes through symbolic links (6.2). |
| A local program | Writes fake notifications into the spool folder (10.2) | The folder has mode 0700, and game runs cannot reach it. Size limits, exact fields, and one notice for each session (S41). The text goes through the same escapes as agent text (S40). A notification never starts a run. |
| A malicious or prompt-injected agent | Writes a reply that injects Lua or fakes WoW chat links. Asks for permission with a false label. Writes a huge reply. | Lua escape and UI escape (S8 to S10). Honest permission popup (6.4). Size limits (S12). |
| An old screenshot | A strip is replayed from an old file, for example after `state.json` is lost | Freshness check (S11). |
| Another addon or a WeakAura | Runs Lua in the same environment as our addon. It can call our handlers, fill our input box, click our buttons, read and change `GnomishRelayDB`, and replace a slot body during a load. | Signed state (6.6.1) stops changes to stored messages. A call to our handlers gets no more than a message that the user typed: the ceiling, the classifier, the sandbox, and desktop approvals (6.6.2 to 6.6.4, 9.3) bound every game message, whoever sent it. |
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
17. The proved resolver (S5) splits paths only at `/`. On Windows, the bridge first turns each `\` of a game folder into `/`, so each `..` counts. It refuses a game folder with `:`, which starts a drive or names a stream. Roots lose the `\\?\` prefix of `canonicalize`.
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
- When the popup offers "Always allow" (6.6.5), one more line names the exact rule and its folder. The bridge makes the line, and the popup never cuts it.
- A misclick must not allow. A new popup plays the ready-check sound, and its buttons take no click for 1 second: the player can be in the middle of a click in the game. **Reject** and **Always reject** sit at the left, and the allow buttons at the right, with a wide gap between.
- When more requests wait, the popup says "1 of 3" at the top right. It shows the oldest request first.
- The popup has the dark dialog border of the game. Its height follows the text, up to 600 pixels, so a long command never runs over the buttons. The text shows in the shipped mono font (13.2).

Theorem S15 covers these rules.

### 6.5 Known leaks

- Reply text sits in a global table after a slot loads. Any addon can read it.
- `GnomishRelayDB` is a global table. Any addon can read the chats in it.
- The strip is signed, not encrypted. The prompt is in the pixels of each strip screenshot until the bridge deletes it. If the bridge does not run, these files stay. A cloud sync of the Screenshots folder (for example OneDrive on Windows) copies them.
- An addon that loads before ours, for example one named `!Evil`, can replace global functions such as `string.char`, `tonumber`, or `bit.band` before `Key.lua` and `Sha256.lua` run. It can then read the strip key. Lua in WoW gives an addon no way to stop this. Layers 2 to 4 of 6.6 assume that any game message can come from another addon, so the key is a check against programs outside the game, not against other addons.
- Code in the sandbox can still send data to the allowed API host, for example with an upload under another account key. A proxy that ends TLS and pins the account closes this. It is not in v1.
- A command of a game run can send data to each host of the proxy list (6.6.4), for example a push to `github.com` with a token of its own. The list limits where a command connects, not what it sends.

### 6.6 Four layers of defense

Each layer covers a hole in the layer before it. No layer depends on a model that judges another model.

| Layer | Question | Where |
|---|---|---|
| 1. Signed state | Did anything change a message after our code signed it? | Addon |
| 2. Game ceiling | What can a message from the game do at most? | Bridge config |
| 3. Action classifier | Does this tool call run, ask in the game, ask on the desktop, or never run? | Bridge, proved in `protocol` |
| 4. Sandbox | What can happen when layers 1 to 3 fail? | Operating system |

The trust of "always allow" (6.6.5) rests on layers 2 to 4. It never rests on layer 1.

#### 6.6.1 Signed state

**Signed state.** Another addon can change `GnomishRelayDB` without a call to our code. So the addon signs messages from private state:

- The addon keeps the text of each open message in its private table (`ns`), not only in `GnomishRelayDB`.
- When the user sends a message, the addon signs it at once. It stores the signed frame and its time in `GnomishRelayDB`, next to the text.
- After a `/reload`, the addon sends only frames with a valid tag. It never signs text that it reads back from `GnomishRelayDB`.
- A stored frame or an outbox frame older than 270 seconds is too old for the bridge (S11 allows 300). The message then ends with "Not sent. Send it again.", and the user decides. While the bridge is offline (7.4), the text is "Not sent: the bridge is not running. On the desktop, run gnomish-relay restart."
- An outbox entry (7.5) is the same signed frame. The bridge checks the tag, the time, and the replay store for it (S2, S11, S7), as for a strip.
- A permission answer carries a hash of the exact text that the popup showed: `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256, in hex. The bridge refuses an answer whose hash does not match its own text of the request.

No WoW mechanism lets an addon prove that the user typed a message. For example, another addon can fill the chat box with `/ai …` and wait for the user to press Enter. So layers 2 to 4 assume that any game message can come from another addon.

#### 6.6.2 Game ceiling

Every message from the game (a strip or the reload outbox) runs under one ceiling from `config.toml`. No message from the game can raise it. Only a click on the desktop changes the config (9.3, "Raise the level"), and no addon can make that click (S6).

| Setting | Default |
|---|---|
| Write | The chat folder only (the `auto-edit` level of 9.3) |
| Read | `allowed_roots` |
| Commands | The allow table of the config, and the "Always allow" rules of the folder (6.6.5). All others ask. |
| Network | For commands, only through the bridge proxy, to the allowed hosts (6.6.4). The agent process reaches public hosts through a proxy of its own, and none of this computer but `local_ports` (6.6.4, "The agent process behind the proxy"). Each network tool of the agent asks on the desktop (6.6.3). |

- The `full-auto` level (6.2 rule 5) skips the questions of the game only. `deny` and `desktop` answers of the classifier still apply, and so does the sandbox.
- The allow table of the config (12) covers commands. A command that it covers runs with no question at `auto-edit` and `full-auto`. It never covers a `deny`, `desktop`, or "never always" command (S17).
- The game never answers a permission request of a terminal session, and never sends a task to one. Terminal sessions only send notifications (section 10). The bridge does not run them, so this spec gives them no rules.
- The bridge shows a desktop notice for each game message: "New task from WoW: <first line>". The config can turn this off.

#### 6.6.3 Action classifier

The bridge classifies each tool call before it runs. It works on the structured tool input, never on the prompt text. The same input always gives the same answer.

There are four answers, in this order from strict to open:

| Answer | Meaning |
|---|---|
| `deny` | Never runs. Only for the files that guard the relay itself. |
| `desktop` | The user approves on the desktop. The game shows a notice with no buttons. No addon can click a desktop prompt. |
| `ask` | The user approves in the game popup (6.4). |
| `allow` | Runs with no question. |

**How tool calls reach it.** The classifier sees only the tool calls that a backend sends to the bridge. So its coverage is a property of each backend. `crates/bridge/src/gate.rs` is the one place that turns a verdict into an action (9.3), for every backend:

- **Claude (`kind = "claude"`): every tool call.** The bridge registers a `PreToolUse` hook in the `initialize` control request of `claude -p` (9.2). Claude Code then sends a `hook_callback` control request before each tool call, also the reads and the calls that the permission mode lets run with no question. The hook answers `allow` or `deny` itself, after the gate. It never answers `ask`, because that hands the call to the permission rules of Claude Code, which the settings of the user can loosen. A `hook_callback` that the bridge cannot read gets `deny`.
  - Tools: `Read` is a read of `file_path`. `Write`, `Edit`, and `MultiEdit` are writes of `file_path`, and `NotebookEdit` of `notebook_path`. `Glob`, `Grep`, and `LS` read their `path`, else the chat folder; a `Glob` pattern that starts with `/`, `\`, or `~`, or holds `..` or `:`, is unknown. `Bash` is its `command`, in the chat folder. A relative path is relative to the chat folder.
  - Tools of the session only run with no question: `ToolSearch`, `TodoWrite`, `EnterPlanMode`, `ExitPlanMode`, and `AskUserQuestion`. They change nothing outside the session, and a question for each would make Claude unusable.
  - Every other tool is unknown: `WebFetch`, `WebSearch`, `Task` and other subagents, MCP tools (`mcp__*`), and any new tool.
  - Checked live on Claude Code 2.1.282: the hook fires for a `Read` in `acceptEdits` mode. It also fires when `--settings` holds `disableAllHooks: true` and an allow rule for `Read`, so neither the settings of the user nor an allow rule skips it. A `hook_callback` answer that Claude Code cannot read lets the tool run, so the bridge only ever sends a well-formed answer.
  - A second line: the bridge tracks the id of each tool call that the hook answered. A `tool_result` with no error for any other id means that a tool ran with no check. The run then stops at once with "A tool ran with no check by the bridge, so the run stopped.". That call already ran. A result with an error does not count, because a call with bad input fails before the hook.
  - `can_use_tool` still works. A call that the hook allowed gets `allow`. Any other call goes through the gate.
  - The Bash tool of Claude keeps its folder between calls. A relative redirect after a `cd` in an earlier call resolves from that folder, and the classifier resolves it from the chat folder. The sandbox (6.6.4) is the wall for this case.
- **Codex (`kind = "codex"`): every command and every file change.** The thread runs with `approvalPolicy: "untrusted"` at every level. In codex-cli 0.157.0 that asks before every command that no `allow` rule of Codex covers, and before every patch, in both sandboxes (`core/src/exec_policy.rs`, `core/src/safety.rs`). The bridge classifies the script inside `<shell> -lc '<script>'`. What still runs with no request, and so without the classifier:
  - A command that an `allow` rule of Codex covers: `/etc/codex/rules`, `$CODEX_HOME/rules` (for example `default.rules` from an "always allow" in a terminal), and the `.codex/rules` of a trusted project. Such a command also runs outside the sandbox.
  - A request that a `PermissionRequest` hook of Codex answers, and an MCP server with `approval_mode = "approve"`.
  - The retry outside the sandbox of a command that the bridge allowed.
  - Input to a running command (`write_stdin`), `view_image`, MCP tools with `readOnlyHint`, the MCP resource tools, and the tools of the session (plan, tool search, sleep). Web search is off (`config.web_search = "disabled"`).
  - MCP tool approvals come as `mcpServer/elicitation/request`. The bridge declines each one.
  The sandbox of Codex (6.6.4) bounds all of these. The bridge cannot give Codex an empty `CODEX_HOME`, because the login lives there.
- **Other ACP agents: only the calls that they ask about.** The bridge classifies each `session/request_permission`: the `kind` of the tool call says what its paths are (`read` and `search` read, `edit`, `delete`, and `move` write), from its `locations` and the `file_path`, `path`, or `notebook_path` of its `rawInput`. `execute` is the `command` of `rawInput`. Any other call is unknown. The agent decides what it asks, and the calls that it does not ask about already ran. So for these agents the answer is at most `ask` at every level, even `full-auto`: no allow table and no rule from the game gives `allow`.
- A terminal session of Claude uses the same hook through `gnomish-relay-hook pretool`. This subcommand ignores `GNOMISH_RELAY_JOB`, and it fails closed: if the bridge does not answer, the answer is `deny`.

**Desktop approval.** The bridge runs in the background with no window. So it shows a dialog of the OS with Approve and Deny, and the command line is the fallback:

- The bridge writes each open request to `approvals/<id>.json` in the data folder (12), with mode 0600. The id is 12 random hex digits. The file holds the agent, the folder, the time, and the popup text (S15).
- `gnomish-relay approve` lists the open requests. `gnomish-relay approve <id>` allows one, and `gnomish-relay deny <id>` refuses one. Each writes an answer file next to the request, with `create_new`, so it never follows a link. A request has at most one answer.
- The bridge checks for the answer every 100 ms, up to `permission_timeout_minutes`. No answer refuses the call. The bridge then deletes the files. At start it deletes the files of an old bridge.
- **The game gets a notice, not a popup** (decided with a UX advisor on 2026-09-26). The game sends no request for a desktop call, and has no Deny for it. The desktop dialog is the only prompt, so the player never sees two prompts for one call.
  - The bridge writes one progress line of its own for the last desktop request of the run: `Desktop: <state> <id> <how>`, and ` raise <level>` for a raise (9.3). `<state>` is `wait`, `approved`, `denied`, or `none` (no answer). `<id>` is the 12 hex digits of the request. `<how>` is `dialog`, or `command` when `desktop.rs` finds no dialog tool.
  - The line comes right after the level line (9.3, "The level in the game"), so S9 and S20 do not change, and `Activity` keeps at most 5 lines. `Activity::step` puts "agent: " in front of an agent line that starts with "Desktop:", as for "Level:". The id comes from the bridge, so no agent text is in the line.
  - The addon takes the line only at its place, and only with a known state, a 12-digit id, and a known `<how>`. The Activity row then shows "Approve on desktop", "Approved on desktop", "Denied on desktop", or "No answer on desktop".
  - At a new `wait`, the game prints one whisper line with the whisper sound: `[Claude] whispers: [chat] Approve on your desktop.`, or `Run: gnomish-relay approve <id>` with `command`. The addon keeps the ids of the last 16 requests that got a line in its saved variables, so a `/reload` does not print it again.
  - While a request waits, the addon loads a slot every 5 seconds, at most 24 times for each request (7.3). Then it goes back to the schedule.

**The dialog** (decided with an advisor on 2026-09-26). In the first test in the game, the user saw only the game popup, and did not know about the command. `crates/bridge/src/dialog.rs` shows the dialog with the tools that the user already has. The bridge installs nothing.

| OS | Tool | Approve when |
|---|---|---|
| Linux | `notify-send -a "Gnomish Relay" -u critical -A approve=Approve -A deny=Deny`, only when the notice server lists the `actions` capability (`gdbus call ... GetCapabilities`). Else `zenity --question --no-markup --default-cancel`, only with `DISPLAY` or `WAYLAND_DISPLAY`. Else no dialog. | notify-send prints `approve`; zenity exits with 0 |
| macOS | `osascript` with `display dialog`, buttons Deny and Approve, `default button "Deny"`, `cancel button "Deny"` | the output holds `button returned:Approve` and not `gave up:true` |
| Windows | PowerShell `MessageBox` with Yes and No, `Button2` (No) as the default, `DefaultDesktopOnly` so that it is on top. The text starts with "Yes = Approve, No = Deny." | the output is `Yes` |

- The text of the dialog is "An agent from the game asks to:", the popup text of S15 (the full raw command or path, then "the agent says"), and then "Agent: <name>. Folder: <folder>. Request <id>.". It never shows only text that the agent chose.
- The text goes in an argument or an environment variable, never into a script. A notice server shows the body as markup, so the bridge escapes `&`, `<`, and `>` for notify-send. Else `<b>` or an S15 escape such as `<U+202E>` hides text. zenity gets `--no-markup`. The markup escape is bridge code, not proved, so a named test covers it.
- Deny is the default button everywhere, so Enter never approves. A closed, dismissed, or timed-out dialog, and any output that is not the Approve answer, is Deny.
- The dialog runs in its own thread. Every 100 ms it checks whether its request still waits. When the request has an answer from the command line, or the gate closed it (the timeout, Stop, or a new message, 9.3), the thread stops the dialog. It sends SIGTERM through `kill` first, because notify-send then closes its notice, and a kill after 0.5 s.
- The answer of the dialog goes through the same `create_new` answer file as the command line. So the first answer wins. A dialog answer after the request closed can leave an orphan answer file. Nothing reads it, and the next start deletes it.
- zenity and osascript also give up by themselves after one hour, in case the bridge stops first.
- Why notify-send first: it needs only the session bus, which the systemd service of the bridge has. GNOME shows a critical notice over a full-screen game and keeps it until a click. With no `actions` capability, a notice has no Approve button and closes as a Deny, so the bridge checks the capability for each dialog.
- Why no `kdialog`: it shows the text as rich text when the text looks like HTML, with no option to turn that off. KDE Plasma's notice server has `actions`, so notify-send covers KDE.
- On Windows, the bridge starts PowerShell with `CREATE_NO_WINDOW`, so no console window flashes.
- With no dialog tool, the bridge shows a plain notice with "Run: gnomish-relay approve <id>", with `notify-send`, `osascript`, or a PowerShell toast. With no such tool, the log line is the notice.
- The bridge writes a log line for each request, the tool of its dialog, and the answer of the dialog.

**Input.** The bridge builds the input in `crates/bridge/src/action_input.rs`:

- A file call carries its read paths and its write paths. A shell command carries its raw bytes and its working folder. Every other tool call is "unknown".
- Each path is resolved with `canonicalize` at check time. A new file resolves through its folder. The path then has the form of `resolve_folder` (S5): it starts with `/`, it has no empty part, no `.` and no `..`, and no trailing `/`. On Windows the drive is the first part, for example `/C:/Users/x`. A path in any other form is `desktop`.
- The policy holds `allowed_roots`, the chat folder, the `deny` folders (the config folder and the data folder of the bridge, 12), the two lists of `desktop` patterns, and the allow table of the config.
- The rules from the game are "always allow" rules (6.6.5). Each rule is the first words of a command: `cargo test` covers `cargo test -q`. An empty rule covers nothing.
- A path or a command longer than 1 MiB is `desktop`.

**Case.** macOS and Windows compare paths without case, so `~/.SSH` is `~/.ssh` there. The classifier compares the `deny` folders and the `desktop` patterns without ASCII case on every OS. This is stricter, never looser. The checks for `allowed_roots` and the chat folder compare with case, which is also stricter.

**Rules for paths:**

- **Unknown tools are `desktop`.** The classifier knows file reads, file writes, and shell commands. Every other tool is `desktop`: web fetch, web search, MCP tools, and subagents.
- **Inside:** a path is inside a folder when the parts of the folder start the parts of the path (S5). A write outside the chat folder is `desktop`. A read outside `allowed_roots` is `desktop`.
- **`deny` paths:** the strip key, `timeways.key`, `config.toml`, and everything else in the config folder of the bridge (12). An approved access would let the agent sign fake strips or raise its own ceiling. The bridge writes `config.toml` itself after a raise on the desktop (9.3), never through the classifier.
- **`deny` paths in the data folder** (12, and 9.7 decision 12): `state.json`, `approvals/`, `timeways/`, `rules.json` (6.6.5), `bridge.lock`, `bridge.pid`, `bridge.log`, and everything else there. An approved access would let the agent clear the replay store, answer its own desktop request, or change the story state. The bridge writes these files itself, never through the classifier.
- **`desktop` patterns** are whole parts that match anywhere in a path, for example `.git/hooks`. A last `*` in a part matches the rest of a part, so `.env.*` matches `.env.local`.
- **`desktop` paths, for reads and writes:** `.ssh`, `.aws`, `.gnupg`, `.env` files, other credential files (`.netrc`, `.git-credentials`, `.config/gh`, `.docker/config.json`, `.kube`), the tokens of package tools (`.cargo/credentials.toml`, `.npmrc`, `.yarnrc.yml`, `.pypirc`, `.config/pip`, `.gem/credentials`), keychains, and browser profiles. `action_input.rs` has the full list. The proxy of the sandbox (6.6.4) reaches the hosts of the package tools, so a command must not read their tokens. A hidden `.npmrc` or `.config/pip` also hides the settings in it, for example a registry of a project.
- **`desktop` paths, for writes:** files that code on the host runs later, outside the sandbox. They are `.claude/`, `.git/hooks/`, `.git/config`, `.envrc`, `.vscode/`, and `.github/workflows/`.

**Rules for commands:**

- **Grammar:** `crates/protocol/src/shell.rs` splits a command into simple commands, with a strict part of POSIX `sh`: words with single quotes, double quotes, and `\`; the operators `;`, `&&`, `||`, `|`, `|&`, `&`, and a newline; subshells in `(` `)`; and the redirects `>`, `>>`, `>|`, `<`, `<>`, `&>`, `&>>`, and a descriptor before them, such as `2>`. `2>&1` copies a descriptor and names no file.
- **Does not parse:** an open quote, a trailing `\`, an open `(`, a redirect with no file, `$` outside single quotes (every expansion), a backtick outside single quotes, a heredoc or here-string (`<<`), process substitution (`<(`, `>(`), a brace other than `{}`, a comment, a reserved word such as `if` or `then` as the command name, and a glob in the command name. A command that does not parse is `desktop`. The popup shows its raw text (6.4).
- **Command substitution:** a command with `$(` or a backtick outside single quotes is `desktop`, also inside double quotes. The check follows the quote state of the splitter. Inside single quotes both are plain text, so `git commit -m 'fix `x`'` is not a substitution, but `git commit -m "fix `x`"` is. An escaped one, such as `\$(`, still counts: that is stricter than the shell. An open single quote does not parse.
- **Names:** a name matches after its folder, its ASCII case, and a last `.exe` come off, so `/usr/bin/SUDO.exe` is `sudo`. Most lists match any word of a simple command, so a wrapper such as `timeout 5 sudo x` cannot hide a name.
- **`desktop` commands:** `eval`, `sudo` and the other commands that change the user (`sudoedit`, `doas`, `su`, `pkexec`, `run0`, `gsudo`, `runas`), a shell after a `|` (every simple command after the first `|` counts), `cmd.exe`, and PowerShell. PowerShell stays `desktop` until the classifier has a PowerShell parser.
- **Commands that run other commands** always ask: `xargs`, `env`, `sh`, `bash`, `python`, `node`, `perl`, and the other interpreters, wrappers such as `timeout`, `nohup`, and `strace`, schedulers such as `crontab`, `find` with `-exec`, `-ok`, or `-delete`, `git` with `-c`, `--upload-pack`, `--exec`, or `config`, a command name such as `.`, `source`, `command`, `export`, or `trap`, and a first word with `=`, such as `LD_PRELOAD=x cmd`. `command_rules.rs` has the full lists.
- **Network tools** (`curl`, `wget`, `nc`, `ssh`, `scp`, `rsync`, and more) always ask.
- **Redirects:** a redirect target resolves from the working folder with `resolve_folder`, and then follows the rules for paths: `>` is a write, `<` is a read. `/dev/null` is always allowed. A target that starts with `~` or holds a glob is `desktop`, because the shell expands it and the classifier cannot. A command with a file redirect and a `cd`, `pushd`, or `popd` is `desktop`, because the target then resolves from another folder.
- **Never "always":** `rm -r`, `chmod`, `chown`, `chgrp`, a forced `git push` (`-f`, `--force`, `+main`), `git reset --hard`, the commands that run other commands, network tools, and every `desktop` answer. They get "Allow once" at most. The allow table of the config cannot allow them either.
- **Everything else** asks, unless the allow table of the config or a rule from the game covers every simple command of it. Then it is `allow`.
- A prompt keyword (for example `.ssh` or `token`) is only a signal. It moves the whole run to `ask`. It is never the wall.

**The answer** of a tool call is the strictest answer of its parts: each path, each redirect target, and each simple command. The ceiling of a tool call is its answer when a game rule covers every command. No rule list gets more (S17).

**Limits.** The proofs work on the paths of file tools. Paths inside the arguments of a command are out of scope: a command asks by default, the popup shows the raw command (S15), and the sandbox is the wall for commands (6.6.4). The proofs work on paths that the bridge has already resolved. A symbolic link made after the check is a race that the proofs do not cover.

The classifier core is pure and lives in `protocol`. Theorems S16, S17, S27, and S28 cover it.

#### 6.6.4 Sandbox

The bridge runs every command of an agent run from the game inside a sandbox. The user does nothing.

| Rule | Value |
|---|---|
| Write | The chat folder, and a private temp folder of the run |
| Read | The system, except the `deny` paths and both lists of `desktop` paths of 6.6.3, which are hidden |
| Network | Only through the bridge proxy, to the allowed hosts (see "Network: the proxy"). The agent process itself is outside this sandbox, behind a wall of its own (see "Where the wall is" and "The agent process behind the proxy"). |
| Children | Every child process, for example `cargo test`, is inside the same sandbox |

The sandbox closes the hole that a classifier cannot close: an allowed command such as `cargo test` runs code that the agent can edit first.
It covers shell commands. The file tools of Claude run outside it, so the classifier (6.6.3) guards them.

**The policy (S31).** `sandbox_policy` in `crates/protocol/src/sandbox.rs` builds it from the chat folder, the temp folder, the `deny` folders (the config folder and the data folder, 12), and both lists of `desktop` patterns. A path is hidden with the predicate of the classifier: inside a `deny` folder, or a run of its parts matches a pattern, with no regard to ASCII case. The writable paths are the chat folder and the temp folder, each clean in the form of S5 and not hidden. If the chat folder lies inside a hidden path, the policy leaves it out, and the bridge refuses the run: "The chat folder is inside a folder that the sandbox hides (the config folder, the data folder, or a credential folder), so the agent cannot work there." S31 proves the policy (14.1).

**Where the wall is** (decided with an advisor on 2026-09-26). The sandbox holds the commands, not the agent process:

- The agent writes its own state all the time: sessions, logins, and settings in `~/.claude`, `~/.claude.json`, and `~/.codex`. With these folders read-only, the agents stop working. With them writable, a command can plant a hook, an MCP server, or an allow rule that runs later with no sandbox, in a terminal session of the user. So the agent process stays outside, and only its commands go in.
- The bridge also resumes Claude sessions from `~/.claude/projects` (9.6), which needs the real folder.
- The exception is a `command` agent (9.2). The bridge sees none of its tool calls, so the whole harness goes into the sandbox. Its writes into the home folder go to a copy-on-write view that goes away with the run (see "A harness with only a command line").
- Nested sandboxes fail on macOS inside a real wall. Checked on the macOS runner of CI on 2026-09-27: inside a profile that allows everything, `sandbox-exec` starts again, but inside a profile that denies the network, it fails with "sandbox_apply: Operation not permitted" (the test `seatbelt_cannot_start_inside_a_seatbelt_wall`). Claude and Codex use Seatbelt for their own commands there.

**Claude (`kind = "claude"`).** Claude Code runs each command of its Bash tool through `CLAUDE_CODE_SHELL_PREFIX`. The bridge sets it to `<gnomish-relay> --sandbox-run`: this program, at its absolute path. Claude Code then runs `bash -c -l "'<gnomish-relay>' --sandbox-run '<command>'"`, and the bridge program starts the command inside the sandbox of the run. Checked on Claude Code 2.1.283 in the code of the program: it quotes the part before the last " -" as the program and adds the command as one quoted word. The same prefix wraps command hooks and MCP servers.

- At the start of each run, the bridge makes the private temp folder (`gnomish-relay-run-<random>`, mode 0700, under the temp folder of the OS) and writes the walls of the run to `<data>/sandbox/<name>.json`: the tool, the writable paths, and the hidden paths that exist. `GNOMISH_RELAY_SANDBOX` names the file, and `TMPDIR` is the temp folder. Both go away at the end of the run.
- The wrapper never runs a command outside the sandbox. With no walls, or with a tool that does not start, the command fails with exit status 126.
- The command gets only the variables of the allowlist of 6.2 rule 12, `TMPDIR`, and the variables of the proxy and of the caches ("Network: the proxy"). The `env` list of the entry, for example `ANTHROPIC_API_KEY`, stays with the agent.
- The wrapper leaves a mark in the temp folder. If a Bash call ran and the mark is missing, Claude Code ignored the prefix, and the run stops at once with "A command ran outside the sandbox, so the run stopped.".
- The bridge refuses a run when the bridge program is inside the chat folder, because a command could change it: "The bridge program <path> is inside the chat folder, so a command could change it. Install it somewhere else, for example ~/.local/bin."
- The flags of a game run: `--setting-sources ""`, so the settings of the user and of the project do not apply; `--strict-mcp-config`, so no MCP server starts; and `--settings` with `sandbox.enabled: false` and `env.CLAUDE_CODE_SHELL_PREFIX`. A project from the web can hold a `.claude/settings.json` with hooks, or with an `env` that clears the prefix, so no project setting applies. The model and the other settings of the user do not apply either: the `command` of the entry can add `--model`.
- The own sandbox of Claude Code stays off. Checked on 2.1.283: the keys are `sandbox.enabled`, `sandbox.failIfUnavailable`, and `sandbox.allowUnsandboxedCommands`. With `failIfUnavailable`, Claude Code refuses to start on a Linux with no `socat` ("sandbox required but unavailable: ... socat not installed"), and on macOS its Seatbelt and the Seatbelt of the bridge would nest, which is reported to fail. The sandbox of the bridge covers the same commands.

**Linux: `bwrap`.** `--ro-bind / /`, `--dev /dev`, `--proc /proc`, an empty `--tmpfs` on `/tmp`, `/var/tmp`, and `/run` (they hold the sockets of the ssh agent and the desktop), a writable `--bind` of the chat folder and the temp folder, then a `--tmpfs` over each hidden folder and a read-only empty file over each hidden file, then `--remount-ro` of each of these. Then `--unshare-all` (no network but a loopback of its own, own process ids), `--die-with-parent`, and `--new-session`. These walls belong to the run, not to one command: see "One sandbox for each run". Between the writable binds and the hidden paths come the copy-on-write views of `~/.cargo` and `~/.rustup` (see "The downloads of cargo and rustup"). A folder that holds a writable path keeps its place: a chat folder under `/tmp` still works.

**One sandbox for each run** (asked for by the user on 2026-09-27, worked out with an advisor, built on Linux). A dev server must work across Bash calls: `npm run dev &` in one call, and `curl localhost:3000` in a later call. So on Linux the commands of a run share one sandbox:

- At the start of the run, before the agent starts, the bridge starts a holder in the walls above: `bwrap <walls> --unshare-all --die-with-parent --new-session -- <gnomish-relay> --sandbox-hold <launch socket> <proxy socket> <local ports>`. The holder relays the proxy port and the local ports (see "Network: the proxy"), and listens on the launch socket `.gnomish-relay-launch` in the temp folder of the run. It writes `ready` when it takes commands.
- The wrapper (`--sandbox-run`) sends one request to the launch socket: the shell, the working folder, the command, and the variables of the command. The holder starts `bash -c <command>` inside the sandbox, in a process group of its own, with no input. It sends back the output and the error output in frames, and last the exit status. The wrapper prints them as its own, and exits with that status.
- With no holder, a command does not start: the wrapper exits with status 126, "The sandbox of the run is not running." There is no fallback.
- A server that a command starts in the background runs until the run ends or Stop: the bridge stops the holder then, and each process in the sandbox ends with it. When the wrapper goes away first, for example at the timeout of Claude Code, the holder stops the process group of that command.
- The holder lives outside the wall of the agent, as a child of the bridge. The launch socket lies in the temp folder, which the wall binds back, so the wrapper in the wall reaches it. A command also reaches the socket, and it can only start another command in the same sandbox.
- Why not `nsenter`: a first design joined the namespaces of the holder with `nsenter`. It worked on Arch Linux, but on the Ubuntu runner of CI `nsenter` could not join the network namespace ("Operation not permitted"), and `nsenter --wd=<folder>` opened the folder before it joined, so `cd ..` left the walls. The holder needs no join.
- Commands of one run can signal each other. That is one sandbox.
- A limit: a background command that keeps the standard output open keeps the Bash call of Claude open until the command ends or times out, as with no sandbox. Use `> log 2>&1 &`, or the background option of the Bash tool.
- macOS: Seatbelt starts a sandbox for each command, with no network but the proxy port and `local_ports`. So a server of one command does not answer a later command there. Opening the whole loopback of this computer would be the wrong trade, so this is a named gap.

**macOS: `sandbox-exec`** with a generated profile: `(allow default)`, `(deny network*)`, `(deny file-write*)`, an allow of writes to a few devices (`/dev/null`, `/dev/tty`, `/dev/fd`), an allow of writes to the chat folder and the temp folder, and last a deny of reads and writes under each hidden path, `/private/tmp`, and `/private/var/tmp`. With the proxy, one rule after `(deny network*)` allows the loopback port of the proxy of the run: `(allow network-outbound (remote ip "localhost:<port>"))`. A later rule wins in Seatbelt. Each path is a string literal in the profile, with the escape of S32. Mach services stay reachable, so the keychain answers with the rules of its own access lists. With the proxy, the profile denies the two services of the keychain: `(deny mach-lookup (global-name "com.apple.SecurityServer") (global-name "com.apple.secd"))`. `git credential-osxkeychain` gives the GitHub token of the user with no prompt, and the proxy reaches `github.com`, so a command could push to the repositories of the user. The deny has a cost, checked on the macOS 26 runner of CI: a tool that checks TLS with the Security framework of macOS fails, for example `curl` with its SecureTransport backend ("Couldn't understand the server certificate format"). The Go tools and Rust programs with `native-tls` check TLS in the same way. The system `curl` and `git` (LibreSSL), cargo (the system libcurl), npm, and pip do not, and they work. A build that signs code with a key in the keychain fails too. A deny of `com.apple.secd` alone does not stop a read of the keychain.

**Hidden paths that exist.** A `bwrap` mount and a Seatbelt `subpath` name a real path, so the bridge looks for the hidden paths at the start of each run: the `deny` folders, each pattern in the home folder (for example `~/.ssh` and `~/.config/gh`), and a walk of the chat folder. The walk does not follow links, but a link with a hidden name hides its real target. The walk has no size limit: it does not follow links, so it ends, and it reads about 1000000 entries in less than a second. A walk that takes more than 5 seconds gets a log line with its time and its count. The walk skips no folder, not even `target` or `node_modules`. A command can mark any folder as a cache, for example with a `CACHEDIR.TAG` file, and a skip would then show a real `.env` in that folder to the next run. Limits:

- A path that matches a pattern in another folder, for example `.env` in another project under `allowed_roots`, stays readable for commands. The classifier still asks before a command that names it.
- A file that a command makes during the run and that matches a pattern is not hidden. It holds only what the agent wrote.
- The logins of the agents are `desktop` paths too (`.claude.json`, `.claude/.credentials.json`, `.codex/auth.json`), so no command reads them.
- In the sandbox `.git/config` reads as empty, so `git` works with no remote and no settings of the repository, and `git config` fails. The hooks of git are gone.

**The `.git` entries** (fixed on 2026-09-27; the tests came first). Git outside the sandbox trusts what a `.git` names: its hooks, its config (for example `core.fsmonitor` and `core.hooksPath`), and the folder that a `.git` file points to. A command must not change any of it:

- **Pinned.** The walk of the chat folder notes each `.git`, a folder or a file, at any depth and in any ASCII case. `bwrap` binds each one onto itself after the writable binds, a folder writable and a file read-only. A mount point cannot be moved or removed, so `mv .git x`, `rm -rf .git`, and a new `gitdir:` line all fail. Seatbelt denies writes to the path of each one with a `literal` rule, so the entry cannot be moved, removed, or written, and the files inside stay writable. So a commit still works: objects, refs, the index, and `HEAD` are writable.
- **Every git folder, not only the top one.** A folder inside a `.git` with a `HEAD` file is a git folder: a submodule under `.git/modules/`, or a worktree under `.git/worktrees/`. Each one hides `config`, `hooks`, `commondir`, and `config.worktree`, as the top `.git` hides `config` and `hooks`.
- **A `.git` link stops the run.** A command could replace the link, and a mount cannot pin a link. The error names the link.
- **macOS: the folders above stay in place.** A Seatbelt rule names a path, so a move of a folder above a hidden or pinned path takes the path out of the rule, for example `mv packages p2 && cat p2/app/.env`. Seatbelt denies writes to the path of each folder between a writable folder and a hidden or pinned path inside it. Such a folder cannot be moved or removed in the sandbox. On Linux a mount follows the move of a folder above it, and a hard link across a mount fails.
- **Limit.** A new `.git` that a command makes in a folder with none is not pinned. Git outside the sandbox finds it only when it runs in that folder. A chat folder with no `.git` at the start has the same limit.

**Network: the proxy** (asked for by the user and decided with an advisor on 2026-09-26). The user needs `cargo fetch`, `cargo build` with new dependencies, `npm install`, `pip install`, and `git fetch` over https in a game run. So a command reaches a short list of package hosts through a proxy of the bridge, and nothing else:

- **The proxy** (`proxy.rs`) runs in the bridge, outside the sandbox, one for each run. It takes only HTTP `CONNECT` to a host of the allow list on port 443 or 80. It never ends the TLS, so it sees only the host name. It takes no plain `GET http://...`: the tools use https, and a forward of plain HTTP needs a second parser.
- **The host check.** The name must be on the list, compared exactly and without ASCII case. An IP address in any form (`[::1]`, `127.1`, `0x7f000001`) is refused before any lookup, and `localhost` is only for `local_ports`. The first line must be exactly `CONNECT <host>:<port> HTTP/1.<digit>`. `hosts.rs` and `connect.rs` in `protocol` hold these rules. The proxy resolves the name once and refuses it when any address is not on the public internet: loopback, private, link-local, shared (100.64/10), multicast, reserved, the IPv6 forms that hold such an IPv4 address, and the local NAT64 range `64:ff9b:1::/48` with the rest of `64:ff9b::/32` outside `64:ff9b::/96` (`ip.rs` in `protocol`, fuzzed against a table of ranges). It then connects to a checked address. No second lookup happens, so a name cannot resolve to a public address for the check and to an inside address for the connection.
- **Limits.** At most 64 connections at once for each run, 10 seconds for the request head (at most 8 KiB) and for the connection, and 5 minutes with no byte in either way. Each refusal writes a log line with the chat, the host, and the reason, and the command gets `403` with the reason.
- **Linux.** `bwrap --unshare-all` leaves the command a network with only its own loopback. The proxy listens on a Unix socket in the temp folder of the run, which the sandbox already binds at the same path. The holder of the run (see "One sandbox for each run") listens on `127.0.0.1:3128` of that network and on each port of `local_ports`, and relays each connection to the socket. No `socat` and no `unsafe`.
- **macOS.** The proxy listens on a free loopback port. The profile allows only that port, after `(deny network*)`, and denies the services of the keychain (see "macOS: `sandbox-exec`"). Other programs of the user can also reach the port, but they already have the full network, and the proxy gives them nothing more.
- **The variables.** The command gets `HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy`, `ALL_PROXY`, `CARGO_HTTP_PROXY`, `npm_config_https_proxy`, and `npm_config_proxy` with `http://127.0.0.1:<port>`, and `NO_PROXY` and `no_proxy` set to `localhost,127.0.0.1,::1`: the loopback of the sandbox on Linux, and the ports of `local_ports` that Seatbelt allows on macOS. A command that clears them has no way out: the OS network stays off. The caches of npm and pip in the home folder are read-only, so `npm_config_cache` and `PIP_CACHE_DIR` point into the temp folder of the run.
- **The default hosts,** each checked against what the tool fetches:

| Host | Why |
|---|---|
| `index.crates.io` | The sparse index of crates.io |
| `static.crates.io` | The crate files: the `dl` of the index config |
| `static.rust-lang.org` | `rustup`, when `rust-toolchain.toml` asks for a toolchain |
| `github.com` | `git fetch` over https, and git dependencies of cargo |
| `codeload.github.com` | Archives of a tag or a branch |
| `objects.githubusercontent.com`, `release-assets.githubusercontent.com` | Release files, where `github.com` sends a download |
| `raw.githubusercontent.com` | Single files, for example install scripts of build tools |
| `registry.npmjs.org` | npm packages and their files |
| `pypi.org`, `files.pythonhosted.org` | The index of pip and its files |

- `crates.io` itself is not on the list. It is the API host of `cargo publish`, and `cargo fetch` does not need it.
- **S31 stays as it is.** S31 is about hidden paths and writable paths, and the proxy adds neither: the socket lies in the temp folder. `Network::Off` of the policy stays true at the OS level: `bwrap` gives no network but a private loopback, and Seatbelt denies all network but one loopback port. The proxy is a channel of the bridge beside the policy, not a part of it.
- **What the list does not stop.** A list limits where a command connects. It does not stop data that leaves to an allowed host. A command with a token of its own can push to `github.com` or publish to npm. Without an end of the TLS in the bridge, nothing closes this. So the tokens of these tools are hidden (6.6.3), and the list stays short.
- **Build scripts.** A command such as `cargo build` runs a `build.rs` or an npm install script that the agent can edit. It reaches the allowed hosts too. The short list keeps this small.
- The agent process has a proxy of its own, with other rules: see "The agent process behind the proxy" below.
- With no host in the list (12), the proxy does not start, and commands have no network at all.

**The agent process behind the proxy** (asked for by the user on 2026-09-27, decided by the user with an advisor on 2026-09-27; built on Linux). S33 to S35 are approved and proved (14.1). A part that is not built yet says so.

*What it gives.* Without a wall, a hostile prompt can make the agent process send data anywhere: with WebFetch that the user approves by mistake, with an MCP server, or with any command that runs in the tree of the agent with no sandbox. For Claude the gain is small, because WebFetch and MCP already ask or are off, and its commands already go through the command proxy. For ACP agents the gain is large: their commands run in the tree of the agent with no sandbox (6.6.4, "Other ACP agents"), so the wall of the agent is the only network wall of those commands. The same holds for the commands of Codex that an `allow` rule of Codex runs outside its sandbox. The wall also keeps the agent away from this computer: its loopback, its network, the sockets of the desktop, and the other processes of the user.

*Measured on 2026-09-27* (Arch Linux, `bwrap` 0.13.0). Each agent ran in `bwrap --dev-bind / / --unshare-net`, with `gnomish-relay --sandbox-forward` as the only way out, to a logging proxy, with `HTTPS_PROXY` set:

| Agent | Hosts it connected to |
|---|---|
| Claude Code 2.1.283, `claude -p` with the flags of a game run, a prompt "Reply with only the word hi." | `api.anthropic.com` (11 connections) and `http-intake.logs.us5.datadoghq.com` (telemetry) |
| The same with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | `api.anthropic.com` only |
| codex-cli 0.157.0, `codex exec`, not logged in on this computer, so every call got `401` | `chatgpt.com`, `github.com`, and `api.openai.com` (a WebSocket first, then HTTPS) |

- Not measured, but named in the programs: `platform.claude.com/v1/oauth/token` (the refresh of a Claude login), and `auth.openai.com` (the refresh of a Codex login). A live run with a Codex login is still to do.
- Both agents took the proxy from `HTTPS_PROXY`, and nothing tried to connect around it: a connect with no proxy has no route in the namespace.
- Nesting works on Linux. Inside the wall of the agent (`--unshare-net --unshare-pid --proc /proc`, a tmpfs on `/run` and `/tmp`), a Bash call of the real `claude` went through the real `--sandbox-run`: `bwrap --unshare-all` in the wall, with its own forwarder. The command reached `index.crates.io` through the command proxy (200), a direct connect failed, and the command did not see the socket of the agent proxy. `codex sandbox` (the Linux sandbox of Codex) also works in the wall, and so does a nested `--overlay`.

*Two kinds of scrutiny* (decided by the user): "things run by the agent and outside the agent deserve different scrutiny".

| Who | Hosts through the proxy | This computer |
|---|---|---|
| The agent process (`claude`, `codex`, ACP agents, and the `claude` model calls of Timeways) | Any public host, with `agent_network = "open"` (the default). With `agent_network = "strict"`, only the model hosts of the backend and the `agent_hosts` of the entry. | Only the ports of `local_ports` |
| A command of a game run (6.6.4) | The package hosts and `allow_hosts`, as before | Only the ports of `local_ports` |

- "Public" is the address rule of "The host check": the proxy resolves the name once, refuses it when any address is loopback, private, link-local (this holds the cloud metadata address `169.254.169.254`), shared, multicast, reserved, or an IPv6 form of such an address, and connects to a checked address. The name rules stay the same: no IP address in any form, no `localhost`.
- Ports 443 and 80 only, in both modes. The agent reads `~/.ssh`, so with port 22 open it could push to any repository with the keys of the user. Port 25 would send mail.
- The proxy writes a log line for each host that it opens, with the chat, and for each refusal.
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` for Claude: no telemetry and no update check.
- The command proxy lies in the temp folder of the run, which the agent sees, and the agent can run `--sandbox-run` itself. So in `strict` mode the hosts of the commands are hosts of the agent too.

*The model hosts of each backend* (for `strict`):

| Backend | Model hosts |
|---|---|
| `claude`, and the model calls of Timeways | `api.anthropic.com`, `platform.claude.com` |
| `codex` | `api.openai.com`, `chatgpt.com`, `auth.openai.com` |
| ACP and `command` | None. The entry names its hosts in `agent_hosts`. A preset of `command` adds the model hosts of its tool (`harness_presets.rs`), and a `command` agent also reaches the hosts of the sandbox, because it runs its own commands. |

An entry with Bedrock, Vertex, or another `ANTHROPIC_BASE_URL` names its hosts in `agent_hosts`.

*The wall on Linux.* The agent process keeps most of its file access: it writes `~/.claude`, `~/.claude.json`, and `~/.codex`, and it resumes sessions ("Where the wall is"). Its network, its processes, its sockets, and the startup files of the user change. `process.rs` starts the agent as:

`bwrap --dev-bind / / --tmpfs /run --tmpfs /tmp --tmpfs /var/tmp --tmpfs /dev/shm <binds back> <read-only startup files> <sockets covered> --unshare-net --unshare-pid --proc /proc --die-with-parent --new-session -- <gnomish-relay> --sandbox-forward <socket> <local ports> --exec <agent> <args>`

- `--unshare-net` gives the tree a network with only its own loopback. The forwarder listens on `127.0.0.1:3128` there and relays each connection to the agent proxy. The `--exec` form starts the agent with no shell, with its arguments as they are (6.2 rule 11).
- `--unshare-pid` with a new `/proc`. With the `/proc` of the host, the agent reads `/proc/<pid>/root` of any process of the user, and through it the session bus under `/run`. Then `systemd-run --user` starts code with the full network. A new `/proc` also stops `ptrace` and `pidfd_getfd` of host processes, which `ptrace_scope = 0` allows. Measured: `claude -p` and a nested `bwrap` both work with it.
- The tmpfs on `/run`, `/tmp`, `/var/tmp`, and `/dev/shm` hides the sockets there: the session bus, the systemd manager of the user, the ssh agent, Docker, and X11. An abstract socket belongs to one network namespace, so the new namespace hides those. The binds back put the chat folder and the temp folders of the run at their paths again, when they lie under one of these folders.
- A socket at a path elsewhere still works across namespaces, for example `~/.docker/desktop/docker.sock` (`docker run --network host` is a full way out), the sockets of Lima, Colima, and Podman machines, of terminal programs, and of editors. At the start of each run, the bridge looks for socket files in the top 3 levels of the home folder, and binds `/dev/null` over each one. A socket in another place stays reachable. This is a named limit.
- Stop: with `--unshare-pid`, the whole tree ends when `bwrap` ends, also a grandchild. The 10-second grace of Stop (9.4) still comes first, so a session file is not cut short.
- The variables: the allowlist of 6.2 rule 12, the `env` list of the entry, the proxy variables of "The variables" with port 3128, `NO_PROXY` and `no_proxy` set to `localhost,127.0.0.1,::1` (see `local_ports`), and for Claude `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
- `gnomish-relay check-agent` starts a Codex or ACP agent inside the same wall. The check of Claude makes no model call (`--version` and `auth status`), so it runs with no wall.
- Tests: `crates/bridge/tests/agent_wall.rs` runs `fake-claude`, `fake-codex`, and `fake-acp-agent` in the real wall, with the network probes of `src/bin/shared/net_probe.rs`. The fuzz target `agent_wall` reads the `bwrap` arguments as `bwrap` does.

*The socket of the agent proxy.* Each run has a second proxy for the agent, with the rules of the agent:

- Its socket lies in the data folder (`<data>/sandbox/<name>.sock`). S31 hides the data folder from every command, so no command reaches the agent proxy, for example to reach a public host that is not a package host. A test checks that the socket lies in a hidden path.
- A Unix socket path holds at most 108 bytes on Linux. The bridge binds through `/proc/self/fd/<folder>/<name>.sock`, so a long data folder still works, with no `unsafe` code and no change of the working folder. Inside the wall, a bind puts the socket at `/run/gnomish-relay/agent.sock`.

*`local_ports`* (`[sandbox] local_ports = [5432, 3000]`). The agent and its commands reach `127.0.0.1` and `::1` of this computer on exactly these ports. Everything else on the loopback of this computer stays closed.

- Measured: `psql`, `redis-cli`, and other database clients open a plain TCP connection, and do not speak HTTP `CONNECT`. So the forwarder in each namespace (the wall of the agent and the sandbox of a command) listens on `127.0.0.1:<port>` and `[::1]:<port>` for each port. It relays each connection through the proxy socket with `CONNECT localhost:<port> HTTP/1.1`. A client inside connects to `localhost:<port>` as usual. A failed listen on `::1` (IPv6 off) writes a log line and is not an error.
- The proxy takes `localhost:<port>` only when the port is on the list. It connects to `127.0.0.1:<port>`, and to `[::1]:<port>` only after "connection refused", with no lookup. Only the proxy checks the list: the agent can change the walls file and the arguments of the forwarder.
- `NO_PROXY` holds `localhost,127.0.0.1,::1`, so `curl http://localhost:3000` goes straight to the loopback of the namespace: to the relay for a listed port, and to a server of the agent for any other port. A server that the agent starts in the wall works on the loopback of the wall. A listed port is taken by the relay inside the wall, so a server of the agent cannot listen on it; the error of the forwarder names the port.
- The proxy refuses 2375 and 2376 (Docker over TCP) and 9222 (the debug port of a browser), because each one runs any code with no question. Config load refuses them too.
- A warning: every other listed port is the choice of the user. A dev server with an eval endpoint, or a database with a superuser that runs programs, gives the agent the same power.
- macOS: the Seatbelt profile of a command allows `(remote ip "localhost:<port>")` for each port. The agent on macOS has no wall yet.
- A local model entry (for example Ollama on port 11434) works when its port is in `local_ports`. Setup adds the port of a local model that it configures.

*The startup files are read-only for the agent.* The agent keeps its file writes, so it could write code that runs later outside the wall, with the full network. The wall binds each of these read-only, when it exists:

- The shells: `~/.bashrc`, `~/.bash_profile`, `~/.bash_login`, `~/.bash_logout`, `~/.profile`, `~/.zshrc`, `~/.zprofile`, `~/.zshenv`, `~/.zlogin`, `~/.zlogout`, `~/.config/zsh`, `~/.oh-my-zsh/custom`, `~/.config/fish`.
- The desktop and the session: `~/.config/systemd/user`, `~/.local/share/systemd/user`, `~/.config/autostart`, `~/.config/environment.d`, `~/.pam_environment`, `~/.xprofile`, `~/.xinitrc`, `~/.config/plasma-workspace/env`.
- Programs early on `PATH`: `~/.local/bin` and `~/.cargo/bin`.
- Tools that run code from their config: `~/.ssh`, `~/.gitconfig`, `~/.config/git`, `~/.cargo/config.toml`, `~/.npmrc`, `~/.config/pip`, `~/.pip`, `~/.pypirc`, `~/.vimrc`, `~/.config/nvim`, `~/.config/direnv`, `~/.gnupg`, `~/.config/Code/User`.
- The config of the agents, which starts programs in a terminal session later: `~/.claude/settings.json`, `~/.claude/settings.local.json`, `~/.claude/CLAUDE.md`, the folders `hooks`, `commands`, `agents`, `skills`, and `plugins` in `~/.claude`, and `~/.codex/config.toml`.
- These stay writable: `~/.claude/projects`, the other session files, `~/.claude.json`, `~/.claude/.credentials.json`, `~/.codex/auth.json`, `~/.codex/sessions`, and `~/.cache`.
- A limit: a file that is missing at the start has no mount, because the home folder is the real disk and a mount point would appear there. So the bridge notes the missing paths of the list at the start of a run and looks again at the end. For each new one, it writes a log line and the reply ends with "The agent made ~/.zshrc during the run. Check it before you open a new terminal."

*The tools of the agent that use the network:*

- WebFetch runs in the agent process. It asks on the desktop as before (6.6.3). After Approve it works in `open` mode for a public host, and fails in `strict` mode for a host that is not on the list.
- WebSearch runs on the servers of Anthropic, through `api.anthropic.com`. The proxy cannot see it. The classifier still asks on the desktop.
- MCP servers: a game run of Claude starts none (`--strict-mcp-config`). An MCP server of Codex from the config of the user starts in the wall, with the rules of the agent.

*What the wall does not stop:*

- In `open` mode it does not stop data that leaves to a public host. It stops the way to this computer and its network, and it logs each host. `strict` mode limits the hosts, and still does not stop data that leaves to an allowed host: a key of the attacker on `api.anthropic.com`, WebSearch, and on the Codex side all of `chatgpt.com`.
- A socket file outside the top 3 levels of the home folder, a listed local port, and a startup file that is not on the list.
- The agent can still write the walls file of its run in `<data>/sandbox/`. That is true of the command sandbox too.

*With no working `bwrap`,* the agent runs with no wall, the log line at start says so, and the first reply carries "(No network wall for the agent on this computer: it has the full network.)" (decided by the user).

*macOS* (decided by the user: a spike first). The spike on the macOS runner of CI on 2026-09-27: inside a Seatbelt profile that denies the network, `sandbox-exec` fails with "sandbox_apply: Operation not permitted" (the test `seatbelt_cannot_start_inside_a_seatbelt_wall`). A Seatbelt wall on the agent would break the command sandbox of the bridge and the sandbox of Codex, so the agent has no wall on macOS, and the first reply carries the notice of no wall. If that test ever fails, nesting works, and macOS can get a wall.

*Windows.* No wall, as for commands (6.6.4, "Windows"). The setup recommends WSL2.

*Timeways.* The story program already has no network. The model calls of `model_claude.rs` run `claude -p` with no tools; they go into the same wall with the hosts of `claude`. The local route (`model_local.rs`) is `curl` of the bridge to `local_url`, not agent code, and stays as it is.

*The keys* (12): `[sandbox] agent_network = "open" | "strict"` is one key for every agent and for Timeways. `[sandbox] local_ports` is one list for the agent and for commands. `agent_hosts` is a list in each `[agents.<name>]` entry, for `strict` mode.

*Verification.* The pure parts of the proxy decision live in `crates/protocol`, over bytes and integers: `hosts.rs` (the host name and the list), `ip.rs` (the public address), and `connect.rs` (the target of a request). The bridge keeps the sockets, the lookup, and the connection. The statements, approved by the user on 2026-09-27 and proved (14.1):

- **S33, host check.** For every allow list and every host, `host_allowed(list, host)` is true exactly when the host is a good host name and equals a name of the list without ASCII case. A good host name (`good_host_name`) has 1 to 253 bytes, at least two labels, labels of 1 to 63 bytes of `[A-Za-z0-9-]` that do not start or end with `-`, and a last label that starts with a letter and is not `localhost`. So no IP address in any form passes. Lean shape: `∀ list host, hosts.host_allowed list host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ∧ ∃ h ∈ strs list.val, lowerAscii h = lowerAscii (bytes host.val) ⦄`, and `∀ host, hosts.good_host_name host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ⦄`.
- **S34, public address.** For every IPv4 address, `is_public_v4` is true exactly when the address is in none of the ranges of the table `v4NotPublic` (0/8, 10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.0.2/24, 192.88.99/24, 192.168/16, 198.18/15, 198.51.100/24, 203.0.113/24, and 224/3). For every IPv6 address, `is_public_v6` gives the answer of the embedded IPv4 address for `::ffff:0:0/96`, `64:ff9b::/96`, and `2002::/16`, and otherwise is true exactly when the address is in none of the ranges of `v6NotPublic` (`::/16`, `100::/16`, `2001::/23`, `2001:db8::/32`, `64:ff9b::/32`, `fc00::/7`, `fe80::/10`, `fec0::/10`, and `ff00::/8`; `::/16` holds `::` and `::1`, and `64:ff9b::/32` holds the local range `64:ff9b:1::/48`). Lean shape: `∀ o, ip.is_public_v4 o ⦃ r => r = true ↔ ¬ inRanges v4NotPublic (v4Nat o) ⦄` and `∀ s, ip.is_public_v6 s ⦃ r => r = true ↔ match embeddedV4 (v6Nat s) with | some v4 => ¬ inRanges v4NotPublic v4 | none => ¬ inRanges v6NotPublic (v6Nat s) ⦄`.
- **S35, the target of a request.** For every request head of at most 8 KiB, every mode, every host list, and every port list, `check_target(mode, list, ports, head)` returns a target or a defined refusal, and never panics. It returns a target only when the first line, up to the first CR LF, is `CONNECT <host>:<port> HTTP/1.<d>` with one space between the three parts, `<d>` one digit, `<port>` 1 to 5 digits whose value `p` is at most 65535, and no `:` or space in `<host>`, and then in one of three ways:
  - `Local(p)`: the host is `localhost` without ASCII case, `p` is on the port list, and `p` is not 2375, 2376, or 9222.
  - `Remote(h, p)` in the mode `Listed`: `p` is 443 or 80, `host_allowed(list, host)` is true (S33), and `h` is the host in lower case.
  - `Remote(h, p)` in the mode `Public`: `p` is 443 or 80, `good_host_name(host)` is true (S33), and `h` is the host in lower case. The bridge then connects only to an address for which `is_public` is true (S34); this last step is bridge code, and a named test covers it.
  Lean shape: `∀ mode list ports head, head.val.length ≤ 8192 → connect.check_target mode list ports head ⦃ r => ∀ t, r = .Ok t → ∃ h ds p d rest, bytes head.val = "CONNECT " ++ h ++ ":" ++ ds ++ " HTTP/1." ++ [d] ++ "\r\n" ++ rest ∧ isDigit d ∧ allDigits ds ∧ 1 ≤ ds.length ∧ ds.length ≤ 5 ∧ decimalValue ds = p ∧ ' ' ∉ h ∧ ':' ∉ h ∧ ((t = .Local p ∧ lowerAscii h = "localhost" ∧ p ∈ ports ∧ p ∉ [2375, 2376, 9222]) ∨ (∃ th, t = .Remote th p ∧ (p = 443 ∨ p = 80) ∧ bytes th.val = lowerAscii h ∧ (mode = .Listed → hostAllowed list h) ∧ (mode = .Public → goodHostName h))) ⦄`.

Tests and fuzzing, with no proof:

- Fuzz: `connect` checks `check_target` in each mode against a model in the fuzz target. A new target `public_ip` compares `is_public` with a table built from the `ipnet` crate. A new target `agent_wall` checks the `bwrap` arguments of the agent: `--unshare-net`, `--unshare-pid`, and `--proc` are there, each tmpfs comes before its binds back, each startup file is read-only, and the agent and its arguments come last, each one as it is.
- Unit tests: the model hosts of each backend, the keys, the path of the agent socket inside a hidden path, the scan for socket files, the check of new startup files, and the variables of the agent.
- E2E with the fake agents and the real `bwrap` (`crates/bridge/tests/agent_wall.rs`). `fake-claude`, `fake-codex`, and `fake-acp-agent` get script steps that connect with no proxy, send a `CONNECT` through `HTTPS_PROXY`, connect to a local port, and serve on the loopback of the wall. The test proxy resolves `allowed.test` to a public address, as in `command_sandbox.rs`. For each fake agent: a direct connect fails; a public host through the proxy works in `open` mode; a host that is not on the list gets `403` in `strict` mode; a name that resolves to `127.0.0.1` gets `403`; a listed local port works and another one does not; a server of the agent on the loopback of the wall answers. Also: a command of `fake-claude` still runs in the command sandbox, reaches the command proxy, and does not reach the agent socket; a socket in the fake home and one under `/run` are not reachable; `/proc/<pid of the bridge>` does not exist in the wall; a startup file cannot change; a grandchild of the agent ends at Stop. CI sets `GNOMISH_REQUIRE_BWRAP`, so these do not skip.
- Live tests marked `#[ignore]`: the real `claude -p` answers in the wall in both modes, and a Bash call inside it still runs in the command sandbox. The same for `codex app-server` with a login.

**The downloads of cargo and rustup** (decided with an advisor on 2026-09-26). cargo writes each new crate into `~/.cargo/registry`, and rustup each toolchain into `~/.rustup`. The sandbox keeps both read-only, so a download fails there. The choices, and why only one works:

- A writable `~/.cargo` lets a command change a crate source that a build on the host runs later (`build.rs`, proc macros). That is a way out of the sandbox.
- Another `CARGO_HOME` changes the path of each crate source. cargo then builds every dependency again, and again after each build on the host, because the path is part of its fingerprint (checked on cargo 1.97). A large workspace builds for many minutes.
- So on Linux, `bwrap` lays a copy-on-write view over `~/.cargo` and `~/.rustup` at the same paths: `--overlay-src <folder> --overlay <upper> <work> <folder>`. The upper and work folders lie in the temp folder of the run. A command sees the cached crates of the host, at the paths that the host uses, so nothing builds again. A new crate lands in the temp folder, and the real folder never changes. Later commands of the same run see it, and it goes away with the run.
- S31 stays true: every write lands in the temp folder. A hidden file in these folders, such as `~/.cargo/credentials.toml`, stays hidden, because `bwrap` covers it after the overlay. A folder that is writable already or that is inside a hidden path gets no overlay.
- The view needs a `bwrap` with `--overlay` and Linux 5.11 or later. The `bwrap` 0.9.0 of Ubuntu 24.04 cannot make it (checked on the CI runner), and `bwrap` 0.13.0 on Arch Linux can. At start, the bridge runs `bwrap --version` inside a sandbox with a view of `/etc`. With no view, a new download of cargo or rustup fails, and the log line at start says so.
- macOS has no copy-on-write view. There cargo builds with the crates that the host has, and a new crate fails. `static.rust-lang.org` is of use only on Linux for the same reason.

What each backend and OS enforces:

| Backend | Linux | macOS | Windows |
|---|---|---|---|
| Claude | The sandbox of the bridge (`bwrap`) around each command | The sandbox of the bridge (`sandbox-exec`) around each command | None: fallback. Under WSL2, as Linux. |
| Codex (`codex app-server`) | Its own sandbox: `read-only` at `ask`, `workspace-write` otherwise | The same | Its own Windows sandbox |
| Other ACP agents | None: they ask at most (6.6.3) | None | None |
| `command` | The sandbox of the bridge (`bwrap`) around the whole harness | The sandbox of the bridge (`sandbox-exec`) around the whole harness | None: the bridge does not start it |

**Codex.** Codex runs its commands in its own sandbox. The bridge sets `sandbox_workspace_write.exclude_slash_tmp`, and a private temp folder of the run as `TMPDIR` (checked on codex-cli 0.157.0: `thread/start` answers with `excludeSlashTmp: true`). Against S31:

- Writes match: the chat folder and the temp folder only, and at `ask` nothing.
- Network is stricter: Codex commands get no network, and no proxy.
- Reads do not match: `workspace-write` reads the whole disk, with no hidden path. A command can read `~/.ssh` and the keys of the bridge. The bridge cannot put Codex in its own sandbox: the login and the rules of Codex live in `CODEX_HOME`, and on macOS the sandbox of Codex cannot start inside Seatbelt. codex-cli 0.157.0 has `permissions.<profile>.filesystem.deny_read`, but its format has no documentation yet, so it waits for a live test.
- A command that an `allow` rule of Codex covers runs outside the sandbox (6.6.3).

**Other ACP agents.** The bridge cannot reach their commands: an agent runs a command itself and asks only when it wants to. So they have no sandbox, and the answer of each of their calls is at most `ask` (6.6.3). The `@anthropic-ai/sandbox-runtime` of an earlier plan is not built.

**Fallback, when the computer has no sandbox tool** (Windows, a Linux with no working `bwrap`):

- Every command asks in the game, at every level, also a command of the allow table (`gate::without_sandbox`).
- File edits inside the chat folder still work.
- The first reply of Claude after the start of the bridge begins with "(No sandbox on this computer: every command asks in the game.)". The bridge writes the tool of the sandbox to its log at start. A notice in the chat header waits for new slot fields.
- With no sandbox, the popup offers no "Always allow" (6.6.5): every command asks anyway, so a rule does nothing. The second warning step of the earlier plan went.
- At start the bridge runs `bwrap --version` inside a sandbox of the same kind. A `bwrap` that is missing, or that cannot make namespaces, counts as no sandbox.

**Windows** (tested on the Windows runners of CI on 2026-09-27, and decided with an advisor). A sandbox there needs calls of the Windows API, and these calls need `unsafe` code, which every crate of this project forbids (CLAUDE.md). The planned backend was an AppContainer through the `rappct` crate (MIT, 0.13.3), approved by the user on 2026-09-26: the crate holds the `unsafe` calls. A spike on Windows Server 2022, Windows Server 2025, and Windows 11 on ARM showed that an AppContainer cannot run the commands of Claude:

- Claude Code on Windows runs each command of its Bash tool with Git Bash, so the wrapper must start `bash -c <command>` inside the sandbox. The runtime of Git Bash (MSYS2) stops at start in an AppContainer with status `0xC0000142`, and so does each MSYS2 tool, for example `ls.exe`. The runtime makes a folder of named objects at an absolute path under `\BaseNamedObjects`, and the AppContainer redirects only the names of the Win32 calls (microsoft/mxc issue 1061). No setting changes this.
- Measured, cause unknown: a deny entry in an ACL did not stop the AppContainer. The chat folder had `icacls <chat> /grant *<capability SID>:(OI)(CI)M`. Then `icacls .env /deny *<SID>:F`, `icacls .git\hooks /deny *<SID>:(OI)(CI)F`, and `icacls .git /deny *<SID>:(DE)`, each with the package SID, the capability SID, or ALL APPLICATION PACKAGES (`S-1-15-2-1`). A command still read `.env`, wrote `.git/hooks/pre-commit`, added a new hook, and renamed `.git`. Only `del .env` failed. This goes against the documented access check, so a later attempt checks this setup first. If it holds, a hidden path inside the writable chat folder needs a protected ACL on the user's own files and on each folder above a `.git`, which changes these files for good.
- `git.exe` of Git for Windows fails with "could not open '/dev/null'". A Rust program cannot start a child with piped or null output, because Rust std makes a named pipe in the global namespace. So cargo cannot run rustc.

What works in the spike: a launch with a profile (a SID with no profile gives "file not found", and `rappct` 0.13.3 falls back to such a SID with no error when the profile has no description); writes only where a grant names a capability SID of the command; no read of the home folder; no internet with no capability; a connection to an AF_UNIX socket in a folder with a grant; and loopback between two processes of one AppContainer. `cmd.exe` and native tools such as `curl.exe` run.

So Windows keeps the fallback, and the bridge has no `rappct` dependency. The setup recommends Codex, which has its own Windows sandbox, or Claude under WSL2, where the sandbox is `bwrap`. A later Windows backend needs a shell that starts in an AppContainer, or a sandbox of another kind, for example a separate user of the OS. A low-integrity token blocks writes but still reads `~/.ssh`, and a virtual machine for each command is too slow. The source of the spike is commit `265414d` on the branch `spike-appcontainer` (CI run 36286834623), not on `main`: code with no caller costs readability.

**One launch step for every tool.** The wrapper asks `command_sandbox::launch` how to start a command inside the walls of the run. The tools of the sandbox are the variants of `Sandbox` (`bwrap`, `sandbox-exec`, none), and `launch` gives a `Launch` for each one. `bwrap` and `sandbox-exec` are a program with its arguments. A Windows backend adds a variant to `Sandbox` and to `Launch`, one arm in `detect`, `launch`, and the start of the wrapper, and nothing else: the Claude backend, the gate, and the walls stay as they are.

**Processes.** For game messages, the bridge starts one agent process per run. Each run has its own walls and its own temp folder (9.4).

**The story program of Timeways (9.7, decision 9).** The story program reads hostile text, so the bridge starts it in a sandbox of its own. `crates/bridge/src/story_sandbox.rs` builds it with the tools that the user already has. The bridge installs nothing.

| Rule | Value |
|---|---|
| Write | Only its story folder, `<data>/timeways/story/` (mode 0700) |
| Read | The system, except the config folder, the data folder (its own folder comes back), and the `desktop` paths of 6.6.3 under the home folder that exist. The lore pack and the program file stay readable, also inside a hidden folder or `/tmp`. `/tmp`, `/var/tmp`, and `/run` are private and empty, because they hold the sockets of the ssh agent and the desktop. |
| Network | None |
| Children | In the same sandbox |

| OS | Sandbox | Tests |
|---|---|---|
| Linux | `bwrap` (bubblewrap): `--ro-bind / /`, a `--tmpfs` over each hidden folder and `/dev/null` over each hidden file, a writable `--bind` of its folder, a `--ro-bind` of the lore pack and of the program file, then `--remount-ro` of each hidden folder, `--unshare-all`, `--die-with-parent`, and `--new-session`. | Real `bwrap` runs in CI (14.5). |
| macOS | `sandbox-exec` with a generated Seatbelt profile: `(allow default)`, `(deny network*)`, `(deny file-write*)`, a deny of reads and writes under each hidden path, then an allow of reads of the lore pack and of the program file, and last an allow of its folder. The paths go in as `-D` parameters, never into the profile text. | Built and checked as text only. It never ran: the CI tests run with no sandbox on macOS. Mach services stay reachable. |
| Windows | None | |

- At start the bridge runs `bwrap --version` inside a sandbox of the same kind. A `bwrap` that is missing, or that cannot make namespaces (some systems block them for normal users), counts as no sandbox.
- With no sandbox, the story program still runs. The bridge writes a log line, and the first reply with text after the bridge starts carries "The Timeways story program runs with no sandbox here." (9.8).
- `bwrap` sets `PWD` to the story folder. Its other variables are the allowlist of 6.2 rule 12.
- The `desktop` patterns that match anywhere, such as a `.env` file in a project, are not hidden. The sandbox hides only the ones under the home folder.
- **The program file** (decided with an advisor on 2026-09-26). The bridge resolves the `program` of the config to its real path, with no link in it, and starts that path. The sandbox shows only this one file, read-only, never its folder. So a program under `/tmp` or `/run` starts. A program inside a hidden path is refused, and the bridge does not start: the config folder, the data folder, or a `desktop` path. The error is "[story] program <path> is inside a folder that the sandbox hides (the config folder, the data folder, or a credential folder). Install it somewhere else, for example ~/.local/bin.". A missing program also stops the start of the bridge, with "[story] program <path> is missing".
- Why: the program is the user's own choice, and the sandbox runs it anyway, so a read of its file shows nothing new. A program in a hidden path is different. The story folder is inside the data folder and is writable, so a program there can write itself and keep a break-in across starts. A program in the other hidden paths is odd and can be a sign of tampering. The check reuses the list of hidden paths, so there is one rule: a hidden path never holds the program.
- Limits: the sandbox shows the program file alone. A program that loads libraries from its own folder (`$ORIGIN`), a program in a folder bundle, or a script whose interpreter lies in a hidden path does not start. Libraries under `/usr` are readable.

#### 6.6.5 "Always allow"

Asked for by the user on 2026-09-27, designed with an advisor, and approved by the user on 2026-09-27. Before it, every command that the allow table did not cover asked in the game, every time. With "Always allow", the user approves a command once, and it stays approved in that folder. The code is in `crates/protocol/src/always.rs` (the rules, S36 to S39), `crates/bridge/src/always_rules.rs` (the file), `always_offer.rs` (the choice of the popup), and `gate.rs`.

The goal is one click for the common case, with a bounded worst case. Any game click can come from another addon (6.6.1). So a rule is safe to add with one click only where the sandbox is the wall for what the rule allows.

*Why one click, and no desktop click.* A hostile addon can already click "Allow" on every popup, and it can already send messages. A forged rule gives it only one thing more: the rule stays after the addon is gone. The rule is one pattern in one folder, and its commands run in the sandbox (6.6.4): writes only in the chat folder, network only to the allowed hosts, and the secrets hidden. The larger risk of a rule is a prompt-injected agent, and a desktop click does not help against that. A desktop click for each new rule brings back most of the friction.

**When the popup offers "Always allow".** All of these hold, else the popup has only Allow and Deny:

1. The call is a shell command.
2. The level of the run (9.3, the lower of the chat and the config) is `auto-edit`. At `full-auto` the command runs with no question anyway. At `ask` the level promises that every command asks, so rules do not apply there.
3. The commands run in the command sandbox of 6.6.4: Claude with `bwrap` or `sandbox-exec`. With no sandbox, `gate::without_sandbox` asks anyway, so a rule does nothing. An ACP agent picks its questions, so a rule never gives it `allow` (6.6.3).
4. Not Codex, for now. Codex retries an allowed command outside its sandbox with no request, and its sandbox reads `~/.ssh` and the keys of the bridge (6.6.4, "Codex"). So for Codex the sandbox is not the wall. The allow table of the config has the same hole today. Codex gets Always after a live test shows that the retry asks.
5. `offer` (in `protocol`) gives a proposal: 1 to 3 new rules, one for each simple command that no rule covers yet, and with them the classifier gives `allow` for the whole call. So a `deny`, `desktop`, or "never always" part, a redirect into a hidden path, and a command substitution never get Always (S17, S36 to S39).
6. The rule line fits in 48 bytes, so the popup never cuts it (6.4).
7. The folder keeps room for the new rules: at most 64 rules in one folder. A full folder gets no Always, and the Settings tab shows its rules. The bridge never drops a rule by itself, because a dropped rule brings back popups that the user does not expect.
8. The bridge has its rules file. A gate with no file, as in `check-agent`, offers no Always.

**The pattern.** The bridge makes each rule from the words of one simple command, with `propose` in `protocol`. The agent and the game never choose it.

- For a tool with subcommands, the rule is its name and its first word: `cargo test *`, `git status *`, `npm run *`. A tool with subcommands and no first word, such as `cargo` alone, gets no rule, because `cargo *` also covers `cargo publish`. The list is in `always.rs`: `git`, `cargo`, `npm`, `pnpm`, `yarn`, `go`, `uv`, `pip`, `poetry`, `gradle`, `mvn`, `dotnet`, `rustup`, and `just`.
- For every other tool, the rule is the name only: `rg *`, `ls *`, `pytest *`, `tail *`, and `make *` for `make test`.
- Why not narrower: at `auto-edit` the agent can edit `package.json`, a `Makefile`, and the tests in the chat folder. So `npm run build *` protects nothing that `npm run *` does not, and it costs more clicks.
- No proposal when the first word after the name starts with `-` or `+` (`git -C x status`, `cargo +nightly test`), because `git *` is far too wide.
- No proposal when the name holds `/` (`./gradlew`, `./x.sh`). The agent can write such a script, so the rule is "all Bash". A user adds such a script to the allow table by hand.
- No proposal for the tools that run any program or download code: `npx`, `npm exec`, `pnpm exec`, `pnpm dlx`, `yarn dlx`, `yarn exec`, `bunx`, `uvx`, `pipx`, `uv run`, `poetry run`, and `docker`. They amount to "all Bash".
- No proposal for the commands that publish or send to other people: `git push`, `cargo publish`, `npm publish`, `twine`, and `gh`. The proxy reaches `github.com`, and a push is a way to leak data (6.5). The allow table of the config can still name them.
- Each word is printable ASCII, 1 to 64 bytes, with no special character of the allow table (12). The rule holds the literal bytes of the words, because the matcher compares bytes.
- A `cd` is a simple command as any other, so `cd lib && cargo test` proposes `cd *` and `cargo test *`. A `cd` with a redirect stays `desktop` (6.6.3).

**The folder of a rule.** A rule covers one folder: the chat folder, resolved, and every chat inside it, as `[allow.folders]` does. One exception: when the chat folder is an entry of `allowed_roots` or the home folder, the rule covers that exact folder only. Else one click in `~/Documents/Code` gives a global rule from the game. The rule folder is the chat folder, not the current folder of the Bash tool, so a `cd` into a subfolder keeps the rules of the chat.

**The popup** (6.4). The buttons are Allow once, Always allow, and Reject. One more line, above the buttons, names the rule: "Always allow: cargo test *, tail * in Code/Personal/gnomish-relay". The folder is its path from the folder above its allowed root, `~/` in the home folder, and else the full path. A folder name that does not fit is cut from the left, after "...". The line is bridge text: the rule words (plain ASCII) and a folder name that the user chose, with each character that is not printable ASCII as `?`. The addon shows it with the escape of S10. It comes in the `label` of the `allow_always` option of the live file, so S20 does not change. The button is "Always allow", the label that `Popup.lua` already has for the kind. The answer hash (6.6.1) of an Always answer covers the popup text and this line, so the hash binds the rule that the user saw. The bridge fixed the rule when it opened the request, so an answer only picks the option.

**After a grant.**

- The bridge adds the rules to `rules.json` and allows the call. A rule that exists already gives no second row.
- Every other open request that the new rules now cover runs, so the user does not click twice. A run that waits for the game reads the rules about every 100 ms. When they cover its call, the call runs, and the run tells the bridge to take its popup away.
- On the click, the addon prints one whisper line: `[Claude] whispers: [chat] Rule added: cargo test * in Code/Personal/gnomish-relay. Remove it in Settings.` A click on the line opens Settings. The addon marks the settings list as old, so the next open of Settings asks for a new one.
- The desktop shows a plain notice: "Rule added: <line>. Remove it in the Settings tab of the game, or run: gnomish-relay rules". There is no Undo button: a notice cannot hold a button on all three OSes, and the whisper line has none.
- A rule that the bridge cannot write leaves a log line. The call still runs, because the user allowed it.

**The store.** The rules live in `rules.json` in the data folder (12), never in `config.toml`: the game never writes the config (6.6.2). The data folder is a `deny` path, so the agent never reads or writes the file (6.6.3). The file has mode 0600, and the bridge writes it with an atomic rename. Each row has an id (4 hex digits), the folder, its scope (`tree` for the folder and every folder inside it, `exact` for a root or the home folder), the words, the time it was added, and the day of its last use.

- The bridge reads the file again for each tool call, so `gnomish-relay rules remove` works while the bridge runs. A lock keeps two runs from losing each other's rule.
- At load, each row must have the shape that `propose` makes: 1 or 2 plain words (the word rules above) and no `/` in the first, an absolute folder with no `.` or `..`, and no unknown field. A link in place of the file, or a file over 1 MiB, is no rules. A bad row is dropped with a log line, so a broken file never widens a rule. A missing or broken file is an empty list.
- Global rules stay in `[allow] commands` of the config, which the user edits by hand.

**Expiry.** A rule ends 30 days after its last use. So a rule that the user uses stays, and a stale or forged one goes. The bridge writes the day of the last use at most once a day for each rule, so a command does not write the file each time. A deleted or moved folder leaves orphan rules, which expire. A new folder at the same path gets them until then, and Settings shows them.

**See and remove.**

- The Settings tab (13.1) has a group "Always Allowed": one row for each rule, with its pattern, its folder, its last use, and a remove button. The mouse wheel scrolls it. The game can remove a rule, because a removal only narrows: a forged removal costs only a click. The addon sends `rule=remove:<id>` in a control record of the chat `settings`, and then asks for a new list. The bridge removes the rule before it answers the list. A removed row stays grey ("Removing...") until the next settings list.
- `gnomish-relay rules` lists the rules on the desktop, and `gnomish-relay rules remove <id>` removes one.
- The settings list (13.4) gets `rule` lines. Diag shows no second copy.

**What never becomes a rule.** File edits: a write outside the chat folder is `desktop`, and one inside it already runs at `auto-edit`. Unknown tools. Every `deny` and `desktop` answer, so the startup files (`.claude/`, `.git/hooks/`, and the others of 6.6.3) and the hidden paths. The "never always" commands of 6.6.3. Desktop requests: the desktop dialog never offers Always, because a desktop request is the dangerous case.

**The own "always" of each backend.** The `permission_suggestions` of Claude and the `acceptForSession` and `acceptWithExecpolicyAmendment` of Codex stay off (9.3). The bridge keeps the only rules, so the classifier sees each rule.

**Verification.** The user approved S36 to S39 on 2026-09-27. They are proved (`proofs/Protocol/Always.lean`), and `proofs/Axioms.lean` checks their axioms. The approved Lean shapes below are the plan; `proofs/Statements.lean` has the exact text. There, `isDesktop` is `desktopSimple`, `isCapped` is `neverAlways`, `simplesOf` is `inCall`, and `rules ++ rs` is every slice that holds the rules and then `rs`. `plainWord` asks printable ASCII with no space, which is stricter than the plan.

- In `protocol`, in the Aeneas subset: `propose(simple) -> Option<rule>` and `offer(call, policy, rules) -> Option<rules>` in `always.rs`. The matcher `rule_matches` is the one of S17. `offer` checks its rules with `classify` before it returns them, and refuses a rule list of more than 4096 rules, so the join of the lists never overflows.
- **S36, proposal shape.** For every simple command, `propose` never panics, and `propose(s) = some r` gives `r = take k s.words` with `k` 1 or 2. Each word of `r` is 1 to 64 bytes of printable ASCII with no special character and does not start with `-` or `+`, and the first word holds no `/`. So the rule covers the command that it came from. Lean shape: `∀ s, always.propose s ⦃ o => ∀ r, o = some r → ∃ k, (k = 1 ∨ k = 2) ∧ r.val = s.words.val.take k ∧ (∀ w ∈ r.val, plainWord w.val) ∧ ¬ hasSlash (r.val.head!).val ⦄`.
- **S37, no proposal for the capped.** For every simple command that is `desktop`, "never always", a tool that runs any program, or a command that publishes, `propose` gives `none`. Lean shape: `∀ s, (isDesktop s ∨ isCapped s.words ∨ noRuleTool s.words) → always.propose s ⦃ o => o = none ⦄`.
- **S38, an offer allows exactly its call.** For every call, policy, and rule list, `offer` never panics, and `offer = some rs` gives: `rs` has 1 to 3 rules, `classify(call, rules ++ rs) = Allow`, and each rule of `rs` covers a simple command of the call. Lean shape: `∀ call policy rules, always.offer call policy rules ⦃ o => ∀ rs, o = some rs → 1 ≤ rs.len ∧ rs.len ≤ 3 ∧ action.classify call policy (rules ++ rs) = ok Verdict.Allow ∧ ∀ r ∈ rs, ∃ s ∈ simplesOf call, ruleMatches r s.words ⦄`.
- **S39, an offer stays under the ceiling.** For every call, `offer = some rs` gives `ceiling(call) = Allow`. It follows from S17. Lean shape: `∀ call policy rules rs, always.offer call policy rules = ok (some rs) → action.ceiling call policy = ok Verdict.Allow`.
- Each goes into `proofs/Axioms.lean`.
- Fuzz targets: `always` (random simple commands and calls: `propose` and `offer` never panic, each proposal is a prefix of its words, and each offer makes `classify` give `allow`), and `rules_file` (any text as `rules.json`: no panic, and each rule that loads has an id of 4 hex digits, a clean absolute folder, 1 or 2 plain words, and a last use in the last 30 days).
- Unit tests in the bridge (`always_rules.rs`, `always_offer.rs`, `gate.rs`, `activity.rs`, `settings_list.rs`, `flags.rs`, `relay.rs`): the folder of a rule (a root and the home folder cover only themselves), expiry and the daily write, the full list, a duplicate rule, the answer to the other open requests, the level and backend checks of the offer, the 48-byte line, and the settings lines.
- Fake-game tests in `addon_flow.rs`: the popup shows the rule line and the three buttons; an Always click sends the hash of the text and the line, and whispers the rule; the bridge takes it; an Always answer that another addon forges with the hash of the text alone counts for nothing; the whisper line opens Settings; Settings lists the rules, and a remove sends the id, asks for a new list, and greys the row. With the fake agents (`claude_gate.rs`, `codex.rs`): an Always click adds the rule, and the next same command runs with no question; with no sandbox, and for Codex, the popup has no Always. In `gate.rs`: at `ask` the popup has no Always and a rule does not apply; a `git push`, an `rm -rf`, and `npx` get no Always; a rule that another popup adds ends an open question.

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

**Frame layout (bytes):**

```
[0x6E 0x52] [version] [time: 4 bytes] [frame id hi, lo] [len hi, lo] [payload: len bytes] [fletcher16 s1, s2] [mac: 8 bytes]
```

- Magic bytes `0x6E 0x52` differ from `wow-claude` (`0xC7 0x1A`). A wrong magic means "not a strip".
- `version` is the protocol version, 1 for this spec.
- `time` is the Unix time from `time()` in the game, big-endian. The bridge uses it for the freshness check (S11).
- `frame id` is the message id modulo 65536. It only tells frames apart. The record ids are the real keys.
- Fletcher-16 covers version to payload. It catches damaged pixels.
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
The two rows fix the cell width but not the row height. So the decoder reads the data rows with each size that matches, and keeps the first one whose bytes decode as a frame with a valid checksum and whose tag checks under a key. The checksum does not cover the tag, so a wrong row height can read the payload right and a tag alone in the last row wrong. With no reading that passes the tag, the bridge logs the first reading with a valid checksum as rejected.
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

The flags split in two (9.7, decision 6). Every app sends the **transport flags**: `h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, and `restored`. Only the relay reads the **coding flags**: `perm=`, `level=`, `agent=`, `attach=`, `list`, `list=folders`, `list=settings`, `mkdir=1`, `d`, `n`, and `stop`. `flags.rs` has one parser for each part, so a coding flag in a record of another app does nothing.

| Flag | Meaning |
|---|---|
| `n` | Start a new agent session for this chat. |
| `h` | Hello only. It announces the token and the addon version. It has no prompt. The addon sends one at login and after it applies a restore bundle (7.6). |
| `d` | The chat is deleted. The bridge stops its runs, and drops its replies, its session link, and its history. A reply of a deleted chat can never be read, so it must leave the body (7.3). The addon keeps the id in `db.forget`, and sends it with each strip until a strip goes out while the bridge is online. The agent session itself stays, so Resume can bring the chat back. |
| `list` | Asks for the saved sessions of the agents (9.6). The record is a message of the chat `relay`, and the reply is the list. |
| `list=folders` | Asks for the folder tree of the browser (9.9). The record is a message of the chat `folders`, and the reply is the tree. Any other `list=` value is ignored. |
| `list=settings` | Asks for the settings list of the bridge (13.4). The record is a message of the chat `settings`, and the reply is the list. |
| `mkdir=1` | The folder of the record is a new folder. The bridge makes its last part before the run (9.9). Only a record with `n` makes it. Any other `mkdir=` value is ignored. |
| `attach=<session>` | The first message of a resumed chat. It has no text. The session must be in the last list (9.6). |
| `agent=<name>` | The agent for a new chat. The config must have an `[agents.<name>]` entry, or the message ends with "Agent not set up." |
| `level=<level>` | The mode of the chat: `ask`, `auto-edit`, or `full-auto`. The run gets the lower of this level and the level of the agent in the config (S6). An unknown word counts as `ask`. |
| `perm=<request>:<option>` | The answer to a permission request (9.3). |
| `read=<id>,<id>` | The final replies in the last body that the addon has shown. The bridge then takes them out of the slot body (7.3). A lost strip loses nothing: the next strip names them again. |
| `restored` | The addon has applied the restore bundle for its token (7.6). |
| `next=<n>` | The next slot that the addon loads (7.3). |
| `build=<n>` | The client build from `GetBuildInfo`, digits only (7.8). |
| `ver=<n>` | The protocol version of the addon (7.7). |
| `out=shot` or `out=fail` | The result of the last screenshot (7.8). |
| `in=slots` or `in=missing` | The result of the last slot load (7.8). |
| `listen=start` or `listen=stop` | Push-to-talk for the chat of the record (13.3, later). |
| `voice=skip` or `voice=stop` | Skip to the next paragraph of the spoken reply, or end it (13.3, later). |

**Strip lifetime:**
The strip shows only while its screenshot is taken, about half a second.
If no acknowledgment comes in 40 seconds, the addon shows the strip again, up to 3 times in all.
Then the addon uses the reload fallback (7.5).
A wait for the shared corner (7.1.2) is not part of the 40 seconds, and it is not a show.

#### 7.1.2 The shared corner

Every app of the shared transport (9.7, decision 14) draws its strip in the same top-left corner.
Two strips at the same time give a screenshot that no app can read.
Also, a `SCREENSHOT_SUCCEEDED` or `SCREENSHOT_FAILED` event has no owner, and every addon gets it.
So the apps take turns through one shared global, `GnomishStripCorner` (9.7, decision 13).
`Strip.lua` follows `models/corner.qnt` (14.2).

**The value.** It holds the holder (the name of the strip frame of the app), the time when the hold ends, and a wait mark for each app that waits.
A wait mark holds the time when the app started to wait, and the time when it last asked. All times come from `GetTime()`, one clock for all addons.
WoW Lua runs one handler at a time, so an app reads and writes the value in one step.
A `/reload` resets the globals of all addons together, so no holder stays from an older UI session.

**The rules.**

- An app takes the corner when no other app holds it and no other app waits longer. A hold ends after 12 seconds, so an app that stops with an error frees the corner.
- After its strip ends, the app keeps the corner for a tail of 2 seconds. A late event of its own shot then finds no strip of another app to end.
- An app that cannot take the corner writes its wait mark, and asks again at its next Tick, one second later. A mark that is older than 3 seconds belongs to an app that stopped waiting.
- With no hostile addon, an app waits at most 15 seconds: its own tail, one strip and tail of the other app, and one Tick.
- While an app waits, it signs nothing, and no show counts. So its 40-second retry timer and its 3 shows wait too, and a wait never starts the outbox.
- A screenshot event ends a strip only when the app holds the corner and has called `Screenshot()`. So `out=shot` and `out=fail` report only the shots of the app.
- A screenshot of the player during our shot can still end our strip early, because an event has no owner. This costs at most one early end or one wrong `out=` value, and the next retry covers it. So the addon does not try to match events to shots.
- After 30 seconds of waiting, the app shows one line: "<title>: screenshots are blocked by another addon. Turn off the addons that take screenshots, then type /reload." The window shows "Screenshots blocked". The line shows again only after the corner was free between. 30 seconds is two times the longest honest wait, and far below the 270-second limit of a signed frame.
- A blocked app keeps waiting. It does not use the outbox: a hostile holder stays across every `/reload`, so the outbox would ask for a reload for each message.
- Each app hooks the "Screen captured" text, and each hook hides the text of the shots of its own app only. A second `Hide` does nothing.

**A hostile addon.** Every addon can read and write the value. A hostile addon can already block the strip, for example with a hook on `Screenshot()`. So the blocked line is the answer to a hostile holder.
The addon reads and writes the value with `rawget` and `rawset`, so a metatable has no effect. A value or a field of a wrong type counts as missing.

**Versions.** The Timeways copy of the transport is pinned to a relay tag, so two versions of `Strip.lua` can run at the same time. A new shape of the value needs a new global name.

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
It syncs each file before the rename, and skips a file that already holds the same bytes: a sync costs milliseconds on Windows, and the restore and live files seldom change.
The window starts at the next slot that the addon reported (`next` flag, 7.1.1).
Writing all 1000 slots at every publish costs too much disk: a 20 KB body every 3 seconds is 20 MB per publish.

- The addon reports `next` in every strip. When it nears the end of the window without a strip to send, it sends a hello with `next`.
- At a hello, or when the saved variables file changes (a `/reload`), the bridge starts the window at the reported slot, or at slot 1.
- A slot outside the window holds an older body. A read of an older body is harmless: every record stays in the body until a `read` flag names it, so a later poll gets it. The model (14.2) checks this.

Each slot is a folder `GnomishRelay_S0001` to `GnomishRelay_S1000` with four files:

- `GnomishRelay_SNNNN.toc`: `## Interface: 16001`, `## LoadOnDemand: 1`, `## Dependencies: GnomishRelay`, and the three Lua file names.
- `Inbox.lua`: the body.
- `Restore.lua`: the restore bundle (7.6). With no restore, its token is empty, and no addon takes it.
- `Live.lua`: the progress lines of each run, and the permission requests for the game (9.3, S20, S21).

The body sets one global table. S9 fixes its shape:

```lua
GnomishRelay_SlotData = {proto = 1, now = 1790211081, replies = {
{chat = "c1", id = 12, status = "working", text = ""},
{chat = "c1", id = 11, status = "done", text = "..."},
}}
```

**The key check.** A player with an old key sees no reply and no reason: the bridge refuses each strip for its tag. So the bridge counts the strips with a bad tag since the last good relay strip. While the count is not 0, `Inbox.lua` ends with one more line after the table: `GnomishRelay_SlotData.badTags = 2`. The line holds only the global and a decimal number, so no outside text reaches it, and S9 still covers the table. A message that the addon gives up on while the count is not 0 ends with "Not sent: the bridge does not know this key. On the desktop, run gnomish-relay setup, then type /reload." A bad tag has no app, so only the relay body counts it.

`Live.lua` carries the progress and the permission requests (9.3, S20), and `Restore.lua` the restore bundle (7.6, S18).
Later fields (the session and the denied rules) go into a file of their own, or need an approved change of S9. The notifications of section 10 ride in `Live.lua`, with the restatement of S20 (10.3).

- `proto` and the pool sizes let the addon detect a mismatch (7.7).
- `replies` holds every record that the addon has not read, at most 30. Each `text` is at most 32 KB. The bridge cuts longer text and adds a note with the full length.
- A final reply stays in the body until a `read` flag names it. Then the bridge takes it out.
- When the body holds 30 records, the bridge refuses new messages. It does not mark a refused message as seen, so the addon sends it again: the strip shows again, and the outbox (7.5) keeps it. The bridge takes new messages again after the next `read` flag.
- The `transport.qnt` model (14.2) checks these rules. Without them, a reply can drop out of the body before the addon reads it.
- In `Live.lua`, `permissions` holds open permission requests (9.3), and `notices` holds the notifications of terminal sessions (10.3).
- String escapes follow one function in the `protocol` crate. The addon reads the file as Lua source, so the escape rules are part of the protocol.
- The bridge writes progress at most every 3 seconds. It writes final replies at once.
- If `LoadAddOn` returns `MISSING` or `DISABLED`, the addon reports "slots not installed".

#### 7.3.1 Reply blocks

Agent replies are Markdown. The game cannot parse Markdown safely, so the bridge renders it with `render_markdown` in `protocol` (`markdown.rs` and `inline.rs`).
The rendered text goes into the normal `text` field, so S9, S18, and S20 do not change.

- The bridge renders only the text of a `done` reply. Errors, lists, and user messages stay plain.
- An attach reply (9.6) is `prompt\nanswer`. The bridge renders only the answer.
- The history of the bridge keeps the rendered text, so a restore shows the same blocks as the live reply.

**Format.** The text starts with the marker `ESC M 1` (`1B 4D 31`). Each block is one line: `\n`, a kind byte, then fields that each start with `US` (`1F`). A last `\n` ends the text.

| Kind | Block | Fields |
|---|---|---|
| `h` | Heading | level `1` to `3` (`####` and deeper show as `3`), text |
| `p` | Paragraph. The lines of one paragraph join with a space. | text |
| `l` | List item. A line below it continues it. | level `0` to `4` (2 spaces of indent per level), the number or nothing for a bullet, text |
| `q` | Quote. Its lines join. | text |
| `c` | One line of a code fence (```` ``` ```` or `~~~`). A tab becomes 4 spaces. | text |
| `t` | Table row. The delimiter row (`\|---\|`) shows nothing. | `1` for the row above a delimiter row, else `0`, then one field per cell |
| `r` | Rule (`---`, `***`, `___`) | none |

**Text rules:**

- Every `|` of the agent text is doubled (S10). A `\|` inside a table cell is a `|` of the cell.
- Control bytes go, and a tab becomes a space. So ESC, US, and `\n` never come from the agent.
- Texts of `h`, `p`, `l`, and `q` go into SimpleHTML, so `<`, `>`, and `&` become `&lt;`, `&gt;`, and `&amp;`. Texts of `c` and `t` go into font strings, and keep those bytes.
- Inline marks become WoW color codes: bold `ffd100`, italic `c0c8ff`, both `ffe680`, inline code `b8e0b8`, and link text `69b4ff`. A link shows its text only: WoW cannot open a browser.
- The renderer writes the only `|c` and `|r` codes. Colors never nest, and each one closes in its own field.
- A mark with no closing mark is text. So are `*` between spaces and `_` inside a word.
- The output is at most 16 times the input plus 4 bytes (S25). A bold text with many `*_*_` switches costs 12 bytes for each input byte, so a bound of 10 is false.

**Cuts.** The body cuts a text at 32 KB (S12) and the restore at 500 bytes (S18). A cut text has no last `\n`. The addon still shows its last line, without color codes, and without a half code, a half entity, or a half character at its end.

S22 to S25 (14.1) prove the renderer for every input of at most 1 MiB. A reply is at most 256 KiB.
The fuzz target `markdown` checks the same shape, escapes, and size bound on the compiled code.

**Poll schedule after a send:** the addon loads a slot at 5, 10, 16, 24, 34, 46, 60, 80, 100, 130, 160, 200, 240, and 300 seconds.
Then it loads one every 60 seconds until the reply is done.
While a run of the relay works (a `working` record), the relay loads one every 15 seconds. A permission popup waits for a poll, so it comes at most 15 seconds late, not 60. Activity shows "Next check in 12 s".
With no message pending, it loads one slot every 10 minutes, for the status light. With notifications on and a terminal session open, it loads one every 3 minutes, and every 60 seconds while a terminal turn runs (10.4).
A signal (7.4) makes the addon load a slot at once.

**Slot budget:** there are 1000 slots per UI session. Each reply costs about one slot when signals work, and about four when they do not. Each desktop request costs at most 24 more slots (6.6.3). A working run costs 4 slots a minute, so the slots of a UI session last about 4 hours of agent work, and "Reload soon" covers the rest. The polls for notifications cost 60 slots in each hour of terminal work, and 20 in each hour with an idle terminal session (10.4).
The window never shows the slot count. `/relay diag` shows it.
Below 20 free slots, the window shows "Reload soon" with a **Reload** button. Only a click on **Reload** reloads. A reload from Enter took the game away for seconds with no warning, so Send never reloads.
`ReloadUI` needs a hardware event, and a click is one. The addon never reloads in combat.
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
| `note/kkkk` | 2000 | A notification or a permission request is waiting (section 10, 9.3). |
| `ctl/empty`, `ctl/valid` | 2 | The self-test at login. |

- `NNN = ((id − 1) mod 200) + 1`.
- Rule 5 in 7.2 makes each signal one-shot until the game restarts. After the message ids wrap past 200, a signal can already be valid. The addon treats an unexpected "valid" as unreliable and uses the poll schedule.
- `presence` and `note` are counters. The bridge keeps 50 files ahead of the counter empty. The counters live in `state.json`. `presence` wraps after about 16 hours.
- **Self-test:** at login, the addon plays `ctl/empty` and `ctl/valid`. If `ctl/empty` plays, or `ctl/valid` does not, signals are off for this session. The addon then uses slot polls only.
- **Status light:** with presence signals, 90 seconds of silence means "stale" and 300 seconds means "down". Without them, the addon reads the `now` of each body that it loads. The bridge writes a body at least every 60 seconds. So a body older than 150 seconds at its poll means "offline", at once. A body 90 to 150 seconds old missed a heartbeat, and the light says "slow". A player who sends a message then learns within 5 seconds that the bridge stopped. With no body for 12 minutes, the light is offline too.

Total file count for slots and signals: about 17,000.

### 7.5 Reload fallback

The addon uses the reload fallback when the strip gets no acknowledgment, the pool is empty, or the slots are missing.

1. The addon writes the signed frame of the message into `outbox` in its saved variables (6.6.1). The bridge checks it as a strip: tag, time, and replay store. A frame counts only if the key of the app whose saved variables hold it signed it (9.7, decision 3).
2. The window shows "Press Reload to send 1 message." with a **Reload** button. `ReloadUI` needs a hardware event, and a click is one. The button does nothing in combat.
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
- Each slot body carries `proto` and the pool sizes. Each report carries the protocol version of the addon (`ver=<n>`).
- The bridge keeps a range of addon versions for each app: `version_fit` in `crates/protocol/src/version.rs` says `Supported`, `TooOld`, or `TooNew`, and S30 proves that it never fails and matches the range exactly (14.1). Today both ranges are 1 to 1. The bridge logs each new version that an addon reports.
- While the last version of an app is out of its range, each message of that app counts as seen, gets one error reply in that app's body, and never reaches an agent or the story program:

| App | Too old | Too new |
|---|---|---|
| Relay | "Type /reload in the game to load the new Gnomish Relay." (the bridge writes the relay addon again at each start, so an old relay addon only needs a reload) | "Update the desktop program: gnomish-relay update." |
| Timeways | "Update Timeways." | "Update the desktop program: gnomish-relay update." |

- With no `ver=` yet, for example just after a bridge restart, the version counts as supported, so a restart refuses no good message.
- `ver=` is the version of each app: `Health.lua` of the shared transport sends `ns.App.version`, from the `App.lua` of the app. Each app changes on its own: the coding flags of the relay, and the batch lines of Timeways (9.8). The Timeways `App.lua` needs `version` when it copies this `Health.lua`.
- On a mismatch, the addon shows "bridge and addon versions do not match" and stops sending.
- Pool sizes live in one place: the `protocol` crate. The setup step writes them into the addon.
- The release version (for example `0.2.0`) is not a protocol version. It lives in two places: `version` of `[workspace.package]` in `Cargo.toml`, which every crate takes, and `## Version` in `GnomishRelay.toc`. The ranges above use only the protocol versions, so a release with no protocol change keeps them at 1 to 1.

### 7.8 Design for breakage

The transport rests on client behaviors that Blizzard never promised: an addon can call `Screenshot()`, and a load-on-demand addon reads its files fresh.
A client patch can break either one. So a patch costs a day of work, not the project.

**One interface per direction.** The core never knows which channel carries a message.

| Direction | Interface | Channels, in order |
|---|---|---|
| Out (game to bridge) | Addon `Out.Send(frame)`, bridge `trait FrameSource` | Strip by `Screenshot()`, reload outbox (7.5) |
| In (bridge to game) | Addon `In.Poll()`, bridge `trait Publisher` | Slots (7.3), fonts (17), reload inbox (7.5) |

- The protocol core, the model, and the proofs work on frames and records. They do not change when a channel changes.
- A new channel is one new module on each side, with its own tests. Nothing else changes.

**Self-test and health report.**

- At login, the addon makes sure that each client function it needs exists (`Health.Required`). If one is missing, it shows one line, "Gnomish Relay: this game version has no <name>. The relay is off. On the desktop, run gnomish-relay update.", and starts nothing.
- The first hello strip and the first poll test the two channels. `SCREENSHOT_SUCCEEDED` or `SCREENSHOT_FAILED` gives the result of each shot, and `LoadAddOn` gives the result of each slot.
- Each strip carries the client build and the last result of each channel: `build=<number>`, `out=shot|fail`, and `in=slots|missing`.
- When a channel starts to fail, the addon shows one line: "Gnomish Relay: screenshots are blocked. Check the free disk space and the Screenshots folder, then type /reload." or "Gnomish Relay: slots are missing. Run gnomish-relay install with the game closed." The window shows the same state in the bridge light.
- `/relay diag` shows the build and the last success of each channel. `gnomish-relay doctor` comes later.
- Today each direction has one channel. A move to the next channel of the table comes with the second channel.

**Builds.**

- The bridge keeps the last client build whose screenshots and slots both work, in `state.json`, and logs each new one.
- Later: the window shows "New game version: checking the relay" until the self-test passes.
- A known break goes into a table in the bridge, so the setup can name the channel that works on each build.

**API compliance.** The addon calls only the API of the real Forever client (1.60.1, the Mainline UI code).

- `scripts/wow-api.sh` reads two sources at pinned commits: the `forever` branch of Gethe/wow-ui-source (Blizzard's UI code) and of Ketho/BlizzardInterfaceResources (the API that the client reports).
- It checks every WoW name that the addon, `wow.yml`, or the fake game uses. A name that the client does not have, or has only in a `Blizzard_Deprecated` addon, stops it. So does an event that the addon registers and the client does not have. It also checks the `## Interface` number of the TOC against the build.
- It writes `addon/tests/api.lua`: the used globals, every widget type with its methods, and each used template with its mixin methods and child keys.
- It writes `addon/tests/api-signatures.lua` from the generated API docs of the client (`Blizzard_APIDocumentationGenerated`). For each used function, each widget method with a called name, and each registered event, it keeps the arguments, the returns, the payload, and every flag: `SecretArguments`, `SecretReturns`, `SecretWhen...`, `HasRestrictions`, `IsProtectedFunction`, and the others. A used function with no doc entry goes into an `undocumented` list, so a new doc entry also shows.
- A client patch can keep a name and change what it takes, returns, or hides behind a secret value. The diff of `api-signatures.lua` then names the change.
- Selene allows only the globals in `wow.yml`. The fake game refuses every method and child key that the real kind and template of an object do not have. The fake game does not check argument counts: the docs mark some arguments as required that the client accepts as missing, for example the last four of `SetPoint`.
- CI runs the script at the pinned commits, and fails if `api.lua` or `api-signatures.lua` changes. A nightly job runs it at the newest commits with the addon tests, and opens an issue when the client changes.
- Other addon repos run the same script with their own paths: `wow-api.sh --addon <folder> --lint <wow.yml> --api <file> --signatures <file>`. With no path, it checks this addon.

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
 │   PNG → decode → policy → queue      │
 │   agent runner → publisher           │
 │   spool ◄── terminal hooks (planned) │
 └──────┬───────────────────────────────┘
        │ ACP / claude / codex / command
 ┌──────▼──────────┐
 │  Coding agents  │  Claude Code, Codex, Gemini CLI, ...
 └─────────────────┘
```

### 8.1 Repository layout

```
gnomish-relay/
  addon/GnomishRelay/   Lua addon
  addon/transport/      the shared Lua transport of every app (9.7). Install copies it into each addon.
  crates/
    protocol/           frames, records, slot body, escapes, dedup, counters. No I/O. Verified with Aeneas.
    app-protocol/       the checked lines of the app protocol (9.8). No I/O. Apps test against it.
    bridge/             the daemon: screenshot reader, policy, queue, publisher, state, and the agents of 9.2
  fuzz/                 the fuzz targets (14.4)
  proofs/               Lean project with the Aeneas output and the proofs
  models/               Quint model of the transport
  tests/vectors/        golden strip images
  SPEC.md
```

Planned, not built: a crate `agents/` for the `Agent` trait and the backends of 9.2, which are in `bridge/` today. The hook command of section 10 is a subcommand of the one binary (10.1), so it gets no crate of its own.

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

- `state.json`: the replay store, the unread records, the waiting messages, the slot window, the tokens, and the restore history (7.6).
- `timeways/state.json`: the lane of Timeways (9.7), only with a Timeways key. Later also agent session IDs per chat, the folder of each session, and signal counters.
- `timeways/story/`: the folder of the story program (9.8). Only the story program writes there, and the bridge never reads it.
- Planned, not built: `transcripts.json`, with every prompt and reply of each chat: 200 messages per chat, 4000 characters each. Today `state.json` keeps only the short history of the restore bundle (7.6).

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
The bridge takes an OS advisory lock on `bridge.lock` in the data folder at start (`File::try_lock` of the standard library). The OS releases the lock when the process stops, also after a crash.
If the lock is taken, the bridge stops with an error that names the process of the other bridge.
The bridge writes its process id into `bridge.pid`. Windows does not let another process read a locked file, so the id has its own file.

## 9. Agents

### 9.1 The Agent trait

Today the trait has one call. It runs one message to its final reply:

```rust
trait Agent: Send + Sync {
    fn run(&self, job: &Job) -> Result<String, String>;
}
```

`Job` carries the chat, the folder after the policy check, the level after the ceiling (S6), and the text.
Each run of the `acp` backend starts the agent process, opens a session, sets the mode of the level, sends the prompt, and stops the process.
Each run of the `claude` backend starts `claude -p` with the mode of the level, sends the prompt, and stops the process at the end of the turn.
Each run of the `codex` backend starts `codex app-server`, opens a thread with the sandbox and the approval policy of the level, starts one turn, and stops the process at the end of the turn.
Each run of the `command` backend starts the harness inside the sandbox of the run, gives it the message, and takes its output as the reply.

- **Resume.** The bridge keeps the agent session of each chat in `state.json`, with its agent and its folder. The next message of the chat resumes it, unless the message has the `n` flag, or the agent or the folder changed. The client uses `session/resume` if the agent offers it, else `session/load`. The history that `session/load` replays stays out of the reply. If neither works, the run opens a new session, and the reply starts with "(New session: the agent could not resume the old one.)".
- **Later: continue a terminal session.** A new chat can take the session of a Claude or other agent session that runs in a terminal. The bridge lists the recent sessions of each agent (`session/list`, where the agent offers it), and the chat resumes the one you pick. The terminal window does not show the game messages live: no agent lets another program type into its open window. `claude --resume` shows them later.
- **Stop.** Stop in the game ends the waiting messages of the chat, and signals the run in progress. The client sends `session/cancel` (`claude`: an `interrupt` control request, `codex`: `turn/interrupt`), answers every open permission request with "cancelled" (`claude`: a deny, `codex`: `cancel`), and waits 10 seconds for the agent to end the turn. Then it kills the process. The reply is "Stopped.", and the session stays for the next message. A Stop before the prompt ends the run at once.

Next, the trait grows events for progress and for permission requests from the game (9.3). Those need new fields in the slot body, so they wait for an approved S9 statement.

### 9.2 Backends

**The goal: one generic backend for any LLM coding harness.** It has two parts:

- `acp` runs any harness that speaks ACP. It exists.
- `command` runs a harness that has only a command line, inside the sandbox. It exists.

`claude` and `codex` exist too. Most players have these two harnesses, so the bridge speaks their own protocols, with no Node.

| Backend | How it works | Progress | Live permissions | Allow & retry |
|---|---|---|---|---|
| `acp` (main) | Agent Client Protocol: JSON-RPC over stdin and stdout. The bridge is the client. | Yes | Yes | Not necessary |
| `claude` | `claude -p` with stream-json on stdin and stdout, and `--permission-prompt-tool stdio`. Needs no Node. | Yes | Yes | Not necessary |
| `codex` | `codex app-server`: JSON-RPC over stdin and stdout, with approval requests. Needs no Node. | Yes | Yes | Not necessary |
| `command` | An argument template. The message goes in, the output comes out. The whole harness runs inside the sandbox. | Its output lines | No | No. The level picks the walls. |

**Support levels.** Any agent with a command line runs. How well the relay protects it depends on what the bridge can see:

| Level | Connection | What the classifier sees | Examples |
|---|---|---|---|
| Full | ACP, `claude`, `codex`, or a tool-call hook | Every tool call that needs an answer, before it runs | Gemini CLI, Claude, Codex, any ACP agent |
| Sandbox only | `command` | Nothing. The sandbox holds the whole harness. | Aider, `llm`, a script |

- A Full agent runs in its "ask for everything" mode. The classifier then answers most questions itself. In a looser mode the agent acts without asking, and the classifier never sees the action.
- `command` needs the sandbox. With no sandbox, the bridge does not start it (see "A harness with only a command line"). There is no "Trusted" level: with no sandbox and no classifier, nothing guards the run.
- An ACP agent needs one line in the config. An agent with a hook system needs a small hook command. Every other CLI agent uses `command`.

Agents that speak ACP with no adapter (checked 2026-09-25 in the official registry, `github.com/agentclientprotocol/registry`, one `agent.json` per agent). Setup knows these commands (11.3):

| Agent | Command |
|---|---|
| Gemini CLI | `gemini --acp` (`--experimental-acp` is the old name) |
| Qwen Code | `qwen --acp` |
| opencode | `opencode acp` |
| goose | `goose acp` |
| GitHub Copilot CLI | `copilot --acp` |
| Cursor | `cursor-agent acp` |
| Kimi CLI | `kimi acp` |
| Augment (auggie) | `auggie --acp` |
| Cline | `cline --acp` |
| Kilo | `kilo acp` |
| Mistral Vibe | `vibe-acp` |

Agents through an adapter:

- Claude Code: the `claude-agent-acp` adapter (formerly `claude-code-acp`). It needs Node. The `claude` backend below needs only the `claude` program, so setup uses that.
- Codex: the `codex-acp` adapter, now in the `agentclientprotocol` organization. It starts `codex app-server` itself. The `codex` backend below needs only the `codex` program, so setup uses that. Codex has no ACP mode of its own (issue openai/codex#9085 is open).

The bridge speaks ACP protocol version 1 in `crates/bridge/src/acp.rs`, with no crate: JSON-RPC 2.0, one message per line.
The `agent-client-protocol` crate needs an async runtime, and the bridge needs only a few messages. Version 2 of the schema is still an alpha.

The agent process is untrusted:

- It gets only `PATH`, `HOME`, `LANG`, `TERM`, `USER`, the temp and Windows profile variables, the `env` list of its entry, and `GNOMISH_RELAY_JOB=1`.
- Each line from it is at most 8 MiB, and the reply is at most 256 KiB. A line that is not JSON ends the run.
- The run ends at `timeout_minutes` (default 30). The bridge then kills the process.
- The bridge declares no `fs` and no `terminal` capability, and answers every other request from the agent with "method not found".
- If the config names a mode for the level, and the agent does not offer it, the run stops. With no mode, the agent runs at its own default, which can be more open.
- The gate (6.6.3, 9.3) answers each permission request. With nobody in the game, a question refuses the call. The reply then ends with "Not allowed from the game:" and the calls that a rule, the desktop, or no answer refused.

**Claude Code with no adapter (`kind = "claude"`).** Most players have the native `claude` program and no Node. The bridge speaks the stream-json protocol of `claude -p` itself, in `crates/bridge/src/claude.rs`. It was checked on Claude Code 2.1.282.

- The command is `claude -p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --permission-mode <mode>` in the chat folder, plus `--resume <id>` for the session of the chat, and the flags of a game run: `--setting-sources "" --strict-mcp-config --settings <json>` (6.6.4). The `command` of the entry comes first, so it can add flags.
- The bridge first sends the `initialize` control request, as the Claude Agent SDK does, and waits for its answer. Then it sends the prompt as one `user` message.
- `system` with subtype `init` gives the session id. Each `tool_use` block of an `assistant` message becomes a progress line (9.3). The `result` message ends the turn, and its `result` text is the reply. A `result` with `is_error` is an error with its text, for example "Invalid API key · Please run /login".
- The `PreToolUse` hook of 6.6.3 gates every tool call. Its timeout is `permission_timeout_minutes` plus 5 minutes, because Claude Code runs the tool when the hook times out. A `can_use_tool` control request goes through the same gate. The bridge answers every other control request with an error.
- The same limits as ACP apply: the environment allowlist, the line and reply limits, the run timeout, and the last line of stderr in an error. The backends share `process.rs` and `turn.rs` for them.
- If the session of the chat has no file, the run starts a new session, and the reply starts with the note of 9.1.

**Codex with no adapter (`kind = "codex"`).** The bridge speaks the protocol of `codex app-server` itself, in `crates/bridge/src/codex.rs`. It was checked on codex-cli 0.157.0 with `codex app-server generate-json-schema` and `generate-ts`. The protocol is JSON-RPC 2.0 with no `jsonrpc` field, one message per line. The bridge uses no method that needs the `experimentalApi` capability.

- The command is the `command` of the entry plus `app-server`, in the chat folder. The bridge sends `initialize` and then the `initialized` notification.
- A new chat gets `thread/start` with `cwd`, `sandbox`, `approvalPolicy`, `approvalsReviewer: "user"`, and `config: { web_search: "disabled" }` (9.3). `approvalsReviewer` keeps a reviewer model from the config of the user out of the way. A chat with a thread gets `thread/resume` with the same values and `excludeTurns`. If the resume fails, the run starts a new thread, and the reply starts with the note of 9.1.
- `turn/start` sends the prompt as one `text` input. `item/started` of a `commandExecution`, `fileChange`, `mcpToolCall`, or `webSearch` becomes a progress line. The text of the last `agentMessage` of `item/completed` is the reply. `turn/completed` ends the turn: `completed` is a reply, `interrupted` is "Stopped.", and `failed` is an error with the message of Codex.
- `item/commandExecution/requestApproval` and `item/fileChange/requestApproval` go through the gate (6.6.3). The bridge answers every other request of the server with "method not found".
- Codex keeps its login and its threads in `CODEX_HOME`, else `~/.codex`. `HOME` passes, so the default works. A user who sets `CODEX_HOME` or `OPENAI_API_KEY` adds it to the `env` list of the entry.
- `check-agent` runs `codex --version` and `codex login status`, with no model call. It fails with "Codex needs a login." when the status command fails.
- The entry has no `modes` table. Config load refuses one.
- The same limits as ACP apply, through `process.rs` and `turn.rs`.

**A harness with only a command line (`kind = "command"`)** (asked for by the user on 2026-09-27: "We need a generic backend for all llm harnesses"; decided with an advisor on 2026-09-27). The code is in `crates/bridge/src/harness.rs`, `harness_args.rs`, `harness_output.rs`, `harness_process.rs`, `harness_sandbox.rs`, and `harness_presets.rs`. Such a harness runs its own tools, so the classifier sees none of its calls. So the whole harness runs inside the sandbox of 6.6.4, and the level picks the walls (9.3).

One line is enough for a tool that the bridge knows:

```toml
[agents.aider]
kind = "command"
preset = "aider"
permission = "auto-edit"
env = ["OPENAI_API_KEY"]
```

- **The template.** `command` is the program and its arguments. `{prompt}` in an argument is the message, and `{prompt_file}` is the path of a file with the message, mode 0600, in the temp folder of the run. A placeholder can be a whole argument or a part of one, for example `--message={prompt}`. With no placeholder, the message goes to stdin, and then stdin closes. The bridge never uses a shell, so the message is always one argument or bytes on stdin.
  - A whole `{prompt}` argument that starts with `-` gets a space in front, so the harness never reads the message as a flag. A NUL byte becomes a space.
  - The text that fills a placeholder is never read again, so a message that holds `{prompt_file}` stays as it is.
  - Config load refuses a word such as `{promt}`, a placeholder in the program, `modes`, and `preset` or `resume` on another kind.
- **Presets.** `preset` fills the template, so the user writes one line. `command` then replaces only the program and can add flags before the arguments of the preset, for example `command = ["aider", "--model", "o3"]`. Checked against the docs of each tool on 2026-09-27. A live test with the real tool is still to do: none of them is installed on the computer of the build.

| Preset | Arguments after the program | The message | Resume | Model hosts for `strict` |
|---|---|---|---|---|
| `aider` | `--message-file={prompt_file} --yes-always --no-pretty --no-stream --no-fancy-input --no-check-update --no-show-model-warnings --analytics-disable`, and at `ask` also `--chat-mode=ask --dry-run --no-auto-commits` | a file | `--restore-chat-history` | none: the model decides, so the entry names them in `agent_hosts` |
| `gemini` | `--prompt={prompt} --approval-mode=yolo` | an argument | none | `generativelanguage.googleapis.com`, `cloudcode-pa.googleapis.com`, `oauth2.googleapis.com` |
| `opencode` | `run --auto {prompt}` | an argument | none | none |
| `goose` | `run -i - -q --no-session` | stdin | none | none |
| `llm` | `--no-log` | stdin | none | `api.openai.com` |

- **Resume.** `resume` lists the arguments for a chat that goes on: the chat ran with this agent in this folder before, and the message has no `n` flag. They go at the end, or before a `--`. The chat keeps a mark as its session, not an id. With no `resume`, each message is a fresh run. The home folder of the harness goes away after each run (below), so only a harness that keeps its history in the chat folder can go on. Of the presets, that is aider. A flag such as `--continue` takes the newest session of the harness, so two chats in one folder share it.
- **The output.** Each line of stdout and stderr becomes a progress line (9.3), through `Activity::step`, so the guards for "Level:" and "Desktop:" apply. All of stdout is the reply, as Markdown. The bridge takes out escape sequences and control characters, keeps the text after the last CR of a line, as a terminal shows a progress bar, and sets `NO_COLOR=1` and `TERM=dumb`. A reply over 256 KiB keeps its end, from the start of a line, after the note "(The output was too long for the game. This is its end.)": a harness prints its answer last. More than 16 MiB of stdout stops the run with "The agent wrote more output than the limit, so the run stopped.".
- **The end.** Exit status 0 is the reply. Any other status is the error "The agent failed (exit status <n>): <the last line of stderr>". No output is "(The agent gave no output.)". The run timeout, Stop, and the output limit kill the whole process group (9.4).
- **The sandbox.** The walls of 6.6.4, around the harness and every program that it starts: writes only in the chat folder and the temp folder of the run, the `deny` and `desktop` paths hidden, private `/tmp`, `/run`, and `/var/tmp`, the `.git` entries pinned, and on Linux its own network, processes, and `/proc`. On Linux: `bwrap <the walls> --chdir <chat> --unshare-all --die-with-parent --new-session -- <gnomish-relay> --sandbox-forward <proxy socket> <local ports> --exec <harness> <arguments>`. On macOS: `sandbox-exec -p <profile> -- <harness> <arguments>`. No holder: the harness is the one process tree of the run.
  - **The network.** One proxy for each run, with the rules of the agent (6.6.4, "Two kinds of scrutiny"), because the bridge cannot tell the harness from its commands. With `agent_network = "open"`, any public host. With `"strict"`, the model hosts of the preset, `agent_hosts`, and the hosts of the sandbox (the default hosts and `allow_hosts`). The ports of `local_ports` work as for every agent. On macOS the profile denies the keychain, so a harness there takes its key from `env`.
  - **The home folder.** On Linux the harness sees a copy-on-write view of the home folder (`--overlay-src`, as for cargo in "The downloads of cargo and rustup"), before the binds of the chat folder, with the hidden paths covered after it. So its sessions, caches, and a refreshed login work during the run, and go away with it. No write reaches the real home folder, so no startup file, MCP server, or hook of a harness can wait for a terminal session of the user. The view needs `bwrap` with `--overlay`, a temp folder outside the home folder, and a home folder outside `/tmp`, `/var/tmp`, and `/run`. Else, and on macOS, the home folder is read-only, and a harness that must write there fails. A mount under the home folder, such as a FUSE folder, shows empty in the view.
  - **Sockets.** A read-only mount does not stop a connection to a socket file. So the walls cover each socket file in the top 3 levels of the home folder with the empty file, as the wall of the agent does. The view of the home folder hides them too.
  - **The variables.** The allowlist of 6.2 rule 12, the `env` list of the entry, `GNOMISH_RELAY_JOB=1`, the proxy and cache variables of "The variables" (6.6.4), `TMPDIR` and `XDG_CACHE_HOME` in the temp folder, `NO_COLOR=1`, and `TERM=dumb`.
- **No sandbox.** On Windows, and on a Linux with no working `bwrap`, the bridge does not start the harness: "This agent runs its own tools, and this computer has no sandbox for them, so the bridge does not start it. Use an agent with kind acp, claude, or codex here, or run the bridge under WSL2 on Windows." There is no opt-in. With no sandbox and no classifier, nothing guards the run.
- **`check-agent`** runs `<program> --version` in the same walls, at `ask`, with no model call. So a program in a hidden path fails there, not in the game. Exit status 126 or 127 is the error "<program> does not start in the sandbox: <the last line of stderr>". It prints the version, whether the entry resumes, the sandbox and what happens to the writes into the home folder, how the message goes in, what the levels mean, and the network.
- **What this does not stop.** In `open` mode the harness and every program that it runs reach any public host, with the keys of its `env` list. That is wider than the commands of Claude, which reach only the hosts of the sandbox. The proxy logs each host. A harness can print a partial answer and exit with 0.
- **Tests.** `crates/bridge/tests/harness.rs` runs `fake-cli-agent` in the real sandbox: the three ways in, progress lines, a crash, a long and a huge output, Stop with a program in the background, the timeout, the writes and the hidden paths, `ask`, the home folder, the proxy in both modes, a socket in the home folder, the variables, resume, `check-agent`, and no sandbox. A test marked `#[ignore]` runs each preset whose tool is on `PATH`, with a real model call. The fuzz target `harness` checks the template and the output reader.

**Adding an agent.** Any ACP agent is one entry in `config.toml`. Nothing else changes:

```toml
[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"
env = ["GEMINI_API_KEY"]
modes = { ask = "default" }
```

Then run `gnomish-relay check-agent gemini`. It starts the agent, opens one session in `default_cwd`, and shows the name, the version, whether it resumes sessions, and its mode ids. It fails if a mode in `modes` does not exist.
For `kind = "claude"`, `check-agent` runs `claude --version` and `claude auth status --json`, with no model call. It fails with "Claude Code needs a login." when `loggedIn` is not true.
The addon sends `agent=gemini` for a chat that uses it. An agent with no entry gets "Agent not set up.".
`kind = "echo"` answers with the message, for a test of the path through the game with no agent.

### 9.3 Permissions

Each agent in the config has one permission level:

| Level | Meaning |
|---|---|
| `ask` | Every write and every command needs an answer. A read inside `allowed_roots` needs none. |
| `auto-edit` | File edits inside the chat folder, and the commands of the allow table (12), need no answer. |
| `full-auto` | No question in the game. The desktop and `deny` answers of 6.6.3 still apply. |

**The gate.** Each tool call gets one verdict from the classifier (6.6.3). The level of the job then picks the action, in `gate::decide`:

| Verdict | `ask` | `auto-edit` | `full-auto` |
|---|---|---|---|
| `deny` | refuse | refuse | refuse |
| `desktop` | desktop | desktop | desktop |
| `ask` | game | game | run |
| `allow`, a read | run | run | run |
| `allow`, a write or a command | game | run | run |

- At `ask`, only a call that only reads runs with no question. A write inside the chat folder, and a command in the allow table, ask in the game. A tool of the session (6.6.3) counts as a read.
- For an ACP agent that picks its questions (6.6.3), `ask` and `allow` both ask in the game, at every level.
- A refusal names its reason to the agent: "It touches the config folder of Gnomish Relay, which the agent never reaches.", "Denied on the desktop.", "No answer on the desktop.", "Denied in the game.", "No answer from the game.", "The player sent a new message.", or "Not allowed from the game." when nobody in the game listens.
- The game gets Allow and Deny for a game question. A desktop question shows only a notice in the game (6.6.3).

**Raise the level** (asked for by the user, decided with an advisor on 2026-09-26). A chat that asks for more than the config allows, for example `auto-edit` with `permission = "ask"`, gets one desktop dialog. The code is in `crates/bridge/src/raise.rs` and `config_edit.rs`.

- The dialog is a desktop request of 6.6.3 of its own kind. So it has the same dialog, the same `gnomish-relay approve` fallback, the same 0600 request file, and the first answer wins. Its text is fixed text of the bridge and the name of the agent from the config, never text from the game:
  - `auto-edit`: "A chat from WoW asks for more access. Allow <agent> to edit files in the chat folder with no question, in every chat from WoW? Commands still ask in the game, unless you added an Always rule there. This writes permission = "auto-edit" to config.toml. Approve only if you just sent a message from WoW."
  - `full-auto` gets a stronger warning: "A chat from WoW asks for full access. Allow <agent> to edit files AND run commands with no question, in every chat from WoW? Any addon that can send a chat message can then run code on this computer, inside the sandbox. The Gnomish Relay addon never asks for this by itself. This writes permission = "full-auto" to config.toml." The addon never asks for `full-auto` today, so this dialog means that another addon made the message. The bridge still offers it, because the user asked for a stronger warning, not for no dialog.
- The run waits for the answer before the agent starts, so an approved run uses the new level. The game shows the notice of 6.6.3 with ` raise <level>`, and its whisper line is "Approve on your desktop: let <agent> work at <level>." The run timeout stops during the wait, as for any question.
- On Approve, the bridge reads `config.toml` again with the checks of config load, changes the one line `permission = "..."` of the `[agents.<name>]` table, and keeps the comments and every other line. It then parses the new text: it must load, the agent must have the new level, and every other level must be the same. Else it writes nothing. It writes the file with an atomic rename and mode 0600. Then it sets the new level of that agent in the policy of the running bridge. It reloads nothing else.
- The bridge checks the edit before it shows the dialog, so the user never approves a change that it cannot write. It refuses a quoted table name, an inline table, dotted keys, a missing or double `permission` line, a value that is not a plain `"..."` string, and a config that does not load. Then there is no dialog, and a log line says what to fix.
- On Deny, a closed dialog, no answer, Stop, a new message, or a write that fails, the run goes on at the level of the config, and the level line of the game shows it (9.3, "The level in the game").
- At most one raise waits at a time. A second chat that needs a raise meanwhile runs at once at the level of the config, with no dialog. So one answer never goes to many runs.
- Every answer that is not Approve starts 10 quiet minutes with no raise dialog, for every agent. A hostile addon that sends messages then gets at most one dialog in 10 minutes. A Deny on the desktop (or a closed dialog) also ends the raise dialogs for that agent until the bridge starts again: a user who set `ask` on purpose then sees the dialog once, not every 10 minutes.
- Only a message with work for the agent can raise. A list of sessions and an attach never do.
- The bridge writes a log line for each raise, its request id, its answer, and whether it wrote the config.
- S6 does not change: at the start of the agent, the level of the run is at most the level of the config. The config changes only through the desktop.

Each backend maps the level differently:

- `acp`: the bridge sets the session mode. Mode IDs differ per agent, so the config has a `modes` table per agent.
- `claude`: `--permission-mode`, and the hook of 6.6.3 for every call. `ask` is `manual`, and `auto-edit` and `full-auto` are `acceptEdits`. The hook decides, so the mode matters only when the hook fails. Then `manual` asks the bridge, and `acceptEdits` does not. The `modes` table of the entry can name another mode: `acceptEdits`, `auto`, `dontAsk`, `manual`, or `plan`. Config load refuses any other name. It also refuses `bypassPermissions`: in that mode Claude Code asks nothing, so no tool call reaches the bridge, and the ceiling of the game has no effect.
  - Not `plan` by default (decided with an advisor on 2026-09-26). In the first test in the game, a "create a file" message at `ask` asked on the desktop. In `plan` mode, Claude Code writes its plan to `~/.claude/plans/<name>.md`, and `.claude/` is a `desktop` write path. The gate already asks in the game before each write at `ask`, so plan mode added only this file. The gate makes no exception for plan files: Claude picks the path, and `.claude/` holds settings and hooks that run code. A user who sets `modes = { ask = "plan" }` gets one desktop question for each plan.
- `codex`: the sandbox of the thread, and `approvalPolicy: "untrusted"` at every level, which sends the most calls to the bridge (6.6.3). `ask` is `read-only`, and `auto-edit` and `full-auto` are `workspace-write`, with `exclude_slash_tmp` and a private `TMPDIR` (6.6.4). The sandbox applies after the answer of the gate. The bridge never uses `danger-full-access`, `never`, `on-request`, or `granular`: none of them asks more than `untrusted`.
- `command`: the harness has no permission channel, so the level picks its walls (9.2, "A harness with only a command line"). `ask` makes the chat folder read-only: the harness reads and answers, and changes nothing. At `auto-edit` and `full-auto` the chat folder is writable, and the harness runs its own commands with no question, inside the sandbox. `auto-edit` cannot keep "commands ask" here, so the first reply after the start of the bridge says "(<agent> runs its own commands with no question, inside the sandbox.)", and so do setup and `check-agent`. The raise dialog to `auto-edit` for such an agent says "Allow <agent> to edit files in the chat folder AND run its own commands with no question, in every chat from WoW? It runs them inside the sandbox." The Settings tab shows the kind `command` next to the level. The addon does not change.
- For game messages, Codex runs through `codex app-server` or ACP only, so the bridge sees each question of its tool calls (6.6.3).

**Live permission flow (ACP):**

1. The agent sends `session/request_permission`. The gate answers it, and a game question waits for the game.
2. The bridge writes the popup text with `popup_text` (S15): the command line of the tool call, else its path or address, else its title, and then its title as "the agent says". It adds the request to `permissions` in `Live.lua` (S20), with the options numbered `o1` to `o4`.
3. The addon shows a popup with the text and the options.
4. The user picks an option. The addon sends a control record with `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256 of the text that the popup showed, in hex.
5. The bridge takes the answer only for an open request of the same chat, a real option, and a matching hash. Then it answers the agent. A second answer does nothing.

**Live permission flow (`claude`):** the hook and a `can_use_tool` control request go through the same steps. The popup text is the `command` of the tool input, else its `file_path`, `notebook_path`, `path`, `url`, or `pattern`, else the tool name. "The agent says" is the tool name and the `description` of the request. The game gets two options: Allow (`allow_once`) and Deny (`reject_once`), and "Always allow" (`allow_always`) between them when the bridge offers it (6.6.5). The hook answers with `permissionDecision` and a `permissionDecisionReason` that Claude sees. For `can_use_tool`, an allow sends the tool input back unchanged as `updatedInput`. A deny sends a `message` that Claude sees, for example "Denied in the game.". The answer never holds the `permission_suggestions` of the request: they add permanent allow rules of Claude Code, and only the bridge keeps rules (6.6.5).

**Live permission flow (`codex`):** an approval request of the server goes through the same steps. For a command, the popup text is its `command`. For a file change, it is the paths of the change, from the `fileChange` item of `item/started`. If the request has a `grantRoot`, the popup text is "write anything in <root>". "The agent says" is the `reason`, else "run a command" or "change files". The game gets Allow and Deny. Allow sends `accept`, and Deny or no answer sends `decline`. The bridge never sends `acceptForSession`, `acceptWithExecpolicyAmendment`, or `applyNetworkPolicyAmendment`: each adds a rule for later calls (6.6.5).

Rules:

- The request id holds the time of the question, so an old strip cannot answer a new request after a restart of the bridge.
- The bridge offers its own `allow_always` for a command of a Claude run at `auto-edit` in the sandbox (6.6.5). It never passes an "always" option of an agent to the game. A rule applies at the level of the run.
- Each tool call of the agent also becomes a progress line in `Live.lua`, for the activity panel. A run shows its level line first, and then its last 4 lines.

**The level in the game** (decided with an advisor on 2026-09-26). The bridge runs a chat at the lower of its level and the `permission` of the config (S6). In the first test in the game, the header said "Claude · auto-edit", but the run was at `ask`. So the game now shows the level that applies:

- When a run starts, the bridge writes its level line as the first progress line of the run: "Level: auto-edit", or "Level: ask (config)" when the config lowered the level. It writes the line at every run, so a raised config also clears an old "(config)".
- The line stays first while the agent adds steps. `Activity` keeps it and the last 4 lines of the agent, so the proved writer of S20 still gets at most 5 lines, and S9 and S20 do not change.
- Only the bridge writes a line that starts with "Level:". `Activity::step` is the one place where agent lines come in, and it puts "agent: " in front of such a line.
- The addon takes the level only from the first line of the progress of a working message, and only when that line is one of the exact texts of the bridge. It keeps the level with the chat, in the saved variables. The header then shows it, for example "Claude · ask (config)". Before the first run, the header shows the level that the chat asks for.
- A lowered run also starts its reply with "(Ran at ask: the config allows at most ask.)", because a short run can end before the addon loads a slot. The addon does not read this note: after rendering, an agent can write the same text.
- The run timeout stops while the run waits for a permission answer. A separate `permission_timeout_minutes` applies (default 10). After it, the bridge answers "cancelled".
- If the game closes or reloads, open requests stay in the next publish until they time out.
- Stop ends an open request as "cancelled", and the run as "Stopped.".

**A new message ends a wait** (asked for by the user, 2026-09-26, with an advisor). While a run waits for an answer, in the game popup or on the desktop, a new message of that chat:

- ends the waiting request. The desktop dialog closes, and the agent gets "The player sent a new message.";
- stops the turn as Stop does, so the old message ends as "Stopped.";
- runs next, and resumes the same agent session, so the agent sees the refused call and the new message.

Rules:

- Only a message that the bridge newly accepts counts. A duplicate, a refused message (full body, full queue, or the rate), a list, an attach, and a message of another chat never end a wait.
- While a run works and waits for nothing, a new message waits in the queue, as before. Else each follow-up would end a long run.
- A raise that a new message ends counts as no answer (the quiet time of 10 minutes starts).
- This gives an addon no new power: Stop already ends a run, and the cancel only ever answers no.
- The other waiting messages of the chat keep their order.

### 9.4 Agent processes

- ACP: one agent process per run. ACP agents have no sandbox, so each of their calls asks at most (6.6.4).
- `claude`, `codex`, and `command`: one process per run. For `command`, the process is the sandbox, with the harness and every program that it starts.
- `max_parallel_runs` counts active runs, not processes.
- If an ACP process stops, the bridge starts it again and resumes the open sessions. If a session cannot resume, the bridge reports an error for that chat.
- Stop for `command` kills the whole process group at once, with no grace: a harness has no cancel channel. On Linux the sandbox has its own process ids, so every program of the harness ends with it.
- The bridge declares ACP client capabilities `fs` and `terminal` as false in v1. The agent uses its own tools.
- `process.rs` starts every agent process: never through a shell, with the allowlist of 6.2 rule 12, a limit of 8 MiB on each line, and the last 2 KiB of stderr for an error. `turn.rs` holds the run timeout, Stop with its 10-second grace, and the wait for an answer from the game. ACP, `claude`, and `codex` share them.
- If an agent needs a login, the bridge reports "agent needs login" in the game. The bridge never handles credentials.
- The bridge removes `CLAUDECODE` from the environment of each child process. It sets `GNOMISH_RELAY_JOB=1` (section 10).

### 9.5 Sessions and folders

Claude stores sessions per project folder.
If a chat changes folder, the bridge starts a new session for it.
The bridge stores the folder of each session in `state.json`.

### 9.6 Resume a session

The player can continue a saved session of an agent in the game, for example a Claude Code session from a terminal.
The bridge cannot join a session that runs in a terminal: the terminal owns its input. So the game continues the saved session instead.

**The list.** **Resume** in the window sends a `list` record. The bridge asks each agent of the config for `session/list`, when the agent offers it, and answers with one line per session:

```
agent \t session \t age in seconds \t 1 if active \t chat \t folder \t folder name \t title
```

- Only a session whose folder is inside a root shows (6.2, rule 1). The others stay hidden, with no count.
- The list holds the 30 newest sessions. A title is at most 100 bytes, and control characters become spaces.
- `folder` is relative to the base folder. The game sends it back as the folder of the chat, and it resolves to the same folder. On Windows, the game cannot send an absolute path (7.1.1).
- `chat` names the game chat that already has the session. A click on that row opens the chat, not a second one.
- A session that changed in the last 5 minutes is active: it is probably open in a terminal.
- An agent whose list fails is left out. When every agent fails, the reply is an error.
- The addon keeps the last list in its saved variables, so the picker opens at once. A newer list replaces it.

**The attach.** A click on a session makes a new chat with the title, the agent, and the folder of the session. Its first message has the `attach` flag and no text.

- The bridge accepts only a session of its last list. So the folder check of the list guards the attach too.
- An active session gets `session/fork`: the chat continues a copy, and the terminal keeps the original. With no fork, the chat continues the session itself.
- The bridge replays the session with `session/load`, and answers with the last exchange: the last prompt on the first line, and the last answer below it. The addon shows them as history, with no whisper.
- The chat then works as any other chat. Its next message resumes the session (9.5). The level ceiling of the config applies (S6).
- A delete of the chat (7.1.1, `d`) never deletes the session. A later Resume brings it back.

**Claude Code sessions (`kind = "claude"`).** `claude -p` has no list call, so the bridge reads the session files of Claude Code, with the rules of `listSessions`, `getSessionMessages`, and `forkSession` of the Claude Agent SDK. No model call happens. The code is in `claude_sessions.rs`.

- The files are `<config>/projects/<folder>/<session id>.jsonl`. `<config>` is `CLAUDE_CONFIG_DIR` if the `env` list of the entry names it, else `~/.claude`: the agent sees the same folder.
- Only a file with a UUID name counts. The list reads the 60 newest files by time of change, and 64 KiB from each end of each file.
- The title is the newest `customTitle`, then the one in `<session id>/custom-title.json`, then `aiTitle`, `lastPrompt`, `summary`, and the first prompt. The folder is the newest `relocatedCwd`, else the first `cwd`. The time is the time of change of the file.
- A file whose first line is a subagent line, or that has no title or no folder, is left out. So is a session that went on in another file (`continued-in`).
- The attach reads the last 8 MiB of the file. It takes the chain of the newest leaf by `parentUuid`, the last real prompt on it, and the text of every assistant message after that prompt. Tool results, notes of Claude Code in a tag, and slash commands are not prompts.
- The fork writes a copy next to the file, in mode 0600, as `forkSession` does: a new session id, a new uuid for each entry, a `forkedFrom` note, no progress or subagent entries, and the title with " (fork)". The file must be at most 64 MiB. A live test resumed such a copy with its history.

**Codex threads (`kind = "codex"`).** `codex app-server` has the calls that the list and the attach need, and none of them reaches the model:

- The list is `thread/list`, newest change first, with the threads of the terminal, the IDE, `codex exec`, and the app server (`sourceKinds`). The title is the `name` of the thread, else its `preview`. The time is `updatedAt`, in seconds.
- The attach reads the newest turn with `thread/turns/list` (`limit` 1, `itemsView` `full`): the last `userMessage` is the prompt, and each `agentMessage` after it is the answer.
- The fork is `thread/fork`. The chat continues the new thread.

### 9.7 A second app: Timeways

Timeways is a separate story addon (`~/Documents/Code/Personal/timeways`). It uses this bridge as its desktop program: the same strip, the same slots, and the same proofs, with its own key, its own slots, and its own lane. This section is the approved plan (2026-09-25). A reviewer checked it, and the user approved every decision below.

**Status (2026-09-26):** steps 1 to 8 are done, with 5b. Step 6 runs the model calls of the story program with no tools, with the budget of decision 10 (details there, and the protocol in 9.8). The Timeways lane is on only when `timeways.key` exists in the config folder. Since step 8, setup makes that key only when the Timeways addon folder exists (11.3), so an install with no Timeways works exactly as before. The story program runs only with the Timeways lane and a `[story]` section with a `program` in the config (12). Without `[story]`, the lane answers each message with "Timeways story program not running." in its own slots. The app protocol is in 9.8, and the story sandbox in 6.6.4. The loopback of step 5 runs in the fake game of the tests: the shared Lua transport sends a real strip with a batch of the Timeways addon, the fake story program answers, and the Lua slot poll reads the answer from `Timeways_S0001`. A loopback in the real game waits for a Timeways addon build. The story program of the Timeways repo speaks the same shapes, and the fake story program copies them. An end-to-end test runs the real story program with the real bridge (9.8). Since step 5b, the test addon sends, retries, and polls through `Messages.lua` (13.2), behind the seam of the Timeways addon: `ns.Link = { Fits(text), Send(text) }`, and one call for each final reply. Since step 7, the two addons take turns for the strip corner (7.1.2).

**Decisions:**

1. **Keys.** The relay key stays `strip.key`. The Timeways key is `timeways.key`, in the same config folder. The bridge refuses to start if the two keys are the same.
2. **Routing (S29).** The bridge checks the tag of each strip under both keys. One key verifies: the strip goes to that app. No key verifies: `BadTag`. Both verify: `Ambiguous`, and the bridge drops the strip and logs it. S29 proves this choice, not the cryptography. In the bridge, the keys are a `KeySet { relay, timeways }` struct, not a list, so an index cannot swap the apps.
3. **Outbox frames.** A frame in the saved variables of one app counts only if it verifies under that app's key. Any other frame is refused.
4. **One lane for each app.** Each lane has its own replay store, state file, rate limit, slot window, saved-variables watch, reload inbox, tokens, and restore. The Timeways lane holds no agents in its type, so a Timeways strip can never start a coding agent. Its state lives in `<data>/timeways/state.json`. The relay state stays where it is. The Timeways slots are `Timeways_S0001` to `Timeways_S1000`, and its saved variables file is `Timeways.lua` (global `TimewaysDB`). The lane publishes only when those slot folders exist.
5. **Names for each app.** The slot, restore, and live files set a Lua global whose name depends on the app, for example `GnomishRelay_SlotData` and `Timeways_SlotData`. The strip frame, the slot addon names, and the saved-variables name also differ for each app. S9, S18, and S20 are restated over an `App` enum in `protocol` (approved). One app can then never overwrite a value that the other app is about to read.
6. **Flags.** The flags split into transport flags (`h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, `restored`) and coding flags (`perm=`, `level=`, `agent=`, `attach=`, `list`, `d`, `n`, `stop`). The Timeways lane parses the transport flags only. A Timeways record with a non-empty `cwd` is refused: it counts as seen, and its reply is the error "Timeways takes no folder.".
7. **Restore.** Timeways has no restore bundle. The story state lives on the desktop, so the addon rebuilds from there. A Timeways hello never starts a relay restore and never retires a relay token.
8. **The story program.** The bridge starts `timeways-story` when the Timeways key exists, from a path in the config (never a `PATH` lookup), with no shell and the environment allowlist of 6.2. It talks JSON lines over stdin and stdout, with a size limit on each line, a version handshake, and a timeout for each request. The bridge checks each message against a fixed shape. The bridge writes all files that the game reads.
9. **The story sandbox.** The story program reads hostile text: records from any addon, other players' names and messages, and model answers. So it runs in the sandbox of 6.6.4. It writes only `<data>/timeways/`, has no network, and cannot read the `deny` and `desktop` paths. On Windows there is no sandbox yet: Timeways runs, and the bridge shows a one-time warning. (Step 5: the story program writes only `<data>/timeways/story/`, because `<data>/timeways/state.json` holds the replay store of the lane. The same warning shows on a Linux with no working `bwrap`.)
10. **Model calls.** The story program asks the bridge for a model call over the app protocol. The bridge runs the model with no tools and returns only text.
    - **Claude:** `claude -p --tools "" --strict-mcp-config`, with the flags that load no user or project settings (checked live), in an empty private temp folder for each call. The `PreToolUse` gate denies every tool on this route, and the check on tool results stays on.
    - **A local model** (Ollama, LM Studio): through `curl` with `-q` first, `--proto =http`, `--max-redirs 0` and no `-L`, `--noproxy '*'`, `--max-time`, and the prompt through stdin (`--data-binary @-`), never in the arguments. The bridge limits the size of the answer while it reads it. The config accepts only `127.0.0.1` and `[::1]`, not `localhost`. The answer is hostile text, like an agent reply.
    - **Budget.** The bridge enforces a budget of calls for each app with the proved limiter of S14. A hostile addon cannot spend the model subscription faster than that.
    - **Step 6, as built.** `crates/bridge/src/model.rs` holds the open calls and the budget, `model_claude.rs` the Claude route, and `model_local.rs` the local route. Each call runs on its own thread, with the timeout `[story] model_timeout_seconds` (12). A stop of the story program, and the end of the bridge, end every open call: the bridge kills its `claude` or `curl` process at once. An answer of a call that ended so never reaches the next story program.
    - **The Claude flags, checked live on Claude Code 2.1.283.** The command is `claude -p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --tools "" --strict-mcp-config --setting-sources "" --safe-mode --disable-slash-commands --no-session-persistence`, plus `--model <claude_model>` when the config names one. With these flags the `init` message lists no tools, no MCP servers, no skills, no slash commands, and no plugins of the user. A `UserPromptSubmit` hook of the user does not run. A `CLAUDE.md` in a parent folder and a `CLAUDE.local.md` in the folder do not reach the model. `--setting-sources ""` alone keeps out the plugins and the skills of the user and the `CLAUDE.md`. `--safe-mode` alone keeps out the skills of the user and the `CLAUDE.md`, but a plugin of the user stays. The bridge uses both, and `--disable-slash-commands` also removes the built-in skills. `--bare` keeps out the same, but it also skips the OAuth login, so the bridge does not use it. The `PreToolUse` hook of the `initialize` request still fires with these flags: with `--tools Read`, a read of a file reached the hook, and the deny of the hook stopped it. The prompt goes in as one stream-json `user` message on stdin, never in the arguments. The folder of each call is new, empty, mode 0700, and removed after the call. The environment is the allowlist of 6.2 rule 12. `command` is always `claude` from `PATH`, never the command of a relay agent.
    - **The gate on this route** answers every `PreToolUse` hook and every `can_use_tool` request with a deny ("The story program gets no tools."). The check on tool results stays on: a tool result with no error for a call that the hook never saw stops the call, and the call fails.
    - **The local route.** The command is `curl -q --proto =http --max-redirs 0 --noproxy * --max-time <seconds> --silent --show-error --fail --header "Content-Type: application/json" --data-binary @- <local_url>/v1/chat/completions`, with no shell and the environment allowlist of 6.2. The body is `{"model": <local_model>, "messages": [{"role": "user", "content": <prompt>}], "stream": false}`, on stdin. Ollama and LM Studio both serve this path. The answer is the `content` of the `message` of the first item of `choices`. `--fail` makes a status of 400 or more a failed call. `curl` never follows a redirect, so a 3xx fails: its body has no `choices`. The bridge reads at most 256 KiB of the output of `curl`. At the limit it closes the output, and the call fails.
    - **Size limits.** A prompt is at most 256 KiB (9.8). An answer text is at most 16 KiB: the bridge removes every control character but a newline and a tab, and cuts a longer text at a character. The longest text of a reply of the story program is 8 KiB, so 16 KiB leaves room.
    - **Open calls.** At most 2 calls of the story program are open at once. A third call, a call with the number of an open call, a call over the budget, and every call with no model get `model_failed` at once.
    - **The budget, as built** (decided with an advisor on 2026-09-26). The bridge limits the model calls of each app with the proved limiter of S14, and changes no proof. The limiter counts time in steps of W seconds, where W is `[story] budget_window_minutes` (default 20, 1 to 1440). It admits at most 10 calls in any 60 steps. In real time, at most 10 calls start in any window of W minutes less one step. With the default, at most 10 calls start in any 19 minutes 40 seconds, about 30 in an hour. The time comes from a clock that never goes back, because S14 needs the times in order. Only a call that runs counts: the bridge checks the model, the open calls, and the call number first, and the budget last. A restart of the bridge resets the budget. Only the user can restart the bridge.
    - **Why the budget works this way.** S14 fixes 10 messages in 60 time units, and does not fix the unit. A limiter with a parameter needs a new Lean statement. A chain of several limiters needs a claim that no theorem states. A coarse time unit needs neither, so the proof covers the budget as built. The price is a fixed count of 10 in each window: the config picks the window, not the count.
11. **Prompt injection.** Other players' text reaches the prompt. With no tools, it can reach only three things: the text that the user sees (bounded by S10 and S24), the story world (bounded by the rules of the world), and the budget. This is the accepted boundary. Each part has a named test. (Step 5: a Timeways reply is a JSON line, not Markdown blocks, so S24 does not apply to it. The bridge applies the escape of S10 to each text in the reply, and S8, S9, and S12 bound the slot file, 9.8.)
12. **Protected files.** The data folder joins the config folder in the `deny_folders` of the classifier (6.6.3), with a named test for each file in it. The sandbox of 6.6.4 hides it too.
13. **The shared strip corner.** Both addons draw the strip in the same corner, so they take turns through a shared "busy until" value. While an addon waits for the corner, its 40 s retry timer stops. Each addon counts only the screenshot events of its own strip. An addon that cannot get the corner shows "Screenshots blocked by another addon" before its frame reaches the 270 s limit. A Quint model (`models/corner.qnt`) checks this with the timers.
    - **Done (step 7).** The rules are in 7.1.2. An advisor agent and the implementer chose these details (2026-09-26):
      - A blocked app waits, and does not use the outbox. A hostile holder stays across every `/reload`, so the outbox would ask for one reload for each message.
      - The app that waits longest goes next. Without a turn rule, an app that sends often can take the corner again at each release, and no bound holds. The model first had one waiter field. It then found a trace where the second waiter lost its turn, so each app now has its own wait mark.
      - The holder keeps the corner for a 2-second tail after its strip. The model found a trace where a late event of one app ended the strip of the other.
      - The blocked line shows once for each blocked time, not once for each UI session. So a second attack shows too.
      - Each app keeps its own hook for the "Screen captured" text. One shared hook needs a shared flag that a hostile addon can set.
      - The value goes through `rawget` and `rawset`, so a metatable of a hostile addon has no effect.
14. **Shared Lua transport.** `Codec.lua`, `Sha256.lua`, `Strip.lua`, the slot poll, `Health.lua`, and `Messages.lua` move into one source folder with parameters: the app name, the slot prefix, the global names, and the saved variables. The relay repo copies the folder at package time and never commits a copy. The Timeways repo checks its copy with a plain diff against the pinned relay tag.
15. **Setup.** A player with only Timeways gets no folder question and no coding agents, only a `[story]` section in the config for the model. The bridge makes the Timeways slots only when the Timeways addon folder exists. It writes only `Key.lua` into the Timeways folder, and writes it again at start if it is missing. It never writes other Timeways files. (Step 8, decided with an advisor on 2026-09-26. `crates/bridge/src/setup.rs` has the steps, `config_text.rs` the text of the config, and `model_setup.rs` the search for a model.)
    - **Relay on or off.** Setup decides in this order. `--relay` or `--roots` turns the relay on. Then a config decides: on only when it has the relay part (12). Then a `GnomishRelay` folder turns it on, and so does a game with no `Timeways` folder, as before step 8. Only a player with Timeways, no relay folder, and no config gets a question: "Also set up Gnomish Relay, coding agents in the game? (y/N)". With no terminal, the answer is no. Why: no fits a player who came for Timeways, and a relay user with Timeways already has the relay folder or the config.
    - **The config with no relay.** It holds `[wow]` and `[story]`, and no relay key (12). `allowed_roots` alone turns the relay on. The bridge then has no relay lane, and a `Config` holds `relay: Option<RelayConfig>`. Why: an idle relay lane needs a fake policy, and a fake policy is a trap, because an admitted strip then reaches an agent path. Setup still makes `strip.key`, so `KeySet` and S29 need no change. `setup --relay` adds the relay later: its top keys before the old text, because TOML needs them before the first table, and its tables after.
    - **One `--new-key` for every app.** It makes a new `strip.key`, and a new `timeways.key` when the `Timeways` folder exists, and writes both `Key.lua` files. Why: an addon that reads one key reads both (decision 18), so after a leak both change. A new Timeways key is never equal to the relay key, and setup loads both keys at its end, as the bridge does.
    - **`program` and `lore_pack` are optional**, both or neither (12). Setup writes them as commented lines, and searches for nothing. Why: the story program does not ship yet, so a search finds nothing. `// TODO: find timeways-story when Timeways ships it`.
    - **The model.** Setup takes the first model it finds: `claude` on `PATH` (with `claude_model = "haiku"`), then Ollama on 127.0.0.1:11434, then LM Studio on 127.0.0.1:1234. It asks a local server for `/v1/models` with `curl` and the flags of a model call, and takes the first id that is not an embedding model. Another model that it finds goes in as commented lines. With none, `[story]` has no model. Why: `claude` is a deliberate install, its answers are better than a small local model, and the budget of decision 10 bounds its use. `curl` is already the only HTTP client of the bridge.
    - **An existing config.** Setup adds `[story]` to a config that has none when the `Timeways` folder exists, for a relay user who installs Timeways later. It checks every new text with the config loader before it writes. It never changes a key that exists.
    - **`## Group:`** is in neither app's slots. Nothing shows yet that the Forever client reads it. After a test in the game, both apps get it in one commit.
16. **Life cycle.** `restart` and `update` also stop and start the story program. The bridge kills its process group when it exits. A story program that crashes starts again after a backoff.
17. **Versions.** The hello carries the version of each app. A version out of range gets the reply "update the addon". (Step 8, decided with an advisor on 2026-09-26: each message of an app out of range gets one error reply with the text of 7.7. A newer addon of either app gets "Update the desktop program: gnomish-relay update.", an older Timeways gets "Update Timeways.", and an older relay gets the reload text, because the bridge writes the relay addon at each start. The range check is the pure function `version_fit` in `protocol`, with unit tests for every edge and a check in the `flags` fuzz target. S30 proves it (14.1), approved by the user on 2026-09-26. Each app sends its own number through `ns.App.version`, because the batch lines of Timeways and the coding flags of the relay change on their own.)
18. **What the key split protects.** It stops a bug or a hacked story program from reaching the agents through the bridge. It does not stop a hostile addon that loads first from reading either key (6.5).
19. **Paths from game input.** Realm and character names map to safe ids, as S13 does for chat ids. They never become file names directly.

**Checks for each part:**

| Part | Lean | Quint | Fuzz | Tests |
|---|---|---|---|---|
| Routing by key | S29 | | the `frame` and `relay` targets with two keys | all 4 key results, the `KeySet` swap |
| Names for each app | S9, S18, S20 restated | | the `lua`, `restore`, and `live` targets for each app | both apps in one fake game |
| Version range | a small pure function in `protocol` (`version_fit`, S30) | | the `flags` target | the update reply of each app, too old and too new |
| Budget | S14 | | | a hostile addon at full rate |
| Lanes | | | the `relay` target with two lanes | no job from a Timeways strip; no shared seen store, body, or restore |
| App protocol | | | new target `app_protocol` | a fake `timeways-story`: crash, garbage, huge line, hang |
| Model calls | | | new target `model_http` | a fake model server; the gate denies every tool; live tests |
| Story sandbox | | | | its environment; a write outside its folder fails; a network connect fails; a program under `/tmp` starts; a program in a hidden folder is refused |
| Corner | | `corner.qnt` | | two addons in one fake game with a fake clock |
| Shared transport | | | | all addon tests, the golden and differential vectors of 14.3 for both sets of parameters |

The relay tests use a small second test addon built from the shared transport, not the real Timeways addon.

**Order of the build:**

1. The one-lane refactor: the bridge uses one `Lane` type for the relay, with all tests green and no change of behavior.
2. The shared Lua transport with parameters, and the names for each app (S9, S18, and S20 restated).
3. The second key, routing (S29), and the Timeways lane, in one step. No commit has a Timeways key without a Timeways lane.
4. The data folder in `deny_folders`, the flags split, and the outbox rule.
5. The app protocol, the story program with its sandbox, and its life cycle, with a fake echo story program and a test addon: a loopback proved in the game before the real story work.
5b. Shared message logic: the send queue, the signed outbox, retries, `next`, `read`, the hello, the slot poll with reply handling, and the health flags move from `GnomishRelay/Transport.lua` into `addon/transport/Messages.lua`, with `ns.App` parameters, so Timeways and the test addon share the logic that `models/transport.qnt` checks. The relay keeps chats, sessions, restore, live, and popups on top of it. (Done. No transport rule changed, so the model did not change.)
6. Model calls with no tools, and the budget.
7. The shared corner and its Quint model. (Done. `Strip.lua` takes turns through `GnomishStripCorner` (7.1.2), and `models/corner.qnt` checks the rules.)
8. Setup for two apps, and versions. (Done. Setup is in 9.7, decision 15, and the version range in 7.7, with S30.)

### 9.8 The app protocol

The bridge and the story program of Timeways talk in JSON lines: one JSON object on each line, over the stdin and stdout of the story program. The story program is untrusted, like an agent, and so is the addon. `crates/app-protocol/src/addon_lines.rs` checks the lines of the addon, `crates/app-protocol/src/story_lines.rs` has the other messages, and `crates/bridge/src/story.rs` has the life cycle.

**Start.** The bridge starts the story program only when the Timeways lane is on (`timeways.key` exists) and the config has a `[story]` section with a `program` (12). The command line is `<program> <lore pack> <story folder>`. The program path is absolute: the bridge never looks it up on `PATH`. The bridge starts its real path, with no link in it, and refuses a program inside a path that the sandbox hides (6.6.4). The lore pack is the SQLite file of the lore. The story folder is `<data>/timeways/story/`, which the bridge makes with mode 0700. The bridge starts the program with no shell, with the environment allowlist of 6.2 rule 12, with the story folder as its working folder, and in the sandbox of 6.6.4. On Linux and macOS the story program leads its own process group. On Windows, `taskkill /T` stops its process tree.

**What the story program writes.** Only files below its story folder, for example `worlds/<realm id>/<character id>.jsonl`. Each id is a safe encoding of a name from the game: `[A-Za-z0-9]` stays, and every other byte becomes `_XX` in hex (9.7, decision 19). The bridge writes every file that the game reads, with the proved writers (S9, S12). The story program never writes a slot file. Problems go to its stderr.

**Batches from the addon.** The Timeways addon sends one message for each batch, in its one chat. The text of the message is JSON lines: an optional `character_entered` line first, then game events, then at most one line with a reply, last. The bridge checks each line:

- One JSON object, at most 4 KiB, with a string `type` of 1 to 32 bytes of `[a-z_]`.
- No `id` key. The bridge adds `id`, never the addon.
- No control character in any string or key. A line break is a control character, so an addon joins game text onto one line before it sends it. Nesting depth at most 4 (a flat object is 1). At most 64 keys in all.
- Three types keep an exact shape (`deny_unknown_fields`, a key twice is an error):

| `type` | Fields | Checks |
|---|---|---|
| `character_entered` | `realm`, `name` | `realm` at most 64 bytes, `name` at most 48 bytes |
| `lore_asked` | `at`, `question`, `target` (optional) | `question` at most 1 KiB, `target` at most 128 bytes |
| `journal_asked` | `page` (optional, default 0) | |
| `talk_asked` | `at`, `npc`, `text` | `npc` 1 to 64 bytes, `text` at most 255 bytes |

- `lore_asked`, `journal_asked`, and `talk_asked` are the lines with a reply.
- Any other `type` is a game event with no reply, for example `zone_entered`, `npc_met`, `level_reached`, and `npc_defeated`. The story program checks its fields, and ignores a type that it does not know. So a new event of Timeways needs no change in the bridge.

A line that fails a check is dropped and logged. The batch is refused with an error reply, and none of its lines go on, when its character line is too long or holds a control character ("The realm or the name of the character is too long."), or when its order is wrong: a second character line, a character line that is not first, or a line with a reply that is not last ("The lines of the batch are in the wrong order.").

**Messages from the bridge.** Each line that the bridge sends is made from the checked value, never from the raw bytes of the addon.

| `type` | Fields | When |
|---|---|---|
| `hello` | `protocol` (the version of the bridge, now 1), `app` (`"timeways"`) | The first line after each start |
| each line of a batch | its own fields, and `id` (a number from 1 up, the same for each line of one batch) | For each batch that the lane marked as seen on disk |
| `batch_end` | `id` | After the last line of a batch with no line with a reply. A line with a reply ends its batch itself, so each batch gets exactly one answer line. |
| `model_answered` | `call`, `text` | The answer of the model to a `model_call`: at most 16 KiB, with no control character but a newline and a tab (9.7, decision 10) |
| `model_failed` | `call` | A `model_call` with no answer: no model, too many open calls, over the budget, a timeout, or a failed call |

The bridge sends each batch as soon as the story program is ready. A batch that waits for its answer never holds up the next one: answers match by `id`. The limits of each chat (the queue cap of S14, and 30 records in the body) bound the batches that wait.

**Messages from the story program.** Each line must have one of these shapes, with `deny_unknown_fields`: an unknown type, an unknown field, a missing field, a field twice, or a value of the wrong type refuses the line. The `journal` is the one exception: its other fields are bounded JSON (below).

| `type` | Fields | Checks |
|---|---|---|
| `hello` | `protocol` | |
| `lore_answer` | `id`, `text` (a string or `null`), `passages` (a list of `text` and `source`), `narrator` (optional) | `text` at most 8 KiB, at most 8 passages, each `text` at most 4 KiB and each `source` at most 512 bytes. No control character but a newline and a tab. |
| `journal` | `id`, `page`, `pages`, `narrator` (optional), and any other fields | `page` and `pages` are integers from 0 up, and `page` is below `pages` unless `pages` is 0. The other fields are bounded JSON (below). |
| `talk_answer` | `id`, `npc`, `text` (a string or `null` when no model answered), `narrator` (optional) | `npc` at most 64 bytes. `text` at most 1600 bytes (400 characters), on one line, with no control character. |
| `events_seen` | `id`, `narrator` (a string or `null`) | The answer to a batch of game events only |
| `model_call` | `call`, `prompt` | `prompt` at most 256 KiB. |

**Model calls.** A `model_call` can come at any time, also when no batch waits, for example the call of the bard for a saga after `events_seen`. The bridge ties it to no batch and answers it by its `call`. A call that belongs to no batch gives no reply to the game. At most 2 model calls of the story program are open at once (one of the narrator, one of the bard); a third one gets `model_failed` at once. A call stays open while the model runs, off the main loop, until its answer, its timeout, or a stop of the story program. The model has no tools and returns only text. The budget, the size limits, and the two routes (Claude and a local model) are in 9.7, decision 10. With no model in `[story]`, every call gets `model_failed` at once, and the story program still answers with its passages. The story route never reaches an agent, a job, or a chat of the relay (9.7, decisions 4 and 18).

**The journal is bounded JSON.** The journal of the story program grows often, for example with chapters, the trust of a person, and new kinds of deeds. So only `type`, `id`, `page`, `pages`, and `narrator` of a `journal` line have a fixed shape, and a new field needs no change in the bridge. The other fields must hold only objects, arrays, strings, integers, `null`, and booleans. The line is depth 1, and the depth is at most 6. Each string is at most 1600 bytes, with no control character. Each key is 1 to 32 bytes of `[a-z_]`. An object holds at most 64 keys, and an array at most 200 items. A `note` key is refused, because the note of a reply is the bridge's own. The bridge writes the journal again from the checked value, with every `|` doubled (S10). A string over its limit is a text error; every other failed check is a shape error. The whole line is at most 24576 bytes, as for every answer.

`narrator` is a line of the narrator, the voice of the chronicle. It is at most 1000 bytes, with no control character. A longer one, or one with a control character, is dropped and logged, and the rest of the answer stays.

**Replies.**

- A batch with a `lore_asked`, `journal_asked`, or `talk_asked` line waits for the `lore_answer`, `journal`, or `talk_answer` with its `id`, for the request timeout. A batch of game events only waits for `events_seen`, for 60 seconds (or the request timeout, if that is shorter).
- The done reply is one JSON line that the bridge makes from the checked answer, with no `id`, and always with `narrator` (a string or `null`): for example `{"type":"events_seen","narrator":null}`. The bridge doubles every `|` in each text of it (S10), so the game shows the text as it is. The addon shows these texts with no escape of its own.
- A batch of game events never gets an error. At its deadline, or when the story program stops, is refused for its version, or does not start, it gets a done reply with an empty text.
- An answer for an `id` that already ended, for example an `events_seen` after its deadline, or a second answer line with the same `id`, is late: the bridge drops it and logs it. An answer of the wrong type for its batch is a bad line.
- An answer line of more than 24576 bytes gets the error reply "The Timeways answer is too long for the game.", never a cut line. So does a reply that the slot writer would cut (S12). A reply record holds at most 32 KB after the Lua escape, and the Lua escape writes 4 bytes for some bytes, so the bridge checks the real size too.
- An error reply is plain text.

**Rules:**

- A line from the story program is at most 1 MiB. The reader skips the rest of a longer line.
- A line that fails a check is a bad line. The bridge logs it and skips it. An answer for an `id` that the bridge never gave is a bad line too. More than 10 bad lines in one run of the story program stop it.
- **Handshake.** The story program answers the `hello` of the bridge with its own `hello` within 10 seconds (or the request timeout, if that is shorter). Any other line first is a bad line. No `hello` in time stops the story program.
- **Versions.** The story program compares nothing; the bridge compares. A story program with a higher `protocol` gets "Update the desktop program: gnomish-relay update." as the answer to each batch. A lower one gets "Update Timeways.". The bridge stops it, logs both versions, and does not start it again until the bridge restarts: a restart cannot fix a version.
- **Timeout.** A batch with a reply line has the timeout of `[story] timeout_seconds` (default 120 seconds), from the time that the lane gives it to the story program. With no answer in time it ends with "The Timeways story program did not answer in time.". Such a batch that was sent with no answer in time means a hang: the bridge kills the story program. A batch of game events with no `events_seen` in time is no hang.
- **Stop.** The bridge kills the process group of the story program when a hang, a crash, or too many bad lines stop it. Each sent batch with a reply line then ends with "The Timeways story program stopped.", and each sent batch of events with an empty done reply. A batch that is not sent yet waits for the next start.
- **Restart.** After a stop, the bridge starts the story program again after 1 second, then 2, 4, and so on up to 60 seconds. After a run of 60 seconds or more, the wait starts at 1 second again.
- **End of input.** The story program exits when its stdin closes: the bridge is gone. With `bwrap`, `--die-with-parent` also stops it. `gnomish-relay restart` and `update` restart the bridge, so the story program starts again with it. A systemd service stops the whole control group. On macOS and on a Linux with no `bwrap`, a bridge that a signal kills cannot kill the process group, because the bridge has no signal handler (it forbids `unsafe`). There the story program depends on the end-of-input rule.
- **No sandbox.** With no sandbox (6.6.4), the first reply with text after the bridge starts carries the warning: a `note` field in a JSON reply, or a second line in an error reply.
- **Logs.** Each log line of the story program starts with `timeways:`. The last line of its stderr goes into the log after a crash, with the escapes of 6.2 rule 15.

The tests use a fake story program, `crates/bridge/src/bin/fake-story.rs`, with scripts: echo (a `lore_answer` of "story: <question>", the journal with a chapter and the three kinds of deeds, a `talk_answer`, and `events_seen`), a `null` text, two answers with one `id`, three bard calls after `events_seen`, `events_seen` and answers with a narrator line and with one that is too long, a late `events_seen`, a missing `events_seen`, crash, crash once, garbage lines, a flood of bad lines, a huge line, an answer line one byte over the limit, a hang, no hello, a higher and a lower version, answers for unknown ids, an answer for another id, a model call whose answer becomes the lore text, a model call and then a crash, its environment, a child process, and probes of the sandbox. It writes each line that it gets into `seen.txt` in its folder, and the `id` of each `batch_end` into `ends.txt`.

The model calls have tests of their own (`crates/bridge/tests/model.rs`). A fake model server (`tests/fake_model/`) is a thread with a `TcpListener` on 127.0.0.1 that answers like the OpenAI-compatible API: a normal answer, a slow answer, a huge answer, garbage, a redirect, a 500, and control characters. The tests show that `curl` never follows the redirect, that the huge answer fails, and that a proxy in the environment of `curl` gets no connection. The fake `claude` tries a tool on the story route and gets the deny of the gate, runs a tool with no hook and stops the call, and reports its folder, its arguments, and its environment. Two `#[ignore]` live tests run the real `claude` with a prompt that asks it to read a file (the answer never holds the text of the file), and a real Ollama when one listens on 127.0.0.1:11434.

**The end-to-end test** (`crates/bridge/tests/timeways_e2e.rs`) runs the real bridge with the real `timeways-story` of a Timeways checkout, with no game. The bridge starts the story program from a `[story]` config, in its real sandbox. The test builds a small lore pack from invented passages with the real `timeways-pack`. The test addon of the fake game sends batches that `Json.lua`, `Inputs.lua`, and `Outbox.lua` of the Timeways addon make: the character, four game events, and a question; then a journal request; then a talk; then game events only. The Lua slot poll reads each reply. The test checks the passages and their sources, the spoiler limit, the journal, the done reply of the events, the failed model calls with no model, the files of the story program, and an empty relay lane. A second case asks the real `claude` for words. Both cases are `#[ignore]`, and they skip with no Timeways checkout, so CI does not run them yet. To run them:

```sh
scripts/e2e-timeways.sh            # TIMEWAYS_REPO is the checkout; the default is ../timeways
scripts/e2e-timeways.sh --claude   # also the case with the real claude
```

The script builds `timeways-story` and `timeways-pack` into `target/timeways`, so it never shares a build folder with the Timeways repo.

### 9.9 Choose the folder of a chat

A new chat starts in `default_cwd`. The folder in the chat header is a button. A click opens the folder browser in the center of the window (13.1). The browser shows a tree of the folders inside `allowed_roots`.

**The request.** Each time the browser opens, the addon sends a `list=folders` record of the chat `folders`. The browser shows the last tree at once, and a small spinner turns until the new tree comes. The bridge walks the roots in a thread, off the main loop (`folder_walk.rs`), and answers with the tree (`folder_list.rs`).

**The reply.** The first line is `default_cwd` as the player reads it. Then comes one line per folder, breadth first:

```
parent \t name \t mark
```

- `parent` is the line number of the parent folder. The first folder is on line 1. A root has parent 0.
- `name` is the name of the folder. A root has its whole path as its name.
- `mark` is `g` for a git repository, else empty.
- A last line `+` says that the tree is cut.
- A path in the home folder starts with `~/`, for example `~/Documents/Code`. This form is used only when `default_cwd` and every root are in the home folder. Else every path is whole. So the addon can compare the parts of any two paths.

**The folder of a line.** The addon joins the path of the root and the names down to the folder. The folder that the game sends back is the path from `default_cwd` to that folder, with `..` for each step up. This is the form of 9.6 and of `relative_folder`, so one folder always has one text. The folder resolves again through the resolver (S5), and it resolves to the same folder.

**The walk.**

- Breadth first, with sorted names, so a limit cuts off the deepest folders, and the order is the same on each run.
- At most 4 levels below a root. A root is level 0.
- The walk goes into a repository, so the browser can show its subfolders.
- It never follows a symbolic link or a Windows junction. Two roots that overlap give each folder once. A root stays a root.
- It skips hidden folders (a name that starts with `.`) and `node_modules`, `target`, `build`, `dist`, `vendor`, `venv`, `__pycache__`, `Library`, and `AppData`.
- It reads at most 3000 folders, and stops after 2 seconds. A stop at one of these two limits cuts the tree. The depth limit does not.
- A repository is a folder with a `.git` entry: a folder, or a file as in a worktree. The walk never reads the `gitdir:` line of such a file, because it can lead out of the roots.
- The walk asks the classifier (6.6.3) for a read of each folder, with the folder as the chat folder. Only a folder with the answer `allow` shows, and a folder that does not show hides its subfolders. So the config folder and the data folder of the bridge (`deny`) and credential folders such as `snap/firefox` (`desktop`) never show. This is a filter of the tree, not a wall: the classifier still checks every tool call in the chat.
- A folder that the game cannot send back is left out with its subfolders: a relative path with a control character, a path that is not UTF-8 or longer than 255 bytes, a name that fails the name rules below, or a `:` on Windows (7.1.1).
- The walk never fails. A folder that it cannot read is left out.

**The size cut.** The reply is one record, at most 32 KB after the Lua escape (S12). A tab, a newline, and a byte that is not printable ASCII cost 4 bytes there. The bridge keeps the longest breadth-first start of the tree that fits, and adds the `+` line. So the shallow folders always come.

**The browser.**

- A filter box at the top has the focus when the browser opens. It has no hint text. It matches the folders of the tree, as the game sends them back, by subsequence and without case. Repositories come first, then the shorter paths, at most 16 rows. Each row shows the name, a `git` mark for a repository, and the parent folder in grey at the right. Up and Down move the choice. Enter picks it. Escape clears the focus and closes the browser, so the keys of the game work again. A typed text is only a filter, never a path.
- With an empty filter, the browser shows at most 5 recent folders: the folders of the newest chats, then the folders of the Resume list (9.6). They need no request. A folder that the last tree does not have is gone, and it does not show. One click on a recent folder sets it.
- Below them is a gold breadcrumb, for example `Code › Personal › gnomish-relay`. A click on a part goes up to it. The first part is the root, so the player cannot go above the roots. With more than one root, the first part is "Roots", and it lists the roots.
- Then come the subfolders of the current folder. A click opens one. The folder of the chat is green. **Open** sets the current folder.
- The last row is "New folder". It opens an edit box in its place. The addon checks the name: it is not empty, `.`, or `..`, it has no `/`, `\`, or control character, it is at most 255 bytes, and no subfolder there has the name (without case). A refused name shows a short reason in red. Enter sets `<current folder>/<name>` as the folder of the chat, and the header marks it "new".
- The browser opens at the folder of the chat, or at the default folder when the tree does not have it.

**The chat.**

- A choice sets the folder of the chat. The chat takes the name of the folder, with " 2", " 3", and so on when another chat has the name. A chat in the default folder keeps its "Chat N" name.
- The first message fixes the folder (9.5). After it, the button of the browser says "New Chat here", and each choice makes a new chat in the chosen folder, with the agent of the chat.
- The header shows the folder as the player reads it, with the folder icon and the dropdown arrow. Before the first tree, it shows the relative folder, and nothing for the default folder.

**A new folder.** The first message of a chat in a new folder has the flag `mkdir=1` next to `n`. The folder of the record is the new folder. The bridge makes it before the run starts, so a chat that never sends leaves no empty folder.

- The bridge takes `mkdir=1` only with `n`. On any other message, the flag does nothing.
- The relay refuses a new folder whose record folder is absolute, or whose last part fails the name rules. The reply is "Folder not made: bad name.". The folder check of 6.2 rule 1 comes first, as for every message.
- When the run starts, the bridge makes only the last part, with `create_dir`, never `create_dir_all`. The parent must exist. The bridge resolves the parent with `canonicalize` and checks the roots again, so a link in the path cannot lead out (6.2, rule 10). The new folder must get `allow` from the classifier, as in the walk, so a folder inside a `deny` folder or a `desktop` path is never made.
- A folder that is already there is fine: a run after a bridge restart asks again. A file with the name is an error.
- An error ends the message with a reply that starts "Folder not made:", and the run never starts. Each refusal has its own reason.

**Decisions.** The implementer and the coordinator chose these (2026-09-26). The advisor agent did not answer in time, so the coordinator gave the defaults.

1. **`list=folders`, in its own chat `folders`.** The addon routes the reply by chat, so a reply after `/reload` still finds its cache. The bridge runs one job per chat, so a session list and a folder list can run at the same time.
2. **No version change (7.7).** The bridge writes the relay addon again at each start (11.3), so the addon is never newer than its bridge. A version change would also change the proof of S30.
3. **A compact tree, not one full path per line.** A name costs fewer bytes than a full path, so more folders fit in 32 KB. The line numbers of the parents only point back, so the parser can never build a loop.
4. **The addon makes the relative folder from the parts of two paths.** A root inside the home folder with `default_cwd` below it would else give two texts for one folder, for example `../../Documents` and `..`. Then the green mark, the recent folders, and the names would disagree.
5. **A breadth-first cut with a mark.** The shallow folders are the ones that a player opens first. The addon shows the cut tree, and the breadcrumb and the filter still work on it.
6. **One breadcrumb root per root.** A player with one root never sees a list of roots. With more roots, the list of roots is the top.
7. **The header shows the path as the player reads it** (`~/Documents/Code`). The old header showed the relative folder, and nothing for the default folder, which a button cannot show.
8. **`mkdir=1` with the folder of the chat, on the first message only.** The record already carries the folder, so a second field would only repeat it. A later message cannot make a folder, so a lost reply never makes one twice in another place.
9. **The filter matches the relative folder.** It is the text that the game sends back. Repositories first, because most chats work in a repository.
10. **Recent folders drop a folder that the tree does not have.** A removed folder would only give an error.
11. **The walk goes into repositories now.** The browser needs their subfolders. The limits stay the same.
12. **The classifier is the filter** of the walk and of a new folder, so the tree, a new folder, and the tool calls never disagree about a folder.
13. **The tree stays text in the saved variables** (`folders.text`). A parsed tree has loops through the parent links, and the game cannot save a loop.
14. **No Refresh button.** Each open asks for a new tree, and the spinner shows the wait.

**The browser** is in 13.1.

## 10. Notifications from terminal sessions

**Status: approved (2026-09-28), mostly not built.** Built: only the pure parts in `protocol`, with their proofs: `notice.rs` (S40), `sessions.rs` (S41), and the notices of `live.rs` (S20 restated). The bridge writes an empty notice list. Not built: the `hook` subcommand, the spool folder, `hooks install`, the notices in the addon, the fuzz targets of 10.7, and `crates/bridge/tests/notices_e2e.rs`. The text below is the plan.

The implementer and an advisor agent with a UX critic view wrote the proposal (2026-09-27). The user approved it with the changes of a UX review: the name "Notifications", a bell at the minimap in place of a tab, and the texts, sounds, and settings below. Each "Why" says what real use showed.

The user plays WoW while Claude Code or Codex works in a terminal. When a terminal session waits for the user, or finishes long work, the game shows a notification: the agent, the repo, the first words of the message, and a sound. A notification carries no command and never starts a run. The user answers in the terminal.

**Decisions, and why:**

1. **A spool folder, not a socket.** One code path on Linux, macOS, and Windows with only `std`, so no `interprocess` crate and no ACL code for a named pipe. A file write never blocks the terminal session. The data folder is already hidden from game runs (6.6.3, 6.6.4).
2. **One notification for each session, and a later event takes it away.** The user often answers at the terminal before the game polls. A "Waiting for you" that shows 3 minutes after the answer teaches the user to ignore notifications. This rule replaces the old merge "within 30 s".
3. **Finished work shows only after long work** (default over 1 minute, a setting). A session that waits for the user always shows. Why: a line after each short turn floods the game chat.
4. **Faster polls only while a terminal session is open.** Signals do not work (7.4), so a notification waits for the next slot poll, and the idle poll is 10 minutes. Faster polls cost slots, so they happen only when a notification can come.
5. **No `notify` change for Codex.** Codex gets its `hooks.json`. Why: `notify` takes one program, and a chain to the program of the user can break it.
6. **No tab.** A bell at the edge of the minimap shows while notifications exist. A tab needs the window, and the user plays with the window closed.
7. **The line names the agent and the repo, never "whispers".** A whisper looks like the chat of an in-game agent, and the user tries to answer it in the game.

### 10.1 The hook command

The hook is a subcommand of the one binary: `gnomish-relay hook claude` and `gnomish-relay hook codex`. The agent starts it for each hook event, and gives the event as JSON on stdin.

- If `GNOMISH_RELAY_JOB` is set, the command exits at once and writes nothing. The bridge sets it for each process of a run (6.2), so the runs from the game never notify, also through a nested `claude`.
- It reads at most 1 MiB of stdin. It takes `hook_event_name`, `session_id`, `cwd`, and the text of the event, and ignores every other field.
- The repo name is the name of the git top folder of `cwd` (the first folder upward with `.git`), else the name of `cwd`. The command never sends the full path, because a notification can show on a stream or a screenshot.
- It writes one spool file (10.2) and exits. It never prints to stdout: the stdout of a `Stop` hook can keep Claude working.
- It always exits 0, also after an error, so a hook never fails a turn. A timer ends the process after 300 ms.
- With no spool folder (no bridge runs), or with 100 or more files in it (the bridge stopped), it writes nothing.

**Events:**

| Agent | Hook | Event | Text |
|---|---|---|---|
| Claude Code | `SessionStart` | `session-start` | none |
| Claude Code | `UserPromptSubmit` | `turn-start` | none (the prompt never leaves the terminal) |
| Claude Code | `Stop` | `finished` | `last_assistant_message`, or "Turn done." when it is empty (a turn that ends on a tool call) |
| Claude Code | `StopFailure` | `failed` | the error text |
| Claude Code | `Notification`, matcher `permission_prompt\|elicitation_dialog\|elicitation_url_dialog\|worker_permission_prompt` | `waiting` | `message` |
| Claude Code | `SessionEnd` | `session-end` | none |
| Codex | `SessionStart` | `session-start` | none |
| Codex | `UserPromptSubmit` | `turn-start` | none |
| Codex | `Stop` | `finished` | the last message of the agent, or "Turn done." |
| Codex | `PermissionRequest` | `waiting` | the command or the request |

- `idle_prompt` is not in the matcher: it fires 60 seconds after each `Stop`, so it is a copy of `finished`.
- `SubagentStop` and `agent_needs_input` (a teammate) give no notification.
- Esc in Claude ends a turn with no `Stop`. The next `turn-start` or `session-end` of the session ends it.
- Gemini CLI: not checked yet (17). Another tool can run `gnomish-relay hook claude` from a wrapper script, with a JSON line of its own.
- The advisor found these names in the binaries of Claude Code 2.1.283 and codex-cli 0.157.0 (2026-09-27). The Codex hooks need the feature flag `codex_hooks` (10.5).

### 10.2 The spool folder

The spool folder is `<data>/notices/`, mode 0700. The bridge makes it at start and removes it at a clean exit.

- **A file** is one JSON object, at most 4 KiB: `{"v":1,"source":"claude","event":"finished","session":"…","repo":"…","text":"…"}`. The name is unique: the process id, the time in nanoseconds, and a counter, then `.json`. The command writes `<name>.tmp` first and renames it, so the bridge never reads half a file.
- **The bridge reads the folder every 250 ms**, with the watch of the saved variables. It takes at most 64 files for each read, oldest first. It deletes each file before it parses it, so a bad file never comes back. It ignores `*.tmp` files, and deletes a `.tmp` file that is older than 60 seconds. It never follows a link.
- **The checks.** The fields are exact (`deny_unknown_fields`, a key twice is an error). `source` is `claude` or `codex`. `event` is one of the events of 10.1. `session` is 1 to 128 bytes of `[A-Za-z0-9_-]`. `repo` and `text` are strings. A file that fails a check is dropped, and the bridge logs one line for it.
- **Every text is untrusted.** Any local process of the user can write a file. So the bridge cuts `repo` to 64 bytes and `text` to 600 bytes, at a character. `notice_text` turns each control character into a space, and removes each bidi, zero-width, and tag character. Every `|` is doubled (S10). The live writer escapes each string (S8). S40 proves the cut and the escape.
- **The time is the bridge's own.** A file has no time field. The bridge takes the time of the read, so the length of a turn never comes from the file.
- **No rate limit.** A flood of files cannot cost slots, because only a poll of the addon costs a slot, and the bridge writes the live file at most every 3 seconds. The session table has at most 32 sessions, so memory stays bounded.

### 10.3 Sessions and notices

The bridge keeps a table of the terminal sessions. Each session has a state and at most one notice. `apply_event` in `protocol` changes the table for each event. S41 proves it.

| Event | The session | Its notice |
|---|---|---|
| `session-start` | open | removed |
| `turn-start` | open, a turn runs from now | removed (the user is at the terminal) |
| `waiting` | open, the turn still runs | a new notice `waiting` |
| `finished` | open, no turn runs | a new notice `finished`, with `took`, the length of the turn |
| `failed` | open, no turn runs | a new notice `failed`, with `took` |
| `session-end` | removed | removed |

- `took` is the length of the turn in seconds, at least 1. It is 0 when the bridge saw no `turn-start`, for example after a restart of the bridge. The addon counts 0 as long work.
- A running turn ends after 30 minutes with no event of its session. An open session ends after 12 hours with no event. So a crash of the agent never keeps the faster polls on.
- With 32 sessions, a new session takes the place of a session with no notice: the one whose latest event is the oldest. Only when all 32 sessions have a notice, it takes the place of the session whose notice is the oldest (by the time of the notice). "Oldest" always means the latest event or the notice time, never the start of the session. (The user chose this rule on 2026-09-28.)
- **A notice id** is the next number of a counter, and never less than the Unix time. The counter lives in `<data>/notices.json` with the table, so a restart of the bridge keeps them. Why the time: after a wipe of the data folder, the new ids never repeat the ids that the addon already showed, as for message ids (13.2).

**In the live file.** The notices ride in `Live.lua`, in a new table after `permissions` (S20, restated):

```lua
notices = {busy = 1, open = 2, list = {
{id = 1790300123, at = 1790300100, source = "claude", kind = "waiting", repo = "gnomish-relay", took = 0, text = "Claude needs your permission to use Bash"},
}},
```

- `busy` is the number of sessions with a running turn, and `open` the number of open sessions.
- `list` holds the newest 20 notices. `at` is the time of the bridge. The addon computes the age from the `now` of the body in the same slot, so a clock difference between the desktop and the game has no effect.
- Why the live file and not a fourth slot file: WoW finds only the files that exist at launch (7.2, rule 1). A new file in the slot TOC needs a new `setup` with the game closed in each install. The live file works in the running game. The notices add at most about 56 KB, so the live file stays below its bound of 256 KiB (S21).
- The live file of Timeways holds an empty `notices` table. The writer stays one template for both apps.

### 10.4 In the game

**The poll.** A notification shows at the next slot poll. The addon sets its `PollEvery` hook (13.2) from the last live file:

| State | Poll |
|---|---|
| A desktop request waits (6.6.3) | every 5 s, as before |
| Notifications on, and `busy` > 0 | every 60 s |
| Notifications on, and `open` > 0 | every 3 min |
| Else | the schedule of 7.3 (10 minutes when idle) |

- Cost: 60 slots in each hour of terminal work, and 20 in each hour with an idle session. The 1000 slots of a UI session then last about 16 hours of terminal work, less the slots of game chats. Diag shows the free slots, and "Reload soon" (7.3) covers the rest.
- The addon learns that a session is open only at a poll, so the first notification of an evening can wait up to 10 minutes.
- Notifications off stops the faster polls, so it is also a way to save slots.

**The list.** The addon keeps the notices of the last live file that pass the filter, less the ones that the user cleared. So an answered notice leaves the list at the next poll: the bridge took it away (10.3). The filter: a `finished` or `failed` notice with a `took` below the setting shows nowhere, not in the list and not in the chat. `took` = 0 passes every setting but Never. `waiting` always passes.

**New notices.** A notice is new when its id is not in the last 64 ids that the addon showed. The saved variables keep these ids, so a `/reload` shows nothing twice. For the new notices of one poll:

- **The chat line.** It starts with a bell icon, and has the color of the reply line (13.1). Each `waiting` notice gets its own line: `[Claude · gnomish-relay] Waiting for you: <text>`. The `finished` and `failed` notices get one line together. With one: `[Codex · lighthouse] Finished in 4 min: <text>`, or `Failed after 2 min: <text>`. With more: `3 agents finished: gnomish-relay, lighthouse, timeways`. The text is its first 120 characters. A click on the line opens the list. It is a link of the addon (`|Hgnomishrelaynotices|h`), never a chat that can take an answer.
- **The sound.** One sound for each poll: the Battle.net toast sound (`SOUNDKIT.UI_BNET_TOAST`) when a `waiting` notice is new, else the whisper sound. Built-in sound kits only, so nothing needs to exist at game start.
- **The toast.** For a new `waiting` notice: a small frame at the bottom left, above the chat frame, as the Battle.net toast. Its first line is `<Agent> is waiting · <repo>`, and below it at most 2 lines of the text. It goes away after 8 seconds. A click opens the list.
- **In combat** (`InCombatLockdown`), the chat line shows at once. The toast and the sound wait until combat ends, and then come only for notices that are still in the list.

**The bell.** A round button with a bell on the edge of the minimap (parent `Minimap`). It shows only while the list holds a notice, and it glows while a `waiting` notice is in the list. The user can drag it along the edge of the minimap, and the saved variables keep its angle. A click opens the list, and a second click closes it.

**The list frame.** A small frame in the style of a tooltip, below the minimap:

- The title "Notifications", and a × that closes it. Escape also closes it.
- One row for each notice, newest first: the agent icon, the repo in gold, the state ("Waiting" in orange, "Finished · 4 min" in green, "Failed · 2 min" in red), the age, and the first words of the text on a second line. A click on a row shows its full text (at most 600 bytes), and a second click folds it.
- **Clear** empties the list and hides the bell. A cleared notice never comes back, also when the next live file still holds it.
- A notice has no button that runs anything. Later: "Continue in the game" through Resume (9.6), with the session of the notice, only after `session-end`, because two programs on one session conflict.

**Settings.** One new group "Notifications" in the Settings tab, after Appearance (13.1). It shows only after `hooks install`: the settings list (13.4) has a `hook` line with `on`.

**Diag.** Three new rows, also only after `hooks install`: Hooks (the state of each agent from the settings list), Sessions (the running and open terminal sessions of the last live file), and Last notification (its age). Diag also shows the free slots.

**Two WoW clients** on one computer each read their own slots, so both show each notification and both spend slots. This is accepted.

### 10.5 Install and remove

`gnomish-relay hooks install [--claude] [--codex]` adds the hooks. With no flag, it adds them for each of `claude` and `codex` on `PATH`. `gnomish-relay hooks remove` takes them out, and `gnomish-relay hooks status` shows them. Their output speaks of notifications. `setup` changes no agent settings. It prints one line at its end: "For notifications from Claude Code and Codex in a terminal, run: gnomish-relay hooks install". Why: the settings of the agents belong to the user, so only an explicit command changes them.

**Claude Code** (`~/.claude/settings.json`):

- The command adds one group to `hooks.<event>` for each event of 10.1: `{"matcher": …, "hooks": [{"type": "command", "command": "\"<absolute path>\" hook claude", "timeout": 5, "async": true}]}`. With `async`, Claude never waits for the hook and ignores its output. The path is quoted, for a space on Windows.
- It keeps every other key and every hook of the user, in their order, with an indent of 2 spaces. So `serde_json` needs its `preserve_order` feature.
- It finds its own groups by the command `hook claude` after a path whose file name is `gnomish-relay`. So a second install changes nothing, and an install after a move of the binary replaces the old path.
- If the file does not parse, or `hooks` or one of its events has another type, it changes nothing and names the key.
- It follows a link to the real file (for a dotfiles folder), and writes the real file with an atomic rename in its folder.
- Before its first change, it copies the file to `settings.json.gnomish-relay.bak`. It never writes over an existing backup, so the backup is the file from before the first install.
- `remove` takes out only its own groups, and an event with no group left. Install and then remove give the same JSON value as before.

**Codex** (`~/.codex/hooks.json` and `~/.codex/config.toml`):

- The command merges its groups into `hooks.json` by the same rules, with `hook codex`. With no file, it makes one.
- The hooks need `codex_hooks = true` under `[features]` in `config.toml`. The command adds the line, and says so. It changes the file as `config_edit.rs` does: it adds one line and changes no other byte. It refuses when the file sets `codex_hooks = false`, because the user chose that.
- It never changes `notify`.
- `remove` takes out its groups, and leaves the feature line. It says so.

**After an install**, the command prints "Restart the Claude Code and Codex sessions that run now." Both load hooks only at the start of a session.

**`hooks status`** shows for each agent: on, off, or on with a path that does not exist (a moved binary). It also shows `disableAllHooks` in the Claude settings, and a Codex config with no `codex_hooks`. The settings list (13.4) carries the same state, so Diag shows it.

### 10.6 Checks for each part

| Part | Lean | Fuzz | Tests |
|---|---|---|---|
| Notice text | S40 | `notice_file` | each character class that goes, the cut at a character, a `\|` |
| Sessions and notices | S41 | `notice_file` (a sequence of files) | each row of the table in 10.3, stale notices, the 32-session limit, both expiries |
| Live file with notices | S20 restated, S21 | `live` with notices | Lua 5.1 reads the file back; Timeways has an empty table |
| Hook input | | new target `hook_input` | each event of each agent with the input shapes of the checked versions, an empty message, 1 MiB of input, no spool folder, a full spool folder, `GNOMISH_RELAY_JOB` |
| Spool reader | | `notice_file` | a half file, a `.tmp` file, a link, 65 files, a file of 4 KiB and 1 byte |
| Settings merge | | new target `hooks_merge` | hooks of the user stay, a second install, a moved binary, a broken file, a link, the backup, install then remove |
| Addon | | | the fake game (10.7) |

### 10.7 Verification plan

**Pure parts in `crates/protocol`** (the Aeneas subset of CLAUDE.md). The user approved these statements on 2026-09-28, and all four are proved: `notice.rs` (S40), `sessions.rs` (S41), and `live.rs` (S20, S21).

- **S40, notice text.** `notice_text(bytes, max)` never panics and returns at most `max` bytes. The output holds no byte below `0x20`, no `0x7F`, and no bidi, zero-width, or tag character. Read in tokens, it holds each `|` only as `||`. It never ends inside a UTF-8 sequence. Lean shape: `∀ (t : Slice U8) (max : Usize), t.val.length ≤ 2 ^ 20 → notice.notice_text t max ⦃ v => v.val.length ≤ max.val ∧ noticeSafe v.val ∧ pipesDoubled v.val ∧ endsOnChar v.val ⦄`.
- **S41, sessions and notices.** For every table that fits and every event, `apply_event` never panics. The result has at most 32 sessions and at most one notice for each session. A `waiting`, `finished`, or `failed` event leaves exactly its own notice on its session. A `session-start`, `turn-start`, or `session-end` event leaves no notice on its session. The other sessions keep their notices, except in a full table where every session has a notice: then only the session with the oldest notice loses it (10.3). Lean shape: `∀ (ss : Slice sessions.Session) (e : sessions.Event) (now : U32), sessionsFit ss.val → sessions.apply_event ss e now ⦃ r => r.val.length ≤ maxSessions ∧ oneNoticeEach r.val ∧ noticeAfter r.val e ∧ othersKept ss.val r.val e ⦄`.
- **S20, restated.** The live file is the fixed template of its app with escaped holes, now with the `notices` table. A new `S20_prepare_notices` keeps the newest 20 notices, cuts only the ends of strings, and makes them fit. Lean shape: `∀ app progress requests notices, fitsLive progress.val requests.val notices → live.live_body app progress requests notices ⦃ v => bytes v.val = liveOf app progress.val requests.val notices ⦄`.
- **S21, the same bound.** A live file that fits is still at most 256 KiB. Only `fitsLive` and `liveOf` grow.

**Fuzz targets:** `hook_input` (the stdin of each agent to a spool file: no panic, at most 4 KiB, one JSON object), `notice_file` (spool bytes to an event or a refusal, then `apply_event`), `live` (now with notices), and `hooks_merge` (any `settings.json` text: the merge never panics; it refuses and changes nothing, or its result parses, holds every key and hook of the input, and holds each group of ours once).

**Unit tests** for each rule of 10.1 to 10.5, with sentence names, for example `a_hook_in_a_bridge_job_writes_nothing`, `a_turn_start_removes_the_notice_of_its_session`, `short_finished_work_shows_nowhere`, and `install_keeps_the_hooks_of_the_user`.

**Fake-game tests** (`crates/bridge/tests/addon_flow.rs`, with the fake WoW API): the hook subcommand writes a spool file, the bridge publishes, and the Lua poll shows the line, the sound, the toast, and the bell. Other cases: nothing twice across a `/reload`, an answered notice that leaves the list, the filter of short work, one line for three `finished` notices, a toast and a sound that wait for the end of combat, the 60 s and 3 min polls, notifications off, Clear, the angle of the bell, and a Settings group that shows only after `hooks install`. A seeded test feeds the addon 600 random live files, as for the settings list (14.4).

**The end-to-end test** (`crates/bridge/tests/notices_e2e.rs`) runs the real bridge and the real binary in a temp home, with no game. `hooks install` merges into a `settings.json` that holds hooks of the user. The test then runs the real `gnomish-relay hook claude` with the stdin of each Claude event, and reads `Live.lua` with the Lua slot poll. It also checks that the hook with no bridge exits 0 in less than 300 ms with an empty stdout. A live test marked `#[ignore]` runs the real `claude -p` with `--settings <temp file>`, so the real `~/.claude` stays the same, and waits for the `finished` notice. A live Codex test waits until a temp `CODEX_HOME` can keep the login.

## 11. Platforms

Only a few paths change per platform. All other code is shared.

| Part | Linux | Windows | macOS |
|---|---|---|---|
| WoW folder | Inside the Wine prefix | `Program Files (x86)\World of Warcraft\_classic_beta_` | `/Applications/World of Warcraft/_classic_beta_` |
| Notifications from hooks (10.2) | A spool folder | A spool folder | A spool folder |
| Replace a file that the game has open | Rename always works | Rename can fail. Retry with backoff, then log. | Rename always works |

### 11.1 Linux notes (the first target)

The development machine runs Wayland with XWayland. The home file system is ext4.

- WoW can run on D3D12 through vkd3d-proton, or on D3D11 through DXVK.
- The bridge finds `Interface/AddOns` and `WTF/Account/<ACCOUNT>` without regard to case. It never makes a second folder that differs only in case, for example `Addons` next to `AddOns`.
- The game makes `Interface/` and `WTF/` only after its first start. The setup step makes `Interface/AddOns` if it is missing.

### 11.2 Other platform notes

- **File system:** any file system works except FAT32 and exFAT. (`wow-claude` says NTFS. That line comes from `wow-forever-codex`, which stores 65,535 font files. It has no reason in `wow-claude`.)
- **HDR** is not tested.
- **The `claude` command on Windows** is `claude.cmd` in some installs. The bridge finds the path with the `which` crate.
- **Claude on native Windows has no sandbox** for its commands (6.6.4, "Windows"). Every command asks in the game. For commands that run with no question, use Codex, which has its own Windows sandbox, or Claude under WSL2.

### 11.3 Install

The goal: one download, one command, and no step inside the game.
The install scripts put the program on `PATH`, also in the open terminal on Windows, and print the `PATH` line on Linux and macOS when it is missing.

**`gnomish-relay setup`** does every step, and a second run changes nothing that works:

1. **Find the game.** It looks for a `_classic_beta_` folder in the default places and in the install paths of Battle.net's `product.db`:
   - Windows: `Program Files (x86)\World of Warcraft`, and `%ProgramData%\Battle.net\Agent\product.db`.
   - macOS: `/Applications/World of Warcraft`, and `/Users/Shared/Battle.net/Agent/product.db`.
   - Linux: each Wine prefix (`~/.wine`, `~/Games/*`, Bottles also as a Flatpak, and Steam Proton), with the `product.db` of the prefix. `C:` maps to `drive_c`, and other drives to `dosdevices`.
   With more than one, or none, it asks in a terminal. `setup <folder>` skips the search, and takes the `World of Warcraft` folder or `_classic_beta_`. It makes `Interface/AddOns` if WoW has not made it yet, and it finds that folder in any case.
2. **Make the keys**, 32 random bytes from the OS each, with mode 0600, once: `strip.key` always, and `timeways.key` only when the game has an `Interface/AddOns/Timeways` folder (in any case). The Timeways key is never equal to the strip key. `--new-key` makes a new key for each app that is there, and then each addon needs a `/reload`.
3. **Install the addons.** Relay on or off: 9.7, decision 15. With the relay on, setup writes the addon files, which are built into the program, into `Interface/AddOns/GnomishRelay`, and writes `Key.lua` from the strip key. A folder that is a link (a developer checkout, 16) stays as it is, and only `Key.lua` changes. With a Timeways folder, setup writes only `Key.lua` into it, from `timeways.key`, in the same format: the Timeways TOC lists `Key.lua` first. It never makes the Timeways folder and never writes another file in it.
4. **Write the config**, once. With the relay on, it has an `[agents.<name>]` entry for each known agent on `PATH`: `claude` (as `kind = "claude"`), `codex` (as `kind = "codex"`), and the ACP agents of 9.2. The default agent is the first one it finds, in the order of `KNOWN_AGENTS` in `install.rs`. With none, it is `echo`. Each entry gets `permission = "auto-edit"`. For each harness with no ACP mode on `PATH` (aider and `llm`), setup asks "Found aider. Add it as an agent? It runs its own commands with no question, inside the sandbox. (y/N)", and adds a `kind = "command"` entry with its preset only on a yes. With no terminal, the answer is no. A harness that has an ACP mode (gemini, goose, opencode) gets its ACP entry, which asks about its tool calls. A local model that answers on the loopback (Ollama on 11434, LM Studio on 1234) puts its port into `[sandbox] local_ports`, so an agent that uses it reaches it from its wall (6.6.4).
   - Why `auto-edit` (decided with an advisor on 2026-09-26): the config is the ceiling of every chat (S6), and the addon asks for `auto-edit`. With `ask` in the config, the player got a game popup for each edit and could not change that from the game. At `auto-edit`, edits inside the chat folder run, and each command still asks in the game unless the allow table covers it. The `desktop` and `deny` answers do not change. Every kind gets the same level, so the rule is simple. An ACP agent at `auto-edit` also edits the chat folder with no popup when it asks. `echo` has no tools. The config also gets a commented example of the allow table (12): setup allows no command. With a Timeways folder, the config gets a `[story]` section with the model that setup finds (9.7, decision 15). With the relay off, the config has no relay part, and setup asks no folder question.
5. **Make the slot addons**: `GnomishRelay_S0001` to `S1000` with the relay on, and `Timeways_S0001` to `S1000` (with `## Dependencies: Timeways`) with a Timeways folder. WoW finds a new addon only at launch, so after new slots the game needs a restart. Setup says so.
6. **Start the bridge at login**, with `--autostart`: a systemd user service on Linux, a launchd agent on macOS (log in `~/Library/Logs/gnomish-relay.log`), and a `Run` entry of the user on Windows, which needs no admin rights. On Windows, `run --background` starts the bridge with no console window, with its log in the data folder.

The order is key, addon, slots, config, then autostart: the addon and the slots need nothing else. A failed autostart prints one line, and setup goes on.
With no code folder found, the folder question has no default: the home folder holds `~/.ssh` and the browser profiles.
The last lines say what setup found and the next action, for example "Agent: claude", "Level: auto-edit. It edits files in the chat folder with no question, and asks in the game before each command. To change it, edit permission in <config file>", "Story model: claude (haiku)", and "Restart WoW, then type /relay". The level line shows the level of the default agent in the config, also for a config that setup did not write. With the relay off, setup says "Gnomish Relay: off. To add coding agents: gnomish-relay setup --relay" and "Restart WoW, then log in". `gnomish-relay install` makes the slots of each app that is on.

**Keeping it working.**

- At each start, the bridge writes `Key.lua` again if it is missing, and the addon files again if their version differs. An addon app such as CurseForge can replace the folder, and a `/reload` then loads the files.
- At each start, the bridge also writes the Timeways `Key.lua` again when it is missing or old, and only that file. It never makes a Timeways key: that is the job of setup.
- With no key, the addon shows one line: "Gnomish Relay: run gnomish-relay setup. Get it at github.com/eserilev/gnomish-relay".
- With no fresh body one minute after login, the addon shows one line: "Gnomish Relay: bridge not running. On the desktop, run gnomish-relay restart."
- Setup starts the default agent once, with no prompt. A missing login then shows in setup ("Agent: claude needs a login. Run: claude"), not as the first reply in the game.

**Updates and restarts.**

- `gnomish-relay restart` stops the bridge and starts it again, for example after a config edit. With the service of setup, it uses the service: `systemctl --user restart` on Linux and `launchctl kickstart -k` on macOS. With no service (Windows, or no `--autostart`), it stops the process in `bridge.pid`, waits up to 10 s for the lock (8.4), and starts `run --background`.
- `gnomish-relay update` downloads the archive of the latest release for this OS with `curl`, checks its SHA-256 sum, and unpacks it with `tar`. Every supported OS has both tools. `GNOMISH_URL` changes the download folder, as in `install.sh`.
- If the new program is the same as the installed one, update changes nothing. Otherwise, it puts the new program in place of the old one and restarts the bridge.
- Windows refuses to replace a running program, but it lets update rename it. So update renames the old program to `gnomish-relay.exe.old` first, and the next update deletes that file.
- The sum comes from the same release as the archive. It finds a broken download, not a changed release.
- The new bridge writes the new addon files of the relay at its start, and only `Key.lua` for Timeways. Then the game needs a `/reload`, and update says so. The new bridge starts the story program again (9.8).

**Distribution.**

- A version tag (`v*`) starts `.github/workflows/release.yml`. It builds the program for Linux (x86-64), macOS (Arm and x86-64), and Windows (x86-64), and attaches each archive with its SHA-256 sum to a GitHub Release. The release stays a draft until every build is attached. Before the draft, the workflow runs fmt, clippy, and the tests on the three OSes, and checks that the tag, the `Cargo.toml` version, and the TOC version match.
- `scripts/install.sh` (Linux and macOS) and `scripts/install.ps1` (Windows) download the archive of the latest release, check its SHA-256 sum, install the program, and run `setup --autostart`. Setup asks its questions on the terminal, also under `curl | sh`.
- Setup asks which folders the agents can use. It suggests the usual folders of code projects that hold a git repository, or the home folder. `--roots a,b` gives them with no question.
- Setup installs no agent. It uses the agents that are already on `PATH`. With none, the config uses `echo`, and setup says so.
- Later: winget, Homebrew, and the AUR point at the release.
- The addon is also listed on CurseForge and Wago Addons, so players can find it. The listing points to the program: the addon alone does nothing, because each computer needs its own key.

## 12. Config

The config file is `config.toml` in the config folder of the OS:

| OS | Config folder | Data folder (`state.json`) |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/gnomish-relay`, or `~/.config/gnomish-relay` | `$XDG_DATA_HOME/gnomish-relay`, or `~/.local/share/gnomish-relay` |
| macOS | `~/Library/Application Support/gnomish-relay` | the same |
| Windows | `%APPDATA%\gnomish-relay` | `%LOCALAPPDATA%\gnomish-relay` |

`gnomish-relay setup <wow folder>` writes the first config. It never changes a key that exists. It only adds a missing `[story]` section when the Timeways addon is there, or the relay part with `--relay` (11.3).

The bridge accepts only the keys that it implements. Any other key is an error, so a typo never leaves a wider default in place.
Today these keys work: `allowed_roots`, `default_cwd`, `default_agent`, `timeout_minutes`, `permission_timeout_minutes`, `[wow] path`, `[agents.<name>]` with `kind`, `command`, `permission`, `env`, `modes`, `agent_hosts`, `preset`, and `resume`, `[allow]` with `commands` and `[allow.folders]`, `[sandbox]` with `allow_hosts`, `default_hosts`, `local_ports`, and `agent_network`, and `[story]` with `program`, `lore_pack`, `timeout_seconds`, `model`, `claude_model`, `local_url`, `local_model`, `model_timeout_seconds`, and `budget_window_minutes`.

**The story program of Timeways** (9.8) starts only with a `[story]` section and a `timeways.key`:

```toml
[story]
program = "~/.local/bin/timeways-story"   # an absolute path, or one that starts with ~/; never in the config folder, the data folder, or a credential folder (6.6.4)
lore_pack = "~/.local/share/timeways/lore.sqlite"   # the same; the first argument
timeout_seconds = 120                     # 1 to 600; the longest wait for one reply
model = "claude"                          # or "local"; with no model, every model call fails
claude_model = "haiku"                    # optional, only with model = "claude"
model_timeout_seconds = 60                # 1 to 600; the longest wait for one model answer
budget_window_minutes = 20                # 1 to 1440; at most 10 model calls in this window
```

A local model instead of Claude:

```toml
model = "local"
local_url = "http://127.0.0.1:11434"      # Ollama; LM Studio listens on 1234
local_model = "llama3.2"
```

- The bridge never looks up `program` on `PATH`. A name with no folder is an error, for `program` and for `lore_pack`.
- `program` and `lore_pack` go together: both, or neither. With neither, the story program does not start, the bridge logs one line at start, and each Timeways message gets "Timeways story program not running.". Setup cannot know them before Timeways ships its program, so it writes them as commented lines (11.3).
- With no `[story]`, each Timeways message gets the answer "Timeways story program not running.".
- With `[story]` and no `timeways.key`, the bridge logs one line and starts no story program.
- `local_url` is only `http://127.0.0.1:<port>` or `http://[::1]:<port>`, with nothing after the port. Config load refuses `localhost`, any other host, `https`, and a path, because `localhost` can resolve to another host.
- `model = "local"` needs `local_url` and `local_model`. A key of one model with the other model, or with no `model`, is an error, so a typo never leaves a model that the user did not mean.
- A model name has no space and does not start with `-`, because `claude_model` goes into an argument of `claude`.
- The model route takes nothing from `[agents.*]`: `model = "claude"` always runs `claude` from `PATH`, with the environment allowlist of 6.2 and no `env` list (9.7, decision 10).

**A config with no relay part.** `allowed_roots` alone turns the relay on. With `allowed_roots`, `default_agent` and its `[agents.<name>]` entry are needed, as before. With no `allowed_roots`, each of `default_agent`, `default_cwd`, `timeout_minutes`, `permission_timeout_minutes`, `[agents]`, `[allow]`, and `[sandbox]` is an error ("<key> needs allowed_roots"), so a typo never leaves a relay half set up. A player with only Timeways gets this config from setup (9.7, decision 15):

```toml
[wow]
path = "~/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_"

[story]
model = "claude"
claude_model = "haiku"
```

With no relay part, the bridge has no relay lane. It does not write the relay addon, and it logs and drops a strip signed with `strip.key`. `say` and `check-agent` stop with "the relay is off. Run: gnomish-relay setup --relay". Setup still makes `strip.key`, so the key check of 9.7, decision 1, stays the same.

**The allow table** lists the commands that run from the game with no question at `auto-edit` and `full-auto` (9.3):

```toml
[allow]
commands = ["cargo test *", "cargo fmt --check"]

[allow.folders]
"~/Code/lighthouse" = ["npm test *"]
```

- A pattern is plain words with a space between them. It covers every command that starts with these words, so a last `*` only shows that more words can follow: `cargo test *` and `cargo test` are one rule.
- A word with shell syntax (`*`, `?`, `[`, `]`, `$`, a backtick, a quote, `\`, `;`, `&`, `|`, `<`, `>`, `(`, `)`, `{`, `}`, `~`, `#`, or `=`) is an error, and so is an empty pattern.
- `commands` applies to every chat. A folder of `[allow.folders]` must exist, and its patterns apply to each chat inside it.
- A pattern never allows a `deny`, `desktop`, or "never always" command (6.6.3, S17). A config with no `[allow]` has an empty table.
- A dialog of the OS, `gnomish-relay approve`, and `gnomish-relay deny` answer the desktop requests of 6.6.3. They live in `approvals` in the data folder.

**The hosts of the sandbox** are the only hosts that a command of a game run reaches, through the proxy of the bridge (6.6.4):

```toml
[sandbox]
allow_hosts = ["nodejs.org"]   # added to the default hosts of 6.6.4
default_hosts = true           # false leaves only allow_hosts
local_ports = [5432, 3000]     # ports of this computer for the agent and its commands
agent_network = "open"         # "strict": the agent reaches only its model hosts and agent_hosts
```

- A host is an exact name, compared without ASCII case. It has at least one dot, and its last label starts with a letter. A `*`, a port, a scheme, an IP address in any form, and `localhost` are errors, so a typo never opens more than one name.
- With `default_hosts = false`, no `allow_hosts`, and no `local_ports`, the proxy does not start, and commands have no network at all.
- `local_ports` (6.6.4, "`local_ports`") lists ports of the loopback of this computer, for example a database or a dev server. 2375, 2376 (Docker), 9222 (the debugger of a browser), and 3128 (the forwarder of the sandbox) are errors.
- Hosts that a user can add: `nodejs.org` (headers for native modules of npm), `proxy.golang.org` and `sum.golang.org` (Go modules).
- Only the desktop changes `config.toml` (6.6.2), so no message from the game adds a host.

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

[agents.claude]
kind = "claude"
command = ["claude"]
permission = "auto-edit"
modes = { ask = "manual" }   # optional; see 9.3

[agents.codex]
kind = "codex"
command = ["codex"]
permission = "ask"

[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"

[agents.aider]
kind = "command"
preset = "aider"            # or a full template: command = ["tool", "--message={prompt}"]
permission = "auto-edit"    # the chat folder is writable, and aider runs its commands with no question, inside the sandbox
env = ["OPENAI_API_KEY"]
```

An older config with `kind = "acp"` and `command = ["claude-agent-acp"]` still works. Its mode IDs are not checked yet.

## 13. The addon

### 13.1 Look

The window follows the classic Guild & Communities frame, and uses the built-in game textures and fonts.
The mockup is the reference for the layout.

- **Frame:** the dark metal frame, a black title bar with the gold title "Gnomish Relay", and gold-framed red minimize and close buttons.
- **Bridge light:** at the right of the title bar, so every tab shows it: a dot and a label. "Checking the bridge" in grey until the first poll, "Bridge online" in green, "Bridge slow" in amber, and "Bridge offline" in red (7.4). A channel problem (7.8) or a version mismatch (7.7) shows here in red too.
- **Portrait:** a round emblem at the top-left corner: a red pipe wrench on a brass cog. It is our own drawing, shipped as a texture.
- **Left column:** one tile per chat, with the agent as the shield icon. The selected tile glows green. A gold "!" marks a new reply. An orange "?" marks a chat whose permission popup waits for the player. The last tiles are "Start a New Chat" and "Resume". When the tiles do not fit in the column, the mouse wheel scrolls them, and a new chat scrolls to the end. A new chat starts in the default folder, and its center shows the empty transcript. Resume shows the picker of 9.6 in the center: a gold heading for each folder, then one row per session with its title, its agent, and its age, or a green "open" for an active session. A right-click on a chat tile asks `Delete "<name>"?`, or `Stop and delete "<name>"?` while the agent works, with **Delete** and **Cancel**. The question is a dialog of the game (`StaticPopupDialogs`), so it has the border of the game, and Escape closes it.
- **Center:** a dropdown for the agent and the permission mode, and the folder button. The folder button shows a small folder icon, the folder of the chat, and a dropdown arrow. It turns gold on hover. A click opens the folder browser of 9.9 in place of the transcript, and the input stays. A second click, a choice, or Escape in the filter closes it. Below them, the transcript on a black background: `[You]: text` and `[Claude]: text`. The text is white. Only the name has a color: the user in blue, each agent in its own color. A sent message with no final reply shows its delivery state in grey at its right: "Sending...", "Retry 2 of 3" at the second show of its strip (7.1.1), "Delivered" once a body holds its record, and "Needs reload" while it waits in the outbox (7.5). The state goes when the reply comes. The mouse wheel scrolls it, and a new entry scrolls it to the bottom. A new entry draws below the others, and the old entries stay as they are. The whole chat draws again only when the chat, the font size, or the width changes, or when the history drops its first entry.
- **Replies:** a rendered reply (7.3.1) shows its blocks below the name.
  - Headings, paragraphs, list items, and quotes go into one SimpleHTML frame, with real sizes for `h1` to `h3`, and a bullet or the number before each item.
  - Code shows in a black box in the shipped mono font (13.2).
  - A table is a grid of font strings with a gold header row. A table with more than 8 columns, or too wide for the transcript, shows each row as a card: the first cell in gold, and each other cell below it with the name of its column.
  - If anything fails while a reply draws, it shows as plain text.
  - User messages, errors, and replies from before 7.3.1 stay plain text.
- **Errors:** an error comes from the relay, not from the agent. So it shows as a grey line `[Relay]: Not sent. Send it again.`, never under the name of the agent. Below it, a blue "Put the text back" link puts the text of its message back in the input, with the focus. Enter then sends it again. The link never sends by itself, so the player sees each text that goes out (6.6.1).
- **Input:** one line, with no label. While it is empty and has no focus, it shows a grey hint: "Type a task. Enter sends." Enter sends, empties the line, and clears the focus, so the keys of the game work again. The limit is the room of one strip: a payload of 3200 bytes (7.1), less the other fields of the record and 440 bytes for the report. That leaves about 2600 bytes of text. With fewer than 400 bytes left, a small counter above the right end says "100 bytes left", and past the limit it says "5 bytes too many" in red.
- **Right column, Activity:** a cast bar while the agent works, and one row per step. A tooltip on each row shows the details. While a popup of the chat waits, the cast bar stands still in grey and says "Waiting for you: approve in popup" in orange: the run makes no progress then. At the bottom, a grey line gives the time to the next poll: "Next check in 12 s". The cast bar and this line change at most 5 times a second.
- **Side tabs:** Chats, Settings, and Diag, on the right edge of the window. The window stays on screen with its tabs: the clamp of the window counts the tabs as part of it. Notifications get no tab: a bell at the minimap shows them (10.4). Settings and Diag take the place of the center and the Activity panel. The chat tiles stay on the left, and a click on a tile goes back to Chats.
- **Settings** (asked for by the user, decided with an advisor on 2026-09-26, 13.5). The page, in this order:
  - **New Chats:** Agent, a dropdown of the agents in the settings list (13.4), and Level, a dropdown of `ask` and `auto-edit`. After the level, a grey hint: "Max: <level> (set on the desktop)", the level of the chosen agent in the config.
  - **Appearance:** Font Size, a slider from 12 to 20 (default 14). It applies at once to all chat text: headings, paragraphs, code boxes, tables, and the input. The window keeps its size, and long lines wrap. Reply line: an on and off box, 5 colors (copper `f0a860` is the default), and a Sound box, with a preview of the whisper line below. Window position: **Reset** puts the window in the center.
  - **Notifications** (section 10), after Appearance, only after `hooks install`: Notifications, an on and off box (default on); off stops the lines, the sounds, the toasts, the bell, and the faster polls of 10.4, and greys the other two rows. Finished work, a dropdown: Always, Over 1 min (default), Over 3 min, and Never. Alerts: three boxes, Chat line, Sound, and Toast (default on).
  - **Always Allowed** (6.6.5): one row for each rule of the settings list, with the pattern, the folder, the last use, and a remove button, 6 rows at a time. The mouse wheel scrolls it. With no rule: "No rules yet. Click Always allow in a popup to add one."
  - At the bottom, the status line: "Online · 2m ago", the age of the settings list. It is orange when the list is older than 10 minutes, and grey "Offline · <age>" while the bridge is offline. With no list, it says "No data yet.". A click asks for a new list.
- **Diag:** the settings list of the bridge, read only: the status, the allowed roots, the default folder, the agents with their levels, the allow table with the patterns of each folder, the timeouts, and the sandbox. With `[story]`, the Timeways model and budget. After `hooks install`, the rows of 10.4: Hooks, Sessions, and Last notification. Then the versions, and the lines of `/relay diag`. While the bridge is offline, its values are grey. The mouse wheel scrolls the page.
- **Key binding:** `Bindings.xml` adds "Open or close the window" under "Gnomish Relay" in the Key Bindings menu of the game. It calls the global `GnomishRelay_Toggle`.
- **Bottom bar:** a red **Stop** button, only while an agent works. It stops the run.
- **Game chat:** a finished reply shows one line, `[Claude] whispers: [chat] …`, in its own color (copper by default, a setting). For a rendered reply, the line shows the plain words of its first block. A click on it opens the chat. It plays the whisper sound. Settings can turn the line or its sound off. A desktop request (6.6.3) always gets its line, because it is the only notice in the game. A notification of a terminal session gets its own line with a bell (10.4).
- **Permission requests** use the separate popup of 6.4, never the window. A desktop request has no popup: an Activity row and one whisper line (6.6.3).

### 13.2 Code

The addon is our own code. It uses the design of `wow-claude`, not its files.
All state is local to the addon files, which share one table. The files load in this order.
The files marked "shared" are in `addon/transport` (9.7, decision 14). They read the names of the app from `App.lua`.

| File | Job |
|---|---|
| `Key.lua` | The strip key. `scripts/dev-link.sh` writes it, and git ignores it. |
| `App.lua` | The names of the app: its title, the chat of a hello, the slot prefix, the three slot globals, the strip frame, and the saved variables. |
| `Sha256.lua` (shared) | SHA-256 and HMAC-SHA256 for the strip tag. |
| `Codec.lua` (shared) | Records, frames, and cells: the Lua side of `crates/protocol`. |
| `Saved.lua` (shared) | The saved variables table of the app. |
| `Store.lua` | The saved data of the relay: chats, deletes, and settings. |
| `Health.lua` (shared) | The login self-test and the health of each channel (7.8), and the line for a blocked strip corner (7.1.2). Its lines start with the title of the app. |
| `Strip.lua` (shared) | Takes the shared strip corner in turn with the other apps (7.1.2), draws a frame, and takes one screenshot of it. |
| `Slots.lua` (shared) | Loads one slot, and takes the three globals of the app. |
| `Messages.lua` (shared) | The send queue, the signed outbox, retries and give-up, the hello, the report flags (`next`, `read`, `restored`, and the health flags), and the slot poll with the replies. It follows `models/transport.qnt`. It keeps the token and the message ids. An app sets its hooks: the store of its messages, the fields of a record, and the calls for each reply. |
| `Transport.lua` | The relay on top of `Messages.lua`: the coding flags, the session list, the folder tree request, Stop, Delete and its `d` records, the restore bundle, the live file, and the permission answers. |
| `Blocks.lua` | Splits a rendered reply (7.3.1) into blocks and fields, and gives its plain words. |
| `Transcript.lua` | The transcript of the window: a scroll frame that stacks entries and draws blocks. |
| `Folders.lua` | The folder tree of 9.9: the parser, the relative folders, the filter, the recent folders, and the name rules. |
| `Browser.lua` | The folder browser of 9.9 in the center of the window. |
| `BridgeSettings.lua` | The settings list of 13.4: the parser, the cache, and the agent of a new chat. |
| `RulesGroup.lua` | The "Always Allowed" group of the Settings tab (6.6.5). |
| `SettingsTab.lua` | The Settings tab of 13.1. |
| `DiagTab.lua` | The Diag tab of 13.1. |
| `Window.lua` | The window of 13.1, its side tabs, and its place. |
| `Popup.lua` | The permission popup (6.4). Each button names the kind of its option, never the label of the agent. |
| `Core.lua` | Startup, slash commands, and the whisper line. |

**The hooks of `Messages.lua`.** An app sets them after the file loads. The defaults suit an app with one chat, such as Timeways. An advisor agent and the implementer chose this split (2026-09-26, 9.7 step 5b):

- A chat is always a table with an `id` field. Two accepted types would be clever code.
- `Store` gives `Add`, `Open`, and `Find` for the messages. The default store keeps the messages in a `sent` list in the saved variables of the app. It keeps the last 64 answered messages, because a later body can still hold their final replies, and the addon must report them in `read`. The relay keeps its messages in its chats, so the saved data of the relay did not change.
- `Fields` gives `cwd`, `flags`, and `name` of a record. The relay puts its coding flags here.
- `Messages.lua` marks a message as answered, and then calls `OnReply(chat, id, status, text)` once, or `OnGiveUp` for "Not sent" and "Too long". The default `OnGiveUp` calls `OnReply` with `error`. The relay adds a give-up to the history with no whisper, as before.
- `OnStatus` gets each record of a known message, also `working`. `OnOther` gets each record of no known message, and its result says whether the addon reports it as read (the session list of the relay).
- `Control(chat, id, flags)` sends a record once, and starts a strip. `Riders` are records that go only with a strip that goes out anyway, for example the `d` records of the relay. A rider that started a strip would start one every second while the bridge is off. `d` is a coding flag (9.7, decision 6), so the deletes stay in `Transport.lua`.
- `OnPoll(restore, live)` gets the other files of each slot. `Awaits` keeps the fast poll schedule while the app waits for a reply that is not a message.
- `PollEvery` gives the seconds to the next poll while the app waits for something off the schedule, or nil. The default is nil, so Timeways does not change. The relay gives 5 while a desktop request waits (6.6.3), and 15 while a run works (7.3).
- The title of the app starts each line of `Health.lua`, and `helloChat` is the chat of a hello.

The folder also holds `JetBrainsMono-Regular.ttf`, the mono font of code boxes, with its license in `JetBrainsMono-OFL.txt` (SIL Open Font License 1.1). Setup installs both.
The game finds a new file only at launch. Until then, code boxes use `Fonts\ARIALN.TTF` of the game.

Message ids start from the clock, so the ids after a saved-data wipe never repeat the ids in an older body.

The tests run the addon in a real Lua 5.1 with a fake WoW API (`addon/tests/wow.lua`), from `crates/bridge/tests`.
They decode each strip with the proved Rust decoder and check its tag against the Rust HMAC.
They also check the SHA code against both kinds of `bit` results: unsigned as in WoW, and signed as in LuaJIT.

The folder also holds `Bindings.xml`, the key binding of 13.1. The game reads it from the folder by itself, so the TOC does not list it.

**Settings of the addon.** The saved variables hold the font size, the reply line, its color and its sound, the place of the window, and the agent and level of new chats. They apply at once, and the bridge never sees them. A chosen agent that the last settings list does not have gives the `default_agent` of the list.

Still to come: the agent dropdown in the header, and the emblem texture.

Slash commands:

| Command | Action |
|---|---|
| `/relay` | Open or close the window. |
| `/ai <text>` | Send a message to the current chat. |
| `/relay diag` | Show transport diagnostics. |
| `/relay poll` | Load the next slot now. |
| `/relay size <n>` | Set the font size of the chat text, from 12 to 20. |

### 13.3 Voice (later)

**Not planned now.** Nobody owns voice, and it has no date. The text below is a design note.

Voice comes after the ACP backend (step 9), because it needs a real agent to be useful.
Both directions run on the bridge side. The WoW client gives addons no microphone, and no speech-to-text API.

**Voice output.** The bridge speaks each final reply on the desktop. The full text always stays in the window.

The config key `voice` sets what the voice reads:

| Value | The voice reads |
|---|---|
| `auto` (default) | The full reply if it has no code. With code, it reads the text parts and skips each code block, diff, and long path with one short sound. |
| `full` | The full reply, also the code. |
| `summary` | One short spoken line. The bridge asks the agent for it at the end of each run. If the agent gives none, the voice reads the first sentence. |

- Playback goes paragraph by paragraph. **Skip** jumps to the next paragraph, and **Stop** ends the reply.
- Skip and Stop have keys in the game. The addon sends `voice=skip` or `voice=stop` through the strip, so a key takes about half a second. The desktop has the same two hotkeys, with no delay.
- A new reply waits until the current one ends, or you skip it.
- The default engine is a local model (Piper), so no reply text leaves the computer. A cloud voice is an option in the config.
- The fallback is `C_VoiceChat.SpeakText(voiceID, text, rate, volume)` in the game. It exists in the Forever client and uses the voices of the operating system. Under Wine, the client can have no voices (17).
- The config turns voice output on per agent. It is off by default.

**Voice input.** You hold a key in the game and talk. The bridge records and transcribes.

1. You hold the push-to-talk key of the addon. The addon sends a `listen=start` record for the open chat through the strip.
2. The bridge records the microphone.
3. You release the key. The addon sends `listen=stop`.
4. The bridge transcribes the audio on the computer (Whisper). The text becomes a message of the chat, with the same checks and the same ceiling as a typed message.
5. The window shows the text as your message.

**Privacy rules for voice input.** Any addon can send a game record (6.6.1). So a hostile addon can send `listen=start` and record the room.

- The bridge records only when the config turns voice input on. It is off by default.
- While the bridge records, the desktop shows a sign, and the game window shows a red "Listening" light.
- One recording stops after 60 seconds, also with no `listen=stop`.
- The bridge never stores the audio. It deletes the audio after the transcription.
- The config can require a desktop hotkey to start a recording, so that no game record can start one. On Wayland, a global hotkey needs the portal (17).
- A transcript runs under the game ceiling (6.6.2). Voice gives no more rights than typing.

### 13.4 The settings list

The Settings and Diag tabs (13.1) show values of the bridge. The game never writes `config.toml` (6.6.2), so it only reads them. `settings_list.rs` writes the list.

**The request.** The addon sends a `list=settings` record of the chat `settings`, as for `list=folders` (9.9). The reply is one record, and the addon keeps its text in its saved variables with the time. So the tabs show the last list at once, also while the bridge is offline.

**The reply.** One line per value: `key \t value`. Some values hold more tabs, so a reader splits a line at its first tab only. A control character inside a field becomes a space. The lines come in this order:

| Key | Value |
|---|---|
| `version` | The version of the bridge. |
| `sandbox` | How commands from the game run (6.6.4), as the bridge prints it at start. |
| `default_cwd` | `default_cwd`, with `~/` for the home folder. |
| `allowed_root` | One root of `allowed_roots`. One line for each root. |
| `default_agent` | The name of the default agent. |
| `agent` | `name \t kind \t level`: one line for each agent. The level is the level of the config now, so a raise on the desktop (9.3) shows at the next list. |
| `timeout_minutes`, `permission_timeout_minutes` | The two timeouts. |
| `story_model` | Only with `[story]`: `none`, `claude`, `claude <model>`, or `local <model>`. The address of a local model stays on the desktop. |
| `story_budget_window_minutes` | Only with `[story]`. |
| `rule` | `id \t folder \t pattern \t days`: one "Always allow" rule (6.6.5), with the days since its last use. |
| `allow` | One pattern of `[allow] commands`, as words. |
| `allow_folder` | `folder \t pattern`: one pattern of `[allow.folders]`. |
| `hook` | Proposal (10.5): `agent \t state`, one line for `claude` and one for `codex`. The state is `on`, `off`, `moved` (the path of the hook does not exist), or `disabled` (`disableAllHooks`, or no `codex_hooks`). |

- The list never holds a key, an `env` entry, or the command line of an agent. A command line can hold a secret, and the game does not need it.
- The reply is at most 32 KB after the Lua escape (S12). The allow table comes last, because only it can be long. The `rule` lines (6.6.5) come just before it, so a cut removes allow patterns first. A list that does not fit keeps its first lines and ends with a line `+`, as the folder tree does.
- No version change (7.7): the bridge writes the relay addon again at each start, so the addon is never newer than its bridge.
- **When the addon asks.** When the Settings or Diag tab opens and the list is older than 10 minutes, or there is none, and at a click on the status line. Each ask costs a strip, so a tab that opens again soon asks nothing.
- The addon parser takes a line only with a known shape: an agent needs a valid name and a known level, and a folder rule needs a folder and a pattern. A seeded test feeds it random bytes (14.4).

### 13.5 Decisions for the desktop notice, the new message, and the Settings tab

The implementer and an advisor agent chose these (2026-09-26).

1. **The desktop state rides in the live file** as one progress line of the bridge, right after the level line. S9 and S20 do not change, and no slot file is new.
2. **A separate `PollEvery` hook** in the shared `Messages.lua`, with nil as the default, so the Timeways copy keeps its schedule.
3. **At most 24 fast polls for each desktop request.** A request that waits for the whole `permission_timeout_minutes` then costs 24 of the 1000 slots, and the normal schedule still finds the answer.
4. **The whisper line of a desktop request is once for each request**, also across a `/reload`. The saved variables keep the last 16 ids, so the list stays small.
5. **A new message ends only a wait for an answer.** During a normal turn, a follow-up waits in the queue, else each follow-up ends a long run. Only a newly accepted record counts: a duplicate, an outbox copy, or a refused record never ends a wait.
6. **The old message ends as "Stopped."**, the text of Stop, so the player sees one known end.
7. **No Pings tab yet.** An empty tab is a promise that the game does not keep. (Section 10 gives notifications a bell at the minimap, not a tab.)
8. **A settings list, not a new slot file.** It is one more list in its own chat, as `list=folders`. Some values hold tabs, so a line splits at its first tab only.
9. **The addon asks for the list only when a tab opens and the list is old**, and at a click on the status line, because each ask costs a strip.
10. **The Level dropdown has no `full-auto`.** The config caps every level anyway (S6), so this only keeps the page honest.
11. **Own dropdowns.** A button and a list of choices, with no dropdown API of the client, so a client patch cannot break them. An open list closes with its page and at a click outside it (`GLOBAL_MOUSE_DOWN`), as a menu of the game does.
12. **The reply line setting does not stop the line of a desktop request**, the only notice of that request in the game.

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
| S9 | **Slot body shape:** for each app, the slot file writer only puts escaped strings and numbers into a fixed table shape, under the global name of that app. | A malicious agent changes `proto`, adds fields, or runs code in the slot file. |
| S18 | **Restore file shape:** for each app, the restore writer only puts escaped strings and numbers into a fixed table shape, under the global name of that app. Its prepare step keeps the last 16 chats and the last 10 messages of each, and cuts only the ends of strings. | A chat name or a message from a malicious agent runs code in the restore file. |
| S19 | **Restore size bound:** for each app, a restore file that fits is at most 512 KiB. | A long chat history makes a restore file that the game cannot load. |
| S20 | **Live file shape:** for each app, the live file writer only puts escaped strings and numbers into a fixed table shape, under the global name of that app. Its prepare steps keep the last 30 progress entries with their last 5 lines, the first 4 permission requests with their first 4 options, and the newest 20 notices of terminal sessions (restated 2026-09-28, 10.3), and cut only the ends of strings. | An agent puts code into a progress line or a popup, or a local process into a notification. |
| S21 | **Live size bound:** for each app, a live file that fits is at most 256 KiB, also with 20 notices. | Progress, popups, or notices make a file that the game cannot load. |
| S10 | **UI escape:** the display sanitizer doubles every `\|` in agent text. | A malicious agent fakes a WoW chat link (`\|H...\|h`), a texture, or a color that imitates a system message. |
| S11 | **Freshness:** the bridge accepts a frame only if its time is at most 5 minutes old and at most 1 minute in the future. | An old screenshot of a strip is replayed. The MAC is still valid, so S2 does not stop it. |
| S12 | **Size bounds:** for each app and every input, a slot body is at most 1 MB, and each reply record in it is at most 32 KB. | A malicious agent writes a huge reply, and the bridge writes 200 huge slot files. |
| S13 | **ID charset:** the id validator accepts only `[a-z0-9_-]`, 1 to 32 characters. | A chat id like `../../x` reaches a file path or a state key. |
| S14 | **Rate limit and queue cap:** the limiter never admits more than N messages in any window. A chat queue never holds more than 20 messages. | Strip spam fills memory or starts many runs. |
| S15 | **Honest popup:** the popup text contains the full raw command, or its start and end with a cut mark. It contains no raw control, bidi, or zero-width characters. | A malicious agent asks for permission with a false label, or hides the dangerous part of a command. |
| S16 | **Classifier paths:** for a file tool call, if the answer is `ask` or `allow`, then every write path is inside the chat folder, every read path is inside `allowed_roots`, every path is clean (the form of S5), and no path is a `desktop` or `deny` path. Any path inside a `deny` folder (the config folder or the data folder of the bridge) gives `deny`. Limits (6.6.3): paths inside command arguments are out of scope, and the proof works on the paths that the bridge resolved. A symbolic link made after the check is a race that the proof does not cover. | An approved tool call in the game reads `~/.ssh`, writes outside the project, or reads the strip key. |
| S17 | **Classifier ceiling:** with the order `deny < desktop < ask < allow`, for every tool call and every rule list from the game, `classify(call, rules) ≤ ceiling(call)`. `ceiling` is the answer of the config when a game rule covers every command. An unknown tool is `desktop`, and a command with a "never always" or `desktop` part is at most `ask`, for every rule list. | A rule from the game, or a crafted command, gets more than the config allows. |
| S22 | **Renderer totality:** for every Markdown text of at most 1 MiB, `render_markdown` returns a value. It never panics and never reads out of bounds. | A crafted reply crashes the bridge. |
| S23 | **Block shape:** the output of `render_markdown` is the marker `1B 4D 31`, zero or more blocks, and `\n`. Each block is `\n`, a kind byte, and fields that each start with US, in the shape of 7.3.1. No field holds `\n`, US, ESC, any other byte below `20`, or `7F`. | Agent text makes a false block, fakes the marker, or moves text into another field or kind. |
| S24 | **Reply escape (extends S10):** every text of the output, read left to right in tokens, holds each `\|` only as `\|\|`, `\|r`, or one of the five color codes of `inline.rs`. A color code comes only when no color is open, `\|r` only when one is, and no color is open at the end of a text. The texts of `h`, `p`, `l`, and `q` hold no `<` or `>`, and each `&` starts `&lt;`, `&gt;`, or `&amp;`. | A malicious agent fakes a WoW link, texture, or color, or puts SimpleHTML markup into the window. |
| S25 | **Reply size bound:** the output of `render_markdown` is at most 16 bytes for each input byte, plus 4. | Rendering makes a reply grow without a bound. With the cut of S12, the body stays within 1 MB. |
| S27 | **Totality:** `classify`, `ceiling`, and the shell splitter `split` return an answer for every input. They never panic. | A crafted command or path crashes the bridge. |
| S29 | **Routing by key:** `route(relay_ok, timeways_ok)` gives the one app whose key verifies the tag of a strip. No key gives `BadTag`, and both keys give `Ambiguous`. The two tag checks are inputs, so S29 proves the choice, not the cryptography: `verify_tag` stays opaque, as in S2. | A strip of one app reaches the other app, for example a story strip reaches the coding agents. |
| S30 | **Version range:** for every app and every version, `version_fit` never fails. It gives `Supported` exactly when `oldest app ≤ v ≤ newest app`, `TooOld` exactly when `v < oldest app`, and `TooNew` exactly when `newest app < v`. | A message of an addon whose version the bridge does not speak reaches an agent or the story program, or a good version gets the update text. |
| S31 | **Sandbox policy:** for every config, each `deny` and `desktop` path is hidden; no writable path is inside a hidden path; writes go only to the chat folder and a private temp folder. `sandbox_policy` builds the policy from the chat folder, the temp folder, the `deny` folders, and both lists of `desktop` patterns. "Hidden" is the predicate of the classifier (6.6.3), and "inside" is the parts prefix of S5. A writable path has the clean form of S5. | A command of a game run reads the strip key or `~/.ssh`, or writes a file that code on the host runs later, such as `.git/hooks/pre-commit`. |
| S32 | **Seatbelt escape:** for every path, the escaped path in the Seatbelt profile reads back as the same path and never ends the string literal early. `sbpl_string` puts a `\` before each `"` and `\`, and refuses a NUL byte, which no path holds. A small model of the string reader of SBPL states "reads back", as S8 does for Lua. | A folder name with a `"` ends a literal and adds a rule to the profile, for example `(allow default)`. |
| S33 | **Host check:** for every allow list and every host, `host_allowed(list, host)` is true exactly when the host is a good host name and equals a name of the list without ASCII case, and `good_host_name` is true exactly for a good host name (6.6.4). A good host name has 1 to 253 bytes, at least two labels of 1 to 63 bytes of `[A-Za-z0-9-]` that do not start or end with `-`, and a last label that starts with a letter and is not `localhost`. | A proxy request for an IP address, `localhost`, or a name that differs from the list only in its case, or a lookalike such as `github.com.evil.net`. |
| S34 | **Public address:** for every IPv4 address, `is_public_v4` is true exactly when the address is in no range of `v4NotPublic`. For every IPv6 address, `is_public_v6` gives the answer of the IPv4 address that an IPv4-mapped, NAT64, or 6to4 form holds, and otherwise is true exactly when the address is in no range of `v6NotPublic` (6.6.4). | An allowed name that resolves to this computer, its network, or the cloud metadata address, also through an IPv6 form of such an address. |
| S35 | **The target of a request:** for every request head of at most 8 KiB, `check_target` returns a target or a defined refusal, and never panics. A target comes only from a first line `CONNECT <host>:<port> HTTP/1.<d>`, and it is a listed local port on `localhost` (never 2375, 2376, or 9222), or a host on port 443 or 80 that the mode allows (S33) (6.6.4). | A crafted request crashes the proxy, reaches a port of this computer that is not listed, or a host that the list does not allow. |
| S36 | **Proposal shape:** `propose` never panics. A rule is the first 1 or 2 words of its command, so it covers that command. Each word is plain (printable ASCII with no space and no shell syntax, 1 to 64 bytes, not starting with `-` or `+`), and the first word holds no `/`. | A crafted command makes a rule that covers more than the command, or a word that the rules file reads back as another rule. |
| S37 | **No proposal for the capped:** a `desktop` command, a "never always" command, a tool that runs any program, and a command that publishes get no rule. | One click in the game makes a lasting rule for `sudo`, `curl`, `npx`, or `git push`. |
| S38 | **An offer allows exactly its call:** `offer` never panics. An offer has 1 to 3 rules; with them the classifier gives `allow` for the call, and each rule covers a simple command of the call. | The popup offers a rule that does not make the call run, or a rule for a command that the call does not hold. |
| S39 | **An offer stays under the ceiling:** a call gets an offer only when the ceiling of the config is `allow`. | A rule from the game gets more than the config allows. |
| S40 | **Notice text** (proved 2026-09-28): `notice_text` never panics, returns at most `max` bytes with no control, bidi, zero-width, or tag character, holds each `\|` only as `\|\|`, and never ends inside a UTF-8 sequence (10.7). | Any local process writes a notification that fakes a chat link or a system line in the game, or hides text. |
| S41 | **Sessions and notices** (proved 2026-09-28): `apply_event` never panics. The table keeps at most 32 sessions and at most one notice for each. A `waiting`, `finished`, or `failed` event leaves exactly its own notice, and a start or an end leaves none on its session. The other sessions keep their notices, except in a full table where every session has one: then only the session with the oldest notice loses it (10.7). | Stale or piled-up notifications teach the user to ignore them, or a flood of files grows the table without a bound. |
| S28 | **Command floor:** a command that does not parse (the grammar of `split`, 6.6.3) is `desktop`. A command with `$(` or a backtick outside single quotes, by the quote state of the splitter, is `desktop`. `eval`, `sudo`, `cmd.exe`, PowerShell, or a shell after a `\|` make a command at most `desktop`. Commands that run other commands and network tools make it at most `ask`. | A prompt injection runs code through `eval`, a pipe into a shell, or `sudo`, or reaches the network with no question. |

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

### 14.2 Quint models of the transport

`models/transport.qnt` models the addon, the bridge, the slots, the signals, `/reload`, and a saved-data wipe.
The model checker checks these properties:

- The agent never runs one message twice.
- A publish never loses a reply that the addon has not read.
- After a saved-data wipe, the restore never duplicates or drops a chat.
- Each sent message ends with a reply or an error, also across `/reload`.

Write the model before the bridge state machine. The Rust state machine follows the model.

`models/corner.qnt` models the shared strip corner of 7.1.2: two addons, the shared value, the screenshot events that both addons get, the screenshots of the player, a lost event, the retry timer and the shows, and a hostile addon that writes the value. The times are small, but they keep the order of the real times. The model checker checks these properties:

- Two addons never show a strip at the same time.
- A screenshot event ends only the strip of the addon that holds the corner, never a strip of another shot.
- The retry timer does not run while an addon waits, and a wait adds no show. So the outbox comes only after the real shows.
- An addon that cannot get the corner shows the blocked line before its frame is too old.
- With no hostile addon, a waiting addon gets the corner within the bound of 7.1.2, and no blocked line shows.

`scripts/check-model.sh` runs both models. For each model, a set of witnesses must fail, so the simulator surely reaches the hard states: a turn after a wait, the blocked line, the outbox, and a retry.

### 14.3 Tests

- **Property tests** (`proptest`) for the codec, with pixel noise, color shift, and a cell pitch of 3 to 8 pixels.
- **Golden vectors:** screenshots of known strips from the real game, in `tests/vectors/<build>/` (14.3.1). The real bridge reader must decode each one, and its tag must check under the public test key. (`wow-claude` makes its images at test time and tests them only on Windows.)
- **Differential tests:** the Lua encoder and the Rust decoder agree on every vector. The Rust slot writer and a Lua reader agree on every body.
- **Addon harness:** run the addon in a Lua VM against a fake of the WoW API (`addon/tests/wow.lua`). Where the real game has a choice, the fake takes it from the newest fixture of the self-test (14.3.1).
- **Fuzzing:** `cargo-fuzz` on the frame decoder and the record parser. No panic and no hang on any input.
- **Fake agent and fake capture** for the bridge loop. No test needs the game or a real LLM, except live tests marked `#[ignore]`.
- **Coverage gates:** `protocol` 95% of lines, `bridge` and `agents` 80%.
- **CI** on Linux, Windows, and macOS. CI runs everything except the live tests.
- **CI time.** One fuzz job builds the targets once and runs each target for 5 seconds. The nightly run gives each target its own job and 600 seconds. The proofs and the model run only when `crates/protocol`, `proofs/`, or `models/` change (`scripts/ci-changes.sh`). A weekly run and the nightly run check everything. The rust jobs keep a build cache and run the tests with `cargo nextest`, which runs the test binaries side by side.

#### 14.3.1 The self-test of the game

The API gate (7.8) makes the fake game strict about names. But the fake game also guesses behavior: the time of a screenshot event, return values, the order of events, and how the screen draws the strip. Bugs came from these guesses, for example a strip that the old bridge read with a wrong tag. So the real game measures itself, and the tests use the measurements.

**The addon.** `addon/GnomishRelaySelfTest` is for developers only. `install.rs` never builds it in, and no release has it (a test checks `install.rs`). `scripts/selftest-link.sh` links it into the game, with three small load-on-demand helpers: `_Slot`, `_Off`, and `_Old` (an old `## Interface` number). It also links the shared `Sha256.lua`, `Codec.lua`, `Saved.lua`, `Health.lua`, and `Strip.lua` into it, so its strips come from the real code path.

The run starts 5 seconds after `PLAYER_ENTERING_WORLD`, when the saved results do not name the current build. `/grst` runs it again, and `/grst scale` also draws each strip at two other UI scales. It measures:

| Part | What it measures |
|---|---|
| Client | `GetBuildInfo`, the physical and UI screen size, the UI scale, and the CVars of a screenshot. |
| Load | The type of the saved variables when the first file runs and at `ADDON_LOADED`, and the order of the login events, for a login and for a `/reload`. |
| Lua | `_VERSION`, `%q` of control bytes, `bit` results for signed input, and `hooksecurefunc` on a missing global. |
| Fonts (7.3.1, 13.1) | `SimpleHTML:SetFont` for `h1` to `h3` and `p`, `GetContentHeight` at once, in the next frame, and later, the height of a text with and without `|c` codes, the width of the bullet and of no-break spaces in the body font, and what `FontString:SetFont` returns for a present and a missing file. |
| Addons (7.3) | What `LoadAddOn` returns for a present, a missing, a disabled, and an out-of-date addon, and for a second load. `IsAddOnLoaded` after a load. A load right after `EnableAddOn`, as `Slots.lua` does. Whether `ADDON_LOADED` fires inside the call. |
| Secrets | `issecretvalue` of `UnitHealth`, `UnitPower`, `UnitGroupRolesAssigned`, and `UnitDetailedThreatSituation`, and `C_CombatLog.IsCombatLogRestricted` (the tank addon tests T1, T3, T6, T7, and T8). A check that needs combat, a group, a target, or a nameplate says so and measures nothing. The first fight of the session runs the combat checks, and one strip in combat. A secret value never goes into the saved variables. |
| Timing | `C_Timer.After` for 0, 0.01, 0.1, and 1 second, 10 steps of a ticker, the order of three timers that are due together, and `GetTime` against `time()`. |
| Screenshots (7.1) | For each shot: the time from `Screenshot()` to each event, and when "Screen captured" shows. |
| Golden strips | Payloads of 0, 1, 62, 137, 500, and 3200 bytes, and two records as the relay sends them. With 62 and 137, the tag sits alone in the last row (7.1). One more strip hides right after its `Screenshot()` call: it tells whether the picture comes from the call or from the end of the frame. |

**The public test key** is the 32 bytes `gnomish-relay public test key 01`. It signs only the golden strips, never a message. Each strip has the frame time 1790211079 and its own frame id, so each vector is reproducible.

**Collect.** WoW writes saved variables only at a `/reload` or a logout. After the `/reload`, `gnomish-relay selftest collect [folder] [--out <repo>]` does this:

1. It reads `GnomishRelaySelfTest.lua`, the newest one of all accounts, with the limits of `saved.rs`. It refuses results that name another key than the public test key.
2. It scans the `Screenshots` folder for PNGs from the time of the run. It decodes each one with the real bridge reader and the test key, and keeps a file only when its time, frame id, and payload match a shot. It never takes a path from the saved file.
3. It writes `tests/fixtures/forever-<build>.json`: the measurements, and the behavior of the fake game that follows from them. It deletes the placeholder fixture.
4. It writes `tests/vectors/<build>/`: each PNG, `manifest.json` with each payload and the key, and the raw saved file, which shows how WoW writes saved variables.

It reads no key and no config of the relay, and it never deletes a screenshot. A running bridge leaves the test strips alone: their tag fails, so the bridge only logs them.

**The fake game.** The tests load the newest real fixture, or `tests/fixtures/forever-placeholder.json` while none exists. The placeholder holds the guesses of the fake game from before the self-test, and it says so. The fake game takes these values from the fixture: `GetBuildInfo`, the screen size, the delay of the slowest shot, the event of a good shot, when the picture is taken, when "Screen captured" shows, the returns of `LoadAddOn` and `FontString:SetFont`, whether `GetContentHeight` waits for the next frame, whether the saved variables load before or after the files, the login events, the timer order, the `bit` results, and `hooksecurefunc` on a missing global. The addon tests also run the relay in the other behaviors that it depends on: "Screen captured" before and after the event, a picture after the handler, saved variables after the files, a content height in the next frame, a disabled slot with and without a working `EnableAddOn`, an out-of-date slot, and a `hooksecurefunc` that refuses a missing global. A timer order that the fake game has no model for stops it at load. A test also fails when the measured shot delay no longer fits the one-second waits of the addon tests.

**Tests.** `crates/bridge/tests/golden.rs` decodes every committed vector on all three OSes. It skips with a message only while no real fixture exists. When a real fixture exists, a missing vector folder fails, and so does a placeholder that is still there. `crates/bridge/tests/selftest.rs` runs the self-test addon in the fake game, and collect on what it leaves.

**The API gate.** The self-test calls functions that the relay must never call. So it has its own lint list (`selftest.yml`), and `scripts/selftest-api.sh` writes its own API files (`addon/tests/selftest-api.lua` and `selftest-api-signatures.lua`). CI and the nightly job run both gates.

**After a client patch:**

1. Close the game, and run `scripts/selftest-link.sh`.
2. Start the game and log in. When the chat says "done", type `/reload`.
3. Run `gnomish-relay selftest collect` in the repo, run the tests, and commit `tests/fixtures` and `tests/vectors`.

The first run ever needs one more `/reload`: its first session has no saved file, so it cannot see the load order. Collect says so.

**Decisions.** An advisor agent and the implementer chose these (2026-09-26):

- The results go into the saved variables as hex of JSON. The bridge reads hex fields as it reads the outbox frames. Plain Lua tables need a Lua parser in Rust, and they depend on the way WoW escapes a string, which the self-test measures only now.
- Collect finds each screenshot by the frame that it holds, not by a name or a time. WoW names a screenshot by the second, and the saved file is untrusted text.
- The strips at other UI scales run only on `/grst scale`. `UIParent:SetScale` is allowed out of combat, but a fight that starts before the restore blocks it. The addon then restores the scale at the end of the fight.
- The run starts after `PLAYER_ENTERING_WORLD`, not at `PLAYER_LOGIN`: a shot at login can catch the loading screen.
- The placeholder fixture is the only place for the guesses. The fake game has no second copy of them.

Each target runs in CI for a short time and nightly for a long time. Every crash becomes a regression test.

| Target | Why |
|---|---|
| Frame decoder and record parser | Backs up S1 and S3 on the compiled code. The `frame` target also backs up S29 with two keys. |
| PNG decoding with the size limit | Any local program can write to the Screenshots folder. We did not write the PNG decoder. |
| Hook socket messages | Any process of the same user can connect. |
| `resolve_folder` with Unix and Windows path forms | Windows has `\\?\`, UNC paths, `C:foo`, `file:stream`, and reserved names such as `CON`. S5 must hold for all of them. |
| Lua escape, with the output loaded in a real Lua 5.1 VM | Backs up S8 against the real Lua parser. Inputs include NUL bytes, invalid UTF-8, and `]]`. |
| UI escape and popup text | Backs up S10 and S15. |
| `config.toml` parser | A broken or hostile config gives an error, never a wider permission. |
| Restore and live files, loaded in a real Lua 5.1 VM | Back up S18 to S21: every field loads back as the prepared bytes, and each file stays under its bound. |
| Flags from the game | `perm=`, `level=`, `build=`, and `agent=` take only values of the right shape, and `mkdir=` only `1`. A coding flag never changes the transport flags, which are all that the Timeways lane reads. Any `ver=` gets an update text exactly when it is out of the range of its app. |
| Messages from an ACP agent | The agent is untrusted. A progress line stays short, a popup text is printable (S15), and the game never gets "allow always". |
| The Markdown renderer (7.3.1) | Agent text reaches the game window. Each block has its shape, no agent byte starts a WoW code or HTML markup, and the size stays within its bound. |
| Messages of `codex app-server` | The agent is untrusted. A progress line stays short, and a popup text is printable (S15). |
| Lines of `claude -p` and Claude Code session files | The agent and its files are untrusted. A progress line stays short, a popup text is printable (S15), and a copy of a session keeps no old id. |
| The bridge state machine (`relay`) | The promises of the transport model (14.2) on the real code, with a Timeways lane next to the relay lane: no Timeways record becomes a job. A job that makes a new folder (9.9) is a first message, and the last part of its folder passes the name rules. |
| The action classifier and the shell splitter (6.6.3) | Backs up S16, S17, S27, and S28 on the compiled code: no panic, no rule list above the ceiling, a file call that runs stays inside its folders, and the command floor holds. |
| Lines of the story program and batches of the addon (`app_protocol`, 9.8) | Both are untrusted. No line panics a reader. A batch that passes has at most one line with a reply, and each line that goes on is JSON with the `id` of the bridge. An answer that passes is at most 24576 bytes, and its reply for the game is one JSON line with every `\|` doubled (S10) that the slot writer keeps whole (S12). A journal that passes holds no `note`. Both answers to a model call are one JSON line with its `call`. |
| The sandbox policy, the Seatbelt escape and profile, and the `bwrap` arguments (`sandbox`, 6.6.4) | Backs up S31 and S32 on the compiled code: each writable path is the chat folder or the temp folder and is not hidden, each `deny` folder is hidden, each path reads back from its literal with the model of S32, the profile holds exactly the expected literals in order, and `bwrap` binds each writable path and ends with the command. |
| "Always allow" (`always`, 6.6.5) | A command of the agent makes each rule. Backs up S36 to S39 on the compiled code: each proposal is the first words of its command and holds only plain words, and each offer makes the classifier give `allow` under a ceiling of `allow`. |
| `rules.json` (`rules_file`, 6.6.5) | A local program or a damaged disk can change the file. No text panics the reader, and each rule that loads has the shape that `propose` makes. |
| The head of a `CONNECT` request to the proxy of the sandbox (`connect`, 6.6.4) | A command of a game run writes it. No input panics the parser. A target that passes is a host name in lower case, never an IP address in any form, and the allow list matches only its exact names. |
| Answers of a local model (`model_http`, 9.7 decision 10) | The local model is untrusted. No answer panics the reader. The text that goes to the story program is at most 16 KiB, has no control character but a newline and a tab, and its `model_answered` line is one JSON line. |
| The stdin of a hook (`hook_input`, 10.7) | Any agent version writes it. No input panics the hook, and its spool file is one JSON object of at most 4 KiB. |
| A spool file (`notice_file`, 10.7) | Any local process of the user writes it. No input panics the reader, each accepted text passes S40, and a sequence of files keeps the table of S41. |
| The settings of an agent (`hooks_merge`, 10.7) | The user writes them. The merge never panics. It refuses and changes nothing, or its result parses, holds every key and hook of the input, and holds each group of ours once. |

The addon parsers have seeded tests in the addon harness instead of a fuzz target: the folder tree of 9.9 and the settings list of 13.4 each get 600 random inputs, and each result keeps its rules.

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
- The environment of the story program: it contains only the allowlist, with and without `bwrap`.
- The story sandbox on Linux with a real `bwrap` (`crates/bridge/tests/story_sandbox.rs`): a write outside its folder fails, also to `state.json` of the lane and to the lore pack; a read of the config folder, the keys, the data folder, and `~/.ssh` fails, and a read of the lore pack inside the hidden data folder works; a connect to a port that answers outside the sandbox fails inside it. With no working `bwrap` these tests skip with a message. CI installs `bwrap` on Linux and sets `GNOMISH_REQUIRE_BWRAP`, so there they cannot skip.
- A hang of the story program: its child process stops too (Linux).
- The command sandbox with the real tool: `bwrap` on Linux and `sandbox-exec` on macOS (`crates/bridge/tests/command_sandbox.rs`). Each test runs a command through `gnomish-relay --sandbox-run`, as Claude Code does. A write inside the chat folder and the temp folder works. A write outside fails: to another project, the home folder, `/tmp`, and `/var/tmp`. A read of the strip key, the state of the bridge, `~/.ssh`, and a `.env` in the chat folder shows no secret. A git hook cannot change. A child process of a child process stays inside. A link out of the chat folder writes nothing and reads no key. A connect to a port that answers outside fails inside. Quotes, line breaks, and `$(…)` in a command stay inside. The home folder of the tests has a `"`, a `\`, and a space in its name, so each path of the profile needs the escape of S32. CI sets `GNOMISH_REQUIRE_BWRAP` on Linux and `GNOMISH_REQUIRE_SANDBOX_EXEC` on macOS, so there they cannot skip.
- The proxy of the command sandbox with the real tool (the same file). The proxy of the test knows `allowed.test`, which it resolves to a public address, and its connect step leads that address to a web server of the test; the address check stays real. Through the proxy, `curl` inside the sandbox reaches `allowed.test`. A host that is not on the list, an IP address, and a name that resolves to `127.0.0.1` or `192.168.1.1` get `403`. A connection that skips the proxy, to a port that answers outside, fails and reaches nothing. One small test fetches `https://index.crates.io/config.json` through the real proxy with the default hosts, so TLS inside the sandbox is checked; it skips when the computer is offline. On macOS, a keychain of the test holds an item that any program reads with no prompt: a command with no proxy reads it, and a command with the proxy does not. A live test marked `#[ignore]` runs `cargo fetch` of a small crate inside the sandbox, with the real home folder, and checks that the cache of the host did not change.
- The copy-on-write view of `~/.cargo` with the real `bwrap` (the same file): a command reads a cached crate and writes a new one, the new one lands in the temp folder of the run and not in the home folder, and `credentials.toml` in the view shows no secret. With a `bwrap` that cannot make the view, as in CI, this test skips.
- The proxy in its public mode, and `local_ports` (`proxy.rs`): a public host that is on no list gets a tunnel, a name that leads to this computer is still refused, a listed local port reaches the loopback of this computer, and a port that is not listed, 2375, 2376, and 9222 get `403`.
- The wall of the agent with the real `bwrap` (`crates/bridge/tests/agent_wall.rs`), for `fake-claude`, `fake-codex`, and `fake-acp-agent`: a direct connection fails, a public host through the proxy works, a host that is not on the list gets `403` in `strict` mode, a name that leads to this computer gets `403`, a listed local port works and another port of this computer stays closed, a server of the agent answers on the loopback of its wall, `/proc/<pid of the bridge>` does not exist, a socket in the home folder and one under `/tmp` are out of reach, a startup file cannot change and a new one gets the notice, a grandchild of the agent ends with the wall, and the socket of the agent proxy lies in the data folder and goes away with the run. A command of `fake-claude` runs in the command sandbox of the run, reaches the command proxy, gets `403` for a public host that is not a package host, and does not see the socket of the agent proxy. Live tests marked `#[ignore]` run the real `claude` in its wall in both modes, with a Bash call in the command sandbox, and a model call of Timeways in the strict wall. On macOS, a test checks that Seatbelt cannot start inside a Seatbelt wall.
- One sandbox for each run with the real `bwrap` (`crates/bridge/tests/command_sandbox.rs` and `launch.rs`): a server that one command starts in the background answers a later command and nothing outside, a background process ends with the run, a command has no capabilities and cannot unmount a hidden path or remount `/`, `cd ..` does not leave the walls, with no holder a command does not start, and when the wrapper goes away the group of its command stops.
- The proxy with no sandbox (`proxy.rs`): an allowed host gets a tunnel to exactly the address that the check passed, a name with one public and one private address is refused, and each IPv6 form of a private IPv4 address is refused. A plain HTTP request gets `405`, a port other than 443 and 80 gets `403`, a head that is too long or never ends gets `400`, and a connection over the limit gets `503`.
- Claude with the real sandbox (`claude_gate.rs`): the scripted `claude` runs an allowed command through the prefix, and the command writes its chat folder and nothing outside. A command that ran without the wrapper stops the run. With no sandbox, a command of the allow table asks in the game, and the reply carries the notice.

### 14.6 Supply chain

- `cargo deny` (licenses, sources, duplicate versions) and `cargo audit` (known CVEs) run in CI.
- `Cargo.lock` is in the repo.
- The verified core (`crates/protocol`) has no dependencies. Every dependency there is code that we trust but do not prove.
- New dependencies in the bridge need a reason in the commit or PR.

## 15. Build order

0. **Start WoW once.** This makes `Interface/` and `WTF/Account/`.
1. **Done: `Screenshot()` spike.** A test addon draws a strip and calls `Screenshot()` from an event, with no key press. If a PNG appears, WoW writes the strip image itself, and the bridge needs no screen capture. The addon hides the "Screen captured" text through the `ActionStatus` frame.
2. **Cut: capture spike.** Step 1 passed, so the bridge has no screen capture.
3. **Done: Wine rules spike.** Test the five rules in 7.2 under Wine: the `ctl` self-test, a fresh read of a load-on-demand file, and "a new file is not found". Results in `spikes/README.md`. The HMAC-SHA256 cost in WoW Lua is not measured yet.
4. **Done: `protocol` crate with Aeneas.** Frame, cells, records, slot body, escapes, and every theorem in 14.1. `VERIFICATION.md` has the status.
5. **Done: slot writer.** Publish a fixed reply. Make sure that it shows in the game. Passed in the game on 2026-09-24: `install`, then `say`, then `/relay poll` showed the reply. The steps are in `addon/README.md`.
6. **Done: addon port** with the stub harness (the fake game, `addon/tests/wow.lua`) and the differential tests (`crates/bridge/tests/addon_codec.rs`).
7. **Done: Quint model** of the transport. **Done (7a):** the bridge reads strips from screenshots, checks the tag and the time, queues per chat, runs an echo agent, and publishes. Tests run one message around the whole loop. **Done (7b, part):** the addon signs each message at send, and the bridge reads the signed outbox frames from the saved variables. **Done (7b):** `state.json` and the restore bundle in `Restore.lua`. Passed in the game on 2026-09-24: a message went out as a strip, and the echo came back through the slots.
8. **Done: threat model in code:** `allowed_roots`, the policy, and the MAC check. **Done (8a):** `config.toml`, the `level` flag under the ceiling of the config (S6), and "Agent not set up." **Done (8b):** the action classifier (6.6.3) in `protocol`, with S16, S17, S27, and S28 proved, and the input of the classifier in the bridge. **Done (8c):** every backend calls the classifier through one gate (6.6.3, 9.3): the hook of Claude for every tool call, the approvals of Codex, and the permission requests of ACP agents. The config has its allow table, and `gnomish-relay approve` answers desktop requests.
9. **ACP backend.** **Done (9a):** any ACP agent from one config entry, `check-agent`, the process limits, and permissions under the ceiling. **Done (9b):** session resume and Stop for a run in progress. **Done (9c):** progress and permission requests in `Live.lua`, the popup in the addon, and the checked `perm=` answer. **Done (9d):** Markdown replies show as blocks in the window (7.3.1), with S22 to S25 proved. **Done:** live tests with Claude in the game on 2026-09-26 (9.3).
10. **Done: "Always allow" (6.6.5, 9.3).** One click in the game adds a rule that the sandbox bounds, with S36 to S39 proved. The Settings tab and `gnomish-relay rules` list and remove the rules.
11. **Notifications from terminal sessions (section 10).** **Done:** the pure parts in `protocol`, with S40, S41, and S20 restated. **Next:** the rest. Approved on 2026-09-28: the hook subcommand, the spool folder, one notice for each session with S40 and S41, the notices in `Live.lua` (S20 restated), the bell at the minimap, and `hooks install`. No `note` signal: signals do not work (7.4).
12. **Done: a generic backend for any LLM coding harness (9.2).** `acp` for any harness that speaks ACP, the `claude` and `codex` backends, and `command` for a harness that has only a command line, inside the sandbox, with presets for aider, gemini, opencode, goose, and llm. **Next:** a live test of each preset with the real tool.
13. **Voice (13.3). Not planned now.** Voice output first, then push-to-talk with its privacy rules.
14. **Done: a deeper API gate.** `scripts/wow-api.sh` checks that each WoW name exists and is not deprecated, and that each registered event exists. It also writes `addon/tests/api-signatures.lua`: the arguments, the returns, the payload, and the secret and restriction flags of each used function, widget method, and event, from the generated API docs of the client. A new secret flag breaks an addon, even when the name stays the same, so any change fails CI and the nightly job (7.8). The script takes the addon folders and the output paths as arguments, so the Timeways repo and the tank addon repo can run it too.
15. **A second app: Timeways (9.7).** The steps are in 9.7, "Order of the build". **Done:** steps 1 to 8, with 5b. Step 5 is the app protocol (9.8), the story sandbox (6.6.4), and the life cycle, with a loopback in the fake game. Step 6 is the model calls with no tools, through `claude -p` or a local model, and the budget (9.7, decision 10). Step 7 is the shared strip corner (7.1.2) with its Quint model. Step 8 is setup for two apps (9.7, decision 15) and the version range of each app (7.7, S30). **Next:** a loopback in the real game, when Timeways ships an addon build.
16. **Done: the command sandbox (6.6.4).** The policy (S31) and the Seatbelt escape (S32) are proved. Each command of Claude from the game runs in `bwrap` on Linux or `sandbox-exec` on macOS, and Codex writes only its chat folder and a private temp folder. Windows and a computer with no working tool get the fallback. **Done:** the proxy for commands (6.6.4): a command reaches only the allowed package hosts, through a Unix socket and a forwarder on Linux and one loopback port on macOS. **Done:** the agent process behind the proxy on Linux (6.6.4, "The agent process behind the proxy"), `local_ports`, and one sandbox for each run. S33 to S35 are proved. **Stopped:** the Windows launcher with an AppContainer (`rappct`), because Git Bash cannot start in an AppContainer (6.6.4, "Windows").

Steps 1 to 5 prove the channels. After those, the rest is normal Rust work.

## 16. Development environment

- `dev gnomish-relay` opens tmux with nvim, the agent, and a terminal in this folder.
- Link `addon/GnomishRelay` into `_classic_beta_/Interface/AddOns`. Then an edit plus `/reload` loads the new code, with no copy step. `scripts/dev-link.sh` does this, and also links each file of `addon/transport` into `addon/GnomishRelay`. Git ignores these links.
- After each client patch, run the self-test of the game (14.3.1): `scripts/selftest-link.sh`, a login and a `/reload`, then `gnomish-relay selftest collect`. Commit the new fixture and vectors. `scripts/selftest-link.sh --remove` takes the self-test out of the game.
- Run the bridge in the bottom-right pane.
- Aeneas and Charon are built in `~/verif`. `proofs/TOOLS` pins their commits, and CI builds the same commits with Nix.

## 17. Open questions

- When can Windows get a sandbox for the commands of Claude (6.6.4, "Windows")? Try the AppContainer again when `msys-2.0.dll` starts in an AppContainer (microsoft/mxc issue 1061), or when Claude Code runs its commands through a shell other than MSYS2.
- What does `permissions.<profile>.filesystem.deny_read` of Codex take, so that Codex can hide the `deny` and `desktop` paths (6.6.4)?
- Not planned now (voice, 13.3): does `C_VoiceChat.SpeakText` have any voices under Wine? A spike calls `C_VoiceChat.GetTtsVoices()` in the game.
- Not planned now (voice, 13.3): can the bridge take a global push-to-talk hotkey on Wayland through the GlobalShortcuts portal?
- Two WoW accounts on one computer have two tokens. A hello from the second account starts a restore, and its `restored` flag retires the first token. How does the bridge tell two accounts from a saved-data wipe?
- Not planned now: can font files replace the `.wav` signals? The slot polls of 7.3 work without signals.
- How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
- Does Gemini CLI have hooks for notifications?
- How large is the hitch at a higher window size? (The "Screen captured" hide works.)
