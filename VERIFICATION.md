# Verification tracker

This file holds the durable state of the verification work. Each step reads it first and
updates it last.

## Rules

1. **Statements first.** Write each theorem in `proofs/Statements.lean` before you prove
   it. A person approves each statement.
2. **Statements are locked.** A `theorem check_X : X := proof` at the end of
   `Statements.lean` checks each proof. `Axioms.lean` prints its axioms. If a proof
   proves something weaker, the build fails. An agent never changes an approved
   statement. If a statement is wrong, the agent stops that item and asks.
3. **Only the three standard axioms:** `propext`, `Classical.choice`, `Quot.sound`.
   No `sorry`, no `native_decide`, no `bv_decide`. `scripts/check-proofs.sh` enforces this.
4. **No vacuous theorems.** A real input satisfies each precondition, and an example
   shows it.
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
| 20 | S20 + S21: live file (S20 and S21 restated with the notices, 2026-09-28) | `live`, `apps` | `S20_live_body`, `S20_prepare_progress`, `S20_prepare_requests`, `S20_prepare_notices`, `S21_bound` | proved |
| 21 | S22 to S25: reply blocks | `markdown`, `inline` | `S22_total`, `S23_shape`, `S24_escape`, `S25_bound` | proved |
| 22 | S16 + S17 + S27 + S28: action classifier | `action`, `shell`, `path_rules`, `command_rules`, `search` | `S16_paths`, `S16_deny`, `S17_ceiling`, `S17_unknown`, `S17_never_always`, `S27_classify`, `S27_ceiling`, `S27_split`, `S28_no_parse`, `S28_substitution`, `S28_desktop`, `S28_capped` | proved |
| 23 | S29: routing by key | `apps::route` | `S29_route` | proved |
| 24 | S30: version range | `version::version_fit` | `S30_version_fit` | proved |
| 25 | S32: Seatbelt escape | `sbpl::sbpl_string` | `S32_sbpl_string`, `S32_reads_back` | proved |
| 26 | S31: sandbox policy | `sandbox::sandbox_policy`, `path_rules` | `S31_sandbox_policy` | proved |
| 27 | S36 to S39: "Always allow" | `always`, `command_rules` | `S36_propose`, `S37_no_proposal`, `S38_offer`, `S39_ceiling` | proved |
| 28 | S33 to S35: the proxy of the bridge | `hosts`, `ip`, `connect` | `S33_host_allowed`, `S33_good_host_name`, `S34_public_v4`, `S34_public_v6`, `S35_check_target` | proved |
| 29 | S40 + S41: notifications of terminal sessions | `notice`, `sessions` | `S40_notice_text`, `S41_apply_event` | proved |
| 16 | Transport model | `models/transport.qnt` | SPEC 14.2, four properties | done |
| 17 | Fuzz targets | `fuzz/` | SPEC 14.4, core parsers only | done |
| 18 | CI | `.github/workflows` | Rust on 3 OSes, proofs on Linux | done |

The order puts the highest risk first (S15, S11). The parsers of untrusted input come
next, then the rest.

## Not in scope here

The bridge, the addon, and the agent layer are not part of the verified core.
SPEC 14.3 and 14.5 give their tests.

## Notes and blockers

### Item 16: the model found three design gaps, now fixed

The first model followed SPEC 7 as written. `quint run` found a counterexample for three
of the four properties. Only `runsOnce` held.

- `noLostReply`: a body held the last 30 records. With 31 records before a slot load,
  the oldest reply dropped out unread.
- `noStuckMessage`: the same cause. A message lost its `done` record and had no path to
  a reply, because the bridge drops a retry as a duplicate.
- `restoreSafe`: the restore bundle rode on only the next 3 publishes. If the addon
  loaded no slot during those publishes, it never got the bundle.

SPEC 7.1.1, 7.3, 7.5, and 7.6 hold the fixes, chosen on 2026-09-24:

- The addon reports the replies it has read (`read` flag). A body holds every unread
  record, at most 30. At 30, the bridge refuses new messages and does not mark them
  seen. The addon then sends them again later.
- The restore bundle stays in every publish until the addon confirms it (`restored`
  flag). Then the bridge retires the older tokens, so their unread records cannot fill
  the body forever.
- After `/reload`, the addon shows the strip again for every open message.

