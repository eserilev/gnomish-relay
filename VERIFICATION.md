# Verification tracker

This file is the durable state of the verification work. Every step starts by
reading it and ends by updating it.

## Rules

1. **Statements first.** Every theorem is stated in `proofs/Statements.lean` before
   it is proved. A person approves each statement.
2. **Statements are locked.** A `theorem check_X : X := proof` at the end of
   `Statements.lean` checks each proof, and `Axioms.lean` prints its axioms. If a proof proves something weaker, the
   build fails. An agent never changes an approved statement. If a statement is
   wrong, the agent stops that item and asks.
3. **Only the three standard axioms:** `propext`, `Classical.choice`, `Quot.sound`.
   No `sorry`, no `native_decide`, no `bv_decide`. `scripts/check-proofs.sh` enforces this.
4. **No vacuous theorems.** Each precondition has an example that a real input
   satisfies.
5. **One commit per item,** after `scripts/check-all.sh` passes, with this file updated.
6. **Stuck rule.** After 5 real attempts on one proof, mark it `blocked`, write
   down why, and move to the next item.

## Status

Legend: `todo`, `stated` (approved, not proved), `proved`, `done` (for work that is not a proof), `blocked`.

| # | Item | Rust | Statement | Status |
|---|---|---|---|---|
| 1 | C1: cell round trip | `cell` | `C1` | proved |
| 2 | S11: freshness | `frame::is_fresh` | `S11_fresh` | proved |
| 3 | S2 + S11: frame check | `frame::check_frame` | `S2_S11_check` | proved |
| 4 | S6: permission level and answers | `policy` | `S6_level`, `S6_answer` | proved |
| 5 | S15: permission popup | `popup`, `ascii` | `S15_popup`, `S15_printable`, `S15_faithful` | proved |
| 6 | S8: Lua literals | `lua` | `S8_lua_string`, `S8_reads_back` | proved |
| 7 | S10: WoW chat text | `wow_text` | `S10_chat_safe` | proved |
| 8 | C2: frame encoder | `frame::encode_frame` | `C2_encode`, `C2_encode_too_long` | proved |
| 9 | C2 + S1 + S2: frame decoder | `frame::decode_frame`, `signed_len` | `C2_decode`, `S1_S2_decode`, `S2_signed_len` | proved |
| 10 | S13: id charset | `record::is_valid_id` | `S13_valid_id` | proved |
| 11 | C3 + S3 + S4: records | `record` | `C3_parse`, `C3_serialize`, `S3_S4_parse` | proved |
| 12 | S5: folder policy | `folder` | `S5_folder`, `S5_folder_complete` | proved |
| 13 | S7: replay protection | `seen` | `S7_seen` | proved |
| 14 | S14: rate limit and queue | `rate` | `S14_admit`, `S14_window`, `S14_queue` | proved |
| 15 | S9 + S12: slot body | `slot`, `apps` | `S9_slot_body`, `S12_prepare`, `S12_bound` | proved |
| 19 | S18 + S19: restore file | `restore`, `apps` | `S18_restore_body`, `S18_prepare`, `S19_bound` | proved |
| 20 | S20 + S21: live file | `live`, `apps` | `S20_live_body`, `S20_prepare_progress`, `S20_prepare_requests`, `S21_bound` | proved |
| 21 | S22 to S25: reply blocks | `markdown`, `inline` | `S22_total`, `S23_shape`, `S24_escape`, `S25_bound` | proved |
| 22 | S16 + S17 + S27 + S28: action classifier | `action`, `shell`, `path_rules`, `command_rules`, `search` | `S16_paths`, `S16_deny`, `S17_ceiling`, `S17_unknown`, `S17_never_always`, `S27_classify`, `S27_ceiling`, `S27_split`, `S28_no_parse`, `S28_substitution`, `S28_desktop`, `S28_capped` | proved |
| 23 | S29: routing by key | `apps::route` | `S29_route` | proved |
| 16 | Transport model | `models/transport.qnt` | SPEC 14.2, four properties | done |
| 17 | Fuzz targets | `fuzz/` | SPEC 14.4, core parsers only | done |
| 18 | CI | `.github/workflows` | Rust on 3 OSes, proofs on Linux | done |

