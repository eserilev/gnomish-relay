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
| 15 | S9 + S12: slot body | `slot` | `S9_slot_body`, `S12_prepare`, `S12_bound` | proved |
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

### Item 17: what the fuzz targets check

Each target checks the property of its proof on the compiled code, not only "no crash":
`frame` (S1, C2), `records` (S3, C3), `folder` (S5), `lua` (S8 in a real Lua 5.1),
`lua_model` (the Lean lexer model against a real Lua 5.1), `chat_text` (S10), and
`popup` (S15). The PNG, hook socket, and config targets of SPEC 14.4 belong to the
bridge, so they wait for it. `scripts/fuzz.sh SECONDS` runs them all.
