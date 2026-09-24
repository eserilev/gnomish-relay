# Leveling: a draft for later

Status: ideas only. We brainstorm this later. Nothing here is built, and nothing here is part of SPEC.md yet.

## Goal

Make the killer use case feel good: you start a long task, you keep playing, and the task finishes while you quest.
Levels are cosmetic. They never unlock a tool, a permission, or a feature.

## The core: Gnomish Engineering

- A profession skill from 1 to 300, as in classic professions. The window shows it next to the portrait: "Engineering 147/300".
- Each kind of task is a "recipe" with the classic colors: orange when new, then yellow, green, and gray.
  A new kind of task gives a big skill-up. The 50th task of the same kind gives almost nothing, so varied work pays.
- Ranks with trainer titles: Apprentice, Journeyman, Expert, Artisan, Master Tinker.

## What gives XP

XP comes from outcomes, never from usage.

- A task that ends `done`, not `error`.
- Tests that turn green after they were red.
- A pull request that you open. A bigger bonus when it merges.
- **AFK Tinkering:** a long run that finishes while you play.
- A terminal ping that you answer from the game.
- **Rested XP:** after a day with no coding, the next tasks give double XP. A small nudge against burnout.

## Achievements

Each one uses the real "Achievement earned" toast and sound.

| Achievement | How |
|---|---|
| Flawless Contraption | 100 green test runs |
| Zero to PR | From the first message to an open pull request, without leaving the game |
| Raid Night Deploy | A merge between 20:00 and midnight |
| Gnomeregan Survivor | A task that finished after a saved-data wipe |
| One More Quest | 10 tasks in one session |
| Rubber Duck | A task with questions only, no edits |

## Rewards

All cosmetic:

- Window skins: a brass frame, an elite gold-dragon portrait ring, Gnomeregan colors.
- New whisper colors and agent name colors.
- Mechanical pets as agent avatars, for example a clockwork squirrel for Claude and a mechanical chicken for Codex. A pet levels with use.
- Titles in the window: "Cogspinner <name>", "High Tinker <name>".

## Quests from your repos

- Daily quests from real work: "Fix one flaky test", "Close an issue labeled `good first issue`". The bridge reads the issues of your repos.
- A quest-log panel in the window, and `!` and `?` marks on the chat tiles.

## Rules

1. **No XP for "Allow" clicks.** A reward for approvals trains people to click without reading. That breaks the security design (SPEC.md 6.6).
2. **No XP for tokens or messages.** That rewards spam and cost, not good work.
3. **No gated features.** A level-1 user has the same relay as a level-60 user.
4. **Local only.** No leaderboard and no sharing by default. Task names can leak private work.
5. **The bridge awards XP, not the addon.** Another addon can change the saved data (SPEC.md 6.6.1). XP is cosmetic, but it stays honest.

## How it fits the design

- The bridge already knows each outcome. It adds "XP events" to the slot body. The body writer and its proofs (S9, S12) need a new field for them.
- The addon plays the level-up and achievement effects, and stores the level for display only.
- A good first slice: the Engineering skill bar and 10 achievements with the toast.

## Open questions

- Does XP count tasks from terminal sessions too, or only tasks from the game?
- How does the bridge see test results and merges: from the agent's output, from git, or from GitHub?
- Is Rested XP a nudge or a nag?