The order puts the highest risk first (S15, S11), then the parsers of untrusted
input, then the rest.

## Not in scope here

The bridge, the addon, and the agent layer. They are not part of the verified core.
Their tests are in SPEC 14.3 and 14.5.

## Notes and blockers

### Item 16: the model found three design gaps, now fixed

The first model followed SPEC 7 as written. `quint run` found a counterexample for three
of the four properties. Only `runsOnce` held.

- `noLostReply`: a body held the last 30 records. With 31 records before a slot load,
  the oldest reply dropped out unread.
- `noStuckMessage`: the same cause. A message whose `done` record dropped out had no
  path to a reply, because the bridge drops a retry as a duplicate.
- `restoreSafe`: the restore bundle rode on only the next 3 publishes. An addon that
  loaded no slot during those publishes never got it.

The fixes, chosen on 2026-09-24, are in SPEC 7.1.1, 7.3, 7.5, and 7.6:

- The addon reports the replies it has read (`read` flag). A body holds every unread
  record, at most 30. With 30, the bridge refuses new messages and does not mark them
  seen, so the addon sends them again later.
- The restore bundle stays in every publish until the addon confirms it (`restored`
  flag). Then the bridge retires the older tokens, so their unread records cannot fill
  the body forever.
- After `/reload`, the addon shows the strip again for every open message.

`scripts/check-model.sh` checks all four properties and `bodyBounded` (the body keeps
the S12 limit). It also checks four witnesses: states such as a full body and a
confirmed restore, which the simulator must reach. A witness that is never reached
means that the properties pass only because the hard states never happen.

### Item 21: reply blocks

- `Protocol/Spec/Markdown.lean` states the block shape and the text tokens as a grammar
  (`Block`, `Cells`, `Text`, `Rendered`). S24 reads a text left to right in tokens, so
  `||r` is an escaped `|` and then the letter `r`.
- The proofs follow the writers. Each writer appends a piece and says how the piece keeps
  the blocks well formed (`LineOut`) and how long it is. A paragraph, a list item, or a
  quote stays open, so the next line can add a space and more text to it.
- The size bound counts the `|r` that an open color still owes, so a color switch costs
  at most 12 bytes for each byte of Markdown. The first draft of S25 said 10. A bold text
  with many `*_*_` switches breaks 10, so the approved bound is 16.
- S22 and S25 hold for an input of at most 1 MiB, as S8, S10, and S15 do. `Vec` in Aeneas
  has a maximum length, so no bound is possible for every length.
- S23 follows from S24: every token of an escaped text is a field byte.
- No Rust change was needed. `step*` stops at `let x ← if c then a else b`, so the helper
  `ite_bind` moves the rest of the block into each branch.

### Items 15, 19, and 20: the global of each app (SPEC 9.7, decision 5)

The user approved one restatement of S9, S18, and S20 (2026-09-25): the same meaning,
with the global name that belongs to the given app. The writers take an `apps::App`, and
the specs `slotBodyOf`, `restoreOf`, and `liveOf` start with `slotGlobal`,
`restoreGlobal`, and `liveGlobal` of that app. Nothing else in the statements changed.

- The bounds S12, S19, and S21 are not restated. They still speak of the relay files
  (`slotBodyBytes`, `restoreBytes`, and `liveBytes`, now the relay case of each spec).
- The proofs show the same bounds for every app (`slot_body_of_bound`,
  `restore_of_bound`, and `live_of_bound`). Each `check_` theorem of a bound uses the
  relay case of one of them. A restatement of S12, S19, and S21 over the app needs a new
  approval.

### Item 23: routing by key (SPEC 9.7, decision 2)

The user approved routing as S29 in words: "S29 proves the choice, not the
cryptography". `route` takes the two tag results as bools, so `verify_tag` stays opaque,
as in S2. The statement lists all four inputs, and `induction` on each bool proves it.
The bridge keeps the keys in `KeySet { relay, timeways }`, and the fuzz target `frame`
checks the same choice with two real keys.

### Item 17: what the fuzz targets check

