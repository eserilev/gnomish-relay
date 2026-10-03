# Gnomish Relay: Specification

Status: draft 4, 2026-09-29. Section 15 shows what is built. "Not planned now" marks a part with no owner and no date. Its text stays as a design note.
Draft 2 applies a review against the `wow-claude` source code.
Draft 3 applies the spike results in `spikes/README.md`: the strip goes out through `Screenshot()`, and `.wav` signals do not work.
Draft 4 makes the spec match the code and marks the parts that are not built.

## 1. Summary

Gnomish Relay connects AI coding agents to World of Warcraft: Forever and WoW Classic: TBC Anniversary (7.9).
You send a task from a chat window in the game. The agent does the work on your computer.
The reply comes back into the game with a whisper sound.

Gnomish Relay also shows notifications from agent sessions in a normal terminal (section 10).
When a terminal session ends a turn or needs input, a message appears in the game.

Gnomish Relay has two parts:

- **The addon**: a WoW addon in Lua. It shows the chat window.
- **The bridge**: a Rust program on your computer. It moves messages between the game and the agents.

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
- Other WoW versions. Gnomish Relay supports only the clients of 7.9.
- A hosted service. Everything runs on the local machine.

## 4. Prior work and credits

Gnomish Relay uses the design of two earlier projects:

- [chelinho139/wow-claude](https://github.com/chelinho139/wow-claude) (MIT), now `chelinho139/wow-ai`.
  It is a Windows-only Node bridge for Claude Code. It inspired the addon and the transport. Gnomish Relay has no code from it.
- [0xInuarashi/wow-forever-codex](https://github.com/0xinuarashi/wow-forever-codex).
  It measured the file-load rules of the Forever client and invented the pixel-out channel.

The README credits both projects. `wow-forever-codex` has no license file, so Gnomish Relay takes no code from it.
References to `wow-claude` files use the path in that repo, for example `bridge/protocol.js`.

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
The bridge treats all four as untrusted.

### 6.1 Attackers

| Attacker | How | Defense |
|---|---|---|
| Another window over the game (browser, video, overlay) | Shows a fake strip | WoW takes the screenshot itself, so other windows are not in it. Each strip carries a MAC. |
| A local program | Drops a crafted PNG into the Screenshots folder, or replaces a slot folder with a symbolic link | MAC (6.3) and freshness check (S11). Image size limit before decoding. No writes or deletes through symbolic links (6.2). |
| A local program | Writes fake notifications into the spool folder (10.2) | The folder has mode 0700, and game runs cannot reach it. Size limits, exact fields, one notice per session (S41). The text gets the same escapes as agent text (S40). A notification never starts a run. |
| A malicious or prompt-injected agent | Writes a reply that injects Lua or fakes WoW chat links. Asks for permission with a false label. Writes a huge reply. | Lua escape and UI escape (S8 to S10). Honest permission popup (6.4). Size limits (S12). |
| An old screenshot | A strip is replayed from an old file, for example after `state.json` is lost | Freshness check (S11). |
| Another addon or a WeakAura | Runs Lua in the same environment as our addon. It can call our handlers, fill our input box, click our buttons, read and change `GnomishRelayDB`, and replace a slot body during a load. | Signed state (6.6.1) stops changes to stored messages. A call to our handlers gets no more than a typed message: the ceiling, the classifier, the sandbox, and desktop approvals (6.6.2 to 6.6.4, 9.3) bound every game message. It can ask for a new chat folder; only a desktop click adds one (9.12). It can ask for full-auto in a chat; only a desktop click gives it (9.3, "Full-auto for one chat"). The exception is a chat that the user switched to full-auto: a message in it, from any addon, runs with no question, inside the sandbox. |
| A prompt injection in a file | The agent reads a README, an issue, or a web page with hidden instructions | The action classifier (6.6.3) and the sandbox (6.6.4). Layer 1 does not help: the prompt came from the user. |
| A stream or recording | The strip shows the prompt on screen | None. Do not stream while you use the relay. The README says this. |

A hostile addon in the same Lua environment can call every entry point of our addon.
No WoW mechanism proves that the user typed a message (6.6.1).
So the bridge bounds what any game message can do (6.6).

### 6.2 Bridge policy

1. The folder of a chat must be inside `allowed_roots` from the config. A folder in the home folder under no root runs only after a desktop click adds it to the roots (9.12). The bridge rejects all other folders.
2. The permission level of each agent comes only from the bridge config. A game message cannot raise it.
3. A permanent "always allow" rule from the game follows 6.6.5.
4. The bridge limits the message rate to 10 messages per minute (planned config key `max_messages_per_minute`, 12).
5. The bridge never runs the agent at `full-auto` without a desktop Approve for that chat and its folder (9.3, "Full-auto for one chat"). The config can turn full-auto off for every chat (`allow_full_auto = false`, 12). The sandbox still applies (6.6.4).
6. The bridge rejects frames with a timestamp more than 5 minutes old or more than 1 minute in the future (S11).
7. The bridge never writes, renames, or deletes through a symbolic link. It opens files with `O_NOFOLLOW` (Unix) or checks the reparse point (Windows).
8. The bridge deletes only screenshots that decode as a frame with a good checksum, because a normal screenshot never does. It deletes a valid strip after it takes it. It also deletes a strip that is old, early, or signed with another key: such a strip never becomes valid, and its pixels hold a prompt. It logs one line with the next step. It keeps a strip of the public test key (14.3.1) for `selftest collect`, and a frame that fails for another reason. It never deletes other screenshots.
9. The bridge limits sizes: an image before decoding (4096 × 4096 px), a hook message (64 KB), a reply record (32 KB), a slot body (S12), and each chat queue (20 messages). It reads a screenshot file or a saved variables file up to one byte past the limit, and refuses a bigger file unread. A size check before the read misses a file that grows. The screenshot limit is the PNG of a 4096 × 4096 image with 16-bit RGBA and no compression, plus 1 MiB (129 MiB). The saved variables limit is 16 MiB.
10. The bridge resolves symbolic links in a chat folder with `canonicalize`, then checks `allowed_roots` again on the result. The relay checks only the folder text when the message comes. So at the start of each run, after a new folder is made (9.9), the bridge resolves the folder once. It passes only the real path to the agent, the gate, and the sandbox. A missing folder, or a link that leaves every root, ends the run with an error, and nothing runs.
17. The proved resolver (S5) splits paths only at `/`. On Windows, the bridge first turns each `\` of a game folder into `/`, so each `..` counts. It refuses a game folder with `:`, which starts a drive or names a stream. Roots lose the `\\?\` prefix of `canonicalize`. A game folder that starts with `~/` resolves from the home folder (9.9). The resolver gets the rest, so S5 holds for it as for any other text.
11. The bridge never starts a process through a shell. It passes the command as an argument list.
12. The bridge gives each agent process only an allowlist of environment variables (`PATH`, `HOME`, `LANG`, `TERM`, and the variables in the agent config). All others, for example API keys of other tools, stay out.
13. The bridge writes prompt files with mode 0600 in a private folder, and deletes them after the run.
14. Setup writes `config.toml` with mode 0600. The bridge refuses a config that other users can write, because the config sets the ceiling of every game message. The strip key is in its own file, `strip.key`, with mode 0600. Each private file (`config.toml`, the keys, `state.json`, `rules.json`, and the walls files of the sandbox) has mode 0600 from the moment the bridge makes its temp file, so no chmod comes after the rename. The bridge opens `bridge.log` and the files of `logs/` (8.5) with mode 0600 and never through a link. The config folder and the data folder have mode 0700, also when an older bridge made them with the umask.
15. The bridge escapes control characters and newlines in `bridge.log` and in the JSON log (8.5), so a prompt cannot fake a log line.
16. The bridge sends a restore bundle only in answer to a hello with a valid MAC.
18. A control record of the coding app (Stop, Delete, a permission answer, a rule removal, or a hello) applies once per frame (fixed on 2026-09-29; the tests came first). Its id is 0, so the replay store (S7) cannot tell two of them apart, and a replayed strip would stop or delete a later run. So the bridge keeps the tag of each frame for 360 seconds after first sight, while the frame passes S11 (`MAX_AGE` + `MAX_AHEAD`). A frame with a known tag applies no control record and no report again. Its messages still go through the replay store, so a refused message gets its next chance. The tags and their first-sight times are in `state.json`, written in the same step as the frame. So a replayed frame after a bridge restart also applies no control.

### 6.3 Strip authentication

Setup makes a random 32-byte key.
It writes the key into the key addon of the app (7.3.2) and into `strip.key` in the config folder.
Each strip ends with a truncated HMAC-SHA256 tag (8 bytes) of the header and payload.
The bridge drops each strip with a wrong tag, deletes its screenshot (6.2, rule 8), and logs it.
The bridge compares tags in constant time (`subtle::ConstantTimeEq`).

HMAC-SHA256 in Lua costs about 0.1 ms for a full 3221-byte strip under LuaJIT with the JIT off. The plain Lua 5.1 of WoW is a few times slower, still well under 1 ms.

### 6.4 Honest permission popup

A malicious agent can ask for permission with a false label, for example "run tests" for `rm -rf ~`.
So the popup never shows the agent label as the main text. Rules:

- The popup text comes from the raw tool input: the real command line or file path.
- If the text is too long, the popup shows the start and the end, with a visible "cut" mark between.
- Control characters, Unicode bidi characters, and zero-width characters show as visible escapes, for example `<U+202E>`.
- The agent label shows below the raw command, marked as "the agent says".
- When the popup offers "Always allow" (6.6.5), one more line names the exact rule and its folder. The bridge makes the line, and the popup never cuts it.
- A misclick must not allow. A new popup plays the ready-check sound, and its buttons ignore clicks for 1 second, because the player can be mid-click in the game. **Deny** and **Always deny** sit at the left, the allow buttons at the right, with a wide gap between.
- When more requests wait, the popup says "1 of 3" at the top right. It shows the oldest request first.
- The popup has the dark dialog border of the game. Its height follows the text, up to 600 pixels, so a long command never runs over the buttons. The text uses the shipped mono font (13.2).

Theorem S15 covers these rules.

### 6.5 Known leaks

- Reply text sits in a global table after a slot loads. Any addon can read it.
- `GnomishRelayDB` is a global table. Any addon can read the chats in it.
- The strip is signed, not encrypted. The prompt is in the pixels of each strip screenshot until the bridge deletes it. If the bridge does not run, these files stay until its next start deletes them (6.2, rule 8). A cloud sync of the Screenshots folder (for example OneDrive on Windows) copies them.
- An addon that loads before ours, for example `!Evil`, can replace global functions such as `string.char`, `tonumber`, or `bit.band` before `KeyHandoff.lua` and `Sha256.lua` run. It can then read the strip key. It can also read the global of the key addon in the short time that it exists (7.3.2). Lua in WoW cannot stop this. Layers 2 to 4 of 6.6 assume that any game message can come from another addon, so the key guards against programs outside the game, not against other addons.
- Code in the sandbox can still send data to the allowed API host, for example with an upload under another account key. A proxy that ends TLS and pins the account closes this. It is not in v1.
- A command of a game run can send data to each host of the proxy list (6.6.4), for example a push to `github.com` with its own token. The list limits where a command connects, not what it sends.

### 6.6 Four layers of defense

Each layer covers a hole in the layer before it. No layer depends on a model that judges another model.

| Layer | Question | Where |
|---|---|---|
| 1. Signed state | Did anything change a message after our code signed it? | Addon |
| 2. Game ceiling | What can a game message do at most? | Bridge config |
| 3. Action classifier | Does this tool call run, ask in the game, ask on the desktop, or never run? | Bridge, proved in `protocol` |
| 4. Sandbox | What can happen when layers 1 to 3 fail? | Operating system |

The trust of "always allow" (6.6.5) rests on layers 2 to 4, never on layer 1.

#### 6.6.1 Signed state

**Signed state.** Another addon can change `GnomishRelayDB` without a call to our code. So the addon signs messages from private state:

- The addon keeps the text of each open message in its private table (`ns`), not only in `GnomishRelayDB`.
- When the user sends a message, the addon signs it at once. It stores the signed frame and its time in `GnomishRelayDB`, next to the text.
- After a `/reload`, the addon sends only frames with a valid tag. It never signs text that it reads back from `GnomishRelayDB`.
- A stored or outbox frame older than 270 seconds is too old for the bridge (S11 allows 300). The message then ends with "Not sent." and a Resend link, and the user decides. While the bridge is offline (7.4), the text is "Not sent: the desktop app isn't running. On your desktop, run gnomish-relay restart."
- An outbox entry (7.5) is the same signed frame. The bridge checks its tag, time, and the replay store (S2, S11, S7), as for a strip.
- A permission answer carries a hash of the exact popup text: `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256, in hex. The bridge refuses an answer whose hash does not match its own text of the request.

The key reaches the addon through its key addon (7.3.2). After the load, it lives only in the private table `ns`.

No WoW mechanism lets an addon prove that the user typed a message. For example, another addon can fill the chat box with `/ai …` and wait for the user to press Enter. So layers 2 to 4 assume that any game message can come from another addon.

#### 6.6.2 Game ceiling

Every game message (a strip or the reload outbox) runs under one ceiling from the desktop: `config.toml`, plus the chats that the user switched to full-auto on the desktop. No game message can raise it. Only a desktop click changes the config (9.3, "Raise the level") or approves full-auto for a chat (9.3, "Full-auto for one chat"). No addon can make that click (S6).

| Setting | Default |
|---|---|
| Write | The chat folder only (the `auto-edit` level of 9.3) |
| Read | `allowed_roots` |
| Commands | At `auto-edit`: the commands that the command sandbox holds (6.6.4, "The sandbox answers at `auto-edit`"), the allow table of the config, and the "Always allow" rules of the folder (6.6.5). All others ask. |
| Network | Commands go only through the bridge proxy, to the allowed hosts (6.6.4). The agent process reaches public hosts through its own proxy, and nothing on this computer but `local_ports` (6.6.4, "The agent process behind the proxy"). Each network tool of the agent asks on the desktop (6.6.3). |

- The `full-auto` level of a chat (6.2 rule 5, 9.3) asks no question, in the game or on the desktop. Only the `deny` answers, the sandbox, and the walls for file tools still apply (9.3, "Full-auto for one chat").
- The allow table of the config (12) covers commands. A covered command runs with no question at `auto-edit` and `full-auto`. It never covers a `deny`, `desktop`, or "never always" command (S17).
- The game never answers a permission request of a terminal session, and never sends a task to one. Terminal sessions only send notifications (section 10). The bridge does not run them, so this spec gives them no rules.
- The bridge shows a desktop notice for each game message: "New task from WoW: <first line>". The config can turn this off.

#### 6.6.3 Action classifier

The bridge classifies each tool call before it runs. It reads the structured tool input, never the prompt text. The same input always gives the same answer.

Four answers, from strict to open:

| Answer | Meaning |
|---|---|
| `deny` | Never runs. Only for the files that guard the relay itself. |
| `desktop` | The user approves on the desktop. The game shows a notice with no buttons. No addon can click a desktop prompt. |
| `ask` | The user approves in the game popup (6.4). |
| `allow` | Runs with no question. |

**How tool calls reach it.** The classifier sees only the tool calls that a backend sends to the bridge, so coverage depends on the backend. `crates/bridge/src/gate.rs` is the one place that turns a verdict into an action (9.3), for every backend:

- **Claude (`kind = "claude"`): every tool call.** The bridge registers a `PreToolUse` hook in the `initialize` control request of `claude -p` (9.2). Claude Code then sends a `hook_callback` control request before each tool call, also reads and calls that the permission mode lets run freely. The hook answers `allow` or `deny` itself, after the gate. It never answers `ask`: that hands the call to the permission rules of Claude Code, which user settings can loosen. A `hook_callback` that the bridge cannot read gets `deny`.
  - Tools: `Read` reads `file_path`. `Write`, `Edit`, and `MultiEdit` write `file_path`, and `NotebookEdit` writes `notebook_path`. `Glob`, `Grep`, and `LS` read their `path`, else the chat folder. A `Glob` pattern that starts with `/`, `\`, or `~`, or holds `..` or `:`, is unknown. `Bash` is its `command`, in the chat folder.
  - Each path is the path that Claude Code opens (its `expandPath`, checked on 2.1.285). The bridge trims white space around it, `~` and `~/` start in the home folder, and a relative path starts in the chat folder. Else `" /etc"` would be `chat/ /etc` to the bridge and `/etc` to Claude Code. A path with a NUL byte, any other leading `~` (such as `~other`), or a path that is still not absolute is unknown.
  - `Grep` of a folder also reads each hidden path in it. The bridge walks the folder as the sandbox walks the chat folder (6.6.4, "Hidden paths that exist"). The walk follows no link, and neither does `rg --hidden` of Claude Code. So a `.env` in the folder makes the call `desktop`, whatever its `glob` is. A failed walk makes the call unknown. `Glob` and `LS` show only names, so they get no walk.
  - Session tools run with no question: `ToolSearch`, `TodoWrite`, `EnterPlanMode`, `ExitPlanMode`, and `AskUserQuestion`. They change nothing outside the session, and a question for each would make Claude unusable.
  - Every other tool is unknown: `WebFetch`, `WebSearch`, `Task` and other subagents, MCP tools (`mcp__*`), and any new tool.
  - Checked live on Claude Code 2.1.282: the hook fires for a `Read` in `acceptEdits` mode. It also fires when `--settings` holds `disableAllHooks: true` and an allow rule for `Read`. So neither user settings nor an allow rule skips it. Claude Code runs the tool when it cannot read a `hook_callback` answer, so the bridge sends only well-formed answers.
  - A second line: the bridge tracks the id of each tool call that the hook answered. A `tool_result` with no error for any other id means that a tool ran unchecked. The run then stops at once with "Stopped: a tool ran without a check from Gnomish Relay.". That call already ran. A result with an error does not count, because a call with bad input fails before the hook.
  - `can_use_tool` still works. A call that the hook allowed gets `allow`. Any other call goes through the gate.
  - The Bash tool of Claude keeps its folder between calls. A relative redirect after a `cd` in an earlier call resolves from that folder, but the classifier resolves it from the chat folder. The sandbox (6.6.4) is the wall for this case.
- **Codex (`kind = "codex"`): every command and every file change.** The thread runs with `approvalPolicy: "untrusted"` at every level. In codex-cli 0.157.0 that asks before every patch, and before every command that no Codex `allow` rule covers, in both sandboxes (`core/src/exec_policy.rs`, `core/src/safety.rs`). The bridge classifies the script inside `<shell> -lc '<script>'`. These still run with no request, so without the classifier:
  - A command that a Codex `allow` rule covers: `/etc/codex/rules`, `$CODEX_HOME/rules` (for example `default.rules` from an "always allow" in a terminal), and the `.codex/rules` of a trusted project. Such a command also runs outside the sandbox.
  - A request that a Codex `PermissionRequest` hook answers, and an MCP server with `approval_mode = "approve"`.
  - The retry outside the sandbox of a command that the bridge allowed.
  - Input to a running command (`write_stdin`), `view_image`, MCP tools with `readOnlyHint`, the MCP resource tools, and the session tools (plan, tool search, sleep). Web search is off (`config.web_search = "disabled"`).
  - MCP tool approvals come as `mcpServer/elicitation/request`. The bridge declines each one.
  The Codex sandbox (6.6.4) bounds all of these. The bridge cannot give Codex an empty `CODEX_HOME`, because the login lives there.
- **Other ACP agents: only the calls that they ask about.** The bridge classifies each `session/request_permission`. The `kind` of the tool call gives the meaning of its paths (`read` and `search` read; `edit`, `delete`, and `move` write). The paths come from its `locations` and the `file_path`, `path`, or `notebook_path` of its `rawInput`. `execute` is the `command` of `rawInput`. Any other call is unknown. The agent decides what it asks, and unasked calls already ran. So for these agents the answer is at most `ask` at every level, even `full-auto`: no allow table and no game rule gives `allow`.
- A terminal session of Claude uses the same hook through `gnomish-relay-hook pretool`. This subcommand ignores `GNOMISH_RELAY_JOB` and fails closed: if the bridge does not answer, the answer is `deny`.

**Desktop approval.** The bridge runs in the background with no window. So it shows an OS dialog with Approve and Deny, and the command line is the fallback:

- The bridge writes each open request to `approvals/<id>.json` in the data folder (12), with mode 0600. The id is 12 random hex digits. The file holds the agent, the folder, the time, the popup text (S15), and the wait in minutes (`permission_timeout_minutes`). The dialog ends with "No answer in <n> minutes counts as Deny.", and `gnomish-relay approve` shows the minutes left of each request.
- Not yet: the reason for the desktop, for example "It reads ~/.ssh, outside the chat folder". The classifier (6.6.3) gives a verdict with no reason, so a reason needs a second proved function. `Core.lua` builds the in-game line from the `Desktop:` line, so minutes in the game need a change of that line and of the addon.
- `gnomish-relay approve` lists the open requests. `gnomish-relay approve <id>` allows one, and `gnomish-relay deny <id>` refuses one. Each writes an answer file next to the request with `create_new`, so it never follows a link. A request has at most one answer.
- The bridge checks for the answer every 100 ms, up to `permission_timeout_minutes`. No answer refuses the call. The bridge then deletes the files. At start it deletes the files of an old bridge.
- **The game gets a notice, not a popup** (decided with a UX advisor on 2026-09-26). The game sends no request for a desktop call and has no Deny for it. The desktop dialog is the only prompt, so the player never sees two prompts for one call.
  - The bridge writes its own progress line for the last desktop request of the run: `Desktop: <state> <id> <how>`, plus ` raise <level>` for a raise (9.3), or ` folder` for a new folder (9.12). `<state>` is `wait`, `approved`, `denied`, or `none` (no answer). `<id>` is the 12 hex digits of the request. `<how>` is `dialog`, or `command` when `desktop.rs` finds no dialog tool.
  - The line comes right after the level line (9.3, "The level in the game"), so S9 and S20 do not change, and `Activity` keeps at most 5 lines. `Activity::step` puts "agent: " before an agent line that starts with "Desktop:", as for "Level:". The id comes from the bridge, so the line holds no agent text.
  - A second bridge line comes right after it: `Desktop: asks <text>`. It says what the request is for. `<text>` is the first line of the popup text (S15) for a tool call, the first line of the request text for a merge, and the folder for a new folder. A raise has no second line: the whisper line names its level. The bridge removes control characters and cuts the text at a character boundary to fit `MAX_LINE`, with "..." at the end. The popup text already shows the raw command, so this line adds no new agent text to the game. An agent line that starts with "Desktop:" gets "agent: " before it, so no agent can write this line.
  - The addon takes the line only at its place, and only with a known state, a 12-digit id, and a known `<how>`. The Activity row then shows "Approve on your desktop", "Approved on your desktop", "Denied on your desktop", or "No answer on your desktop".
  - At a new `wait`, the game prints one whisper line with the whisper sound: `[Claude] whispers: [chat] Approve on your desktop.`, or `Approve on your desktop: run gnomish-relay approve <id>` with `command`. The addon keeps the ids of the last 16 requests that got a line in its saved variables, so a `/reload` does not print it again.
  - While a request waits, the addon loads a slot every 5 seconds, at most 24 times per request (7.3). Then it goes back to the schedule.
  - **The request in the chat** (asked for by the user on 2026-09-30, after a banner that never showed: "show the request in the game"). While the run has a desktop request, the chat shows a box between the transcript and the input. While the request waits, the box shows:
    - "Waiting for your approval on your desktop".
    - The text of `Desktop: asks` on one line, cut with "...", with all of it in the tooltip. For a raise: "Let <agent> work at <level>.".
    - A box with `gnomish-relay approve <id>` and a Copy button. WoW has no clipboard API, so Copy selects the text, and the player presses Ctrl+C.

    When the request ends, the box shows "Approved on your desktop", "Denied on your desktop", or "No answer on your desktop" until the run ends. The Activity row does not show the `Desktop: asks` line.

**The dialog** (decided with an advisor on 2026-09-26). In the first test in the game, the user saw only the game popup and did not know about the command. `crates/bridge/src/dialog.rs` shows the dialog with tools that the user already has. The bridge installs nothing.

| OS | Tool | Approve when |
|---|---|---|
| Linux | `notify-send -a "Gnomish Relay" -u critical --print-id -A approve=Approve -A deny=Deny`, only when the notice server lists the `actions` capability (`gdbus call ... GetCapabilities`). Else `zenity --question --no-markup --default-cancel`, only with `DISPLAY` or `WAYLAND_DISPLAY`. Else no dialog. | the last line of notify-send is `approve`; zenity exits with 0 |
| macOS | `osascript` with `display dialog`, buttons Deny and Approve, `default button "Deny"`, `cancel button "Deny"` | the output holds `button returned:Approve` and not `gave up:true` |
| Windows | PowerShell `MessageBox` with Yes and No, `Button2` (No) as the default, `DefaultDesktopOnly` to stay on top. The text starts with "Yes = Approve, No = Deny." | the output is `Yes` |

- The dialog text is "An agent in WoW wants to:", the popup text of S15 (the full raw command or path, then "the agent says"), then the lines "Agent: <name>", "Folder: <folder>", and "Request: <id>". It never shows only text that the agent chose.
- A merge request of Merge (9.11) has its own text: "A chat from WoW asks to merge <branch> into <start branch> in <folder>. Approve only if you just clicked Merge in WoW.", then "Request: <id>". Every name in it comes from git, not from the game.
- The text goes in an argument or an environment variable, never into a script. A notice server shows the body as markup, so the bridge escapes `&`, `<`, and `>` for notify-send. Else `<b>` or an S15 escape such as `<U+202E>` hides text. zenity gets `--no-markup`. The markup escape is unproved bridge code, so a named test covers it.
- Deny is the default button everywhere, so Enter never approves. Only a press on Approve approves. Only a press on Deny denies: the last line `deny` of notify-send, exit code 1 of zenity, the Deny button of osascript (its cancel button), and `No` of the message box.
- **A dialog that closes with no button press is no answer** (decided with the user on 2026-09-30: "a closed banner means no answer, and show the request in the game"). That day, on GNOME 50.5, a click on the banner closed it with no action, and the old bridge logged `Some(Deny) in the dialog`. The user said: "I did not deny." On GNOME, a click on the banner body, a banner that closes by itself, and a replaced banner all close it with no action. So the request keeps waiting, and the game keeps its line. Only Approve or Deny in a dialog, `gnomish-relay approve` or `deny`, Stop, a new message, or the timeout ends it.
  - Other cases of no answer: notify-send with no last line `approve` or `deny`, a zenity that times out (exit 5) or whose window closes (exit 3, from `ZENITY_ESC=3`), an osascript that gave up, and a message box with no `Yes` or `No`.
  - After a dialog closes with no answer, the bridge shows one more dialog for the request, then no more. After notify-send, that is zenity when the computer has zenity and a display, else the same notice again. zenity comes first because its window stays until a click: it cannot expire, and no other notice replaces it. The second dialog gives a player who clicked the banner by mistake one more chance. A third would start an endless loop of banners, so the game line and `gnomish-relay approve <id>` are the last prompts.
  - The close reason (Desktop Notifications spec, `NotificationClosed`): before notify-send starts, the bridge starts `gdbus monitor --session --dest org.freedesktop.Notifications`. notify-send prints the id of its notice first (`--print-id`). When the notice closes with no action, the bridge looks for `NotificationClosed (uint32 <id>, uint32 <reason>)` for up to 1 s, and logs the reason: 1 expired, 2 dismissed by the user, 3 closed by a call, 4 undefined. With no `gdbus` or no such line, the log says "reason unknown".
  - No signal of the spec says that a notice is on screen. GNOME puts a banner that it does not show (Do Not Disturb, or a full-screen window) in the notification list, and notify-send waits. So the bridge cannot detect a notice that never shows. The line in the chat covers that case. This happened on 2026-09-30 at 22:46: no banner showed, and the request waited until the player pressed Stop.
- The dialog runs in its own thread. Every 100 ms it checks whether its request still waits. When the request has a command-line answer, or the gate closed it (the timeout, Stop, or a new message, 9.3), the thread stops the dialog. It sends SIGTERM through `kill` first, because notify-send then closes its notice, and a kill after 0.5 s.
- The dialog answer goes through the same `create_new` answer file as the command line, so the first answer wins. A dialog answer after the request closed can leave an orphan answer file. Nothing reads it, and the next start deletes it.
- zenity and osascript also give up by themselves after one hour, in case the bridge stops first.
- Why notify-send first: it needs only the session bus, which the systemd service of the bridge has. GNOME keeps a critical notice until a click, with no timeout. With no `actions` capability, a notice has no Approve button and can never answer, so the bridge checks the capability for each dialog. The notice is not `--transient`, so it stays in the GNOME notification list, with its buttons, after its banner goes.
- Why no `kdialog`: it shows text that looks like HTML as rich text, with no option to turn that off. The KDE Plasma notice server has `actions`, so notify-send covers KDE.
- On Windows, the bridge starts PowerShell with `CREATE_NO_WINDOW`, so no console window flashes.
- With no dialog tool, the bridge shows a plain notice with "Run: gnomish-relay approve <id>", through `notify-send`, `osascript`, or a PowerShell toast. With none of these, the log line is the notice.
- The bridge logs each request, the tool of each dialog, and how each dialog ended: Approve, Deny, closed with no answer and the reason, or ended by another answer.

**The log of requests** (asked for by the user after the fresh-install test of 2026-09-30, when `bridge.log` did not say what the requests were for). When it opens a request, the bridge writes one line for each game question and for each desktop request:

- `game request: <agent> in <chat folder>: <summary>`
- `desktop request <id> (<kind>): <agent> in <folder>: <summary>. To approve, run gnomish-relay approve <id>. Wants to: <text>`. `<kind>` is `tool call`, `raise`, `merge`, or `folder`. A raise, a merge, and a folder request have no summary and no `Wants to`: the folder names them.
- `<text>` is the first line of the popup text (S15): the raw command with its escapes, or the path or the title. The bridge removes control characters and cuts it to 300 bytes at a character boundary, with "..." at the end. The user asked for it on 2026-09-30: the log said only `command ?`, so nobody could see what was asked.
- The summary comes from the classifier input, never from the popup text, the title, or the prompt:
  - A command is `command` and the names of its first 4 simple commands, for example `command cargo, tail`. A name is the last part of the first word. A name that is not a plain word (6.6.5, for example `TOKEN=x`) is `?`. A command that does not parse is `command ?`.
  - A file call is `read` or `write` and at most 3 of its paths, each relative to the chat folder when inside it.
  - Any other tool is its name when the title starts with a plain word, else `a tool`.
- So the log holds the command of each request as its own field, with secrets hidden (8.5). The desktop request line also gives it after "Wants to:". No file content and no "the agent says" text reach the log. The log escapes each line as before (`run::log`).
- Example: `game request: claude in /home/x/Code/app: command git`.

**Input.** The bridge builds the input in `crates/bridge/src/action_input.rs`:

- A file call carries its read paths and write paths. A shell command carries its raw bytes and its working folder. Every other tool call is "unknown".
- Each path resolves with `canonicalize` at check time. A new file resolves through its folder. A link to a missing file resolves through its target, because a write creates the target: `chat/x` with `x -> ~/.bash_aliases` is a write of `~/.bash_aliases`. A chain of more than 64 links does not resolve; the OS refuses to open it too. The path then has the form of `resolve_folder` (S5): it starts with `/`, and has no empty part, no `.`, no `..`, and no trailing `/`. On Windows the drive is the first part, for example `/C:/Users/x`. A path in any other form is `desktop`.
- The policy holds `allowed_roots`, the chat folder, the `deny` paths (the config folder and the data folder of the bridge, 12, and the private files of the game), the two lists of `desktop` patterns, and the allow table of the config.
- The game rules are "always allow" rules (6.6.5). Each rule is the first words of a command: `cargo test` covers `cargo test -q`. An empty rule covers nothing.
- A path or a command longer than 1 MiB is `desktop`.

**Case.** macOS and Windows compare paths without case, so `~/.SSH` is `~/.ssh` there. The classifier compares the `deny` folders and the `desktop` patterns without ASCII case on every OS. This is stricter, never looser. The checks for `allowed_roots` and the chat folder compare with case, which is also stricter.

**Rules for paths:**

- **Unknown tools are `desktop`.** The classifier knows file reads, file writes, and shell commands. Every other tool is `desktop`: web fetch, web search, MCP tools, and subagents.
- **Inside:** a path is inside a folder when the folder parts start the path parts (S5). A write outside the chat folder is `desktop`. A read outside `allowed_roots` is `desktop`.
- **`deny` paths:** the strip key, `timeways.key`, `config.toml`, and everything else in the config folder of the bridge (12). An approved access would let the agent sign fake strips or raise its own ceiling. The bridge writes `config.toml` itself after a desktop raise (9.3), never through the classifier.
- **`deny` paths in the game folder** (fixed on 2026-09-29; the tests came first):
  - The key addons `GnomishRelay_Key` and `Timeways_Key` (7.3.2), which hold a plain copy of each strip key, and an old `Key.lua` in the `GnomishRelay` and `Timeways` folders.
  - `WTF/Account`, which holds the saved variables of every account, with the chats.
  - `Screenshots`, which holds the prompt in the pixels of each strip.

  The sandbox (6.6.4) hides them too, as it hides every `deny` path. The bridge writes each `Key.lua` with mode 0600, and writes an older key file again when others can read it.
  - The whole key addon folder is `deny`, but for an app addon only an old `Key.lua`. A developer checkout links `AddOns/GnomishRelay` into the repository (16), and an agent edits the other files of that folder.
  - Not the slot folders: they hold the agent replies, which any addon can read anyway (6.5). Also, 1000 slots per app would need 2000 mounts, about 0.5 s per run with `bwrap`.
- **`deny` paths in the data folder** (12, and 9.7 decision 12): `state.json`, `approvals/`, `timeways/`, `rules.json` (6.6.5), `bridge.lock`, `bridge.pid`, `bridge.log`, `logs/`, `reports/`, and everything else there. An approved access would let the agent clear the replay store, answer its own desktop request, or change the story state. The bridge writes these files itself, never through the classifier.
- **`desktop` patterns** are whole parts that match anywhere in a path, for example `.git/hooks`. A last `*` in a part matches the rest of the part, so `.env.*` matches `.env.local`.
- **`desktop` paths, for reads and writes:** `.ssh`, `.aws`, `.gnupg`, `.env` files, other credential files (`.netrc`, `.git-credentials`, `.config/gh`, `.docker/config.json`, `.kube`), package tool tokens (`.cargo/credentials.toml`, `.npmrc`, `.yarnrc.yml`, `.pypirc`, `.config/pip`, `.gem/credentials`), keychains, and browser profiles. `action_input.rs` has the full list. The sandbox proxy (6.6.4) reaches the hosts of the package tools, so a command must not read their tokens. A hidden `.npmrc` or `.config/pip` also hides its settings, for example a project registry.
- **`desktop` paths, for writes:** files that host code runs later, outside the sandbox: `.claude/`, `.git/hooks/`, `.git/config`, `.envrc`, `.vscode/`, `.github/workflows/`, `.codex/`, `.mcp.json`, and the hook tool files `.husky/`, `.githooks/`, `.pre-commit-config.yaml`, `lefthook.yml`, and `.lefthook.yml`. The sandbox hides them from commands (6.6.4).
- **Every `.git` entry, for classifier writes:** a write to a path with a `.git` part, in any ASCII case, is `desktop`. Git on the host trusts every file there, not only `hooks` and `config`. A `commondir` that points at a folder whose `config` sets `core.fsmonitor` runs code at the next `git status`. So do `config.worktree` and the config of a submodule under `.git/modules/`. A nested `.git` file names another git folder. A file tool never needs to write there. The sandbox does not hide all of `.git`, because `git commit` writes it. It guards `.git` in its own way (6.6.4, "The `.git` entries").

**Rules for commands:**

- **Grammar:** `crates/protocol/src/shell.rs` splits a command into simple commands, with a strict part of POSIX `sh`:
  - words with single quotes, double quotes, and `\`;
  - the operators `;`, `&&`, `||`, `|`, `|&`, `&`, and a newline;
  - subshells in `(` `)`;
  - the redirects `>`, `>>`, `>|`, `<`, `<>`, `&>`, `&>>`, and a descriptor before them, such as `2>`. `2>&1` copies a descriptor and names no file.
- **Does not parse:** an open quote, a trailing `\`, an open `(`, a redirect with no file, `$` outside single quotes (every expansion), a backtick outside single quotes, a heredoc or here-string (`<<`), process substitution (`<(`, `>(`), a brace other than `{}`, a comment, a reserved word such as `if` or `then` as the command name, and a glob in the command name. A command that does not parse is `desktop`. The popup shows its raw text (6.4).
- **Command substitution:** a command with `$(` or a backtick outside single quotes is `desktop`, also inside double quotes. The check follows the quote state of the splitter. Inside single quotes both are plain text, so `git commit -m 'fix `x`'` is not a substitution, but `git commit -m "fix `x`"` is. An escaped one, such as `\$(`, still counts, which is stricter than the shell. An open single quote does not parse.
- **Names:** a name matches after its folder, its ASCII case, and a last `.exe` come off, so `/usr/bin/SUDO.exe` is `sudo`. Most lists match any word of a simple command, so a wrapper such as `timeout 5 sudo x` cannot hide a name.
- **`desktop` commands:** `eval`, `sudo` and the other commands that change the user (`sudoedit`, `doas`, `su`, `pkexec`, `run0`, `gsudo`, `runas`), a shell after a `|` (every simple command after the first `|` counts), `cmd.exe`, and PowerShell. PowerShell stays `desktop` until the classifier has a PowerShell parser.
- **Commands that run other commands** always ask: `xargs`, `env`, `sh`, `bash`, `python`, `node`, `perl`, and the other interpreters; wrappers such as `timeout`, `nohup`, and `strace`; schedulers such as `crontab`; `find` with `-exec`, `-ok`, or `-delete`; `git` with `-c`, `--upload-pack`, `--exec`, or `config`; a command name such as `.`, `source`, `command`, `export`, or `trap`; and a first word with `=`, such as `LD_PRELOAD=x cmd`. `command_rules.rs` has the full lists.
- **Network tools** (`curl`, `wget`, `nc`, `ssh`, `scp`, `rsync`, and more) always ask.
- **Redirects:** a redirect target resolves from the working folder with `resolve_folder`, then follows the rules for paths: `>` is a write, `<` is a read. `/dev/null` is always allowed. A target that starts with `~` or holds a glob is `desktop`: the shell expands it, and the classifier cannot. A command with a file redirect and a `cd`, `pushd`, or `popd` is `desktop`, because the target then resolves from another folder.
- **Never "always":** `rm -r`, `chmod`, `chown`, `chgrp`, a forced `git push` (`-f`, `--force`, `+main`), `git reset --hard`, the commands that run other commands, network tools, and every `desktop` answer. They get "Allow once" at most. The allow table of the config cannot allow them either.
- **Everything else** asks, unless the allow table of the config or a game rule covers each of its simple commands. Then it is `allow`.
- A prompt keyword (for example `.ssh` or `token`) is only a signal. It moves the whole run to `ask`. It is never the wall.

**The answer** of a tool call is the strictest answer of its parts: each path, each redirect target, and each simple command. The ceiling of a tool call is its answer when a game rule covers every command. No rule list gets more (S17).

**Limits.** The proofs cover the paths of file tools. Paths inside command arguments are out of scope: a command asks by default, the popup shows the raw command (S15), and the sandbox is the wall for commands (6.6.4). The proofs cover paths that the bridge already resolved. A symbolic link made after the check is a race that the proofs do not cover.

The classifier core is pure and lives in `protocol`. Theorems S16, S17, S27, and S28 cover it.

#### 6.6.4 Sandbox

The bridge runs every command of a game run inside a sandbox. The user does nothing.

| Rule | Value |
|---|---|
| Write | The chat folder, and a private temp folder of the run |
| Read | The system, except the `deny` paths and both lists of `desktop` paths of 6.6.3, which are hidden |
| Network | Only through the bridge proxy, to the allowed hosts (see "Network: the proxy"). The agent process is outside this sandbox, behind its own wall (see "Where the wall is" and "The agent process behind the proxy"). |
| Children | Every child process, for example `cargo test`, is in the same sandbox |

The sandbox closes the hole that a classifier cannot close: an allowed command such as `cargo test` runs code that the agent can edit first.
It covers shell commands. The file tools of Claude run outside it, so the classifier (6.6.3) guards them.

**The sandbox answers at `auto-edit`** (asked for by the user after a fresh-install test on 2026-09-30). At `auto-edit` with an empty allow table, the player got 3 game popups in 30 seconds for `ls`, `git status`, and `cargo test`. A popup that comes that often teaches the player to click Allow without reading, and then it protects nothing. So at `auto-edit` the sandbox, not the popup, is the wall for a plain command:

- **A command runs with no question** when all of these hold. `gate::sandbox_holds` checks them.
  1. The run level is `auto-edit`.
  2. The bridge command sandbox holds every command of the backend: Claude with `bwrap` or `sandbox-exec` (`gate::SandboxWall::Holds`).
  3. The classifier gives `ask`, and the ceiling (6.6.3) of the call is `allow`. So the call has no `deny` or `desktop` part, no "never always" command (a command that runs other commands, a network tool, `rm -r`, `chmod`, a forced push, `git reset --hard`), and no redirect outside the chat folder.
  4. `propose` (6.6.5) gives a rule for each simple command. So the call has no tool that runs any program or downloads code (`npx`, `docker`, `uv run`), no publish command (`git push`, `cargo publish`, `npm publish`, `twine`, `gh`), no name with a `/` (`./x.sh`), and no flag after a tool with subcommands (`git -C x status`).
- In short: a command runs with no question when one "Always allow" click could cover each of its simple commands. Both checks are proved functions of `protocol` (S17, S36, S37). The gate only joins them, so the proved verdicts do not change.
- **Still asks in the game:** each command that fails 3 or 4. **Still asks on the desktop:** each `desktop` answer. **Still refused:** each `deny` answer.
- **Every command still asks** with no command sandbox: Windows, a Linux with no working `bwrap` (the fallback below), Codex, and the other ACP agents. Codex retries an allowed command outside its sandbox with no request, and its sandbox reads `~/.ssh` and the bridge keys (6.6.5, rule 4), so for Codex the sandbox is not the wall. An ACP agent runs its commands itself (6.6.3).
- `ask` and `full-auto` do not change: at `ask` every command asks, and at `full-auto` every command that is not `deny` runs with no question (9.3, "Full-auto for one chat").
- **Against the threat model.** A hostile addon gains nothing: it could already click Allow on each popup (6.6.1), and one "Always allow" click already allowed these commands for good (6.6.5). A prompt-injected agent gains one skipped click. The walls stay: writes only in the chat folder and the temp folder, secrets hidden, and network only through the proxy to the allowed hosts. The list in 4 stops a direct `git push`, not a push from a script that the agent writes. A `Makefile` or a `build.rs` in the chat folder can run anything inside the sandbox, as the "Always allow" rules already accept (6.6.5, "Why not narrower"). The known leak stays: a command with its own token, for example one from the prompt, can send data to an allowed host (6.6.4, "What the list does not stop").
- **A cost to "Always allow".** At `auto-edit` in the sandbox, a command that could get a rule now runs. So the popup rarely offers Always: only when the allow table of the config covers a part that `propose` refuses, such as `./gradlew build && make`. Existing rules still count.

**The policy (S31).** `sandbox_policy` in `crates/protocol/src/sandbox.rs` builds it from the chat folder, the temp folder, the `deny` folders (the config folder and the data folder, 12), and both lists of `desktop` patterns. A path is hidden by the classifier predicate: it is inside a `deny` folder, or a run of its parts matches a pattern, without ASCII case. The writable paths are the chat folder and the temp folder, each clean in the form of S5 and not hidden. If the chat folder lies inside a hidden path, the policy leaves it out, and the bridge refuses the run: "The chat folder is inside a folder that the sandbox hides (the config folder, the data folder, or a credential folder), so the agent cannot work there." S31 proves the policy (14.1).

**Where the wall is** (decided with an advisor on 2026-09-26). The sandbox holds the commands, not the agent process:

- The agent writes its own state all the time: sessions, logins, and settings in `~/.claude`, `~/.claude.json`, and `~/.codex`. With these folders read-only, the agents stop working. With them writable, a command can plant a hook, an MCP server, or an allow rule that later runs unsandboxed in a terminal session of the user. So the agent process stays outside, and only its commands go in.
- The bridge also resumes Claude sessions from `~/.claude/projects` (9.6), which needs the real folder.
- The exception is a `command` agent (9.2). The bridge sees none of its tool calls, so the whole harness goes into the sandbox. Its writes into the home folder go to a copy-on-write view that goes away with the run (see "A harness with only a command line").
- Nested sandboxes fail on macOS inside a real wall. Checked on the macOS CI runner on 2026-09-27: inside a profile that allows everything, `sandbox-exec` starts again. Inside a profile that denies the network, it fails with "sandbox_apply: Operation not permitted" (the test `seatbelt_cannot_start_inside_a_seatbelt_wall`). Claude and Codex use Seatbelt for their own commands there.

**Claude (`kind = "claude"`).** Claude Code runs each command of its Bash tool through `CLAUDE_CODE_SHELL_PREFIX`. The bridge sets it to `<gnomish-relay> --sandbox-run`: this program, at its absolute path. Claude Code then runs `bash -c -l "'<gnomish-relay>' --sandbox-run '<command>'"`, and the bridge program starts the command inside the run sandbox. Checked on Claude Code 2.1.283 in its code: it quotes the part before the last " -" as the program and adds the command as one quoted word. The same prefix wraps command hooks and MCP servers.

- At the start of each run, the bridge makes the private temp folder (`gnomish-relay-run-<random>`, mode 0700, under the OS temp folder). It writes the run walls to `<data>/sandbox/<name>.json`: the tool, the writable paths, and the hidden paths that exist. `GNOMISH_RELAY_SANDBOX` names the file, and `TMPDIR` is the temp folder. Both go away at the end of the run.
- The wrapper never runs a command outside the sandbox. With no walls, or with a tool that does not start, the command fails with exit status 126.
- The command gets only the allowlist variables of 6.2 rule 12, `TMPDIR`, and the proxy and cache variables ("Network: the proxy"). The `env` list of the entry, for example `ANTHROPIC_API_KEY`, stays with the agent.
- The wrapper leaves a mark in the temp folder. If a Bash call ran and the mark is missing, Claude Code ignored the prefix. The run then stops at once with "Stopped: a command ran outside the sandbox.".
- The bridge refuses a run when the bridge program is inside the chat folder, because a command could change it: "The bridge program <path> is inside the chat folder, so a command could change it. Install it somewhere else, for example ~/.local/bin."
- The flags of a game run:
  - `--setting-sources ""`, so user and project settings do not apply;
  - `--strict-mcp-config`, so no MCP server starts;
  - `--settings` with `sandbox.enabled: false` and `env.CLAUDE_CODE_SHELL_PREFIX`.

  A project from the web can hold a `.claude/settings.json` with hooks, or with an `env` that clears the prefix, so no project setting applies. The model and other user settings do not apply either: the `command` of the entry can add `--model`.
- The own sandbox of Claude Code stays off. Checked on 2.1.283: the keys are `sandbox.enabled`, `sandbox.failIfUnavailable`, and `sandbox.allowUnsandboxedCommands`. With `failIfUnavailable`, Claude Code refuses to start on a Linux with no `socat` ("sandbox required but unavailable: ... socat not installed"). On macOS its Seatbelt and the bridge Seatbelt would nest, which reportedly fails. The bridge sandbox covers the same commands.

**Linux: `bwrap`.** The walls, in order:

- `--ro-bind / /`, `--dev /dev`, `--proc /proc`;
- an empty `--tmpfs` on `/tmp`, `/var/tmp`, and `/run` (they hold the sockets of the ssh agent and the desktop);
- a writable `--bind` of the chat folder and the temp folder;
- the copy-on-write views of `~/.cargo` and `~/.rustup` (see "The downloads of cargo and rustup");
- a `--tmpfs` over each hidden folder and a read-only empty file over each hidden file, then `--remount-ro` of each;
- `--unshare-all` (no network but its own loopback, own process ids), `--die-with-parent`, and `--new-session`.

These walls belong to the run, not to one command: see "One sandbox for each run". A folder that holds a writable path keeps its place: a chat folder under `/tmp` still works. A socket at a path works across network namespaces, for example `~/.docker/desktop/docker.sock`, and Docker runs any program outside the sandbox. So the hidden paths of each run also hold each socket file in the top 3 levels of the home folder, as in the agent wall (see "The wall on Linux").

**One sandbox for each run** (asked for by the user on 2026-09-27, worked out with an advisor, built on Linux). A dev server must work across Bash calls: `npm run dev &` in one call, and `curl localhost:3000` in a later call. So on Linux the commands of a run share one sandbox:

- At the start of the run, before the agent starts, the bridge starts a holder in the walls above: `bwrap <walls> --unshare-all --die-with-parent --new-session -- <gnomish-relay> --sandbox-hold <launch socket> <proxy socket> <local ports>`. The holder relays the proxy port and the local ports (see "Network: the proxy"). It listens on the launch socket `.gnomish-relay-launch` in the run temp folder. It writes `ready` when it takes commands.
- The wrapper (`--sandbox-run`) sends one request to the launch socket: the shell, the working folder, the command, and its variables. The holder starts `bash -c <command>` inside the sandbox, in its own process group, with no input. It sends back the output and the error output in frames, and last the exit status. The wrapper prints them as its own and exits with that status.
- With no holder, a command does not start: the wrapper exits with status 126, "The sandbox of the run is not running." There is no fallback.
- A server that a command starts in the background runs until the run ends or Stop. The bridge then stops the holder, and each process in the sandbox ends with it. When the wrapper goes away first, for example at the Claude Code timeout, the holder stops the process group of that command.
- The holder lives outside the agent wall, as a child of the bridge. The launch socket lies in the temp folder, which the wall binds back, so the wrapper in the wall reaches it. A command also reaches the socket, but it can only start another command in the same sandbox.
- Why not `nsenter`: a first design joined the holder namespaces with `nsenter`. It worked on Arch Linux. On the Ubuntu CI runner, `nsenter` could not join the network namespace ("Operation not permitted"). Also, `nsenter --wd=<folder>` opened the folder before the join, so `cd ..` left the walls. The holder needs no join.
- Commands of one run can signal each other. That is one sandbox.
- A limit: a background command that keeps standard output open keeps the Claude Bash call open until the command ends or times out, as with no sandbox. Use `> log 2>&1 &`, or the background option of the Bash tool.
- macOS: Seatbelt starts a sandbox for each command, with no network but the proxy port and `local_ports`. So a server of one command does not answer a later command there. Opening the whole loopback of this computer is the wrong trade, so this is a named gap.

**macOS: `sandbox-exec`** with a generated profile:

- `(allow default)`, `(deny network*)`, `(deny file-write*)`;
- an allow of writes to a few devices (`/dev/null`, `/dev/tty`, `/dev/fd`), and to the chat folder and the temp folder;
- last, a deny of reads and writes under each hidden path, `/private/tmp`, and `/private/var/tmp`.

With the proxy, one rule after `(deny network*)` allows the loopback port of the run proxy: `(allow network-outbound (remote ip "localhost:<port>"))`. A later rule wins in Seatbelt. Each path is a string literal in the profile, with the escape of S32.

Mach services stay reachable, so the keychain answers by its own access lists. `git credential-osxkeychain` gives the GitHub token of the user with no prompt, and the proxy reaches `github.com`, so a command could push to the user's repositories. So with the proxy, the profile denies the two keychain services: `(deny mach-lookup (global-name "com.apple.SecurityServer") (global-name "com.apple.secd"))`. A deny of `com.apple.secd` alone does not stop a keychain read. The deny has a cost, checked on the macOS 26 CI runner:

- A tool that checks TLS with the macOS Security framework fails, for example `curl` with its SecureTransport backend ("Couldn't understand the server certificate format"). Go tools and Rust programs with `native-tls` check TLS the same way.
- The system `curl` and `git` (LibreSSL), cargo (the system libcurl), npm, and pip do not, and they work.
- A build that signs code with a keychain key fails too.

**Hidden paths that exist.** A `bwrap` mount and a Seatbelt `subpath` name a real path. So at the start of each run the bridge looks for the hidden paths: the `deny` folders, each pattern in the home folder (for example `~/.ssh` and `~/.config/gh`), and a walk of the chat folder.

- The walk does not follow links, but a link with a hidden name hides its real target.
- The walk has no size limit. It follows no links, so it ends, and it reads about 1000000 entries in less than a second. A walk over 5 seconds gets a log line with its time and count.
- The walk skips no folder, not even `target` or `node_modules`. A command can mark any folder as a cache, for example with a `CACHEDIR.TAG` file. A skip would then show a real `.env` in that folder to the next run.
- For the same reason, a folder that the walk cannot read, or a git folder whose `HEAD` it cannot check, stops the run with an error that names the folder. A command can `chmod 000` a folder in one run, and a skip would show its `.env` to the next run. The chat folder belongs to the user, so such a folder is not normal, and the user restores its permissions. An error is simpler than an empty cover over the folder, and it shows nothing.

Limits:

- A path that matches a pattern in another folder, for example `.env` in another project under `allowed_roots`, stays readable for commands. The classifier still asks before a command that names it.
- A matching file that a command makes during the run is not hidden. It holds only what the agent wrote.
- The agent logins are `desktop` paths too (`.claude.json`, `.claude/.credentials.json`, `.codex/auth.json`), so no command reads them.
- In the sandbox `.git/config` reads as empty, so `git` works with no remote and no repository settings, and `git config` fails. The git hooks are gone.
- macOS: Seatbelt cannot show an empty file in place of a file, and git stops at a config that it cannot read ("fatal: unable to access '.git/config': Operation not permitted", found on the macOS CI runner on 2026-09-30). So there the `config` and `config.worktree` of each git folder stay readable, and a `literal` rule denies writes to each. A command on macOS reads the remotes and repository settings, and `git config` fails. The hooks stay hidden: git takes an unreadable hook as no hook.

**The `.git` entries** (fixed on 2026-09-27; the tests came first). Git outside the sandbox trusts what a `.git` names: its hooks, its config (for example `core.fsmonitor` and `core.hooksPath`), and the folder that a `.git` file points to. A command must not change any of it:

- **Pinned.** The walk of the chat folder notes each `.git`, folder or file, at any depth and in any ASCII case. `bwrap` binds each one onto itself after the writable binds, a folder writable and a file read-only. A mount point cannot move or go away, so `mv .git x`, `rm -rf .git`, and a new `gitdir:` line all fail. Seatbelt denies writes to the path of each one with a `literal` rule, so the entry cannot be moved, removed, or written, but the files inside stay writable. So a commit still works: objects, refs, the index, and `HEAD` are writable.
- **Every git folder, not only the top one.** A folder inside a `.git` with a `HEAD` file is a git folder: the top `.git`, a submodule under `.git/modules/`, or a worktree under `.git/worktrees/`. A folder under `refs/` or `logs/` of a git folder is never one (fixed on 2026-09-30, after a live run; the tests came first). `logs/HEAD` is the reflog, and `refs/remotes/origin/HEAD` names the default branch of a remote. Git reads each file under `refs/` as a ref, so a stand-in there gave "fatal: bad object refs/remotes/origin/config" at `git fetch`.
  - At the start of each run, the bridge removes what an older build made in such a folder with a `HEAD` file: an empty `config` or `config.worktree`, a `commondir` with the text `.`, and an empty `hooks` folder. It removes only regular files and folders, never a link, and logs one line for each. A real ref is never empty.
  - Each git folder guards `config`, `hooks`, `commondir`, and `config.worktree`. The sandbox hides `config`, `hooks`, and `config.worktree`. It binds an existing `commondir` read-only, because git in the sandbox reads it.
- **A missing guarded name gets a stand-in, except `commondir`** (fixed on 2026-09-29; the tests came first). The `.git` folder is writable, so a command could make a `commondir` that points at a folder whose `config` sets `core.fsmonitor`. The next `git status` on the host then runs that program. A `bwrap` mount needs a path that exists, and a mount point that `bwrap` makes is an empty file, which git cannot read. So at the start of a run that writes the chat folder, the bridge makes each missing `hooks` as an empty folder, and each missing `config` and `config.worktree` as an empty file, with `create_new`. Git reads an empty `config.worktree` only with `extensions.worktreeConfig`, and then as no settings. The walls cover each stand-in as they cover a real one. The stand-ins stay after the run: a second run in the same folder at the same time covers them too, and a removal would take away its mounts.
- **A missing `commondir` is watched, not made** (fixed on 2026-09-29, after a live run; the tests came first). The first fix made a `commondir` stand-in with the text `.`. Git reads it as no `commondir`. But Claude Code, and likely libgit2, JGit, and IDEs, read a git folder with any `commondir` as a linked worktree. Claude Code then failed to make a worktree: "Could not read the repository git config to neutralize filter drivers". A `commondir` with the absolute path of the git folder breaks it too. So the walls note each missing `commondir` of a git folder (`watched`), and:
  - Seatbelt denies a write to each one with a `literal` rule, which also covers a path that does not exist.
  - With `bwrap`, the wrapper removes each one that exists when its command ends. The permission hook of a Claude run does the same before each tool call, and the end-of-run check does it again. A regular file or an empty folder goes. A link or a full folder stays. Each one goes into a file next to the walls file, where no command reaches. The reply ends with "Removed <path>, which a command made." or "A command made <path>, which is a link. Remove it before you run git there.". The wrapper also prints the line to the command.
  - A real `commondir`, for example of a linked worktree under `.git/worktrees/`, exists at the start, so it is pinned read-only and never removed.
  - **The repair.** At the start of each run, the bridge removes a `commondir` stand-in of the first fix: a regular file with the text `.` in a git folder not under `worktrees/`. It logs one line for each. A real `commondir` never holds `.`.
  - **Limit: a race with a background process.** With `bwrap`, a command can start a background process that writes `.git/commondir` after the command ends. The file then exists until the next check (the next tool call, the next command, or the end of the run). Claude Code runs git on the host during the run, so a `git status` in that window runs the `core.fsmonitor` of the config that the file names. The window is short, and the process needs the right moment. The end-of-run check removes the file before the user runs git. A background process of the holder ends with the run, so it cannot write after that check.
- **Why not a read-only `.git`.** Git makes `index.lock` in the `.git` folder and renames it over `index`, and a rename over a mount point fails. So a read-only `.git` with writable `objects` and `refs` breaks `git add` and `git commit`.
- **A link at a guarded name stops the run.** A mount covers the target of a link, and a command can replace the link itself. The error names the link.
- **The check at the end of a run.** A new git folder, for example a submodule from `git submodule add`, has no walls in that run. So the bridge notes each `.git` entry and each guarded name at the start. It walks the chat folder again after the run, when no command runs. The reply of a Claude run or a `command` run then ends with "The run made <paths> in the chat folder. Git on this computer runs what they name. Check them before you run git there.".
- **A `.git` link stops the run.** A command could replace the link, and a mount cannot pin a link. The error names the link.
- **macOS: the folders above stay in place.** A Seatbelt rule names a path. So a move of a folder above a hidden or pinned path takes the path out of the rule, for example `mv packages p2 && cat p2/app/.env`. Seatbelt denies writes to the path of each folder between a writable folder and a hidden or pinned path inside it. Such a folder cannot be moved or removed in the sandbox. On Linux a mount follows the move of a folder above it, and a hard link across a mount fails.
- **Limit.** A new `.git` that a command makes in a folder with none is not pinned. Git outside the sandbox finds it only when it runs in that folder, and the end-of-run check names it. **Commit** of a change summary refuses a new repository: a commit records it as a gitlink, and plain git in the parent then runs in it (9.11). A chat folder with no `.git` at the start has the same limit.

**Network: the proxy** (asked for by the user and decided with an advisor on 2026-09-26). A game run needs `cargo fetch`, `cargo build` with new dependencies, `npm install`, `pip install`, and `git fetch` over https. So a command reaches a short list of package hosts through a bridge proxy, and nothing else:

- **The proxy** (`proxy.rs`) runs in the bridge, outside the sandbox, one for each run. It takes only HTTP `CONNECT` to a host of the allow list on port 443 or 80. It never ends the TLS, so it sees only the host name. It takes no plain `GET http://...`: the tools use https, and a plain HTTP forward needs a second parser.
- **The host check.** The name must be on the list, compared exactly and without ASCII case. An IP address in any form (`[::1]`, `127.1`, `0x7f000001`) is refused before any lookup, and `localhost` is only for `local_ports`. The first line must be exactly `CONNECT <host>:<port> HTTP/1.<digit>`. `hosts.rs` and `connect.rs` in `protocol` hold these rules.
  - The proxy resolves the name once. It refuses the name when any address is not on the public internet: loopback, private, link-local, shared (100.64/10), multicast, reserved, the IPv6 forms that hold such an IPv4 address, and the local NAT64 range `64:ff9b:1::/48` with the rest of `64:ff9b::/32` outside `64:ff9b::/96` (`ip.rs` in `protocol`, fuzzed against a table of ranges).
  - It then connects to a checked address. No second lookup happens, so a name cannot resolve to a public address for the check and to an inside address for the connection.
- **Limits.** At most 64 connections at once per run. 10 seconds for the request head (at most 8 KiB) and for the connection. 5 minutes with no byte in either direction. Each refusal logs the chat, the host, and the reason, and the command gets `403` with the reason.
- **Linux.** `bwrap --unshare-all` leaves the command only its own loopback. The proxy listens on a Unix socket in the run temp folder, which the sandbox already binds at the same path. The run holder (see "One sandbox for each run") listens on `127.0.0.1:3128` of that network and on each port of `local_ports`, and relays each connection to the socket. No `socat` and no `unsafe`.
- **macOS.** The proxy listens on a free loopback port. The profile allows only that port, after `(deny network*)`, and denies the keychain services (see "macOS: `sandbox-exec`"). Other user programs can also reach the port, but they already have the full network, and the proxy gives them nothing more.
- **The variables.** The command gets `HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy`, `ALL_PROXY`, `CARGO_HTTP_PROXY`, `npm_config_https_proxy`, and `npm_config_proxy` with `http://127.0.0.1:<port>`. `NO_PROXY` and `no_proxy` are `localhost,127.0.0.1,::1`: the sandbox loopback on Linux, and the `local_ports` that Seatbelt allows on macOS. A command that clears them has no way out: the OS network stays off. The npm and pip caches in the home folder are read-only, so `npm_config_cache` and `PIP_CACHE_DIR` point into the run temp folder.
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
- **S31 stays as it is.** S31 is about hidden and writable paths, and the proxy adds neither: the socket lies in the temp folder. `Network::Off` of the policy stays true at the OS level: `bwrap` gives no network but a private loopback, and Seatbelt denies all network but one loopback port. The proxy is a bridge channel beside the policy, not part of it.
- **What the list does not stop.** A list limits where a command connects, not data that leaves to an allowed host. A command with its own token can push to `github.com` or publish to npm. Only a TLS end in the bridge closes this. So the tokens of these tools are hidden (6.6.3), and the list stays short.
- **Build scripts.** A command such as `cargo build` runs a `build.rs` or an npm install script that the agent can edit. It reaches the allowed hosts too. The short list keeps this small.
- The agent process has its own proxy, with other rules: see "The agent process behind the proxy" below.
- With no host in the list (12), the proxy does not start, and commands have no network at all.

**The agent process behind the proxy** (asked for by the user on 2026-09-27, decided by the user with an advisor on 2026-09-27; built on Linux). S33 to S35 are approved and proved (14.1). An unbuilt part says so.

*What it gives.* Without a wall, a hostile prompt can make the agent process send data anywhere: through WebFetch that the user approves by mistake, an MCP server, or any unsandboxed command in the agent tree. For Claude the gain is small: WebFetch and MCP already ask or are off, and its commands already go through the command proxy. For ACP agents the gain is large. Their commands run unsandboxed in the agent tree (6.6.4, "Other ACP agents"), so the agent wall is the only network wall of those commands. The same holds for Codex commands that a Codex `allow` rule runs outside its sandbox. The wall also keeps the agent away from this computer: its loopback, its network, the desktop sockets, and the other user processes.

*Measured on 2026-09-27* (Arch Linux, `bwrap` 0.13.0). Each agent ran in `bwrap --dev-bind / / --unshare-net`, with `HTTPS_PROXY` set and `gnomish-relay --sandbox-forward` as the only way out, to a logging proxy:

| Agent | Hosts it connected to |
|---|---|
| Claude Code 2.1.283, `claude -p` with the flags of a game run, a prompt "Reply with only the word hi." | `api.anthropic.com` (11 connections) and `http-intake.logs.us5.datadoghq.com` (telemetry) |
| The same with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | `api.anthropic.com` only |
| codex-cli 0.157.0, `codex exec`, not logged in on this computer, so every call got `401` | `chatgpt.com`, `github.com`, and `api.openai.com` (a WebSocket first, then HTTPS) |

- Not measured, but named in the programs: `platform.claude.com/v1/oauth/token` (the refresh of a Claude login), and `auth.openai.com` (the refresh of a Codex login). A live run with a Codex login is still to do.
- Both agents took the proxy from `HTTPS_PROXY`, and nothing tried to connect around it: a connect with no proxy has no route in the namespace.
- Nesting works on Linux. Inside the agent wall (`--unshare-net --unshare-pid --proc /proc`, a tmpfs on `/run` and `/tmp`), a Bash call of the real `claude` went through the real `--sandbox-run`: `bwrap --unshare-all` in the wall, with its own forwarder. The command reached `index.crates.io` through the command proxy (200), a direct connect failed, and the command did not see the agent proxy socket. `codex sandbox` (the Linux sandbox of Codex) also works in the wall, and so does a nested `--overlay`.

*Two kinds of scrutiny* (decided by the user): "things run by the agent and outside the agent deserve different scrutiny".

| Who | Hosts through the proxy | This computer |
|---|---|---|
| The agent process (`claude`, `codex`, ACP agents, and the `claude` model calls of Timeways) | Any public host, with `agent_network = "open"` (the default). With `agent_network = "strict"`, only the model hosts of the backend and the `agent_hosts` of the entry. | Only the ports of `local_ports` |
| A command of a game run (6.6.4) | The package hosts and `allow_hosts`, as before | Only the ports of `local_ports` |

- "Public" is the address rule of "The host check": one lookup, a refusal when any address is not public, and a connect to a checked address. Link-local holds the cloud metadata address `169.254.169.254`. The name rules stay: no IP address in any form, no `localhost`.
- Ports 443 and 80 only, in both modes. The agent reads `~/.ssh`, so with port 22 open it could push to any repository with the user's keys. Port 25 would send mail.
- The proxy logs each host that it opens, with the chat, and each refusal.
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` for Claude: no telemetry and no update check.
- The command proxy lies in the run temp folder, which the agent sees, and the agent can run `--sandbox-run` itself. So in `strict` mode the command hosts are agent hosts too.

*The model hosts of each backend* (for `strict`):

| Backend | Model hosts |
|---|---|
| `claude`, and the model calls of Timeways | `api.anthropic.com`, `platform.claude.com` |
| `codex` | `api.openai.com`, `chatgpt.com`, `auth.openai.com` |
| ACP and `command` | None. The entry names its hosts in `agent_hosts`. A `command` preset adds the model hosts of its tool (`harness_presets.rs`). A `command` agent also reaches the sandbox hosts, because it runs its own commands. |

An entry with Bedrock, Vertex, or another `ANTHROPIC_BASE_URL` names its hosts in `agent_hosts`.

*The wall on Linux.* The agent process keeps most of its file access: it writes `~/.claude`, `~/.claude.json`, and `~/.codex`, and it resumes sessions ("Where the wall is"). Its network, processes, sockets, and the user's startup files change. `process.rs` starts the agent as:

`bwrap --dev-bind / / --tmpfs /run --tmpfs /tmp --tmpfs /var/tmp --tmpfs /dev/shm <binds back> <read-only startup files> <sockets covered> --unshare-net --unshare-pid --proc /proc --die-with-parent --new-session -- <gnomish-relay> --sandbox-forward <socket> <local ports> --exec <agent> <args>`

- `--unshare-net` gives the tree only its own loopback. The forwarder listens on `127.0.0.1:3128` there and relays each connection to the agent proxy. The `--exec` form starts the agent with no shell, with its arguments unchanged (6.2 rule 11).
- `--unshare-pid` with a new `/proc`. With the host `/proc`, the agent reads `/proc/<pid>/root` of any user process, and through it the session bus under `/run`. Then `systemd-run --user` starts code with the full network. A new `/proc` also stops `ptrace` and `pidfd_getfd` of host processes, which `ptrace_scope = 0` allows. Measured: `claude -p` and a nested `bwrap` both work with it.
- The tmpfs on `/run`, `/tmp`, `/var/tmp`, and `/dev/shm` hides the sockets there: the session bus, the user systemd manager, the ssh agent, Docker, and X11. An abstract socket belongs to one network namespace, so the new namespace hides those. The binds back restore the chat folder and the run temp folders at their paths, when they lie under one of these folders.
- A socket at a path elsewhere still works across namespaces. Examples: `~/.docker/desktop/docker.sock` (`docker run --network host` is a full way out), the sockets of Lima, Colima, and Podman machines, of terminal programs, and of editors.
  - At the start of each run, the bridge looks for socket files in the top 3 levels of the home folder. It also looks in the top 4 levels of `~/.lima`, `~/.colima`, `~/.docker`, `~/.rd`, and `~/.local/share/containers` (for example `~/.lima/default/sock/docker.sock`). It binds `/dev/null` over each one.
  - The scan looks into the container tool folders first, and stops after 200 000 entries. At that limit it logs a line, because a socket after the limit stays reachable. A socket in another place stays reachable too. This is a named limit.
- Stop: with `--unshare-pid`, the whole tree ends when `bwrap` ends, grandchildren too. The 10-second grace of Stop (9.4) still comes first, so a session file is not cut short.
- The variables: the allowlist of 6.2 rule 12, the `env` list of the entry, the proxy variables of "The variables" with port 3128, `NO_PROXY` and `no_proxy` set to `localhost,127.0.0.1,::1` (see `local_ports`), and for Claude `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
- `gnomish-relay check-agent` starts a Codex or ACP agent inside the same wall. The Claude check makes no model call (`--version` and `auth status`), so it runs with no wall.
- Tests: `crates/bridge/tests/agent_wall.rs` runs `fake-claude`, `fake-codex`, and `fake-acp-agent` in the real wall, with the network probes of `src/bin/shared/net_probe.rs`. The fuzz target `agent_wall` reads the `bwrap` arguments as `bwrap` does.

*The socket of the agent proxy.* Each run has a second proxy for the agent, with the agent rules:

- Its socket lies in the data folder (`<data>/sandbox/<name>.sock`). S31 hides the data folder from every command, so no command reaches the agent proxy, for example to reach a public host that is not a package host. A test checks that the socket lies in a hidden path.
- A Unix socket path holds at most 108 bytes on Linux. The bridge binds through `/proc/self/fd/<folder>/<name>.sock`, so a long data folder still works, with no `unsafe` code and no change of the working folder. Inside the wall, a bind puts the socket at `/run/gnomish-relay/agent.sock`.

*`local_ports`* (`[sandbox] local_ports = [5432, 3000]`). The agent and its commands reach `127.0.0.1` and `::1` of this computer on exactly these ports. All else on the loopback of this computer stays closed.

- Measured: `psql`, `redis-cli`, and other database clients open a plain TCP connection and do not speak HTTP `CONNECT`. So the forwarder in each namespace (the agent wall and the command sandbox) listens on `127.0.0.1:<port>` and `[::1]:<port>` for each port. It relays each connection through the proxy socket with `CONNECT localhost:<port> HTTP/1.1`. A client inside connects to `localhost:<port>` as usual. A failed listen on `::1` (IPv6 off) logs a line and is not an error.
- The proxy takes `localhost:<port>` only for a listed port. It connects to `127.0.0.1:<port>`, and to `[::1]:<port>` only after "connection refused", with no lookup. Only the proxy checks the list, because the agent can change the walls file and the forwarder arguments.
- `NO_PROXY` holds `localhost,127.0.0.1,::1`, so `curl http://localhost:3000` goes straight to the namespace loopback: to the relay for a listed port, and to an agent server for any other port. A server that the agent starts in the wall works on the wall loopback. The relay inside the wall takes each listed port, so an agent server cannot listen on it; the forwarder error names the port.
- The proxy refuses 2375 and 2376 (Docker over TCP) and 9222 (the browser debug port), because each one runs any code with no question. Config load refuses them too.
- A warning: every other listed port is the user's choice. A dev server with an eval endpoint, or a database with a superuser that runs programs, gives the agent the same power.
- macOS: the Seatbelt profile of a command allows `(remote ip "localhost:<port>")` for each port. The agent on macOS has no wall yet.
- A local model entry (for example Ollama on port 11434) works when its port is in `local_ports`. Setup adds the port of a local model that it configures.

*The startup files are read-only for the agent.* The agent keeps its file writes, so it could write code that later runs outside the wall with the full network. The wall binds each of these read-only, when it exists:

- The shells: `~/.bashrc`, `~/.bash_profile`, `~/.bash_login`, `~/.bash_logout`, `~/.profile`, `~/.zshrc`, `~/.zprofile`, `~/.zshenv`, `~/.zlogin`, `~/.zlogout`, `~/.config/zsh`, `~/.oh-my-zsh/custom`, `~/.config/fish`.
- The desktop and the session: `~/.config/systemd/user`, `~/.local/share/systemd/user`, `~/.config/autostart`, `~/.config/environment.d`, `~/.pam_environment`, `~/.xprofile`, `~/.xinitrc`, `~/.config/plasma-workspace/env`, and the WSL2 start file, `~/.config/gnomish-relay/wsl-start.sh` (11.5).
- Programs early on `PATH`: `~/.local/bin` and `~/.cargo/bin`.
- Tools that run code from their config: `~/.ssh`, `~/.gitconfig`, `~/.config/git`, `~/.cargo/config.toml`, `~/.npmrc`, `~/.config/pip`, `~/.pip`, `~/.pypirc`, `~/.vimrc`, `~/.config/nvim`, `~/.config/direnv`, `~/.gnupg`, `~/.config/Code/User`.
- The agent config, which later starts programs in a terminal session: `~/.claude/settings.json`, `~/.claude/settings.local.json`, `~/.claude/CLAUDE.md`, the folders `hooks`, `commands`, `agents`, `skills`, and `plugins` in `~/.claude`, and `config.toml`, `hooks.json`, and the folder `rules` in `~/.codex`. A rule in `rules` runs a command outside the Codex sandbox.
  - When the bridge has `CLAUDE_CONFIG_DIR` or `CODEX_HOME` set to an absolute path, the same names in that folder are read-only too. So are the ones in the folders that the last `hooks` command saved (10.5). A bridge service lacks the variables of a shell rc file, so only that file names a moved folder there.
- These stay writable: `~/.claude/projects`, the other session files, `~/.claude.json`, `~/.claude/.credentials.json`, `~/.codex/auth.json`, `~/.codex/sessions`, and `~/.cache`.
- A limit: a file that is missing at the start has no mount, because the home folder is the real disk and a mount point would appear there. So the bridge notes the missing paths of the list at the start of a run and looks again at the end. For each new one, it logs a line, and the reply ends with "The agent made ~/.zshrc during the run. Check it before you open a new terminal."

*The agent tools that use the network:*

- WebFetch runs in the agent process. It asks on the desktop as before (6.6.3). After Approve it works in `open` mode for a public host, and fails in `strict` mode for an unlisted host.
- WebSearch runs on the Anthropic servers, through `api.anthropic.com`. The proxy cannot see it. The classifier still asks on the desktop.
- MCP servers: a Claude game run starts none (`--strict-mcp-config`). A Codex MCP server from the user config starts in the wall, with the agent rules.

*What the wall does not stop:*

- In `open` mode it does not stop data that leaves to a public host. It stops the way to this computer and its network, and it logs each host. `strict` mode limits the hosts, but still does not stop data that leaves to an allowed host: an attacker key on `api.anthropic.com`, WebSearch, and on the Codex side all of `chatgpt.com`.
- A socket file outside the levels that the scan reads, a socket after the scan limit, a listed local port, and an unlisted startup file.
- The agent can still write the walls file of its run in `<data>/sandbox/`. That is true of the command sandbox too.

*With no working `bwrap`,* the agent runs with no wall. The log line at start says so, and the first reply carries "(The agent has full network access: this computer can't limit it.)" (decided by the user).

*macOS* (decided by the user: a spike first). The spike of 2026-09-27 showed that `sandbox-exec` cannot start inside a Seatbelt wall that denies the network (see "Where the wall is"). So a Seatbelt wall on the agent would break the bridge command sandbox and the Codex sandbox. The agent has no wall on macOS, and the first reply carries the no-wall notice. If the test `seatbelt_cannot_start_inside_a_seatbelt_wall` ever fails, nesting works, and macOS can get a wall.

*Windows.* No wall, as for commands (6.6.4, "Windows"). Under WSL2, the Linux wall applies, and it keeps the Windows drives read-only (11.5).

*Timeways.* The story program already has no network. The model calls of `model_claude.rs` run `claude -p` with no tools; they go into the same wall with the `claude` hosts. The local route (`model_local.rs`) is a bridge `curl` to `local_url`, not agent code, and stays as it is.

*The keys* (12): `[sandbox] agent_network = "open" | "strict"` is one key for every agent and for Timeways. `[sandbox] local_ports` is one list for the agent and for commands. `agent_hosts` is a list in each `[agents.<name>]` entry, for `strict` mode.

*Verification.* The pure parts of the proxy decision live in `crates/protocol`, over bytes and integers: `hosts.rs` (the host name and the list), `ip.rs` (the public address), and `connect.rs` (the target of a request). The bridge keeps the sockets, the lookup, and the connection. The statements, approved by the user on 2026-09-27 and proved (14.1):

- **S33, host check.** For every allow list and host, `host_allowed(list, host)` is true exactly when the host is a good host name and equals a list name without ASCII case. A good host name (`good_host_name`) has 1 to 253 bytes and at least two labels. Each label has 1 to 63 bytes of `[A-Za-z0-9-]` and does not start or end with `-`. The last label starts with a letter and is not `localhost`. So no IP address in any form passes. Lean shape: `∀ list host, hosts.host_allowed list host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ∧ ∃ h ∈ strs list.val, lowerAscii h = lowerAscii (bytes host.val) ⦄`, and `∀ host, hosts.good_host_name host ⦃ ok => ok = true ↔ goodHostName (bytes host.val) ⦄`.
- **S34, public address.** For every IPv4 address, `is_public_v4` is true exactly when the address is in no range of the table `v4NotPublic` (0/8, 10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.0.0/24, 192.0.2/24, 192.88.99/24, 192.168/16, 198.18/15, 198.51.100/24, 203.0.113/24, and 224/3). For every IPv6 address, `is_public_v6` gives the answer of the embedded IPv4 address for `::ffff:0:0/96`, `64:ff9b::/96`, and `2002::/16`. Otherwise it is true exactly when the address is in no range of `v6NotPublic` (`::/16`, `100::/16`, `2001::/23`, `2001:db8::/32`, `64:ff9b::/32`, `fc00::/7`, `fe80::/10`, `fec0::/10`, and `ff00::/8`; `::/16` holds `::` and `::1`, and `64:ff9b::/32` holds the local range `64:ff9b:1::/48`). Lean shape: `∀ o, ip.is_public_v4 o ⦃ r => r = true ↔ ¬ inRanges v4NotPublic (v4Nat o) ⦄` and `∀ s, ip.is_public_v6 s ⦃ r => r = true ↔ match embeddedV4 (v6Nat s) with | some v4 => ¬ inRanges v4NotPublic v4 | none => ¬ inRanges v6NotPublic (v6Nat s) ⦄`.
- **S35, the target of a request.** For every request head of at most 8 KiB, every mode, host list, and port list, `check_target(mode, list, ports, head)` returns a target or a defined refusal, and never panics. It returns a target only when the first line, up to the first CR LF, is `CONNECT <host>:<port> HTTP/1.<d>`. One space divides the three parts, `<d>` is one digit, `<port>` is 1 to 5 digits whose value `p` is at most 65535, and `<host>` has no `:` or space. Then the target is one of three:
  - `Local(p)`: the host is `localhost` without ASCII case, `p` is on the port list, and `p` is not 2375, 2376, or 9222.
  - `Remote(h, p)` in the mode `Listed`: `p` is 443 or 80, `host_allowed(list, host)` is true (S33), and `h` is the host in lower case.
  - `Remote(h, p)` in the mode `Public`: `p` is 443 or 80, `good_host_name(host)` is true (S33), and `h` is the host in lower case. The bridge then connects only to an address for which `is_public` is true (S34). This last step is bridge code, and a named test covers it.
  Lean shape: `∀ mode list ports head, head.val.length ≤ 8192 → connect.check_target mode list ports head ⦃ r => ∀ t, r = .Ok t → ∃ h ds p d rest, bytes head.val = "CONNECT " ++ h ++ ":" ++ ds ++ " HTTP/1." ++ [d] ++ "\r\n" ++ rest ∧ isDigit d ∧ allDigits ds ∧ 1 ≤ ds.length ∧ ds.length ≤ 5 ∧ decimalValue ds = p ∧ ' ' ∉ h ∧ ':' ∉ h ∧ ((t = .Local p ∧ lowerAscii h = "localhost" ∧ p ∈ ports ∧ p ∉ [2375, 2376, 9222]) ∨ (∃ th, t = .Remote th p ∧ (p = 443 ∨ p = 80) ∧ bytes th.val = lowerAscii h ∧ (mode = .Listed → hostAllowed list h) ∧ (mode = .Public → goodHostName h))) ⦄`.

Tests and fuzzing, with no proof:

- Fuzz: `connect` checks `check_target` in each mode against a model in the fuzz target. A new target `public_ip` compares `is_public` with a table built from the `ipnet` crate. A new target `agent_wall` checks the agent `bwrap` arguments: `--unshare-net`, `--unshare-pid`, and `--proc` are there, each tmpfs comes before its binds back, each startup file is read-only, and the agent and its arguments come last, each unchanged.
- Unit tests: the model hosts of each backend, the keys, the agent socket path inside a hidden path, the scan for socket files, the check of new startup files, and the agent variables.
- E2E with the fake agents and the real `bwrap` (`crates/bridge/tests/agent_wall.rs`). `fake-claude`, `fake-codex`, and `fake-acp-agent` get script steps that connect with no proxy, send a `CONNECT` through `HTTPS_PROXY`, connect to a local port, and serve on the wall loopback. The test proxy resolves `allowed.test` to a public address, as in `command_sandbox.rs`. For each fake agent:
  - a direct connect fails;
  - a public host through the proxy works in `open` mode;
  - an unlisted host gets `403` in `strict` mode;
  - a name that resolves to `127.0.0.1` gets `403`;
  - a listed local port works, and another one does not;
  - an agent server on the wall loopback answers.

  Also: a command of `fake-claude` still runs in the command sandbox, reaches the command proxy, and does not reach the agent socket. A socket in the fake home and one under `/run` are not reachable. `/proc/<pid of the bridge>` does not exist in the wall. A startup file cannot change. A grandchild of the agent ends at Stop. CI sets `GNOMISH_REQUIRE_BWRAP`, so these do not skip.
- Live tests marked `#[ignore]`: the real `claude -p` answers in the wall in both modes, and a Bash call inside it still runs in the command sandbox. The same for `codex app-server` with a login.

**The downloads of cargo and rustup** (decided with an advisor on 2026-09-26). cargo writes each new crate into `~/.cargo/registry`, and rustup each toolchain into `~/.rustup`. The sandbox keeps both read-only, so a download fails there. The choices, and why only one works:

- A writable `~/.cargo` lets a command change a crate source that a later host build runs (`build.rs`, proc macros). That is a way out of the sandbox.
- Another `CARGO_HOME` changes the path of each crate source. cargo then builds every dependency again, and again after each host build, because the path is part of its fingerprint (checked on cargo 1.97). A large workspace builds for many minutes.
- So on Linux, `bwrap` lays a copy-on-write view over `~/.cargo` and `~/.rustup` at the same paths: `--overlay-src <folder> --overlay <upper> <work> <folder>`. The upper and work folders lie in the run temp folder. A command sees the host's cached crates at the host paths, so nothing builds again. A new crate lands in the temp folder, and the real folder never changes. Later commands of the run see it, and it goes away with the run.
- S31 stays true: every write lands in the temp folder. A hidden file in these folders, such as `~/.cargo/credentials.toml`, stays hidden, because `bwrap` covers it after the overlay. A folder that is already writable or inside a hidden path gets no overlay.
- The view needs a `bwrap` with `--overlay` and Linux 5.11 or later. The `bwrap` 0.9.0 of Ubuntu 24.04 cannot make it (checked on the CI runner), and `bwrap` 0.13.0 on Arch Linux can. At start, the bridge runs `bwrap --version` inside a sandbox with a view of `/etc`. With no view, a new cargo or rustup download fails, and the log line at start says so.
- macOS has no copy-on-write view. There cargo builds with the host's crates, and a new crate fails. For the same reason, `static.rust-lang.org` is of use only on Linux.

What each backend and OS enforces:

| Backend | Linux | macOS | Windows |
|---|---|---|---|
| Claude | The bridge sandbox (`bwrap`) around each command | The bridge sandbox (`sandbox-exec`) around each command | None: fallback. Under WSL2, as Linux (11.5). |
| Codex (`codex app-server`) | Its own sandbox: `read-only` at `ask`, `workspace-write` otherwise | The same | Its own Windows sandbox |
| Other ACP agents | None: they ask at most (6.6.3) | None | None |
| `command` | The bridge sandbox (`bwrap`) around the whole harness | The bridge sandbox (`sandbox-exec`) around the whole harness | None: the bridge does not start it |

**Codex.** Codex runs its commands in its own sandbox. The bridge sets `sandbox_workspace_write.exclude_slash_tmp`, and a private run temp folder as `TMPDIR` (checked on codex-cli 0.157.0: `thread/start` answers with `excludeSlashTmp: true`). Against S31:

- Writes match: the chat folder and the temp folder only, and at `ask` nothing.
- Network is stricter: Codex commands get no network and no proxy.
- Reads do not match: `workspace-write` reads the whole disk, with no hidden path. A command can read `~/.ssh` and the bridge keys. The bridge cannot put Codex in its own sandbox: the Codex login and rules live in `CODEX_HOME`, and on macOS the Codex sandbox cannot start inside Seatbelt. codex-cli 0.157.0 has `permissions.<profile>.filesystem.deny_read`, but its format has no documentation yet, so it waits for a live test.
- A command that a Codex `allow` rule covers runs outside the sandbox (6.6.3).

**Other ACP agents.** The bridge cannot reach their commands: an agent runs a command itself and asks only when it wants to. So they have no sandbox, and each of their calls gets at most `ask` (6.6.3). The `@anthropic-ai/sandbox-runtime` of an earlier plan is not built.

**Fallback, when the computer has no sandbox tool** (Windows, a Linux with no working `bwrap`):

- Every command asks in the game, at every level, also a command of the allow table (`gate::without_sandbox`).
- File edits inside the chat folder still work.
- The first Claude reply after the bridge starts begins with "(This computer has no sandbox, so every command asks in the game first.)". The bridge logs the sandbox tool at start. A notice in the chat header waits for new slot fields.
- With no sandbox, the popup offers no "Always allow" (6.6.5): every command asks anyway, so a rule does nothing. The second warning step of the earlier plan went.
- At start the bridge runs `bwrap --version` inside a sandbox of the same kind. A `bwrap` that is missing or cannot make namespaces counts as no sandbox.

**Windows** (tested on the Windows CI runners on 2026-09-27, and decided with an advisor). A sandbox there needs Windows API calls, and these need `unsafe` code, which every crate of this project forbids (CLAUDE.md). The planned backend was an AppContainer through the `rappct` crate (MIT, 0.13.3), approved by the user on 2026-09-26: the crate holds the `unsafe` calls. A spike on Windows Server 2022, Windows Server 2025, and Windows 11 on ARM showed that an AppContainer cannot run the commands of Claude:

- Claude Code on Windows runs each Bash tool command with Git Bash, so the wrapper must start `bash -c <command>` inside the sandbox. The Git Bash runtime (MSYS2) stops at start in an AppContainer with status `0xC0000142`, and so does each MSYS2 tool, for example `ls.exe`. The runtime makes a folder of named objects at an absolute path under `\BaseNamedObjects`, and the AppContainer redirects only the names of the Win32 calls (microsoft/mxc issue 1061). No setting changes this.
- Measured, cause unknown: a deny entry in an ACL did not stop the AppContainer. The chat folder had `icacls <chat> /grant *<capability SID>:(OI)(CI)M`. Then came `icacls .env /deny *<SID>:F`, `icacls .git\hooks /deny *<SID>:(OI)(CI)F`, and `icacls .git /deny *<SID>:(DE)`, each with the package SID, the capability SID, or ALL APPLICATION PACKAGES (`S-1-15-2-1`). A command still read `.env`, wrote `.git/hooks/pre-commit`, added a new hook, and renamed `.git`. Only `del .env` failed. This goes against the documented access check, so a later attempt checks this setup first. If it holds, a hidden path inside the writable chat folder needs a protected ACL on the user's own files and on each folder above a `.git`, which changes these files for good.
- `git.exe` of Git for Windows fails with "could not open '/dev/null'". A Rust program cannot start a child with piped or null output, because Rust std makes a named pipe in the global namespace. So cargo cannot run rustc.

What works in the spike:

- a launch with a profile (a SID with no profile gives "file not found", and `rappct` 0.13.3 falls back to such a SID with no error when the profile has no description);
- writes only where a grant names a capability SID of the command;
- no read of the home folder;
- no internet with no capability;
- a connection to an AF_UNIX socket in a folder with a grant;
- loopback between two processes of one AppContainer.

`cmd.exe` and native tools such as `curl.exe` run.

So Windows keeps the fallback, and the bridge has no `rappct` dependency. Setup recommends Codex, which has its own Windows sandbox, or Claude under WSL2, where the sandbox is `bwrap` (11.5). A later Windows backend needs a shell that starts in an AppContainer, or another kind of sandbox, for example a separate OS user. A low-integrity token blocks writes but still reads `~/.ssh`, and a virtual machine for each command is too slow. The spike source is commit `265414d` on the branch `spike-appcontainer` (CI run 36286834623), not on `main`: code with no caller costs readability.

**One launch step for every tool.** The wrapper asks `command_sandbox::launch` how to start a command inside the run walls. The sandbox tools are the variants of `Sandbox` (`bwrap`, `sandbox-exec`, none), and `launch` gives a `Launch` for each. `bwrap` and `sandbox-exec` are a program with its arguments. A Windows backend adds a variant to `Sandbox` and to `Launch`, and one arm in `detect`, `launch`, and the wrapper start. Nothing else changes: the Claude backend, the gate, and the walls stay as they are.

**Processes.** For game messages, the bridge starts one agent process per run. Each run has its own walls and its own temp folder (9.4).

**The story program of Timeways (9.7, decision 9).** The story program reads hostile text, so the bridge starts it in its own sandbox. `crates/bridge/src/story_sandbox.rs` builds it with tools that the user already has. The bridge installs nothing.

| Rule | Value |
|---|---|
| Write | Only its story folder, `<data>/timeways/story/` (mode 0700) |
| Read | The system, except the config folder, the data folder (its own folder comes back), and the existing `desktop` paths of 6.6.3 under the home folder. The lore pack and the program file stay readable, also inside a hidden folder or `/tmp`. `/tmp`, `/var/tmp`, and `/run` are private and empty, because they hold the sockets of the ssh agent and the desktop. |
| Network | None |
| Children | In the same sandbox |

| OS | Sandbox | Tests |
|---|---|---|
| Linux | `bwrap` (bubblewrap): `--ro-bind / /`, a `--tmpfs` over each hidden folder and `/dev/null` over each hidden file, a writable `--bind` of its folder, a `--ro-bind` of the lore pack and of the program file, then `--remount-ro` of each hidden folder, `--unshare-all`, `--die-with-parent`, and `--new-session`. | Real `bwrap` runs in CI (14.5). |
| macOS | `sandbox-exec` with a generated Seatbelt profile: `(allow default)`, `(deny network*)`, `(deny file-write*)`, a deny of reads and writes under each hidden path, then an allow of reads of the lore pack and the program file, and last an allow of its folder. The paths go in as `-D` parameters, never into the profile text. | Built and checked as text only. It never ran: the CI tests run with no sandbox on macOS. Mach services stay reachable. |
| Windows | None | |

- At start the bridge runs `bwrap --version` inside a sandbox of the same kind. A `bwrap` that is missing or cannot make namespaces (some systems block them for normal users) counts as no sandbox.
- With no sandbox, the story program still runs. The bridge logs a line, and the first reply with text after the bridge starts carries "The Timeways story program runs with no sandbox here." (9.8).
- `bwrap` sets `PWD` to the story folder. Its other variables are the allowlist of 6.2 rule 12.
- The `desktop` patterns that match anywhere, such as a `.env` file in a project, are not hidden. The sandbox hides only the ones under the home folder.
- **The program file** (decided with an advisor on 2026-09-26). The bridge resolves the `program` of the config to its real path, with no link in it, and starts that path. The sandbox shows only this one file, read-only, never its folder. So a program under `/tmp` or `/run` starts. A program inside a hidden path (the config folder, the data folder, or a `desktop` path) is refused, and the bridge does not start. The error is "[story] program <path> is inside a folder that the sandbox hides (the config folder, the data folder, or a credential folder). Install it somewhere else, for example ~/.local/bin.". A missing program also stops the bridge start, with "[story] program <path> is missing".
- Why: the program is the user's own choice, and the sandbox runs it anyway, so a read of its file shows nothing new. A program in a hidden path is different. The story folder is inside the data folder and is writable, so a program there can rewrite itself and keep a break-in across starts. A program in the other hidden paths is odd and can be a sign of tampering. The check reuses the list of hidden paths, so one rule holds: a hidden path never holds the program.
- Limits: the sandbox shows the program file alone. A program that loads libraries from its own folder (`$ORIGIN`), a program in a folder bundle, or a script whose interpreter lies in a hidden path does not start. Libraries under `/usr` are readable.

#### 6.6.5 "Always allow"

Since 2026-09-30, the sandbox answers at `auto-edit` (6.6.4), so the popup rarely offers Always. It stays for the allow table cases of that section, and existing rules still count.

Asked for by the user on 2026-09-27, designed with an advisor, and approved by the user on 2026-09-27. Before it, every command that the allow table did not cover asked in the game, every time. With "Always allow", the user approves a command once, and it stays approved in that folder. The code is in `crates/protocol/src/always.rs` (the rules, S36 to S39), `crates/bridge/src/always_rules.rs` (the file), `always_offer.rs` (the popup choice), and `gate.rs`.

The goal is one click for the common case, with a bounded worst case. Any game click can come from another addon (6.6.1). So a one-click rule is safe only where the sandbox is the wall for what the rule allows.

*Why one click, and no desktop click.* A hostile addon can already click "Allow" on every popup and send messages. A forged rule only adds persistence: the rule stays after the addon is gone. The rule is one pattern in one folder, and its commands run in the sandbox (6.6.4): writes only in the chat folder, network only to the allowed hosts, and secrets hidden. The larger risk of a rule is a prompt-injected agent, and a desktop click does not help against that. A desktop click for each new rule brings back most of the friction.

**When the popup offers "Always allow".** All of these hold, else the popup has only Allow and Deny:

1. The call is a shell command.
2. The run level (9.3, the lower of the chat and the config) is `auto-edit`. At `full-auto` the command runs with no question anyway. At `ask` the level promises that every command asks, so rules do not apply.
3. The commands run in the command sandbox of 6.6.4: Claude with `bwrap` or `sandbox-exec`. With no sandbox, `gate::without_sandbox` asks anyway, so a rule does nothing. An ACP agent picks its questions, so a rule never gives it `allow` (6.6.3).
4. Not Codex, for now. Codex retries an allowed command outside its sandbox with no request, and its sandbox reads `~/.ssh` and the bridge keys (6.6.4, "Codex"). So for Codex the sandbox is not the wall. The allow table of the config has the same hole today. Codex gets Always after a live test shows that the retry asks.
5. `offer` (in `protocol`) gives a proposal: 1 to 3 new rules, one for each simple command that no rule covers yet. With them the classifier gives `allow` for the whole call. So a `deny`, `desktop`, or "never always" part, a redirect into a hidden path, and a command substitution never get Always (S17, S36 to S39).
6. The rule line fits in 48 bytes, so the popup never cuts it (6.4).
7. The folder has room for the new rules: at most 64 rules per folder. A full folder gets no Always, and the Settings tab shows its rules. The bridge never drops a rule by itself, because a dropped rule brings back unexpected popups.
8. The bridge has its rules file. A gate with no file, as in `check-agent`, offers no Always.

**The pattern.** The bridge makes each rule from the words of one simple command, with `propose` in `protocol`. The agent and the game never choose it.

- For a tool with subcommands, the rule is its name and first word: `cargo test *`, `git status *`, `npm run *`. Such a tool with no first word, such as `cargo` alone, gets no rule, because `cargo *` also covers `cargo publish`. The list is in `always.rs`: `git`, `cargo`, `npm`, `pnpm`, `yarn`, `go`, `uv`, `pip`, `poetry`, `gradle`, `mvn`, `dotnet`, `rustup`, and `just`.
- For every other tool, the rule is the name only: `rg *`, `ls *`, `pytest *`, `tail *`, and `make *` for `make test`.
- Why not narrower: at `auto-edit` the agent can edit `package.json`, a `Makefile`, and the tests in the chat folder. So `npm run build *` protects nothing more than `npm run *`, and it costs more clicks.
- No proposal when the first word after the name starts with `-` or `+` (`git -C x status`, `cargo +nightly test`), because `git *` is far too wide.
- No proposal when the name holds `/` (`./gradlew`, `./x.sh`). The agent can write such a script, so the rule means "all Bash". A user adds such a script to the allow table by hand.
- No proposal for tools that run any program or download code: `npx`, `npm exec`, `pnpm exec`, `pnpm dlx`, `yarn dlx`, `yarn exec`, `bunx`, `uvx`, `pipx`, `uv run`, `poetry run`, and `docker`. They amount to "all Bash".
- No proposal for commands that publish or send to other people: `git push`, `cargo publish`, `npm publish`, `twine`, and `gh`. The proxy reaches `github.com`, and a push can leak data (6.5). The allow table of the config can still name them.
- Each word is printable ASCII, 1 to 64 bytes, with no special character of the allow table (12). The rule holds the literal bytes of the words, because the matcher compares bytes.
- A `cd` is a simple command like any other, so `cd lib && cargo test` proposes `cd *` and `cargo test *`. A `cd` with a redirect stays `desktop` (6.6.3).

**The folder of a rule.** A rule covers one folder: the resolved chat folder, and every chat inside it, as `[allow.folders]` does. One exception: when the chat folder is an entry of `allowed_roots` or the home folder, the rule covers that exact folder only. Else one click in `~/Documents/Code` gives a global rule from the game. The rule folder is the chat folder, not the current folder of the Bash tool, so a `cd` into a subfolder keeps the chat rules.

**The popup** (6.4). The buttons are Allow once, Always allow, and Deny. One more line, above the buttons, names the rule: "Always allow: cargo test *, tail * in Code/Personal/gnomish-relay".

- The folder is its path from the folder above its allowed root, `~/` in the home folder, else the full path. A folder name that does not fit is cut from the left, after "...".
- The line is bridge text: the rule words (plain ASCII) and a folder name that the user chose, with each non-printable-ASCII character as `?`. The addon shows it with the escape of S10.
- It comes in the `label` of the `allow_always` option of the live file, so S20 does not change. The button is "Always allow", the label that `Popup.lua` already has for the kind.
- The answer hash (6.6.1) of an Always answer covers the popup text and this line, so the hash binds the rule that the user saw. The bridge fixed the rule when it opened the request, so an answer only picks the option.

**After a grant.**

- The bridge adds the rules to `rules.json` and allows the call. An existing rule gives no second row.
- Every other open request that the new rules now cover runs, so the user does not click twice. A run that waits for the game reads the rules about every 100 ms. When they cover its call, the call runs, and the run tells the bridge to take its popup away.
- On the click, the addon prints one whisper line: `[Claude] whispers: [chat] Always allowed now: cargo test * in Code/Personal/gnomish-relay. Click to manage your rules.` A click on the line opens Settings. The addon marks the settings list as old, so the next Settings open asks for a new one.
- The desktop shows a plain notice: "Always allowed now: <line>. To remove it, use Settings in the game or run gnomish-relay rules.". There is no Undo button: a notice cannot hold a button on all three OSes, and the whisper line has none.
- A rule that the bridge cannot write leaves a log line. The call still runs, because the user allowed it.

**The store.** The rules live in `rules.json` in the data folder (12), never in `config.toml`: the game never writes the config (6.6.2). The data folder is a `deny` path, so the agent never reads or writes the file (6.6.3). The file has mode 0600, and the bridge writes it with an atomic rename. Each row has an id (4 hex digits), the folder, its scope (`tree` for the folder and every folder inside it, `exact` for a root or the home folder), the words, the time it was added, and the day of its last use.

- The bridge reads the file again for each tool call, so `gnomish-relay rules remove` works while the bridge runs. A lock keeps two runs from losing each other's rule.
- At load, each row must have the shape that `propose` makes: 1 or 2 plain words (the word rules above) with no `/` in the first, an absolute folder with no `.` or `..`, and no unknown field. A link in place of the file, or a file over 1 MiB, is no rules. A bad row is dropped with a log line, so a broken file never widens a rule. A missing or broken file is an empty list.
- Global rules stay in `[allow] commands` of the config, which the user edits by hand.

**Expiry.** A rule ends 30 days after its last use. The group says "Rules expire after 30 days without use." So a used rule stays, and a stale or forged one goes. The bridge writes the last-use day at most once a day per rule, so a command does not write the file each time. A deleted or moved folder leaves orphan rules, which expire. Until then, a new folder at the same path gets them, and Settings shows them.

**See and remove.**

- The Settings tab (13.1) has a group "Always allowed": one row per rule, with its pattern, folder, last use, and a remove button. The mouse wheel scrolls it. The game can remove a rule, because a removal only narrows: a forged removal costs only a click. The addon sends `rule=remove:<id>` in a control record of the chat `settings`, then asks for a new list. The bridge removes the rule before it answers the list. A removed row stays grey ("Removing...") until the next settings list.
- `gnomish-relay rules` lists the rules on the desktop, and `gnomish-relay rules remove <id>` removes one.
- The settings list (13.4) gets `rule` lines. Diag shows no second copy.

**What never becomes a rule:**

- File edits: a write outside the chat folder is `desktop`, and one inside it already runs at `auto-edit`.
- Unknown tools.
- Every `deny` and `desktop` answer, so the startup files (`.claude/`, `.git/hooks/`, and the others of 6.6.3) and the hidden paths.
- The "never always" commands of 6.6.3.
- Desktop requests: the desktop dialog never offers Always, because a desktop request is the dangerous case.

**The own "always" of each backend.** The `permission_suggestions` of Claude and the `acceptForSession` and `acceptWithExecpolicyAmendment` of Codex stay off (9.3). The bridge keeps the only rules, so the classifier sees each rule.

**Verification.** The user approved S36 to S39 on 2026-09-27. They are proved (`proofs/Protocol/Always.lean`), and `proofs/Axioms.lean` checks their axioms. The approved Lean shapes below are the plan; `proofs/Statements.lean` has the exact text. There, `isDesktop` is `desktopSimple`, `isCapped` is `neverAlways`, `simplesOf` is `inCall`, and `rules ++ rs` is every slice that holds the rules and then `rs`. `plainWord` asks for printable ASCII with no space, which is stricter than the plan.

- In `protocol`, in the Aeneas subset: `propose(simple) -> Option<rule>` and `offer(call, policy, rules) -> Option<rules>` in `always.rs`. The matcher `rule_matches` is the one of S17. `offer` checks its rules with `classify` before it returns them. It refuses a rule list of more than 4096 rules, so the join of the lists never overflows.
- **S36, proposal shape.** For every simple command, `propose` never panics, and `propose(s) = some r` gives `r = take k s.words` with `k` 1 or 2. Each word of `r` is 1 to 64 bytes of printable ASCII with no special character, and does not start with `-` or `+`. The first word holds no `/`. So the rule covers its source command. Lean shape: `∀ s, always.propose s ⦃ o => ∀ r, o = some r → ∃ k, (k = 1 ∨ k = 2) ∧ r.val = s.words.val.take k ∧ (∀ w ∈ r.val, plainWord w.val) ∧ ¬ hasSlash (r.val.head!).val ⦄`.
- **S37, no proposal for the capped.** For every simple command that is `desktop`, "never always", a tool that runs any program, or a command that publishes, `propose` gives `none`. Lean shape: `∀ s, (isDesktop s ∨ isCapped s.words ∨ noRuleTool s.words) → always.propose s ⦃ o => o = none ⦄`.
- **S38, an offer allows exactly its call.** For every call, policy, and rule list, `offer` never panics, and `offer = some rs` gives: `rs` has 1 to 3 rules, `classify(call, rules ++ rs) = Allow`, and each rule of `rs` covers a simple command of the call. Lean shape: `∀ call policy rules, always.offer call policy rules ⦃ o => ∀ rs, o = some rs → 1 ≤ rs.len ∧ rs.len ≤ 3 ∧ action.classify call policy (rules ++ rs) = ok Verdict.Allow ∧ ∀ r ∈ rs, ∃ s ∈ simplesOf call, ruleMatches r s.words ⦄`.
- **S39, an offer stays under the ceiling.** For every call, `offer = some rs` gives `ceiling(call) = Allow`. It follows from S17. Lean shape: `∀ call policy rules rs, always.offer call policy rules = ok (some rs) → action.ceiling call policy = ok Verdict.Allow`.
- Each goes into `proofs/Axioms.lean`.
- Fuzz targets:
  - `always` (random simple commands and calls): `propose` and `offer` never panic, each proposal is a prefix of its words, and each offer makes `classify` give `allow`.
  - `rules_file` (any text as `rules.json`): no panic, and each loaded rule has an id of 4 hex digits, a clean absolute folder, 1 or 2 plain words, and a last use in the last 30 days.
- Unit tests in the bridge (`always_rules.rs`, `always_offer.rs`, `gate.rs`, `activity.rs`, `settings_list.rs`, `flags.rs`, `relay.rs`): the folder of a rule (a root and the home folder cover only themselves), expiry and the daily write, the full list, a duplicate rule, the answer to the other open requests, the level and backend checks of the offer, the 48-byte line, and the settings lines.
- Fake-game tests in `addon_flow.rs`: the popup shows the rule line and the three buttons. An Always click sends the hash of the text and the line, and whispers the rule. The bridge takes it. An Always answer that another addon forges with the hash of the text alone counts for nothing. The whisper line opens Settings. Settings lists the rules, and a remove sends the id, asks for a new list, and greys the row.
- With the fake agents (`claude_gate.rs`, `codex.rs`): an Always click adds the rule, and the next same command runs with no question. With no sandbox, and for Codex, the popup has no Always.
- In `gate.rs`: at `ask` the popup has no Always and a rule does not apply. A `git push`, an `rm -rf`, and `npx` get no Always. A rule that another popup adds ends an open question.

#### 6.6.6 Git actions from the game

Asked for by the user on 2026-09-29, designed by the implementer. Section 9.11 has the features. The game can commit, revert, merge, and discard the work of a chat, and ask for the CI checks of its branch. Any game click can come from another addon (6.6.1), so each action gets the level of its worst case. The bridge runs git itself, on the host, never the agent.

| Action | From the game | Why this level |
|---|---|---|
| Own branch (`branch=1`) | The flag of a message | It makes a folder next to the repository and a branch, as `mkdir=1` makes a folder (9.9). Nothing runs in it that a message cannot start anyway. |
| Commit | One click, at every level | It writes only the git folder of the chat folder: new objects, the index, and the checked-out branch. Hooks and `core.fsmonitor` are off, so no chat folder code runs. `git reset` undoes it. At `ask`, the click is the answer that a `git commit` command asks for. |
| Revert | One click after a confirm in the game | It writes only the files that the run changed, in the chat folder, and only while they still hold what the run left. A write in the chat folder is what an "Allow once" click gives. The bridge logs the run tree, so `git restore --source=<tree>` brings the files back. |
| Discard | One click after a confirm in the game | It removes the chat's own copy and its branch, both the work of the chat. The bridge logs the last commit, so `git branch <name> <commit>` brings the branch back. Uncommitted changes in the copy go into that commit first. |
| Merge | Approve on the desktop | It writes outside the chat folder: the start branch of the chat, and the folder that has it checked out. The agent code then runs on the host with no sandbox, for example at the next build. 6.6.3 makes every write outside the chat folder `desktop`, so a merge is `desktop` too. |
| Checks | One click, only with `[git] ci_checks = true` | It only reads GitHub, with the user's login (9.11, "CI checks"). |

- Each action is a message of its chat, with the flag `git=<action>` (7.1.1). So it goes through the replay store (S7), the rate limit, and the chat queue: it waits for a chat run to end, and a replayed strip never repeats it.
- The confirm in the game stops a misclick, not another addon. The level of each action already holds when another addon clicks.
- Commit and Revert name a run by its message id. The bridge acts only on its own record of that run (9.11), never on game or agent text. The commit message is the only game text, and it goes to git as one argument.
- git on the host trusts `.git/config` and the hooks. The sandbox hides them from commands, and the classifier makes every file tool write to `.git` `desktop` (6.6.3, 6.6.4). So they hold what the user put there. The bridge still turns off the hooks (`core.hooksPath` names an empty folder, and `--no-verify`) and `core.fsmonitor`, because a user hook can run chat folder code, for example a lint config that the agent wrote.
- Limit: a filter driver that the user set up, such as Git LFS, runs at `git add`. The chat folder can only pick a user driver in `.gitattributes`, not define one. A copy whose git files another chat pointed at another git folder gets no git call at all (9.11, "The link check").
## 7. Transport

WoW addons run in a sandbox. An addon cannot open a network socket or read a file while the game runs, with one exception (7.3).
Gnomish Relay uses three side channels: the strip in a screenshot (out), slots (in), and a reload fallback.
Signals (7.4) do not work on the tested client.

### 7.1 Strip: game to bridge

The addon draws a strip in the top-left corner of the screen (smallest form: the line of 7.1.3), then calls `Screenshot()` from a timer.
WoW saves a PNG in `_classic_beta_/Screenshots`. The bridge watches that folder, decodes the strip, and deletes the file.
The spike proved this path (2026-09-23): the call takes under 1 ms, the file arrives after about 0.4 s, and every color is exact.

- At login, the addon sets the `screenshotFormat` CVar to `png`.
- The addon hides the "Screen captured" text of its own screenshots through the `ActionStatus` frame. Screenshots of the user still show it.
- The bridge ignores screenshots with no valid strip. Those are the screenshots of the user.
- The first strip ever prints one line, once (the saved variables remember it): "<title>: the colored bar that flashes at the top left is how your messages reach the desktop app. That's normal."
- In combat, a strip waits for the end of the fight, unless it carries a message or a player control (Stop, a permission answer, a rule delete). So a hello and a list request (sessions, folders, settings) wait, but ride on any strip that goes anyway. A long fight can reach the end of the slot window (7.3). Polls and replies then wait for the next strip, and none is lost.

**Frame layout (bytes):**

```
[0x6E 0x52] [version] [time: 4 bytes] [frame id hi, lo] [len hi, lo] [payload: len bytes] [fletcher16 s1, s2] [mac: 8 bytes]
```

- Magic bytes `0x6E 0x52` differ from `wow-claude` (`0xC7 0x1A`). A wrong magic means "not a strip".
- `version`: the protocol version, 1 for this spec.
- `time`: the Unix time from `time()` in the game, big-endian, for the freshness check (S11).
- `frame id`: the message id modulo 65536. It only tells frames apart. The record ids are the real keys.
- Fletcher-16 covers version to payload, to catch damaged pixels.
- The MAC covers magic to checksum, to stop fake strips (6.3).
- `len` is at most 3200. The addon refuses longer text and tells the user.

**Cells.** This part is the old strip, now the fallback of the line (7.1.3).

- Each cell carries 3 bits, most significant first: bit 2 red, bit 1 green, bit 0 blue. Each channel is fully on or off, so there are 8 colors.
- The decoder reads each channel at the cell center and compares it to 128.
- A row has 200 cells. A strip has at most 48 rows.
- A cell is 3 physical pixels (`GetPhysicalScreenSize()`, `SetIgnoreParentScale`, strata `TOOLTIP`), so the old strip is 600 by at most 144 pixels. Its center pixel is away from both neighbors, so a 1-pixel blur does not reach it. A 2-pixel cell has no such center.

**The decoder finds the grid itself.** A scaled screenshot makes the cell size fractional: the spike measured 3.875 px by 4 px at 1280×720, before the addon set its own scale.
So each strip starts with two calibration rows of known colors: row 1 counts 0 to 7, row 2 counts 7 to 0.
The decoder tries every cell size from 2 to 8 pixels and keeps a size that matches both rows exactly. Row 2 runs backwards, so a grid one cell off fails. The lower bound of 2 reads 3-pixel cells in a screenshot scaled to two thirds.
The two rows fix the cell width, not the row height. So the decoder reads the data rows with each matching size. It keeps the first reading that decodes as a frame with a valid checksum and a tag that checks under a key. The checksum does not cover the tag, so a wrong row height can read the payload right and a tag alone in the last row wrong. If no reading passes the tag, the bridge logs the first reading with a valid checksum as rejected.
The search starts at the top-left corner of the image. With 8-pixel cells, a strip is 1600×384 pixels.
The decoder also reads the bytes past the frame end, which the frame ignores (its header gives the length). The beacon of the line test (7.1.4) sits there.

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

The flags split in two (9.7, decision 6). Every app sends the **transport flags**: `h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, and `restored`. Only the relay reads the **coding flags**: `perm=`, `level=`, `agent=`, `attach=`, `list`, `list=folders`, `list=subfolders`, `list=settings`, `mkdir=1`, `branch=1`, `git=`, `d`, `n`, and `stop`. `flags.rs` has one parser for each part, so a coding flag from another app does nothing.

| Flag | Meaning |
|---|---|
| `n` | Start a new agent session for this chat. |
| `h` | Hello only, with no prompt. It announces the token and the addon version. Sent at login and after the addon applies a restore bundle (7.6). |
| `d` | The chat is deleted. The bridge stops its runs and drops its replies, session link, and history. Nobody can read its replies, so they must leave the body (7.3). The addon keeps the id in `db.forget` and sends it with each strip until a strip goes out while the bridge is online. The agent session stays, so Resume can bring the chat back. |
| `list` | Asks for the saved agent sessions (9.6). The record is a message of the chat `relay`. The reply is the list. |
| `list=folders` | Asks for the folder tree of the browser (9.9). The record is a message of the chat `folders`. The reply is the tree. Other `list=` values are ignored. |
| `list=subfolders` | Asks for the subfolders of the `cwd` folder, for the browser (9.9, "One folder"). The record is a message of the chat `subfolders`. The reply is a tree as for `list=folders`. |
| `list=settings` | Asks for the bridge settings list (13.4). The record is a message of the chat `settings`. The reply is the list. |
| `mkdir=1` | The record folder is new. The bridge makes its last part before the run (9.9), only for a record with `n`. Other `mkdir=` values are ignored. |
| `branch=1` | The chat works on its own branch, in its own copy of the repository (9.11). The addon sends it with every message of such a chat. Other `branch=` values are ignored. |
| `git=<action>` | A git action of the player on the chat (9.11, 6.6.6). `commit:<id>` and `revert:<id>` name the message of a run with a change summary. `merge`, `discard`, and `checks` act on the chat. The text of a `commit:` message is the commit message. Another value gives the error reply "The desktop app doesn't know that action. Update it: run gnomish-relay update." |
| `attach=<session>` | The first message of a resumed chat, with no text. The session must be in the last list (9.6). |
| `agent=<name>` | The agent for a new chat. With no `[agents.<name>]` entry in the config, the message ends with "That agent isn't in config.toml. Pick another one in Settings, or add it on your desktop." |
| `level=<level>` | The chat mode: `ask`, `auto-edit`, or `full-auto`. The run gets the lower of this level and the chat ceiling: the agent level in the config, at most `auto-edit`, or `full-auto` after a desktop Approve for the chat (S6, 9.3). An unknown word counts as `ask`. |
| `perm=<request>:<option>` | The answer to a permission request (9.3). |
| `read=<id>,<id>` | The final replies in the last body that the addon showed. The bridge then takes them out of the slot body (7.3). A lost strip loses nothing: the next strip names them again. |
| `restored` | The addon applied the restore bundle for its token (7.6). |
| `next=<n>` | The next slot that the addon loads (7.3). |
| `build=<n>` | The client build from `GetBuildInfo`, digits only (7.8). |
| `ver=<n>` | The protocol version of the addon (7.7). |
| `out=shot` or `out=fail` | The result of the last screenshot (7.8). |
| `in=slots` or `in=missing` | The result of the last slot load (7.8). |
| `listen=start` or `listen=stop` | Push-to-talk for the record chat (13.3, later). |
| `voice=skip` or `voice=stop` | Skip to the next paragraph of the spoken reply, or end it (13.3, later). |

**Strip lifetime:**
The strip shows only while its screenshot is taken, about half a second.
With no acknowledgment in 40 seconds, the addon shows it again, up to 3 times in all, then uses the reload fallback (7.5).
A wait for the shared corner (7.1.2) is not part of the 40 seconds, and it is not a show.

#### 7.1.2 The shared corner

Every app of the shared transport (9.7, decision 14) draws its strip in the same top-left corner.
Two strips at once give a screenshot that no app can read.
Also, a `SCREENSHOT_SUCCEEDED` or `SCREENSHOT_FAILED` event has no owner: every addon gets it.
So the apps take turns through one shared global, `GnomishStripCorner` (9.7, decision 13).
`Strip.lua` follows `models/corner.qnt` (14.2).

**The value.** It holds the holder (the name of the app's strip frame), the end time of the hold, and a wait mark for each waiting app.
A wait mark holds when the app started to wait and when it last asked. All times come from `GetTime()`, one clock for all addons.
WoW Lua runs one handler at a time, so an app reads and writes the value in one step.
A `/reload` resets the globals of all addons together, so no holder stays from an older UI session.

**The rules.**

- An app takes the corner when no other app holds it or waits longer. A hold ends after 12 seconds, so an app that stops with an error frees the corner.
- After its strip ends, the app keeps the corner for a 2-second tail. So a late event of its own shot finds no strip of another app to end.
- An app that cannot take the corner writes its wait mark and asks again at its next Tick, one second later. A mark older than 3 seconds belongs to an app that stopped waiting.
- With no hostile addon, an app waits at most 15 seconds: its own tail, one strip and tail of the other app, and one Tick.
- While an app waits, it signs nothing and no show counts. So its 40-second retry timer and its 3 shows wait too, and a wait never starts the outbox.
- A screenshot event ends a strip only when the app holds the corner and has called `Screenshot()`. So `out=shot` and `out=fail` report only the app's own shots.
- A player screenshot during our shot can still end our strip early, because an event has no owner. This costs at most one early end or one wrong `out=` value, and the next retry covers it. So the addon does not try to match events to shots.
- After 30 seconds of waiting, the app shows one line: "<title>: another addon is in the way of the colored bar. Turn off addons that take screenshots, then type /reload." The window shows "Screenshots blocked". The line shows again only after the corner was free in between. 30 seconds is twice the longest honest wait, and far below the 270-second limit of a signed frame.
- A blocked app keeps waiting and does not use the outbox. A hostile holder stays across every `/reload`, so the outbox then asks for a reload for each message.
- Each app hooks the "Screen captured" text, and each hook hides it only for its own app's shots. A second `Hide` does nothing.

**A hostile addon.** Every addon can read and write the value. A hostile addon can already block the strip, for example with a hook on `Screenshot()`. So the blocked line is the answer to a hostile holder.
The addon reads and writes the value with `rawget` and `rawset`, so a metatable has no effect. A value or field of a wrong type counts as missing.

**Versions.** The Timeways copy of the transport is pinned to a relay tag, so two versions of `Strip.lua` can run at once. A new shape of the value needs a new global name.

#### 7.1.3 The line: a strip of 1-pixel cells

**Status: built (2026-09-29).** The old strip (7.1) is 600 by up to 144 pixels, its size changes with each message, and players see it in play. The line is the smallest strip that reads exactly: 1-pixel cells in a line 1 pixel tall at the top-left corner. The old strip stays as the fallback.

**Modes.** A mode is a cell size and a number of bits per cell. The addon's line test measures each mode (7.1.4), and the addon draws the smallest mode that reads exactly. For developers, the self-test measures them too (14.3.1).

| Mode | Cell size | Bits per cell | Bits per channel | Levels of a channel |
|---|---|---|---|---|
| 1 | 1 px | 24 | 8 | 0 to 255 |
| 2 | 1 px | 12 | 4 | 0, 17, ..., 255 |
| 3 | 1 px | 6 | 2 | 0, 85, 170, 255 |
| 4 | 2 px | 24 | 8 | 0 to 255 |
| 5 | 2 px | 12 | 4 | 0, 17, ..., 255 |
| 6 | 2 px | 6 | 2 | 0, 85, 170, 255 |

The table is in order of preference. The 1-pixel modes come first, because height matters most to the player.

**The line.**

- A row has 200 cells. The line starts at pixel (0, 0). Row `r` starts at `y = r × size`.
- The cells, in order: the marker (10 cells), the check (12 bytes), then the frame (7.1), with the same bytes, checksum, and tag.
- Zero bytes pad the frame to a multiple of 3 bytes. Black cells fill the last row to 200 cells.
- So the width is fixed, and a long frame adds rows. At 24 bits, one row holds a frame of up to 558 bytes. The largest frame (3221 bytes) takes 6 rows at 24 bits, 11 at 12 bits, and 22 at 6 bits. Why a fixed width: the player sees a steady shape, and most frames fit one row. A width that follows the frame changes with each message, and the player asked us to stop that.
- **The marker** is 10 cells of full colors (each channel 0 or 255, 3 bits as in 7.1): `7 0 4 2 1 6 5 3`, then the mode `m`, then `7 − m`. Every mode draws full colors exactly, so the reader finds the marker before it knows the mode.
- **The check** is the 12 bytes `01 23 45 67 89 AB CD EF FE DC BA 98`, packed as the frame. It holds every level of every channel in all three bit counts. A reader that gets them wrong has the wrong mode or a changed picture.
- **Packing.** Three bytes are 24 bits: 1, 2, or 4 cells, most significant bit first. In each cell, the first third of the bits is red, then green, then blue. A channel of `k` bits with level `L` has the value `L × 255 / (2^k − 1)`.

**The addon draws it.**

- The strip frame ignores the parent scale and has the scale `768 / h`, where `h` is the height from `GetPhysicalScreenSize()`. So one UI unit is one physical pixel. `PixelUtil.GetPixelToUIUnitFactor` of the Forever client computes the same factor, but the addon does not call `PixelUtil`: the formula is one line with no second use.
- Each cell is a texture of `size × size` units at a whole-unit offset, with `SetColorTexture(r / 255, g / 255, b / 255)` and `SetSnapToPixelGrid(true)`. The snap rounds a float error of the scale to the nearest pixel.

**The reader.** For each cell size, 1 then 2, the reader reads the 10 marker cells at the top-left corner. It reads cell `c` of row `r` at pixel `(c × size + size / 2, r × size + size / 2)`, with integer division. The marker must match exactly, with a mode of this cell size. Then the check must read back exactly. A channel value `v` reads as level `round(v × (2^k − 1) / 255)`. Then the frame in the other rows must decode (7.1). With no line, the reader searches the grid of the old strip.

**How the mode reaches the addon.**

1. The line test (7.1.4) gives the bridge a result for the physical screen size of the game: a mode, or no mode with a reason. The bridge keeps it in `strip-line.json` in its data folder. `gnomish-relay selftest collect` (14.3.1) also writes there.
2. At each publish, the bridge reads the file (at most 4 KiB) and sends the newest result. A mode is 0 to 6, where 0 means that no mode reads exactly. Each screen side is 1 to 16384 pixels. A file that fails counts as no file, and the bridge logs each new error once. The bridge adds one line after the body of each app (7.3): `GnomishRelay_SlotData.line = {mode = 1, width = 2560, height = 1440}`. As with the key check, the line holds only decimal numbers, so S9 still covers the table.
3. `Slots.lua` hands the line of each loaded body to `Strip.lua`. It keeps a line with a mode of 0 to 6 and two numbers in the app's saved variables, as `stripLine`. A body with no line removes `stripLine`. An addon from before v0.3.1 knows no mode 0, so it drops that line and keeps the old strip.
4. The reader finds the mode in the marker. So the bridge needs no state for it, and a strip of either shape reads.

`Slots.lua` and `Strip.lua` are shared transport files (9.7). Timeways gets the line and the line test with its next transport pin, with no change to its code.

**Fallback.** The addon draws the old strip in each of these cases:

- It has no `stripLine` with a mode of 1 to 6: the line test has not run, found no clean mode, or the bridge has not sent the result yet.
- The physical screen size is not the size of `stripLine`. The next old strip then carries the line test again (7.1.4).
- The strip carries the same frame id as the last line, and the id is not 0. This is a message retry (7.1, "Strip lifetime"), so the line did not reach the bridge. The retry uses the old strip, which always reads.
- Two different frame ids needed such a retry in this UI session. The line then stays off until the next `/reload`. A retry can also come from a bridge that was off, so a `/reload` tries the line again.

**Decisions.** The implementer chose these (2026-09-29):

- The packing lives in the bridge reader (`crates/bridge/src/line.rs`) and in `Codec.lua`, with differential tests between them, as `addon_codec.rs` does for the frame. The proved core (`crates/protocol`) does not change: the frame bytes, checksum, and tag are the same, and S1 and S3 still cover them. The line reader reads untrusted input, so the `screenshot` fuzz target covers it.
- A 2-pixel cell is the next step after 1 pixel. A 3-pixel cell is the old strip.
- Since v0.3.1, the relay addon measures the modes (7.1.4). v0.3.0 left that to the self-test, to save 6 shots and 6 flashes at each login. But the self-test never ships, so no player got the line. The line test takes no shot of its own: it rides on an old strip that goes anyway.

**Tests.** `line.rs` and `calibration.rs` have unit tests for each mode, the marker, the check, and each verdict. `tests/strip.rs` reads a line of each mode through a PNG. `tests/addon_codec.rs` compares `Codec.LineRows` with `line::rows`. The fake game keeps each shot as rectangles of physical pixels (`picturesOf`). So `tests/strip_line.rs` draws the line of `Strip.lua` into a PNG, reads it in every mode, and checks each fallback. `tests/selftest.rs` runs the self-test in the fake game, and collect on its pictures. Sharp pictures choose mode 1. Blurred pictures keep the old strip, with a verdict for each mode.

#### 7.1.4 The line test: the addon measures the modes

**Status: built (v0.3.1).** No player runs the self-test, so the relay addon measures the modes of 7.1.3 itself, with no setting and no extra shot.

**When.** The addon adds the line test to an old strip in these cases:

- It has no `stripLine` for the current physical screen size, for example at the first strip ever or after a resolution change. Then at most 3 old strips of a UI session carry the test, so a bridge that never answers costs little.
- It has a `stripLine` of mode 0 for this screen: the last test found no clean mode. Then the first old strip of each UI session carries the test, so a fix of the game settings takes effect after a `/reload`.

With a `stripLine` of mode 1 to 6 for this screen, the addon never adds the test. A retry and a line that is off (7.1.3, "Fallback") draw the old strip with no test.

**What the addon draws.** The message still goes in the old strip, so nothing waits for the test. In the same picture, the addon draws two more parts:

- **The beacon.** 8 bytes right after the frame, in the old strip cells: `4C 54`, the physical width and height from `GetPhysicalScreenSize()` (2 bytes each, big-endian), and the Fletcher-16 of those 6 bytes. It tells the bridge that a test is in the picture, and for which screen. The frame, its checksum, and its tag do not change.
- **The test lines.** One line of 200 cells for each mode of 7.1.3, drawn as `Codec.LineRows` draws a line: the marker, the check, then a 96-byte test payload. The payload is 12 bytes each of `00`, `FF`, `55`, and `AA`, then the 48 bytes `(i × 37 + 11) mod 256` for `i` from 0 to 47. The flat runs show a color shift (`55` and `AA` are the mid levels that gamma moves). The rest gives edges in every channel. The payload fits one row in every mode. The line of mode `m` starts at pixel `(608, 4 × (m − 1))`: 8 pixels right of the old strip, with a gap between lines. So the test takes 400 by 22 pixels next to the old strip.

**What the bridge does.** After the bridge takes a strip (its tag checks), it looks for the beacon after the frame. With a beacon:

1. A screenshot of another size than the beacon is scaled, so every mode gets the verdict "scale".
2. Else the bridge cuts each test line out of the screenshot and judges it with the self-test verdicts (14.3.1): clean, color shift, blur, scale, or not found.
3. The result is the first clean mode in the order of 7.1.3. With no clean mode, the result is mode 0. Its reason is the first blur or color shift in that order, else a scale, else "not found". A blur of 1-pixel cells can show the marker at another width by chance, so a scale says less than a blur.
4. The bridge writes the result into `strip-line.json` as the newest result, in place of an older result for that screen size. The file keeps the last 8 screen sizes. Then both lanes publish, so the addon gets the line with its next slot.

**What the player sees.** The first strip of the first session shows the old strip with the small test next to it, for about half a second. With a clean mode, every later strip is a line 1 pixel tall. With no clean mode, the old strip stays, and the bridge says why:

- `gnomish-relay status` prints one line for the newest result: "Colored bar: " and the text below. The settings list (13.4) carries the text as `strip`, and the Diag tab shows it in a row "Colored bar". Players know the strip as the colored bar (README), so the copy uses that name.
- "a thin line, 1 px tall (mode 1), for 2560x1440." for a clean mode. The pixels, the mode, and the screen follow the result.
- "full size, because your screen blurs 1-px lines (anti-aliasing, render scale, or an upscaler). Messages still get through." for blur.
- "full size, because your screenshots are scaled (render scale below 100%, or a screen size that isn't the screenshot size). Messages still get through." for scale.
- "full size, because your game changes colors (gamma, brightness, or a color filter). Messages still get through." for a color shift.
- "full size, because the test line didn't show in the screenshot. Messages still get through." for not found.
- "not measured yet. Your next message from the game measures it." with no result.

**Decisions.** The implementer chose these (2026-09-30):

- The test rides on a real strip. A shot of its own needs a frame with no message and one more turn of the shared corner (7.1.2).
- The beacon sits in the old strip cells. A record flag changes the records, and Timeways copies the transport at a pin. The old strip reads in every picture that the bridge can read, so a blur or scale that hides the lines still leaves the beacon.
- The bridge takes the screen size from the beacon, not the image. The addon compares `stripLine` with `GetPhysicalScreenSize()`, and a scaled screenshot has another size.
- The test lines sit right of the old strip, not under it. So the flash stays in the top 144 pixels, and a screen 1024 pixels wide shows every line. A narrower screen cuts the 2-pixel lines first.
- The bridge sends mode 0, so the addon stops the test for the rest of the session. The test comes again after a `/reload`, so a settings fix shows up with no command.
- A test line is a known picture, so it never holds a prompt. The strip file goes as usual.

**Tests.** `crates/bridge/tests/line_test.rs` runs `Strip.lua` in the fake game and reads its pictures through PNGs. A fresh session draws the test with its first strip. The bridge sends the smallest clean mode, and the next strip is a 1-pixel line. A blurred test keeps the old strip, and the status says why. A new screen size tests again. A Timeways strip carries the test too. `line_test.rs` in the bridge has unit tests for the beacon and each result.

### 7.2 Why each channel works

`wow-forever-codex` measured these rules of the WoW client on Windows:

1. The client finds addon files only at launch. A file that does not exist at launch is never found.
2. The client reads a load-on-demand addon from disk when it loads, not at launch.
3. Each addon loads once per UI session. `/reload` starts a new UI session.
4. `PlaySoundFile` fails for an empty `.wav` file and works for a valid one.
5. After a `.wav` file plays once, the client treats it as valid until the client process stops, even across a `/reload`.

The spike tested the rules under Wine (2026-09-23). Rules 1, 2, and 3 hold. Rule 4 fails: `PlaySoundFile` reports "will play" for an empty file. So rule 5 does not matter.

### 7.3 Slots: bridge to game

There are 1000 slots. The addon loads them in order, from the first slot that it has not loaded in this UI session.

The bridge writes each body only into a window of 30 slots, starting at the next slot that the addon reported (`next` flag, 7.1.1).
All 1000 slots at every publish cost too much disk: a 20 KB body every 3 seconds is 20 MB per publish.
Each file gets a sync and an atomic rename. The bridge skips a file that already holds the same bytes: a sync costs milliseconds on Windows, and the restore and live files seldom change.

- The addon reports `next` in every strip. Near the end of the window with no strip to send, it sends a hello with `next`.
- At a hello, or when the saved variables file changes (a `/reload`), the bridge starts the window at the reported slot, or at slot 1.
- Each token has its own window: two WoW accounts on one computer can play at once (7.6), and each game loads slots from its own place. The bridge keeps the windows of the 3 tokens with the newest reports, and each publish writes all of them. A strip, or a changed saved variables file, moves only the window of its token. A file with no token moves every window to slot 1.
- A slot outside the window holds an older body. The addon never loads past the window of its last strip. In a long fight, the polls stop there and go on after the hello at the end of the fight.
- A slot of an earlier UI session can still hold an older body, with an old live file, restore bundle, `working` records, and clock. So the addon skips a body whose `now` is older than the `now` of the last body that it applied.
- A skipped body loses no reply. Every record stays in the body until a `read` flag names it, so a later poll gets it. The model (14.2) checks this.

Each slot is a folder `GnomishRelay_S0001` to `GnomishRelay_S1000` with four files:

- `GnomishRelay_SNNNN.toc`: the `## Interface` list of 7.9, a grey `## Title` ("Gnomish Relay reply slot NNNN (leave on)"), `## LoadOnDemand: 1`, `## Dependencies: GnomishRelay`, and the three Lua file names. The AddOns list of the game shows all 1000 slots, so the title says what they are and to leave them on.
- `Inbox.lua`: the body.
- `Restore.lua`: the restore bundle (7.6, S18). With no restore, its token is empty, and no addon takes it.
- `Live.lua`: the progress lines of each run, and the permission requests for the game (9.3, S20, S21). `permissions` holds open permission requests (9.3), and `notices` holds the notifications of terminal sessions (section 10, with the restatement of S20, 10.3).

Later fields (the session and the denied rules) go into a file of their own, or need an approved change of S9.

The body sets one global table. S9 fixes its shape:

```lua
GnomishRelay_SlotData = {proto = 1, now = 1790211081, replies = {
{chat = "c1", id = 12, status = "working", text = ""},
{chat = "c1", id = 11, status = "done", text = "..."},
}}
```

**The key check.** A player with an old key sees no reply and no reason, because the bridge refuses each strip for its tag. So the bridge counts the strips with a bad tag since the last good relay strip. While the count is not 0, `Inbox.lua` ends with one more line after the table: `GnomishRelay_SlotData.badTags = 2`. The line holds only the global and a decimal number, so no outside text reaches it, and S9 still covers the table. Meanwhile, a message that the addon gives up on ends with "Not sent: your game and the desktop app don't match. On your desktop, run gnomish-relay setup, then type /reload." A bad tag has no app, so only the relay body counts it.

- `proto` and the pool sizes let the addon detect a mismatch (7.7).
- `replies` holds every record that the addon has not read, at most 30. Each `text` is at most 32 KB. The bridge cuts longer text and adds a note with the full length.
- A final reply stays in the body until a `read` flag names it. Then the bridge takes it out.
- When the body holds 30 records, the bridge refuses new messages until the next `read` flag. It does not mark a refused message as seen, so the addon sends it again: the strip shows again, and the outbox (7.5) keeps it.
- The `transport.qnt` model (14.2) checks these rules. Without them, a reply can drop out of the body before the addon reads it.
- String escapes follow one function in the `protocol` crate. The addon reads the file as Lua source, so the escape rules are part of the protocol.
- The bridge writes progress at most every 3 seconds, and final replies at once.
- If `LoadAddOn` returns `MISSING` or `DISABLED`, the addon reports "slots not installed".

#### 7.3.1 Reply blocks

Agent replies are Markdown. The game cannot parse Markdown safely, so the bridge renders it with `render_markdown` in `protocol` (`markdown.rs` and `inline.rs`).
The rendered text goes into the normal `text` field, so S9, S18, and S20 do not change.

- The bridge renders only the text of a `done` reply. Errors, lists, and user messages stay plain, except an error with bridge blocks (below).
- An attach reply (9.6) is `prompt\nanswer`. The bridge renders only the answer.
- The bridge blocks and the usage line of an attach reply go into the answer, after its marker, so the first line stays the prompt. Before a fix on 2026-09-30, the blocks replaced the whole reply, and the game showed the marker as the prompt and a branch block as the answer.
- The addon reads an attach reply that starts with the marker as an answer with no prompt. So a reply of an older desktop app shows its blocks, not boxes.
- The bridge history keeps the rendered text, so a restore shows the same blocks as the live reply.

**Format.** The text starts with the marker `ESC M 1` (`1B 4D 31`). Each block is one line: `\n`, a kind byte, then fields that each start with `US` (`1F`). A last `\n` ends the text.

| Kind | Block | Fields |
|---|---|---|
| `h` | Heading | level `1` to `3` (`####` and deeper show as `3`), text |
| `p` | Paragraph. Its lines join with a space. | text |
| `l` | List item. A line below it continues it. | level `0` to `4` (2 spaces of indent per level), the number or nothing for a bullet, text |
| `q` | Quote. Its lines join. | text |
| `c` | One line of a code fence (```` ``` ```` or `~~~`). A tab becomes 4 spaces. | text |
| `t` | Table row. The delimiter row (`\|---\|`) shows nothing. | `1` for the row above a delimiter row, else `0`, then one field per cell |
| `r` | Rule (`---`, `***`, `___`) | none |
| `u` | The run usage (9.10): one grey line below the reply. Only the bridge writes it, as the first block after the marker, so a cut never drops it. | the line, for example `1.2k in · 350 out · $0.04` |

**Text rules:**

- Every `|` of the agent text is doubled (S10). Inside a table cell, a `\|` is a `|` of the cell.
- Control bytes go, and a tab becomes a space. So ESC, US, and `\n` never come from the agent.
- Texts of `h`, `p`, `l`, and `q` go into SimpleHTML, so `<`, `>`, and `&` become `&lt;`, `&gt;`, and `&amp;`. Texts of `c` and `t` go into font strings and keep those bytes.
- Inline marks become WoW color codes: bold `ffd100`, italic `c0c8ff`, both `ffe680`, inline code `b8e0b8`, and link text `69b4ff`. A link shows only its text: WoW cannot open a browser.
- Only the renderer writes `|c` and `|r` codes. Colors never nest, and each one closes in its own field.
- A mark with no closing mark is text. So are `*` between spaces and `_` inside a word.
- The output is at most 16 times the input plus 4 bytes (S25). A bold text with many `*_*_` switches costs 12 bytes per input byte, so a bound of 10 is false.

**Blocks of the bridge** (9.11). The bridge puts its own blocks right after the marker and the usage line `u`, before the renderer blocks, so a cut of a long reply never takes them. Their kinds are upper-case letters, which the renderer never writes. The renderer drops every `\n` and `US` of the agent, so no agent text can make one. Each field loses its control characters, and every `|` is doubled (S10). These blocks go only into the body, never into the restore history (7.6). There, an error with blocks is its plain text with no marker, because the addon shows a restored error as plain text.

| Kind | Block | Fields |
|---|---|---|
| `B` | The branch of the chat folder | the branch (empty for a detached `HEAD`), `1` for an own branch else `0`, the start branch |
| `G` | The change summary of the run | files, lines added, lines removed |
| `F` | One changed file | the path, lines added, lines removed (both empty for a binary file), `A` for a new file, `D` for a removed one, else `M` |
| `M` | The files that do not show | their count |
| `T` | The test line | passed, failed, skipped |
| `C` | The CI line | passed, failed, running, the names of at most two failed checks, divided by `, ` |

A reply with bridge blocks and no agent text is the marker and the bridge blocks alone. An error reply with bridge blocks is rendered too: the marker, the bridge blocks, and the error text as a paragraph through the renderer, so its bytes get the same escapes. The addon then shows the error line and the blocks (13.1). An error text can hold agent text, for example the error of a Claude turn. So the bridge removes each ESC byte from every other error text, and only the bridge starts an error with the marker.

**Cuts.** The body cuts a text at 32 KB (S12), and the restore at 500 bytes (S18). A cut text has no last `\n`. The addon still shows its last line, with no color codes, and with no half code, half entity, or half character at its end.

S22 to S25 (14.1) prove the renderer for every input of at most 1 MiB. A reply is at most 256 KiB.
The fuzz target `markdown` checks the same shape, escapes, and size bound on the compiled code.

**Poll schedule after a send:** the addon loads a slot at 5, 10, 16, 24, 34, 46, 60, 80, 100, 130, 160, 200, 240, and 300 seconds, then every 60 seconds until the reply is done.
While a relay run works (a `working` record), the relay loads one every 15 seconds. So a permission popup, which waits for a poll, comes at most 15 seconds late, not 60. Activity shows "Checking again in 12s".
With no message pending, it loads one slot every 10 minutes, for the status light. With notifications on and a terminal session open, it loads one every 3 minutes, and every 60 seconds while a terminal turn runs (10.4).
A signal (7.4) makes the addon load a slot at once.

**Slot budget:** there are 1000 slots per UI session. A reply costs about one slot when signals work, and about four when they do not. A desktop request costs at most 24 more slots (6.6.3). A working run costs 4 slots a minute, so a UI session lasts about 4 hours of agent work, and "Reload soon" covers the rest. Notification polls cost 60 slots per hour of terminal work, and 20 per hour with an idle terminal session (10.4).
The window never shows the slot count. `/relay diag` shows it.
Below 20 free slots, the window shows "Reload soon to keep chatting." with a **Reload** button. Only a click on **Reload** reloads. In combat, the game allows no reload, so a click shows "Reload works after combat." in the red error text. Send never reloads: a reload from Enter took the game away for seconds with no warning.
`ReloadUI` needs a hardware event, and a click is one. The addon never reloads in combat.
The chat history is in the saved variables, so a `/reload` keeps it.

#### 7.3.2 The key addon

An addon app such as CurseForge replaces the whole addon folder at each update, so a key file inside `GnomishRelay` goes away. So the desktop app writes each strip key into an addon of its own, next to the slots (fixed on 2026-09-29, tests first).

| App | Key addon | Global | In the app addon |
|---|---|---|---|
| Relay | `GnomishRelay_Key` | `GnomishRelayKey` | `App.lua` names both. `KeyHandoff.lua` (shared) takes the key. |
| Timeways | `Timeways_Key` | `TimewaysKey` | The same, in the Timeways repo. |

- The key addon holds two files. `<name>.toc` has the `## Interface` list of 7.9, a grey `## Title` ("Gnomish Relay key (leave on)"), a `## Notes` line, `## LoadOnDemand: 1`, and `Key.lua`, with no `## Dependencies`. `Key.lua` is one line: `GnomishRelayKey = "<64 hex digits>"`. The desktop app writes it only from a key of 64 hex digits, with mode 0600, in a folder with mode 0700, and never through a link.
- The app addon lists `KeyHandoff.lua` right after `App.lua`, before each file that reads `ns.key`. `KeyHandoff.lua` calls `C_AddOns.EnableAddOn` and `C_AddOns.LoadAddOn` for the key addon. On the next line, it reads the global with `rawget` and sets it to nil with `rawset`. It keeps the key in `ns.key` only if the value is a string of 64 hex digits.
- The desktop app writes the key addon and the slots, never the relay addon `GnomishRelay`: players get it only from CurseForge (11.3). The CurseForge app manages only that folder, so an addon update never removes a key or a slot.
- **Three tries** (asked for on 2026-09-30, before any test in the real game). Nobody has checked that, in the Forever client, `LoadAddOn` of a load-on-demand addon works during the file load of another addon. So `KeyHandoff.Try` runs at the file load, in the app's `ADDON_LOADED`, and at `PLAYER_LOGIN`. A try runs only while the app has no key, and the first key wins. Each try reads and clears the global in the same call. The two later tries widen the exposure (see the end of "The exposure" below). The app has no key only if all three fail. `/relay diag` shows "Gnomish Relay: key loaded at <step>": `file load`, `ADDON_LOADED`, or `PLAYER_LOGIN`. With no key, `/relay diag` shows only the login line of the first-run window, which says what to do next.
- **Why load on demand, and not `## OptionalDeps`.** With `## OptionalDeps: GnomishRelay_Key`, WoW loads the key addon before the relay at login. But it then also loads when the relay is off or fails, and its global stays for the whole UI session. A load-on-demand addon runs only when code calls `LoadAddOn`, and only once in a UI session: a second `LoadAddOn` runs no file.
- **The exposure.** The global exists from the `Key.lua` of the key addon to the line after `LoadAddOn`. Only code that the load runs can read it then: at the file load, the `ADDON_LOADED` handlers of the addons that loaded before the relay. Such an addon can also call `LoadAddOn` for the key addon first and take the global, or put a metatable on `_G`. The relay then has no key and shows the first-run window. When the file-load try works, a later addon finds no global and cannot load the key addon again. This is the bound of the known leak of 6.5: an addon that loads first can already replace `string.char` or `tonumber` and read the key. The key is a check against programs outside the game, not against other addons (6.6.1), and a call to our handlers from any addon gets no more than a typed message (6.1). So the file-load try adds no reader that the old `Key.lua` did not have. When that try fails, the later tries give every addon a chance to read the key. An addon that loads after the relay can call `LoadAddOn` for the key addon before the relay tries again. At `PLAYER_LOGIN`, every addon has loaded, and the `ADDON_LOADED` handler of each one runs inside `LoadAddOn` and can read the global. We accept this, because the key is no defense against addons (6.6.1). When the first test in the real game shows which try works, we remove the others.
- **With no key.** The relay starts no transport. At login, once per UI session, it prints one line and shows the first-run window. `/relay`, both key bindings, and `/ai` open the window again. It looks like the Timeways setup window: a rock background with a dialog border, the title "Gnomish Relay Setup", a close button, and a parchment sheet. The sheet holds a heading, one sentence, the install line in an edit box that keeps its text (a click selects all of it), "Click a line and press Ctrl+C to copy it (Cmd+C on a Mac).", and "Run it on your computer. Then restart WoW.". **Close** is below the sheet. A Mac client gets the Terminal line. A Windows client gets the PowerShell line and the Linux line, because Linux players run the Windows client under Wine.
  - With no key in any earlier UI session: "Gnomish Relay needs its desktop app. Get it at github.com/eserilev/gnomish-relay, then restart WoW."
  - With a key in an earlier UI session (`hadKey` in the saved variables), the key addon is new since the game launched: "Gnomish Relay: restart WoW to finish setup. If this shows again, run gnomish-relay setup on your desktop." The window then shows no install line.
- **Migration.** Setup and each bridge start delete `Key.lua` in the real folder of `GnomishRelay`, also in a developer's linked folder (16). WoW finds the new key addon only at launch. So setup says "Restart WoW", and `update` says "Restart WoW to finish." when the key addon was missing before.
- **The addon folder stays as it is.** An older setup copied the relay addon into `GnomishRelay`. That folder stays: the desktop app deletes no file in it except the old `Key.lua`. Setup and `status` check the addon version (7.7), and name the fix when it is out of range.
- **Timeways moves later.** While the TOC in the Timeways folder lists `Key.lua`, the desktop app also writes that file, in the old format. Otherwise, the desktop app deletes it. `// TODO: remove when every Timeways release reads Timeways_Key`.

### 7.4 Signals

**Status: signals do not work on the tested client (rule 4 fails).** The addon polls slots on the schedule in 7.3. This section stays for clients where the self-test passes. A replacement signal through font files (as in `wow-forever-codex`) is an open question.

Each signal is a `.wav` file. The bridge makes it empty (off) or writes a valid silent sound (on): 8 kHz, 8-bit, mono, 80 samples.
Every 2 seconds, the addon checks a signal with `PlaySoundFile` on a muted channel.

| Family | Files | Meaning |
|---|---|---|
| `ack/NNN` | 200 | The bridge decoded message NNN. The addon takes it off the strip. |
| `sig/NNN` | 200 | The reply for message NNN is ready. |
| `act/NNN/kk` | 200 × 60 | Heartbeat kk for message NNN. The agent still works. |
| `presence/kkkk` | 2000 | The bridge is alive. One every 30 seconds. |
| `note/kkkk` | 2000 | A notification or a permission request waits (section 10, 9.3). |
| `ctl/empty`, `ctl/valid` | 2 | The self-test at login. |

- `NNN = ((id − 1) mod 200) + 1`.
- Rule 5 in 7.2 makes each signal one-shot until the game restarts. After the message ids wrap past 200, a signal can already be valid. The addon treats an unexpected "valid" as unreliable and uses the poll schedule.
- `presence` and `note` are counters in `state.json`. The bridge keeps 50 files ahead of each counter empty. `presence` wraps after about 16 hours.
- **Self-test:** at login, the addon plays `ctl/empty` and `ctl/valid`. If `ctl/empty` plays, or `ctl/valid` does not, signals are off for this session, and the addon uses slot polls only.
- **Status light:** with presence signals, 90 seconds of silence means "stale", and 300 seconds means "down". Without them, the addon reads the `now` of each body that it loads. The bridge writes a body at least every 60 seconds. So a body older than 150 seconds at its poll means "offline" at once, and a body 90 to 150 seconds old missed a heartbeat: the light says "slow". So a player who sends a message learns within 5 seconds that the bridge stopped. With no body for 12 minutes, the light is offline too.

Total file count for slots and signals: about 17,000.

### 7.5 Reload fallback

The addon uses the reload fallback when the strip gets no acknowledgment, the pool is empty, or the slots are missing.

1. The addon writes the signed frame of the message into `outbox` in its saved variables (6.6.1). The bridge checks it as a strip: tag, time, and replay store. A frame counts only if the key of the app whose saved variables hold it signed it (9.7, decision 3). The file keeps old frames across reloads. So at each read, the log gets one count line for the frames more than 5 minutes old, and one for the frames with a future time. Every other skipped frame gets its own line.
2. The window shows "1 message is waiting. Reload to send it." with a **Reload** button. `ReloadUI` needs a hardware event, and a click is one. In combat, the button does nothing.
3. At reload, WoW writes the saved variables file.
4. The bridge watches `WTF/Account/<ACCOUNT>/SavedVariables/GnomishRelay.lua` (modification time, every 250 ms).
5. The bridge writes the reply into `GnomishRelay/Inbox.lua`. The main addon reads it at the next reload.

After each `/reload`, the addon shows the strip again for every sent message that has no reply and is not in the outbox.
The saved variables also carry the `read` and `restored` state, so the bridge reads them from the file too.

### 7.6 Restore after a saved-data wipe

The beta client sometimes wipes addon saved data. The addon then makes a new token.
When a hello comes from an unknown token and the bridge already knows another token, the bridge writes a restore bundle for the new token.
The bundle goes into `Restore.lua` in each slot of the window, next to the body, so the body keeps its own 1 MiB bound (S12).
The bundle stays in each publish until a strip from that token has the `restored` flag. The flag ends the restore and retires no token.
The addon applies a bundle only once. It merges the chats by chat id, so a second copy of the bundle changes nothing.

**Two accounts, or a wipe.** Two WoW accounts on one computer also have two tokens, and they can play at once. A strip carries only the token, so a hello from the second account looks the same as a hello after a wipe. Only the saved variables show the account: WoW keeps them per account in `WTF/Account/<ACCOUNT>/SavedVariables/GnomishRelay.lua`, and the file holds the token (`["token"]`, one tab deep). So the bridge decides by the account folder:

- In `state.json`, the bridge keeps the account folder of each token that it read in a saved variables file.
- **The rule:** a new token in the file of a folder that held another token is a wipe. The older token of that folder retires: its records leave the slot body, and its window goes (7.3). A run of a retired token that ends later goes only into the history.
- A token in another folder is another account. Tokens of two folders never retire each other.
- WoW writes the file only at a `/reload`, a logout, or an exit. So after a wipe, the old token retires at the next `/reload` or logout of that account, not at once. Until then, its records stay in the body. They are the records that nobody read before the wipe, so they are few.
- A token that the bridge never saw in a file never retires, for example a second account before its first `/reload` or logout.
- The restore does not wait for the file, because a player after a wipe wants the chats back now. So the first hello of a second account also gets a restore, and that account shows the chats of the first one as a copy. It is the same person on the same computer. Both accounts can then send to such a chat, and each account sees only its own messages and their replies.

The bundle holds the 16 chats with the latest activity, and the last 10 messages of each (S18).
Each message is cut to 500 bytes, at a character boundary. The file is at most 512 KiB (S19).
The bridge keeps this history in `state.json`. The full transcripts come later (8.3).

### 7.7 Versioning

- The strip has a version byte. The bridge drops and logs frames with an unknown version.
- Each slot body carries `proto` and the pool sizes. Each report carries the addon protocol version (`ver=<n>`).
- The bridge keeps a range of addon versions for each app. `version_fit` in `crates/protocol/src/version.rs` says `Supported`, `TooOld`, or `TooNew`. S30 proves that it never fails and matches the range exactly (14.1). Today both ranges are 1 to 1. The bridge logs each new version that an addon reports.
- While the last version of an app is out of its range, each message of that app counts as seen, gets one error reply in that app's body, and never reaches an agent or the story program:

| App | Too old | Too new |
|---|---|---|
| Relay | "Update Gnomish Relay in the CurseForge app, then restart WoW." | "Update the desktop app: run gnomish-relay update." |
| Timeways | "Update Timeways." | "Update the desktop app: run gnomish-relay update." |

- Setup and `status` also check the relay addon on disk, with no game running. They read `version` of `ns.App` in `GnomishRelay/App.lua`, check it with `version_fit`, and show the Relay messages of the table above. With no `App.lua`, or no `version` in it, the addon counts as too old. The desktop app never copies an addon over it (11.3).
- With no `ver=` yet, for example just after a bridge restart, the version counts as supported, so a restart refuses no good message.
- `ver=` is the version of each app: `Health.lua` of the shared transport sends `ns.App.version`, from the app's `App.lua`. Each app changes on its own: the coding flags of the relay, and the batch lines of Timeways (9.8). The Timeways `App.lua` needs `version` when it copies this `Health.lua`.
- On a mismatch, the addon shows "bridge and addon versions do not match" and stops sending.
- Pool sizes live in one place: the `protocol` crate. The setup step writes them into the addon.
- The release version (for example `0.2.0`) is not a protocol version. It lives in two places: `version` of `[workspace.package]` in `Cargo.toml`, which every crate takes, and `## Version` in `GnomishRelay.toc`. The ranges above use only protocol versions, so a release with no protocol change keeps them at 1 to 1.

### 7.8 Design for breakage

The transport rests on client behaviors that Blizzard never promised: an addon can call `Screenshot()`, and a load-on-demand addon reads its files fresh.
A client patch can break either one. The design makes such a patch cost a day of work, not the project.

**One interface per direction.** The core never knows which channel carries a message.

| Direction | Interface | Channels, in order |
|---|---|---|
| Out (game to bridge) | Addon `Out.Send(frame)`, bridge `trait FrameSource` | Strip by `Screenshot()`, reload outbox (7.5). Planned: window capture (7.8.1). |
| In (bridge to game) | Addon `In.Poll()`, bridge `trait Publisher` | Slots (7.3), fonts (17), reload inbox (7.5) |

- The protocol core, the model, and the proofs work on frames and records. A channel change does not touch them.
- A new channel is one new module on each side, with its own tests. Nothing else changes.

**Self-test and health report.**

- At login, the addon makes sure that each client function it needs exists (`Health.Required`). If one is missing, it shows one line, "Gnomish Relay is off: this version of the game has no <name>. On your desktop, run gnomish-relay update.", and starts nothing.
- The first hello strip and the first poll test the two channels. `SCREENSHOT_SUCCEEDED` or `SCREENSHOT_FAILED` gives the result of each shot, and `LoadAddOn` of each slot.
- Each strip carries the client build and the last result of each channel: `build=<number>`, `out=shot|fail`, and `in=slots|missing`.
- When a channel starts to fail, the addon shows one line: "Gnomish Relay: can't take screenshots. Free up disk space and check the Screenshots folder, then type /reload." or "Gnomish Relay: some addon files are missing. Close the game, then run gnomish-relay install." The window shows the same state in the bridge light.
- `/relay diag` shows the build and the last success of each channel. `gnomish-relay doctor` comes later.
- Today each direction has one channel. A move to the next channel of the table comes with the second channel.

**Builds.**

- In `state.json`, the bridge keeps the last client build whose screenshots and slots both work, and logs each new one.
- Later: the window shows "New game version: checking the relay" until the self-test passes.
- A known break goes into a table in the bridge, so setup can name the channel that works on each build.

**API compliance.** The addon calls only the API of the real Forever client (1.60.1, the Mainline UI code).

- `scripts/wow-api.sh` reads two sources at pinned commits: the `forever` branch of Gethe/wow-ui-source (Blizzard's UI code) and of Ketho/BlizzardInterfaceResources (the API that the client reports).
- It checks every WoW name that the addon, `wow.yml`, or the fake game uses. It stops on a name that the client lacks or has only in a `Blizzard_Deprecated` addon, and on a registered event that the client lacks. It also checks that the `## Interface` list of the TOC holds the build number.
- It writes `addon/tests/api.lua`: the used globals, every widget type with its methods, and each used template with its mixin methods and child keys.
- It writes `addon/tests/api-signatures.lua` from the client's generated API docs (`Blizzard_APIDocumentationGenerated`). For each used function, each widget method with a called name, and each registered event, it keeps the arguments, returns, payload, and every flag: `SecretArguments`, `SecretReturns`, `SecretWhen...`, `HasRestrictions`, `IsProtectedFunction`, and the others. A used function with no doc entry goes into an `undocumented` list, so a new doc entry also shows.
- A client patch can keep a name and change what it takes, returns, or hides behind a secret value. The diff of `api-signatures.lua` then names the change.
- Selene allows only the globals in `wow.yml`. The fake game refuses every method and child key that the real kind and template of an object lack. It does not check argument counts: the docs mark as required some arguments that the client accepts as missing, for example the last four of `SetPoint`.
- CI runs the script at the pinned commits and fails if `api.lua` or `api-signatures.lua` changes. A nightly job runs it at the newest commits with the addon tests, and opens an issue when the API that the addon uses changes. A new build with no such change differs only in the build lines, and the nightly ignores those (`git diff -I`).
- Other addon repos run the same script with their own paths: `wow-api.sh --addon <folder> --lint <wow.yml> --api <file> --signatures <file>`. With no path, it checks this addon.

#### 7.8.1 Window capture: the fallback for `Screenshot()`

**Status: planned, not built** (decided by the user on 2026-10-03). `Screenshot()` works today, so nobody builds this until it breaks. Build it when the self-test, the health line, or the nightly API gate shows that `Screenshot()` is blocked or restricted for addons (7.8). The reload outbox (7.5) is no real fallback: a reload for each message is worse than a switch to a terminal.

**The idea.** The addon still draws the signed strip. The desktop app reads the pixels of the strip from the WoW window itself, as screen-sharing programs do. The strip format, the tag, the reader, and the replay store stay the same. Only the `FrameSource` changes (one new module on each side, as above). Blizzard cannot block it with an addon patch, because the addon only draws pixels. Goal 5 holds: no game memory, no code in the game, no keys to the game.

**Shared design.**

- A new frame source reads only the top-left corner of the WoW window, as large as the largest strip, about 10 times a second. It never stores an image.
- The bridge says `capture=on` in the slot body when its capture reads strips. The addon then makes no `Screenshot()` call, and shows each strip for about 0.5 s. With no `capture=on`, the addon takes screenshots as today.
- The retry schedule stays. After repeated failures with capture, the addon goes back to `Screenshot()`.
- Side effects: no "Screen captured" text, no PNG files, and less delay than a screenshot.
- Limits: a minimized window draws nothing, as today. Capture asks for 8-bit color, so an HDR screen gives the same colors.

**Each OS.**

| OS | Capture | Permission |
|---|---|---|
| Windows | Windows Graphics Capture of the WoW window. `PrintWindow` as the fallback. | None. Windows 10 shows a yellow border, and Windows 11 can turn it off. |
| Windows with WSL2 (11.5) | The Linux bridge cannot see Windows windows, so the Windows `gnomish-relay.exe` captures and passes the bytes to the bridge. | None |
| macOS | ScreenCaptureKit on the WoW window, cropped to the corner. macOS 12.3 or later. | Screen Recording, asked once by the system. Setup says that the app reads only one corner of the WoW window. |
| Linux | The first path that works, in this order: | |
| — X11 session, or Wine in XWayland on Wayland (the default of Wine and Proton) | Find the window by its class, and read the corner with XShm (`x11rb`, pure Rust). | None. The spike checks it under XWayland on GNOME. |
| — Wine as a native Wayland window | The ScreenCast portal (`zbus`) and PipeWire. The player picks the WoW window once, and a restore token keeps the choice on GNOME and KDE. | Once, or at each start on wlroots desktops |
| — gamescope (Steam Deck, Bazzite) | The PipeWire stream of gamescope, with no portal. | None |
| — none of these | `Screenshot()`, as today. | |

**Linux details.** The bridge runs as a user service, so it needs `DISPLAY` and `XAUTHORITY`, or `WAYLAND_DISPLAY`. Setup checks them and writes them into the service file when the session does not pass them. The bridge loads `libpipewire` only when it is there, so the one Linux binary works on every distro.

**The order of work, when it starts:** a spike on Linux under XWayland, then Windows, macOS, and WSL2. Each step is one capture module and its tests. The bridge tests use the fake capture.

### 7.9 Supported clients

Gnomish Relay supports two WoW clients. `crates/bridge/src/wow_client.rs` lists them, in this order.

| Client | Folder in `World of Warcraft` | `## Interface` | Measured build |
|---|---|---|---|
| WoW: Forever | `_classic_beta_` | `16001` | 1.60.1 |
| WoW Classic: TBC Anniversary | `_anniversary_` | `20506` | 2.5.6.69795, on the 12.0.7 engine |

- **One addon for all clients.** Each TOC lists every number: `## Interface: 16001, 20506`. The game loads an addon when one number matches. This applies to the relay addon, the key addons (7.3.2), and the slots (7.3). So one CurseForge file serves each client, and the desktop app writes the same files for each one.
- **Every client of one install at once** (asked for by the user on 2026-10-02, after 0.5.0 told players to switch with `setup --wow`). `[wow] path` names one client folder. The desktop app serves it, and each sibling client folder that has the relay or the Timeways addon (`wow_client::served_games`). A client with no addon gets no key addon and no slots, so its AddOns list stays clean. Battle.net puts every client in one `World of Warcraft` folder, so a player with Forever and TBC Anniversary switches nothing. A folder with another name is served alone. Setup still takes one folder: with more than one install, the one played last (11.3).
  - Each served game has its own `GameFolders`: `AddOns`, `Screenshots`, and `WTF/Account`. The bridge watches the screenshots and saved variables of each game, and each publish writes the slot windows into each `AddOns` folder. Each game loads only its own slots, and each token keeps its own window (7.3), as two accounts in one game already do. A failed write to one game leaves the others.
  - The account name is `<game folder>/<account folder>`, for example `_anniversary_/ACCOUNT1`. One Battle.net account has an account folder with the same name in each client, and a new token in the same account is a wipe (7.6). Without the game in the name, a login in TBC Anniversary retired the chats of Forever. An account name in a state from before 0.5.1 has no game, so it matches nothing. At worst, one old token keeps its window until the 3 newest tokens push it out.
  - At each start, the desktop app writes the key addon and the missing slots into each served game. So a client installed after setup works after a game restart.
  - **A client that gets the addon later** (asked for by the user on 2026-10-03). Once a minute, while no run is in progress, the bridge asks `served_games` again (`game_watch.rs`). When a client folder now has the addon, the bridge starts `gnomish-relay restart` as its own process, once, and logs the folder. The new bridge serves that game from its start. Why a restart and no change in place: the sandbox gets the private paths of the games at the start (6.6.4), so a game added in place leaves its key and saved chats open to agents. The player restarts the game once, because WoW finds the new slots only at launch. The sandbox hides the private paths of every served game (6.6.3).
  - `status` shows the addon line of each game that has the addon, with the client name. Auto-update follows the newest addon version of any served game, because the CurseForge app updates each client on its own. `install` and `say` write into each game.
  - Tests: `two_games.rs` (`a_strip_in_the_second_game_comes_back_in_the_slots_of_both_games`, `the_same_account_name_in_two_games_keeps_the_chats_of_both`), `wow_client.rs`, `game_folders.rs`, `start.rs` (`the_private_files_of_every_served_game_are_hidden`), `auto_update.rs`, and `update.rs`.
- **TBC Anniversary passes the rules of 7.2.** The self-test (14.3.1) ran on 2026-10-02. All six line modes read clean, and `Screenshot()` works from an addon. A load-on-demand addon whose file changed after launch loads the new content, so rule 2 holds.
- **`FontString:SetFont` with a missing file.** Forever returns `false`. TBC Anniversary raises "Invalid font asset". The fixture keeps it as `missing: "raises"`, and the fake game raises too. So the addon calls `SetFont` inside `pcall` for a font that can be missing (the mono font after an update).
- **Two "Screen captured" frames.** TBC Anniversary has two frames named `ActionStatus`: the Classic UI one, a child of `WorldFrame`, and the one of `Blizzard_ActionStatus`, under `UIParent`. The global name points at the `WorldFrame` one (measured on 2026-10-02), but the other one shows "Screen captured". So at load, `Strip.lua` walks every frame with `EnumerateFrames`, hooks each one named `ActionStatus`, and hides the text of our shots on each. The fake game models the second frame (`AddSecondActionStatus`).
- **Atlases.** TBC Anniversary lacks the atlases `minimap-genericevent-hornicon` (the bell, 10.4) and `QuestBG-Parchment` (the first-run window, 7.3.2). `SetAtlas` with a missing name draws nothing and raises no error. So `Atlases.lua` asks `C_Texture.GetAtlasInfo` first, and draws a stand-in that every client has: the atlas `communities-icon-notification` for the bell, and the file `Interface\QuestFrame\QuestBG` for the parchment. The fake game takes a list of missing atlases.
- **Fixtures.** Each client has its own fixtures (14.3.1), named by the client. The fake game of the tests stays Forever. The golden vectors of every client decode in `golden.rs`.
- **The API gate (7.8) runs once for each client.** `scripts/wow-api.sh --client anniversary` reads the `classic_anniversary` branch of both sources at pinned commits, and writes `addon/tests/api-anniversary.lua` and `api-signatures-anniversary.lua`. The TOC check looks for the client number in the `## Interface` list. CI, `check-all.sh`, and the nightly job run the gate for both clients. The fake game of the tests loads only the Forever files. The gate does not check atlas names: they are data, not API names.
- **A key addon for another WoW version.** An older desktop app writes the key addon with the number of only one client. The other client refuses it as out of date, and `LoadAddOn` gives the reason `INTERFACE_VERSION`. `KeyHandoff` keeps that reason, and the first-run window then says "Update the desktop app", with the line "Gnomish Relay: the desktop app is out of date. On your desktop, run gnomish-relay update, then restart WoW." It shows no install line. Before this, the window wrongly said that the desktop app was missing (seen on 2026-10-02). Test: `a_key_addon_for_another_wow_version_says_to_update_the_desktop_app`.
- **CurseForge game versions.** The packager tags the upload with the game version of each number in `## Interface`: `16xxx` is WoW: Forever, and `20xxx` is Burning Crusade Classic. When CurseForge lacks a version, the packager only warns and tags an older one. So the first job of `release.yml`, the version job, runs `scripts/curseforge_versions.py`. It reads the CurseForge game versions and stops the whole release when the version of a client is missing. Nothing is published yet at that point, so a release is never half out, with binaries on GitHub and no addon on CurseForge.
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
 │   spool ◄── terminal hooks (10)      │
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
  fuzz/                 the fuzz targets (14.3)
  proofs/               Lean project with the Aeneas output and the proofs
  models/               Quint model of the transport
  tests/vectors/        golden strip images
  SPEC.md
```

Planned, not built: a crate `agents/` for the `Agent` trait and the backends of 9.2, now in `bridge/`. The hook command (section 10) is a subcommand of the one binary (10.1), with no crate.

### 8.2 Bridge main loop

1. Watch the Screenshots folder for a new file.
2. If a new strip is visible, decode it, check the MAC, and drop duplicates.
3. Apply the policy (6.2). If a record fails the policy, publish an error reply for it.
4. Put the record in the FIFO queue of its chat.
5. Start a run when fewer than `max_parallel_runs` runs are active.
6. Publish progress and the final reply. Raise the signals.

Each chat has a FIFO queue. A second message to a busy chat waits. It never replaces the first.
(`wow-claude` keeps one queued job per chat, so a second message replaces the first. Do not copy this.)

**The limit on parallel runs.** Runs cost memory, CPU, and money, so at most `max_parallel_runs` are active (12, default 3).

- A run is a message, or an attach (9.6) with a run in progress. A list of sessions, folders, or settings never counts and never waits: it is short and starts no agent turn.
- A message over the limit waits in its chat queue. When a run ends, the oldest waiting message of all chats starts. `state.json` keeps the waiting messages oldest first, so a restart keeps the order.
- A message that waits for the limit, not for its own chat, shows one bridge progress line: "Waiting: 3 other chats are running", plus ", 1 ahead of this one" when older messages wait too. Only the bridge writes "Waiting:" lines: `Activity::step` puts "agent: " in front of such an agent line, as for "Level:" (9.3).
- The addon shows the line on the cast bar of the Activity column, grey and still, as for a waiting popup (13.1), not as a step row.
- Stop ends a waiting message as before, and its line goes.
- A lower `max_parallel_runs` applies at the next bridge start. Runs in progress go on, and new runs wait until fewer than the new limit are active.

### 8.3 State

The bridge keeps its state in JSON files in the OS data folder:

- `state.json`: the replay store, unread records, waiting messages, the slot window and account folder of each token, the tokens, and the restore history (7.6).
- `timeways/state.json`: the Timeways lane (9.7), only with a Timeways key. Later also agent session IDs per chat, the folder of each session, and signal counters.
- `timeways/story/`: the story program folder (9.8). Only the story program writes there. The bridge never reads it.
- `usage.json`: tokens and cost of each of the last 31 days (9.10).
- `state.json` also keeps the full-auto chats: chat id and real folder of each approval (9.3, "Full-auto for one chat").
- Planned, not built: `transcripts.json`, with every prompt and reply per chat: 200 messages per chat, 4000 characters each. Today `state.json` keeps only the short history of the restore bundle (7.6).

Rules:

- Each state file write is atomic: write a temp file, then rename.
- A run starts only after `state.json` marks its message as seen, so a crash cannot run a message twice.
- A run in progress when the bridge stops ends as an error after the restart. It never runs again: it can have changed files already.
- A damaged `state.json` stops the bridge at start. A fresh start forgets which messages ran.
- The rate limiter is not in the state. A restart gives a fresh minute.
- Waiting jobs are keyed by chat and message id. A second token with the same pair waits until the first job leaves the queue.
- Sessions are keyed by chat id alone, so they survive a saved-data wipe.
- Dedup keeps the last 1000 ids per token. Tokens unseen for 30 days are pruned.
- Folders compare by exact path. Linux paths are case sensitive. (`wow-claude` lowercases paths. Do not copy this.)

### 8.4 Only one bridge

Two bridges fight over the screen and the slot files.
At start, the bridge takes an OS advisory lock on `bridge.lock` in the data folder (`File::try_lock` of the standard library). The OS releases it when the process stops, also after a crash.
If the lock is taken, the bridge stops with an error that names the other bridge process.
The bridge writes its process id into `bridge.pid`, a separate file because Windows does not let another process read a locked file.

### 8.5 Logs

Asked for by the user on 2026-09-30. One bug took more than 5 commands to find: the log did not show the run folder, the full command of a desktop request, or the steps of one message.

**One facade.** All code logs through `run::log(line)`, backed by the `tracing` crate. `run` (the bridge loop) installs the subscriber at start. With no subscriber, for example in `setup`, `log` writes the old line to stderr.

**Spans.** Each event inside a span carries the span fields:

- **A message span** for each relay-lane job that runs an agent or a git action: `chat`, `message_id`, `agent`, `permission`, and `folder`. `folder` is the real run folder. It changes when the folder resolves, when a folder request gets its answer, and when the chat gets its own branch folder (9.11). The run thread, the dialog thread of a desktop request, and the main loop all enter this span for the events of that message: start, game question, desktop request, notice, agent end, reply written.
- **A command span** around each game question and each desktop request of a tool call: `command`, the full command of a shell call.
- **A request span** for each desktop request: `request` (its id) and `kind`.

**The command field.** It is the raw command of the classifier input (6.6.3), not the popup text. The bridge cuts it to 500 characters, removes control characters, and replaces possible secrets with `***`:

- the value of an assignment word with no dash, such as `TOKEN=***`;
- the value of a flag whose name holds `key`, `token`, `secret`, `password`, `passwd`, or `auth`, in the same word (`--password=***`) or the next (`--password ***`), and the word after `Bearer`;
- a word that starts like a known token: `sk-`, `ghp_`, `gho_`, `ghs_`, `github_pat_`, `xoxb-`, `xoxp-`, `AKIA`, `glpat-`.

**Outputs.**

- **stderr**: journald under the systemd service (11.3), else `bridge.log`. The line keeps its old form, `<unix seconds> <line>`, with the escape of 6.2, rule 15. Span fields follow as ` key=value`, outer span first. A value with a space, a quote, or nothing in it is in double quotes. A plain `log` line with no span is exactly the old line.
- **A JSON-lines file**, `logs/bridge.jsonl` in the data folder. One object per event: `time` (ISO 8601 in UTC), `unix`, `level`, `line`, and each span field as a string. Mode 0600, never opened through a link. Before a write takes the file over 5 MB, the bridge renames it to `bridge.jsonl.1`, `.1` to `.2`, up to `.4`, and starts a new file. So at most 5 files, 25 MB in all.
- **The level filter.** `RUST_LOG` sets it, in the syntax of `tracing_subscriber::filter::Targets`, for example `debug` or `info,bridge=debug`. Default `info`. A value that does not parse gives `info` and one log line.
- `bridge.log` stays for a start with no service and for command details, such as a failed download (11.4). Only `run` writes the JSON log, so the JSON file does not replace it.

**Privacy.** The log never holds message text, agent replies, file contents, keys, tokens, or environment variable values. Commands, folder paths, agent names, and request ids are fine: they belong to the user, and the log never leaves the user's machine. A summary of 6.6.3 still never holds an argument. Only the `command` field holds arguments, with secrets hidden as above.

**`gnomish-relay report`.** It writes one file, `reports/report-<unix seconds>.txt` in the data folder, mode 0600, and uploads nothing. The file holds:

- the desktop app version, the OS, and the CPU architecture;
- the output of `gnomish-relay status`;
- `config.toml`, parsed and rewritten as TOML with no comments. A string value is `(removed)` if its key holds `key`, `token`, `secret`, `password`, or `auth`, or if it starts like a known token. A config that does not parse gives one line and no content;
- the last hour of lines from `logs/bridge.jsonl*`, oldest first;
- the last hour of lines from `bridge.log`.

The home folder shows as `~` everywhere in the file. The file never holds `strip.key`, `timeways.key`, or any other config folder file. The command prints the file path and one line: "To report a bug, attach this file to a new issue: https://github.com/eserilev/gnomish-relay/issues".

**Tests** (`crates/bridge/src/logging.rs`, `crates/bridge/src/report.rs`, and `crates/bridge/tests/report.rs`):

- `a_message_span_carries_chat_message_id_and_folder`
- `a_desktop_request_event_carries_its_full_command`
- `the_json_file_rotates_at_its_size_limit`
- `the_report_holds_no_key_or_token`
- `the_journald_line_keeps_its_old_form`
- `a_command_hides_its_secret_values`

## 9. Agents

### 9.1 The Agent trait

Today the trait has one call. It runs one message to its final reply:

```rust
trait Agent: Send + Sync {
    fn run(&self, job: &Job) -> Result<String, String>;
}
```

`Job` carries the chat, the folder after the policy check, the level after the ceiling (S6), and the text.
Each run starts a new agent process and stops it at the end of the turn:

- `acp`: opens a session, sets the mode of the level, sends the prompt.
- `claude`: `claude -p` with the mode of the level, then the prompt.
- `codex`: `codex app-server`, a thread with the sandbox and approval policy of the level, one turn.
- `command`: the harness inside the run sandbox. The message goes in, and the output is the reply.

Details:

- **Resume.** `state.json` keeps the agent session of each chat, with its agent and folder. The next message resumes it, unless it has the `n` flag or the agent or folder changed. The client uses `session/resume` if offered, else `session/load`. The history that `session/load` replays stays out of the reply. If neither works, the run opens a new session, and the reply starts with "(Started a new session: the old one couldn't be resumed.)".
- **Later: continue a terminal session.** A new chat can take a Claude or other agent session that runs in a terminal. The bridge lists recent sessions of each agent (`session/list`, where offered), and the chat resumes the one you pick. The terminal window does not show game messages live: no agent lets another program type into its open window. `claude --resume` shows them later.
- **Stop.** Stop in the game ends the waiting messages of the chat and signals the run in progress. The client sends `session/cancel` (`claude`: an `interrupt` control request, `codex`: `turn/interrupt`). It answers every open permission request with "cancelled" (`claude`: a deny, `codex`: `cancel`). After 10 seconds for the agent to end the turn, it kills the process. The reply is "Stopped.", and the session stays for the next message. A Stop before the prompt ends the run at once.
- **The timeout** (`timeout_minutes`) ends the turn the same way, but kills after 5 seconds. The reply is "Timed out.". Why the wait: Claude reports the turn cost only in its last message, so a killed run adds nothing to the daily cost (9.10).

Next, the trait gets events for progress and for permission requests from the game (9.3). Those need new slot body fields, so they wait for an approved S9 statement.

### 9.2 Backends

**The goal: one generic backend for any LLM coding harness.** Both parts exist:

- `acp` runs any harness that speaks ACP.
- `command` runs a command-line-only harness inside the sandbox.

`claude` and `codex` exist too. Most players have these two, so the bridge speaks their own protocols, with no Node.

| Backend | How it works | Progress | Live permissions | Allow & retry |
|---|---|---|---|---|
| `acp` (main) | Agent Client Protocol: JSON-RPC over stdin and stdout. The bridge is the client. | Yes | Yes | Not necessary |
| `claude` | `claude -p` with stream-json on stdin and stdout, and `--permission-prompt-tool stdio`. No Node. | Yes | Yes | Not necessary |
| `codex` | `codex app-server`: JSON-RPC over stdin and stdout, with approval requests. No Node. | Yes | Yes | Not necessary |
| `command` | An argument template: message in, output out. The whole harness runs in the sandbox. | Its output lines | No | No. The level picks the walls. |

**Support levels.** Any agent with a command line runs. Its protection depends on what the bridge sees:

| Level | Connection | What the classifier sees | Examples |
|---|---|---|---|
| Full | ACP, `claude`, `codex`, or a tool-call hook | Every tool call that needs an answer, before it runs | Gemini CLI, Claude, Codex, any ACP agent |
| Sandbox only | `command` | Nothing. The sandbox holds the whole harness. | Aider, `llm`, a script |

- A Full agent runs in its "ask for everything" mode, and the classifier answers most questions. In a looser mode the agent acts without asking, and the classifier never sees the action.
- `command` needs the sandbox. With none, the bridge does not start it (see "A harness with only a command line"). There is no "Trusted" level: with no sandbox and no classifier, nothing guards the run.
- An ACP agent needs one config line. An agent with a hook system needs a small hook command. Every other CLI agent uses `command`.

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

Agents through an adapter. Setup uses the native backend for both, because it needs only the agent program:

- Claude Code: the `claude-agent-acp` adapter (formerly `claude-code-acp`), which needs Node. The `claude` backend needs only the `claude` program.
- Codex: the `codex-acp` adapter, now in the `agentclientprotocol` organization. It starts `codex app-server` itself. The `codex` backend needs only the `codex` program. Codex has no ACP mode of its own (issue openai/codex#9085 is open).

The bridge speaks ACP protocol version 1 in `crates/bridge/src/acp.rs`, with no crate: JSON-RPC 2.0, one message per line.
The `agent-client-protocol` crate needs an async runtime, and the bridge needs only a few messages. Version 2 of the schema is still an alpha.

The agent process is untrusted:

- It gets only `PATH`, `HOME`, `LANG`, `TERM`, `USER`, the temp and Windows profile variables, the `env` list of its entry, and `GNOMISH_RELAY_JOB=1`.
- Lines from it are at most 8 MiB, the reply at most 256 KiB. A non-JSON line ends the run.
- The run ends at `timeout_minutes` (default 30), as in 9.1: the bridge asks the agent to end the turn, and kills it 5 seconds later.
- The bridge declares no `fs` and no `terminal` capability. It answers every other agent request with "method not found".
- If the config names a mode for the level that the agent does not offer, the run stops. With no mode, the agent runs at its own default, which can be more open.
- The gate (6.6.3, 9.3) answers each permission request. With nobody in the game, a question refuses the call. The reply then ends with "Not allowed from the game:" and the calls that a rule, the desktop, or no answer refused.

**Claude Code with no adapter (`kind = "claude"`).** The bridge speaks the stream-json protocol of `claude -p` in `crates/bridge/src/claude.rs`. Checked on Claude Code 2.1.282.

- The command, in the chat folder: `claude -p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --permission-mode <mode>`. Plus `--resume <id>` for the chat session, and the game-run flags: `--setting-sources "" --strict-mcp-config --settings <json>` (6.6.4), and `--append-system-prompt` with the summary note and the sandbox line (13.1, "Summary first"). The `command` of the entry comes first, so it can add flags.
- The bridge first sends the `initialize` control request, as the Claude Agent SDK does, and waits for the answer. Then it sends the prompt as one `user` message.
- `system` with subtype `init` gives the session id. Each `tool_use` block of an `assistant` message becomes a progress line (9.3). The `result` message ends the turn, and its `result` text is the reply. A `result` with `is_error` is an error with its text, for example "Invalid API key · Please run /login".
- The `PreToolUse` hook of 6.6.3 gates every tool call. Its timeout is `permission_timeout_minutes` plus 5 minutes, because Claude Code runs the tool when the hook times out. A `can_use_tool` control request goes through the same gate. Every other control request gets an error.
- The ACP limits apply: environment allowlist, line and reply limits, run timeout, last stderr line in an error. The backends share `process.rs` and `turn.rs` for them.
- If the chat session has no file, the run starts a new session with the note of 9.1.

**Codex with no adapter (`kind = "codex"`).** The bridge speaks the `codex app-server` protocol in `crates/bridge/src/codex.rs`. Checked on codex-cli 0.157.0 with `codex app-server generate-json-schema` and `generate-ts`. The protocol is JSON-RPC 2.0 with no `jsonrpc` field, one message per line. The bridge uses no method that needs the `experimentalApi` capability.

- The command is the `command` of the entry plus `app-server`, in the chat folder. The bridge sends `initialize`, then the `initialized` notification.
- A new chat gets `thread/start` with `cwd`, `sandbox`, `approvalPolicy`, `approvalsReviewer: "user"`, `developerInstructions` with the summary note and the sandbox line (13.1, "Summary first"), and `config: { web_search: "disabled" }` (9.3). `approvalsReviewer` keeps a reviewer model from the user's config out of the way. A chat with a thread gets `thread/resume` with the same values and `excludeTurns`. If the resume fails, the run starts a new thread with the note of 9.1.
- `turn/start` sends the prompt as one `text` input. `item/started` of a `commandExecution`, `fileChange`, `mcpToolCall`, or `webSearch` becomes a progress line. The text of the last `agentMessage` of `item/completed` is the reply. `turn/completed` ends the turn: `completed` is a reply, `interrupted` is "Stopped.", and `failed` is an error with the Codex message.
- `item/commandExecution/requestApproval` and `item/fileChange/requestApproval` go through the gate (6.6.3). Every other server request gets "method not found".
- Codex keeps its login and threads in `CODEX_HOME`, else `~/.codex`. `HOME` passes, so the default works. A user who sets `CODEX_HOME` or `OPENAI_API_KEY` adds it to the `env` list of the entry.
- `check-agent` runs `codex --version` and `codex login status`, with no model call. If the status fails: "Codex isn't logged in. Run codex login.".
- Config load refuses a `modes` table on the entry.
- The ACP limits apply, through `process.rs` and `turn.rs`.

**A harness with only a command line (`kind = "command"`)** (asked for by the user on 2026-09-27: "We need a generic backend for all llm harnesses"; decided with an advisor on 2026-09-27). Code: `crates/bridge/src/harness.rs`, `harness_args.rs`, `harness_output.rs`, `harness_process.rs`, `harness_sandbox.rs`, and `harness_presets.rs`. Such a harness runs its own tools, so the classifier sees none of its calls. So the whole harness runs in the sandbox of 6.6.4, and the level picks the walls (9.3).

One line is enough for a tool that the bridge knows:

```toml
[agents.aider]
kind = "command"
preset = "aider"
permission = "auto-edit"
env = ["OPENAI_API_KEY"]
```

- **The template.** `command` is the program and its arguments. `{prompt}` is the message. `{prompt_file}` is the path of a file with the message, mode 0600, in the run temp folder. A placeholder can be all or part of an argument, for example `--message={prompt}`. With no placeholder, the message goes to stdin, which then closes. The bridge never uses a shell, so the message is always one argument or bytes on stdin.
  - A whole `{prompt}` argument that starts with `-` gets a leading space, so the harness never reads it as a flag. A NUL byte becomes a space.
  - Filled text is never read again, so a message that holds `{prompt_file}` stays as it is.
  - Config load refuses a word such as `{promt}`, a placeholder in the program, `modes`, and `preset` or `resume` on another kind.
- **Presets.** `preset` fills the template, so the user writes one line. `command` then replaces only the program, and can add flags before the preset arguments, for example `command = ["aider", "--model", "o3"]`. Checked against each tool's docs on 2026-09-27. A live test with the real tools is still to do: none is installed on the build computer.

| Preset | Arguments after the program | The message | Resume | Model hosts for `strict` |
|---|---|---|---|---|
| `aider` | `--message-file={prompt_file} --yes-always --no-pretty --no-stream --no-fancy-input --no-check-update --no-show-model-warnings --analytics-disable`, and at `ask` also `--chat-mode=ask --dry-run --no-auto-commits` | a file | `--restore-chat-history` | none: the model decides, so the entry names them in `agent_hosts` |
| `gemini` | `--prompt={prompt} --approval-mode=yolo` | an argument | none | `generativelanguage.googleapis.com`, `cloudcode-pa.googleapis.com`, `oauth2.googleapis.com` |
| `opencode` | `run --auto {prompt}` | an argument | none | none |
| `goose` | `run -i - -q --no-session` | stdin | none | none |
| `llm` | `--no-log` | stdin | none | `api.openai.com` |

- **Resume.** `resume` lists the arguments for a chat that goes on: it ran with this agent in this folder before, and the message has no `n` flag. They go at the end, or before a `--`. The chat keeps a mark as its session, not an id. With no `resume`, each message is a fresh run. The harness home folder goes away after each run (below), so only a harness that keeps its history in the chat folder can go on. Of the presets, that is aider. A flag such as `--continue` takes the newest harness session, so two chats in one folder share it.
- **The output.** Each stdout and stderr line becomes a progress line (9.3) through `Activity::step`, so the guards for "Level:" and "Desktop:" apply. All of stdout is the reply, as Markdown. The bridge removes escape sequences and control characters, and keeps the text after the last CR of a line, as a terminal shows a progress bar. It sets `NO_COLOR=1` and `TERM=dumb`. A harness prints its answer last, so a reply over 256 KiB keeps its end, from a line start, after the note "(The output was too long for the game. This is its end.)". Over 16 MiB of stdout stops the run with "The agent wrote more output than the limit, so the run stopped.".
- **The end.** Exit status 0 is the reply. Any other status is the error "The agent failed (exit status <n>): <the last line of stderr>". No output is "(The agent didn't reply.)". The run timeout, Stop, and the output limit kill the whole process group (9.4).
- **The sandbox.** The walls of 6.6.4 hold the harness and every program that it starts: writes only in the chat folder and the run temp folder, the `deny` and `desktop` paths hidden, private `/tmp`, `/run`, and `/var/tmp`, the `.git` entries pinned, and on Linux its own network, processes, and `/proc`. On Linux: `bwrap <the walls> --chdir <chat> --unshare-all --die-with-parent --new-session -- <gnomish-relay> --sandbox-forward <proxy socket> <local ports> --exec <harness> <arguments>`. On macOS: `sandbox-exec -p <profile> -- <harness> <arguments>`. No holder: the harness is the one process tree of the run.
  - **The network.** One proxy per run, with the agent rules (6.6.4, "Two kinds of scrutiny"), because the bridge cannot tell the harness from its commands. With `agent_network = "open"`: any public host. With `"strict"`: the preset model hosts, `agent_hosts`, and the sandbox hosts (the default hosts and `allow_hosts`). `local_ports` works as for every agent. On macOS the profile denies the keychain, so the harness takes its key from `env`.
  - **The home folder.** On Linux the harness sees a copy-on-write view of the home folder (`--overlay-src`, as for cargo in "The downloads of cargo and rustup"). It goes before the chat folder binds, and the hidden paths cover it after. So sessions, caches, and a refreshed login work during the run and go away with it. No write reaches the real home folder, so no harness startup file, MCP server, or hook can wait for a user terminal session. The view needs `bwrap` with `--overlay`, a temp folder outside the home folder, and a home folder outside `/tmp`, `/var/tmp`, and `/run`. Else, and on macOS, the home folder is read-only, and a harness that must write there fails. A mount under the home folder, such as a FUSE folder, shows empty.
  - **Sockets.** A read-only mount does not stop a connection to a socket file. So the walls cover each socket file in the top 3 levels of the home folder with the empty file, as the agent wall does. The home view hides them too.
  - **The variables.** The allowlist of 6.2 rule 12, the `env` list of the entry, `GNOMISH_RELAY_JOB=1`, the proxy and cache variables of "The variables" (6.6.4), `TMPDIR` and `XDG_CACHE_HOME` in the temp folder, `NO_COLOR=1`, and `TERM=dumb`.
- **No sandbox.** On Windows, and on Linux with no working `bwrap`, the bridge does not start the harness: "This agent runs its own tools, and this computer has no sandbox for them, so the bridge does not start it. Use an agent with kind acp, claude, or codex here, or run the bridge under WSL2 on Windows." There is no opt-in.
- **`check-agent`** runs `<program> --version` in the same walls, at `ask`, with no model call. So a program in a hidden path fails there, not in the game. Exit status 126 or 127 is the error "<program> does not start in the sandbox: <the last line of stderr>". It prints the version, whether the entry resumes, the sandbox and the fate of home folder writes, how the message goes in, what the levels mean, and the network.
- **What this does not stop.** In `open` mode the harness and its programs reach any public host, with the keys of its `env` list. Claude commands reach only the sandbox hosts. The proxy logs each host. A harness can print a partial answer and exit with 0.
- **Tests.** `crates/bridge/tests/harness.rs` runs `fake-cli-agent` in the real sandbox: the three ways in, progress lines, a crash, long and huge output, Stop with a background program, the timeout, writes and hidden paths, `ask`, the home folder, the proxy in both modes, a home folder socket, the variables, resume, `check-agent`, and no sandbox. A test marked `#[ignore]` runs each preset whose tool is on `PATH`, with a real model call. The fuzz target `harness` checks the template and the output reader.

**Adding an agent.** Any ACP agent is one entry in `config.toml`. Nothing else changes:

```toml
[agents.gemini]
kind = "acp"
command = ["gemini", "--acp"]
permission = "ask"
env = ["GEMINI_API_KEY"]
modes = { ask = "default" }
```

Then run `gnomish-relay check-agent gemini`. It starts the agent, opens one session in `default_cwd`, and shows the name, version, session resume support, and mode ids. It fails if a mode in `modes` does not exist.
For `kind = "claude"`, `check-agent` runs `claude --version` and `claude auth status --json`, with no model call. If `loggedIn` is not true: "Claude Code isn't logged in. Run claude and log in.".
The addon sends `agent=gemini` for a chat that uses it. An agent with no entry gets "That agent isn't in config.toml. Pick another one in Settings, or add it on your desktop."
`kind = "echo"` answers with the message, to test the game path with no agent. Setup writes it only when it finds no agent, so the answer gives the next step: "No agent yet. Install Claude Code or Codex, then run gnomish-relay setup."

### 9.3 Permissions

Each config agent has one permission level:

| Level | Meaning |
|---|---|
| `ask` | Every write and command needs an answer. A read inside `allowed_roots` needs none. |
| `auto-edit` | No answer for file edits in the chat folder, commands that the command sandbox holds (6.6.4, "The sandbox answers at `auto-edit`"), and allow-table commands (12). Other commands ask. |
| `full-auto` | No question at all, in the game or on the desktop, inside the sandbox. Only per chat, after one desktop Approve (see "Full-auto for one chat"). |

**The gate.** The classifier gives each tool call one verdict (6.6.3). The job level then picks the action, in `gate::decide`:

| Verdict | `ask` | `auto-edit` | `full-auto` |
|---|---|---|---|
| `deny` | refuse | refuse | refuse |
| `desktop` | desktop | desktop | run; a file tool outside the walls fails |
| `ask` | game | game | run |
| `ask`, a command that the sandbox holds | game | run | run |
| `allow`, a read | run | run | run |
| `allow`, a write or a command | game | run | run |

- At `ask`, only a read-only call runs with no question. A chat-folder write and an allow-table command ask in the game. A session tool (6.6.3) counts as a read.
- For an ACP agent that picks its questions (6.6.3), `ask` and `allow` both ask in the game, at every level.
- A refusal gives the agent its reason: "It touches the settings or data folder of Gnomish Relay, which agents can't reach.", "Full-auto keeps file writes in the chat folder and keeps secrets hidden.", "Denied on your desktop.", "No answer on your desktop.", "Denied in the game.", "No answer from the game.", "The player sent a new message.", or "Not allowed from the game." when nobody in the game listens.
- A game question gets Allow and Deny. A desktop question shows only a notice in the game (6.6.3).

**Raise the level** (asked for by the user, decided with an advisor on 2026-09-26). A chat that asks for more than the config allows, for example `auto-edit` with `permission = "ask"`, gets one desktop dialog. Code: `crates/bridge/src/raise.rs` and `config_edit.rs`.

- The dialog is its own kind of desktop request (6.6.3): same dialog, same `gnomish-relay approve` fallback, same 0600 request file, first answer wins. Its text is fixed bridge text plus the agent name from the config, never game text:
  - `auto-edit`: "A chat from WoW asks for more access. Allow <agent> to edit files in the chat folder with no question, in every chat from WoW? Claude Code also runs commands inside the sandbox without asking. Risky commands still ask in the game. This writes permission = "auto-edit" to config.toml. Approve only if you just sent a message from WoW."
- A raise goes up to `auto-edit` only. Full-auto is never a config level for every chat. A chat at `full-auto` gets the dialog of "Full-auto for one chat" instead (changed on 2026-09-30). With `permission = "ask"`, a chat at `full-auto` first gets the raise to `auto-edit`, and on a later message the full-auto dialog. One message never shows two dialogs.
- The agent starts only after the answer, so an approved run uses the new level. The game shows the notice of 6.6.3 with ` raise <level>`, and the whisper line "Approve on your desktop to let <agent> work at <level>." The run timeout stops during the wait, as for any question.
- On Approve, the bridge reads `config.toml` again with the config load checks. It changes the one `permission = "..."` line of the `[agents.<name>]` table, and keeps comments and all other lines. It parses the new text: it must load, the agent must have the new level, and all other levels must be unchanged. Else it writes nothing. It writes with an atomic rename and mode 0600. Then it sets the new agent level in the running policy, and reloads nothing else.
- The bridge checks the edit before the dialog, so the user never approves a change that it cannot write. It refuses a quoted table name, an inline table, dotted keys, a missing or double `permission` line, a value that is not a plain `"..."` string, and a config that does not load. Then there is no dialog, and a log line says what to fix.
- On Deny, a closed dialog, no answer, Stop, a new message, or a failed write, the run goes on at the config level, and the game level line shows it (9.3, "The level in the game").
- At most one raise waits at a time. Meanwhile a second chat that needs a raise runs at once at the config level, with no dialog. So one answer never goes to many runs.
- Every answer other than Approve starts 10 quiet minutes with no raise dialog, for every agent. So a hostile addon that sends messages gets at most one dialog per 10 minutes. A Deny on the desktop (or a closed dialog) also ends raise dialogs for that agent until the bridge restarts. So a user who set `ask` on purpose sees the dialog once, not every 10 minutes.
- Only a message with work for the agent can raise. A list of sessions and an attach never do.
- The bridge logs each raise: request id, answer, and whether it wrote the config.
- S6 does not change: at agent start, the run level is at most the chat ceiling. Only the desktop changes the ceiling.

**Full-auto for one chat** (decided by the user on 2026-09-30, in their words: "I need to set always allow per chat, and it needs to work as always allow, even cat .git/config or whatever. Needs to be always allowed in that case. That's the price we pay for always allow."). The design follows the permission modes of Claude Code: one switch per chat, no question per action after it, and the mode always on screen. Claude Code trusts its own keyboard. A game message can come from another addon (6.6.1), so the first switch of a chat adds one desktop Approve. The bridge never passes `bypassPermissions` to Claude Code. The hook still sees each call, so the file tool walls, the tool result check, and the log still work, and the gate answers `allow`. Code: `crates/bridge/src/full_auto.rs` and `gate.rs`.

- **What runs with no question.** In a chat at `full-auto`, the gate asks nothing, in the game or on the desktop. Nothing in the chat waits for an answer.
  - Every command, in the sandbox of 6.6.4: game questions, `desktop` answers (a command that does not parse, such as `command ?`, `sudo`, `eval`, a command substitution, a shell after `|`, a redirect outside the chat folder), and the "never always" commands (`git push`, `gh`, `rm -rf`, `npx`, `curl`, `cargo publish`, `chmod`).
  - Every file tool inside the walls: a read of any non-secret path, also outside `allowed_roots`, and a write in the chat folder, also to `.git/config`, `.git/hooks/`, `.claude/`, `.envrc`, `.vscode/`, and `.github/workflows/`.
  - Every other tool: web fetch, web search, and subagents. They run in the agent process, behind its own wall (6.6.4, "The agent process behind the proxy"). A game run of Claude starts no MCP server (6.6.4).
- **What fails with no question.** These calls fail at once, and the agent sees the reason:
  - A `deny` answer: the files that guard the relay (6.6.3), with the reason of 9.3.
  - A file tool that reads or writes a secret path, writes outside the chat folder, or has a path that does not resolve: "Full-auto keeps file writes in the chat folder and keeps secrets hidden." Secret paths are the `desktop` paths for reads and writes of 6.6.3: `.ssh`, `.env` files, tool tokens, agent logins. Claude file tools run in the agent process, outside the sandbox, so the gate applies the sandbox walls itself (`gate::within_walls`). It asks the proved classifier with a second policy, `action_input::wall_policy`: one root `/`, the same `deny` folders and secret patterns, and no write patterns. A `desktop` answer of that policy means exactly one of those three cases. A write to the run temp folder fails too: the gate does not know that folder.
  - What the sandbox stops (6.6.4): a command write outside the chat folder and the temp folder fails ("Read-only file system" with `bwrap`, "Operation not permitted" with Seatbelt). A hidden path reads as an empty folder or file. A host not on the allow list gets `403` from the proxy. Other network use fails, because the sandbox OS network is off. Nothing asks. To reach another host, add it to `[sandbox] allow_hosts` (12).
- **Why `deny` stays.** File tools run outside the sandbox, so the gate is their only wall. A write to `approvals/<id>.answer` approves the agent's own desktop requests. A write to `config.toml` widens the ceiling. A write to `state.json` adds full-auto chats. A read of `strip.key` signs fake messages. For commands, the sandbox hides the same paths, so the `deny` there costs nothing.
- **Only with a wall that holds.** Full-auto needs a command sandbox that holds every agent command: kind `claude` with `bwrap` or `sandbox-exec` (6.6.4, `gate::SandboxWall::Holds`). Codex retries an allowed command outside its sandbox with no request, and its sandbox reads `~/.ssh` (6.6.4), so for Codex "It stays in the sandbox" is false. An ACP agent runs its commands itself. A `command` agent already runs every command in the sandbox with no question at `auto-edit`. So for every other agent, and with no command sandbox, a chat at `full-auto` runs at `auto-edit` (at most the config), with no dialog. The gate also treats `full-auto` as `auto-edit` for a job whose wall leaks (`gate::Job`), so no backend gets it by mistake.
- **The first switch.** The first message of a chat at `full-auto` with no approval opens one desktop request before the agent starts, as a raise does. The text is fixed bridge text, the agent name from the config, the chat name, and the real run folder: "Let <agent> run anything with no question in the chat "<name>" (<folder>)? It stays in the sandbox, but it can push, publish, and change files in this folder that git and other tools run later. Approve only if you just picked full-auto in WoW." The name comes from the game, so the bridge cuts it to 40 characters and turns control characters into spaces. The game shows the notice of 6.6.3 with ` raise full-auto`, and the whisper line "Approve on your desktop to let <agent> work at full-auto."
  - On Approve, the bridge stores the approval, and the run is at `full-auto`. The run level line changes to "Level: full-auto".
  - On Deny, a closed dialog, no answer, Stop, or a new message, the run goes on at `auto-edit` (at most the config). The level line says "Level: auto-edit", so the header shows that the chat is not at full-auto.
  - The quiet rules of a raise apply, per chat in place of per agent: one dialog at a time, for raise and full-auto dialogs together; 10 quiet minutes after any answer other than Approve; and after a Deny, no more full-auto dialog for that chat until the bridge restarts. A message with no dialog runs at `auto-edit` at once.
  - A message that needs a folder request (9.12) gets no full-auto dialog. One message never shows two dialogs.
- **The approvals.** `state.json` keeps them as `full_auto`: chat id and real folder of each approved chat, at most 64. A new approval pushes out the oldest. So a bridge restart and a `/reload` keep them.
  - A message of the chat at `full-auto` with an approval for the same chat and real folder runs at `full-auto` with no dialog.
  - Another real folder asks again: the player picked a new folder, or a folder link now points elsewhere.
  - A message at a lower level removes the chat approval, so the next switch up asks again. Switching down never asks. Delete removes the approval too.
- **The config.** The agent `permission` still caps the other levels. `ask` keeps every chat of that agent at `ask`, with no full-auto dialog: a user who set `ask` on purpose gets no full-auto question. `auto-edit`, which setup writes, allows full-auto per chat after its one Approve, so a fresh install needs no config edit. `permission = "full-auto"` counts as `auto-edit`: no config value gives full-auto to every chat. `allow_full_auto = false` (12, default `true`) turns full-auto off for every chat: no dialog, stored approvals do not apply, and a chat at `full-auto` runs at `auto-edit`.
- **The trade-off, honestly.** The user accepts it only for the chats that they switch. A prompt-injected agent in a full-auto chat (a README, an issue, or a web page with hidden instructions) can, with no question:
  - run any command in the sandbox, also `rm -rf` of the chat folder, and send data to the allowed hosts;
  - push to GitHub or publish a package, with a token that it finds or that the prompt holds: the proxy reaches `github.com` and `registry.npmjs.org` (6.6.4, "What the list does not stop");
  - write `.git/config`, `.git/hooks/`, `.claude/`, `.envrc`, `.vscode/`, or a workflow in the chat folder. Code on this computer runs these later, outside the sandbox: the next `git status` in a terminal (`core.fsmonitor`), Claude Code in a terminal, direnv, an editor, or CI. The end-of-run check (6.6.4, "The `.git` entries") still names a new git folder.
  - Another addon can send a message in an approved chat, by its id, and get the same. It cannot approve a new chat.
- **Tests.** In `gate.rs`: `at_full_auto_a_desktop_command_a_never_always_command_and_an_unparsable_command_run_with_no_question`, `at_full_auto_a_write_of_git_config_in_the_chat_folder_runs_with_no_question`, `at_full_auto_a_read_outside_the_roots_runs_with_no_question`, `at_full_auto_a_file_write_outside_the_chat_folder_or_a_secret_read_fails_with_no_question`, `at_full_auto_a_deny_still_refuses`, `full_auto_with_a_leaking_wall_or_no_sandbox_works_as_auto_edit`, and `auto_edit_still_asks_on_the_desktop_and_in_the_game`. In `full_auto.rs`: the dialog text, the store, and the name cut. In `tests/run_loop.rs`: `the_first_switch_to_full_auto_asks_once_on_the_desktop_and_the_next_message_does_not`, `a_denied_full_auto_runs_the_chat_at_auto_edit`, `a_full_auto_approval_survives_a_restart_of_the_bridge`, `a_new_folder_asks_for_full_auto_again`, `a_lower_level_forgets_the_full_auto_approval`, `with_allow_full_auto_off_a_chat_at_full_auto_runs_at_auto_edit_with_no_dialog`, and `an_agent_whose_wall_leaks_gets_no_full_auto_dialog`. In `tests/claude_gate.rs` and `tests/codex.rs`: the fake agents at `full-auto`. In `tests/addon_flow.rs`: the dropdown, Shift+Tab, and the header.

Each backend maps the level differently:

- `acp`: the bridge sets the session mode. Mode IDs differ per agent, so each agent has a `modes` table in the config.
- `claude`: `--permission-mode`, and the hook of 6.6.3 for every call. `ask` is `manual`. `auto-edit` and `full-auto` are `acceptEdits`. The hook decides, so the mode matters only when the hook fails: then `manual` asks the bridge, and `acceptEdits` does not. The `modes` table of the entry can name another mode: `acceptEdits`, `auto`, `dontAsk`, `manual`, or `plan`. Config load refuses any other name, and also `bypassPermissions`: in that mode Claude Code asks nothing, so no tool call reaches the bridge, and the game ceiling has no effect.
  - Not `plan` by default (decided with an advisor on 2026-09-26). In the first game test, a "create a file" message at `ask` asked on the desktop. In `plan` mode, Claude Code writes its plan to `~/.claude/plans/<name>.md`, and `.claude/` is a `desktop` write path. At `ask` the gate already asks in the game before each write, so plan mode added only this file. The gate makes no exception for plan files: Claude picks the path, and `.claude/` holds settings and hooks that run code. A user who sets `modes = { ask = "plan" }` gets one desktop question per plan.
- `codex`: the thread sandbox, and `approvalPolicy: "untrusted"` at every level, which sends the most calls to the bridge (6.6.3). `ask` is `read-only`. `auto-edit` is `workspace-write`, with `exclude_slash_tmp` and a private `TMPDIR` (6.6.4). A Codex chat never runs at `full-auto` (see "Full-auto for one chat"). The sandbox applies after the gate answer. The bridge never uses `danger-full-access`, `never`, `on-request`, or `granular`: none asks more than `untrusted`.
- `command`: the harness has no permission channel, so the level picks its walls (9.2, "A harness with only a command line"). `ask` makes the chat folder read-only: the harness reads and answers, and changes nothing. At `auto-edit` and `full-auto` the chat folder is writable, and the harness runs its own commands with no question, inside the sandbox. `auto-edit` cannot keep "commands ask" here. So the first reply after bridge start says "(<agent> runs its own commands with no question, inside the sandbox.)", and so do setup and `check-agent`. The raise dialog to `auto-edit` for such an agent says "Allow <agent> to edit files in the chat folder AND run its own commands with no question, in every chat from WoW? It runs them inside the sandbox." The Settings tab shows the kind `command` next to the level. The addon does not change.
- For game messages, Codex runs only through `codex app-server` or ACP, so the bridge sees each question of its tool calls (6.6.3).

**Live permission flow (ACP):**

1. The agent sends `session/request_permission`. The gate answers it. A game question waits for the game.
2. The bridge writes the popup text with `popup_text` (S15): the tool call command line, else its path or address, else its title, then its title as "the agent says". It adds the request to `permissions` in `Live.lua` (S20), with options `o1` to `o4`.
3. The addon shows a popup with the text and the options.
4. The user picks an option. The addon sends a control record with `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256 of the shown popup text, in hex.
5. The bridge takes the answer only for an open request of the same chat, a real option, and a matching hash. Then it answers the agent. A second answer does nothing.

**Live permission flow (`claude`):** the hook and a `can_use_tool` control request go through the same steps. The popup text is the `command` of the tool input, else its `file_path`, `notebook_path`, `path`, `url`, or `pattern`, else the tool name. "The agent says" is the tool name and the request `description`. The game gets Allow (`allow_once`) and Deny (`reject_once`), with "Always allow" (`allow_always`) between them when the bridge offers it (6.6.5). The hook answers with `permissionDecision` and a `permissionDecisionReason` that Claude sees. For `can_use_tool`, an allow sends the tool input back unchanged as `updatedInput`. A deny sends a `message` that Claude sees, for example "Denied in the game.". The answer never holds the request's `permission_suggestions`: they add permanent Claude Code allow rules, and only the bridge keeps rules (6.6.5).

**Live permission flow (`codex`):** a server approval request goes through the same steps. For a command, the popup text is its `command`. For a file change, it is the change paths, from the `fileChange` item of `item/started`. A move shows as `<path> -> <move_path>`. With a `grantRoot`, the popup text is "write anything in <root>". The classifier checks the root and every change path, also the `move_path` of a move, because Codex applies each patch path, also one outside the root. "The agent says" is the `reason`, else "run a command" or "change files". The game gets Allow and Deny. Allow sends `accept`. Deny or no answer sends `decline`. The bridge never sends `acceptForSession`, `acceptWithExecpolicyAmendment`, or `applyNetworkPolicyAmendment`: each adds a rule for later calls (6.6.5).

Rules:

- The request id holds the question time, so after a bridge restart an old strip cannot answer a new request.
- The bridge offers its own `allow_always` for a command of a Claude run at `auto-edit` in the sandbox (6.6.5). It never passes an agent "always" option to the game. A rule applies at the run level.
- Each agent tool call also becomes a progress line in `Live.lua`, for the activity panel. A run shows its level line first, then its last 4 lines.

**The level in the game** (decided with an advisor on 2026-09-26). The bridge runs a chat at the lower of its level and the config `permission` (S6). In the first game test, the header said "Claude · auto-edit", but the run was at `ask`. So the game now shows the level that applies:

- At run start, the bridge writes the level line as the first progress line: "Level: auto-edit", or "Level: ask (config)" when the config lowered the level. It writes it every run, so a raised config also clears an old "(config)".
- The line stays first while the agent adds steps. `Activity` keeps it and the last 4 agent lines, so the proved writer of S20 still gets at most 5 lines, and S9 and S20 do not change.
- Only the bridge writes "Level:" lines. `Activity::step` is the one entry for agent lines, and it puts "agent: " in front of such a line.
- The addon takes the level only from the first progress line of a working message, and only when it is an exact bridge text. It keeps the level with the chat, in the saved variables. The header shows it, for example "Claude · ask (config)". Before the first run, the header shows the level that the chat asks for.
- "(config)" means that the config lowered the level. A chat at `full-auto` with no approval runs at `auto-edit`, and its line is "Level: auto-edit", with no "(config)": the config did not lower it. "Level: full-auto" comes only after the chat approval (see "Full-auto for one chat").
- A lowered run also starts its reply with "(Ran at ask, the most that config.toml allows.)", because a short run can end before the addon loads a slot. The addon does not read this note: after rendering, an agent can write the same text.
- The run timeout stops while the run waits for a permission answer. A separate `permission_timeout_minutes` applies (default 10). After it, the bridge answers "cancelled".
- If the game closes or reloads, open requests stay in the next publish until they time out.
- Stop ends an open request as "cancelled", and the run as "Stopped.".

**A new message ends a wait** (asked for by the user, 2026-09-26, with an advisor). While a run waits for an answer, in the game popup or on the desktop, a new message of that chat:

- ends the waiting request. The desktop dialog closes, and the agent gets "The player sent a new message.";
- stops the turn as Stop does, so the old message ends as "Stopped.";
- runs next, and resumes the same agent session, so the agent sees the refused call and the new message.

Rules:

- Only a message that the bridge newly accepts counts. A duplicate, a refused message (full body, full queue, or the rate), a list, an attach, and a message of another chat never end a wait.
- While a run works and waits for nothing, a new message waits in the queue, as before. Else each follow-up ends a long run.
- A raise that a new message ends counts as no answer (the 10 quiet minutes start).
- This gives an addon no new power: Stop already ends a run, and the cancel only ever answers no.
- The other waiting messages of the chat keep their order.

### 9.4 Agent processes

- ACP: one agent process per run. ACP agents have no sandbox, so each of their calls asks at most (6.6.4).
- `claude`, `codex`, and `command`: one process per run. For `command`, the process is the sandbox, with the harness and all its programs.
- `max_parallel_runs` counts active runs, not processes (8.2).
- If an ACP process stops, the bridge restarts it and resumes the open sessions. If a session cannot resume, the bridge reports an error for that chat.
- Stop for `command` kills the whole process group at once, with no grace: a harness has no cancel channel. On Linux the sandbox has its own process ids, so every harness program ends with it.
- The bridge declares ACP client capabilities `fs` and `terminal` as false in v1. The agent uses its own tools.
- `process.rs` starts every agent process: never through a shell, with the allowlist of 6.2 rule 12, 8 MiB per line, and the last 2 KiB of stderr for an error. `turn.rs` holds the run timeout, Stop with its 10-second grace, and the wait for a game answer. ACP, `claude`, and `codex` share them.
- If an agent needs a login, the bridge reports it in the game with the next step. For Claude, a failed run whose error names a login (for example "Please run /login", a command inside Claude) ends with "Claude needs you to log in again. On your desktop, run claude and log in." The bridge never handles credentials.
- The bridge removes `CLAUDECODE` from the environment of each child process and sets `GNOMISH_RELAY_JOB=1` (section 10).

### 9.5 Sessions and folders

Claude stores sessions per project folder.
If a chat changes folder, the bridge starts a new session for it.
`state.json` stores the folder of each session.

### 9.6 Resume a session

The player can continue a saved agent session in the game, for example a Claude Code session from a terminal.
The bridge cannot join a session that runs in a terminal, because the terminal owns its input. So the game continues the saved session.

**The list.** **Resume** in the window sends a `list` record. The bridge asks each config agent for `session/list`, when offered, and answers with one line per session:

```
agent \t session \t age in seconds \t 1 if active \t chat \t folder \t folder name \t title
```

- A session shows when its folder exists and is inside a root (6.2, rule 1) or is a new folder (9.12). Others stay hidden, with no count. So the home folder, a folder outside it, a hidden folder, and a `deny` or `desktop` path never show. The relay checks the folder text, and the list checks the real path with the four rules of 9.12, so a link cannot lead out. Changed on 2026-09-30: with only the roots, an install with no root had an empty list, although the user had many sessions.
- The list holds the 30 newest sessions. A title is at most 100 bytes, with control characters turned into spaces.
- `folder` is in the old form of 9.9: the path from `default_cwd`. An older addon sends it back as it is, and it resolves to the same folder. The addon turns it into the home form with the tree of 9.9 before it saves it as the chat folder. On Windows, the game cannot send an absolute path (7.1.1).
- `chat` names the game chat that already has the session. A click on that row opens that chat, not a second one.
- A session changed in the last 5 minutes is active: probably open in a terminal.
- An agent whose list fails is left out. When every agent fails, the reply is an error.
- The addon keeps the last list in its saved variables, so the picker opens at once. A newer list replaces it.

**The attach.** A click on a session makes a new chat with the session title, agent, and folder. Its first message has the `attach` flag and no text.

- The bridge accepts only a session of its last list, so the list folder check guards the attach too.
- The attach of a session in a new folder waits for the desktop request of 9.12, as a message in a new folder does: "Let agents from WoW work in <folder>? …". After Approve, the folder is a root, and the attach runs. Deny and no answer end the attach with the reply of 9.12, and nothing attaches.
- An active session gets `session/fork`: the chat continues a copy, and the terminal keeps the original. With no fork, the chat continues the session itself.
- The bridge replays the session with `session/load`, and answers with the last exchange: the last prompt on the first line, the last answer below. The addon shows them as history, with no whisper.
- The prompt and answer lose terminal escape sequences (color, title) and control characters except newline and tab. A session can hold terminal output, and the game shows those bytes as boxes.
- The chat then works as any other chat. Its next message resumes the session (9.5). The config level ceiling applies (S6).
- A chat delete (7.1.1, `d`) never deletes the session. A later Resume brings it back.

**Claude Code sessions (`kind = "claude"`).** `claude -p` has no list call, so the bridge reads the Claude Code session files, with the rules of `listSessions`, `getSessionMessages`, and `forkSession` of the Claude Agent SDK. No model call happens. Code: `claude_sessions.rs`.

- The files are `<config>/projects/<folder>/<session id>.jsonl`. `<config>` is `CLAUDE_CONFIG_DIR` if the `env` list of the entry names it, else `~/.claude`, so the agent sees the same folder.
- Only files with a UUID name count. The list reads the 60 newest files by change time, and 64 KiB from each end of each file.
- The title is the newest `customTitle`, then the one in `<session id>/custom-title.json`, then `aiTitle`, `lastPrompt`, `summary`, and the first prompt. The folder is the newest `relocatedCwd`, else the first `cwd`. The time is the file change time.
- Left out: a file whose first line is a subagent line, a file with no title or no folder, and a session that went on in another file (`continued-in`).
- The attach reads the last 8 MiB of the file. It takes the chain of the newest leaf by `parentUuid`, the last real prompt on it, and the text of every assistant message after that prompt. Tool results, Claude Code notes in a tag, and slash commands are not prompts.
- The fork writes a copy next to the file, mode 0600, as `forkSession` does: a new session id, a new uuid per entry, a `forkedFrom` note, no progress or subagent entries, and the title with " (fork)". The file must be at most 64 MiB. A live test resumed such a copy with its history.

**Codex threads (`kind = "codex"`).** `codex app-server` has the calls that the list and the attach need, and none reaches the model:

- The list is `thread/list`, newest change first, with the threads of the terminal, the IDE, `codex exec`, and the app server (`sourceKinds`). The title is the thread `name`, else its `preview`. The time is `updatedAt`, in seconds.
- The attach reads the newest turn with `thread/turns/list` (`limit` 1, `itemsView` `full`): the last `userMessage` is the prompt, and each `agentMessage` after it is the answer.
- The fork is `thread/fork`. The chat continues the new thread.
### 9.7 A second app: Timeways

Timeways is a separate story addon (`~/Documents/Code/Personal/timeways`). It uses this bridge as its desktop program, with the same strip, slots, and proofs. It has its own key, slots, and lane. This section is the approved plan (2026-09-25). A reviewer checked it, and the user approved every decision below.

**Status (2026-09-26):** steps 1 to 8 are done, with 5b. Step 6 runs the model calls of the story program with no tools, with the budget of decision 10 (protocol in 9.8).

- The Timeways lane is on only when `timeways.key` exists in the config folder. Only `setup --timeways` makes that key (decision 15). So an install with no Timeways works as before.
- The story program runs only with the Timeways lane and a `[story]` section with a `program` in the config (12). Without `[story]`, the lane answers each message in its own slots with "Timeways isn't running on your computer. Run gnomish-relay restart."
- The loopback of step 5 runs in the fake game of the tests. The shared Lua transport sends a real strip with a batch of the Timeways addon. The fake story program answers, and the Lua slot poll reads the answer from `Timeways_S0001`. A loopback in the real game waits for a Timeways addon build.
- The story program of the Timeways repo speaks the same shapes, and the fake story program copies them. An end-to-end test runs the real story program with the real bridge (9.8).
- Since step 5b, the test addon sends, retries, and polls through `Messages.lua` (13.2). This is behind the seam of the Timeways addon: `ns.Link = { Fits(text), Send(text) }`, and one call for each final reply.
- Since step 7, the two addons take turns for the strip corner (7.1.2).

**Decisions:**

1. **Keys.** The relay key stays `strip.key`. The Timeways key is `timeways.key`, in the same config folder. The bridge refuses to start if the two keys are the same.
2. **Routing (S29).** The bridge checks the tag of each strip under both keys. If one key verifies, the strip goes to that app. If no key verifies: `BadTag`. If both verify: `Ambiguous`, and the bridge drops and logs the strip. S29 proves this choice, not the cryptography. The bridge holds the keys in a `KeySet { relay, timeways }` struct, not a list, so an index cannot swap the apps.
3. **Outbox frames.** A frame in the saved variables of one app counts only if it verifies under the key of that app. The bridge refuses any other frame.
4. **One lane for each app.** Each lane has its own replay store, state file, rate limit, slot window, saved-variables watch, reload inbox, tokens, and restore. The Timeways lane type holds no agents, so a Timeways strip can never start a coding agent. Its state is in `<data>/timeways/state.json`. The relay state stays where it is. The Timeways slots are `Timeways_S0001` to `Timeways_S1000`. Its saved variables file is `Timeways.lua` (global `TimewaysDB`). The lane publishes only when those slot folders exist.
5. **Names for each app.** The slot, restore, and live files set a Lua global with a name for each app, for example `GnomishRelay_SlotData` and `Timeways_SlotData`. The strip frame, the slot addon names, and the saved-variables name also differ for each app. S9, S18, and S20 are restated over an `App` enum in `protocol` (approved). So one app can never overwrite a value that the other app is about to read.
6. **Flags.** The flags split into transport flags (`h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, `restored`) and coding flags (`perm=`, `level=`, `agent=`, `attach=`, `list`, `d`, `n`, `stop`). The Timeways lane parses only the transport flags. The lane refuses a Timeways record with a non-empty `cwd`. The record counts as seen, and its reply is the error "Timeways takes no folder.".
7. **Restore.** Timeways has no restore bundle. The story state is on the desktop, so the addon rebuilds from there. A Timeways hello never starts a relay restore and never retires a relay token.
8. **The story program.** The bridge starts `timeways-story` when the Timeways key exists. It uses a path from the config (never a `PATH` lookup), no shell, and the environment allowlist of 6.2. They talk JSON lines over stdin and stdout, with a size limit on each line, a version handshake, and a timeout for each request. The bridge checks each message against a fixed shape. The bridge writes all files that the game reads.
9. **The story sandbox.** The story program reads hostile text: records from any addon, the names and messages of other players, and model answers. So it runs in the sandbox of 6.6.4. It writes only `<data>/timeways/`, has no network, and cannot read the `deny` and `desktop` paths. On Windows there is no sandbox yet: Timeways runs, and the bridge shows a one-time warning. (Step 5: the story program writes only `<data>/timeways/story/`, because `<data>/timeways/state.json` holds the replay store of the lane. The same warning shows on a Linux with no working `bwrap`.)
10. **Model calls.** The story program asks the bridge for a model call over the app protocol. The bridge runs the model with no tools and returns only text.
    - **Claude:** `claude -p --tools "" --strict-mcp-config`, with flags that load no user or project settings, in a new empty private temp folder for each call (details below).
    - **A local model** (Ollama, LM Studio): through `curl` with `-q` first, `--proto =http`, `--max-redirs 0` and no `-L`, `--noproxy '*'`, `--max-time`, and the prompt through stdin (`--data-binary @-`), never in the arguments. The bridge limits the answer size while it reads. The config accepts only `127.0.0.1` and `[::1]`, not `localhost`. The answer is hostile text, like an agent reply.
    - **Budget.** The bridge enforces a budget of calls for each app with the proved limiter of S14, so a hostile addon cannot spend the model subscription faster (details below).
    - **Later, a hosted model** that the player pays for comes as one more route, with no addon change. Its shape is in 11.6.
    - **Step 6, as built.** `crates/bridge/src/model.rs` holds the open calls and the budget, `model_claude.rs` the Claude route, and `model_local.rs` the local route. Each call runs on its own thread, with the timeout `[story] model_timeout_seconds` (12). A stop of the story program, and the end of the bridge, end every open call: the bridge kills its `claude` or `curl` process at once. The answer of such a call never reaches the next story program.
    - **The Claude flags, checked live on Claude Code 2.1.283.** The command is `claude -p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --tools "" --strict-mcp-config --setting-sources "" --safe-mode --disable-slash-commands --no-session-persistence`, plus `--model <claude_model>` when the config names one.
      - With these flags, the `init` message lists no tools, MCP servers, skills, slash commands, or plugins of the user. A `UserPromptSubmit` hook of the user does not run. A `CLAUDE.md` in a parent folder and a `CLAUDE.local.md` in the folder do not reach the model.
      - `--setting-sources ""` alone keeps out the plugins and skills of the user and the `CLAUDE.md`. `--safe-mode` alone keeps out the skills of the user and the `CLAUDE.md`, but a plugin of the user stays. The bridge uses both. `--disable-slash-commands` also removes the built-in skills.
      - `--bare` keeps out the same, but it also skips the OAuth login, so the bridge does not use it.
      - The `PreToolUse` hook of the `initialize` request still fires with these flags. A test with `--tools Read` showed it: a file read reached the hook, and the deny of the hook stopped it.
      - The prompt goes in as one stream-json `user` message on stdin, never in the arguments. The folder of each call is new, empty, mode 0700, and removed after the call. The environment is the allowlist of 6.2 rule 12. `command` is always `claude` from `PATH`, never the command of a relay agent.
    - **The gate on this route** answers every `PreToolUse` hook and every `can_use_tool` request with a deny ("The story program gets no tools."). The check on tool results stays on: a tool result with no error, for a call that the hook never saw, stops the call, and the call fails.
    - **The local route.** The command is `curl -q --proto =http --max-redirs 0 --noproxy * --max-time <seconds> --silent --show-error --fail --header "Content-Type: application/json" --data-binary @- <local_url>/v1/chat/completions`, with no shell and the environment allowlist of 6.2. The body goes on stdin: `{"model": <local_model>, "messages": [{"role": "user", "content": <prompt>}], "stream": false}`. Ollama and LM Studio both serve this path. The answer is the `content` of the `message` of the first item of `choices`. With `--fail`, a status of 400 or more fails the call. `curl` never follows a redirect, so a 3xx fails: its body has no `choices`. The bridge reads at most 256 KiB of the `curl` output. At the limit, it closes the output, and the call fails.
    - **Size limits.** A prompt is at most 256 KiB (9.8). An answer text is at most 16 KiB. The bridge removes every control character except newline and tab, and cuts a longer text at a character boundary. The longest text of a story program reply is 8 KiB, so 16 KiB leaves room.
    - **Open calls.** At most 2 calls of the story program are open at once. These calls get `model_failed` at once: a third call, a call with the number of an open call, a call over the budget, and every call with no model.
    - **The budget, as built** (decided with an advisor on 2026-09-26). It changes no proof.
      - The limiter counts time in steps of W seconds, where W is `[story] budget_window_minutes` (default 20, 1 to 1440). It admits at most 10 calls in any 60 steps.
      - In real time, at most 10 calls start in any window of W minutes less one step. With the default, that is at most 10 calls in any 19 minutes 40 seconds, about 30 in an hour.
      - The time comes from a clock that never goes back, because S14 needs the times in order.
      - Only a call that runs counts. The bridge checks the model, the open calls, and the call number first, and the budget last.
      - A restart of the bridge resets the budget. Only the user can restart the bridge.
    - **Why the budget works this way.** S14 fixes 10 messages in 60 time units, but not the unit. A limiter with a parameter needs a new Lean statement. A chain of limiters needs a claim that no theorem states. A coarse time unit needs neither, so the proof covers the budget as built. The price is a fixed count of 10 in each window: the config picks the window, not the count.
11. **Prompt injection.** The text of other players reaches the prompt. With no tools, it can reach only three things: the text that the user sees (bounded by S10 and S24), the story world (bounded by the rules of the world), and the budget. This is the accepted boundary. Each part has a named test. (Step 5: a Timeways reply is a JSON line, not Markdown blocks, so S24 does not apply to it. The bridge applies the escape of S10 to each text in the reply, and S8, S9, and S12 bound the slot file, 9.8.)
12. **Protected files.** The data folder joins the config folder in the `deny_folders` of the classifier (6.6.3), with a named test for each file in it. The sandbox of 6.6.4 hides it too.
13. **The shared strip corner.** Both addons draw the strip in the same corner, so they take turns through a shared "busy until" value. While an addon waits for the corner, its 40 s retry timer stops. Each addon counts only the screenshot events of its own strip. If an addon cannot get the corner, it shows "Screenshots blocked by another addon" before its frame reaches the 270 s limit. A Quint model (`models/corner.qnt`) checks this with the timers.
    - **Done (step 7).** The rules are in 7.1.2. An advisor agent and the implementer chose these details (2026-09-26):
      - A blocked app waits, and does not use the outbox. A hostile holder stays across every `/reload`, so the outbox would ask for one reload for each message.
      - The app that waits longest goes next. Without a turn rule, an app that sends often can take the corner again at each release, and no bound holds. The model first had one waiter field. It found a trace where the second waiter lost its turn, so each app now has its own wait mark.
      - The holder keeps the corner for a 2-second tail after its strip. The model found a trace where a late event of one app ended the strip of the other.
      - The blocked line shows once for each blocked time, not once for each UI session. So a second attack shows too.
      - Each app keeps its own hook for the "Screen captured" text. One shared hook needs a shared flag that a hostile addon can set.
      - The value goes through `rawget` and `rawset`, so the metatable of a hostile addon has no effect.
14. **Shared Lua transport.** `Codec.lua`, `Sha256.lua`, `Strip.lua`, the slot poll, `Health.lua`, and `Messages.lua` move into one source folder with parameters: the app name, the slot prefix, the global names, and the saved variables. The relay repo copies the folder at package time and never commits a copy. The Timeways repo checks its copy with a plain diff against the pinned relay tag.
15. **Setup: two products, one desktop app.** Gnomish Relay and Timeways are two separate products. They share one desktop app, but each product has its own setup, which sets up only that product. (The user decided this on 2026-09-30. It replaces the shared setup of step 8. Then, a plain setup also set up Timeways when the Timeways addon folder existed, so a relay player saw a story model, a story program download, and its errors. `crates/bridge/src/setup.rs` has the steps, `setup_command.rs` the two flows, `config_text.rs` the config text, and `model_setup.rs` the model search.)
    - **`gnomish-relay setup`** sets up only Gnomish Relay (11.3): the relay key addon, the relay slots, the relay part of the config, and the agents. It never writes `timeways.key`, the Timeways key addon, the Timeways slots, or `[story]`. It looks for no story model, asks no Timeways question, downloads nothing for Timeways, and prints no Timeways line. This is true also when the `Timeways` addon folder exists. It keeps an existing Timeways setup as it is. `--relay` does the same as no flag, for old scripts.
    - **`gnomish-relay setup --timeways`** sets up only Timeways: `timeways.key`, the key addon `Timeways_Key` (7.3.2), the Timeways slots, `[story]` with its model and the local model question (11.6), the story program, and the lore pack (11.4).
      - It never sets up the relay part: no relay key addon, relay slots, agents, folders, or relay question.
      - Its lines are for a player who is not an engineer (11.3, "The output of setup --timeways"). It prints no relay line: no agent, sandbox, permissions, hooks, or log line. Two lines still show: a failed autostart, and the one line about a config that it repaired or that has an error (12, and "A relay part with an error" below).
      - `--timeways` with `--relay` or `--roots` stops before the first file with "Set up one app at a time: run gnomish-relay setup for Gnomish Relay, or gnomish-relay setup --timeways for Timeways."
    - **The shared desktop app.** Each setup makes `strip.key` when it is missing, because `KeySet` needs it (with no relay addon, it does nothing). Each setup also writes `[wow]` of the config and the autostart. A Timeways key is never equal to the relay key.
    - **Both products on one computer.** Each setup adds only its own part and keeps every line of the other part, in either order. The desktop app serves each part that the config folder has: the relay lane with the relay part of the config, and the Timeways lane with `timeways.key` (and the story program with `[story]`, 9.8).
    - **The Timeways part** is `timeways.key` or a `[story]` section. The start of the desktop app, `update`, `status`, and `install` touch Timeways only when this part exists:
      - The start writes the Timeways key addon again only with `timeways.key` (11.3).
      - `update` updates the story program only when `[story] program` is a program of a Timeways release (11.4).
      - `install` makes the Timeways slots only with `timeways.key`.
      - `status` prints no Timeways line.
    - **Why `--timeways` works with no addon folder.** Players get the Timeways addon from CurseForge. The key addon and the slots are folders of their own, outside the CurseForge folder. So setup can come first, and the player needs no second setup after the addon. Setup then ends with "Get the Timeways addon on CurseForge, then restart WoW." In the Timeways folder, it writes only the old `Key.lua`, and only while the Timeways TOC lists it. It never writes other Timeways files.
    - **The config with no relay.** It holds `[wow]` and `[story]`, and no relay key (12). Only `allowed_roots` turns the relay on. Without it, the bridge has no relay lane, and a `Config` holds `relay: Option<RelayConfig>`. Why: an idle relay lane needs a fake policy. A fake policy is a trap, because an admitted strip then reaches an agent path. Setup still makes `strip.key`, so `KeySet` and S29 need no change. A later `gnomish-relay setup` adds the relay part. Its top keys go before the old text, because TOML needs them before the first table. Its tables go after.
    - **`--new-key` makes a new key for its own product.** `setup --new-key` makes a new `strip.key` and writes the relay key addon. `setup --timeways --new-key` makes a new `timeways.key` and writes the Timeways key addon. Why: each setup touches only its own product. An addon that reads one key addon can read both (decision 18). So after a leak, a player with both products runs both commands. A new key is never equal to the key of the other product. Setup loads both keys at its end, as the bridge does.
    - **`program` and `lore_pack` are optional**, both or neither (12). Setup writes them as commented lines. `setup --timeways` sets both when it installs the programs of the Timeways release and builds the lore pack (11.4).
    - **The model.** `setup --timeways` takes the first model it finds, in this order: `claude` on `PATH` (with `claude_model = "haiku"`), Ollama on 127.0.0.1:11434, LM Studio on 127.0.0.1:1234.
      - It asks a local server for `/v1/models` with `curl` and the flags of a model call. It takes the first id that is not an embedding model.
      - Each other model that it finds goes in as commented lines.
      - With no model, `[story]` has no model, and setup offers to install a free local model (11.6). A later `setup --timeways` fills a `[story]` with no model in the same way.
      - It prints "AI model: claude (haiku)", or "AI model: llama3.2:3b on this computer".
      - Why: `claude` is a deliberate install, and its answers are better than a small local model. The budget of decision 10 bounds its use. `curl` is already the only HTTP client of the bridge.
    - **An existing config.** `setup --timeways` adds `[story]` to a config that has none, for a relay user who adds Timeways later. `gnomish-relay setup` adds the relay part to a Timeways config. Each checks every new text with the config loader before it writes. Neither changes an existing key, except the repair of `default_cwd` (12).
    - **A relay part with an error stops no Timeways setup.** `setup --timeways` checks only the parts of the config that it writes: `[wow]` and `[story]`. An error in the relay part (for example an `[agents.<name>]` entry with an unknown kind) is not its job. It sets up Timeways, keeps the relay part as it is, and prints one line: "Gnomish Relay: config.toml has an error, and the desktop app won't start until it's fixed: <error>". A plain setup still stops on such an error, because the relay part is its own.
    - **`## Group:`** is in the slots of neither app. Nothing shows yet that the Forever client reads it. After a test in the game, both apps get it in one commit.
16. **Life cycle.** `restart` and `update` also stop and start the story program. The bridge kills its process group when it exits. If the story program crashes, it starts again after a backoff.
17. **Versions.** The hello carries the version of each app. A version out of range gets the reply "update the addon".
    - Step 8 (decided with an advisor on 2026-09-26): each message of an app out of range gets one error reply with the text of 7.7.
    - A newer addon of either app gets "Update the desktop app: run gnomish-relay update.". An older Timeways gets "Update Timeways.". An older relay gets "Update Gnomish Relay in the CurseForge app, then restart WoW.". Until 2026-09-30, the bridge wrote the relay addon at each start, so an older relay got a reload text.
    - The range check is the pure function `version_fit` in `protocol`. It has unit tests for every edge and a check in the `flags` fuzz target. S30 proves it (14.1), approved by the user on 2026-09-26.
    - Each app sends its own number through `ns.App.version`, because the batch lines of Timeways and the coding flags of the relay change separately.
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
5b. Shared message logic moves from `GnomishRelay/Transport.lua` into `addon/transport/Messages.lua`, with `ns.App` parameters: the send queue, the signed outbox, retries, `next`, `read`, the hello, the slot poll with reply handling, and the health flags. So Timeways and the test addon share the logic that `models/transport.qnt` checks. The relay keeps chats, sessions, restore, live, and popups on top of it. (Done. No transport rule changed, so the model did not change.)
6. Model calls with no tools, and the budget.
7. The shared corner and its Quint model. (Done. `Strip.lua` takes turns through `GnomishStripCorner` (7.1.2), and `models/corner.qnt` checks the rules.)
8. Setup for two apps, and versions. (Done. Setup is in 9.7, decision 15, and the version range in 7.7, with S30.)
### 9.8 The app protocol

The bridge and the Timeways story program talk in JSON lines: one JSON object on each line, over the stdin and stdout of the story program. The story program is untrusted, like an agent, and so is the addon. `crates/app-protocol/src/addon_lines.rs` checks the addon lines, `crates/app-protocol/src/story_lines.rs` has the other messages, and `crates/bridge/src/story.rs` has the life cycle.

**Start.** The bridge starts the story program only when the Timeways lane is on (`timeways.key` exists) and the config has a `[story]` section with a `program` (12).

- The command line is `<program> <lore pack> <story folder>`. The lore pack is the SQLite file of the lore. The story folder is `<data>/timeways/story/`, which the bridge makes with mode 0700.
- The program path is absolute: the bridge never looks it up on `PATH`. The bridge starts its real path, with no link in it. It refuses a program inside a path that the sandbox hides (6.6.4).
- The bridge starts the program with no shell, the environment allowlist of 6.2 rule 12, the story folder as working folder, and the sandbox of 6.6.4.
- On Linux and macOS, the story program leads its own process group. On Windows, `taskkill /T` stops its process tree.

**What the story program writes.** Only files below its story folder, for example `worlds/<realm id>/<character id>.jsonl`. Each id is a safe encoding of a name from the game: `[A-Za-z0-9]` stays, and every other byte becomes `_XX` in hex (9.7, decision 19). The bridge writes every file that the game reads, with the proved writers (S9, S12). The story program never writes a slot file. Problems go to its stderr.

**Batches from the addon.** The Timeways addon sends one message for each batch, in its one chat. The message text is JSON lines, in this order: an optional `character_entered` line, then game events, then at most one line with a reply. The bridge checks each line:

- One JSON object, at most 4 KiB, with a string `type` of 1 to 32 bytes of `[a-z_]`.
- No `id` key. The bridge adds `id`, never the addon.
- No control character in any string or key. A line break is a control character, so an addon joins game text onto one line before it sends it. Nesting depth at most 4 (a flat object is 1). At most 64 keys in all.
- These types keep an exact shape (`deny_unknown_fields`, a key twice is an error):

| `type` | Fields | Checks |
|---|---|---|
| `character_entered` | `realm`, `name` | `realm` at most 64 bytes, `name` at most 48 bytes |
| `lore_asked` | `at`, `question`, `target` (optional) | `question` at most 1 KiB, `target` at most 128 bytes |
| `journal_asked` | `page` (optional, default 0) | |
| `talk_asked` | `at`, `npc`, `text` | `npc` 1 to 64 bytes, `text` at most 255 bytes |
| `draft_asked` | `at`, `idea` | `idea` at most 255 bytes: the player's idea for a quest of their own |

- `lore_asked`, `journal_asked`, `talk_asked`, and `draft_asked` are the lines with a reply.
- Any other `type` is a game event with no reply, for example `zone_entered`, `npc_met`, `level_reached`, and `npc_defeated`. The story program checks its fields, and ignores a type that it does not know. So a new Timeways event needs no change in the bridge.

The bridge drops and logs a line that fails a check. The bridge refuses the whole batch with an error reply, and sends none of its lines, in two cases:

- The character line is too long or holds a control character: "The realm or the name of the character is too long."
- The order is wrong: a second character line, a character line that is not first, or a line with a reply that is not last: "The lines of the batch are in the wrong order."

**Messages from the bridge.** The bridge makes each line from the checked value, never from the raw bytes of the addon.

| `type` | Fields | When |
|---|---|---|
| `hello` | `protocol` (the version of the bridge, now 1), `app` (`"timeways"`) | The first line after each start |
| each line of a batch | its own fields, and `id` (a number from 1 up, the same for each line of one batch) | For each batch that the lane marked as seen on disk |
| `batch_end` | `id` | After the last line of a batch with no line with a reply. A line with a reply ends its batch itself, so each batch gets exactly one answer line. |
| `model_answered` | `call`, `text` | The model answer to a `model_call`: at most 16 KiB, with no control character except newline and tab (9.7, decision 10) |
| `model_failed` | `call` | A `model_call` with no answer: no model, too many open calls, over the budget, a timeout, or a failed call |

The bridge sends each batch as soon as the story program is ready. A batch that waits for its answer never holds up the next one: answers match by `id`. The limits of each chat (the queue cap of S14, and 30 records in the body) bound the waiting batches.

**Messages from the story program.** Each line must have one of these shapes, with `deny_unknown_fields`. An unknown type, an unknown field, a missing field, a field twice, or a value of the wrong type refuses the line. The one exception is `journal`: its other fields are bounded JSON (below).

| `type` | Fields | Checks |
|---|---|---|
| `hello` | `protocol` | |
| `lore_answer` | `id`, `text` (a string or `null`), `passages` (a list of `text` and `source`), `narrator` (optional), `notice` (optional) | `text` at most 8 KiB, at most 8 passages, each `text` at most 4 KiB and each `source` at most 512 bytes. No control character except newline and tab. |
| `journal` | `id`, `page`, `pages`, `narrator` (optional), `notice` (optional), and any other fields | `page` and `pages` are integers from 0 up, and `page` is below `pages` unless `pages` is 0. The other fields are bounded JSON (below). |
| `talk_answer` | `id`, `npc`, `text` (a string, or `null` when no model answered), `narrator` (optional), `notice` (optional) | `npc` at most 64 bytes. `text` at most 1600 bytes (400 characters), on one line, with no control character. |
| `draft_answer` | `id`, `draft` (an object, or `null` or missing when the story program makes no quest of the idea), `narrator` (optional), `notice` (optional) | The draft is `title`, `text`, and `steps` (a list of `goal` and `target`), with no other field. Limits below. |
| `events_seen` | `id`, `narrator` (a string or `null`), `notice` (optional) | The answer to a batch of game events only |
| `model_call` | `call`, `prompt` | `prompt` at most 256 KiB. |

**Model calls.** A `model_call` can come at any time, also when no batch waits. An example is the bard call for a saga after `events_seen`.

- The bridge ties the call to no batch and answers it by its `call`. A call that belongs to no batch gives no reply to the game.
- At most 2 model calls of the story program are open at once (one of the narrator, one of the bard). A third one gets `model_failed` at once.
- A call stays open while the model runs, off the main loop, until its answer, its timeout, or a stop of the story program.
- The budget, the size limits, and the two routes (Claude and a local model) are in 9.7, decision 10.
- With no model in `[story]`, every call gets `model_failed` at once, and the story program still answers with its passages.
- The story route never reaches an agent, a job, or a chat of the relay (9.7, decisions 4 and 18).

**The journal is bounded JSON.** The story program journal grows often, for example with chapters, the trust of a person, and new kinds of deeds. So only `type`, `id`, `page`, `pages`, `narrator`, and `notice` of a `journal` line have a fixed shape, and a new field needs no change in the bridge. The limits on the other fields:

- Only objects, arrays, strings, integers, `null`, and booleans.
- The line is depth 1, and the depth is at most 6.
- Each string is at most 1600 bytes, with no control character. Each key is 1 to 32 bytes of `[a-z_]`.
- An object holds at most 64 keys, and an array at most 200 items.
- A `note` key is refused, because the note of a reply belongs to the bridge.
- A string over its limit is a text error. Every other failed check is a shape error.
- The whole line is at most 24576 bytes, as for every answer.

The bridge writes the journal again from the checked value, with every `|` doubled (S10).

**The draft of a quest.** A `draft_answer` answers a `draft_asked`: a quest that the player asked for, to accept in the addon.

- Its limits count bytes, as the journal does, at 4 bytes for each character, as for `talk_answer`. A `title` of 60 characters is at most 240 bytes. A `text` of 600 characters is at most 2400 bytes. Each `goal` and `target` of 64 characters is at most 256 bytes.
- A draft has at most 6 steps, and it can have none.
- The `text` keeps its newlines and tabs, and has no other control character. The `title`, each `goal`, and each `target` have no control character.
- A draft over a limit refuses the whole line, as a lore answer with too many passages does. It is a bad line, and the batch waits for a good answer until its timeout.
- The bridge doubles every `|` in each of the four texts (S10).

`narrator` is a line of the narrator, the voice of the chronicle. It is at most 1000 bytes, with no control character. The bridge drops and logs a longer one, or one with a control character, and keeps the rest of the answer.

`notice` is a line of Timeways itself, not of the story, for example "You already have 3 tasks. Finish one first.". The addon shows it with the prefix "Timeways:" in place of the narrator prefix. It has the limits of `narrator`, and the same rule for a line over them. A missing or `null` `notice` means no notice, so a story program that never sends one works as before.

**Replies.**

- A batch with a `lore_asked`, `journal_asked`, `talk_asked`, or `draft_asked` line waits, for the request timeout, for the `lore_answer`, `journal`, `talk_answer`, or `draft_answer` with its `id`. A batch of game events only waits for `events_seen`, for 60 seconds (or the request timeout, if that is shorter).
- The done reply is one JSON line that the bridge makes from the checked answer. It has no `id`, and always has `narrator` (a string or `null`): for example `{"type":"events_seen","narrator":null}`. It has `notice` only when the answer has one, after `narrator`. The bridge doubles every `|` in each text of it (S10), so the game shows the text as it is. The addon shows these texts with no escape of its own.
- A batch of game events never gets an error. It gets a done reply with an empty text at its deadline, or when the story program stops, is refused for its version, or does not start.
- An answer for an `id` that already ended is late, and the bridge drops and logs it. Examples: an `events_seen` after its deadline, or a second answer line with the same `id`.
- An answer of the wrong type for its batch is a bad line, and the batch goes on waiting. Examples: a `talk_answer` for a `lore_asked`, or an `events_seen` for a batch with a line with a reply.
- An answer line of more than 24576 bytes gets the error reply "The Timeways answer is too long for the game.", never a cut line. So does a reply that the slot writer would cut (S12). A reply record holds at most 32 KB after the Lua escape. The Lua escape writes 4 bytes for some bytes, so the bridge checks the real size too.
- An error reply is plain text.

**Rules:**

- A line from the story program is at most 1 MiB. The reader skips the rest of a longer line.
- A line that fails a check is a bad line. The bridge logs and skips it. An answer for an `id` that the bridge never gave is a bad line too. More than 10 bad lines in one run of the story program stop it.
- **Handshake.** The story program answers the `hello` of the bridge with its own `hello` within 10 seconds (or the request timeout, if that is shorter). Any other first line is a bad line. No `hello` in time stops the story program.
- **Versions.** The bridge compares the versions, not the story program. A story program with a higher `protocol` gets "Update the desktop app: run gnomish-relay update." as the answer to each batch. A lower one gets "Update Timeways.". The bridge stops it, logs both versions, and does not start it again until the bridge restarts: a restart cannot fix a version.
- **Timeout.** A batch with a reply line has the timeout `[story] timeout_seconds` (default 120 seconds), from the time that the lane gives it to the story program. With no answer in time, it ends with "The Timeways story program did not answer in time.". A sent batch of this kind with no answer in time means a hang, and the bridge kills the story program. A batch of game events with no `events_seen` in time is no hang.
- **Stop.** When a hang, a crash, or too many bad lines stop the story program, the bridge kills its process group. Each sent batch with a reply line then ends with "The Timeways story program stopped.". Each sent batch of events ends with an empty done reply. A batch that is not sent yet waits for the next start.
- **Restart.** After a stop, the bridge starts the story program again after 1 second, then 2, 4, and so on up to 60 seconds. After a run of 60 seconds or more, the wait starts at 1 second again.
- **End of input.** The story program exits when its stdin closes, because then the bridge is gone. With `bwrap`, `--die-with-parent` also stops it. `gnomish-relay restart` and `update` restart the bridge, so the story program starts again with it. A systemd service stops the whole control group. On macOS and on a Linux with no `bwrap`, a bridge that a signal kills cannot kill the process group. The bridge has no signal handler (it forbids `unsafe`). There, the story program depends on the end-of-input rule.
- **No sandbox.** With no sandbox (6.6.4), the first reply with text after the bridge starts carries the warning: a `note` field in a JSON reply, or a second line in an error reply.
- **Logs.** Each log line of the story program starts with `timeways:`. After a crash, the last line of its stderr goes into the log, with the escapes of 6.2 rule 15.

The tests use a fake story program, `crates/bridge/src/bin/fake-story.rs`, with these scripts:

- echo: a `lore_answer` of "story: <question>", the journal with a chapter and the three kinds of deeds, a `talk_answer`, a `draft_answer`, and `events_seen`
- a `null` text, a `null` draft, two answers with one `id`, three bard calls after `events_seen`
- `events_seen` and answers with a narrator line, and with one that is too long; the same with a notice
- a late `events_seen`, a missing `events_seen`
- crash, crash once, garbage lines, a flood of bad lines, a huge line, an answer line one byte over the limit, a hang, no hello
- a higher and a lower version, answers for unknown ids, an answer for another id, an answer of another type
- a model call whose answer becomes the lore text, a model call and then a crash
- its environment, a child process, and probes of the sandbox

It writes each line that it gets into `seen.txt` in its folder, and the `id` of each `batch_end` into `ends.txt`.

The model calls have their own tests (`crates/bridge/tests/model.rs`). A fake model server (`tests/fake_model/`) is a thread with a `TcpListener` on 127.0.0.1. It answers like the OpenAI-compatible API: a normal answer, a slow answer, a huge answer, garbage, a redirect, a 500, and control characters. The tests show three things: `curl` never follows the redirect, the huge answer fails, and a proxy in the `curl` environment gets no connection. The fake `claude` does three things: it tries a tool on the story route and gets the deny of the gate; it runs a tool with no hook and stops the call; it reports its folder, arguments, and environment. Two `#[ignore]` live tests run the real `claude` with a prompt that asks it to read a file (the answer never holds the file text), and a real Ollama when one listens on 127.0.0.1:11434.

**The end-to-end test** (`crates/bridge/tests/timeways_e2e.rs`) runs the real bridge with the real `timeways-story` of a Timeways checkout, with no game.

- The bridge starts the story program from a `[story]` config, in its real sandbox.
- The test builds a small lore pack from invented passages with the real `timeways-pack`.
- The test addon of the fake game sends batches that `Json.lua`, `Inputs.lua`, and `Outbox.lua` of the Timeways addon make. First the character, four game events, and a question. Then a journal request, then a talk, then game events only.
- The Lua slot poll reads each reply.
- The test checks the passages and their sources, the spoiler limit, the journal, the done reply of the events, the failed model calls with no model, the story program files, and an empty relay lane.
- A second case asks the real `claude` for words.

Both cases are `#[ignore]`, and they skip with no Timeways checkout, so CI does not run them yet. To run them:

```sh
scripts/e2e-timeways.sh            # TIMEWAYS_REPO is the checkout; the default is ../timeways
scripts/e2e-timeways.sh --claude   # also the case with the real claude
```

The script builds `timeways-story` and `timeways-pack` into `target/timeways`, so it never shares a build folder with the Timeways repo.
### 9.9 Choose the folder of a chat

A new chat starts in `default_cwd`, and the folder browser opens at once. The player picks the folder first, because the default folder often holds all the projects, and an agent there works on the wrong one. Escape or Cancel keeps `default_cwd`. The folder in the chat header is a button that opens the folder browser in the center of the window (13.1). The browser shows a tree of the folders inside `allowed_roots` and the home folder (9.12).

**The request.** Each time the browser opens, the addon sends a `list=folders` record of the chat `folders`. The browser shows the last tree at once, and a small spinner turns until the new tree comes. The bridge walks the roots in a thread, off the main loop (`folder_walk.rs`), and answers with the tree (`folder_list.rs`).

**The reply.** The first line is `default_cwd` as the player reads it. Then comes one line per folder, each after the line of its parent:

```
parent \t name \t mark
```

- `parent` is the line number of the parent folder. The first folder is on line 1. A root has parent 0.
- `name` is the folder name. A root has its whole path as its name.
- `mark` is `g` for a git repository, else empty.
- A line `?` with a list of line numbers names the folders whose subfolders the reply does not hold in full, for example `?812-2950,3001`. A number is a folder line number, as in `parent`, and a range includes both ends. The line comes after the last folder. With no such folder, the line is not there.
- A line `~`, a tab, and the home folder in the form of the other paths (`~`, or the whole path) says that the bridge takes the home form (below). It comes after the `?` line. A bridge with no home folder leaves it out. An older addon skips it, because it is not a folder line.
- A last line `+` says that the tree is cut.
- A path in the home folder starts with `~/`, for example `~/Documents/Code`, but only when `default_cwd` and every root are in the home folder. Else every path is whole. So the addon can compare the parts of any two paths.

**The folder of a line.** The addon joins the root path and the names down to the folder. The folder that the game sends back is its **home form**: `~/` and the path from the home folder to the folder, with `..` for each step up. For example `~/Documents/Code/Personal/sandcastle`, or `~/../../srv/code` outside the home folder. The home folder itself is `~`.

The home form never depends on `default_cwd`, so a saved chat keeps its folder when `default_cwd` changes. It holds no drive letter, so Windows can send it (7.1.1). One folder always has one text. The bridge resolves it with the resolver (S5), with the home folder as the base, to the same folder. The roots, the rules of 9.12, and the check of the real folder at the start of the run do not change.

- **Only with the `~` line.** The addon sends the home form only with a tree that has the `~` line. An older bridge sends no such line, so it never gets a home form.
- **The old form.** The path from `default_cwd` to the folder, with `..` for each step up, as before 2026-09-30. The empty text is `default_cwd`. The addon sends the old form with a tree that has no `~` line. On Windows, it also sends the old form for a folder on a drive other than that of the home folder. The bridge still takes the old form, because an older addon sends it, and older chats hold it.
- **An old text after a change of `default_cwd`.** An old text resolves against `default_cwd`. The bridge takes the folder relative to the home folder when all of these are true: no folder is at the old text, the text has only plain names (no `..`, `.`, or `/` first), and the folder relative to the home folder exists. Setup writes `default_cwd = "~"` (9.12, decision 5) and repairs to it (12), so the home folder is the usual old base. A new folder (`mkdir=1`) does not exist yet, so it always resolves against `default_cwd`. The home form can already name any such folder, so this step reaches no new folder.
- **The addon rewrites old texts.** When a tree with the `~` line comes, the addon gives each saved chat with an old text its home form. It takes the tree folder that the text names from `default_cwd`, else from the home folder, the same order as the bridge. A text that names no tree folder stays as it is, and the bridge still resolves it. The empty text of a chat in the default folder becomes the home form of `default_cwd`, so the chat stays there when `default_cwd` changes.
- **A folder that is gone.** The run ends with "Can't find ~/Documents/Code/x. Pick another folder." The path is the folder that the bridge looked for, with `~/` in the home folder, so a wrong path shows. Control characters become spaces.

Bug of 2026-09-30. The game saved the folders relative to `default_cwd`. The user changed `default_cwd` from `~` to `~/Documents/Code`, and the desktop app restarted. Then the saved text `Documents/Code/Personal/sandcastle` pointed at `~/Documents/Code/Documents/Code/Personal/sandcastle`. Every saved chat ended with "This chat's folder is gone.", although each folder existed. The bridge never saved its old base, so a remembered base cannot fix the chats of that day. The step back to the home folder fixes them with no user action.

**The walk.**

- Breadth first, with sorted names, so a limit cuts off the deepest folders, and the order is the same on each run.
- At most 4 levels below a root. A root is level 0.
- The walk goes into a repository, so the browser can show its subfolders.
- It never follows a symbolic link or a Windows junction. Two roots that overlap give each folder once. A root stays a root.
- It skips hidden folders (a name that starts with `.`) and `node_modules`, `target`, `build`, `dist`, `vendor`, `venv`, `__pycache__`, `Library`, and `AppData`.
- **The home folder** (9.12). After the roots, a second walk goes through the home folder with the same rules: at most 3 levels below it, at most 1000 folders, and 1 second.
  - It never goes into a root, because the first walk has it.
  - It asks the classifier with the home folder as the only root. So, as in the first walk, only a folder that the classifier reads with no question shows.
  - It always goes down to each root inside the home folder, also below 3 levels, so each such root hangs in the home folder tree. A root outside the home folder stays a root of its own.
  - With no home folder, only the roots show.
- The order of the reply: the home folder, then the folders on the way down to each root, then the root folders breadth first, then the other home folder folders breadth first. So the size cut takes the home folder folders first, and the projects of the roots stay.
- It reads at most 3000 folders, and stops after 2 seconds. A stop at one of these two limits cuts the tree. The depth limit does not.
- **Not walked.** A folder is in the `?` line when the walk did not list all of its subfolders. That is, it is at the depth limit, a visit or time limit stopped the walk before its subfolders, or the size cut left out one of them. A folder that does not show, or that the game cannot send back, does not count: the browser never shows it. An older addon skips the `?` line, because it is not a folder line. A mark on each folder line would hide the folder in an older addon (9.12, decision 4).
- A repository is a folder with a `.git` entry: a folder, or a file as in a worktree. The walk never reads the `gitdir:` line of such a file, because it can lead out of the roots.
- The walk asks the classifier (6.6.3) for a read of each folder, with the folder as the chat folder. Only a folder with the answer `allow` shows, and a folder that does not show hides its subfolders. So the config folder and the data folder of the bridge (`deny`) and credential folders such as `snap/firefox` (`desktop`) never show. This is a filter of the tree, not a wall: the classifier still checks every tool call in the chat.
- The walk leaves out, with its subfolders, a folder that the game cannot send back: a relative path with a control character, a path that is not UTF-8 or longer than 255 bytes, a name that fails the name rules below, or a `:` on Windows (7.1.1).
- The walk never fails. It leaves out a folder that it cannot read.

**The size cut.** The reply is one record, at most 32 KB after the Lua escape (S12). There, a tab, a newline, and a byte that is not printable ASCII cost 4 bytes. The bridge keeps the longest start of the reply that fits, and adds the `+` line. So the shallow root folders always come. The bridge keeps 1 KB of the record for the `?` line. A longer `?` line becomes one range, from the first folder that is not walked to the last folder line. A folder in the range that has all its subfolders only costs one more request.

**One folder.** When the player goes into a folder of the `?` line, the browser asks for its subfolders. The request is a `list=subfolders` record of the chat `subfolders`, with the folder in the `cwd` field, in the form that the game sends back. The addon sends it only for a tree with a `?` line, so an older bridge never gets it.

- The relay takes the folder when it is inside a root, or inside the home folder with no part below the home folder that the walk skips (9.12, rule 2). The home folder itself is fine here, because the request only reads names. Any other folder ends the request with the error "That folder isn't in your folder list."
- The bridge resolves the folder with `canonicalize`, and checks it again, so a link cannot lead out.
- The walk starts at the folder, with the rules and limits of the root walk: the same skipped names, the same depth, visits, and time, no links, and the classifier filter. The classifier gets the roots, or, for a folder under no root, the home folder as the only root, as in the home walk. A folder that fails a check gives a tree with no folders.
- The reply has the form and the size cut of the tree, with the folder as its only root line, and its own `?` line.
- The addon adds the reply folders under the tree folders with the same path. It leaves out, with its subfolders, a folder that the tree does not have, so the reply never widens the tree. A reply folder takes its `?` mark from the reply. The added folders stay until `/reload`.
- The addon asks at most once for each folder while the browser is open. The spinner turns while the request waits.

**The browser.** It is a panel with a gold title, "Pick a project folder". It says at once what it is for and what to do.

- **Search.** A search box under the title has the focus when the browser opens. While it is empty, it shows the grey hint "Search folders".
  - It matches the tree folders, as the game sends them back, by subsequence and without case. Repositories come first, then the shorter paths, at most 16 rows.
  - Each row shows the name, a `git` mark for a repository, and the parent folder in grey at the right.
  - Escape clears the focus and closes the browser, so the game keys work again.
  - A typed text is only a filter, never a path.
- **Recent folders.** With an empty search, the browser shows at most 5 recent folders: the folders of the newest chats, then the folders of the Resume list (9.6). They need no request. A folder that the last tree does not have is gone, and it does not show.
- **Breadcrumb.** Below them is a breadcrumb, for example `Code › Personal › gnomish-relay`. The last part is white: the folder that the player is in. The parts before it are light grey. Under the mouse, a part turns gold and gets a highlight, and a click on it goes there. The first part is the top of the tree: the home folder (`~`), or a root outside it. The player cannot go above it. With more than one top, the first part is "All folders", and it lists them.
- **Subfolders.** Then come the subfolders of the current folder. The chat folder is green. When the current folder has no subfolders, a grey line says "No subfolders.", and **Chat here** and **New folder** stay. While the request for one folder waits, the line says "Loading folders..." instead.
- **Go into a folder.** Every folder row has a gold arrow button at the right: recent, subfolder, and search rows. A double-click on a row, or a click on its arrow, goes into the folder, also when the tree knows no subfolders of it. A recent or search row also clears the search. A single click never goes anywhere.
- **Back.** A **‹ Back** button is at the left of the search box. It goes to the folder above. From a top, it goes to "All folders" when there are more tops. At the top, it is grey and does nothing. While a search has text, Back clears the search.
- **Keys in the search box.** While the search box is empty, Backspace and Left go up, as Back does, and Right goes into the highlighted folder. With text, these keys edit the text. Up, Down, Enter, and Escape work as below. The keys work only while the search box has the focus, so the chat input and the game keys never change. A click on a row gives the focus back to the search box.
- **Choose a folder.** A click on any folder row highlights it. Up and Down move the highlight. A search highlights its first match. Enter in the search box, or **Chat here**, starts the chat in the highlighted folder. With no highlight, they take the current breadcrumb folder. With nothing to open, Enter only clears the focus.
- **Buttons.** At the bottom are **New folder** at the left, then **Chat here** and **Cancel** at the right. **Cancel** does what Escape does.
- **New folder** shows an edit box as the last row, in the current folder.
  - The addon checks the name: not empty, `.`, or `..`; no `/`, `\`, or control character; at most 255 bytes; and no subfolder there with the same name (without case).
  - A refused name shows a short reason in red.
  - Enter sets `<current folder>/<name>` as the chat folder, and the header marks it "new". Escape in the edit box hides it.
- **Never a silent empty list.** A grey line under the rows says why the list is empty:
  - No tree yet, while the bridge state is "checking" or a list request waits: "Loading your folders..." The spinner turns.
  - No tree yet, and the desktop app is offline: "The desktop app isn't running, so your folders can't load. On your desktop, run gnomish-relay restart." The spinner stops, because no answer comes.
  - No tree yet, and the list failed: the error in red.
  - A tree with no folder: "No folders found. Click Cancel to chat in your default folder."
  - A search with no match: "No folder matches."
- The browser opens at the chat folder, or at the default folder when the tree does not have it.

**The chat.**

- A choice sets the chat folder. The chat takes the folder name, with " 2", " 3", and so on when another chat has the name. A chat in the default folder keeps its "Chat N" name.
- The first message fixes the folder (9.5). Before it, the browser button says "Chat here". After it, the button says "New chat here", and each choice makes a new chat in the chosen folder, with the agent of the chat.
- The header shows the folder as the player reads it, with the folder icon and the dropdown arrow. Before the first tree, it shows the relative folder, and nothing for the default folder.

**A new folder.** The first message of a chat in a new folder has the flag `mkdir=1` next to `n`. The record folder is the new folder. The bridge makes it before the run starts, so a chat that never sends leaves no empty folder.

- The bridge takes `mkdir=1` only with `n`. On any other message, the flag does nothing.
- The relay refuses a new folder whose record folder is absolute, or whose last part fails the name rules. The reply is "Couldn't create the folder: the name can't contain / or \. Pick another name." The folder check of 6.2 rule 1 comes first, as for every message.
- When the run starts, the bridge makes only the last part, with `create_dir`, never `create_dir_all`. The parent must exist. The bridge resolves the parent with `canonicalize` and checks the roots again, so a link in the path cannot lead out (6.2, rule 10). The new folder must get `allow` from the classifier, as in the walk, so the bridge never makes a folder inside a `deny` folder or a `desktop` path.
- A folder that is already there is fine, because a run after a bridge restart asks again. A file with that name is an error.
- An error ends the message with a reply that starts "Couldn't create", and the run never starts. Each refusal has its own reason.

**Decisions.** The implementer and the coordinator chose these (2026-09-26). The advisor agent did not answer in time, so the coordinator gave the defaults.

1. **`list=folders`, in its own chat `folders`.** The addon routes the reply by chat, so a reply after `/reload` still finds its cache. The bridge runs one job per chat, so a session list and a folder list can run at the same time.
2. **No version change (7.7).** At that time, the bridge wrote the relay addon again at each start, so the addon was never newer than its bridge. A version change would also change the proof of S30.
3. **A compact tree, not one full path per line.** A name costs fewer bytes than a full path, so more folders fit in 32 KB. The parent line numbers only point back, so the parser can never build a loop.
4. **The addon makes the text of a folder from the parts of two paths.** Else, a root inside the home folder with `default_cwd` below it gives two texts for one folder, for example `../../Documents` and `..`. Then the green mark, the recent folders, and the names disagree.
5. **A breadth-first cut with a mark.** A player opens the shallow folders first. The addon shows the cut tree, and the breadcrumb and the filter still work on it.
6. **One breadcrumb root per root.** A player with one root never sees a list of roots. With more roots, the list of roots is the top.
7. **The header shows the path as the player reads it** (`~/Documents/Code`). The old header showed the relative folder, and nothing for the default folder, which a button cannot show.
8. **`mkdir=1` with the chat folder, on the first message only.** The record already carries the folder, so a second field only repeats it. A later message cannot make a folder, so a lost reply never makes one twice in another place.
9. **The filter matches the path from `default_cwd`.** It was the text that the game sends back before the home form, and it has no common start that matches every query. Repositories first, because most chats work in a repository.
10. **Recent folders drop a folder that the tree does not have.** A removed folder only gives an error.
11. **The walk goes into repositories now.** The browser needs their subfolders. The limits stay the same.
12. **The classifier is the filter** of the walk and of a new folder, so the tree, a new folder, and the tool calls never disagree about a folder.
13. **The tree stays text in the saved variables** (`folders.text`). A parsed tree has loops through the parent links, and the game cannot save a loop.
14. **No Refresh button.** Each open asks for a new tree, and the spinner shows the wait.

**Decisions of 2026-09-30.** In a test in the real game, a new chat showed only an empty box with a small icon. The desktop app was not running, so the list was empty, and nothing said what the box was for.

15. **A title, a hint, and buttons.** The panel says what it is for. Open, New folder, and Cancel are where most folder pickers have them.
16. **A click highlights, a double-click opens.** Before, one click on a recent folder set the folder, and one click on a subfolder went into it. One action with two results confused the player. Now one click never starts a chat. The `›` goes into a folder, as in a tree.
17. **"New folder" is a button, not a row.** As a row, it looked like a folder.
18. **A line for each empty state.** An empty list always shows a reason and a next step.
19. **The empty tree does not say "pick a folder from your home folder".** The home folder is in the tree when it exists (9.12). So an empty tree has no home folder to pick from, and only the default folder is left.

**Decisions of 2026-09-30, after a second test in the game.** The user could not get into a folder. A double-click started the chat, the `›` was small and showed only on some rows, and the breadcrumb did not look like a button. In the user's words: "I want to expand the folder and navigate into a subfolder", and "I need an easy way to go back". So the browser now works as the "Select folder" dialog of Windows and macOS.

20. **A double-click goes into a folder.** Every file explorer does this. This replaces decision 16 for the double-click: one click still only highlights.
21. **An arrow on every folder row.** A folder at the depth limit looked empty, so the arrow showed nowhere useful. Now the arrow shows on each row, and an empty folder says so.
22. **Back at the top left, and keys.** The player always sees the way out. Backspace and Left work only in an empty search box, so a typed search keeps its normal keys.
23. **"Chat here", not "Open".** In a file explorer, Open on a folder goes into it. "Chat here" names the goal, and it matches "New chat here".
24. **A request for one folder, not a deeper walk.** A deeper walk does not fit in one record, and the size cut takes the deep folders first. So the bridge lists a deep folder when the player goes into it. The `?` line says which folders need it, so a folder that the tree has in full costs no request and no slot.

**The browser** is in 13.1.
### 9.10 Cost and usage

A player in the game cannot see the bill of an agent. So the bridge records the tokens of each run, and its cost when the agent gives it (asked for by the user on 2026-09-29).

**What each agent reports.** Checked against Claude Code 2.1.285 and codex-cli 0.157.0 (`codex app-server generate-json-schema`).

| Agent | Where | Tokens | Cost |
|---|---|---|---|
| `claude` | The `result` message at the end of the turn: `usage` and `total_cost_usd`. | In: `input_tokens` + `cache_read_input_tokens` + `cache_creation_input_tokens`. Out: `output_tokens`. Cached: `cache_read_input_tokens`. | `total_cost_usd`, only for a run that pays with an API key (below) |
| `codex` | The `thread/tokenUsage/updated` notification, after each model call of the turn. `tokenUsage.total` counts the whole thread, and `tokenUsage.last` the last call. | The turn is the newest `total` less the `total` before the first call of the turn (the first `total` less its `last`). In: `inputTokens`, which includes the cached ones. Out: `outputTokens`. Cached: `cachedInputTokens`. | none |
| `acp`, `command`, `echo` | none | none | none |

- A run with no report records and shows nothing. So does an attach (9.6), because it calls no model.
- A number that is missing, negative, or not a number counts as 0. A cost that is not a finite number of at least 0 counts as no cost.

**A cost only for an API key.** The user decided on 2026-09-30: "showing the dollar cost of the Claude LLM usage should only be when it's used via API on a per-credit charge basis. An LLM subscription should not show cost, just token usage." Claude Code gives `total_cost_usd` also for a Claude Pro or Max login. Then the number is only an estimate at API prices, and the user does not pay it.

- The bridge reads `apiKeySource` of the `system` message with subtype `init`, which Claude Code sends at the start of each turn. It is the source of the API request credential, never the credential itself. The bridge reads no credential file.
- The values, checked in the init message schema of Claude Code 2.1.286:
  - `ANTHROPIC_API_KEY` (the variable), `apiKeyHelper` (the helper command of the settings), and `/login managed key` (a key of an Anthropic Console account).
  - `none` (no API key: a claude.ai login, a bearer token, or a cloud provider).
  - `user`, `project`, `org`, `temporary`, and `oauth` are old values that current versions never send.
- A run keeps its cost only when `apiKeySource` is `ANTHROPIC_API_KEY`, `apiKeyHelper`, or `/login managed key` (test `an_api_key_run_keeps_its_cost`).
- With `none`, the run has its tokens and no cost (test `a_subscription_run_has_tokens_and_no_cost`). Its line is "4.7k in · 350 out".
- With an old value, another value, or no `apiKeySource`, the bridge is not sure, so the run has no cost (test `a_run_with_no_key_source_has_no_cost`). A wrong cost is worse than a missing one.
- The rule applies where the report enters the bridge. So a run with no cost shows none in block `u`, and adds no cost to `usage.json` or to the total for today in the Settings tab. It never raises the total toward the daily cap.
- A reply saved before this rule keeps its line, and `usage.json` keeps its old costs. The bridge does not rewrite them.

**In the game.** A `done` reply with a report carries the line of block `u` (7.3.1), for example "1.2k in · 350 out · $0.04". Codex and a Claude subscription give no cost, so their line is "1.2k in · 350 out". The addon shows the line in grey below the reply, and never in the whisper line.

- A count below 1000 shows as it is. Up to 999,999, it shows in thousands with one decimal ("1.2k", and "12k" from 10,000). From a million, it shows in millions ("1.2M").
- A cost shows with two decimals ("$0.04"). A cost above 0 and below one cent shows as "<$0.01".
- An error reply shows no line, but its report still counts for the day.

**The total for today.** The bridge adds each report to the total of its day, in `usage.json` in the data folder, with mode 0600. A day is the UTC date, because the bridge has no time zone database. The file keeps the last 31 days. A damaged file logs one line and starts a new one. The total only informs, and a lost total never runs a message twice.

- The settings list (13.4) carries the total for today, and the cap when the config sets one. The Settings tab shows "Today (UTC): 12k in · 4.1k out · $1.20", and " · limit $5.00" with a cap.

**The daily cap.** `daily_cost_cap_usd` in the config (12) is off by default. When the cost of today reaches the cap, a new message does not start its agent. Its reply is the error "Not started: today's agent cost reached your $5.00 limit. It resets at 00:00 UTC, or raise daily_cost_cap_usd in config.toml on your desktop."

- The check comes when the message starts. A run in progress goes on past the cap, because a stop in the middle of a task leaves half-changed files.
- Only a cost counts. Codex and a Claude subscription report no cost, so their runs never raise the total. The cap still stops a Codex message when the cost of other agents reached it.
- A list and an attach never call a model, so the cap never stops them.

### 9.11 Git in a chat

Asked for by the user on 2026-09-29, designed by the implementer. Three parts:

- A chat can work on its own branch in its own copy of the repository.
- Each run ends with a summary of its changes, with Commit and Revert.
- The summary shows the tests and the CI checks.

The trust level of each action is in 6.6.6. The code: `git_host.rs` (git on the host), `chat_branch.rs` and `chat_merge.rs` (the own branch), `run_git.rs` (git around a run), `run_changes.rs` and `run_actions.rs` (the change summary, Commit, and Revert), `git_blocks.rs` (the blocks of 7.3.1), `git_actions.rs` (the actions from the game), `test_summary.rs`, and `ci_checks.rs`.

**Git on the host.** The bridge runs `git` with no shell, in the chat folder, with these settings:

- `-c core.hooksPath=<an empty folder>`, `-c core.fsmonitor=false`, and `-c core.untrackedCache=false`. The empty folder is in a private temp folder of the bridge.
- `GIT_TERMINAL_PROMPT=0`, `GIT_OPTIONAL_LOCKS=0`, `GIT_LITERAL_PATHSPECS=1`, and `GIT_EDITOR=true`.
- A commit also gets `--no-verify`. A diff uses `diff-tree`, which runs no `textconv` or external diff.

At start, the bridge runs `git --version` and reads the major and minor number. If git is missing or older than 2.38, there is no summary and no own branch, and the log says why. (2.38 brings `merge-tree --write-tree`.) Every git action from the game then gets "Git isn't available to the desktop app. Install git, then run gnomish-relay restart."

#### Own branch

Today, two chats in one repository edit the same files at the same time. With **Own branch**, a chat works in a linked worktree of the repository, on a branch of its own.

**The choice** is per chat, at New chat. The chat header shows a check box "Own branch" while the chat has no message and its folder is a repository or a folder inside one (the `g` mark of 9.9). It is on when another chat with a message already has the same folder, else off.

- Why not always on: a worktree is a full checkout, and the build folders (`target`, `node_modules`) are not shared, so the first build there starts from nothing. One chat in a repository gains nothing for that cost.
- Why not always off: two chats in one folder is the case that breaks, and the default turns on exactly then.

The chat keeps the choice, and every message of the chat carries `branch=1` (7.1.1).

**The worktree** comes at the first run of the chat, not at the click, so a chat that never sends leaves nothing. A message that waits for the limit on parallel runs (8.2) makes no worktree until its run starts. In a folder outside a repository, `branch=1` does nothing, and the chat works in its folder.

- The bridge asks git for the top of the repository of the chat folder, the branch that its `HEAD` names (the start branch), and the commit of `HEAD` (the start commit).
  - A repository with no commit refuses the run: "This repo has no commits yet, so the chat can't have its own branch. Make a first commit, or start a chat without Own branch."
  - A detached `HEAD` has no start branch. The chat works, and Merge says that it has no branch to merge into.
- The branch is `gnomish/<name>`. `<name>` is the chat name in lower case, at most 40 bytes, with each run of characters other than `a-z` and `0-9` as one `-`. It is `chat` when nothing is left. A name that a branch or a folder already has gets `-2`, `-3`, and so on.
- The folder is `<the folder above the repository>/.gnomish-worktrees/<repository name>/<name>`. Why there:
  - Outside the repository tree, so the cargo or npm workspace, `rg`, an IDE, and the sandbox walk of a chat in the repository never see a second copy inside it.
  - Next to it, so it is inside `allowed_roots` whenever the repository is not a root itself.
  - Hidden, so the folder browser skips it (9.9).
- When the folder above is outside every root, the run stops: "Couldn't give this chat its own branch: the folder above <repo> isn't in allowed_roots. Add it in config.toml, or start a chat without Own branch."
- The bridge runs `git worktree add -b <branch> <folder> <start commit>`. That is the only change in the `.git` of the repository: the branch, and the git folder of the worktree under `.git/worktrees/`. The bridge adds no other file there.
- The chat folder of the run is the worktree, or its subfolder when the chat folder is a subfolder of the repository. It passes 6.2 rule 10 again. Every rule that names the chat folder takes it: the classifier (6.6.3), the sandbox and its walk (6.6.4), the "Always allow" rules (6.6.5), and the agent session (9.5).
- `state.json` keeps the worktree of each chat: the chat, the top of the repository, the worktree, the chat folder, the branch, the start branch, and the start commit. A later run uses it. When the worktree folder is gone, the bridge forgets it, and the run makes a new one.

**Git inside the sandbox.** The git folder of the worktree is under `<repository>/.git/worktrees/`, outside the chat folder. The sandbox keeps each path outside the chat folder read-only. So a command reads the history and the diffs of the branch, but cannot commit, merge, or move a branch. This also keeps the `commondir`, `config`, and hooks of the repository out of reach. The **Commit** button commits (below).

- The `.git` file of the worktree is a `.git` entry of the chat folder, so the sandbox pins it read-only (6.6.4).
- A file tool write to the git folder is outside the chat folder and has a `.git` part, so it is `desktop` (6.6.3).
- A chat in the repository itself walks `.git/worktrees/<name>/` as a git folder, with the guards of 6.6.4. Its real `commondir` exists, so it is pinned and never removed.

**The link check** (fixed on 2026-09-30; the tests came first). The walls of a run pin only the `.git` entries that exist when the run starts. With parallel runs, another chat can run in the repository, or in the folder above it, while a copy is new. Its agent can then rewrite the `.git` file of the copy, or the `commondir` or `gitdir` of `.git/worktrees/<name>/`. It can point the copy at a git folder whose `config` sets a filter driver. The overrides of the host git (hooks, fsmonitor) do not cover a filter driver, and the next `add -A` on the host runs it.

So before each git call of the bridge on a copy, the bridge checks the link both ways:

- `<copy>/.git` names a folder right under `<the common git folder>/worktrees/`.
- Its `commondir` names the common git folder, and its `gitdir` names `<copy>/.git`.
- Each file must be a regular file.

When a check fails, the run does not start, and the end of a run adds no blocks. A git action replies "The git files of <copy> point somewhere else now, so the desktop app won't run git there. Check that folder on your desktop." A deleted chat keeps the copy. A copy with no `.git` entry is gone, and has no link to check.

**Merge** (Approve on the desktop, 6.6.6). The branch bar of the chat (13.1) has **Merge** when the chat has its own branch:

1. The chat copy must hold no uncommitted change: "Commit or revert this chat's changes first, then press Merge."
2. `git merge-tree --write-tree` of the start branch and the chat branch tests the merge, and changes no folder. A branch that the start branch already holds gives "Nothing to merge: main already has this chat's work."
3. **A conflict** stops the merge before any change outside the chat folder. The bridge then merges the start branch into the chat branch, in the chat copy, and leaves the conflicts there. The reply: "Can't merge yet: main also changed a.rs and b.rs. I started the merge in this chat's copy. Ask the agent to fix the conflicts, then press Commit and Merge again." The agent cannot run `git merge` itself (the git folder is read-only in the sandbox), so the bridge starts it. The commit of the fix of the agent ends that merge.
4. Else the bridge opens a desktop request of its own kind (`merge`, 6.6.3), with fixed text and the names from git: "A chat from WoW asks to merge gnomish/fix-tests into main in ~/Code/app. Approve only if you just clicked Merge in WoW." The game shows the notice of 6.6.3. Deny, no answer, or Stop ends it with "Not merged."
5. On Approve:
   - If a folder has the start branch checked out (`git worktree list`), the bridge runs `git merge --no-edit <branch>` there: a fast-forward, or a merge commit. If git stops, for example because the merge would overwrite changes in that folder, the bridge runs `git merge --abort` when a merge started. The reply is "Couldn't merge in <folder>: <the first line of git>".
   - If no folder has it checked out, the bridge moves the branch itself. For a fast-forward, it moves to the chat commit. Else it moves to a merge commit of the tree of step 2 (`commit-tree`, and `update-ref` with the old commit, so a change in the meantime fails).
6. The reply: "Merged gnomish/fix-tests into main." The chat keeps its branch, so it can go on.

**Discard** (a confirm in the game, 6.6.6): "Discard this chat's branch? This deletes gnomish/fix-tests and its folder."

- When the copy has uncommitted changes, the bridge first saves them in a commit on top of the branch, with a copy of the index and `git commit-tree`. So the branch and the index of the copy stay as they are. When that fails, Discard refuses: "Couldn't discard: the changes in its folder can't be saved: <the first line of git>".
- Then `git worktree remove --force` and `git branch -D`.
- The bridge logs the last commit of the branch, or the commit with the saved changes. The reply is "Discarded gnomish/fix-tests. To get it back, on your desktop run: git branch gnomish/fix-tests a1b2c3d".
- The chat keeps the choice, so its next message makes a new copy from the start branch.

**A deleted chat** (the `d` flag, 7.1.1) removes its worktree when the worktree holds no uncommitted change. It deletes its branch when the start branch already holds it. A worktree with changes stays, and so does a branch with commits that the start branch lacks. The log names each one that stays. While a run of the chat is in progress, the check waits for the end of that run, because the agent still writes in the worktree until Stop ends it. Why: Delete in the game does not warn about lost work, and another addon can send it. The bridge cleans up in its own thread, after `state.json` forgot the worktree. So a stop of the bridge in between leaves the folder, and `git worktree remove` removes it by hand.

#### The change summary

At the end of each run in a repository, the reply shows what the run changed, in a compact block under the reply (13.1): "3 files changed +40 −2", one row for each file with its counts, and **Commit** and **Revert**.

**The snapshot.** At the start and at the end of each run, the bridge records the state of the chat folder as a git tree. It changes nothing in the index, the branches, or the stash of the user:

- It copies the index of the worktree into a private temp folder. It runs `git add -A` and `git write-tree` with `GIT_INDEX_FILE` set to the copy. The copy keeps the file times of the index, so git reads only the changed files. The tree holds every tracked and untracked file, but no ignored one.
- git writes the new blobs and trees into the object store of the repository, as `git stash create` does. No ref names them, so `git gc` removes them after its prune time (two weeks by default). That is the only write into `.git`.
- The record also holds the commit of `HEAD` at both ends.

**The files** come from `git diff-tree -r --numstat --no-renames` between the two trees, limited to the chat folder with `-- <folder>`. So a chat in a subfolder never lists the files that another chat changed in another folder of the repository. The trees still hold the whole repository, and every path starts at its top.

- A new untracked file counts. A file that the user changed before the run counts only when the run changed it again.
- A binary file shows no counts. A run with no change has no block.
- At most 12 files show, and a last row says "and 5 more".
- If another run worked at the same time in the same folder, or in a folder above or below it, the summary still shows, but Commit and Revert refuse: "Another chat worked in this folder during this run, so Commit and Revert can't tell its changes apart. Use git on your desktop." Why: both runs change one work tree, so the files of one summary can hold the work of the other chat, and a Revert would undo it.

**Commit** (one click, 6.6.6). A click shows a small dialog with the commit message, and **Commit** and **Cancel**.

- The message starts as the first line of the message that started the run, at most 72 bytes. The player can change it. Why this text: it is the player's own words for the task, it costs no second model call, and it needs no change to the prompt of every run.
- The bridge commits exactly the files of that summary, with their content at the click: `git add -A -- <files>` and `git commit --only -- <files>`. Why not `git add -A`: in a chat folder that the player shares with the chat, it would put the player's own earlier work into a commit with the chat message.
- While a merge of step 3 of Merge waits in the chat copy, the commit takes every change, and it ends the merge.
- The reply: "Committed 3 files as a1b2c3d on gnomish/fix-tests."
- When the folder holds a git repository that was not there at the start of the run, Commit refuses: "This run made a git repository inside the folder, so Commit is off. Use git on your desktop." Why: the commit records it as a gitlink. The next plain `git status` of the player then runs git inside it, with the config that the agent wrote there.
- A file of the summary that is neither on disk nor in the index stays out of the commit, because git refuses such a path. An example is an untracked file that the run removed. When no file is left: "Nothing to commit: the files of this summary are gone."
- An empty message gets "Commit needs a message."
- git errors come back as "Couldn't commit: <the first line of git>", for example when git has no name and email yet.

**Revert** (a confirm in the game, 6.6.6): "Revert the changes of this reply? This puts back 3 files as they were before it." Only the changes of this run go:

- The bridge refuses when `HEAD` moved during the run ("The agent made a commit in this run, so Revert can't undo it.") or after it ("These changes are committed now, so Revert can't undo them.").
- It takes a third snapshot now. When any file of the run changed after the run, it refuses and changes nothing: "Revert would also undo later changes to a.rs. Nothing changed." So the work of the user, before or after the run, never goes.
- A file that the run changed or removed comes back from the start tree with `git restore --source=<start tree> --worktree`, which leaves the index alone. A file that the run made goes, without a follow of links. So does each folder above it that is then empty, up to the chat folder.
- The log keeps the end tree: `git restore --source=<tree> -- <file>` brings a file back. The reply: "Reverted 3 files."

**The record** of each run with a summary is in `state.json`: the chat, the message id, both trees, both commits, the top of the repository, and the files. The bridge keeps the last 32. An action on an older one gets "This change summary is too old. Nothing changed." A second Commit or Revert of one summary gets "This change summary is already committed." or "…already reverted.".

**Limits.** A change that the player makes in the chat folder during the run counts as a change of the run. A file name that is not UTF-8 shows with `?`, and Commit and Revert refuse its summary.

#### Test and CI status

**Tests.** The bridge reads the command output of a run for the summary lines of test tools. It shows the last one under the reply, below a change summary: "Tests: 412 passed, 2 failed". It needs no repository. It reads the results of the Claude Bash tool and the `aggregatedOutput` of each Codex command. An ACP agent and a `command` harness send no command output, so they get no test line. The lines that count (in `test_summary.rs`):

| Tool | Line |
|---|---|
| `cargo test` | `test result: ok. 12 passed; 0 failed; 1 ignored; …`, added up over the crates of one command |
| `cargo nextest` | `Summary [ 1.2s] 412 tests run: 410 passed, 2 failed, 3 skipped` |
| jest (npm, pnpm, yarn) | `Tests: 2 failed, 410 passed, 412 total` |
| vitest | `Tests  2 failed \| 410 passed (412)` |
| mocha | `412 passing`, `2 failing` |
| `node --test` | `# pass 410`, `# fail 2` |
| pytest | `==== 2 failed, 410 passed, 3 skipped in 1.2s ====` |
| `go test` | each `--- PASS:` and `--- FAIL:` line, else each `ok` and `FAIL` line of a package |

- The last command with such a line wins, so after a fix, a second test run shows the second result.
- The agent writes this output, so the line reports what the commands printed. It is not a proof: an agent can print any line.

**CI checks.** With `[git] ci_checks = true` (12), the bridge shows the CI checks of the pull request of the chat branch: "CI: 5 passed, 1 failed (lint), 2 running".

- It runs `gh pr view <branch> --json statusCheckRollup` in the chat folder at the end of each run in a repository, and when the player clicks **Checks** in the branch bar (13.1).
  - The reply of Checks is the same line, or "No pull request for gnomish/fix-tests yet.". A pull request with no checks shows "CI: no checks on this pull request".
  - The answer of Checks has no text before its blocks, so the game draws no "[Relay]:" line for it.
  - A branch name that starts with `-` never reaches gh, which takes it as a flag. git refuses such a name, but an agent can write `.git/HEAD` by hand. The reply is "This branch name starts with -, so GitHub can't look it up."
- `gh` runs on the host, as a program of the bridge, never inside the sandbox and never by the agent. It only reads. It gets no shell and a timeout of 20 seconds. Its environment holds only:
  - `PATH`, `HOME`, the `XDG_*` folders, and the variables of the session bus (for the keyring).
  - `GH_TOKEN`, `GITHUB_TOKEN`, `GH_HOST`, and `GH_CONFIG_DIR`, when they are set.
  - `GH_PROMPT_DISABLED=1`, `GH_NO_UPDATE_NOTIFIER=1`, and `NO_COLOR=1`.
- A check counts as passed for `SUCCESS`, `NEUTRAL`, and `SKIPPED`. It counts as running while it is not `COMPLETED`, or `PENDING` or `EXPECTED`. Else it counts as failed. The names of the first two failed checks show. Each name loses its control characters, and every `|` is doubled (S10).
- **Why an opt-in.** It is the only network call of the bridge with a login of the user, and a game message from any addon can start it. So a user who does not use GitHub, or does not want the bridge to reach it, has nothing to turn off.
- With `ci_checks` off, Checks gets "Checks are off. To turn them on, set ci_checks = true under [git] in config.toml." and nothing runs.
- With no `gh`, or with `gh` not logged in, Checks gets "Checks need the GitHub CLI. On your desktop, install gh and run gh auth login." A run just has no CI line, and the log says why once.

#### In the game

The bridge adds its own blocks to a reply (7.3.1): the branch of the chat folder, the change summary, the test line, and the CI line. The addon draws them under the reply, and the actions go back as messages with `git=` (7.1.1). A run that ends as an error, for example "Stopped.", gets the blocks too, because an error with changes needs Revert most.

**Decisions.** The implementer chose these (2026-09-29):

1. **A worktree, not a clone or a copy.** It shares the objects of the repository, so it costs only the checkout, and a merge needs no fetch.
2. **Next to the repository, hidden.** Inside the repository, cargo takes the copy for a member of its workspace, and every walk of the repository reads it twice. In the data folder of the bridge, the copy is inside a `deny` path (6.6.3). Anywhere else, it is outside `allowed_roots`.
3. **The agent cannot commit in its own copy.** A writable git folder of the worktree also makes the object store and the branches of the repository writable. Then a command can move `main` with no merge and no desktop approval. The button costs one click.
4. **Bridge blocks, not Markdown.** The renderer drops every control byte of the agent, so a line that starts with a block kind of the bridge (7.3.1) can come only from the bridge. So an agent cannot draw a fake summary with fake buttons.
5. **A snapshot as a tree, not `git stash`.** `git stash create` leaves out untracked files, and `git stash push` changes the folder. A tree of a copied index holds both and changes nothing.
6. **The files of the summary, not all files, for Commit.** See Commit.
7. **The message of the player, not of the agent.** See Commit.
8. **Revert refuses rather than merges.** A three-way merge of a revert with later changes can lose work of the user. A refusal loses nothing.
### 9.12 Trust a folder on first use

The user asked for this on 2026-09-30, after a test of a fresh install: "nobody types folder paths in a terminal". So setup asks no folder question (11.3). It trusts the code project folders that it finds. The player adds another folder in the game, and one click on the desktop allows it. The code: `folder_trust.rs` (the rules of a new folder), `trust.rs` (the desktop request), `config_edit.rs` (the new root), and `roots.rs` (the roots that the parts of the running bridge share).

**A new folder.** A folder from the game that is under no root is a new folder when all of these are true:

1. It is inside the home folder, and it is not the home folder. A folder above the home folder is outside it.
2. No part below the home folder is a name that the walk skips (9.9): a hidden name (it starts with `.`), `node_modules`, `Library`, `AppData`, and the others.
3. The classifier reads it with no question, with the home folder as the only root, as in the walk (9.9). So a `deny` folder (the config and data folders of the bridge) or a `desktop` path (`.ssh`, a browser profile, `snap/firefox`) is never a new folder.
4. It does not hold the config folder or the data folder of the bridge.

The relay checks rules 1 and 2 on the text when the message comes (6.2, rule 1). When the run starts, the bridge checks all four on the real path, after `canonicalize` (6.2, rule 10), so a link cannot lead out. For a folder that the message makes (`mkdir=1`, 9.9), the real path is the real parent and the new name. Any other folder ends the message with a reply, and no dialog shows:

- The home folder: "Agents can't work in your whole home folder. Pick a project folder inside it."
- Outside the home folder: "That folder is outside your home folder. Pick a folder inside it."
- Rule 2: "Agents can't work in hidden or system folders. Pick another folder."
- Rules 3 and 4: "Agents can't work in that folder: it holds private files. Pick another folder."

**The request.** The run of a new folder waits before the agent starts. The request is a desktop request of 6.6.3, of its own kind `folder`. So it has the same dialog, the same `gnomish-relay approve` fallback, and the same 0600 request file, and the first answer wins.

- The text is fixed bridge text and the real path, never text from the game: "Let agents from WoW work in ~/Documents/Code/lighthouse? They can read and change files in this folder." The path starts with `~/`. Each character that is not a letter, a digit, or printable ASCII shows as `<U+XXXX>`, so a bidi or zero-width character cannot hide a part.
- The buttons are **Approve** and **Deny**, as in every desktop dialog, and `gnomish-relay approve` is the fallback. Deny is the default. The coordinator asked for Allow. The UI copy rules of `CLAUDE.md` keep Approve for the desktop and Allow for the game buttons, so one word names one thing.
- The game shows the notice of 6.6.3 with ` folder`. The row says "Approve this folder on your desktop". The whisper line is "Approve this folder on your desktop.", or "Approve this folder on your desktop: run gnomish-relay approve <id>" with `command`.
- The run timeout stops during the wait, as for any question.

**Approve.** When the message makes the folder, the bridge makes it first, because config load needs each root to exist. Then it reads `config.toml` again with the checks of config load. It adds the folder itself to `allowed_roots`, never its parent. Why: the player picked this folder, and a parent gives the agents its other folders too.

- It changes only the one line `allowed_roots = [...]` before the first table, and keeps the comments and every other line. When the config has no `default_cwd` and no root yet, it also adds `default_cwd = "~"`. So the base of the game folders (9.9) stays the same after a restart.
- It then parses the new text. The text must load, the roots must be the old roots and the new folder, and every other value must be the same. Else it writes nothing. It writes the file with an atomic rename and mode 0600.
- The bridge checks the edit before it shows the dialog, so the user never approves a change that it cannot write. It refuses a config with no single `allowed_roots` line, or with the list on more than one line. The message then ends with "Couldn't add the folder: config.toml can't change. See bridge.log.", and a log line says what to fix.
- Then the running bridge adds the root to the relay, the folder walk, the gate of every agent, and the settings list. So the Settings and Diag tabs show it. The run goes on at once, with the folder check of 6.2 rule 10 as for every run.

**No Approve.**

- Deny or a closed dialog: the message ends with "Denied on your desktop. Agents can't work in this folder. Pick another folder." The bridge then refuses this folder with no dialog until it starts again.
- No answer: the message ends with "No answer on your desktop. Send the message again in 10 minutes to ask again."
- Stop and a new message end it as "Stopped." (9.3).

**Limits**, as for a raise (9.3):

- At most one folder request waits at a time. A second new folder in the meantime ends with "Another folder waits for your answer on your desktop. Answer it, then send this again." So a flood of messages gives one dialog.
- Every answer that is not Approve starts 10 quiet minutes with no folder dialog. A new folder then ends with "No new folders for 10 minutes after one wasn't approved. Pick a folder that agents can already use." So a hostile addon that sends messages gets at most one dialog in 10 minutes, and a Deny ends its folder until the bridge starts again.
- A message for a new folder never raises the level in the same run. It runs at the level of the config, so one message never shows two dialogs.

**What the game can do.** Only a click on the desktop, or `gnomish-relay approve`, adds a root. No message from the game writes `config.toml`, and S6 does not change. The home folder, a folder above it, a hidden folder, and a `deny` or `desktop` path never become a root, also with a click: the bridge refuses them before the dialog.

**Checked against the threat model** (6.1). A hostile addon can already send any message. Now it can also ask the user, in an OS dialog, to allow one folder.

- It cannot click the dialog. The dialog names the real folder and what Approve gives, and it comes at most once in 10 minutes.
- A root widens only two things: the reads of the gate (6.6.2), and the folders that a chat can use. The classifier, the sandbox, and the ceiling still bound each run in the new root.
- The browser now lists home folder folders outside the roots. It shows only names, never files, and the classifier filter keeps the credential folders out of it.
- Resume (9.6) also lists the sessions of the new folders, with the same filter: the folder and the title of each session, never its history. The attach of such a session waits for the same desktop request.
- The proved parts (S5, S16, S17) do not change: the relay calls the same resolver with the home folder as the root for rule 1.

**Decisions** (the coordinator and the implementer, 2026-09-30):

1. **The folder itself, not its parent.** The dialog then says exactly what it gives.
2. **The home folder is never a root.** It holds `~/.ssh`, the browser profiles, and the keys of the bridge. The classifier guards them, but a root is also the read area of the gate and the wide scope of "Always allow" rules (6.6.5).
3. **The check of the text comes first.** A message for a hidden folder or a folder outside the home folder gets its reply at once, with no run.
4. **No new mark in the tree.** An older addon drops a line with an unknown mark, and all the folders below it. So the tree does not mark the folders that need a click, and the bridge refuses the home folder with a clear reply.
5. **`default_cwd = "~"` with the first root.** With no roots, the base is the home folder. With a root and no `default_cwd`, config load takes the first root as the base. An old text of the game (9.9) is relative to the base, so a new base moves the folders of the saved chats. Since 2026-09-30, the game sends the home form, which no base changes.
6. **No raise in the same run.** Two dialogs for one message is too much. The raise comes with the next message.

## 10. Notifications from terminal sessions

**Status: built (2026-09-29).** `protocol`, with proofs: `notice.rs` (S40), `sessions.rs` (S41), and the notices of `live.rs` (S20 restated). Bridge: the `hook` subcommand (`hook.rs`, `hook_input.rs`), the spool folder (`spool.rs`), the session table (`terminal_sessions.rs`), and `hooks install` (`hooks_merge.rs`, `hooks_install.rs`). Addon: `Notices.lua`, `NoticeFrames.lua`, and the Settings and Diag parts. Fuzz targets `hook_input`, `notice_file`, and `hooks_merge`, and `crates/bridge/tests/notices_e2e.rs`. The build checked the design against Claude Code 2.1.285 and codex-cli 0.157.0 (2026-09-29). Lines that real use showed wrong say "Changed in the build" and why.

The implementer and an advisor agent with a UX critic view wrote the proposal (2026-09-27). The user approved it with the changes of a UX review: the name "Notifications", a bell at the minimap in place of a tab, and the texts, sounds, and settings below. Each "Why" says what real use showed.

The user plays WoW while Claude Code or Codex works in a terminal. When a terminal session waits for the user, or finishes long work, the game shows a notification: agent, repo, the first words of the message, and a sound. A notification carries no command and never starts a run. The user answers in the terminal.

**Decisions, and why:**

1. **A spool folder, not a socket.** One code path on all three OSes with only `std`: no `interprocess` crate, no ACL code for a named pipe. A file write never blocks the terminal session. Game runs already cannot see the data folder (6.6.3, 6.6.4).
2. **One notification per session, and a later event takes it away.** The user often answers at the terminal before the game polls. A "Waiting for you" 3 minutes after the answer teaches the user to ignore notifications. This rule replaces the old merge "within 30 s". Known limit: no installed hook fires when the user answers a permission question at the terminal. So a `waiting` notice lasts until the next event of its session, usually the turn end, and a stale "Waiting for you" can still show. A fix needs a hook after each tool call (for example `PostToolUse` of Claude) and a new S41 event that removes only a `waiting` notice. It waits until real use asks for it.
3. **Finished work shows only after long work** (default over 1 minute, a setting). A waiting session always shows. Why: a line after each short turn floods the game chat.
4. **Faster polls only while a terminal session is open.** Signals do not work (7.4), so a notification waits for the next slot poll, and the idle poll is 10 minutes. Faster polls cost slots, so they happen only when a notification can come.
5. **No `notify` change for Codex.** Codex gets its `hooks.json`. Why: `notify` takes one program, and a chain to the user's program can break it.
6. **No tab.** A bell at the minimap edge shows while notifications exist. A tab needs the window, and the user plays with it closed.
7. **The line names the agent and the repo, never "whispers".** A whisper looks like an in-game agent chat, and the user tries to answer it in the game.

### 10.1 The hook command

The hook is a subcommand of the one binary: `gnomish-relay hook claude` and `gnomish-relay hook codex`. The agent starts it for each hook event, with the event as JSON on stdin.

- If `GNOMISH_RELAY_JOB` is set, it exits at once and writes nothing. The bridge sets it for each process of a run (6.2), so game runs never notify, also through a nested `claude`.
- It reads at most 1 MiB of stdin. It takes `hook_event_name`, `session_id`, `cwd`, and the event text, and ignores all other fields.
- The repo name is the name of the git top folder of `cwd` (the first folder upward with `.git`), else the name of `cwd`. Never the full path: a notification can show on a stream or a screenshot.
- It writes one spool file (10.2) and exits. It never prints to stdout: the stdout of a `Stop` hook can keep Claude working.
- It always exits 0, also after an error, so a hook never fails a turn. A timer thread ends it after 300 ms, also when the agent never closes stdin.
- With no spool folder (no bridge runs), or 100 or more files in it (the bridge stopped), it writes nothing.

**Events:**

| Agent | Hook | Event | Text |
|---|---|---|---|
| Claude Code | `SessionStart`, matcher `startup\|resume\|clear` | `session-start` | none |
| Claude Code | `UserPromptSubmit` | `turn-start` | none (the prompt never leaves the terminal) |
| Claude Code | `Stop` | `finished` | `last_assistant_message`, or "Done." when empty (the turn ended on a tool call) |
| Claude Code | `StopFailure` | `failed` | `error_details`, else the error code in `error` |
| Claude Code | `Notification`, matcher `permission_prompt\|elicitation_dialog\|elicitation_url_dialog\|worker_permission_prompt` | `waiting` | `message` |
| Claude Code | `SessionEnd` | `session-end` | none |
| Codex | `SessionStart`, matcher `startup\|resume\|clear` | `session-start` | none |
| Codex | `UserPromptSubmit` | `turn-start` | none |
| Codex | `Stop` | `finished` | `last_assistant_message`, or "Done." |
| Codex | `PermissionRequest` | `waiting` | the command in `tool_input.command` (a string or its words), else `tool_name` |
| Codex | `SessionEnd` | `session-end` | none |

- `idle_prompt` is not in the matcher: it fires 60 seconds after each `Stop`, a copy of `finished`. The hook also checks `notification_type`, so a matcher that the user changed never brings it back.
- `compact` is not in the `SessionStart` matcher. Changed in the build: a compaction in a long turn started a session again, so the notice lost the turn length.
- Codex has `SessionEnd` in 0.157.0. Changed in the build: without it, a closed Codex session kept the faster polls on for 12 hours.
- `SubagentStop` and `agent_needs_input` (a teammate) give no notification.
- Esc in Claude ends a turn with no `Stop`. The next `turn-start` or `session-end` of the session ends it.
- Gemini CLI: not checked yet (17). Another tool can run `gnomish-relay hook claude` from a wrapper script, with its own JSON line.
- The advisor found these names in the binaries of Claude Code 2.1.283 and codex-cli 0.157.0 (2026-09-27). The build checked them again in Claude Code 2.1.285 and codex-cli 0.157.0 (2026-09-29). A live `claude -p` with the hooks of `hooks install` wrote `session-start`, `turn-start`, `finished`, and `session-end`, in order. The Codex app server (`hooks/list`) read all five groups of `hooks.json`.

### 10.2 The spool folder

The spool folder is `<data>/notices/`, mode 0700. The relay lane of the bridge makes and empties it at start. Changed in the build: the bridge has no clean exit (the OS or `restart` ends it), so it cannot remove the folder. A file from a time with no bridge has lost its time, so the next start deletes it. With no bridge, a hook writes at most 100 files (10.1).

- **A file** is one JSON object, at most 4 KiB: `{"v":1,"source":"claude","event":"finished","session":"…","repo":"…","text":"…"}`. The unique name is the time in nanoseconds (39 digits), the process id, and a counter, then `.json`. The hook takes the time at its start, before it reads stdin. The time comes first, so names sort oldest first. The hook writes `<name>.tmp` and renames it, so the bridge never reads half a file. The hook cuts `repo` to 64 bytes and `text` to 600 bytes, and turns control characters into spaces. So JSON doubles at most `"` and `\`, and the file stays below 4 KiB.
- **The bridge reads the folder every 250 ms**, with the watch of the saved variables. It takes at most 64 files per read, oldest first. It deletes each file before it parses it, so a bad file never comes back. It ignores `*.tmp` files, and deletes one older than 60 seconds. It never follows a link: it deletes and logs a link or a folder named `*.json`.
- **The checks.** The fields are exact (`deny_unknown_fields`, a key twice is an error). `source` is `claude` or `codex`. `event` is an event of 10.1. `session` is 1 to 128 bytes of `[A-Za-z0-9_-]`. `repo` and `text` are strings. The bridge drops a file that fails a check, and logs one line.
- **Every text is untrusted**: any local process of the user can write a file. So the bridge cuts `repo` to 64 bytes and `text` to 600 bytes, at a character. `notice_text` turns control characters into spaces, and removes bidi, zero-width, and tag characters. Every `|` is doubled (S10). The live writer escapes each string (S8). S40 proves the cut and the escape.
- **The time is the bridge's own.** A file has no time field. The bridge uses the read time, so the turn length never comes from the file.
- **No rate limit.** A flood of files cannot cost slots: only an addon poll costs a slot, and the live file changes at most every 3 seconds. The session table holds at most 32 sessions, so memory stays bounded.

### 10.3 Sessions and notices

The bridge keeps a table of terminal sessions. Each has a state and at most one notice. `apply_event` in `protocol` changes the table for each event. S41 proves it.

| Event | The session | Its notice |
|---|---|---|
| `session-start` | open | removed |
| `turn-start` | open, a turn runs from now | removed (the user is at the terminal) |
| `waiting` | open, the turn still runs | a new notice `waiting` |
| `finished` | open, no turn runs | a new notice `finished`, with `took`, the length of the turn |
| `failed` | open, no turn runs | a new notice `failed`, with `took` |
| `session-end` | removed | removed |

- An answer at the terminal fires no hook, so a `waiting` notice lasts until the next event of its session (10, decision 2).
- `took` is the turn length in seconds, at least 1. It is 0 when the bridge saw no `turn-start`, for example when the turn started with no bridge (the bridge empties the spool folder at start). A restart during a turn keeps the length, because `notices.json` keeps the turn start. The addon counts 0 as long work.
- The bridge drops a `waiting`, `finished`, or `failed` event in the 60 seconds after the `session-end` of its session. Why: Claude runs its hooks async, so the `Stop` file can come after the `SessionEnd` file, and open an ended session with a `took` of 0. A `session-start` or `turn-start` of the same id opens the session again as usual. The bridge keeps at most 32 ended ids, only in memory.
- A running turn ends after 30 minutes with no event of its session. An open session ends after 12 hours with no event. So an agent crash never keeps the faster polls on.
- With 32 sessions, a new session replaces the session with no notice whose latest event is the oldest. Only when all 32 have a notice, it replaces the one with the oldest notice (by notice time). "Oldest" always means the latest event or the notice time, never the session start. (The user chose this rule on 2026-09-28.)
- **A notice id** is the next number of a counter, and never less than the Unix time. The counter lives in `<data>/notices.json` with the table, so a bridge restart keeps both. Why the time: after a wipe of the data folder, new ids never repeat ids that the addon showed, as for message ids (13.2).
- **A load of `notices.json` checks each session as a new event**, because any local process of the user can change the file. It drops a session whose id fails the spool check, whose times or notice id are more than a day ahead, or whose `repo` or `text` is not a `notice_text` output. The check takes out one `|` of each pair and runs `notice_text` again, so saved text is never escaped twice. The load keeps each session id once and the newest 32 sessions, so S41 holds for the loaded table. It drops a last id more than a day ahead, and starts the counter above every kept notice id. The log names what it dropped.

**In the live file.** The notices ride in `Live.lua`, in a new table after `permissions` (S20, restated):

```lua
notices = {busy = 1, open = 2, list = {
{id = 1790300123, at = 1790300100, source = "claude", kind = "waiting", repo = "gnomish-relay", took = 0, text = "Claude needs your permission to use Bash"},
}},
```

- `busy` counts sessions with a running turn, and `open` counts open sessions.
- `list` holds the newest 20 notices. `at` is bridge time. The addon computes the age from the `now` of the body in the same slot, so a clock difference between desktop and game has no effect.
- Why not a fourth slot file: WoW finds only the files that exist at launch (7.2, rule 1). A new slot TOC file needs a new `setup` with the game closed, in each install. The live file works in the running game. The notices add at most about 56 KB, so the live file stays below its bound of 256 KiB (S21).
- The Timeways live file holds an empty `notices` table, so the writer stays one template for both apps.

### 10.4 In the game

**The poll.** A notification shows at the next slot poll. The addon sets its `PollEvery` hook (13.2) from the last live file:

| State | Poll |
|---|---|
| A desktop request waits (6.6.3) | every 5 s, as before |
| Notifications on, the bridge online, and `busy` > 0 | every 60 s |
| Notifications on, the bridge online, and `open` > 0 | every 3 min |
| Else | the schedule of 7.3 (10 minutes when idle) |

- Cost: 60 slots per hour of terminal work, 20 per hour with an idle session. So the 1000 slots of a UI session last about 16 hours of terminal work, less game chats. Diag shows the free slots, and "Reload soon" (7.3) covers the rest. The banner shows only in the window. So below 20 free slots, the addon also prints one chat line: "Gnomish Relay: slots run low. Type /reload to keep replies and notifications." After the last slot, no poll can take a notice away, so the addon empties the list and the bell hides.
- The addon learns of an open session only at a poll, so the first notification of an evening can wait up to 10 minutes.
- Notifications off stops the faster polls, so it also saves slots.
- Only the bridge ends a stale turn (10.3). A stopped bridge leaves `busy` as in the last live file, so the faster polls stop while the bridge is offline (7.3).

**The list.** The addon keeps the notices of the last live file that pass the filter, less the ones that the user cleared. So an answered notice leaves the list at the next poll (10.3). The filter: a `finished` or `failed` notice with a `took` below the setting shows nowhere, not in the list and not in the chat. `took` = 0 passes every setting but Never. `waiting` always passes. A change of the Finished work setting filters the list at once, with no line and no sound.

**New notices.** A notice is new when its id is not in the last 64 ids that the addon saw. A notice that the filter hid also counts as seen. So a lower Finished work setting never alerts old work: such a notice shows in the list with no line and no sound. The saved variables keep these ids, so a `/reload` shows nothing twice. For the new notices of one poll:

- **The chat line** starts with the bell icon, in the color of the reply line (13.1). Each `waiting` notice gets its own line: `[Claude · gnomish-relay] Waiting for you: <text>`. The `finished` and `failed` notices share one line. With one: `[Codex · lighthouse] Finished in 4 min: <text>`, or `Failed after 2 min: <text>`. With more: `3 agents finished: gnomish-relay, lighthouse, timeways`, or with a failure `3 agents done (1 failed): gnomish-relay, lighthouse, timeways`, so a failure never reads as finished. The text is its first 120 bytes, cut at a whole character, as the list counts its 600 bytes. A click on the line opens the list. It is an addon link (`|Hgnomishrelaynotices|h`), never a chat that can take an answer.
- **The sound.** One per poll: the Battle.net toast sound (`SOUNDKIT.UI_BNET_TOAST`) for a new `waiting` notice, else the whisper sound. Only built-in sound kits, so nothing needs to exist at game start.
- **The toast.** For a new `waiting` notice: a small frame at the bottom left, above the chat frame, like the Battle.net toast. Line one is `<Agent> is waiting · <repo>`, then at most 2 lines of text. It goes away after 8 seconds. A click opens the list.
- **In combat** (`InCombatLockdown`), the chat line shows at once. The toast and the sound wait until combat ends, and then come only for notices still in the list.

**The bell.** A round button on the minimap edge (parent `Minimap`). It shows only while the list holds a notice, and glows while the list holds a `waiting` notice. The user can drag it along the edge, and the saved variables keep its angle. A click opens the list, and a second click closes it. Changed in the build: the Forever client has no bell texture. The icon is the horn of a minimap event (the atlas `minimap-genericevent-hornicon`, and its `-small` form in the chat line), on the border and background of a minimap button. TBC Anniversary has no horn and gets a stand-in (7.9). The name "the bell" stays.

**The list frame.** A small tooltip-style frame below the minimap:

- The title "Notifications", and a × that closes it. Escape also closes it.
- One row per notice, newest first: the agent name in its color (the addon has no agent icons), the repo in gold (first 24 bytes, so the head stays one line), the state ("Waiting" in orange, "Finished · 4 min" in green, "Failed · 2 min" in red), the age, and the first words of the text on a second line. A click on a row shows its full text (at most 600 bytes), and a second click folds it.
- **Clear**, in the title row left of the ×, empties the list and hides the bell. The list and the toast stay on the screen: a list longer than the room below the minimap moves up. The list has no scroll, so a list taller than the screen passes its bottom, but Clear at the top stays in reach. A cleared notice never comes back, also when the next live file still holds it.
- A notice has no button that runs anything. Later: "Continue in the game" through Resume (9.6), with the session of the notice, only after `session-end`, because two programs on one session conflict.

**Settings.** A new group "Notifications" in the Settings tab, after Appearance (13.1). It shows only after `hooks install`: the settings list (13.4) has a `hook` line with `on`. Two rows: Notifications and Finished tasks, then the three Alerts boxes. While it shows, the Always allowed group below shows 3 rules at a time, so the page still fits the least window (900 × 560).

**Diag.** Three new rows, also only after `hooks install`, and also while no hook is on, so a moved or disabled hook shows with its fix: Hooks (each agent's state from the settings list), Sessions (running and open terminal sessions of the last live file), and Last notification (its age). Diag also shows the free slots.

**Two WoW clients** on one computer each read their own slots, so both show each notification and both spend slots. This is accepted.

### 10.5 Install and remove

`gnomish-relay hooks install [--claude] [--codex]` adds the hooks. With no flag, it adds them for each of `claude` and `codex` on `PATH`. `gnomish-relay hooks remove` takes them out, and `gnomish-relay hooks status` shows them. Their output speaks of notifications. `setup` changes no agent settings, and prints one line at its end: "For notifications from Claude Code and Codex in a terminal, run: gnomish-relay hooks install". Why: the agent settings belong to the user, so only an explicit command changes them.

**Claude Code** (`~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR/settings.json` when set, as Claude Code reads it):

- The command adds one group to `hooks.<event>` for each event of 10.1: `{"matcher": …, "hooks": [{"type": "command", "command": "\"<absolute path>\" hook claude", "timeout": 5, "async": true}]}`. A group has a `matcher` only where 10.1 names one. With `async`, Claude never waits for the hook and ignores its output. The path is quoted, for a space on Windows.
- It keeps every other key and user hook in order, with an indent of 2 spaces. So `serde_json` needs its `preserve_order` feature. Cargo turns a feature on for the whole build, so `app-protocol` asks for it too, and its JSON lines keep one key order in every build.
- A second install puts our new group in the place of the old one, so a user hook after ours stays after ours.
- A group is ours only when its one hook is our command. A user group that also holds our command stays as it is.
- It finds its groups by the command `hook claude` after a path with the file name `gnomish-relay`. So a second install changes nothing, and an install after a move of the binary replaces the old path.
- If the file does not parse, or `hooks` or one of its events has another type, it changes nothing and names the key.
- It follows a link to the real file (for a dotfiles folder), and writes the real file with an atomic rename in its folder. The settings can hold an API key, so the temp file has mode 0600 from its first byte and never follows a link at its name. The new file keeps the old mode, or gets 0600 when no file existed.
- Before its first change, it copies the file to `settings.json.gnomish-relay.bak`, mode 0600. It never writes over an existing backup, so the backup is the file from before the first install.
- `remove` takes out only its own groups, and each event with no group left. Install then remove gives the same JSON value as before, except that an empty list of one of our events goes, and so does an empty `hooks`. Remove cannot tell a list that the user left empty from one that it emptied. For both agents, an empty list and a missing key mean the same.

**Codex** (`~/.codex/hooks.json`, or `$CODEX_HOME/hooks.json`, and `config.toml` in the same folder):

- The command merges its groups into `hooks.json` by the same rules, with `hook codex`, and makes the file when it is missing. The Codex groups have no `async`. Codex 0.157.0 knows the field, but the hook ends in 300 ms and prints nothing, so a wait costs little. And a field that a later version reads another way cannot break a turn.
- Changed in the build: in codex-cli 0.157.0 the feature `hooks` is stable and on by default, and `codex_hooks` is its old name. So the command never changes `config.toml`. It refuses when the file sets `hooks = false` or `codex_hooks = false` under `[features]`: the user chose that.
- Changed in the build: Codex runs a new or changed hook only after the user trusts it. At its next start, Codex says "Hooks need review" and asks. The command says so after an install. It never writes the trust itself: that is the user's choice.
- It never changes `notify`.
- `remove` takes out its groups.

**After an install**, the command prints "Restart any <agents> sessions that are open now.", with the agents that it changed: both load hooks only at session start. Then it prints "The first notification can take up to 10 minutes. To check now, type /relay poll in the game.": the addon learns of an open session only at a poll (10.4).

**`hooks status`** shows for each agent: on, off, or on with a missing path (a moved binary). After `hooks install` and `hooks status`, one more line shows when no config exists or the config has no relay: "The relay is off, so no notification comes. Run: gnomish-relay setup --relay". Only the relay lane makes the spool folder. It also shows `disableAllHooks` in the Claude settings, and a Codex config that turns hooks off. The settings list (13.4) carries the same state, so Diag shows it. The bridge reads the files at each list, because `hooks install` can run while the bridge runs. The bridge service lacks shell rc variables such as `CLAUDE_CONFIG_DIR` and `CODEX_HOME`. So each `hooks` command saves the two folders that it used in `<data>/hook-folders.json`, and the bridge reads the files there. With no such file, the bridge uses its own variables.

`setup` prints its hint only when it sets up the relay: the notices ride in the relay live file.

### 10.6 Checks for each part

| Part | Lean | Fuzz | Tests |
|---|---|---|---|
| Notice text | S40 | `notice_file` | each character class that goes, the cut at a character, a `\|` |
| Sessions and notices | S41 | `notice_file` (a sequence of files) | each row of the table in 10.3, stale notices, the 32-session limit, both expiries |
| Live file with notices | S20 restated, S21 | `live` with notices | Lua 5.1 reads the file back; Timeways has an empty table |
| Hook input | | new target `hook_input` | each event of each agent with the input shapes of the checked versions, an empty message, 1 MiB of input, no spool folder, a full spool folder, `GNOMISH_RELAY_JOB` |
| Spool reader | | `notice_file` | a half file, a `.tmp` file, a link, 65 files, a file of 4 KiB and 1 byte |
| Settings merge | | new target `hooks_merge` | user hooks stay, a second install, a moved binary, a broken file, a link, the backup, install then remove |
| Addon | | | the fake game (10.7), in `crates/bridge/tests/addon_notices.rs` |

### 10.7 Verification plan

**Pure parts in `crates/protocol`** (the Aeneas subset of CLAUDE.md). The user approved these statements on 2026-09-28. All four are proved: `notice.rs` (S40), `sessions.rs` (S41), and `live.rs` (S20, S21).

- **S40, notice text.** `notice_text(bytes, max)` never panics and returns at most `max` bytes. The output holds no byte below `0x20`, no `0x7F`, and no bidi, zero-width, or tag character. Read in tokens, it holds each `|` only as `||`. It never ends inside a UTF-8 sequence. Lean shape: `∀ (t : Slice U8) (max : Usize), t.val.length ≤ 2 ^ 20 → notice.notice_text t max ⦃ v => v.val.length ≤ max.val ∧ noticeSafe v.val ∧ pipesDoubled v.val ∧ endsOnChar v.val ⦄`.
- **S41, sessions and notices.** For every table that fits and every event, `apply_event` never panics. The result has at most 32 sessions and at most one notice per session. A `waiting`, `finished`, or `failed` event leaves exactly its own notice on its session. A `session-start`, `turn-start`, or `session-end` event leaves no notice on its session. The other sessions keep their notices, except in a full table where every session has a notice: then only the oldest notice goes (10.3). Lean shape: `∀ (ss : Slice sessions.Session) (e : sessions.Event) (now : U32), sessionsFit ss.val → sessions.apply_event ss e now ⦃ r => r.val.length ≤ maxSessions ∧ oneNoticeEach r.val ∧ noticeAfter r.val e ∧ othersKept ss.val r.val e ⦄`.
- **S20, restated.** The live file is the fixed template of its app with escaped holes, now with the `notices` table. A new `S20_prepare_notices` keeps the newest 20 notices, cuts only string ends, and makes them fit. Lean shape: `∀ app progress requests notices, fitsLive progress.val requests.val notices → live.live_body app progress requests notices ⦃ v => bytes v.val = liveOf app progress.val requests.val notices ⦄`.
- **S21, the same bound.** A live file that fits is still at most 256 KiB. Only `fitsLive` and `liveOf` grow.

**Fuzz targets:** `hook_input` (agent stdin to a spool file: no panic, at most 4 KiB, one JSON object), `notice_file` (spool bytes to an event or a refusal, then `apply_event`), `live` (now with notices), and `hooks_merge` (any `settings.json` text: no panic. It refuses and changes nothing, or its result parses, holds every key and hook of the input, and holds each group of ours once).

**Unit tests** for each rule of 10.1 to 10.5, with sentence names, for example `a_hook_in_a_bridge_job_writes_nothing`, `a_turn_start_removes_the_notice_of_its_session`, `short_finished_work_shows_nowhere`, and `install_keeps_the_hooks_of_the_user`.

**Fake-game tests** (`crates/bridge/tests/addon_notices.rs`, with the fake WoW API. `addon_flow.rs` is long already, so notices got their own file): the hook subcommand writes a spool file, the bridge publishes, and the Lua poll shows the line, the sound, the toast, and the bell. Other cases: nothing twice across a `/reload`, an answered notice leaves the list, the short-work filter, one line for three `finished` notices, a toast and sound that wait for combat to end, the 60 s and 3 min polls, notifications off, Clear, the bell angle, and a Settings group that shows only after `hooks install`. A seeded test feeds the addon 600 random live files, as for the settings list (14.3).

**The end-to-end test** (`crates/bridge/tests/notices_e2e.rs`) runs the real bridge and binary in a temp home, with no game. `hooks install` merges into a `settings.json` that holds user hooks. The test runs the real `gnomish-relay hook claude` with the stdin of each Claude event, and reads `Live.lua` with the Lua slot poll. It also checks that the hook with no bridge exits 0 in less than 300 ms with an empty stdout. A live test marked `#[ignore]` runs the real `claude -p` with `--settings <temp file>`, so the real `~/.claude` does not change, and waits for the `finished` notice. It passed with Claude Code 2.1.285 on 2026-09-29. A live Codex test waits until a temp `CODEX_HOME` can keep the login, and until a test can trust hooks with no terminal.

## 11. Platforms

Only a few paths change per platform. All other code is shared.

| Part | Linux | Windows | macOS |
|---|---|---|---|
| WoW folder | Inside the Wine prefix | `Program Files (x86)\World of Warcraft\<client folder>` | `/Applications/World of Warcraft/<client folder>` |
| Notifications from hooks (10.2) | A spool folder | A spool folder | A spool folder |
| Replace a file that the game has open | Rename always works | Rename can fail. Retry with backoff, then log. | Rename always works |

### 11.1 Linux notes (the first target)

The development machine runs Wayland with XWayland, on ext4.

- WoW runs on D3D12 through vkd3d-proton, or on D3D11 through DXVK.
- The bridge finds `Interface/AddOns` and `WTF/Account/<ACCOUNT>` in any case. It never makes a second folder that differs only in case, for example `Addons` next to `AddOns`.
- The game makes `Interface/` and `WTF/` only at its first start. Setup makes `Interface/AddOns` when it is missing.
- Ubuntu 24.04 blocks user namespaces of normal users with AppArmor. So `bwrap` fails its probe and the bridge has no sandbox. Setup and `status` say so. An AppArmor profile in `/etc/apparmor.d/bwrap` gives `bwrap` its namespaces back, then `sudo systemctl reload apparmor`:

  ```
  abi <abi/4.0>,
  include <tunables/global>
  profile bwrap /usr/bin/bwrap flags=(unconfined) {
    userns,
  }
  ```

### 11.2 Other platform notes

- **File system:** any except FAT32 and exFAT. (`wow-claude` says NTFS. That line comes from `wow-forever-codex`, which stores 65,535 font files, and has no reason in `wow-claude`.)
- **HDR** is not tested.
- **The `claude` command on Windows** is `claude.cmd` in some installs. The bridge finds the path with the `which` crate.
- **Claude on native Windows has no sandbox** for its commands (6.6.4, "Windows"), so every command asks in the game. For commands with no question, use Codex, which has its own Windows sandbox, or the desktop app and Claude under WSL2 (11.5).

### 11.3 Install

The goal: the addon from CurseForge, one command for the desktop app, and no step inside the game.

**The addon comes only from CurseForge**, because the owner's income comes from CurseForge installs. So setup, `update`, `install`, and each desktop app start never write, replace, or delete a file in `Interface/AddOns/GnomishRelay`. The one exception is the old `Key.lua` of 7.3.2. They still write the key addon and the slot addons, because CurseForge does not manage them. A folder that an older setup copied stays as it is.

The install scripts put the program on `PATH`, also in the open terminal on Windows. On Linux and macOS they print the `PATH` line when it is missing.

**`gnomish-relay setup`** sets up Gnomish Relay. It does every step, and a second run changes nothing that works. `gnomish-relay setup --timeways` sets up Timeways: the same steps for its own files, plus the steps of 11.4 and 11.6 (9.7, decision 15). Neither sets up the other product.

1. **Find the game.** It looks for each client folder of 7.9 (`_classic_beta_`, `_anniversary_`) in the default places and in the install paths of Battle.net's `product.db`:
   - Windows: `Program Files (x86)\World of Warcraft`, and `%ProgramData%\Battle.net\Agent\product.db`.
   - macOS: `/Applications/World of Warcraft`, and `/Users/Shared/Battle.net/Agent/product.db`.
   - Linux: each Wine prefix (`~/.wine`, `~/Games/*`, Bottles also as a Flatpak, and Steam Proton), with its `product.db`. `C:` maps to `drive_c`, other drives to `dosdevices`.
   Setup asks no path question. A config with an existing game folder skips the search, so a choice stays. With more than one game, setup takes the one played last: the one whose `WTF` folder holds the newest file (`Config.wtf` and the saved variables, which WoW writes at each logout). It says "Using WoW at <path>. To use another one, run gnomish-relay setup --wow <folder>." With none, setup still does each step that needs no game: the keys, the config with no `[wow]`, and the autostart. It skips the key addons and the slots, and its last line is "WoW not found. Start WoW once, then run gnomish-relay setup." (for Timeways: "Timeways: WoW not found. Start WoW once, then run gnomish-relay setup --timeways."). The next setup finds the game and adds `[wow]` to the config. A desktop app with no `[wow]` prints the same line and exits with success, so the login service does not start it again. So with no game, the autostart only registers the login start. `status` prints the line after "Config: OK", and `restart` stops with it. `setup --wow <folder>`, or `setup <folder>`, skips the search. It takes a client folder or the `World of Warcraft` folder. With more than one client in a `World of Warcraft` folder, it takes the first in the order of 7.9. It also puts that folder into `[wow]` of an existing config. It makes `Interface/AddOns` if WoW has not made it yet, and finds that folder in any case.
2. **Make the keys** once, 32 random bytes from the OS each, mode 0600: `strip.key` when missing, and `timeways.key` only with `--timeways`. A `Timeways` addon folder alone makes no Timeways key. The Timeways key never equals the strip key. `--new-key` makes a new key only for the product of this setup (9.7, decision 15), and then its addon needs a `/reload`.
3. **Write the key addons, and check the relay addon.** A plain setup writes the key addon `GnomishRelay_Key` from the strip key (7.3.2). It deletes an old `Key.lua` in the real folder of `GnomishRelay`, also through a link (a developer checkout, 16), and writes no other file there. Then it finds `GnomishRelay` in any case, and checks its version (7.7):
   - With no folder, setup says "Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW." It still makes the slots, the config, and the autostart, so the player can install the addon after.
   - With a version out of range, setup says "Update Gnomish Relay in the CurseForge app, then restart WoW.", or for a newer addon "Update the desktop app: run gnomish-relay update."
   `setup --timeways` writes the key addon `Timeways_Key` from `timeways.key`, also before the Timeways folder exists. It checks no relay addon. It never makes the Timeways folder, and writes no file in it but the old `Key.lua` of 7.3.2.
4. **Write the config**, once. The relay part has an `[agents.<name>]` entry for each known agent on `PATH`: `claude` (as `kind = "claude"`), `codex` (as `kind = "codex"`), and the ACP agents of 9.2. The default agent is the first found, in the order of `KNOWN_AGENTS` in `install.rs`, else `echo`. Each entry gets `permission = "auto-edit"`. For each harness on `PATH` with no ACP mode (aider and `llm`), setup asks "Found aider. Add it as an agent? It runs its own commands without asking, inside the sandbox. (y/N)". Only a yes adds a `kind = "command"` entry with its preset. With no terminal, the answer is no. A harness with an ACP mode (gemini, goose, opencode) gets its ACP entry, which asks about its tool calls. A local model that answers on the loopback (Ollama on 11434, LM Studio on 1234) puts its port into `[sandbox] local_ports`, so an agent that uses it reaches it from its wall (6.6.4).
   - Why `auto-edit` (decided with an advisor on 2026-09-26): the config is the ceiling of every chat (S6), and the addon asks for `auto-edit`. With `ask` in the config, the player got a game popup for each edit and could not change that from the game. At `auto-edit`, edits inside the chat folder run, and so do the commands that the command sandbox holds (6.6.4, "The sandbox answers at `auto-edit`"). Every other command asks in the game unless the allow table covers it. The `desktop` and `deny` answers do not change. Every kind gets the same level, so the rule is simple. An ACP agent at `auto-edit` also edits the chat folder with no popup when it asks. `echo` has no tools. The config also gets a commented example of the allow table (12): setup allows no command. Only `setup --timeways` writes a `[story]` section, with the model that it finds (9.7, decision 15). A config of `setup --timeways` alone has no relay part. Setup asks no folder question (9.12): `allowed_roots` holds the code project folders that setup finds, or is empty.
5. **Make the slot addons**: `GnomishRelay_S0001` to `S1000` in a plain setup, and `Timeways_S0001` to `S1000` (with `## Dependencies: Timeways`) in `setup --timeways`. WoW finds a new addon only at launch, so new slots need a game restart. Setup says so.
6. **Start the bridge at login**, with `--autostart`. A service starts with almost no `PATH`, so the service file gets the `PATH` of the setup shell. On Linux the unit also gets the shell's `XDG_CONFIG_HOME` and `XDG_DATA_HOME` when set, so the service, the hook, `status`, and `restart` use the same config and data folders. `restart` writes the file again with the `PATH` of its shell, so a restart finds an agent installed later in a new folder. `check-agent` and `status` say when an agent program is not on the `PATH` of the service: "<program> is not on the PATH of the login service. Run: gnomish-relay restart". The config keeps the bare program name, not its absolute path. Why: a version manager such as nvm or volta moves the path at each upgrade, and a script agent such as `claude` under npm still needs its interpreter on the service `PATH`. The service is a systemd user service on Linux in `~/.config/systemd/user`, where the user manager reads it also when a shell rc file sets `XDG_CONFIG_HOME`. On macOS it is a launchd agent (log in `~/Library/Logs/gnomish-relay.log`). On Windows it is a user `Run` entry, which needs no admin rights. On Windows, `run --background` starts the bridge with no console window, with its log in the data folder. Under WSL2, a Windows `Run` entry starts the desktop app in the distro and keeps the distro alive (11.5).

The order is key, key addon, slots, config, then autostart: the key addon and the slots need nothing else. A failed autostart prints one line, and setup goes on.
Setup asks all its questions before it writes the first file: the harness question of step 4 in a plain setup, and the local model question of 11.6 in `setup --timeways`. Each is yes or no, and no terminal means no. So a stop at a question (Ctrl+C) leaves nothing half done. A stop between two steps can leave a part, for example the keys and no config. The next setup keeps the keys and completes the rest.
A plain setup prints one line about where agents work: "Agents can work in ~/Documents/Code. To add another folder, pick it in the game.", or with no root "Pick a project folder in the game to get started."
The last lines say what setup found and the next action:
- the agent, for example "Agent: claude (Claude Code 2.1.3)";
- the sandbox: "Sandbox: bwrap", or "Sandbox: none. Install bubblewrap so commands can run without asking", or a line about AppArmor when `bwrap` is there and fails its probe (11.1);
- "Permissions: auto-edit. Agents edit files and run commands in the sandbox on their own, and ask you before anything risky." It has no "change it" part: the game picks only `ask` or `auto-edit`, and a player never edits a config file to change a setting. It shows the level of the default agent in the config, also for a config that setup did not write;
- "All set. Restart WoW, then type /relay". With the relay addon missing or out of range, the line of step 3 replaces it, and comes last.

`setup --timeways` prints the lines below.

**The output of setup --timeways.** Most Timeways players are not engineers (the user, 2026-10-01: "this can't be expected from a normal user"). So a player reads only player words: no "story program", no folders, no page counts, and no command that a working install doesn't need. A good run prints this, after the "WoW:" line:

```
AI model: claude (haiku)
Installing Timeways. It downloads about 133 MB of lore from Wowpedia.
Downloading the Wowpedia lore: 133 MB
Timeways 0.1.0 installed
Lore ready
All set! Restart WoW, then type /timeways test
```

- The last line is "All set! Restart WoW, then type /timeways test" for a new key addon or new slots, "All set! Type /reload in WoW, then /timeways test" after an update of the key addon or a new key, and else "All set! Type /timeways test in WoW to check it".
- A failed program install prints its reason and the command that tries again (11.4). The last line is then "Timeways isn't ready yet.", never "All set". Why: on 2026-10-01 a 404 printed "All set" and the player thought the install worked.
- With no Timeways folder, "Get the Timeways addon on CurseForge, then restart WoW." comes last, also after a failure.
- The new programs run only after a restart. The installers add `--autostart`, which restarts the desktop app. A setup with no `--autostart` restarts a running desktop app, and leaves a stopped one stopped with "The desktop app isn't running. To start it, run gnomish-relay restart". Why: a player never types `gnomish-relay restart` after a working install.
- A working autostart prints no line. "Desktop app: on, starts at login" and the log line are for `gnomish-relay setup` only. `gnomish-relay install` makes the slots of each product in the config folder: the relay slots with the relay part, the Timeways slots with `timeways.key`.

**Keeping it working.**

- At each start, the bridge writes the key addon again if it is missing or old, and deletes an old `Key.lua` in `GnomishRelay` (7.3.2). It writes no other relay addon file. The CurseForge app replaces only the `GnomishRelay` folder, so the key and the slots stay.
- At each start with `timeways.key`, the bridge also writes the Timeways key addon again when it is missing or old, also with no Timeways folder. With no `timeways.key`, it writes nothing for Timeways. It never makes a Timeways key: that is the job of `setup --timeways`.
- With no key, the addon shows one line and the first-run window of 7.3.2.
- With no fresh body one minute after login, the addon shows one line: "Gnomish Relay: the desktop app isn't running. On your desktop, run gnomish-relay restart."
- Setup starts the default agent once, with no prompt. So a missing login shows in setup ("Agent: claude isn't logged in. Run claude"), not as the first reply in the game.
- `gnomish-relay status` prints one line per part, with the next step when it does not work:
  - whether the bridge runs (the lock of 8.4);
  - the time of the last strip that the bridge took (`last-strip` in the data folder);
  - whether the config loads (with the TOML error and its line, or "Setup didn't finish. Run gnomish-relay setup." with no `config.toml`);
  - the sandbox, and the default agent with its version or its login;
  - the strip line of the newest line test (7.1.4);
  - the relay addon: "Addon: OK", "Addon: missing. Get the Gnomish Relay addon on CurseForge: https://www.curseforge.com/projects/1719624. Install it with the CurseForge app, then restart WoW.", "Addon: too old. Update Gnomish Relay in the CurseForge app, then restart WoW.", or "Addon: newer than the desktop app. Update the desktop app: run gnomish-relay update."

  It also says when the default agent program is not on the `PATH` of the login service. It prints no Timeways line. With no relay part, it stops after the config line (or the game line). The logic is in `status.rs`, and `crates/bridge/tests/status.rs` tests it with the fake agents.
- `gnomish-relay help`, `--help`, and `-h` print the usage on stdout and exit with success. An unknown command prints it as an error.

**Updates and restarts.**

- `gnomish-relay restart` stops the bridge and starts it again, for example after a config edit.
  - With the service of setup, it writes the service file again with the shell `PATH`, and restarts the service: `daemon-reload` and `systemctl --user restart` on Linux, `launchctl bootout` and `bootstrap` on macOS. With no service (Windows, or no `--autostart`), it stops the process in `bridge.pid`, waits up to 10 s for the lock (8.4), and starts `run --background`.
  - First it loads the config. A config that does not load stops the restart with the error, its line, and a non-zero exit, because a service restart succeeds even when the new bridge stops at once. With no `config.toml`, `restart`, `status`, and the desktop app start say "Setup didn't finish. Run gnomish-relay setup." in place of the error.
  - Then it waits up to 10 s for the lock, plus one second. It prints "The desktop app is running.", or the last log line and a non-zero exit.
  - It reads the process id in the lock before the restart. If the same process still holds the lock after it, a bridge outside the service (for example one started by hand) blocks the new one. Then `restart` says "another copy of the desktop app (process <pid>) is still running" and exits non-zero. It never stops a process that the user started by hand.
- `gnomish-relay update` downloads the latest release archive for this OS with `curl`, checks its SHA-256 sum, and unpacks it with `tar`. Every supported OS has both tools. `GNOMISH_URL` changes the download folder, as in `install.sh`.
- If the new program equals the installed one, update changes nothing. Else it replaces the old program and restarts the bridge.
- Windows refuses to replace a running program, but lets update rename it. So update first renames the old program to `gnomish-relay.exe.old`, and the next update deletes that file.
- The sum comes from the same release as the archive. It finds a broken download, not a changed release.
- The new bridge writes the key addons again at its start. It never writes the relay addon. The game then needs a `/reload`, and update says so. When the relay key addon was missing before the update, the game needs a restart, and update says "Restart WoW to finish." (7.3.2). The new bridge starts the story program again (9.8).

**Auto-update** (asked for by the user on 2026-10-01). Players update the addons in the CurseForge app, and the desktop app follows on its own. The game takes no part: an addon cannot start a program, and the desktop app already runs from autostart.

- **The trigger is an addon on disk.** Each release gives the addon and the desktop app the same version. So a newer addon from the CurseForge app tells the desktop app that its own release is out. The bridge reads `## Version:` of `GnomishRelay/GnomishRelay.toc` and of `Timeways/Timeways.toc` in the `AddOns` folder, at start and then once a minute. It never writes either folder (11.3).
- **Versions are semver.** A leading `v` goes: the Timeways packager writes the tag, `v0.1.0-rc.2`. A pre-release is older than its release: `0.1.0-rc.2` < `0.1.0`. A text that is not a version, for example `@project-version@` in a developer checkout, is unknown and starts nothing.
- **The relay.** Only with the relay part of the config. When the relay addon is newer than the running desktop app, the bridge updates the desktop app to the addon version.
- **Timeways.** Only when `[story] program` is a program that setup installed (11.4). Setup and `update` write the installed Timeways release version to `timeways-version` in the data folder. The story program has no `--version`, so the bridge never asks it. When the Timeways addon is newer than that version, or the file is missing, the bridge installs the Timeways release of the addon. When that release needs a newer desktop app (`app_version`, 11.4), the bridge first updates the desktop app to its latest release, and the new program installs Timeways. Known limit: Timeways is not on CurseForge yet (`X-Curse-Project-ID: 0`), so this part waits for its project.
- **Only up.** An older addon never changes the desktop app. The messages of 7.7 still tell the player to update the addon.
- **Pinned, not latest.** The download is the release of the addon version: `.../releases/download/v<version>/<file>` in place of `.../releases/latest/download/<file>`. Why: Timeways release candidates are normal releases, so "latest" can be older or newer than the addon. `GNOMISH_URL` and `TIMEWAYS_URL` still change the download folder.
- **Never during a run.** The bridge waits until no agent run is in progress. A desktop request always waits inside a run, so none is open then. Then the bridge starts `gnomish-relay update --auto` as its own process, which outlives the bridge. That process does the steps of `update` with the pinned versions, and restarts the bridge last. It has no terminal, so its lines go to `update.log` in the data folder.
- **One try per version per hour.** A failure, for example no internet or a release with no files attached yet, logs one line with the reason. The bridge tries the same version again after one hour, and at its next start. An update that installs nothing new counts as a failure, so a wrong release never loops.
- **Off switch:** `auto_update = false` in `config.toml` (12, default `true`). `gnomish-relay update` still works by hand.

**Distribution.**

- A version tag (`v*`) starts `.github/workflows/release.yml`. It builds the program for Linux (x86-64), macOS (Arm and x86-64), and Windows (x86-64), and attaches each archive with its SHA-256 sum to a GitHub Release. The release stays a draft until every build is attached. Before the draft, the workflow runs every CI job on the tagged commit (14.7), and checks that the tag, the `Cargo.toml` version, and the TOC version match.
- `scripts/install.sh` (Linux and macOS) and `scripts/install.ps1` (Windows) download the latest release archive, check its SHA-256 sum, install the program, and run `setup --autostart`. With no argument, they set up only Gnomish Relay. With `--timeways`, only Timeways (9.7, decision 15). Setup is their last step, so with no relay addon, the CurseForge line of step 3 is also the last installer line. Setup asks its questions on the terminal, also under `curl | sh`.
- Setup asks no folder question. It trusts the usual code project folders that hold a git repository (`suggest_roots` in `install.rs`), never the home folder. The usual folders are `~/Documents/Code`, `~/code`, `~/Code`, `~/src`, `~/dev`, `~/projects`, `~/Projects`, `~/repos`, and `~/workspace`. A folder counts when a repository is at most 3 levels below it, so `~/Documents/Code/Personal/app` makes `~/Documents/Code` a root. The search follows no link, and skips the folders that the walk skips (9.9). It stops at the first repository, or after 2000 folders or 1 second per usual folder. Setup trusts the usual folder itself, never each repository. With none, `allowed_roots` is empty, and the player picks the first folder in the game (9.12). `--roots a,b` gives the roots instead, for scripts. A later setup with a relay config whose `allowed_roots` is empty searches again. It adds each folder found with the edit of a desktop Approve (9.12), so the config also gets `default_cwd = "~"` and the folders of saved chats stay the same. When the line cannot change, setup says so and keeps the config.
- Setup installs no agent. It uses the agents already on `PATH`. With none, the config uses `echo`, and setup says so. After the player installs an agent, a second setup adds its entry (12).
- Later: winget, Homebrew, and the AUR point at the release.
- Players get the addon only from CurseForge, and the desktop app never installs it. The listing points to the desktop app: the addon alone does nothing, because each computer needs its own key (7.3.2).
  - After the GitHub release is out, `release.yml` calls `.github/workflows/curseforge.yml`. The addon goes out last, because a new addon on disk makes the desktop app update itself from the latest release. `scripts/package-addon.sh` makes the `GnomishRelay` folder: the files of `addon/GnomishRelay` and of `addon/transport` as real files, and `.pkgmeta`. It never holds a key addon or a slot. The BigWigs packager ships only files that git tracks, so the job gives the folder its own git repo with the tag, and runs the packager on it.
  - The project id comes from the repository variable `CURSEFORGE_PROJECT_ID`, else from `## X-Curse-Project-ID` in `GnomishRelay.toc`. The TOC holds a placeholder until the maintainer makes the project. With no numeric id, no `CF_API_KEY` secret, or no tag, the job skips the upload and keeps the zip as a run artifact. A run by hand (`workflow_dispatch`) never uploads, also on a tag.
  - A test checks that the folder holds exactly the files that the addon needs: the TOC, each file that the TOC lists, `Bindings.xml`, the mono font and its license, and `.pkgmeta`. Each file equals the one in the repo.
  - Later: Wago Addons.

### 11.4 The Timeways programs and the lore pack

Setup installs the Timeways story program and builds its lore pack (planned with the Timeways session on 2026-09-30; the tests came first). The release format below is the one that the release job of `eserilev/timeways` makes. `crates/bridge/src/timeways_release.rs` holds every asset name, `lore_pack.rs` the dump and the build, and `timeways_install.rs` the steps.

**When.** `setup --timeways` always installs the programs, builds the lore pack again, and sets the config. A plain setup never does, also with a `Timeways` addon folder (9.7, decision 15). `gnomish-relay update` installs new programs when `[story] program` is set, into the folder of that program. It builds no lore pack. When update installed a new desktop app, the new program does this step, through `gnomish-relay update --timeways-only`. Why: the old program checks a release against its old version range, so it refuses a Timeways that needs the new desktop app. A failed Timeways step prints one line: the error as its own sentence, then the next step ("To try again, run gnomish-relay setup --timeways", or "To try again, run gnomish-relay update"). Setup and update go on.

**A failed download** says what failed and what to do, never the `curl` command line or error text. `curl` runs with its error output caught. The details (the URL, the exit code, and the `curl` error) go to `bridge.log` in the data folder. The reason comes from the `curl` exit code:

- A missing file (HTTP 404): "Couldn't download Timeways (the release isn't published yet). To try again later, run gnomish-relay setup --timeways".
- No connection (no DNS answer, no connection, a timeout, or a failed TLS start): "Couldn't download Timeways. Check your internet connection, then run gnomish-relay setup --timeways".
- Any other failure: "Couldn't download Timeways. To try again, run gnomish-relay setup --timeways".

In `update`, the next step is "run gnomish-relay update". The lore dump gets the same reasons: "Couldn't download the Wowpedia lore (no internet connection). To try again, run gnomish-relay setup --timeways".

**The release.** The base is `https://github.com/eserilev/timeways/releases/latest/download`. `TIMEWAYS_URL` changes it, as `GNOMISH_URL` does for the desktop app (11.3). The files:

| File | What |
|---|---|
| `timeways-manifest.json` | The version, the tag, the addon version, and one entry per target |
| `SHA256SUMS` | One `sha256sum` line per archive |
| `timeways-<target>.tar.gz` | Linux and macOS: `timeways-story`, `timeways-pack`, and `LICENSE` at the top level |
| `timeways-x86_64-pc-windows-msvc.zip` | Windows: `timeways-story.exe`, `timeways-pack.exe`, and `LICENSE` |
| `<archive>.sha256` | The sum of one archive. Setup does not need it. |
| `timeways-addon.zip` | The addon for CurseForge. Setup does not need it. |

The manifest:

```json
{"version": "0.1.0", "tag": "v0.1.0", "app_version": 1,
 "targets": {"x86_64-unknown-linux-gnu": {"asset": "timeways-x86_64-unknown-linux-gnu.tar.gz",
   "sha256": "<64 hex digits>", "programs": ["timeways-story", "timeways-pack"]}},
 "addon": {"asset": "timeways-addon.zip", "sha256": "<64 hex digits>"}}
```

- The target is the desktop app target of this computer, as in the names of 11.3. With no entry for it, setup says "Timeways has no build for this computer yet."
- An asset or a program is a plain file name: no folder, no `..`, no leading dot. `programs` must hold `timeways-story` and `timeways-pack`. Setup and update install only these two, and ignore any other name: the bin folder also holds the desktop app, and a program there can hide a tool such as `git` on the PATH. A Windows program gets `.exe`.
- `app_version` is the `ns.App.version` of the Timeways addon of that release. Setup checks it with `version_fit` (7.7, S30). A version out of the range of this desktop app stops the install: "This Timeways needs a newer desktop app. Run gnomish-relay update first." or "This Timeways is older than this desktop app supports."
- Setup checks the archive's SHA-256 sum against the manifest and against `SHA256SUMS`, and refuses the archive when either differs or is missing. As in 11.3, the sums come from the same release, so they find a broken download, not a changed release.
- Setup unpacks the archive with `tar` into a new work folder in the data folder. It installs each program into `GNOMISH_BIN` when set, else into `~/.local/bin` on Linux and macOS, and `%LOCALAPPDATA%\timeways\bin` on Windows. Timeways gets its own folder: the desktop app data folder holds the `bin` folder of `install.ps1`, and the bridge refuses a story program in a folder that the sandbox hides (9.8). A program equal to the installed one stays. A new one replaces the old one as in `update` (11.3).

**The lore pack.** The pack is never shipped: each computer builds it from the public Wowpedia dump.

- The dump is `https://s3.amazonaws.com/wikia_xml_dumps/w/wo/wowpedia_pages_current.xml.7z`, about 133 MB. `TIMEWAYS_DUMP_URL` changes it. It changes over time, so no sum is pinned.
- Setup downloads it with `curl`, as `update` does, into the Timeways work folder in the data folder. The sandbox hides the data folder (6.6.3), so no agent or command of a game run reads it. While `curl` runs, setup prints the megabytes so far: "Downloading the Wowpedia lore: 45 MB".
- Then it runs `timeways-pack from-dump <dump> <pack>.new`. The program reads the `.7z` itself and never writes over a file, so setup first deletes an old `<pack>.new`. The program prints one line per page. Setup shows only its last two lines: "read N pages, skipped M" and "wrote N passages to <path>".
- On success, setup renames `<pack>.new` over the pack. On a failure, the old pack stays, and setup says "Couldn't build the Timeways lore. Your old lore stays. To try again, run gnomish-relay setup --timeways." Setup deletes the dump at the end either way.
- The pack is `lore.sqlite` in the `timeways` folder next to the data folder: `~/.local/share/timeways/lore.sqlite` on Linux, `~/Library/Application Support/timeways/lore.sqlite` on macOS, and `%LOCALAPPDATA%\timeways\lore.sqlite` on Windows. The story program reads it, so it is outside the folders that the sandbox hides.

**The config.** After the programs and a pack, setup sets `program` and `lore_pack` in `[story]`, with `~/` for a path in the home folder. It replaces the lines of both keys, also the commented ones of 12, and keeps every other line. A config with no `[story]` gets one at its end. As in 11.3, setup checks the new text with the config loader before it writes. With no pack, setup sets neither key: they go together (12).

**The installers.** `install.sh` and `install.ps1` pass their arguments to setup, and always add `--autostart`. `--no-autostart` turns it off. With no argument, they set up only Gnomish Relay. `install.ps1` sets up the desktop app in WSL2 only with `-Wsl` or `--wsl` (11.5).

Timeways players see short lines, in the Timeways README and in its setup window in the game. Each line fetches a small script from the GitHub Pages of the Timeways repo (its `docs` folder). That script runs the installer of this repo with `--timeways`, so this repo keeps the only copy of the install steps.

- Linux and macOS: `curl -fsSL https://eserilev.github.io/timeways/install.sh | sh`
- Windows, from Windows+R: `powershell -c "irm https://eserilev.github.io/timeways/install.txt | iex"`

The short scripts run these lines. They also work by hand, and set up only Timeways:

- Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.sh | sh -s -- --timeways`
- Windows: `& ([scriptblock]::Create((irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1))) --timeways`

**Why short lines:** most Timeways players are not engineers. A short line on a readable address looks safe, and a player can paste it with no typo. Windows+R needs no terminal. The Windows script waits for Enter at its end, so the window stays open for the last line or an error. The name ends in `.txt` because GitHub Pages serves only that as text, and serves `.ps1` as bytes.

**Contract:** the short scripts pass no argument but `--timeways`. A change to the arguments or file names of `install.sh` and `install.ps1` breaks them, so change `docs/` in Timeways in the same step.

### 11.5 Windows with WSL2

Native Windows has no sandbox for Claude's commands (6.6.4, "Windows"). So a Windows player can run the whole desktop app inside WSL2, as a Linux program with the Linux `bwrap` sandbox. WoW stays on Windows. The user approved this path on 2026-09-30.

**Why the whole desktop app, not only the agent.** The permission hook, the `--sandbox-run` wrapper, the holder, the proxy, and the agent wall are Linux code. They talk over Unix sockets, and a Unix socket does not connect a WSL2 process to a Windows process. So all of them run in WSL2, and the Windows desktop app does not run.

**Detection.** The desktop app runs under WSL when `/proc/sys/kernel/osrelease` holds `microsoft` (in any case), or when `WSL_DISTRO_NAME` is set. The distro is the value of `WSL_DISTRO_NAME`. The desktop app can start Windows programs (interop) when `/proc/sys/fs/binfmt_misc/WSLInterop` or `WSLInterop-late` exists. WSL1 is not supported: it has no namespaces, so `bwrap` fails its probe.

**The game folders.** WSL2 mounts each Windows drive under `/mnt/<letter>` (drvfs). Through these mounts, the Linux desktop app reads `Screenshots` and `WTF`, and writes `Interface/AddOns`.

- Setup looks for the game as on Windows: `Program Files (x86)/World of Warcraft` and `Program Files/World of Warcraft` on each drive under `/mnt`, and the paths in `ProgramData/Battle.net/Agent/product.db` of each drive.
- A Windows path, from `product.db` or `setup <folder>`, maps to WSL: `C:\Games\World of Warcraft` is `/mnt/c/Games/World of Warcraft`. The drive letter goes to lower case, and each `\` becomes `/`. Setup does not read another mount root (`root` of `[automount]` in `/etc/wsl.conf`). With another root, give the Linux path to `setup <folder>`.
- The watcher polls with `read_dir` every 250 ms (8.2). inotify sees no Windows change on drvfs, so a poll is the right way. Each poll crosses to Windows (9P) and costs more than on ext4. The folder stays small, because the bridge deletes each strip.
- drvfs shows no Unix modes by default: its `metadata` option is off, so each file shows as mode 0777, and `chmod` changes nothing. No private bridge file lies there. The config and data folders are in the Linux home, so the modes of 6.2 rule 14 hold. The addon files and the key addon lie on drvfs, and every Windows program of the user can read them, as on native Windows.
- `symlink_metadata` shows a Windows link or a junction as a link, so the link checks of 6.2 rule 7 hold on drvfs.
- The atomic rename works on drvfs: WSL replaces the target in one step. WoW reads an addon file only when it loads it, so the rename seldom meets an open file.

**Chat folders.** `allowed_roots` and the folder list of the game are Linux paths. A project in the Linux home is fast, so setup looks for folders there, and the browser shows the Linux home (9.12). A project under `/mnt/c` works, but git, builds, and the walk of the chat folder (6.6.4) are many times slower on drvfs.

**`PATH`.** WSL adds the Windows `PATH` to the Linux `PATH`. There, a Windows `claude` from npm runs Windows Node, outside every wall. So under WSL, setup and the start file drop each `PATH` entry under `/mnt/`, and an agent must be a Linux install. The bridge finds `powershell.exe` and `cmd.exe` in `Windows/System32` of the first drive that has them.

**The sandbox.** Commands run in `bwrap` as on Linux (6.6.4). The installer installs `bubblewrap` as root through `wsl.exe -u root`, so the player types no password. The probe and the AppArmor hint of 11.1 stay: the probe decides, whatever the distro.

- `--ro-bind / /` also binds the Windows drives read-only, so a command writes no Windows file.
- The `desktop` paths of 6.6.3 are also hidden in the Windows home folder, for example `/mnt/c/Users/<you>/.ssh` and `/mnt/c/Users/<you>/.aws`. The bridge learns the folder once at start, from `%USERPROFILE%` through `cmd.exe`. Windows credential stores with other names, such as the browser profiles in `AppData`, stay readable. DPAPI protects the browser ones, and a command cannot call Windows.
- The agent wall binds each Windows drive read-only, then binds back the chat folder and the run's temp folders when they lie on one. Why: the agent keeps its writes to the Linux home ("Where the wall is"), but a Windows file can run later with no wall: a file in the Startup folder, a PowerShell profile, or the `gnomish-relay.exe` that starts the desktop app. So the agent writes no Windows file outside its chat folder.
- A command and the agent cannot start a Windows program: `/run` is empty in both walls, so the interop socket in `/run/WSL` is gone, and `WSL_INTEROP` is not on the allowlist (6.2 rule 12).
- A limit: WSL talks to Windows over `AF_VSOCK`, which a network namespace does not cover. A program that speaks the WSL protocol could reach Windows from a wall. This is not checked (17).

**The desktop dialog** (6.6.3). With interop, the bridge shows the MessageBox of the Windows build, through `powershell.exe`. WSL passes an environment variable to a Windows program only when `WSLENV` names it, so the bridge adds `GNOMISH_NOTICE` to `WSLENV`. A notice uses the Windows toast the same way. With no interop, the Linux dialogs apply (WSLg gives `zenity` a display), and `gnomish-relay approve` is always there. A limit: when the request ends first, the bridge stops the Linux side of the call, but the Windows box stays until the player closes it, and that answer counts for nothing.

**Start at login.** WSL2 stops a distro a few seconds after the last Windows process that uses it ends. A systemd service in the distro does not keep it alive. So a Windows process keeps the desktop app running:

- The `Run` entry "Gnomish Relay" of the Windows user runs `"%LOCALAPPDATA%\gnomish-relay\bin\gnomish-relay.exe" wsl-run <distro> --background`. It is the same entry as for the Windows desktop app (11.3), so only one of the two starts. The Windows program starts `wsl-run <distro>` again with no console window, and exits.
- `wsl-run <distro>` takes a lock on `bridge.lock` in the `wsl` folder of the Windows data folder, so a second copy exits at once. Then it runs `wsl.exe -d <distro> --exec /bin/sh -c '. "$HOME/.config/gnomish-relay/wsl-start.sh"'` with no console window, and waits. When that ends, it waits 3 seconds and starts it again. The running `wsl.exe` keeps the distro alive.
- `wsl-start.sh` sets the setup `PATH` with no Windows entry, and `XDG_CONFIG_HOME` and `XDG_DATA_HOME` when set, then runs `exec <program> run --log`. `run --log` starts `run` with its log in `bridge.log` of the data folder, as `run --background` does, waits for it, and exits with its status. The file has mode 0600, because the launcher reads it with `.` and does not run it. It lies at a fixed place in the home folder, as the systemd unit does (11.3), because the Windows side knows no `XDG_CONFIG_HOME`. The agent wall keeps it read-only (6.6.4, "The startup files are read-only for the agent").
- Why this way (decided on 2026-09-30):
  - A `Run` entry or a scheduled task that runs `wsl.exe` itself shows a console window for the whole session, and a click on its close box stops the desktop app.
  - `conhost.exe --headless` hides that window, but it has no documentation. A VBScript hides it too, but Windows is removing VBScript.
  - A systemd service needs systemd on in the distro, and still needs a Windows process that keeps the distro alive.
  - The Windows program already starts the Windows desktop app with no window (`CREATE_NO_WINDOW`), and a `Run` entry needs no admin rights.
- `setup --autostart` under WSL writes `wsl-start.sh`, then runs `gnomish-relay.exe wsl-autostart <distro>` through interop. That command writes the `Run` entry, stops a running Windows desktop app (two desktop apps fight over the game folder, 8.4), and starts `wsl-run`. Then setup waits for the desktop app as `restart` does. The Linux side finds the Windows program at `%LOCALAPPDATA%\gnomish-relay\bin\gnomish-relay.exe`, from `cmd.exe /c echo %LOCALAPPDATA%`. With no Windows program, or no interop, setup prints "Desktop app: can't start at login (the Windows part of Gnomish Relay is missing. Run the Windows installer in PowerShell)".
- `restart` under WSL writes `wsl-start.sh` again, stops the desktop app, and runs `gnomish-relay.exe wsl-run <distro> --background`. A running `wsl-run` starts the desktop app again after its 3 seconds, and the new one exits at its lock. `restart` then waits for the lock as in 11.3.
- `wsl --shutdown` stops the desktop app, and `wsl-run` starts the distro again 3 seconds later. To stop the desktop app for good, end `gnomish-relay.exe` in the Task Manager, and turn off "Gnomish Relay" in Settings > Apps > Startup.

**Status.** `gnomish-relay status` in WSL prints "Running in WSL2 (distro Ubuntu)" after the desktop app line, and the sandbox line as on Linux. Its check of the agent on the `PATH` of the login service reads `wsl-start.sh`.

**The install flow.** `install.ps1` holds the Windows steps. The decisions that tests can reach are in Rust.

1. The player runs the Windows one-liner (11.3). `install.ps1` installs `gnomish-relay.exe` as before. Under WSL2 it is the desktop app launcher.
2. It takes the WSL2 path only with `-Wsl` or `--wsl`. Without the flag it runs the native Windows setup and asks nothing, as before. **Why opt-in:** the WSL2 path has not passed the manual plan below on a real PC yet. Also, the one-liner fetches `install.ps1` from `main` but the program from the latest release. A default of yes sends every Windows player into an untested path, with a program that can lack `wsl-run`. When the plan passes and a release carries `wsl-run`, the default can become the question "Protect your computer with the Linux sandbox? …" again.
3. It finds the default distro with `wsl.exe --exec sh -c 'echo "$WSL_DISTRO_NAME"; uname -r; id -u'`. This works in every Windows language, unlike the text of `wsl.exe --status`. A release with no `WSL2` in it is WSL1: the installer says "Your Linux runs on WSL1, which has no sandbox. Run: wsl --set-version <distro> 2", and stops. User id 0 means that the distro has no Linux user yet, and setup as root puts every file in `/root`: the installer says "Set up your Linux user first: open <distro> from the Start menu, pick a user name and password, then run this installer again.", and stops.
4. With no distro, it runs `wsl.exe --install` as admin (`Start-Process -Verb RunAs`, so Windows asks the player once). Then it looks for the distro again: a Windows that already has the virtual machine part needs no restart. Else it downloads itself as `install.ps1` into the bin folder (under `irm | iex` it has no file). It adds a `RunOnce` entry that runs it again at the next sign-in with `-Wsl` and the same arguments, and says "Restart Windows to finish. The installer continues after you sign in."
5. With WSL2:
   1. When `bwrap` is missing, it installs `bubblewrap` as root with `apt-get`. With no `apt-get`, it says "Install bubblewrap in <distro> with its package manager, then run gnomish-relay restart in <distro>."
   2. When `claude` is missing, it installs Claude Code in the distro with its native installer (`curl -fsSL https://claude.ai/install.sh | bash`). Then it opens `claude` once for the login: "Log in to Claude, then type /exit.". The first `wsl.exe` call of a new distro first asks for a Linux user name and password. For Codex, the player installs it in the distro and runs setup again.
   3. It runs `install.sh` in the distro with the player's arguments, in a login shell so `~/.local/bin` is on `PATH`. `install.sh` runs `setup --autostart`: it finds WoW under `/mnt`, writes the addons and the config, and starts the desktop app through `wsl-autostart`.
6. Setup prints its summary as on Linux: "Sandbox: bwrap", the agent and its login, and "All set. Restart WoW, then type /relay". The installer adds "The desktop app runs in WSL2 (<distro>)".

**Limits.**

- Hooks of terminal sessions (10) reach the desktop app only from Claude Code and Codex in WSL2. A native Windows terminal session writes to the spool of the Windows data folder, which no desktop app reads.
- The Windows desktop app and the WSL2 one have separate config, keys, and chats. After the switch, WoW needs a `/reload` for the new key, and setup says so.

**Tests.** No test can run WSL2: the GitHub Windows runners have no nested virtualization. Unit tests cover each pure part: the detection from fake `/proc` files and variables, the path mapping, the game search under a fake mount root, the `PATH` filter, the text of `wsl-start.sh`, the `Run` entry and the `wsl.exe` arguments, the dialog under WSL with its `WSLENV`, the status line, the read-only drives of the agent wall, and the hidden paths of the Windows home. The manual test below covers the rest.

**Manual test on Windows 11.** Use a Windows 11 computer with WoW Forever, no WSL, and no Gnomish Relay.

1. In PowerShell, run the one-liner of 11.3 with `-Wsl`. It installs `gnomish-relay.exe` and takes the WSL2 path.
2. Windows asks for admin rights for `wsl --install`. Click Yes. A second window installs WSL and Ubuntu. The installer says "Restart Windows to finish. The installer continues after you sign in."
3. Restart and sign in. Ubuntu opens and asks for a new Linux user name and password. Enter them. A PowerShell window opens by itself and goes on with no sandbox question. If Ubuntu did not open, the installer says "Set up your Linux user first: ...". Do that, then run the one-liner again.
4. The installer installs bubblewrap with no password, then Claude Code, then opens `claude`. Log in, then type `/exit`.
5. `install.sh` runs, and setup prints `WoW: /mnt/c/Program Files (x86)/World of Warcraft/_classic_beta_`, then asks for the agent folders. Accept a folder in the Linux home, or type one, for example `~/code`.
6. The last lines show "Sandbox: bwrap", "Agent: claude (Claude Code <version>)", "Desktop app: on, starts at login", "All set. Restart WoW, then type /relay", and "The desktop app runs in WSL2 (Ubuntu)". No console window stays open.
7. Open Ubuntu from the Start menu and run `gnomish-relay status`. It shows "Desktop app: running (process <pid>)", "Running in WSL2 (distro Ubuntu)", and "Sandbox: bwrap".
8. Start WoW, type `/relay`, and send "hi". The reply comes.
9. Ask the agent to run `cat ~/.ssh/id_ed25519; cat /mnt/c/Users/<you>/.ssh/id_ed25519; touch /mnt/c/Users/<you>/x; cmd.exe /c echo hi`. Each part fails: no such file for the two keys, a read-only file system for `touch`, and an error for `cmd.exe`.
10. Ask the agent for a command that needs your approval on the desktop, for example one that reads `.env` in the chat folder. A Windows message box "Gnomish Relay" opens with Yes = Approve and No = Deny. Click No. The game shows the denial.
11. In Ubuntu, run `gnomish-relay restart`. It prints "The desktop app is running." within 10 seconds.
12. Sign out of Windows and sign in again. With no terminal open, the desktop app runs: `/relay` in WoW works, and `gnomish-relay status` in a new Ubuntu window shows it running.
13. In PowerShell, run `wsl --shutdown`. After about 5 seconds, `/relay` in WoW works again.
14. Run the one-liner again with no flag. It sets up the Windows desktop app. The `Run` entry now starts the Windows one, and `gnomish-relay status` in PowerShell shows "Sandbox: none".

### 11.6 A free local model for Timeways

The user decided this on 2026-09-30, with the Timeways session ("A plus C"). Setup gives Timeways a model when it can. `crates/bridge/src/ollama_install.rs` holds the steps, and `setup_command.rs` the question.

**When.** `setup --timeways` runs this step when `[story]` has no `model` (or the config has no `[story]`). A plain setup never runs it, also with a `Timeways` addon folder (9.7, decision 15). Setup asks the question before it writes the first file (11.3), and installs the model after the config. So the player answers first and then waits once.

**A: a model that the player has.** Setup first looks for a model as in 9.7, decision 15: `claude` on `PATH`, then Ollama, then LM Studio. The first one goes into `[story]`.

**C: no model found.** Setup asks one question in the terminal:

```
No AI model found. Timeways works without one, but it writes no story text.
Setup can install Ollama with its official installer: curl -fsSL https://ollama.com/install.sh | sh
Install a free local model? It runs on this computer and needs about 2 GB. [Y/n]:
```

On Windows the second line names `https://ollama.com/download/OllamaSetup.exe`. So the player sees what setup runs before the yes.

- An empty answer, `y`, or `yes` (any case) is yes. Any other answer is no.
- On no, setup prints "To install it later, run gnomish-relay setup --timeways".
- With no terminal on stdin (for example `curl | sh` with no terminal), the answer is no, and setup downloads nothing. It prints the first line of the question, then "To install a free local model, run gnomish-relay setup --timeways in a terminal". Why: a download of 2 GB never starts without a yes.
- The install is terminal only on every OS. Setup opens no window.

**On yes**, setup does these steps, with a plain-words line for each:

1. When Ollama already answers on `127.0.0.1:11434` (it runs, but has no chat model), setup skips steps 2 and 3.
2. Setup downloads the installer with `curl --proto =https --proto-redir =https --location --max-redirs 5` into a new private temp folder. The URL is a constant, and only HTTPS works, also for each redirect. `https://ollama.com/install.sh` redirects to the Ollama release on GitHub.
3. Setup runs the installer in the terminal, with no shell of its own:
   - Linux and macOS: `sh install.sh`. The script asks for `sudo` on Linux. On macOS it installs the official `Ollama.app` into `/Applications`, links the `ollama` command into `/usr/local/bin` (with `sudo` only when needed), and starts the app with no window.
   - Windows: `OllamaSetup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-`. The installer needs no admin rights: it installs into `%LOCALAPPDATA%\Programs\Ollama`, and starts Ollama in the background.
   - Why the script on macOS, not Homebrew: the script is Ollama's own route, it works with no `brew`, it installs Ollama's signed app and starts its server, and Linux and macOS share one code path. Homebrew, not Ollama, keeps the Homebrew formula, and its server needs `brew services start`.
4. Setup waits up to 60 seconds for `GET /api/version` on `127.0.0.1:11434`.
5. Setup pulls one model, `LOCAL_STORY_MODEL` (one constant in `ollama_install.rs`; Timeways picks it), through `POST /api/pull` with a stream of JSON lines. It shows the megabytes so far. A line with `error`, or an end with no `success` line, is a failure.
6. Setup writes `model = "local"`, `local_url = "http://127.0.0.1:11434"`, and `local_model` into `[story]`, in place of the old model keys and the note "No model found". It checks the new text with the config loader before it writes (11.3).
7. Setup sends one short prompt to the model of the new config, through `model_local.rs`, the same code as a story program model call (9.7, decision 10).

A failed step prints one line: what failed, the reason, and the next step ("To try again, run gnomish-relay setup --timeways"). Setup goes on and ends as usual. When only step 7 fails, the model stays in the config, and the next step is "Check that Ollama is running, then run gnomish-relay restart".

**Network of the story program.** The story program still has no network (6.6.4). It never talks to the model: it sends a `model_call` to the bridge (9.8), and the bridge runs `curl` outside the story sandbox, to the literal loopback address of `local_url`. The Ollama service listens on `127.0.0.1:11434` by default, and the config accepts only a loopback URL (12). So the local model needs no sandbox rule change. Setup does not add port 11434 to `[sandbox] local_ports` here: that key opens the port to the relay agents, and this step is for Timeways only.

**Later: a hosted model (B).** A hosted service that the player pays for can come later as one more `model` value of `[story]`, with no addon change. Its shape:

- `[story] model = "<service>"` and a key for its model name, like `claude_model` and `local_model`. The service API key lives in a file of the config folder, which the sandboxes hide (6.6.3, 6.6.4), never in `config.toml`.
- One more `ModelChoice` variant in `model.rs`, and one module `model_<service>.rs` with an `ask` that takes the prompt and returns the text. `ask_of` in `model.rs` maps each variant to its `ask`. The open calls, the budget, the size limits, and the answer cleaning stay the same for every route.
- The bridge makes the call, not the story program, so the story sandbox keeps no network. The call uses `curl` with `--proto =https`, a fixed host of the service, and no redirect.
- The addon shows `story_model` as plain text (13, Diag), so a new value such as `<service> <model>` needs no addon change.

Nothing in the code blocks this today.

## 12. Config

The config file is `config.toml` in the config folder of the OS:

| OS | Config folder | Data folder (`state.json`) |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/gnomish-relay`, or `~/.config/gnomish-relay` | `$XDG_DATA_HOME/gnomish-relay`, or `~/.local/share/gnomish-relay` |
| macOS | `~/Library/Application Support/gnomish-relay` | the same |
| Windows | `%APPDATA%\gnomish-relay` | `%LOCALAPPDATA%\gnomish-relay` |

`gnomish-relay setup` writes the first config. It never changes an existing key, except `path` of `[wow]` with `--wow` (11.3), and `program` and `lore_pack` of `[story]` after it installs Timeways (11.4). It adds the `[story]` model keys only when `[story]` has no model (11.6). It adds only these missing parts: a `[story]` section with `--timeways`, the relay part in a plain setup (9.7, decision 15), and an `[agents.<name>]` entry for each known agent on `PATH` that a relay config lacks. `default_agent` stays, so setup prints "Added agent: <name>. Pick it for a new chat in the game, in Settings". A config with an inline `agents` table gets no new entry.

The bridge accepts only the keys that it implements. Any other key is an error, so a typo never leaves a wider default in place.
Today these keys work: `allowed_roots`, `default_cwd`, `default_agent`, `timeout_minutes`, `permission_timeout_minutes`, `max_parallel_runs`, `daily_cost_cap_usd`, `allow_full_auto`, `auto_update`, `[wow] path` (missing only until setup finds the game, 11.3), `[agents.<name>]` with `kind`, `command`, `permission`, `env`, `modes`, `agent_hosts`, `preset`, and `resume`, `[allow]` with `commands` and `[allow.folders]`, `[sandbox]` with `allow_hosts`, `default_hosts`, `local_ports`, and `agent_network`, `[git]` with `ci_checks`, and `[story]` with `program`, `lore_pack`, `timeout_seconds`, `model`, `claude_model`, `local_url`, `local_model`, `model_timeout_seconds`, and `budget_window_minutes`.

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

- The bridge never looks up `program` on `PATH`. A name with no folder is an error, for `program` and `lore_pack`.
- `program` and `lore_pack` go together: both, or neither. With neither, the story program does not start, the bridge logs one line at start, and each Timeways message gets "Timeways isn't running on your computer. Run gnomish-relay restart.". Setup writes them as commented lines, and sets both when it installs the Timeways programs and builds the lore pack (11.4).
- With no `[story]`, each Timeways message gets the same answer.
- With `[story]` and no `timeways.key`, the bridge logs one line and starts no story program.
- `local_url` is only `http://127.0.0.1:<port>` or `http://[::1]:<port>`, with nothing after the port. Config load refuses `localhost`, any other host, `https`, and a path, because `localhost` can resolve to another host.
- `model = "local"` needs `local_url` and `local_model`. A key of one model with the other model, or with no `model`, is an error, so a typo never leaves a model that the user did not mean.
- A model name has no space and does not start with `-`, because `claude_model` goes into an argument of `claude`.
- The model route takes nothing from `[agents.*]`: `model = "claude"` always runs `claude` from `PATH`, with the environment allowlist of 6.2 and no `env` list (9.7, decision 10).

**A config with no relay part.** `allowed_roots` alone turns the relay on. With `allowed_roots`, `default_agent` and its `[agents.<name>]` entry are needed, as before. With no `allowed_roots`, each of `default_agent`, `default_cwd`, `timeout_minutes`, `permission_timeout_minutes`, `max_parallel_runs`, `daily_cost_cap_usd`, `allow_full_auto`, `[agents]`, `[allow]`, `[sandbox]`, and `[git]` is an error ("<key> needs allowed_roots"), so a typo never leaves a relay half set up. Setup gives a player with only Timeways this config (9.7, decision 15):

```toml
[wow]
path = "~/Games/battlenet/drive_c/Program Files (x86)/World of Warcraft/_classic_beta_"

[story]
model = "claude"
claude_model = "haiku"
```

With no relay part, the bridge has no relay lane. It does not write the relay key addon, and it logs and drops a strip signed with `strip.key`. `say` and `check-agent` stop with "coding agents are off. To turn them on, run gnomish-relay setup --relay". Setup still makes `strip.key`, so the key check of 9.7, decision 1, stays the same.

**The allow table** lists the commands that run from the game with no question at `auto-edit` and `full-auto` (9.3):

```toml
[allow]
commands = ["cargo test *", "cargo fmt --check"]

[allow.folders]
"~/Code/lighthouse" = ["npm test *"]
```

- A pattern is plain words with spaces between them. It covers every command that starts with these words. So a last `*` only shows that more words can follow: `cargo test *` and `cargo test` are one rule.
- A word with shell syntax (`*`, `?`, `[`, `]`, `$`, a backtick, a quote, `\`, `;`, `&`, `|`, `<`, `>`, `(`, `)`, `{`, `}`, `~`, `#`, or `=`) is an error, and so is an empty pattern.
- `commands` applies to every chat. A folder of `[allow.folders]` must exist, and its patterns apply to each chat inside it.
- A pattern never allows a `deny`, `desktop`, or "never always" command (6.6.3, S17). A config with no `[allow]` has an empty table.
- An OS dialog, `gnomish-relay approve`, and `gnomish-relay deny` answer the desktop requests of 6.6.3. They live in `approvals` in the data folder.

**The hosts of the sandbox** are the only hosts that a game-run command reaches, through the bridge proxy (6.6.4):

```toml
[sandbox]
allow_hosts = ["nodejs.org"]   # added to the default hosts of 6.6.4
default_hosts = true           # false leaves only allow_hosts
local_ports = [5432, 3000]     # ports of this computer for the agent and its commands
agent_network = "open"         # "strict": the agent reaches only its model hosts and agent_hosts
```

- A host is an exact name, compared without ASCII case. It has at least one dot, and its last label starts with a letter. A `*`, a port, a scheme, an IP address in any form, and `localhost` are errors, so a typo never opens more than one name.
- With `default_hosts = false`, no `allow_hosts`, and no `local_ports`, the proxy does not start, and commands have no network.
- `local_ports` (6.6.4, "`local_ports`") lists loopback ports of this computer, for example a database or a dev server. 2375, 2376 (Docker), 9222 (the browser debugger), and 3128 (the sandbox forwarder) are errors.
- Hosts that a user can add: `nodejs.org` (headers for npm native modules), `proxy.golang.org` and `sum.golang.org` (Go modules).
- Only the desktop changes `config.toml` (6.6.2), so no game message adds a host.

**Git** (9.11):

```toml
[git]
ci_checks = true   # show the CI checks of the pull request of a chat branch, through gh
```

- `ci_checks` is `false` by default: it is the only bridge network call with a user login (9.11, "CI checks"). With no relay part, `[git]` is an error ("[git] needs allowed_roots").
- Own branch, the change summary, and the test line need no key. They work in every repository.

The other keys below come with their features. One planned key is not in the config yet: `max_messages_per_minute` (6.2, rule 4). Today the bridge refuses it, so the example leaves it out. A test loads this example, so the example and the loader never differ.
`max_parallel_runs` is 1 to 16 (8.2). `daily_cost_cap_usd` is a number of US dollars above 0 and at most 10000 (9.10). With no key, there is no cap.
`allow_full_auto` is `true` by default: a chat can switch to full-auto after one desktop Approve (9.3, "Full-auto for one chat"). `false` turns full-auto off for every chat. With no relay part, it is an error ("allow_full_auto needs allowed_roots").
Each root and `default_cwd` must exist. The bridge resolves links in them at start. `default_cwd` must be inside a root, or be the home folder. With no `default_cwd`, it is the first root, or the home folder when `allowed_roots` is empty. An empty `allowed_roots` still turns the relay on: every folder then needs a click on the desktop. A click on the desktop adds a root to this list (9.12).

**The rule of the default folder.** One function, `default_folder` in `config.rs`, checks `default_cwd`. Config load, the desktop app start, `status`, `gnomish-relay setup`, and `setup --timeways` all use it. Why the home folder is allowed: `default_cwd` is the base of the old folder texts of the game (9.9) and the folder of a chat with no choice, not a folder where an agent works. The browser lists the home folder, and Resume lists its sessions (9.12). Setup itself writes `default_cwd = "~"` next to the first root (9.12, decision 5). The home folder still never becomes a root, and no chat runs in it (9.12).

**Setup repairs the default folder.** Setup never writes a config that this rule refuses. When the config of an earlier version breaks the rule (a `default_cwd` outside every root, or a missing one), both setups set `default_cwd = "~"`, which the rule always allows. They print one line, for example "Fixed config.toml: default_cwd /srv isn't in allowed_roots, so it's now ~ (your home folder)." Every other config line stays. Why `~`: it is the base that the folder browser shows, and with no roots it is the base already.

```toml
default_cwd = "~/Documents/Code"
allowed_roots = ["~/Documents/Code"]
timeout_minutes = 30
permission_timeout_minutes = 10
max_parallel_runs = 3         # runs over the limit wait for their turn (8.2)
daily_cost_cap_usd = 5.0      # optional; no new run after $5 of agent cost in a UTC day (9.10)
allow_full_auto = true        # false: no chat runs at full-auto (9.3)
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

The window follows the classic Guild & Communities frame, with built-in game textures and fonts. The mockup is the layout reference.

- **Frame:** the dark metal frame, a black title bar with the gold title "Gnomish Relay", and gold-framed red minimize and close buttons.
- **Size:** a grip at the bottom-right corner resizes the window from 900 × 560 up to the screen size. The saved variables keep the size. A saved size larger than the screen (for example after a larger UI scale) opens at the screen size. The transcript, input, folder browser, and Settings and Diag pages grow with the window, and a taller window shows more Diag lines. The chat and Activity columns keep their width. The transcript redraws at the end of a resize, not during it.
- **Bridge light:** a dot and a label at the right of the title bar, so every tab shows it: "Connecting..." in grey until the first poll, "Connected" in green, "Slow connection" in amber, and "Desktop app offline" in red (7.4). A channel problem (7.8) or a version mismatch (7.7) also shows here in red.
- **Portrait:** a round emblem at the top-left: a red pipe wrench on a brass cog, our own drawing, shipped as a texture.
- **Left column:** one tile per chat, with the agent as the shield icon. The selected tile glows green. A gold "!" marks a new reply. An orange "?" marks a chat whose permission popup waits. The last tiles are "New chat" and "Resume".
  - A long chat or agent name ends in "..." left of the mark, so it never runs over the transcript (fixed 2026-09-30).
  - When the tiles do not fit, the mouse wheel scrolls them. A new chat scrolls to the end and opens the folder browser in the center (9.9). Escape or Cancel closes it, and the chat keeps the default folder.
  - Resume shows the picker of 9.6 in the center: a gold heading per folder, then one row per session with title, agent, and age, or a green "open" for an active session.
  - A right-click on a tile opens a menu: the chat name in gold, **Pop out** (see "Mini chats"), and **Delete** in red. It closes at a choice or a click outside it. **Delete** asks `Delete "<name>"?`, or `Stop and delete "<name>"?` while the agent works, with **Delete** and **Cancel**. The question is a game dialog (`StaticPopupDialogs`), so it has the game border and Escape closes it.
- **Center:** one row with a dropdown for the agent and permission mode, the folder button, and at the right end Search, the Pinned button, and the red **Pop out** button (see "Mini chats"). A long folder name is cut so it never covers them. A long agent name and mode end in "..." before the folder button.
  - The folder button shows a folder icon, the chat folder, and a dropdown arrow, and turns gold on hover. A click opens the folder browser of 9.9 in place of the transcript, and the input stays. A second click, a choice, Cancel, or Escape in the search box closes it.
  - Below, the transcript on black: `[You]: text` and `[Claude]: text`. The text is white. Only the name has a color: the user in blue, each agent in its own color.
  - A sent message with no final reply shows its delivery state in grey at its right: "Sending...", "Retry 2 of 3" at the second show of its strip (7.1.1), "Delivered" once a body holds its record, and "Needs reload" while it waits in the outbox (7.5). The state goes when the reply comes.
  - The mouse wheel scrolls the transcript, and a new entry scrolls it to the bottom. A new entry draws below the others, and old entries stay as they are. The whole chat redraws only when the chat, font size, or width changes, or the history drops its first entry. Commit, Revert, and their answers redraw from their reply down.
- **Permission mode** (asked for by the user on 2026-09-30, after the modes of Claude Code, 9.3). The header shows agent and mode: "Claude · auto-edit". `full-auto` always shows in red-orange (`ff5a1f`), so the player always sees it.
  - A click on the label opens a list of "ask", "auto-edit", and "full-auto". It closes at a choice or a click outside it, as the Settings dropdowns do.
  - Shift+Tab in the message box moves to the next mode, as in Claude Code: ask, auto-edit, full-auto, then ask again.
  - A change applies to the next message, and the header shows it at once. The first message at full-auto asks on your desktop (9.3, "Full-auto for one chat"). If the desktop says no, the level line of the run puts "auto-edit" back in the header.
  - The addon changes only the chat mode (`chat.mode`, sent as `level=`). The bridge decides what the chat gets.
- **Replies:** a rendered reply (7.3.1) shows its blocks below the name.
  - Headings, paragraphs, list items, and quotes go into one SimpleHTML frame, with real sizes for `h1` to `h3`, and a bullet or number before each item.
  - Code shows in a black box in the shipped mono font (13.2).
  - A table is a grid of font strings with a gold header row. With more than 8 columns, or too wide for the transcript, each row shows as a card: the first cell in gold, each other cell below it with its column name.
  - Under a reply, top to bottom: its blocks, the change block, the test line, the CI line (9.11), and the usage line (9.10) in grey. The Pin link stays at the right end of the name line.
  - If anything fails while a reply draws, it shows as plain text. User messages, errors, and replies from before 7.3.1 stay plain text.
- **Summary first** (asked for by the user on 2026-09-29, changed 2026-09-30). The window always shows a reply in full, also a long one. The user said: "I should never have to click Show more if I'm in the relay screen." So there is no "Show more" link. The summary serves only the whisper line (13.1, "Game chat").
  - The summary is the first paragraph of the reply, and the agent writes it. The `claude` and `codex` backends add this summary note to the system prompt (9.2): "The user reads your replies in a small window inside a game. When a reply is longer than about 8 lines, start it with a summary of one or two short sentences as its own paragraph. Put no heading or label before the summary."
  - The summary has no marker: a marker is noise in the window, in a terminal session of the same agent, and in the whisper line. The whisper line shows the first block, which is the summary.
  - An agent that ignores the note still gets a whisper line: its first block. ACP and `command` agents get no note. ACP has no system prompt, and a note in the prompt goes into the user's saved session and the Resume list.
  - **The sandbox line** (added 2026-09-30). The note ends with: "Some files are hidden from you by the sandbox: secrets, .git/config, and the settings of tools such as ~/.claude and ~/.config/gh. They look empty or missing, but nothing deleted them. Don't report them as lost." On 2026-09-30 an agent told the user: "Something wiped .git/config and ~/.config/gh/hosts.yml, and ~/.claude/CLAUDE.md is gone". Nothing was wiped: the sandbox (6.6.4) and the agent wall hide these paths. ACP and `command` agents get no line, for the same reason as the note.
- **Pinned replies** (asked for by the user on 2026-09-29). A blue "Pin" link at the right end of the name line of each agent reply pins it, then shows a gold "Unpin".
  - At the right end of the header, a "Pinned 2" button with the count opens the chat's pinned replies, oldest first, each with its first words. The list shows 12 rows at a time, and the mouse wheel scrolls it.
  - A click on a row closes the list and jumps to the reply: the transcript scrolls it to the top and marks it with a gold band.
  - With no pin, the list says "No pinned replies yet. Click Pin on a reply to keep it here." The list closes at a click outside it, as a game menu does, and with the window.
  - A pin is a field of the reply in the chat history, in the saved variables. So each chat has its own pins, a `/reload` keeps them, and Delete removes them with the chat. A reply that the history drops (200 entries) takes its pin with it. A restore bundle (7.6) brings no pins. The bridge never sees a pin.
  - Only agent replies have a pin: the player wrote the messages, and an error has Resend.
- **Search** (asked for by the user on 2026-09-29). A "Search" button at the right end of the header, left of Pinned, and the key binding "Search chat" open a search bar with the focus in its box. It finds text in the chat on screen.
  - The bar: a box with the grey hint "Search this chat", the count ("2 of 5", or "No matches"), and **Previous**, **Next**, and **Close**.
  - The search ignores case. It reads the plain words of each entry: the text of a message or error, and the plain words of a reply (7.3.1). An entry is one match, because the game gives no position of a word inside a wrapped text.
  - Each change of the text jumps to the newest match, as the chat starts at the bottom. Previous jumps to the match above, Next to the one below, and both wrap around. A jump works as the jump to a pin: the entry scrolls to the top with a gold band.
  - Enter clears the focus, so the game keys work again, and the bar stays for Previous and Next. Escape in the box, Close, or a change of chat closes the bar and removes the band.
  - The bar takes the row above the input. While open, it comes before the "Reload soon" banner too: the player asked for it.
- **The row above the input** holds the search bar, the "Reload soon" banner, or the input byte counter. While none shows, the transcript takes the row, and keeps its newest line in view when the row comes and goes.
  - **No Ctrl+F of the window.** A WoW window has no keyboard focus. To see Ctrl+F, a frame needs `EnableKeyboard` and `SetPropagateKeyboardInput`. The game restricts `SetPropagateKeyboardInput` in combat (`HasRestrictions` in the API documentation of the Forever client). A frame that holds the keyboard in combat then eats every key, also the move keys. So the addon never takes the keyboard outside an edit box. The player can bind Ctrl+F to "Search chat" in the Key Bindings menu.
- **Errors:** an error comes from the relay, not the agent, so it shows as a grey line `[Relay]: Not sent.`, never under the agent name. Below it, a blue "Resend" link sends the message again. Before a `/reload`, the addon still holds the text in its private table, so Resend signs and sends it at once. After a `/reload`, only the saved variables hold the text, and the addon never signs that text (6.6.1). Then Resend puts the text in the input with the focus, and Enter sends it.
- **Input:** one line, no label. While it is empty and unfocused, it shows a grey hint: "Type a message, then press Enter." Enter sends, empties the line, and clears the focus, so the game keys work again. The limit is the room of one strip: a payload of 3200 bytes (7.1), less the other record fields and 440 bytes for the report. That leaves about 2600 bytes of text. With fewer than 400 bytes left, a small counter above the right end says "100 left". Past the limit it says "5 over the limit" in red.
- **Earlier messages in the input** (asked for by the user on 2026-09-30: "up arrow down arrow in the relay message screen should scroll through historical messages, like in a normal terminal"). The keys work as in bash, zsh, and the WoW chat box:
  - Each Up goes one sent message further back, and Down one forward. At the oldest message, Up does nothing.
  - Down past the newest message puts back the text typed before the first Up. Typed text is never lost.
  - The cursor goes to the end of the text.
  - Each chat has its own list: the player's messages in the chat history. So the list survives a `/reload`, with no new saved data. A failed message counts. An empty message, a commit message, and the hidden message of a Resume do not. Two identical messages in a row count once, as in a shell.
  - A send or a change of chat starts the list again at the newest message.
  - The input is one line, so Up and Down always go through the list. The input calls `SetAltArrowKeyMode(false)`, so `OnArrowPressed` gets the arrow keys without Alt. The mode is per edit box, so the folder browser search box keeps its own arrow keys.
- **Quick actions** (asked for by the user on 2026-09-29, changed to suggestions on 2026-09-30). An empty chat (no message yet) suggests tasks that the player sends often, for example between two pulls. The transcript center shows one grey line, "Try one of these:", and below it one full-width row per quick action, showing its message. A message too long for one line ends in "...", and the row tooltip shows it whole. A click on a row sends that message exactly as if the player typed it: the same record, signature, limits, and trust rules (6.6.1). The defaults, in order:

  | Message |
  |---|
  | Run the tests and tell me what fails |
  | Fix the failing tests |
  | Show git status |
  | Summarize my uncommitted changes |
  | Open a pull request for these changes |

  - **Why suggestions, not buttons.** In 0.3.0 the quick actions were small buttons above the input, with two-word names such as "Run tests". In a game test, the player did not understand them: only the tooltip showed what a button sends. Chat apps (ChatGPT, Claude.ai) show suggestions in an empty chat, with the words that they send. The player knows that pattern, and the row above the input goes back to the transcript.
  - **Only in an empty chat.** The first message hides the rows, also after a `/reload`, because the saved history holds it. A new chat shows them again. The rows show at every window size from 900 × 560 up.
  - **One list for all chats.** The agent runs each task in the chat folder, so the tasks do not depend on the chat. One list is one place to edit, and each new chat has the suggestions at once. A list per chat makes the player set up each chat again.
  - At most 6 actions. An action with no message does not show. With none to show, the empty chat shows no line and no row.
  - **The saved list.** Each action keeps a name and a message, the 0.3.0 format, so a 0.3.0 list still loads. No screen shows the name now. A saved list that still holds the 0.3.0 defaults gets the new defaults, because the old messages were long for a row.
- **Mini chats** (asked for by the user on 2026-10-01; the design is the canvas "Gnomish Relay mini chats"). A mini chat is one chat popped out into its own small window, so the player can watch it while playing. The implementer and an advisor agent chose these rules on 2026-10-01.
  - **Pop out:** the **Pop out** button of the chat header, or **Pop out** in the tile's right-click menu.
  - **The mini chat:** 340 × 270 at first. Its grip resizes it from 260 × 160 up to the screen size.
    - The header holds a gold dot for a new reply, the chat name, "· Claude", the folder in grey, and the bridge dot. The folder and the dot have tooltips: the full folder and the bridge light label. At its right: **Open in main window** (a back arrow, named in its tooltip) and the close button.
    - Below the header: the transcript, the desktop request box (6.6.3), the mode line, and the message box with the hint "Type a message, then press Enter."
    - The mode line shows `> ask`, `> auto-edit`, or `>> full-auto`. Full-auto is red-orange (`ff5a1f`), as in the window header. The game fonts have no "⏵", so the line uses ">". A click on the mode opens the same list as in the window. At the right of the line, in grey: "Working..." while a run works, and "Shift+Tab to change" while the box has the focus. The mini chat has no cast bar, so "Working..." tells the player that a long run still goes.
  - **One message list.** A mini chat draws the saved history of its chat with the window's transcript code. Pins, Commit, Revert, and Resend work there and act on its chat. The window never draws a popped-out chat. When the selected chat pops out, the window shows the first chat that is not popped out. With no such chat, the center is empty, and a message there makes a new chat.
  - **Keys:** as in the window. Enter sends to its chat, empties the box, and clears the focus. Escape clears the focus. Up and Down go through earlier messages. Shift+Tab moves to the next mode.
  - **A new reply:** the header turns gold with a gold dot, and the tile shows its "!", until the player clicks in the mini chat or focuses its box. A reply that comes while the box has the focus is already read. A popped-out chat's reply gets no whisper line: the gold bar is the notice. The whisper sound still plays, under the Reply whisper and Sound settings, because a player who looks at the map sees no bar. A desktop request still gets its line and sound (6.6.3).
  - **The tile:** a popped-out chat keeps its tile, dimmed, with "Popped out. Click to show it." in place of the agent. The marks "?", "!", and "..." stay. A click on the tile, or on the chat's whisper link, brings the mini chat to the front with the focus in its box. The right-click menu says **Open in main window** in place of **Pop out**.
  - **Close and Open in main window.** The close button ends the pop-out: the chat goes back to its tile, and the window does not change. **Open in main window** also ends the pop-out and opens the window on the chat. Only the window has Stop and Activity, so this button is the way to them.
  - **At most 4 at once**, because four mini chats cover a big part of the screen. At 4, **Pop out** is grey, and its tooltip says "You can pop out up to 4 chats. Close one first." A click then shows the same line in the game error frame. Grey, not disabled: Disable of a button is a protected client function.
  - **Saved:** per chat, the saved variables keep whether it is popped out (`poppedOut`) and the position and size of its mini chat (`mini`). So mini chats come back after a `/reload` or a login, and a chat that pops out again opens at its last position. A new mini chat opens in two columns at the right of the screen. A saved size larger than the screen opens at the screen size. A change of resolution or UI scale puts each mini chat at its position again, and the clamp keeps it on screen. A deleted chat closes its mini chat, and its saved position goes with it.
  - **The window and Escape.** A mini chat is its own window: closing or opening the main window does not change it. It is not in `UISpecialFrames`, so Escape never closes it: players press Escape all the time in a fight.
  - **Combat.** A mini chat is a plain frame on `UIParent` with no protected part, so it works in combat, and Alt+Z hides it with the rest of the UI. The addon focuses its box only after a click of the player, never on its own: focus in combat sends the player's ability keys into the box.
  - **Font size:** the setting applies to the text and box of each mini chat.
  - No Search, Pinned list, folder browser, or quick actions in a mini chat. They need room, and the window has them.
  - The design also showed **Rename** in the tile menu. The addon has no rename yet, so the menu has no such item.
- **Right column, Activity:** a cast bar while the agent works, and one row per step, with a details tooltip. While a popup of the chat waits, the cast bar stands still in grey and says "Waiting for your approval" in orange: the run makes no progress then. While the message waits for the limit on parallel runs (8.2), the bar stands still in grey and shows the bridge's waiting line, for example "Waiting: 3 other chats are running". The bar text stays inside the bar, and a line that is too long ends in "...". At the bottom, a grey line gives the time to the next poll: "Checking again in 12s". The bar and this line change at most 5 times a second.
- **Side tabs:** Chats, Settings, and Diag, on the right edge of the window. The window clamp counts the tabs as part of the window, so the tabs stay on screen. Notifications get no tab: a bell at the minimap shows them (10.4). Settings and Diag replace the center and the Activity panel. The chat tiles stay on the left, and a click on a tile goes back to Chats.
- **Settings** (asked for by the user, decided with an advisor on 2026-09-26, 13.5). The page, in order:
  - **New chats:** Agent, a dropdown of the agents in the settings list (13.4), and Permissions, a dropdown of `ask` and `auto-edit`. After the level, a grey hint: "Up to <level> (set on your desktop)", the level of the chosen agent in the config. Below the row, a grey line says what the lower of the two levels does:
    - at `ask`: "Asks before each edit and each command."
    - at `auto-edit` with a Claude agent and a sandbox in the settings list: "Edits the chat folder and runs commands in the sandbox on its own. Asks for risky commands."
    - at `auto-edit` for a `command` agent: "Edits the chat folder and runs its own commands in the sandbox."
    - else: "Edits the chat folder on its own. Asks before each command."
  - **Appearance:** Font size, a slider from 12 to 20 (default 14). It applies at once to all chat text: headings, paragraphs, code boxes, tables, and the input. The window keeps its size, and long lines wrap. Reply whisper: an on and off box, 5 colors (default copper `f0a860`), and a Sound box, with a preview of the whisper line below. Window position: **Reset** puts the window in the center at its first size (900 × 560). In the same row, Quick actions: **Edit** opens the quick action editor in place of the page.
  - **The quick action editor:** the grey hint "An empty chat suggests these. A click sends one.", then one row per action: its message, **Move up**, **Move down**, and **Remove**. No screen shows the action name, so the editor has no box for it. Enter, a click elsewhere, or a click on an editor button saves a changed message. Escape puts the old text back. An empty message keeps the old one. Below the rows: **Add** (a new empty row with the focus, up to 6), **Reset** (the defaults), and **Done** (back to the page). The editor closes with the page.
  - **Notifications** (section 10), after Appearance, only after `hooks install`. Notifications: an on and off box (default on). Off stops the lines, sounds, banners, bell, and faster polls of 10.4, and greys the other two rows. Finished tasks: a dropdown of Always, Over 1 min (default), Over 3 min, and Never. Alerts: three boxes, Chat line, Sound, and Banner (default on).
  - **Always allowed** (6.6.5): one row per rule of the settings list, with the pattern, folder, last use, and a remove button. It shows 6 rows at a time (3 while the Notifications group shows), and the mouse wheel scrolls it. With no rule: "No rules yet. Click Always allow in a popup to add one."
  - Bottom left, today's usage (9.10): "Today (UTC): 12k in · 4.1k out · $1.20", with " · limit $5.00" when the config sets a cap. With no usage today, the line is empty.
  - Bottom right, the status line: "Online · 2m ago", the age of the settings list. It is orange when the list is older than 10 minutes, and grey "Offline · <age>" while the bridge is offline. With no list, it says "Not loaded yet". A click asks for a new list.
- **Diag:** the bridge's settings list, read only: the status, allowed roots, default folder, agents with their levels, the allow table with the patterns of each folder, the timeouts, the limit on parallel runs, the sandbox, and the strip line (7.1.4). With `[story]`, the Timeways model and budget. After `hooks install`, the rows of 10.4: Hooks, Sessions, and Last notification. Then the versions and the lines of `/relay diag`. While the bridge is offline, its values are grey. The mouse wheel scrolls the page.
- **Key binding:** `Bindings.xml` adds "Toggle window" and "Search chat" under "Gnomish Relay" in the game's Key Bindings menu. They call the globals `GnomishRelay_Toggle` and `GnomishRelay_Search`. "Search chat" opens the window on its chat and opens the search. Neither has a default key.
- **Bottom bar:** a red **Stop** button, only while an agent works. It stops the run.
- **Game chat:** a finished reply shows one line, `[Claude] whispers: [chat] …`, in its own color (a setting, copper by default). For a rendered reply, it shows the plain words of the first block. The usage line (9.10) never shows there. A click on the line opens the chat, and the line plays the whisper sound. Settings can turn the line or its sound off. A desktop request (6.6.3) always gets its line, because it is its only notice in the game. A terminal session notification gets its own line with a bell (10.4). A popped-out chat's reply gets only the sound (see "Mini chats").
- **Permission requests** use the separate popup of 6.4, never the window. A desktop request has no popup: an Activity row and one whisper line (6.6.3).
- **Git** (9.11). The addon takes a chat's branch from the `B` block of its last reply.
  - **Own branch:** a check box at the right end of the row above the chat header, while the chat has no message and its folder is in a repository (the `git` mark of the tree). It starts on when another chat with a message has the same folder. Its tooltip: "Work on a separate branch in a separate copy, so other chats don't touch these files."
  - **The branch bar:** after the first reply in a repository, the same place shows the branch in grey and small buttons. A long branch name ends in "..." at the left edge of the transcript. The buttons: **Merge** and **Discard** for an own branch, and **Checks** for any branch. Discard asks first, in a game dialog: "Discard this chat's branch? This deletes gnomish/fix-tests and its folder." with **Discard** and **Cancel**. Search and Pinned fill the right end of the header row, so the box and the bar take the row above it. The bar hides while the folder browser, Resume, Settings, or Diag shows.
  - **The change block** under a reply or an error: a gold line "3 files changed", then `+40 −2` in green and red, and **Commit** and **Revert** at its right. Below, one row per file: the path in grey, then its counts, or "new" or "removed". Then "Tests: 412 passed, 2 failed" and "CI: 5 passed, 1 failed (lint)", with each failed number in red.
  - **Commit** opens a small dialog with the dark game border, above the screen center: a gold title "Commit changes", the message in an edit box, and **Commit** and **Cancel**. Enter commits, and Escape cancels, also when the edit box has no focus. The dialog closes with the window and when its chat is deleted. An empty message greys **Commit**, and Enter then sends nothing. **Revert** asks first: "Revert the changes of this reply? This puts back 3 files as they were before it." with **Revert** and **Cancel**.
  - A click sends a chat message, so the transcript shows `[You]: Commit "fix the retry test"`, `[You]: Revert`, `[You]: Merge`, `[You]: Discard`, or `[You]: Checks`, with its delivery state. The answer is a grey line, for example `[Relay]: Committed 3 files as a1b2c3d on gnomish/fix-tests.`, with no whisper and no Resend. While a Commit or Revert is on its way, the block says "Sending..." in place of its buttons. After an answer with no error, it says "Committed" or "Reverted" in grey. After an error, or when the message is not sent, the buttons come back.
  - The addon draws bridge blocks only for a reply or error that has them. An error that looks rendered but has no bridge block stays plain text, as before.

### 13.2 Code

The addon is our own code. It uses the design of `wow-claude`, not its files.
All state is local to the addon files, which share one table. The files load in this order.
The "shared" files are in `addon/transport` (9.7, decision 14). They read the app names from `App.lua`.

| File | Job |
|---|---|
| `App.lua` | The app names: title, hello chat, slot prefix, the three slot globals, strip frame, saved variables, and the key addon with its global. |
| `KeyHandoff.lua` (shared) | Loads the key addon and takes the strip key into `ns.key` (7.3.2). |
| `Sha256.lua` (shared) | SHA-256 and HMAC-SHA256 for the strip tag. |
| `Codec.lua` (shared) | Records, frames, and cells: the Lua side of `crates/protocol`. |
| `Saved.lua` (shared) | The saved variables table of the app. |
| `Store.lua` | The relay's saved data: chats, deletes, and settings. |
| `Health.lua` (shared) | The login self-test, the health of each channel (7.8), and the line for a blocked strip corner (7.1.2). Its lines start with the app title. |
| `Strip.lua` (shared) | Takes the shared strip corner in turn with the other apps (7.1.2), draws a frame, and takes one screenshot of it. |
| `Slots.lua` (shared) | Loads one slot, and takes the three app globals. |
| `Messages.lua` (shared) | The send queue, the signed outbox, retries and give-up, the hello, the report flags (`next`, `read`, `restored`, and the health flags), and the slot poll with the replies. It follows `models/transport.qnt`. It keeps the token and the message ids. An app sets its hooks: the store of its messages, the record fields, and the calls for each reply. |
| `Transport.lua` | The relay on top of `Messages.lua`: the coding flags, the session list, the folder tree request, Stop, Delete and its `d` records, the restore bundle, the live file, and the permission answers. |
| `Notices.lua` | Terminal session notifications (10.4): the list, the filter, the chat line, the sound, Clear, and the faster polls. |
| `Blocks.lua` | Splits a rendered reply (7.3.1) into blocks and fields, and gives its plain words. It also reads the bridge blocks (9.11). |
| `Pins.lua` | The pinned replies of 13.1: the Pinned button and its list. |
| `Search.lua` | The search bar of 13.1 and its matches. |
| `QuickActions.lua` | The quick action list (13.1) in the saved variables: defaults and edits. |
| `Suggestions.lua` | The quick actions as suggestions in an empty chat. |
| `QuickEditor.lua` | The quick action editor in the Settings tab. |
| `Changes.lua` | The bridge blocks under a reply (9.11): the change block, the test and CI lines, and the Commit and Revert dialogs. |
| `GitBar.lua` | The Own branch box and the branch bar of the chat header (9.11), with the Discard dialog. |
| `DesktopRequest.lua` | The desktop request box between the transcript and the input (6.6.3, "The request in the chat"), with the Copy button. The window and each mini chat have their own box. |
| `Transcript.lua` | The transcript of the window and each mini chat: a scroll frame that stacks entries and draws blocks. |
| `Folders.lua` | The folder tree of 9.9: the parser, folder texts in the home form and the old form, the filter, the recent folders, and the name rules. |
| `Browser.lua` | The folder browser of 9.9 in the window center. |
| `BridgeSettings.lua` | The settings list of 13.4: the parser, the cache, and the agent of a new chat. |
| `RulesGroup.lua` | The "Always allowed" group of the Settings tab (6.6.5). |
| `SettingsTab.lua` | The Settings tab of 13.1. |
| `DiagTab.lua` | The Diag tab of 13.1. |
| `Window.lua` | The window of 13.1, its side tabs, and its position. |
| `ChatMenu.lua` | The right-click menu of a chat tile: Pop out and Delete. |
| `MiniChat.lua` | The mini chats of 13.1: up to 4 small windows, each with the transcript, desktop request box, mode line, and message box of one chat. |
| `Popup.lua` | The permission popup (6.4). Each button names the kind of its option, never the agent's label. |
| `NoticeFrames.lua` | The bell at the minimap, the notification list, and the toast (10.4). |
| `SetupNeeded.lua` | The first-run window with no key (7.3.2). |
| `Core.lua` | Startup, slash commands, and the whisper line. |

**The hooks of `Messages.lua`.** An app sets them after the file loads. The defaults suit an app with one chat, such as Timeways. An advisor agent and the implementer chose this split (2026-09-26, 9.7 step 5b):

- A chat is always a table with an `id` field. Two accepted types are clever code.
- `Store` gives `Add`, `Open`, and `Find` for the messages. The default store keeps them in a `sent` list in the app's saved variables. It keeps the last 64 answered messages, because a later body can still hold their final replies, and the addon must report them in `read`. The relay keeps its messages in its chats, so its saved data did not change.
- `Fields` gives the `cwd`, `flags`, and `name` of a record. The relay puts its coding flags here.
- `Messages.lua` marks a message as answered, then calls `OnReply(chat, id, status, text)` once, or `OnGiveUp` for "Not sent" and "Too long". The default `OnGiveUp` calls `OnReply` with `error`. The relay adds a give-up to the history with no whisper, as before.
- `OnStatus` gets each record of a known message, also `working`. `OnOther` gets each record of no known message, and its result says whether the addon reports it as read (the relay's session list).
- `Control(chat, id, flags)` sends a record once and starts a strip. `Riders` are records that go only with a strip that goes out anyway, for example the relay's `d` records. A rider that started a strip starts one every second while the bridge is off. `d` is a coding flag (9.7, decision 6), so the deletes stay in `Transport.lua`.
- `OnPoll(restore, live)` gets the other files of each slot. `Awaits` keeps the fast poll schedule while the app waits for a reply that is not a message.
- `PollEvery` gives the seconds to the next poll while the app waits for something off the schedule, or nil. The default is nil, so Timeways does not change. The relay gives 5 while a desktop request waits (6.6.3), 15 while a run works (7.3), and 60 or 180 while a terminal session is open (10.4).
- The app title starts each line of `Health.lua`, and `helloChat` is the chat of a hello.

The folder also holds `JetBrainsMono-Regular.ttf`, the mono font of code boxes, with its license in `JetBrainsMono-OFL.txt` (SIL Open Font License 1.1). Setup installs both.
The game finds a new file only at launch. Until then, code boxes use the game's `Fonts\ARIALN.TTF`.

Message ids start from the clock, so ids after a saved-data wipe never repeat the ids in an older body.

The tests run the addon in a real Lua 5.1 with a fake WoW API (`addon/tests/wow.lua`), from `crates/bridge/tests`.
They decode each strip with the proved Rust decoder and check its tag against the Rust HMAC.
They also check the SHA code against both kinds of `bit` results: unsigned as in WoW, and signed as in LuaJIT.

The folder also holds `Bindings.xml`, the key binding of 13.1. The game reads it from the folder by itself, so the TOC does not list it.

**Settings of the addon.** The saved variables hold the font size, the reply line with its color and sound, the window position and size, the agent and level of new chats, and the quick actions. Each chat holds whether it is popped out, and its mini chat's position and size. A saved list that is not a list of names and messages gives the defaults. Settings apply at once, and the bridge never sees them. A chosen agent that the last settings list does not have gives the list's `default_agent`.

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

**Not planned now.** Nobody owns voice, and it has no date. This is a design note.

Voice comes after the ACP backend (step 9), because it needs a real agent to be useful.
Both directions run on the bridge side: the WoW client gives addons no microphone and no speech-to-text API.

**Voice output.** The bridge speaks each final reply on the desktop. The full text always stays in the window.

The config key `voice` sets what the voice reads:

| Value | The voice reads |
|---|---|
| `auto` (default) | The full reply if it has no code. With code, the text parts, and one short sound for each skipped code block, diff, and long path. |
| `full` | The full reply, also the code. |
| `summary` | One short spoken line, which the bridge asks the agent for at the end of each run. If the agent gives none, the first sentence. |

- Playback goes paragraph by paragraph. **Skip** jumps to the next paragraph, and **Stop** ends the reply.
- Skip and Stop have game keys. The addon sends `voice=skip` or `voice=stop` through the strip, so a key takes about half a second. The desktop has the same two hotkeys, with no delay.
- A new reply waits until the current one ends or you skip it.
- The default engine is a local model (Piper), so no reply text leaves the computer. A cloud voice is a config option.
- The fallback is `C_VoiceChat.SpeakText(voiceID, text, rate, volume)` in the game. It exists in the Forever client and uses the operating system's voices. Under Wine, the client can have no voices (17).
- The config turns voice output on per agent. It is off by default.

**Voice input.** You hold a key in the game and talk. The bridge records and transcribes.

1. Hold the addon's push-to-talk key. The addon sends a `listen=start` record for the open chat through the strip.
2. The bridge records the microphone.
3. Release the key. The addon sends `listen=stop`.
4. The bridge transcribes the audio on the computer (Whisper). The text becomes a chat message, with the same checks and ceiling as a typed message.
5. The window shows the text as your message.

**Privacy rules for voice input.** Any addon can send a game record (6.6.1), so a hostile addon can send `listen=start` and record the room.

- The bridge records only when the config turns voice input on. It is off by default.
- While the bridge records, the desktop shows a sign, and the game window shows a red "Listening" light.
- A recording stops after 60 seconds, also with no `listen=stop`.
- The bridge never stores the audio. It deletes it after the transcription.
- The config can require a desktop hotkey to start a recording, so no game record can start one. On Wayland, a global hotkey needs the portal (17).
- A transcript runs under the game ceiling (6.6.2). Voice gives no more rights than typing.

### 13.4 The settings list

The Settings and Diag tabs (13.1) show bridge values. The game never writes `config.toml` (6.6.2), so it only reads them. `settings_list.rs` writes the list.

**The request.** The addon sends a `list=settings` record of the chat `settings`, as for `list=folders` (9.9). The reply is one record. The addon keeps its text and time in its saved variables, so the tabs show the last list at once, also while the bridge is offline.

**The reply.** One line per value: `key \t value`. Some values hold more tabs, so a reader splits a line at its first tab only. A control character inside a field becomes a space. The lines come in this order:

| Key | Value |
|---|---|
| `version` | The bridge version. |
| `sandbox` | How commands from the game run (6.6.4), as the bridge prints it at start. |
| `default_cwd` | `default_cwd`, with `~/` for the home folder. |
| `allowed_root` | One root of `allowed_roots`, one line per root. |
| `default_agent` | The name of the default agent. |
| `agent` | `name \t kind \t level`, one line per agent. The level is the current config level, so a raise on the desktop (9.3) shows at the next list. |
| `timeout_minutes`, `permission_timeout_minutes` | The two timeouts. |
| `max_parallel_runs` | The limit on parallel runs (8.2). |
| `usage_today` | Today's tokens and cost, as the usage line of 9.10 shows them. Only after the first report of the day. |
| `daily_cost_cap_usd` | The cap, with two decimals. Only when the config sets one. |
| `story_model` | Only with `[story]`: `none`, `claude`, `claude <model>`, or `local <model>`. The address of a local model stays on the desktop. |
| `story_budget_window_minutes` | Only with `[story]`. |
| `strip` | The result of the newest line test, in the words of `gnomish-relay status` (7.1.4). |
| `rule` | `id \t folder \t pattern \t days`: one "Always allow" rule (6.6.5), with the days since its last use. |
| `allow` | One pattern of `[allow] commands`, as words. |
| `allow_folder` | `folder \t pattern`: one pattern of `[allow.folders]`. |
| `hook` | `agent \t state`, one line for `claude` and one for `codex` (10.5). The state is `on`, `off`, `moved` (the hook path does not exist), or `disabled` (`disableAllHooks`, or a Codex config that turns hooks off). These lines come after the timeouts and `[story]`, and before the `rule` lines. |

- The list never holds a key, an `env` entry, or an agent's command line. A command line can hold a secret, and the game does not need it.
- The reply is at most 32 KB after the Lua escape (S12). The allow table comes last, because only it can be long. The `rule` lines (6.6.5) come just before it, so a cut removes allow patterns first. A list that does not fit keeps its first lines and ends with a line `+`, as the folder tree does.
- No version change (7.7): at that time the bridge wrote the relay addon again at each start, so the addon was never newer than its bridge.
- **When the addon asks:** when the Settings or Diag tab opens and the list is missing or older than 10 minutes, and at a click on the status line. Each ask costs a strip, so a tab that opens again soon asks nothing.
- The addon parser takes only a line with a known shape: an agent needs a valid name and a known level, and a folder rule needs a folder and a pattern. A seeded test feeds it random bytes (14.3).

### 13.5 Decisions for the desktop notice, the new message, and the Settings tab

The implementer and an advisor agent chose these (2026-09-26).

1. **The desktop state rides in the live file** as one bridge progress line, right after the level line. S9 and S20 do not change, and no slot file is new.
2. **A separate `PollEvery` hook** in the shared `Messages.lua`, with nil as the default, so the Timeways copy keeps its schedule.
3. **At most 24 fast polls per desktop request.** A request that waits the whole `permission_timeout_minutes` costs 24 of the 1000 slots, and the normal schedule still finds the answer.
4. **The whisper line of a desktop request shows once per request**, also across a `/reload`. The saved variables keep the last 16 ids, so the list stays small.
5. **A new message ends only a wait for an answer.** During a normal turn, a follow-up waits in the queue, else each follow-up ends a long run. Only a newly accepted record counts: a duplicate, an outbox copy, or a refused record never ends a wait.
6. **The old message ends as "Stopped."**, the text of Stop, so the player sees one known end.
7. **No Pings tab yet.** An empty tab is a promise that the game does not keep. (Section 10 gives notifications a bell at the minimap, not a tab.)
8. **A settings list, not a new slot file.** It is one more list in its own chat, as `list=folders`. Some values hold tabs, so a line splits at its first tab only.
9. **The addon asks for the list only when a tab opens and the list is old**, and at a click on the status line, because each ask costs a strip.
10. **The Permissions dropdown offers `full-auto`** (changed on 2026-09-30, 9.3, "Full-auto for one chat"). Before, it had no `full-auto`, because the config capped every level. Now full-auto is a mode of one chat. A new chat at `full-auto` gets its own desktop Approve at its first message.
11. **Own dropdowns:** a button and a list of choices, with no client dropdown API, so a client patch cannot break them. An open list closes with its page and at a click outside it (`GLOBAL_MOUSE_DOWN`), as a game menu does.
12. **The reply line setting does not stop the line of a desktop request**, its only notice in the game.

## 14. Verification and tests

The two priorities are readability and test coverage. `CLAUDE.md` has the rules for code and tests.

### 14.1 Aeneas proofs of the protocol core

`crates/protocol` is pure Rust inside the subset that Aeneas supports (see `CLAUDE.md`).
Charon translates it to LLBC, and Aeneas translates LLBC to Lean. The proofs live in `proofs/`.

Untrusted input enters the system in the core: screenshot pixels in one direction, agent replies in the other.
So most theorems are security properties, and each one closes a named attack.

**Security theorems (untrusted input):**

| # | Theorem | Attack that it closes |
|---|---|---|
| S1 | **Decoder totality:** for every image grid, `decode_frame` returns a frame or a defined error. It never panics or reads out of bounds. | A crafted strip crashes the bridge. |
| S2 | **Checksum and tag gate:** `decode_frame` returns a frame only if the checksum matches and `verify_tag(key, bytes, tag)` is true. `verify_tag` is opaque in the proof. | A fake strip from another addon passes as real. |
| S3 | **Record parser totality:** for every byte string, `parse_records` returns records or a defined error. | A crafted payload crashes the bridge. |
| S4 | **Field isolation:** no byte of one field ends up in another field. | Text bleeds into the `cwd` or `flags` field and changes the folder or the permissions. |
| S5 | **Folder policy:** if `resolve_folder(roots, request)` accepts, the result is inside one of the roots, for every request, also with `..`, `.`, repeated `/`, and trailing `/`. | A message escapes `allowed_roots`, for example `../../.ssh`. |
| S6 | **No privilege from the game:** for every flag list, the effective permission level is at most the ceiling. The bridge gives `effective_level` the chat's ceiling, and only the desktop sets it: the agent's config level, at most `auto-edit`, or `full-auto` for a chat and a real folder that the user approved on the desktop (9.3, "Full-auto for one chat"). The proved function does not change. S17 bounds a rule from the game, and a rule never covers a "never always" command. | A message from the game raises its own permissions. |
| S7 | **Replay protection:** a `(token, id)` pair is accepted at most once while it is in the window. | A replayed strip runs a task twice. |
| S8 | **Lua escape:** for every string, the escape gives a Lua string literal that reads back as the same string. The output never ends the literal early. | A malicious agent's reply injects Lua code into the game. |
| S9 | **Slot body shape:** for each app, the slot file writer puts only escaped strings and numbers into a fixed table shape, under that app's global name. | A malicious agent changes `proto`, adds fields, or runs code in the slot file. |
| S18 | **Restore file shape:** for each app, the restore writer puts only escaped strings and numbers into a fixed table shape, under that app's global name. Its prepare step keeps the last 16 chats and the last 10 messages of each, and cuts only the ends of strings. | A chat name or message from a malicious agent runs code in the restore file. |
| S19 | **Restore size bound:** for each app, a restore file that fits is at most 512 KiB. | A long chat history makes a restore file that the game cannot load. |
| S20 | **Live file shape:** for each app, the live file writer puts only escaped strings and numbers into a fixed table shape, under that app's global name. Its prepare steps keep the last 30 progress entries with their last 5 lines, the first 4 permission requests with their first 4 options, and the newest 20 terminal session notices (restated 2026-09-28, 10.3), and cut only the ends of strings. | An agent puts code into a progress line or a popup, or a local process into a notification. |
| S21 | **Live size bound:** for each app, a live file that fits is at most 256 KiB, also with 20 notices. | Progress, popups, or notices make a file that the game cannot load. |
| S10 | **UI escape:** the display sanitizer doubles every `\|` in agent text. | A malicious agent fakes a WoW chat link (`\|H...\|h`), a texture, or a color that imitates a system message. |
| S11 | **Freshness:** the bridge accepts a frame only if its time is at most 5 minutes old and at most 1 minute in the future. | An old screenshot of a strip is replayed. Its MAC is still valid, so S2 does not stop it. |
| S12 | **Size bounds:** for each app and every input, a slot body is at most 1 MB, and each reply record in it at most 32 KB. | A malicious agent writes a huge reply, and the bridge writes 200 huge slot files. |
| S13 | **ID charset:** the id validator accepts only `[a-z0-9_-]`, 1 to 32 characters. | A chat id like `../../x` reaches a file path or a state key. |
| S14 | **Rate limit and queue cap:** the limiter never admits more than N messages in any window. A chat queue never holds more than 20 messages. | Strip spam fills memory or starts many runs. |
| S15 | **Honest popup:** the popup text holds the full raw command, or its start and end with a cut mark. It holds no raw control, bidi, or zero-width characters. | A malicious agent asks for permission with a false label, or hides the dangerous part of a command. |
| S16 | **Classifier paths:** for a file tool call answered `ask` or `allow`, every write path is inside the chat folder, every read path is inside `allowed_roots`, every path is clean (the form of S5), and no path is a `desktop` or `deny` path. Any path inside a `deny` folder (the bridge's config or data folder) gives `deny`. Limits (6.6.3): paths inside command arguments are out of scope, and the proof works on the paths that the bridge resolved. A symbolic link made after the check is a race that the proof does not cover. | An approved tool call in the game reads `~/.ssh`, writes outside the project, or reads the strip key. |
| S17 | **Classifier ceiling:** with the order `deny < desktop < ask < allow`, for every tool call and every rule list from the game, `classify(call, rules) ≤ ceiling(call)`. `ceiling` is the config's answer when a game rule covers every command. An unknown tool is `desktop`, and a command with a "never always" or `desktop` part is at most `ask`, for every rule list. S17 bounds the verdict, not the gate: a chat at `full-auto` runs every verdict but `deny` (9.3), and the walls for its file tools use the same classifier with another policy. | A rule from the game, or a crafted command, gets more than the config allows. |
| S22 | **Renderer totality:** for every Markdown text of at most 1 MiB, `render_markdown` returns a value. It never panics or reads out of bounds. | A crafted reply crashes the bridge. |
| S23 | **Block shape:** the output of `render_markdown` is the marker `1B 4D 31`, zero or more blocks, and `\n`. Each block is `\n`, a kind byte, and fields that each start with US, in the shape of 7.3.1. No field holds `\n`, US, ESC, any other byte below `20`, or `7F`. | Agent text makes a false block, fakes the marker, or moves text into another field or kind. |
| S24 | **Reply escape (extends S10):** every output text, read left to right in tokens, holds each `\|` only as `\|\|`, `\|r`, or one of the five color codes of `inline.rs`. A color code comes only when no color is open, `\|r` only when one is, and no color is open at the end of a text. The texts of `h`, `p`, `l`, and `q` hold no `<` or `>`, and each `&` starts `&lt;`, `&gt;`, or `&amp;`. | A malicious agent fakes a WoW link, texture, or color, or puts SimpleHTML markup into the window. |
| S25 | **Reply size bound:** the output of `render_markdown` is at most 16 bytes per input byte, plus 4. | Rendering makes a reply grow without a bound. With the cut of S12, the body stays within 1 MB. |
| S27 | **Totality:** `classify`, `ceiling`, and the shell splitter `split` return an answer for every input and never panic. | A crafted command or path crashes the bridge. |
| S29 | **Routing by key:** `route(relay_ok, timeways_ok)` gives the one app whose key verifies the strip's tag. No key gives `BadTag`, and both keys give `Ambiguous`. The two tag checks are inputs, so S29 proves the choice, not the cryptography: `verify_tag` stays opaque, as in S2. | A strip of one app reaches the other app, for example a story strip reaches the coding agents. |
| S30 | **Version range:** for every app and every version, `version_fit` never fails. It gives `Supported` exactly when `oldest app ≤ v ≤ newest app`, `TooOld` exactly when `v < oldest app`, and `TooNew` exactly when `newest app < v`. | A message from an addon version that the bridge does not speak reaches an agent or the story program, or a good version gets the update text. |
| S31 | **Sandbox policy:** for every config, each `deny` and `desktop` path is hidden. No writable path is inside a hidden path. Writes go only to the chat folder and a private temp folder. `sandbox_policy` builds the policy from the chat folder, the temp folder, the `deny` folders, and both lists of `desktop` patterns. "Hidden" is the classifier's predicate (6.6.3), and "inside" is the parts prefix of S5. A writable path has the clean form of S5. | A command of a game run reads the strip key or `~/.ssh`, or writes a file that code on the host runs later, such as `.git/hooks/pre-commit`. |
| S32 | **Seatbelt escape:** for every path, the escaped path in the Seatbelt profile reads back as the same path and never ends the string literal early. `sbpl_string` puts a `\` before each `"` and `\`, and refuses a NUL byte, which no path holds. A small model of the SBPL string reader states "reads back", as S8 does for Lua. | A folder name with a `"` ends a literal and adds a rule to the profile, for example `(allow default)`. |
| S33 | **Host check:** for every allow list and every host, `host_allowed(list, host)` is true exactly when the host is a good host name and equals a list name without ASCII case. `good_host_name` is true exactly for a good host name (6.6.4): 1 to 253 bytes, at least two labels of 1 to 63 bytes of `[A-Za-z0-9-]` that do not start or end with `-`, and a last label that starts with a letter and is not `localhost`. | A proxy request for an IP address, `localhost`, a name that differs from the list only in case, or a lookalike such as `github.com.evil.net`. |
| S34 | **Public address:** for every IPv4 address, `is_public_v4` is true exactly when the address is in no range of `v4NotPublic`. For every IPv6 address, `is_public_v6` gives the answer for the IPv4 address that an IPv4-mapped, NAT64, or 6to4 form holds. Otherwise it is true exactly when the address is in no range of `v6NotPublic` (6.6.4). | An allowed name resolves to this computer, its network, or the cloud metadata address, also through an IPv6 form of such an address. |
| S35 | **The target of a request:** for every request head of at most 8 KiB, `check_target` returns a target or a defined refusal, and never panics. A target comes only from a first line `CONNECT <host>:<port> HTTP/1.<d>`. It is a listed local port on `localhost` (never 2375, 2376, or 9222), or a host on port 443 or 80 that the mode allows (S33) (6.6.4). | A crafted request crashes the proxy, or reaches an unlisted port of this computer or a host that the list does not allow. |
| S36 | **Proposal shape:** `propose` never panics. A rule is the first 1 or 2 words of its command, so it covers that command. Each word is plain (printable ASCII with no space and no shell syntax, 1 to 64 bytes, not starting with `-` or `+`), and the first word holds no `/`. | A crafted command makes a rule that covers more than the command, or a word that the rules file reads back as another rule. |
| S37 | **No proposal for the capped:** a `desktop` command, a "never always" command, a tool that runs any program, and a command that publishes get no rule. Full-auto (9.3) adds no rule either: it is a mode of one chat, not a list of commands, and it ends when the player switches the chat down. | One click in the game makes a lasting rule for `sudo`, `curl`, `npx`, or `git push`. |
| S38 | **An offer allows exactly its call:** `offer` never panics. An offer has 1 to 3 rules. With them, the classifier gives `allow` for the call, and each rule covers a simple command of the call. | The popup offers a rule that does not make the call run, or a rule for a command that the call does not hold. |
| S39 | **An offer stays under the ceiling:** a call gets an offer only when the config's ceiling is `allow`. | A rule from the game gets more than the config allows. |
| S40 | **Notice text** (proved 2026-09-28): `notice_text` never panics. It returns at most `max` bytes with no control, bidi, zero-width, or tag character, holds each `\|` only as `\|\|`, and never ends inside a UTF-8 sequence (10.7). | Any local process writes a notification that fakes a chat link or a system line in the game, or hides text. |
| S41 | **Sessions and notices** (proved 2026-09-28): `apply_event` never panics. The table keeps at most 32 sessions and at most one notice per session. A `waiting`, `finished`, or `failed` event leaves exactly its own notice, and a start or an end leaves none on its session. The other sessions keep their notices, except in a full table where every session has one: then only the session with the oldest notice loses it (10.7). | Stale or piled-up notifications teach the user to ignore them, or a flood of files grows the table without a bound. |
| S28 | **Command floor:** a command that does not parse (the grammar of `split`, 6.6.3) is `desktop`. A command with `$(` or a backtick outside single quotes, by the splitter's quote state, is `desktop`. `eval`, `sudo`, `cmd.exe`, PowerShell, or a shell after a `\|` make a command at most `desktop`. Commands that run other commands, and network tools, make it at most `ask`. | A prompt injection runs code through `eval`, a pipe into a shell, or `sudo`, or reaches the network with no question. |

**Correctness theorems:**

| # | Theorem |
|---|---|
| C1 | **Cell round trip:** bytes → 3-bit cells → bytes gives the same bytes, followed by the zero padding of the last group. **Proved** (`Protocol.Cell.cells_round_trip`, 2026-09-23), for inputs up to 65536 bytes. |
| C2 | **Frame round trip:** for every payload of at most 3200 bytes, `decode_frame(encode_frame(m)) = m`. |
| C3 | **Record round trip:** for records with no RS in any field and no US before `text`, `parse(serialize(r)) = r`. |

**Order:** C1 first, because it is the smallest. Then S15 and S11, because they close the most real risks. Then S1, S3, S8, and S5, because those inputs come from outside. Then the rest.

**Proof hygiene:**

- `proofs/Axioms.lean` prints the axioms of every top theorem. `scripts/check-proofs.sh` fails if the list holds anything other than `propext`, `Classical.choice`, and `Quot.sound`.
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
- The addon's token never retires. An old token retires only when the saved variables file shows the new token (7.6).

Write the model before the bridge state machine. The Rust state machine follows the model.

`models/corner.qnt` models the shared strip corner of 7.1.2: two addons, the shared value, the screenshot events that both addons get, the player's screenshots, a lost event, the retry timer and the shows, and a hostile addon that writes the value. The times are small but keep the order of the real times. The model checker checks these properties:

- Two addons never show a strip at the same time.
- A screenshot event ends only the strip of the addon that holds the corner, never a strip of another shot.
- The retry timer does not run while an addon waits, and a wait adds no show. So the outbox comes only after the real shows.
- An addon that cannot get the corner shows the blocked line before its frame is too old.
- With no hostile addon, a waiting addon gets the corner within the bound of 7.1.2, and no blocked line shows.

`scripts/check-model.sh` runs both models. For each model, a set of witnesses must fail, so the simulator surely reaches the hard states: a turn after a wait, the blocked line, the outbox, and a retry.

### 14.3 Tests

- **Property tests** (`proptest`) for the codec, with pixel noise, color shift, and a cell pitch of 2 to 8 pixels.
- **Golden vectors:** screenshots of known strips from the real game, in `tests/vectors/<build>/` (14.3.1). The real bridge reader must decode each one, and its tag must check under the public test key. (`wow-claude` makes its images at test time and tests them only on Windows.)
- **Differential tests:** the Lua encoder and the Rust decoder agree on every vector. The Rust slot writer and a Lua reader agree on every body.
- **Addon harness:** run the addon in a Lua VM against a fake WoW API (`addon/tests/wow.lua`). Where the real game has a choice, the fake takes it from the newest self-test fixture (14.3.1).
- **Fuzzing:** `cargo-fuzz` on the frame decoder and the record parser. No panic and no hang on any input.
- **Fake agent and fake capture** for the bridge loop. No test needs the game or a real LLM, except live tests marked `#[ignore]`.
- **Coverage gates:** `protocol` 95% of lines, `bridge` and `agents` 80%.
- **CI** on Linux, Windows, and macOS. CI runs everything except the live tests. A weekly job runs the live tests that work on a runner (14.8).
- **CI time.**
  - One fuzz job builds the targets once and runs each target for 5 seconds. The nightly run gives each target its own job and 600 seconds.
  - The proofs and the model run only when `crates/protocol`, `proofs/`, or `models/` change (`scripts/ci-changes.sh`). A weekly run and the nightly run check everything.
  - The rust jobs keep a build cache and run the tests with `cargo nextest`, which runs the test binaries side by side. On Linux the coverage gates run the tests in place of nextest, so each test runs once there.
  - A change of only `*.md` files, `images/`, or `LICENSE` skips the rust and fuzz jobs.
  - The lints, the WoW API gate, and the supply chain share one job.
  - A new push to a branch stops the older CI run of that branch.

#### 14.3.1 The self-test of the game

The API gate (7.8) makes the fake game strict about names. But the fake game also guesses behavior: the time of a screenshot event, return values, event order, and how the screen draws the strip. Bugs came from these guesses, for example a strip that the old bridge read with a wrong tag. So the real game measures itself, and the tests use the measurements.

**The addon.** `addon/GnomishRelaySelfTest` is for developers only. `install.rs` never builds it in, and no release has it (a test checks `install.rs`). `scripts/selftest-link.sh` links it into the game, with three small load-on-demand helpers: `_Slot`, `_Off`, and `_Old` (an old `## Interface` number). It also links the shared `Sha256.lua`, `Codec.lua`, `Saved.lua`, `Health.lua`, and `Strip.lua` into it, so its strips come from the real code path.

The run starts 5 seconds after `PLAYER_ENTERING_WORLD`, when the saved results do not name the current build. `/grst` runs it again, and `/grst scale` also draws each strip at two other UI scales. It measures:

| Part | What it measures |
|---|---|
| Client | `GetBuildInfo`, the physical and UI screen size, the UI scale, and the screenshot CVars. |
| Load | The type of the saved variables when the first file runs and at `ADDON_LOADED`, and the order of the login events, for a login and for a `/reload`. |
| Lua | `_VERSION`, `%q` of control bytes, `bit` results for signed input, and `hooksecurefunc` on a missing global. |
| Fonts (7.3.1, 13.1) | `SimpleHTML:SetFont` for `h1` to `h3` and `p`; `GetContentHeight` at once, in the next frame, and later; the height of a text with and without `|c` codes; the width of the bullet and of no-break spaces in the body font; and what `FontString:SetFont` returns for a present and a missing file. |
| Addons (7.3) | What `LoadAddOn` returns for a present, missing, disabled, and out-of-date addon, and for a second load. `IsAddOnLoaded` after a load. A load right after `EnableAddOn`, as `Slots.lua` does. Whether `ADDON_LOADED` fires inside the call. |
| Secrets | `issecretvalue` of `UnitHealth`, `UnitPower`, `UnitGroupRolesAssigned`, and `UnitDetailedThreatSituation`, and `C_CombatLog.IsCombatLogRestricted` (the tank addon tests T1, T3, T6, T7, and T8). A check that needs combat, a group, a target, or a nameplate says so and measures nothing. The first fight of the session runs the combat checks and one strip in combat. A secret value never goes into the saved variables. |
| Timing | `C_Timer.After` for 0, 0.01, 0.1, and 1 second, 10 steps of a ticker, the order of three timers due together, and `GetTime` against `time()`. |
| Screenshots (7.1) | For each shot: the time from `Screenshot()` to each event, and when "Screen captured" shows. |
| Golden strips | Payloads of 0, 1, 62, 137, 500, and 3200 bytes, and two records as the relay sends them. With 62 and 137, the tag sits alone in the last row (7.1). One more strip hides right after its `Screenshot()` call: it tells whether the picture comes from the call or from the end of the frame. |
| Line modes (7.1.3) | One line in each of the 6 modes, with a test payload of gradients and edges. |

**The line modes (7.1.3).** The run draws one line in each of the 6 modes through the real `Strip.lua`. For each shot, it sets the self-test's `stripLine` to the mode and the current physical screen size, and removes it after. Each line carries the same 924-byte test payload:

- the values 0 to 255 in each channel as gradients (each channel counts in its own direction and step),
- flat runs of the bytes `00`, `FF`, `55`, and `AA` (each gives one flat color in every mode, and `55` and `AA` are the mid levels that gamma moves),
- and black and white cells for sharp edges.

The payload fills 7 rows at 6 bits, so the edges also run across rows.

The first golden strips of the run also carry the line test of 7.1.4, as a player's first strips do. The bridge reader ignores the beacon after the frame, so the vectors read as before.

Collect judges each mode by its screenshots. It judges every screenshot of the run for each mode, and keeps the best verdict: clean, then a failure, then not found. A screenshot counts for a mode only when it shows the mode's marker, at the mode's cell size or at any cell width from 0.5 to 4 pixels. Collect compares each cell with the frame that the self-test signed. Each mode gets one verdict:

| Verdict | What collect saw | What it tells the player |
|---|---|---|
| clean | The marker at the right cell size, and every channel of every cell within a quarter of a level step of its value (24 bits: exact; 12 bits: 4; 6 bits: 21). | The mode works. |
| not found | No screenshot of the run shows the mode's marker. A blur of 1-pixel cells also ends here: the marker mixes with its neighbors. | The line did not draw, or blur or scale hides it. |
| scale | The marker shows at another cell size. | The screenshot is scaled: a render scale below 100%, or a physical screen size that is not the screenshot size. |
| blur | Every wrong value sits next to a cell of another value. A blur changes nothing inside a flat run. | Neighbor pixels mix: anti-aliasing, a render scale, or an upscaler. |
| color shift | A wrong value also sits inside a flat run. | The game changes colors: gamma, brightness, or a color filter. |

The report prints one line per mode, with the largest error, then the chosen mode: the first clean mode in the order of 7.1.3. Collect writes it into `strip-line.json` (7.1.3), and the bridge sends it to the addons with the next publish. The line shots that read also become golden vectors.

**The public test key** is the 32 bytes `gnomish-relay public test key 01`. It signs only the golden strips, never a message. Each strip has the frame time 1790211079 and its own frame id, so each vector is reproducible.

**Collect.** WoW writes saved variables only at a `/reload` or a logout. After the `/reload`, `gnomish-relay selftest collect [folder] [--out <repo>]` does this:

1. It reads `GnomishRelaySelfTest.lua`, the newest one of all accounts, with the limits of `saved.rs`. It refuses results that name a key other than the public test key.
2. It scans the `Screenshots` folder for PNGs from the time of the run. It decodes each one with the real bridge reader and the test key, and keeps a file only when its time, frame id, and payload match a shot. It never takes a path from the saved file.
3. It writes `tests/fixtures/<client>-<build>.json`, for example `anniversary-2.5.6.69795.json`: the measurements, and the fake game behavior that follows from them. The interface number of the build names the client (7.9). After a Forever run, it deletes the placeholder fixture.
4. It writes `tests/vectors/<build>/`: each PNG, `manifest.json` with each payload and the key, and the raw saved file, which shows how WoW writes saved variables. Each PNG keeps only the smallest top-left corner that still decodes its strip. The rest of a screenshot shows the player's screen with the character name, and the repo is public.
5. It judges each line mode, prints the report, and writes `strip-line.json` into the bridge data folder (7.1.3). This is the only file that it writes outside the repo.

It reads no key and no config of the relay, and it never deletes a screenshot. A running bridge leaves the test strips alone: it checks each strip that fails its keys against the test key, and keeps and logs a test strip.

**The fake game.** The tests load the newest real Forever fixture, or `tests/fixtures/forever-placeholder.json` while none exists. A fixture of another client never replaces it: each behavior where that client differs gets its own test (7.9).

- CI also runs the addon behavior suites (`addon_flow`, `addon_notices`, `addon_chat_tools`, `addon_git`, and `addon_update`) once more in the fake game of each other client. `scripts/test-addon-clients.sh` sets `GNOMISH_TEST_CLIENT`, and the tests take that client's fixture and API file.
- The waits after a send come from the measured shot delay (`wait_for_shot`), never less than one second.
- The strip and line suites stay on Forever. They check drawing math against the fixture's screen size, and the screen size is no part of the client.
- The placeholder holds the fake game's guesses from before the self-test, and says so.
- The fake game takes these values from the fixture: `GetBuildInfo`, the screen size, the delay of the slowest shot, the event of a good shot, when the picture is taken, when "Screen captured" shows, the returns of `LoadAddOn` and `FontString:SetFont`, whether `GetContentHeight` waits for the next frame, whether the saved variables load before or after the files, the login events, the timer order, the `bit` results, and `hooksecurefunc` on a missing global.
- The addon tests also run the relay in the other behaviors that it depends on: "Screen captured" before and after the event, a picture after the handler, saved variables after the files, a content height in the next frame, a disabled slot with and without a working `EnableAddOn`, an out-of-date slot, and a `hooksecurefunc` that refuses a missing global.
- A timer order that the fake game has no model for stops it at load.
- A test fails when the measured shot delay no longer fits the one-second waits of the addon tests.

**Tests.** `crates/bridge/tests/golden.rs` decodes every committed vector on all three OSes. It skips with a message only while no real fixture exists. When a real fixture exists, a missing vector folder fails, and so does a placeholder that is still there. `crates/bridge/tests/selftest.rs` runs the self-test addon in the fake game, and collect on what it leaves.

**The API gate.** The self-test calls functions that the relay must never call. So it has its own lint list (`selftest.yml`), and `scripts/selftest-api.sh` writes its own API files (`addon/tests/selftest-api.lua` and `selftest-api-signatures.lua`). CI and the nightly job run both gates.

**After a client patch:**

1. Close the game, and run `scripts/selftest-link.sh`.
2. Start the game and log in. When the chat says "done", type `/reload`.
3. Run `gnomish-relay selftest collect` in the repo, run the tests, and commit `tests/fixtures` and `tests/vectors`.
4. After a new resolution, run steps 1 to 3 again for new golden vectors. The relay addon measures the line of a new resolution itself (7.1.4).

The first run ever needs one more `/reload`: its first session has no saved file, so it cannot see the load order. Collect says so.

**Decisions.** An advisor agent and the implementer chose these (2026-09-26):

- The results go into the saved variables as hex of JSON. The bridge reads hex fields as it reads the outbox frames. Plain Lua tables need a Lua parser in Rust, and depend on how WoW escapes a string, which the self-test measures only now.
- Collect finds each screenshot by the frame that it holds, not by a name or a time. WoW names a screenshot by the second, and the saved file is untrusted text.
- The strips at other UI scales run only on `/grst scale`. `UIParent:SetScale` is allowed out of combat, but a fight that starts before the restore blocks it. The addon then restores the scale at the end of the fight.
- The run starts after `PLAYER_ENTERING_WORLD`, not at `PLAYER_LOGIN`: a shot at login can catch the loading screen.
- The placeholder fixture is the only place for the guesses. The fake game has no second copy of them.

### 14.4 Fuzzing

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
| Flags from the game | `perm=`, `level=`, `build=`, and `agent=` take only values of the right shape, and `mkdir=` only `1`. A coding flag never changes the transport flags, which are all that the Timeways lane reads. Any `ver=` gets an update text exactly when it is out of its app's range. |
| Messages from an ACP agent | The agent is untrusted. A progress line stays short, a popup text is printable (S15), and the game never gets "allow always". |
| The Markdown renderer (7.3.1) | Agent text reaches the game window. Each block has its shape, no agent byte starts a WoW code or HTML markup, and the size stays within its bound. |
| Messages of `codex app-server` | The agent is untrusted. A progress line stays short, and a popup text is printable (S15). |
| Lines of `claude -p` and Claude Code session files | The agent and its files are untrusted. A progress line stays short, a popup text is printable (S15), and a copy of a session keeps no old id. |
| The bridge state machine (`relay`) | The promises of the transport model (14.2) on the real code, with a Timeways lane next to the relay lane: no Timeways record becomes a job. A job that makes a new folder (9.9) is a first message, and the last part of its folder passes the name rules. |
| The action classifier and the shell splitter (6.6.3) | Backs up S16, S17, S27, and S28 on the compiled code: no panic, no rule list above the ceiling, a file call that runs stays inside its folders, and the command floor holds. |
| Lines of the story program and batches of the addon (`app_protocol`, 9.8) | Both are untrusted. No line panics a reader. A batch that passes has at most one line with a reply, and each line that goes on is JSON with the bridge's `id`. An answer that passes is at most 24576 bytes, and its reply for the game is one JSON line with every `\|` doubled (S10) that the slot writer keeps whole (S12). A journal that passes holds no `note`. Both answers to a model call are one JSON line with its `call`. |
| The sandbox policy, the Seatbelt escape and profile, and the `bwrap` arguments (`sandbox`, 6.6.4) | Backs up S31 and S32 on the compiled code: each writable path is the chat folder or the temp folder and is not hidden, each `deny` folder is hidden, each path reads back from its literal with the model of S32, the profile holds exactly the expected literals in order, and `bwrap` binds each writable path and ends with the command. |
| "Always allow" (`always`, 6.6.5) | An agent command makes each rule. Backs up S36 to S39 on the compiled code: each proposal is the first words of its command and holds only plain words, and each offer makes the classifier give `allow` under a ceiling of `allow`. |
| `rules.json` (`rules_file`, 6.6.5) | A local program or a damaged disk can change the file. No text panics the reader, and each rule that loads has the shape that `propose` makes. |
| The head of a `CONNECT` request to the proxy of the sandbox (`connect`, 6.6.4) | A command of a game run writes it. No input panics the parser. A target that passes is a lower-case host name, never an IP address in any form, and the allow list matches only its exact names. |
| Answers of a local model (`model_http`, 9.7 decision 10) | The local model is untrusted. No answer panics the reader. The text for the story program is at most 16 KiB, has no control character but a newline and a tab, and its `model_answered` line is one JSON line. |
| The stdin of a hook (`hook_input`, 10.7) | Any agent version writes it. No input panics the hook, and its spool file is one JSON object of at most 4 KiB. |
| A spool file (`notice_file`, 10.7) | Any local process of the user writes it. No input panics the reader, each accepted text passes S40, and a sequence of files keeps the table of S41. |
| The settings of an agent (`hooks_merge`, 10.7) | The user writes them. The merge never panics. It refuses and changes nothing, or its result parses, holds every key and hook of the input, and holds each group of ours once. |

The addon parsers have seeded tests in the addon harness instead of a fuzz target: the folder tree of 9.9 and the settings list of 13.4 each get 600 random inputs, and each result keeps its rules.

### 14.5 Security tests

Each rule in 6.2 has at least one named test. These tests need a real file system or a real process:

- A slot folder replaced by a symbolic link: the bridge refuses to write.
- A symbolic link in the Screenshots folder: the bridge does not follow it or delete its target.
- A normal screenshot with no strip: the bridge leaves it alone.
- A prompt such as `; rm -rf ~`: the agent gets it as one argument.
- A prompt file: it has mode 0600 in a private folder, and it is gone after the run.
- `config.toml` after setup: it has mode 0600.
- The tag check: it uses `subtle::ConstantTimeEq` (a test on the code, not on timing).
- A hello with a new token and a bad MAC: no restore bundle.
- The environment of an agent process: it holds only the allowlist.
- A prompt with newlines: `bridge.log` has one line for it.
- On macOS and Windows: `allowed_roots` works with a root in a different letter case.
- The environment of the story program: it holds only the allowlist, with and without `bwrap`.
- The story sandbox on Linux with a real `bwrap` (`crates/bridge/tests/story_sandbox.rs`):
  - A write outside its folder fails, also to the lane's `state.json` and to the lore pack.
  - A read of the config folder, the keys, the data folder, and `~/.ssh` fails. A read of the lore pack inside the hidden data folder works.
  - A connect to a port that answers outside the sandbox fails inside it.
  - With no working `bwrap`, these tests skip with a message. CI installs `bwrap` on Linux and sets `GNOMISH_REQUIRE_BWRAP`, so there they cannot skip.
- A hang of the story program: its child process stops too (Linux).
- The command sandbox with the real tool: `bwrap` on Linux and `sandbox-exec` on macOS (`crates/bridge/tests/command_sandbox.rs`). Each test runs a command through `gnomish-relay --sandbox-run`, as Claude Code does.
  - A write inside the chat folder and the temp folder works. A write outside fails: to another project, the home folder, `/tmp`, and `/var/tmp`.
  - A read of the strip key, the bridge state, `~/.ssh`, and a `.env` in the chat folder shows no secret.
  - A git hook cannot change. A grandchild process stays inside.
  - A link out of the chat folder writes nothing and reads no key.
  - A connect to a port that answers outside fails inside.
  - Quotes, line breaks, and `$(…)` in a command stay inside.
  - The home folder of the tests has a `"`, a `\`, and a space in its name, so each profile path needs the escape of S32.
  - CI sets `GNOMISH_REQUIRE_BWRAP` on Linux and `GNOMISH_REQUIRE_SANDBOX_EXEC` on macOS, so there they cannot skip.
- The proxy of the command sandbox with the real tool (the same file). The test proxy knows `allowed.test` and resolves it to a public address. Its connect step leads that address to a test web server, and the address check stays real.
  - Through the proxy, `curl` inside the sandbox reaches `allowed.test`.
  - A host that is not on the list, an IP address, and a name that resolves to `127.0.0.1` or `192.168.1.1` get `403`.
  - A connection that skips the proxy, to a port that answers outside, fails and reaches nothing.
  - One small test fetches `https://index.crates.io/config.json` through the real proxy with the default hosts, so TLS inside the sandbox is checked. It skips when the computer is offline.
  - On macOS, a test keychain holds an item that any program reads with no prompt. A command with no proxy reads it, and a command with the proxy does not.
  - A live test marked `#[ignore]` runs `cargo fetch` of a small crate inside the sandbox, with the real home folder, and checks that the host cache did not change.
- The copy-on-write view of `~/.cargo` with the real `bwrap` (the same file): a command reads a cached crate and writes a new one. The new one lands in the run's temp folder, not in the home folder. `credentials.toml` in the view shows no secret. With a `bwrap` that cannot make the view, as in CI, this test skips.
- The proxy in its public mode, and `local_ports` (`proxy.rs`): a public host on no list gets a tunnel, and a name that leads to this computer is still refused. A listed local port reaches the loopback of this computer. An unlisted port, 2375, 2376, and 9222 get `403`.
- The agent wall with the real `bwrap` (`crates/bridge/tests/agent_wall.rs`), for `fake-claude`, `fake-codex`, and `fake-acp-agent`:
  - A direct connection fails. A public host through the proxy works. A host that is not on the list gets `403` in `strict` mode. A name that leads to this computer gets `403`.
  - A listed local port works, and another port of this computer stays closed. An agent server answers on the loopback of its wall.
  - `/proc/<pid of the bridge>` does not exist. A socket in the home folder and one under `/tmp` are out of reach.
  - A startup file cannot change, and a new one gets the notice.
  - A grandchild of the agent ends with the wall. The agent proxy socket lies in the data folder and goes away with the run.
  - A command of `fake-claude` runs in the run's command sandbox and reaches the command proxy. It gets `403` for a public host that is not a package host, and does not see the agent proxy socket.
  - Live tests marked `#[ignore]` run the real `claude` in its wall in both modes, with a Bash call in the command sandbox, and a Timeways model call in the strict wall.
  - On macOS, a test checks that Seatbelt cannot start inside a Seatbelt wall.
- One sandbox per run with the real `bwrap` (`crates/bridge/tests/command_sandbox.rs` and `launch.rs`):
  - A server that one command starts in the background answers a later command and nothing outside.
  - A background process ends with the run.
  - A command has no capabilities, and cannot unmount a hidden path or remount `/`.
  - `cd ..` does not leave the walls.
  - With no holder, a command does not start. When the wrapper goes away, its command's group stops.
- The proxy with no sandbox (`proxy.rs`): an allowed host gets a tunnel to exactly the address that the check passed. A name with one public and one private address is refused, and so is each IPv6 form of a private IPv4 address. A plain HTTP request gets `405`, a port other than 443 and 80 gets `403`, a head that is too long or never ends gets `400`, and a connection over the limit gets `503`.
- Git in a chat (9.11), with the real `git` in temp folders (`chat_branch.rs`, `run_changes.rs`, `git_actions.rs`, and `crates/bridge/tests/git_chat.rs`):
  - An own branch makes one worktree next to the repository and nothing else in `.git`. A second chat gets another branch.
  - A run in the worktree reaches the agent with the worktree as its folder.
  - A folder above the repository outside the roots refuses the run.
  - A repository hook never runs at a commit, a snapshot, or a worktree.
  - A snapshot leaves the index, the branches, and the stash as they were.
  - The summary counts new, changed, and removed files, and leaves out the user's files from before the run.
  - Commit commits only the files of its summary.
  - Revert brings back only the run's files and keeps the user's earlier work. It refuses after a later change or a commit.
  - Merge asks on the desktop, fast-forwards, or makes a merge commit. On a conflict, it changes nothing outside the chat copy and starts the merge in it.
  - Discard removes the worktree and the branch. A deleted chat keeps a worktree with changes.
  - The test lines of each tool have unit tests, and `ci_checks.rs` runs a fake `gh`.
- Claude with the real sandbox (`claude_gate.rs`): the scripted `claude` runs an allowed command through the prefix, and the command writes its chat folder and nothing outside. A command that ran without the wrapper stops the run. With no sandbox, a command of the allow table asks in the game, and the reply carries the notice.

### 14.6 Supply chain

- `cargo deny` (licenses, sources, duplicate versions) and `cargo audit` (known CVEs) run in CI.
- `Cargo.lock` is in the repo.
- The verified core (`crates/protocol`) has no dependencies. Every dependency there is code that we trust but do not prove.
- A new bridge dependency needs a reason in the commit or PR.

### 14.7 The release gate

On 2026-10-01, version 0.4.0 shipped while CI was red. A new clippy lint and the Windows path tests failed, and two fix releases followed. So a release now publishes nothing unless all of CI passes on the tagged commit.

- `release.yml` calls `ci.yml` as a reusable workflow (`workflow_call`) with `all: true`. Every job runs on the tagged commit: the rust job on Linux, macOS, and Windows (fmt, clippy with `-D warnings`, the tests or the coverage gates, and the doc tests), the checks job (cargo deny, cargo audit, stylua, selene, the script tests, and both WoW API gates), the fuzz smoke run, the Quint model, and the proofs. The path filter does not apply, so no job skips.
- The GitHub release draft needs the version job and the CI job. The builds need the draft, and the CurseForge upload needs the published release. So a failed check stops both the binaries and the addon.
- Each release has player notes: the `## <version>` section of `CHANGELOG.md`.
  - The version job runs `scripts/changelog-section.sh`, so a tag whose version has no such section, or an empty one, fails before CI runs.
  - The draft release takes that section as its notes, with an install line at the end.
  - `curseforge.yml` writes the same section to `CHANGELOG.md` in the addon folder, and `.pkgmeta` names it as the packager's manual changelog, in markdown. So the CurseForge file has the same notes. `.pkgmeta` also ignores that file, so it is not in the zip.
- No tag starts `curseforge.yml` by itself. It uploads only when `release.yml` calls it with `upload: true`. A run by hand only builds the zip.
- A release checks and builds with one fixed Rust version: `RUST_TOOLCHAIN` in `release.yml`, passed to `ci.yml` as the `toolchain` input. A new stable Rust can add a clippy lint, as 1.99 did. CI on `main` keeps the newest stable, so such a lint shows up there first, and a release does not break on it. The maintainer raises the fixed version after CI on `main` passed with the newer one. The fuzz job and the proofs use their own nightly or pinned toolchains.
- Clippy in `ci.yml` runs with `--locked`, as the release build does, so a stale `Cargo.lock` fails in CI and not in the middle of a release.
- One gap stays: the workflow files of the tagged commit decide what runs. A commit that removes the gate from `release.yml` also removes it from its own release. A review of every change to `.github/workflows/` closes the gap.

### 14.8 The weekly live tests

The live tests are marked `#[ignore]`, so CI skips them. Claude Code updates often, and an update can break the desktop app with no failed test. So `.github/workflows/live.yml` runs the live tests that work on a runner, each Wednesday on Linux, and also by hand (`workflow_dispatch`).

- `scripts/live-tests.sh` runs each test alone with `--ignored --exact`. A test passes only when cargo reports exactly one passed test, so a renamed test fails and does not pass with 0 tests.
- The job installs `bwrap` and sets `GNOMISH_REQUIRE_BWRAP`, as CI does. It installs the latest Claude Code with its native installer (`curl -fsSL https://claude.ai/install.sh | bash`), as players do. The job looks for an update that breaks the app, so it pins no version. A run by hand can name a version, to find the update that broke a test. The ACP adapter `@agentclientprotocol/claude-agent-acp` exists only on npm, so the job installs Node and the adapter.
- The key is the repository secret `ANTHROPIC_API_KEY`. The desktop app gives an agent only the variables of its allowlist (6.2 rule 12), and a game run loads no user settings (`--setting-sources ""`). Managed settings still apply. So the job writes the key to a file of mode 0600 in `/etc/claude-code/` and names it in `apiKeyHelper` of `/etc/claude-code/managed-settings.json`. The job never prints the key. With no secret, as in a fork, the Claude tests skip with a notice, and the other live tests still run.
- When the job fails, it opens one issue, "The live tests failed", with the names of the failed tests, the Claude Code version, and the run link. While that issue is open, a later failure adds a comment to it.

The weekly job runs these tests:

| Test | Needs |
|---|---|
| `command_sandbox::tests::a_chat_folder_with_more_than_a_million_entries_gets_a_run` | Time and disk only |
| `claude`: `live_claude_answers_lists_forks_and_resumes` | `claude` and the key |
| `claude_gate`: `live_the_hook_of_claude_fires_for_a_read` | `claude`, the key, and `bwrap` |
| `notices_e2e`: `the_real_claude_fires_the_hooks_and_a_finished_notice_comes` | `claude` and the key |
| `model`: `live_claude_with_no_tools_cannot_read_a_file` | `claude` and the key |
| `model`: `live_claude_answers_a_model_call_inside_the_strict_wall` | `claude`, the key, and `bwrap` |
| `agent_wall`: `live_claude_answers_in_its_wall_and_runs_a_command_in_the_command_sandbox` | `claude`, the key, and `bwrap` |
| `acp`: `live_claude_lists_and_replays_sessions` | `claude-agent-acp`, and the sessions that the tests above made |

It does not run these:

| Test | Why not |
|---|---|
| `codex`: `live_codex_answers_lists_forks_and_resumes` | It needs Codex and an OpenAI login. The repository has no OpenAI secret. |
| `harness`: `each_installed_preset_answers_inside_the_sandbox` | It needs the other agents (aider, gemini, and more) and their logins. With none installed, it passes and tests nothing. |
| `command_sandbox`: `cargo_fetch_of_a_small_crate_works_inside_the_sandbox` | It needs the copy-on-write view of `~/.cargo`, which a runner's `bwrap` cannot make (14.5). |
| `desktop::tests::live_a_real_dialog_asks_on_this_desktop` | It needs a desktop and a person who clicks Approve. |
| `timeways_e2e`: both tests | They need a Timeways checkout: `scripts/e2e-timeways.sh`. |
| `model`: `live_ollama_answers_a_prompt` | It needs a local Ollama with a model. |

## 15. Build order

0. **Start WoW once.** This makes `Interface/` and `WTF/Account/`.
1. **Done: `Screenshot()` spike.** A test addon draws a strip and calls `Screenshot()` from an event, with no key press. If a PNG appears, WoW writes the strip image itself, and the bridge needs no screen capture. The addon hides the "Screen captured" text through the `ActionStatus` frame.
2. **Cut: capture spike.** Step 1 passed, so the bridge has no screen capture.
3. **Done: Wine rules spike.** Test the five rules in 7.2 under Wine: the `ctl` self-test, a fresh read of a load-on-demand file, and "a new file is not found". Results in `spikes/README.md`. The HMAC-SHA256 cost in WoW Lua is not measured yet.
4. **Done: `protocol` crate with Aeneas.** Frame, cells, records, slot body, escapes, and every theorem in 14.1. `VERIFICATION.md` has the status.
5. **Done: slot writer.** Publish a fixed reply, and make sure that it shows in the game. Passed in the game on 2026-09-24: `install`, then `say`, then `/relay poll` showed the reply. The steps are in `addon/README.md`.
6. **Done: addon port** with the stub harness (the fake game, `addon/tests/wow.lua`) and the differential tests (`crates/bridge/tests/addon_codec.rs`).
7. **Done: Quint model** of the transport.
   - **Done (7a):** the bridge reads strips from screenshots, checks the tag and the time, queues per chat, runs an echo agent, and publishes. Tests run one message around the whole loop.
   - **Done (7b, part):** the addon signs each message at send, and the bridge reads the signed outbox frames from the saved variables.
   - **Done (7b):** `state.json` and the restore bundle in `Restore.lua`. Passed in the game on 2026-09-24: a message went out as a strip, and the echo came back through the slots.
8. **Done: threat model in code:** `allowed_roots`, the policy, and the MAC check.
   - **Done (8a):** `config.toml`, the `level` flag under the config ceiling (S6), and "Agent not set up."
   - **Done (8b):** the action classifier (6.6.3) in `protocol`, with S16, S17, S27, and S28 proved, and the classifier input in the bridge.
   - **Done (8c):** every backend calls the classifier through one gate (6.6.3, 9.3): the Claude hook for every tool call, the Codex approvals, and the ACP permission requests. The config has its allow table, and `gnomish-relay approve` answers desktop requests.
9. **ACP backend.**
   - **Done (9a):** any ACP agent from one config entry, `check-agent`, the process limits, and permissions under the ceiling.
   - **Done (9b):** session resume and Stop for a run in progress.
   - **Done (9c):** progress and permission requests in `Live.lua`, the popup in the addon, and the checked `perm=` answer.
   - **Done (9d):** Markdown replies show as blocks in the window (7.3.1), with S22 to S25 proved.
   - **Done:** live tests with Claude in the game on 2026-09-26 (9.3).
10. **Done: "Always allow" (6.6.5, 9.3).** One click in the game adds a rule that the sandbox bounds, with S36 to S39 proved. The Settings tab and `gnomish-relay rules` list and remove the rules.
11. **Done: notifications from terminal sessions (section 10).** The pure parts in `protocol`, with S40, S41, and S20 restated. The hook subcommand, the spool folder, one notice per session, the notices in `Live.lua`, the minimap bell with its list, toast, and settings, the faster polls while a terminal session is open, and `hooks install` for Claude Code and Codex. Checked against Claude Code 2.1.285 and codex-cli 0.157.0 (10.1). No `note` signal: signals do not work (7.4). **Next:** a test in the real game, and a live Codex test.
12. **Done: a generic backend for any LLM coding harness (9.2).** `acp` for any harness that speaks ACP, the `claude` and `codex` backends, and `command` for a harness with only a command line, inside the sandbox, with presets for aider, gemini, opencode, goose, and llm. **Next:** a live test of each preset with the real tool.
13. **Voice (13.3). Not planned now.** Voice output first, then push-to-talk with its privacy rules.
14. **Done: a deeper API gate.** `scripts/wow-api.sh` checks that each WoW name exists and is not deprecated, and that each registered event exists. It also writes `addon/tests/api-signatures.lua`: the arguments, returns, payload, and secret and restriction flags of each used function, widget method, and event, from the client's generated API docs. A new secret flag breaks an addon even when the name stays the same, so any change fails CI and the nightly job (7.8). The script takes the addon folders and output paths as arguments, so the Timeways and tank addon repos can run it too.
15. **A second app: Timeways (9.7).** The steps are in 9.7, "Order of the build". **Done:** steps 1 to 8, with 5b.
    - Step 5 is the app protocol (9.8), the story sandbox (6.6.4), and the life cycle, with a loopback in the fake game.
    - Step 6 is the model calls with no tools, through `claude -p` or a local model, and the budget (9.7, decision 10).
    - Step 7 is the shared strip corner (7.1.2) with its Quint model.
    - Step 8 is setup for two apps (9.7, decision 15) and the version range of each app (7.7, S30).
    - **Next:** a loopback in the real game, when Timeways ships an addon build.
16. **Done: the command sandbox (6.6.4).** The policy (S31) and the Seatbelt escape (S32) are proved. Each Claude command from the game runs in `bwrap` on Linux or `sandbox-exec` on macOS, and Codex writes only its chat folder and a private temp folder. Windows and a computer with no working tool get the fallback.
    - **Done:** the proxy for commands (6.6.4): a command reaches only the allowed package hosts, through a Unix socket and a forwarder on Linux and one loopback port on macOS.
    - **Done:** the agent process behind the proxy on Linux (6.6.4, "The agent process behind the proxy"), `local_ports`, and one sandbox per run. S33 to S35 are proved.
    - **Stopped:** the Windows launcher with an AppContainer (`rappct`), because Git Bash cannot start in an AppContainer (6.6.4, "Windows").
17. **Done: limits and accounts.** The limit on parallel runs, with a waiting line in the game (8.2). Two WoW accounts on one computer, told apart from a wipe by the account folder of each token, with a slot window per token (7.3, 7.6). The tokens and cost of each run, the daily total, and the daily cost cap (9.10). **Next:** a test in the real game with two accounts, and a live run of Claude and Codex that checks the usage line.

18. **Done: git in a chat (9.11, 6.6.6).** An own branch in a worktree per chat, with Merge and Discard; a change summary with Commit and Revert at the end of each run; and the test line and CI checks of the branch. **Next:** a test in the real game.

19. **Windows with WSL2 (11.5).** The desktop app runs in WSL2 with the `bwrap` sandbox, and a Windows `Run` entry keeps it alive. **Next:** the manual test of 11.5 on a real Windows 11 computer.

20. **Done: auto-update (11.3).** The desktop app follows a newer relay or Timeways addon on disk, with the pinned release of the addon version. `auto_update = false` turns it off. **Next:** a real CurseForge update of the relay addon, and the Timeways part after Timeways has its CurseForge project.

Steps 1 to 5 prove the channels. After those, the rest is normal Rust work.

## 16. Development environment

- `dev gnomish-relay` opens tmux with nvim, the agent, and a terminal in this folder.
- Link `addon/GnomishRelay` into `_classic_beta_/Interface/AddOns`. Then an edit plus `/reload` loads the new code, with no copy step. `scripts/dev-link.sh` does this, and also links each file of `addon/transport` into `addon/GnomishRelay`. Git ignores these links. The key addon and the slots are real folders in `AddOns`, never in the repo (7.3.2).
- After each client patch, run the game self-test (14.3.1): `scripts/selftest-link.sh`, a login and a `/reload`, then `gnomish-relay selftest collect`. Commit the new fixture and vectors. `scripts/selftest-link.sh --remove` takes the self-test out of the game.
- Run the bridge in the bottom-right pane.
- Aeneas and Charon are built in `~/verif`. `proofs/TOOLS` pins their commits, and CI builds the same commits with Nix.

## 17. Open questions

- **Slow shots and two apps.** TBC Anniversary under Wine took up to 1.1 s for one shot (2026-10-02). The shared-transport tests expect a relay strip within 4 s while Timeways also sends, and that fails at that shot delay. Measure the delay of a normal session, then choose: a longer target, or a turn order that favors the relay.

- Can a program in a `bwrap` wall under WSL2 reach Windows over `AF_VSOCK` (11.5)? If so, a seccomp filter that refuses `socket(AF_VSOCK, ...)` closes the way.
- When can Windows get a sandbox for Claude's commands (6.6.4, "Windows")? Try the AppContainer again when `msys-2.0.dll` starts in an AppContainer (microsoft/mxc issue 1061), or when Claude Code runs its commands through a shell other than MSYS2.
- What does `permissions.<profile>.filesystem.deny_read` of Codex take, so that Codex can hide the `deny` and `desktop` paths (6.6.4)?
- Not planned now (voice, 13.3): does `C_VoiceChat.SpeakText` have any voices under Wine? A spike calls `C_VoiceChat.GetTtsVoices()` in the game.
- Not planned now (voice, 13.3): can the bridge take a global push-to-talk hotkey on Wayland through the GlobalShortcuts portal?
- Not planned now: can font files replace the `.wav` signals? The slot polls of 7.3 work without signals.
- How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
- Does Gemini CLI have hooks for notifications?
- How large is the hitch at a larger window size? (The "Screen captured" hide works.)
