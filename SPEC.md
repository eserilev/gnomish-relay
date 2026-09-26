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

Theorem S15 covers these rules.

### 6.5 Known leaks

- Reply text sits in a global table after a slot loads. Any addon can read it.
- `GnomishRelayDB` is a global table. Any addon can read the chats in it.
- The strip is signed, not encrypted. The prompt is in the pixels of each strip screenshot until the bridge deletes it. If the bridge does not run, these files stay. A cloud sync of the Screenshots folder (for example OneDrive on Windows) copies them.
- An addon that loads before ours, for example one named `!Evil`, can replace global functions such as `string.char`, `tonumber`, or `bit.band` before `Key.lua` and `Sha256.lua` run. It can then read the strip key. Lua in WoW gives an addon no way to stop this. Layers 2 to 4 of 6.6 assume that any game message can come from another addon, so the key is a check against programs outside the game, not against other addons.
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

- The `full-auto` level (6.2 rule 5) skips the questions of the game only. `deny` and `desktop` answers of the classifier still apply, and so does the sandbox.
- The allow table of the config (12) covers commands. A command that it covers runs with no question at `auto-edit` and `full-auto`. It never covers a `deny`, `desktop`, or "never always" command (S17).
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

**Desktop approval.** The bridge runs in the background with no window, so an answer on the desktop is a command:

- The bridge writes each open request to `approvals/<id>.json` in the data folder (12), with mode 0600. The id is 12 random hex digits. The file holds the agent, the folder, the time, and the popup text (S15).
- `gnomish-relay approve` lists the open requests. `gnomish-relay approve <id>` allows one, and `gnomish-relay deny <id>` refuses one. Each writes an answer file next to the request, with `create_new`, so it never follows a link. A request has at most one answer.
- The bridge checks for the answer every 100 ms, up to `permission_timeout_minutes`. No answer refuses the call. The bridge then deletes the files. At start it deletes the files of an old bridge.
- The bridge writes a log line, and shows a notice of the OS with the tools that the user already has: `notify-send` on Linux, `osascript` on macOS, and a PowerShell toast on Windows. The text goes in an argument or an environment variable, never into a script. With no such tool, the log line is the notice.
- The game popup of the call starts with "Approve on your desktop: gnomish-relay approve" and has only Deny. No addon can answer a desktop request, and a Deny from the game refuses the call.

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
- **`deny` paths:** the strip key, `timeways.key`, `config.toml`, and everything else in the config folder of the bridge (12). An approved access would let the agent sign fake strips or raise its own ceiling.
- **`deny` paths in the data folder** (12, and 9.7 decision 12): `state.json`, `approvals/`, `timeways/`, `bridge.lock`, `bridge.pid`, `bridge.log`, and everything else there. An approved access would let the agent clear the replay store, answer its own desktop request, or change the story state. The bridge writes these files itself, never through the classifier.
- **`desktop` patterns** are whole parts that match anywhere in a path, for example `.git/hooks`. A last `*` in a part matches the rest of a part, so `.env.*` matches `.env.local`.
- **`desktop` paths, for reads and writes:** `.ssh`, `.aws`, `.gnupg`, `.env` files, other credential files (`.netrc`, `.git-credentials`, `.config/gh`, `.docker/config.json`, `.kube`), keychains, and browser profiles. `action_input.rs` has the full list.
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
| Codex (`codex app-server` or ACP) | Its own sandbox, `workspace-write` | Its own sandbox, `workspace-write` | Its own Windows sandbox |
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

