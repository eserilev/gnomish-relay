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
| 16 | Transport model | `models/transport.qnt` | SPEC 14.2, four properties | blocked |
| 17 | Fuzz targets | `fuzz/` | SPEC 14.4, core parsers only | done |
| 18 | CI | `.github/workflows` | Rust on 3 OSes, proofs on Linux | done |

The order puts the highest risk first (S15, S11), then the parsers of untrusted
input, then the rest.

## Not in scope here

The bridge, the addon, and the agent layer. They are not part of the verified core.
Their tests are in SPEC 14.3 and 14.5.

## Notes and blockers

### Item 16: three of the four transport properties fail on SPEC as written

`quint run` finds a counterexample for each of these. The model follows SPEC 7, so
the gaps are in the design, not in the model. Each fix changes SPEC, so a person
decides it. Until then, `scripts/check-model.sh` checks only `runsOnce`.

- `runsOnce` (the agent never runs one message twice): holds in 150,000 traces.
- `noLostReply` fails. A body holds the last 30 records (SPEC 7.3). If 31 records
  arrive before the addon loads a slot, the oldest reply drops out unread. This
  happens when the slot pool is empty and the user does not press the reload key.
- `noStuckMessage` fails for the same reason. The addon takes a message off the
  strip when it sees the `working` record. If the `done` record then drops out, no
  path sends the message again. The bridge drops the retry as a duplicate.
- `restoreSafe` fails. The restore bundle rides on "the next 3 publishes" (SPEC 7.6).
  If 3 publishes happen before the addon loads a slot, the bundle is gone.

The model also fixes one point that SPEC does not state: after `/reload`, the addon
shows the strip again for every open message that is not in the outbox. Without
it, `noStuckMessage` fails at every `/reload`.

### Item 17: what the fuzz targets check

Each target checks the property of its proof on the compiled code, not only "no crash":
`frame` (S1, C2), `records` (S3, C3), `folder` (S5), `lua` (S8 in a real Lua 5.1),
`lua_model` (the Lean lexer model against a real Lua 5.1), `chat_text` (S10), and
`popup` (S15). The PNG, hook socket, and config targets of SPEC 14.4 belong to the
bridge, so they wait for it. `scripts/fuzz.sh SECONDS` runs them all.
