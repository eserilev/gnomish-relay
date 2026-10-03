# Changelog

## 0.5.1

- If you have both WoW: Forever and TBC Anniversary, Gnomish Relay now works in both at
  the same time. No more switching.
- When the desktop app is too old for your version of WoW, the game now says so and tells
  you how to update it.

## 0.5.0

- **TBC Anniversary.** Gnomish Relay now works in WoW Classic: TBC Anniversary too. Get
  the addon on CurseForge, then run `gnomish-relay setup` on your desktop.
- If you have both WoW: Forever and TBC Anniversary, the desktop app uses the one you
  played last. To switch, run `gnomish-relay setup --wow <folder>`.

## 0.4.4

- **Mini chats.** Pop a chat out into its own small window, and keep an eye on it while you
  play. Use the **Pop out** button in the chat header, or right-click a chat and pick
  **Pop out**.
- A mini chat has the full conversation, the message box, and the permissions (ask,
  auto-edit, full-auto). Drag its corner to resize it.
- You can have up to 4 mini chats. They stay where you put them, come back after `/reload`,
  and work in combat.
- A new reply turns the mini chat's header gold. Click **Open in main window** to see Stop
  and Activity.

## 0.4.3

- The desktop app now updates itself when the CurseForge app updates the addon. It waits
  until no chat is running. To turn this off, add `auto_update = false` to `config.toml`.

## 0.4.2

### Fixes

- Windows: setup wrote project folders in your user folder in a way the desktop app
  couldn't read.
- Windows: `gnomish-relay report` now hides your user name in every path.

## 0.4.1

- No changes for players. This version fixes a build problem.

## 0.4.0

- **Full-auto for one chat.** Pick full-auto in the chat header, or press Shift+Tab.
  Approve once on your desktop, and the agent works in that folder without asking. The
  header shows full-auto in orange-red.
- When a request waits for you on your desktop, the chat shows it, with a **Copy** button
  for the approve command.
- New command: `gnomish-relay report`. It saves one file with your recent log, status, and
  settings for a bug report, with keys and tokens removed.
- Press Up and Down in the message box to bring back your earlier messages.
- The folder browser works like a file explorer: open a folder, go back, and search.
- Replies show in full. No more **Show more**.
- **Resume** shows the sessions of every folder you trust, and asks before it opens one.
- Long chat, agent, and branch names end in "..." so they stay in their place.
- Setup finds more of your projects, also ones grouped in subfolders.
- The cost of a Claude run shows only when you pay with an API key.

### Fixes

- Chats keep their folder when your default folder changes.
- A request on your desktop keeps waiting when its notification closes without an answer.
- Resumed sessions no longer show stray terminal characters.

## Earlier

0.3.1 and before: the first versions. Chat with Claude Code and Codex from WoW, answer
permission requests in the game, approve risky actions on your desktop, get notifications
from your terminal, and commit or revert changes with one click. The addon now comes from
CurseForge.