The flags split in two (9.7, decision 6). Every app sends the **transport flags**: `h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, and `restored`. Only the relay reads the **coding flags**: `perm=`, `level=`, `agent=`, `attach=`, `list`, `d`, `n`, and `stop`. `flags.rs` has one parser for each part, so a coding flag in a record of another app does nothing.

| Flag | Meaning |
|---|---|
| `n` | Start a new agent session for this chat. |
| `h` | Hello only. It announces the token and the addon version. It has no prompt. The addon sends one at login and after it applies a restore bundle (7.6). |
| `d` | The chat is deleted. The bridge stops its runs, and drops its replies, its session link, and its history. A reply of a deleted chat can never be read, so it must leave the body (7.3). The addon keeps the id in `db.forget`, and sends it with each strip until a strip goes out while the bridge is online. The agent session itself stays, so Resume can bring the chat back. |
| `list` | Asks for the saved sessions of the agents (9.6). The record is a message of the chat `relay`, and the reply is the list. |
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

`Live.lua` carries the progress and the permission requests (9.3, S20), and `Restore.lua` the restore bundle (7.6, S18).
Later fields (the session, the denied rules, and `notes` for pings) go into a file of their own, or need an approved change of S9.

- `proto` and the pool sizes let the addon detect a mismatch (7.7).
- `replies` holds every record that the addon has not read, at most 30. Each `text` is at most 32 KB. The bridge cuts longer text and adds a note with the full length.
- A final reply stays in the body until a `read` flag names it. Then the bridge takes it out.
- When the body holds 30 records, the bridge refuses new messages. It does not mark a refused message as seen, so the addon sends it again: the strip shows again, and the outbox (7.5) keeps it. The bridge takes new messages again after the next `read` flag.
- The `transport.qnt` model (14.2) checks these rules. Without them, a reply can drop out of the body before the addon reads it.
- `notes` holds terminal pings (section 10). `permissions` holds open permission requests (9.3).
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
- Each slot body carries `proto` and the pool sizes. Each report carries the protocol version of the addon (`ver=<n>`). The bridge logs a version that it does not speak.
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

- At login, the addon makes sure that each client function it needs exists (`Health.Required`). If one is missing, it shows one line, "Gnomish Relay: this game version has no <name>. The relay is off.", and starts nothing.
- The first hello strip and the first poll test the two channels. `SCREENSHOT_SUCCEEDED` or `SCREENSHOT_FAILED` gives the result of each shot, and `LoadAddOn` gives the result of each slot.
- Each strip carries the client build and the last result of each channel: `build=<number>`, `out=shot|fail`, and `in=slots|missing`.
- When a channel starts to fail, the addon shows one line: "Gnomish Relay: screenshots are blocked." or "Gnomish Relay: slots are missing. Run gnomish-relay install with the game closed." The window shows the same state in the bridge light.
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
  addon/transport/      the shared Lua transport of every app (9.7). Install copies it into each addon.
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

- **Resume.** The bridge keeps the agent session of each chat in `state.json`, with its agent and its folder. The next message of the chat resumes it, unless the message has the `n` flag, or the agent or the folder changed. The client uses `session/resume` if the agent offers it, else `session/load`. The history that `session/load` replays stays out of the reply. If neither works, the run opens a new session, and the reply starts with "(New session: the agent could not resume the old one.)".
- **Later: continue a terminal session.** A new chat can take the session of a Claude or other agent session that runs in a terminal. The bridge lists the recent sessions of each agent (`session/list`, where the agent offers it), and the chat resumes the one you pick. The terminal window does not show the game messages live: no agent lets another program type into its open window. `claude --resume` shows them later.
- **Stop.** Stop in the game ends the waiting messages of the chat, and signals the run in progress. The client sends `session/cancel` (`claude`: an `interrupt` control request, `codex`: `turn/interrupt`), answers every open permission request with "cancelled" (`claude`: a deny, `codex`: `cancel`), and waits 10 seconds for the agent to end the turn. Then it kills the process. The reply is "Stopped.", and the session stays for the next message. A Stop before the prompt ends the run at once.

Next, the trait grows events for progress and for permission requests from the game (9.3). Those need new fields in the slot body, so they wait for an approved S9 statement.

### 9.2 Backends

| Backend | How it works | Progress | Live permissions | Allow & retry |
|---|---|---|---|---|
| `acp` (main) | Agent Client Protocol: JSON-RPC over stdin and stdout. The bridge is the client. | Yes | Yes | Not necessary |
| `claude` | `claude -p` with stream-json on stdin and stdout, and `--permission-prompt-tool stdio`. Needs no Node. | Yes | Yes | Not necessary |
| `codex` | `codex app-server`: JSON-RPC over stdin and stdout, with approval requests. Needs no Node. | Yes | Yes | Not necessary |
| `command` | A command template. The prompt goes in, plain text comes out. | No | No | No. Fixed level from config. |

**Support levels.** Any agent with a command line runs. How well the relay protects it depends on what the bridge can see:

| Level | Connection | What the classifier sees | Examples |
|---|---|---|---|
| Full | ACP, `claude`, `codex`, or a tool-call hook | Every tool call that needs an answer, before it runs | Gemini CLI, Claude, Codex, any ACP agent |
| Sandbox only | `command` | Nothing | Aider, `llm`, a script |
| Trusted | `command` with no sandbox | Nothing | The same agents on Windows |

- A Full agent runs in its "ask for everything" mode. The classifier then answers most questions itself. In a looser mode the agent acts without asking, and the classifier never sees the action.
- `command` needs the sandbox. With no sandbox, the level is Trusted: it is off by default, and the config turns it on after a warning.
- The chat header shows the level next to the agent name, for example "Aider · trusted".
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

- The command is `claude -p --input-format stream-json --output-format stream-json --verbose --permission-prompt-tool stdio --permission-mode <mode>` in the chat folder, plus `--resume <id>` for the session of the chat. The `command` of the entry comes first, so it can add flags.
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
- A refusal names its reason to the agent: "It touches the config folder of Gnomish Relay, which the agent never reaches.", "Denied on the desktop.", "No answer on the desktop.", "Denied in the game.", "No answer from the game.", or "Not allowed from the game." when nobody in the game listens.
- The game gets Allow and Deny for a game question, and only Deny for a desktop question (6.6.3).

Each backend maps the level differently:

- `acp`: the bridge sets the session mode. Mode IDs differ per agent, so the config has a `modes` table per agent.
- `claude`: `--permission-mode`, and the hook of 6.6.3 for every call. `ask` is `plan`, and `auto-edit` and `full-auto` are `acceptEdits`. The hook decides, so the mode matters only for the plan of Claude. The `modes` table of the entry can name another mode: `acceptEdits`, `auto`, `dontAsk`, `manual`, or `plan`. Config load refuses any other name. It also refuses `bypassPermissions`: in that mode Claude Code asks nothing, so no tool call reaches the bridge, and the ceiling of the game has no effect.
- `codex`: the sandbox of the thread, and `approvalPolicy: "untrusted"` at every level, which sends the most calls to the bridge (6.6.3). `ask` is `read-only`, and `auto-edit` and `full-auto` are `workspace-write`. The sandbox applies after the answer of the gate. The bridge never uses `danger-full-access`, `never`, `on-request`, or `granular`: none of them asks more than `untrusted`.
- `command`: the level is fixed by the command in the config. The addon shows the level in the chat header. If the level is `full-auto`, the addon shows a warning.
- For game messages, Codex runs through `codex app-server` or ACP only, so the bridge sees each question of its tool calls (6.6.3).

**Live permission flow (ACP):**

1. The agent sends `session/request_permission`. The gate answers it, and a game question waits for the game.
2. The bridge writes the popup text with `popup_text` (S15): the command line of the tool call, else its path or address, else its title, and then its title as "the agent says". It adds the request to `permissions` in `Live.lua` (S20), with the options numbered `o1` to `o4`.
3. The addon shows a popup with the text and the options.
4. The user picks an option. The addon sends a control record with `perm=<request>:<option>:<hash>`. The hash is the first 8 bytes of SHA-256 of the text that the popup showed, in hex.
5. The bridge takes the answer only for an open request of the same chat, a real option, and a matching hash. Then it answers the agent. A second answer does nothing.

**Live permission flow (`claude`):** the hook and a `can_use_tool` control request go through the same steps. The popup text is the `command` of the tool input, else its `file_path`, `notebook_path`, `path`, `url`, or `pattern`, else the tool name. "The agent says" is the tool name and the `description` of the request. The game gets two options: Allow (`allow_once`) and Deny (`reject_once`). The hook answers with `permissionDecision` and a `permissionDecisionReason` that Claude sees. For `can_use_tool`, an allow sends the tool input back unchanged as `updatedInput`. A deny sends a `message` that Claude sees, for example "Denied in the game.". The answer never holds the `permission_suggestions` of the request: they add permanent allow rules, and the game adds no rule (6.6.5).

**Live permission flow (`codex`):** an approval request of the server goes through the same steps. For a command, the popup text is its `command`. For a file change, it is the paths of the change, from the `fileChange` item of `item/started`. If the request has a `grantRoot`, the popup text is "write anything in <root>". "The agent says" is the `reason`, else "run a command" or "change files". The game gets Allow and Deny. Allow sends `accept`, and Deny or no answer sends `decline`. The bridge never sends `acceptForSession`, `acceptWithExecpolicyAmendment`, or `applyNetworkPolicyAmendment`: each adds a rule for later calls (6.6.5).

Rules:

- The request id holds the time of the question, so an old strip cannot answer a new request after a restart of the bridge.
- `allow_always` waits for the rules of 6.6.5. Until then, the bridge does not offer it in the game.
- Each tool call of the agent also becomes a progress line in `Live.lua`: the last 5 lines of each run, for the activity panel.
- The run timeout stops while the run waits for a permission answer. A separate `permission_timeout_minutes` applies (default 10). After it, the bridge answers "cancelled".
- If the game closes or reloads, open requests stay in the next publish until they time out.
- Stop ends an open request as "cancelled", and the run as "Stopped.".

### 9.4 Agent processes

- ACP: one agent process per agent kind. It serves many sessions. For game messages: one process per chat folder, inside the sandbox (6.6.4).
- `claude`, `codex`, and `command`: one process per run.
- `max_parallel_runs` counts active runs, not processes.
- If an ACP process stops, the bridge starts it again and resumes the open sessions. If a session cannot resume, the bridge reports an error for that chat.
- `cancel` for `native-*` and `command` stops the whole process tree.
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

**Decisions:**

1. **Keys.** The relay key stays `strip.key`. The Timeways key is `timeways.key`, in the same config folder. The bridge refuses to start if the two keys are the same.
2. **Routing (S29).** The bridge checks the tag of each strip under both keys. One key verifies: the strip goes to that app. No key verifies: `BadTag`. Both verify: `Ambiguous`, and the bridge drops the strip and logs it. S29 proves this choice, not the cryptography. In the bridge, the keys are a `KeySet { relay, timeways }` struct, not a list, so an index cannot swap the apps.
3. **Outbox frames.** A frame in the saved variables of one app counts only if it verifies under that app's key. Any other frame is refused.
4. **One lane for each app.** Each lane has its own replay store, state file, rate limit, slot window, saved-variables watch, reload inbox, tokens, and restore. The Timeways lane holds no agents in its type, so a Timeways strip can never start a coding agent. Its state lives in `<data>/timeways/`. The relay state stays where it is.
5. **Names for each app.** The slot, restore, and live files set a Lua global whose name depends on the app, for example `GnomishRelay_SlotData` and `Timeways_SlotData`. The strip frame, the slot addon names, and the saved-variables name also differ for each app. S9, S18, and S20 are restated over an `App` enum in `protocol` (approved). One app can then never overwrite a value that the other app is about to read.
6. **Flags.** The flags split into transport flags (`h`, `next=`, `read=`, `ver=`, `build=`, `out=`, `in=`, `restored`) and coding flags (`perm=`, `level=`, `agent=`, `attach=`, `list`, `d`, `n`, `stop`). The Timeways lane parses the transport flags only. A Timeways record with a non-empty `cwd` is refused.
7. **Restore.** Timeways has no restore bundle. The story state lives on the desktop, so the addon rebuilds from there. A Timeways hello never starts a relay restore and never retires a relay token.
8. **The story program.** The bridge starts `timeways-story` when the Timeways key exists, from a path in the config (never a `PATH` lookup), with no shell and the environment allowlist of 6.2. It talks JSON lines over stdin and stdout, with a size limit on each line, a version handshake, and a timeout for each request. The bridge checks each message against a fixed shape. The bridge writes all files that the game reads.
9. **The story sandbox.** The story program reads hostile text: records from any addon, other players' names and messages, and model answers. So it runs in the sandbox of 6.6.4. It writes only `<data>/timeways/`, has no network, and cannot read the `deny` and `desktop` paths. On Windows there is no sandbox yet: Timeways runs, and the bridge shows a one-time warning.
10. **Model calls.** The story program asks the bridge for a model call over the app protocol. The bridge runs the model with no tools and returns only text.
    - **Claude:** `claude -p --tools "" --strict-mcp-config`, with the flags that load no user or project settings (checked live), in an empty private temp folder for each call. The `PreToolUse` gate denies every tool on this route, and the check on tool results stays on.
    - **A local model** (Ollama, LM Studio): through `curl` with `-q` first, `--proto =http`, `--max-redirs 0` and no `-L`, `--noproxy '*'`, `--max-time`, and the prompt through stdin (`--data-binary @-`), never in the arguments. The bridge limits the size of the answer while it reads it. The config accepts only `127.0.0.1` and `[::1]`, not `localhost`. The answer is hostile text, like an agent reply.
    - **Budget.** The bridge enforces a budget of calls for each app with the proved limiter of S14. A hostile addon cannot spend the model subscription faster than that.
11. **Prompt injection.** Other players' text reaches the prompt. With no tools, it can reach only three things: the text that the user sees (bounded by S10 and S24), the story world (bounded by the rules of the world), and the budget. This is the accepted boundary. Each part has a named test.
12. **Protected files.** The data folder joins the config folder in the `deny_folders` of the classifier (6.6.3), with a named test for each file in it. The sandbox of 6.6.4 hides it too.
13. **The shared strip corner.** Both addons draw the strip in the same corner, so they take turns through a shared "busy until" value. While an addon waits for the corner, its 40 s retry timer stops. Each addon counts only the screenshot events of its own strip. An addon that cannot get the corner shows "Screenshots blocked by another addon" before its frame reaches the 270 s limit. A Quint model (`models/corner.qnt`) checks this with the timers.
14. **Shared Lua transport.** `Codec.lua`, `Sha256.lua`, `Strip.lua`, and the slot poll move into one source folder with parameters: the app name, the slot prefix, the global names, and the saved variables. The relay repo copies the folder at package time and never commits a copy. The Timeways repo checks its copy with a plain diff against the pinned relay tag.
15. **Setup.** A player with only Timeways gets no folder question and no coding agents, only a `[story]` section in the config for the model. The bridge makes the Timeways slots only when the Timeways addon folder exists. It writes only `Key.lua` into the Timeways folder, and writes it again at start if it is missing. It never writes other Timeways files.
16. **Life cycle.** `restart` and `update` also stop and start the story program. The bridge kills its process group when it exits. A story program that crashes starts again after a backoff.
17. **Versions.** The hello carries the version of each app. A version out of range gets the reply "update the addon".
18. **What the key split protects.** It stops a bug or a hacked story program from reaching the agents through the bridge. It does not stop a hostile addon that loads first from reading either key (6.5).
19. **Paths from game input.** Realm and character names map to safe ids, as S13 does for chat ids. They never become file names directly.

**Checks for each part:**

| Part | Lean | Quint | Fuzz | Tests |
|---|---|---|---|---|
| Routing by key | S29 | | the `frame` and `relay` targets with two keys | all 4 key results, the `KeySet` swap |
| Names for each app | S9, S18, S20 restated | | the `lua`, `restore`, and `live` targets for each app | both apps in one fake game |
| Version range | a small pure function in `protocol` | | the `flags` target | the "update the addon" reply |
| Budget | S14 | | | a hostile addon at full rate |
| Lanes | | | the `relay` target with two lanes | no job from a Timeways strip; no shared seen store, body, or restore |
| App protocol | | | new target `app_protocol` | a fake `timeways-story`: crash, garbage, huge line, hang |
| Model calls | | | new target `model_http` | a fake model server; the gate denies every tool; live tests |
| Story sandbox | | | | its environment; a write outside its folder fails; a network connect fails |
| Corner | | `corner.qnt` | | two addons in one fake game with a fake clock |
| Shared transport | | | | all addon tests, the golden and differential vectors of 14.3 for both sets of parameters |

The relay tests use a small second test addon built from the shared transport, not the real Timeways addon.

**Order of the build:**

1. The one-lane refactor: the bridge uses one `Lane` type for the relay, with all tests green and no change of behavior.
2. The shared Lua transport with parameters, and the names for each app (S9, S18, and S20 restated).
3. The second key, routing (S29), and the Timeways lane, in one step. No commit has a Timeways key without a Timeways lane.
4. The data folder in `deny_folders`, the flags split, and the outbox rule.
5. The app protocol, the story program with its sandbox, and its life cycle, with a fake echo story program and a test addon: a loopback proved in the game before the real story work.
6. Model calls with no tools, and the budget.
7. The shared corner and its Quint model.
8. Setup for two apps, and versions.

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

### 11.3 Install

The goal: one download, one command, and no step inside the game.
The install scripts put the program on `PATH`, also in the open terminal on Windows, and print the `PATH` line on Linux and macOS when it is missing.

**`gnomish-relay setup`** does every step, and a second run changes nothing that works:

1. **Find the game.** It looks for a `_classic_beta_` folder in the default places and in the install paths of Battle.net's `product.db`:
   - Windows: `Program Files (x86)\World of Warcraft`, and `%ProgramData%\Battle.net\Agent\product.db`.
   - macOS: `/Applications/World of Warcraft`, and `/Users/Shared/Battle.net/Agent/product.db`.
   - Linux: each Wine prefix (`~/.wine`, `~/Games/*`, Bottles also as a Flatpak, and Steam Proton), with the `product.db` of the prefix. `C:` maps to `drive_c`, and other drives to `dosdevices`.
   With more than one, or none, it asks in a terminal. `setup <folder>` skips the search, and takes the `World of Warcraft` folder or `_classic_beta_`. It makes `Interface/AddOns` if WoW has not made it yet, and it finds that folder in any case.
2. **Make the strip key**, 32 random bytes from the OS, into `strip.key` with mode 0600, once. `--new-key` makes a new one, and then the addon needs a `/reload`.
3. **Install the addon.** The addon files are built into the program. Setup writes them into `Interface/AddOns/GnomishRelay`, and writes `Key.lua` from the strip key. A folder that is a link (a developer checkout, 16) stays as it is, and only `Key.lua` changes.
4. **Write the config**, once, with an `[agents.<name>]` entry for each known agent on `PATH`: `claude` (as `kind = "claude"`), `codex` (as `kind = "codex"`), and the ACP agents of 9.2. The default agent is the first one it finds, in the order of `KNOWN_AGENTS` in `install.rs`. With none, it is `echo`. The config also gets a commented example of the allow table (12): setup allows no command.
5. **Make the slot addons.** WoW finds a new addon only at launch, so after a first install the game needs a restart. Setup says so.
6. **Start the bridge at login**, with `--autostart`: a systemd user service on Linux, a launchd agent on macOS (log in `~/Library/Logs/gnomish-relay.log`), and a `Run` entry of the user on Windows, which needs no admin rights. On Windows, `run --background` starts the bridge with no console window, with its log in the data folder.

The order is key, addon, slots, config, then autostart: the addon and the slots need nothing else. A failed autostart prints one line, and setup goes on.
With no code folder found, the folder question has no default: the home folder holds `~/.ssh` and the browser profiles.
The last lines say what setup found and the next action, for example "Agent: claude" and "Restart WoW, then type /relay".

**Keeping it working.**

- At each start, the bridge writes `Key.lua` again if it is missing, and the addon files again if their version differs. An addon app such as CurseForge can replace the folder, and a `/reload` then loads the files.
- With no key, the addon shows one line: "Gnomish Relay: run gnomish-relay setup. Get it at github.com/eserilev/gnomish-relay".
- With no fresh body one minute after login, the addon shows one line: "Gnomish Relay: bridge not running."
- Setup starts the default agent once, with no prompt. A missing login then shows in setup ("Agent: claude needs a login. Run: claude"), not as the first reply in the game.

**Updates and restarts.**

- `gnomish-relay restart` stops the bridge and starts it again, for example after a config edit. With the service of setup, it uses the service: `systemctl --user restart` on Linux and `launchctl kickstart -k` on macOS. With no service (Windows, or no `--autostart`), it stops the process in `bridge.pid`, waits up to 10 s for the lock (8.4), and starts `run --background`.
- `gnomish-relay update` downloads the archive of the latest release for this OS with `curl`, checks its SHA-256 sum, and unpacks it with `tar`. Every supported OS has both tools. `GNOMISH_URL` changes the download folder, as in `install.sh`.
- If the new program is the same as the installed one, update changes nothing. Otherwise, it puts the new program in place of the old one and restarts the bridge.
- Windows refuses to replace a running program, but it lets update rename it. So update renames the old program to `gnomish-relay.exe.old` first, and the next update deletes that file.
- The sum comes from the same release as the archive. It finds a broken download, not a changed release.
- The new bridge writes the new addon files at its start. Then the game needs a `/reload`, and update says so.

**Distribution.**

- A version tag (`v*`) starts `.github/workflows/release.yml`. It builds the program for Linux (x86-64), macOS (Arm and x86-64), and Windows (x86-64), and attaches each archive with its SHA-256 sum to a GitHub Release. The release stays a draft until every build is attached.
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

`gnomish-relay setup <wow folder>` writes the first config. It never replaces a config.

The bridge accepts only the keys that it implements. Any other key is an error, so a typo never leaves a wider default in place.
Today these keys work: `allowed_roots`, `default_cwd`, `default_agent`, `timeout_minutes`, `permission_timeout_minutes`, `[wow] path`, `[agents.<name>]` with `kind`, `command`, `permission`, `env`, and `modes`, and `[allow]` with `commands` and `[allow.folders]`.

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
- `gnomish-relay approve` and `gnomish-relay deny` answer the desktop requests of 6.6.3. They live in `approvals` in the data folder.
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
command = ["aider", "--yes", "--message-file", "{prompt_file}"]
permission = "full-auto"    # --yes approves everything
```

An older config with `kind = "acp"` and `command = ["claude-agent-acp"]` still works. Its mode IDs are not checked yet.

## 13. The addon

### 13.1 Look

The window follows the classic Guild & Communities frame, and uses the built-in game textures and fonts.
The mockup is the reference for the layout.

- **Frame:** the dark metal frame, a black title bar with the gold title "Gnomish Relay", and gold-framed red minimize and close buttons.
- **Portrait:** a round emblem at the top-left corner: a red pipe wrench on a brass cog. It is our own drawing, shipped as a texture.
- **Left column:** one tile per chat, with the agent as the shield icon. The selected tile glows green. A gold "!" marks a new reply. The last tiles are "Start a New Chat" and "Resume". Resume shows the picker of 9.6 in the center: a gold heading for each folder, then one row per session with its title, its agent, and its age, or a green "open" for an active session. A right-click on a chat tile asks `Delete "<name>"?`, or `Stop and delete "<name>"?` while the agent works, with **Delete** and **Cancel**.
- **Center:** a dropdown for the agent and the permission mode, the folder, and the bridge light. Below them, the transcript on a black background: `[You]: text` and `[Claude]: text`. The text is white. Only the name has a color: the user in blue, each agent in its own color. The mouse wheel scrolls it, and a new entry scrolls it to the bottom.
- **Replies:** a rendered reply (7.3.1) shows its blocks below the name.
  - Headings, paragraphs, list items, and quotes go into one SimpleHTML frame, with real sizes for `h1` to `h3`, and a bullet or the number before each item.
  - Code shows in a black box in the shipped mono font (13.2).
  - A table is a grid of font strings with a gold header row. A table with more than 8 columns, or too wide for the transcript, shows each row as a card: the first cell in gold, and each other cell below it with the name of its column.
  - If anything fails while a reply draws, it shows as plain text.
  - User messages, errors, and replies from before 7.3.1 stay plain text.
- **Input:** one empty line, with no label and no hint text. Enter sends. The limit is 3200 characters.
- **Right column, Activity:** a cast bar while the agent works, and one row per step. A tooltip on each row shows the details.
- **Side tabs:** Chats, Terminal pings, Settings, and Diagnostics.
- **Bottom bar:** a red **Stop** button, only while an agent works. It stops the run.
- **Game chat:** a finished reply or a ping shows one line, `[Claude] whispers: [chat] …`, in its own color (copper by default, a setting). For a rendered reply, the line shows the plain words of its first block. A click on it opens the chat. It plays the whisper sound.
- **Permission requests** use the separate popup of 6.4, never the window.

### 13.2 Code

The addon is our own code. It uses the design of `wow-claude`, not its files.
All state is local to the addon files, which share one table. The files load in this order.
The files marked "shared" are in `addon/transport` (9.7, decision 14). They read the names of the app from `App.lua`.

| File | Job |
|---|---|
| `Key.lua` | The strip key. `scripts/dev-link.sh` writes it, and git ignores it. |
| `App.lua` | The names of the app: the slot prefix, the three slot globals, the strip frame, and the saved variables. |
| `Sha256.lua` (shared) | SHA-256 and HMAC-SHA256 for the strip tag. |
| `Codec.lua` (shared) | Records, frames, and cells: the Lua side of `crates/protocol`. |
| `Saved.lua` (shared) | The saved variables table of the app. |
| `Store.lua` | The saved data: token, chats, and the outbox. |
| `Health.lua` | The login self-test and the health of each channel (7.8). |
| `Strip.lua` (shared) | Draws a frame and takes one screenshot of it. |
| `Slots.lua` (shared) | Loads one slot, and takes the three globals of the app. |
| `Transport.lua` | The strip retries, the poll schedule, and the flags. It follows `models/transport.qnt`. |
| `Blocks.lua` | Splits a rendered reply (7.3.1) into blocks and fields, and gives its plain words. |
| `Transcript.lua` | The transcript of the window: a scroll frame that stacks entries and draws blocks. |
| `Window.lua` | The window of 13.1. |
| `Popup.lua` | The permission popup (6.4). Each button names the kind of its option, never the label of the agent. |
| `Core.lua` | Startup, slash commands, and the whisper line. |

The folder also holds `JetBrainsMono-Regular.ttf`, the mono font of code boxes, with its license in `JetBrainsMono-OFL.txt` (SIL Open Font License 1.1). Setup installs both.
The game finds a new file only at launch. Until then, code boxes use `Fonts\ARIALN.TTF` of the game.

Message ids start from the clock, so the ids after a saved-data wipe never repeat the ids in an older body.

The tests run the addon in a real Lua 5.1 with a fake WoW API (`addon/tests/wow.lua`), from `crates/bridge/tests`.
They decode each strip with the proved Rust decoder and check its tag against the Rust HMAC.
They also check the SHA code against both kinds of `bit` results: unsigned as in WoW, and signed as in LuaJIT.

Still to come: pings (section 10), the side tabs, the agent dropdown, and the emblem texture.

Slash commands:

| Command | Action |
|---|---|
| `/relay` | Open or close the window. |
| `/ai <text>` | Send a message to the current chat. |
| `/relay diag` | Show transport diagnostics. |
| `/relay poll` | Load the next slot now. |

### 13.3 Voice (later)

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
| S20 | **Live file shape:** for each app, the live file writer only puts escaped strings and numbers into a fixed table shape, under the global name of that app. Its prepare steps keep the last 30 progress entries with their last 5 lines, and the first 4 permission requests with their first 4 options, and cut only the ends of strings. | An agent puts code into a progress line or a popup. |
| S21 | **Live size bound:** for each app, a live file that fits is at most 256 KiB. | Progress or popups make a file that the game cannot load. |
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
- **CI time.** Each fuzz target runs in its own job for 15 seconds. The proofs and the model run only when `crates/protocol`, `proofs/`, or `models/` change (`scripts/ci-changes.sh`). A weekly run and the nightly run check everything.

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
| Restore and live files, loaded in a real Lua 5.1 VM | Back up S18 to S21: every field loads back as the prepared bytes, and each file stays under its bound. |
| Flags from the game | `perm=`, `level=`, `build=`, and `agent=` take only values of the right shape. A coding flag never changes the transport flags, which are all that the Timeways lane reads. |
| Messages from an ACP agent | The agent is untrusted. A progress line stays short, a popup text is printable (S15), and the game never gets "allow always". |
| The Markdown renderer (7.3.1) | Agent text reaches the game window. Each block has its shape, no agent byte starts a WoW code or HTML markup, and the size stays within its bound. |
| Messages of `codex app-server` | The agent is untrusted. A progress line stays short, and a popup text is printable (S15). |
| Lines of `claude -p` and Claude Code session files | The agent and its files are untrusted. A progress line stays short, a popup text is printable (S15), and a copy of a session keeps no old id. |
| The action classifier and the shell splitter (6.6.3) | Backs up S16, S17, S27, and S28 on the compiled code: no panic, no rule list above the ceiling, a file call that runs stays inside its folders, and the command floor holds. |

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
7. **Done: Quint model** of the transport. **Done (7a):** the bridge reads strips from screenshots, checks the tag and the time, queues per chat, runs an echo agent, and publishes. Tests run one message around the whole loop. **Done (7b, part):** the addon signs each message at send, and the bridge reads the signed outbox frames from the saved variables. **Done (7b):** `state.json` and the restore bundle in `Restore.lua`. Passed in the game on 2026-09-24: a message went out as a strip, and the echo came back through the slots.
8. **Threat model in code:** `allowed_roots`, the policy, and the MAC check. **Done (8a):** `config.toml`, the `level` flag under the ceiling of the config (S6), and "Agent not set up." **Done (8b):** the action classifier (6.6.3) in `protocol`, with S16, S17, S27, and S28 proved, and the input of the classifier in the bridge. **Done (8c):** every backend calls the classifier through one gate (6.6.3, 9.3): the hook of Claude for every tool call, the approvals of Codex, and the permission requests of ACP agents. The config has its allow table, and `gnomish-relay approve` answers desktop requests.
9. **ACP backend.** **Done (9a):** any ACP agent from one config entry, `check-agent`, the process limits, and permissions under the ceiling. **Done (9b):** session resume and Stop for a run in progress. **Done (9c):** progress and permission requests in `Live.lua`, the popup in the addon, and the checked `perm=` answer. **Done (9d):** Markdown replies show as blocks in the window (7.3.1), with S22 to S25 proved. **Next:** a live test with a real agent in the game.
10. **`note` signal and pings:** the hook CLI and the socket.
11. **`native-*` and `command` backends.**
12. **Windows and macOS capture backends.** Mark them experimental until a tester on each OS makes sure that they work.
13. **Voice (13.3).** Voice output first, then push-to-talk with its privacy rules.
14. **Done: a deeper API gate.** `scripts/wow-api.sh` checks that each WoW name exists and is not deprecated, and that each registered event exists. It also writes `addon/tests/api-signatures.lua`: the arguments, the returns, the payload, and the secret and restriction flags of each used function, widget method, and event, from the generated API docs of the client. A new secret flag breaks an addon, even when the name stays the same, so any change fails CI and the nightly job (7.8). The script takes the addon folders and the output paths as arguments, so the Timeways repo and the tank addon repo can run it too.
15. **A second app: Timeways (9.7).** The steps are in 9.7, "Order of the build".

Steps 1 to 5 prove the channels. After those, the rest is normal Rust work.

**Taint spike (before the taint warning ships):** a second test addon calls the send handler of our addon, calls a closure that reads our tables before the probe, fills our input box, and clicks our buttons. The spike records what the probe names in each case, on the Forever client under Wine, Windows, and macOS. Some cases will likely name "GnomishRelay". Nothing in 6.6.5 depends on this spike.

## 16. Development environment

- `dev gnomish-relay` opens tmux with nvim, the agent, and a terminal in this folder.
- Link `addon/GnomishRelay` into `_classic_beta_/Interface/AddOns`. Then an edit plus `/reload` loads the new code, with no copy step. `scripts/dev-link.sh` does this, and also links each file of `addon/transport` into `addon/GnomishRelay`. Git ignores these links.
- Run the bridge in the bottom-right pane.
- Aeneas and Charon are built in `~/verif`. `proofs/TOOLS` pins their commits, and CI builds the same commits with Nix.

## 17. Open questions

- Can an AppContainer or a restricted token give Claude and other agents a sandbox on native Windows?
- Which `claude` flag keeps the project and user settings out of a run (6.6.4)?
- What can Codex read inside `workspace-write`?
- Does `C_VoiceChat.SpeakText` have any voices under Wine? A spike calls `C_VoiceChat.GetTtsVoices()` in the game.
- Can the bridge take a global push-to-talk hotkey on Wayland through the GlobalShortcuts portal?
- Two WoW accounts on one computer have two tokens. A hello from the second account starts a restore, and its `restored` flag retires the first token. How does the bridge tell two accounts from a saved-data wipe?

1. Does X11 capture of the WoW window work under XWayland? (Only for the fallback.)
2. Can font files replace the `.wav` signals?
3. How fast is HMAC-SHA256 in WoW Lua for a 3200-byte strip?
4. What are the ACP mode IDs of `claude-agent-acp` and `codex-acp`?
5. Does Gemini CLI have hooks for pings?
6. How large is the hitch at a higher window size? (The "Screen captured" hide works.)
7. Does the Aeneas standard library model cover the `Vec` and slice functions that the core needs?