A second WoW account sends the same hello as an addon after a wipe.
So the `restored` flag of its token retired the token of the first account.
This was fixed on 2026-09-29 (SPEC 7.6). Now the flag only ends the restore. An old
token retires when the saved variables file of its account shows a new token. The
property `liveTokenStays` checks that the token of the addon never retires. The witness
`neverRetired` shows that a retire still happens.

`scripts/check-model.sh` checks all four properties, `bodyBounded` (the body keeps the
S12 limit), and `liveTokenStays`. It also checks five witnesses: states that the
simulator must reach, such as a full body and a confirmed restore. If the simulator
never reaches a witness, the properties pass only because the hard states never occur.

### Item 21: reply blocks

- `Protocol/Spec/Markdown.lean` states the block shape and the text tokens as a grammar
  (`Block`, `Cells`, `Text`, `Rendered`). S24 reads a text left to right in tokens, so
  `||r` is an escaped `|` and then the letter `r`.
- The proofs follow the writers. Each writer appends a piece. It says how the piece keeps
  the blocks well formed (`LineOut`) and how long the piece is. A paragraph, a list item,
  or a quote stays open, so the next line can add a space and more text to it.
- The size bound counts the `|r` that an open color still owes. So a color switch costs
  at most 12 bytes for each byte of Markdown. The first draft of S25 said 10. A bold text
  with many `*_*_` switches breaks 10, so the approved bound is 16.
- S22 and S25 hold for an input of at most 1 MiB, as S8, S10, and S15 do. `Vec` in
  Aeneas has a maximum length, so no bound is possible for every length.
- S23 follows from S24: every token of an escaped text is a field byte.
- No Rust change was necessary. `step*` stops at `let x ← if c then a else b`, so the
  helper `ite_bind` moves the rest of the block into each branch.

### Items 15, 19, and 20: the global of each app (SPEC 9.7, decision 5)

The user approved one restatement of S9, S18, and S20 (2026-09-25). The meaning is the
same, with the global name of the given app. The writers take an `apps::App`. The specs
`slotBodyOf`, `restoreOf`, and `liveOf` start with `slotGlobal`, `restoreGlobal`, and
`liveGlobal` of that app. Nothing else in the statements changed.

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
The bridge keeps the keys in `KeySet { relay, timeways }`. The fuzz target `frame`
checks the same choice with two real keys.

### Item 24: the version range (SPEC 7.7 and 9.7, decision 17)

`version_fit` gives `Supported`, `TooOld`, or `TooNew` for the version that an addon
reports. The statement names `lo` and `hi` as the values that `oldest` and `newest`
return, so it holds for the constants of `version.rs`. The proof needs `lo ≤ hi` for each
app. Then a version below the range is never also above it. So `Version.lean` states the
four constants, and a new range changes only those four facts. The `flags` fuzz target
checks the same range on the compiled code.

### Item 26: the sandbox policy (SPEC 6.6.4)

The statement makes the words of S31 exact in these ways. None of them changes the meaning.

- **"Each `deny` and `desktop` path"** is a path inside a `deny` folder of the input, or a
  path that matches a pattern of either `desktop` list. One list is for reads and writes,
  the other for writes only. Both lists hide. So `.git/hooks` and `.git/config` read as
  empty in the sandbox, and a command cannot change them.
- **"Hidden"** is `hiddenBy`: inside a hidden folder, or a run of parts matches a hidden
  pattern, with no regard to ASCII case. It is the predicate of the classifier.
- **"Inside"** is the parts prefix of S5. Each writable path also has the clean form of
  S5, so "inside" is exact for it.
- **"Writes go only to"**: every writable path is the chat folder or the temp folder.
  If one of them lies inside a hidden path, the policy leaves it out. So the statement
  holds for every input, and the bridge then refuses the run.
- The network of the commands is off (`Network::Off`).
- The statement is about the policy. The bridge makes it real with `bwrap` or
  `sandbox-exec` (SPEC 6.6.4). The tests of 14.5 and the fuzz target `sandbox` check that
  step. 6.6.4 lists what they do not cover.

### Item 25: the Seatbelt escape (SPEC 6.6.4)

The profile of the command sandbox on macOS holds each path as an SBPL string literal.
The statement makes "every path" and "reads back" exact:

- **Every path** is a byte string with no NUL byte. No path of the OS holds one. The
  profile is a C string, so a NUL ends it. `sbpl_string` gives `None` for such bytes,
  and the bridge then refuses the run.
