import Protocol.Cell.Proofs
import Protocol.Frame.Fresh
import Protocol.Spec.Frame
import Protocol.Spec.Record
import Protocol.Spec.Folder
import Protocol.Spec.Lua
import Protocol.Spec.WowText
import Protocol.Spec.Slot
import Protocol.Spec.Rate
import Protocol.Spec.Popup
import Protocol.Spec.Policy
import Protocol.Policy
import Protocol.Popup
import Protocol.Lua
import Protocol.WowText
import Protocol.Frame.Codec
import Protocol.Record

/-!
# The theorems, stated

Each `def` below is a statement that a person approved. The proofs live in other
files. When a proof is done, a `check_` theorem at the end of this file checks it
against its approved statement. So a proof cannot quietly prove something weaker: if the
theorem changes, this file fails to build.

`f x ⦃ r => P r ⦄` means: `f x` returns without a panic, and its result `r` has
property `P`. So every statement below also says "never panics".

Read each statement with the definitions in `Protocol/Spec/`.
-/

open Aeneas Aeneas.Std Result protocol Protocol.Spec

namespace Protocol.Statements

/-! ## Cells (proved) -/

/-- **C1.** Cells decode to the same bytes, plus the zero padding of the last group. -/
def C1 : Prop :=
  ∀ input : Slice U8, input.val.length ≤ 65536 →
    (do
      let cells ← cell.encode_cells input
      cell.decode_cells (alloc.vec.Vec.deref cells))
    ⦃ r => ∃ out, r = some out ∧
      out.val = input.val ++ List.replicate (Protocol.Cell.padLength input.val.length) 0#u8 ⦄

/-! ## Frames -/