Each target checks the property of its proof on the compiled code, not only "no crash":
`frame` (S1, C2, and S29 with two keys: a frame signed by one key goes only to its app), `records` (S3, C3), `folder` (S5), `lua` (S8 in a real Lua 5.1, and S9 for each app),
`lua_model` (the Lean lexer model against a real Lua 5.1), `chat_text` (S10), `markdown` (S22 to S25), and
`popup` (S15), `action` (S16, S17, S27, and S28: no panic, no rule list above the ceiling, a file call that runs stays inside its folders, and the command floor), `screenshot` (any file in the Screenshots folder never panics the
bridge), `saved` (any saved variables text never panics the frame reader), `restore` and
`live` (S18 to S21 in a real Lua 5.1, for each app: each field loads back in the global
of that app only, and each file stays under its bound), `flags` (each flag value from the game has its shape, and a coding flag never changes the transport flags), `acp` (a message
from an agent gives short progress lines, printable popup text, and no "allow always"), `config` (any
config text gives a config or an error, and a config has only absolute roots and a
known default agent), and `relay` (the promises of the transport model on the real state machine:
no message runs twice, at most 30 unread records, no job outside the root, no job
above the level of the config; a Timeways lane runs next to it, a Timeways record never
becomes a job, and each Timeways message reaches the story once). The hook
socket and config targets wait for those parts. `scripts/fuzz.sh SECONDS` runs them all.

### Item 22: what the classifier statements make exact

The user approved S16, S17, S27, and S28 in words. The Lean statements make them exact
in these ways. None of them changes the meaning.

- **The input.** A file call is a list of read paths and a list of write paths. A
  command is its raw bytes and its working folder. The policy holds the folders, the
  `deny` folders, the two lists of `desktop` patterns, and the allow table of the
  config. The bridge fills the policy in `action_input.rs`. So "the config folder" is
  the `deny` folders of the policy, and "a `desktop` path" is a path that matches a
  pattern of the policy.
- **"Inside"** is the prefix of parts of S5 (`insideRoot`). For `allowed_roots` and the
  chat folder, the path must also be clean (`cleanPath`, the resolved form of S5). The
  `deny` folders and the patterns compare without ASCII case (`lower`).
- **S16** holds for every path, with no precondition on its form: a path that is not
  clean, or longer than 1 MiB, is `desktop`. The deny part holds for every path of any
  length.
- **S17.** "`classify(call, config)`" is `ceiling(call)`: the answer when a game rule
  covers every command. A literal "no rule may raise the answer of the config" would
  forbid the one-click rule of 6.6.5, so the ceiling is the most that the config lets a
  rule reach. The second sentence is stronger than approved: a command with a "never
  always" or `desktop` part is at most `ask` for every rule list, also for the allow
  table of the config, and an unknown tool is `desktop`. "Never always" has an exact
  definition in `Spec/Action.lean` (`neverAlways`).
- **S27** has no precondition. The Rust code refuses a command or a path longer than
  1 MiB, so every inner length bound holds.
- **S28.** "Does not parse" means that `shell.split` returns `none`; the grammar is in
  SPEC 6.6.3. "A command with command substitution" is exact on the raw bytes: `$(` or a
  backtick at a byte where the quote state of the splitter (`modeAt`, from
  `quoteStep`) is not "inside single quotes". An escaped one counts too. On
  2026-09-25 the user approved this change: at first the check counted them inside
  single quotes too. "`eval`, `sudo`, a pipe into a shell,
  `cmd.exe`, PowerShell" are words of a simple command of the parse, by their name
  (`progName`: no folder, lower case, no `.exe`). "Is `desktop`" is "at most `desktop`"
  for these words, because a redirect into a `deny` folder in the same command gives
  `deny`, which is stricter. The lists of names are in `Spec/Action.lean`, and the proof
  checks that they are the bytes of the Rust constants.

Every precondition has a real input: `ls; eval x` parses and has `eval`, `curl x | sh`
has a shell after a `|`, and `cat <<EOF` does not parse. The Rust tests of `action.rs`
and the `action` fuzz target run such inputs.

**S12, S19, and S21 for each app (2026-09-25, approved by the user).** The three size bounds now hold for the file of each app (`slotBodyOf`, `restoreOf`, `liveOf`), the same files that S9, S18, and S20 fix. Their `check_` theorems point at `slot_body_of_bound`, `restore_of_bound`, and `live_of_bound`. No Rust code and no proof changed.
