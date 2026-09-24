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

Legend: `todo`, `stated` (approved, not proved), `proved`, `blocked`.

| # | Item | Rust | Statement | Status |
|---|---|---|---|---|
| 1 | C1: cell round trip | `cell` | `C1` | proved |
| 2 | S11: freshness | `frame::is_fresh` | `S11_fresh` | proved |
| 3 | S2 + S11: frame check | `frame::check_frame` | `S2_S11_check` | proved |
| 4 | S6: permission level and answers | `policy` | `S6_level`, `S6_answer` | proved |
| 5 | S15: permission popup | `popup`, `ascii` | `S15_popup`, `S15_printable`, `S15_faithful` | proved |
| 6 | S8: Lua literals | `lua` | `S8_lua_string`, `S8_reads_back` | stated |
| 7 | S10: WoW chat text | `wow_text` | `S10_chat_safe` | stated |
| 8 | C2: frame encoder | `frame::encode_frame` | `C2_encode`, `C2_encode_too_long` | stated |
| 9 | C2 + S1 + S2: frame decoder | `frame::decode_frame`, `signed_len` | `C2_decode`, `S1_S2_decode`, `S2_signed_len` | stated |
| 10 | S13: id charset | `record::is_valid_id` | `S13_valid_id` | stated |
| 11 | C3 + S3 + S4: records | `record` | `C3_parse`, `C3_serialize`, `S3_S4_parse` | stated |
| 12 | S5: folder policy | `folder` | `S5_folder`, `S5_folder_complete` | stated |
| 13 | S7: replay protection | `seen` | `S7_seen` | stated |
| 14 | S14: rate limit and queue | `rate` | `S14_admit`, `S14_window`, `S14_queue` | stated |
| 15 | S9 + S12: slot body | `slot` | `S9_slot_body`, `S12_prepare`, `S12_bound` | stated |
| 16 | Transport model | `models/transport.qnt` | SPEC 14.2, four properties | todo |
| 17 | Fuzz targets | `fuzz/` | SPEC 14.4, core parsers only | todo |
| 18 | CI | `.github/workflows` | Rust on 3 OSes, proofs on Linux | todo |

The order puts the highest risk first (S15, S11), then the parsers of untrusted
input, then the rest.

## Not in scope here

The bridge, the addon, and the agent layer. They are not part of the verified core.
Their tests are in SPEC 14.3 and 14.5.

## Notes and blockers

None yet.