/-- **C2, encoder.** A payload that fits gives exactly the frame layout. -/
def C2_encode : Prop :=
  ∀ (time : U32) (frameId : U16) (payload : Slice U8) (tag : Std.Array U8 8#usize),
    payload.val.length ≤ maxPayload →
    frame.encode_frame time frameId payload tag ⦃ r => ∃ v, r = some v ∧
      bytes v.val = frameBytes time.val frameId.val (bytes payload.val) (bytes tag.val) ⦄

/-- **C2, encoder limit.** A payload that is too long gives `None`. -/
def C2_encode_too_long : Prop :=
  ∀ (time : U32) (frameId : U16) (payload : Slice U8) (tag : Std.Array U8 8#usize),
    maxPayload < payload.val.length →
    frame.encode_frame time frameId payload tag ⦃ r => r = none ⦄

/-- **C2, decoder.** A frame, plus any padding after it, decodes to its own fields. -/
def C2_decode : Prop :=
  ∀ (input : Slice U8) (time : U32) (frameId : U16) (payload tag extra : List Spec.Byte),
    payload.length ≤ maxPayload → tag.length = 8 →
    bytes input.val = frameBytes time.val frameId.val payload tag ++ extra →
    frame.decode_frame input ⦃ r => ∃ f, r = .Ok f ∧ f.time = time ∧ f.frame_id = frameId ∧
      bytes f.payload.val = payload ∧ bytes f.tag.val = tag ⦄

/-- **S1 + S2, decoder.** For every input: no panic, and a decoded frame is exactly the
frame layout with a correct magic, version, length, and checksum. -/
def S1_S2_decode : Prop :=
  ∀ input : Slice U8,
    frame.decode_frame input ⦃ r => match r with
      | .Ok f => f.payload.val.length ≤ maxPayload ∧ ∃ extra,
          bytes input.val =
            frameBytes f.time.val f.frame_id.val (bytes f.payload.val) (bytes f.tag.val) ++ extra
      | .Err _ => True ⦄

/-- **S2, tag coverage.** The tag covers every byte before it, and nothing else. -/
def S2_signed_len : Prop :=
  ∀ f : frame.Frame, f.payload.val.length ≤ maxPayload →
    frame.signed_len f ⦃ n =>
      n.val = (frameSigned f.time.val f.frame_id.val (bytes f.payload.val)).length ⦄

/-- **S11.** `is_fresh` is exactly the freshness rule. -/
def S11_fresh : Prop :=
  ∀ frameTime now : U32,
    frame.is_fresh frameTime now ⦃ b => (b = true ↔ fresh frameTime.val now.val) ⦄

/-- **S2 + S11.** A frame passes only with a good tag and a fresh time. -/
def S2_S11_check : Prop :=
  ∀ (frameTime : U32) (tagOk : Bool) (now : U32),
    frame.check_frame frameTime tagOk now ⦃ r =>
      (r = .Ok () ↔ (tagOk = true ∧ fresh frameTime.val now.val)) ⦄

/-! ## Records -/

/-- **S13.** `is_valid_id` is exactly the id charset rule. -/
def S13_valid_id : Prop :=
  ∀ b : Slice U8, record.is_valid_id b ⦃ r => (r = true ↔ validId (bytes b.val)) ⦄

/-- **C3.** Well-formed records parse back to themselves. -/
def C3_parse : Prop :=
  ∀ (rs : List record.Record) (payload : Slice U8),
    1 ≤ rs.length → rs.length ≤ maxRecords → (∀ r ∈ rs, wellFormed r) →
    bytes payload.val = recordsBytes rs →
    record.parse_records payload ⦃ res => ∃ out, res = .Ok out ∧ out.val = rs ⦄

/-- **C3, serializer.** `serialize_records` writes exactly the record layout. -/
def C3_serialize : Prop :=
  ∀ rs : Slice record.Record, (recordsBytes rs.val).length ≤ 2 ^ 20 →
    record.serialize_records rs ⦃ v => bytes v.val = recordsBytes rs.val ⦄

/-- **S3 + S4 + S13, parser.** For every payload: no panic, and parsed records are
well-formed (valid ids, no field bleeds into another) and rebuild the exact payload. -/
def S3_S4_parse : Prop :=
  ∀ payload : Slice U8,
    record.parse_records payload ⦃ res => match res with
      | .Ok out => 1 ≤ out.val.length ∧ out.val.length ≤ maxRecords ∧
          (∀ r ∈ out.val, wellFormed r) ∧ recordsBytes out.val = bytes payload.val
      | .Err _ => True ⦄

/-! ## Folders -/

/-- **S5.** An accepted folder is clean and inside a root. -/
def S5_folder : Prop :=
  ∀ (roots : Slice (alloc.vec.Vec U8)) (base request : Slice U8),
    base.val.length + request.val.length ≤ 2 ^ 20 →
    folder.resolve_folder roots base request ⦃ r => match r with
      | some p => cleanPath (bytes p.val) ∧ ∃ root ∈ roots.val, insideRoot (bytes root.val) (bytes p.val)
      | none => True ⦄

/-- **S5, usefulness.** A plain subfolder of a folder inside a root is accepted as is. -/
def S5_folder_complete : Prop :=
  ∀ (roots : Slice (alloc.vec.Vec U8)) (base request : Slice U8) (root : alloc.vec.Vec U8),
    base.val.length + request.val.length ≤ 2 ^ 20 →
    root ∈ roots.val → cleanPath (bytes base.val) → insideRoot (bytes root.val) (bytes base.val) →
    cleanRelative (bytes request.val) →
    folder.resolve_folder roots base request ⦃ r => ∃ p, r = some p ∧
      bytes p.val = bytes base.val ++ [slash] ++ bytes request.val ⦄

/-! ## Permissions -/

/-- **S6.** The game can lower the level, never raise it. -/
def S6_level : Prop :=
  ∀ config requested : policy.Level,
    policy.effective_level config requested ⦃ e => rank e = min (rank config) (rank requested) ⦄

/-- **S6.** "Allow always" from the game never survives. Other answers pass unchanged. -/
def S6_answer : Prop :=
  ∀ a : policy.Answer,
    policy.answer_from_game a ⦃ b => b ≠ .AllowAlways ∧ (a ≠ .AllowAlways → b = a) ⦄

/-! ## Replay -/

def seenKeys (s : seen.Seen) : List (List Spec.Byte × Nat) :=
  s.entries.val.map fun e => (bytes e.token.val, e.id.val)

/-- **S7.** A message is new exactly when it is not remembered. A new message is
remembered, and only the oldest ones are forgotten, to keep 1000. -/
def S7_seen : Prop :=
  ∀ (history : seen.Seen) (token : Slice U8) (id : U32),
    (seenKeys history).length ≤ 1000 → token.val.length ≤ 2 ^ 16 →
    seen.admit history token id ⦃ res =>
      let key := (bytes token.val, id.val)
      (res.1 = true ↔ key ∉ seenKeys history) ∧
      (res.1 = true → seenKeys res.2 =
        (seenKeys history ++ [key]).drop ((seenKeys history).length + 1 - 1000)) ∧
      (res.1 = false → seenKeys res.2 = seenKeys history) ⦄

/-! ## Lua slot files -/

/-- **S8, writer.** `lua_string` writes exactly `luaLiteral`. -/
def S8_lua_string : Prop :=
  ∀ s : Slice U8, s.val.length ≤ 2 ^ 20 →
    lua.lua_string s ⦃ v => bytes v.val = luaLiteral (bytes s.val) ⦄

/-- **S8, reader.** Lua 5.1 reads `luaLiteral s` back as `s`, and the literal ends
exactly where it should. Whatever follows is left alone. -/
def S8_reads_back : Prop :=
  ∀ s rest : List Spec.Byte, luaReadString (luaLiteral s ++ rest) = some (s, rest)

/-- **S9.** The slot body is exactly the fixed template with escaped holes. -/
def S9_slot_body : Prop :=
  ∀ (now : U32) (replies : Slice slot.Reply), fitsSlot replies.val →
    slot.slot_body now replies ⦃ v => bytes v.val = slotBodyBytes now.val replies.val ⦄

/-- **S12.** `prepare_replies` keeps the last 30 replies, cuts only the ends of texts,
and makes them fit. -/
def S12_prepare : Prop :=
  ∀ replies : Slice slot.Reply,
    slot.prepare_replies replies ⦃ ps =>
      fitsSlot ps.val ∧
      List.Forall₂ preparedFrom (replies.val.drop (replies.val.length - maxReplies)) ps.val ⦄

/-- **S12.** A body that fits is at most 1 MiB. -/
def S12_bound : Prop :=
  ∀ (now : Nat) (replies : List slot.Reply), now < 2 ^ 32 → fitsSlot replies →
    (slotBodyBytes now replies).length ≤ slotBodyLimit

/-! ## WoW chat text -/

/-- **S10.** WoW shows `chat_safe` output as exactly the original text, with no escape code. -/
def S10_chat_safe : Prop :=
  ∀ t : Slice U8, t.val.length ≤ 2 ^ 20 →
    wow_text.chat_safe t ⦃ v => wowPlain (bytes v.val) = some (bytes t.val) ⦄

/-! ## Rate limits -/

/-- **S14, one step.** `admit_message` is exactly `admitSpec`. -/
def S14_admit : Prop :=
  ∀ (limiter : rate.RateLimiter) (now : U32), limiter.times.val.length ≤ maxMessages →
    rate.admit_message limiter now ⦃ res =>
      (res.1, res.2.times.val.map (·.val)) = admitSpec (limiter.times.val.map (·.val)) now.val ⦄

/-- **S14, any time window.** For messages in time order, no 60-second window ever
holds more than 10 admitted messages. -/
def S14_window : Prop :=
  ∀ nows : List Nat, nows.Pairwise (· ≤ ·) → ∀ t : Nat,
    ((admittedTimes nows).filter fun a => a ≤ t ∧ t < a + windowSeconds).length ≤ maxMessages

/-- **S14, queue.** A chat queue never holds more than 20 messages. -/
def S14_queue : Prop :=
  ∀ (queue : rate.ChatQueue) (id : U32), queue.ids.val.length ≤ maxQueue →
    rate.enqueue queue id ⦃ r => match r with
      | some q => q.ids.val = queue.ids.val ++ [id] ∧ q.ids.val.length ≤ maxQueue
      | none => queue.ids.val.length = maxQueue ⦄

/-! ## Permission popup -/

/-- **S15, writer.** `popup_text` writes exactly `popupBytes`. -/
def S15_popup : Prop :=
  ∀ command label : Slice U8, command.val.length ≤ 2 ^ 20 → label.val.length ≤ 2 ^ 20 →
    popup.popup_text command label ⦃ v =>
      bytes v.val = popupBytes (bytes command.val) (bytes label.val) ⦄

/-- **S15, no tricks.** Every byte is printable ASCII or the one line break. -/
def S15_printable : Prop :=
  ∀ command label : List Spec.Byte, ∀ b ∈ popupBytes command label, b = ch '\n' ∨ printable b = true

/-- **S15, faithful.** The shown text reads back as the exact raw bytes. -/
def S15_faithful : Prop :=
  ∀ s : List Spec.Byte, unshow (showBytes s) = some s

/-! ## Checks: each proved theorem against its approved statement -/

theorem check_C1 : C1 := fun input h => Protocol.Cell.cells_round_trip input h
theorem check_S11_fresh : S11_fresh := Protocol.Frame.is_fresh_spec
theorem check_S2_S11_check : S2_S11_check := Protocol.Frame.check_frame_spec
theorem check_S6_level : S6_level := Protocol.Policy.effective_level_spec
theorem check_S6_answer : S6_answer := Protocol.Policy.answer_from_game_spec
theorem check_S15_popup : S15_popup := Protocol.Popup.popup_text_spec
theorem check_S15_printable : S15_printable := Protocol.Popup.popup_printable
theorem check_S15_faithful : S15_faithful := Protocol.Popup.showBytes_faithful
theorem check_S8_lua_string : S8_lua_string := Protocol.Lua.lua_string_spec
theorem check_S8_reads_back : S8_reads_back := Protocol.Lua.lua_reads_back
theorem check_S10_chat_safe : S10_chat_safe := Protocol.WowText.chat_safe_spec
theorem check_C2_encode : C2_encode := Protocol.Frame.encode_frame_spec
theorem check_C2_encode_too_long : C2_encode_too_long := Protocol.Frame.encode_frame_too_long
theorem check_C2_decode : C2_decode := Protocol.Frame.decode_frame_complete
theorem check_S1_S2_decode : S1_S2_decode := Protocol.Frame.decode_frame_sound
theorem check_S2_signed_len : S2_signed_len := Protocol.Frame.signed_len_spec
theorem check_S13_valid_id : S13_valid_id := Protocol.Record.is_valid_id_spec
theorem check_S3_S4_parse : S3_S4_parse := Protocol.Record.parse_records_sound
theorem check_C3_serialize : C3_serialize := Protocol.Record.serialize_records_spec

end Protocol.Statements