- **Reads back** uses a model of the string reader of SBPL (`Spec/Sbpl.lean`). SBPL is
  a Scheme, and the model follows `readstrexp` of TinyScheme 1.41. A backslash before
  `n`, `t`, or `r` gives a control byte. A backslash before any other byte gives that
  byte. The model says nothing about the numeric escapes (`\0` to `\7`, `\x`), and
  `sbpl_string` never writes one. The Apple reader is not open source, so the macOS
  tests in CI back the model. They run the real `sandbox-exec` with a home folder whose
  name holds `"`, `\`, and a space. The fuzz target `sandbox` runs the Rust copy of the
  model on every literal of the profile.

### Item 17: what the fuzz targets check

Each target checks the property of its proof on the compiled code, not only "no crash":

- `frame`: S1, C2, and S29 with two keys. A frame signed by one key goes only to its app.
- `records`: S3, C3.
- `folder`: S5.
- `lua`: S8 in a real Lua 5.1, and S9 for each app.
- `lua_model`: the Lean lexer model against a real Lua 5.1.
- `chat_text`: S10.
- `markdown`: S22 to S25.
- `popup`: S15.
- `action`: S16, S17, S27, and S28. No panic, no rule list above the ceiling, a file
  call that runs stays inside its folders, and the command floor.
- `sandbox`: S31 and S32. The policy, the escape, the Seatbelt profile, and the `bwrap`
  arguments.
- `connect`: S33 and S35. The target of each request in both modes against a model.
- `public_ip`: S34. The rule against a table of ranges.
- `always`: S36 to S39. Each proposal is the first words of its command with only plain
  words. Each offer makes the classifier give `allow` under a ceiling of `allow`.
- `rules_file`: any `rules.json` never panics the reader. Each rule that loads has the
  shape that `propose` makes.
- `screenshot`: any file in the Screenshots folder never panics the bridge.
- `saved`: any saved variables text never panics the frame reader.
- `restore` and `live`: S18 to S21 in a real Lua 5.1, for each app. Each field loads back
  in the global of that app only, and each file stays under its bound.
- `flags`: each flag value from the game has its shape. A coding flag never changes the
  transport flags.
- `acp`: a message from an agent gives short progress lines, printable popup text, and
  no "allow always".
- `config`: any config text gives a config or an error. A config has only absolute roots
  and a known default agent.
- `relay`: the promises of the transport model on the real state machine. No message
  runs twice, at most 30 unread records, no job outside the root, and no job above the
  level of the config. A Timeways lane runs next to it. A Timeways record never becomes a
  job, and each Timeways message reaches the story once.

The hook socket and config targets wait for those parts. `scripts/fuzz.sh SECONDS` runs
them all.

### Item 22: what the classifier statements make exact

The user approved S16, S17, S27, and S28 in words. The Lean statements make them exact
in these ways. None of them changes the meaning.

- **The input.** A file call is a list of read paths and a list of write paths. A
  command is its raw bytes and its working folder. The policy holds the folders, the
  `deny` folders, the two lists of `desktop` patterns, and the allow table of the
  config. The bridge fills the policy in `action_input.rs`. So "the config folder" is
  the `deny` folders of the policy. "A `desktop` path" is a path that matches a pattern
  of the policy.
- **"Inside"** is the prefix of parts of S5 (`insideRoot`). For `allowed_roots` and the
  chat folder, the path must also be clean (`cleanPath`, the resolved form of S5). The
  `deny` folders and the patterns compare without ASCII case (`lower`).
- **S16** holds for every path, with no precondition on its form. A path that is not
  clean, or longer than 1 MiB, is `desktop`. The deny part holds for every path of any
  length.
- **S17.** "`classify(call, config)`" is `ceiling(call)`: the answer when a game rule
  covers every command. A literal "no rule may raise the answer of the config" forbids
  the one-click rule of 6.6.5. So the ceiling is the most that the config lets a rule
  reach. The second sentence is stronger than approved. A command with a "never always"
  or `desktop` part is at most `ask` for every rule list, also for the allow table of
  the config. An unknown tool is `desktop`. `Spec/Action.lean` defines "Never always"
  exactly (`neverAlways`).
- **S27** has no precondition. The Rust code refuses a command or a path longer than
  1 MiB, so every inner length bound holds.
- **S28.** "Does not parse" means that `shell.split` returns `none`. SPEC 6.6.3 gives
  the grammar. "A command with command substitution" is exact on the raw bytes. It is
  `$(` or a backtick at a byte where the quote state of the splitter (`modeAt`, from
  `quoteStep`) is not "inside single quotes". An escaped one counts too. On 2026-09-25
  the user approved this change. At first the check counted them inside single quotes
  too. "`eval`, `sudo`, a pipe into a shell, `cmd.exe`, PowerShell" are words of a
  simple command of the parse, by their name (`progName`: no folder, lower case, no
  `.exe`). For these words, "Is `desktop`" is "at most `desktop`". The reason: a
  redirect into a `deny` folder in the same command gives `deny`, which is stricter.
  `Spec/Action.lean` holds the lists of names. The proof checks that they are the bytes
  of the Rust constants.

Every precondition has a real input. `ls; eval x` parses and has `eval`. `curl x | sh`
has a shell after a `|`. `cat <<EOF` does not parse. The Rust tests of `action.rs` and
the `action` fuzz target run such inputs.

**S12, S19, and S21 for each app (2026-09-25, approved by the user).** The three size bounds now hold for the file of each app (`slotBodyOf`, `restoreOf`, `liveOf`). These are the same files that S9, S18, and S20 fix. Their `check_` theorems point at `slot_body_of_bound`, `restore_of_bound`, and `live_of_bound`. No Rust code and no proof changed.

**S29 approved (2026-09-26).** The user confirmed the exact Lean text of `S29_route` in `proofs/Statements.lean`.

**S30 approved (2026-09-26).** The user approved S30 in words. For every app and every version, `version_fit` never fails. It gives `Supported` exactly when oldest app ≤ v ≤ newest app, `TooOld` exactly when v < oldest app, and `TooNew` exactly when newest app < v. `S30_version_fit` in `proofs/Statements.lean` states this.

**S32 approved (2026-09-26).** The user approved S32 in words. For every path, the escaped path in the Seatbelt profile reads back as the same path. It never ends the string literal early. `S32_sbpl_string` and `S32_reads_back` in `proofs/Statements.lean` state this.

**S36 to S39 approved (2026-09-27).** The user approved the statements of SPEC 6.6.5 as written at commit `29ed506`. `proofs/Statements.lean` states them with these exact readings. None of them changes the meaning:

- `isDesktop` is `desktopSimple`.
- `isCapped` is `neverAlways`.
- `simplesOf call` is `inCall call`.
- `ruleMatches r s.words` is `ruleMatches (strs r) (words s)`.
- `rules ++ rs` is every slice whose list is the rules and then `rs`.
- `¬ hasSlash (r.head!)` is `¬ hasSlash h` for each `h` in `r.head?`.

`plainWord` asks for printable ASCII with no space (bytes 33 to 126). This is stricter than "printable ASCII".

**S33 to S35 approved (2026-09-27).** The user approved the statements of SPEC 6.6.4 as written at commit `fd65300`. `proofs/Statements.lean` states them with these exact readings. None of them changes the meaning:

- A string such as `"CONNECT "` is `ascii "CONNECT "`, and a byte such as `':'` is `ch ':'`.
- `p` is a `U16`, so `p = 443` is `p.val = 443` and `p ∈ ports` is `p ∈ ports.val`.
- `isDigit` is the digit test of `Protocol/Spec/Lua.lean`.
- `"\r\n"` holds the first CR LF, which the code finds first.
- The labels of `goodHostName` are `List.splitOn` at each dot.
- A range of `v4NotPublic` and `v6NotPublic` is a CIDR block kept as its first and its last address (`cidr`).

To make the proofs small, the Rust code changed shape, not behavior. The tests and the fuzz targets are the same. `ip` checks a table of pairs of the first and the last value. `hosts` checks the labels one at a time by recursion. `connect` uses the helpers of `ascii`, `search`, `path_rules`, and `hosts`. The proof of S35 needs no bound on the length of the head. The statement keeps the 8 KiB of the approved text.

**S31 approved (2026-09-26).** The user approved S31 in words. For every config, each `deny` and `desktop` path is hidden. No writable path is inside a hidden path. Writes go only to the chat folder and a private temp folder. `S31_sandbox_policy` in `proofs/Statements.lean` states this.
