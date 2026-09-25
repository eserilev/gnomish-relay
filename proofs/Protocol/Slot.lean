import Protocol.Lua
import Protocol.Seen
import Protocol.Spec.Slot
import Protocol.Apps

/-! # The slot body (S9, S12) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Slot

@[simp, scalar_tac_simps]
theorem max_replies_val : slot.MAX_REPLIES.val = 30 := by unfold slot.MAX_REPLIES; rfl

@[simp, scalar_tac_simps]
theorem max_text_val : slot.MAX_TEXT.val = 32768 := by unfold slot.MAX_TEXT; rfl

@[simp, scalar_tac_simps]
theorem max_id_len_val : record.MAX_ID_LEN.val = 32 := by unfold record.MAX_ID_LEN; rfl

theorem escape_length (b : U8) :
    (luaEscapeByte b.bv).length = if 32 ≤ b.val ∧ b.val ≤ 126 ∧ b.val ≠ 34 ∧ b.val ≠ 92 then 1 else 4 := by
  unfold luaEscapeByte
  by_cases h : 32 ≤ b.val ∧ b.val ≤ 126 ∧ b.val ≠ 34 ∧ b.val ≠ 92
  · rw [if_pos ((Protocol.Lua.plain_iff b).mpr h), if_pos h]; rfl
  · rw [if_neg (fun h' => h ((Protocol.Lua.plain_iff b).mp h')), if_neg h]; rfl

@[step]
theorem escaped_len_spec (b : U8) :
    slot.escaped_len b ⦃ r => r.val = (luaEscapeByte b.bv).length ⦄ := by
  unfold slot.escaped_len
  step*
  all_goals rw [escape_length]; simp_all

theorem literal_length (s : List Spec.Byte) :
    (luaLiteral s).length = 2 + (s.flatMap luaEscapeByte).length := by
  simp [luaLiteral]; omega

theorem literal_take_succ (text : List U8) (i : Nat) (h : i < text.length) :
    (luaLiteral (bytes (text.take (i + 1)))).length =
      (luaLiteral (bytes (text.take i))).length + (luaEscapeByte text[i].bv).length := by
  rw [literal_length, literal_length, List.take_add_one, List.getElem?_eq_getElem h]
  simp only [bytes, List.map_append, List.flatMap_append, List.length_append, Option.toList_some,
    List.map_cons, List.map_nil, List.flatMap_cons, List.flatMap_nil, List.append_nil]
  omega

@[step]
theorem next_fits_spec (text : Slice U8) (i size : Usize) (hsize : size.val ≤ 32768) :
    slot.next_fits text i size ⦃ r => (r = true ↔
      ∃ h : i.val < text.val.length, size.val + (luaEscapeByte (text.val[i.val]'h).bv).length ≤ 32768) ⦄ := by
  unfold slot.next_fits
  step*
  · have := Protocol.Lua.luaEscapeByte_length_le i2.bv
    scalar_tac
  · have hlt : i.val < text.val.length := by scalar_tac
    subst i2_post
    have e : (luaEscapeByte (text.val[i.val]'hlt).bv).length = i3.val := i3_post.symm
    have hmax : slot.MAX_TEXT.val = 32768 := max_text_val
    rw [decide_eq_true_iff]
    constructor
    · intro h
      have h' : i4.val ≤ slot.MAX_TEXT.val := h
      exact ⟨hlt, by omega⟩
    · rintro ⟨_, h⟩
      show i4.val ≤ slot.MAX_TEXT.val
      omega

def FitInv (text : Slice U8) (st : Usize × Usize) : Prop :=
  st.2.val ≤ text.val.length ∧ st.1.val ≤ 32768 ∧
    st.1.val = (luaLiteral (bytes (text.val.take st.2.val))).length

@[step]
theorem fitting_prefix_spec (text : Slice U8) :
    slot.fitting_prefix text ⦃ n =>
      n.val ≤ text.val.length ∧ (luaLiteral (bytes (text.val.take n.val))).length ≤ maxText ⦄ := by
  unfold slot.fitting_prefix slot.fitting_prefix_loop
  apply loop.spec_decr_nat (fun st => text.val.length - st.2.val) (FitInv text) _ _ _ _
    ⟨by simp, by simp, by simp [luaLiteral, bytes]⟩
  rintro ⟨size, i⟩ ⟨hi, hs, hsize⟩
  simp only at hi hs hsize
  unfold slot.fitting_prefix_loop.body
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  step*
  · have := Protocol.Lua.luaEscapeByte_length_le i1.bv
    omega
  · have hb : b = true := by assumption
    obtain ⟨hlt, hle⟩ := b_post.mp hb
    subst i1_post
    have e : (luaEscapeByte (text.val[i.val]'hlt).bv).length = i2.val := i2_post.symm
    have hs := literal_take_succ text.val i.val hlt
    unfold FitInv
    dsimp only
    refine ⟨⟨by omega, by omega, ?_⟩, by omega⟩
    rw [i3_post, hs]
    omega
  · rw [← hsize]
    exact ⟨hi, hs⟩

@[step]
theorem min_len_spec (n max : Usize) : slot.min_len n max ⦃ r => r.val = min n.val max.val ⦄ := by
  unfold slot.min_len
  step*

@[step]
theorem first_kept_spec (n : Usize) : slot.first_kept n ⦃ r => r.val = n.val - 30 ⦄ := by
  unfold slot.first_kept
  step*
  scalar_tac

theorem take_min_bytes (l : List U8) : bytes (l.take (min l.length 32)) = (bytes l).take 32 := by
  rcases le_total l.length 32 with h | h
  · rw [min_eq_left h, List.take_length, List.take_of_length_le (by simp [bytes, h])]
  · rw [min_eq_right h, bytes, bytes, List.map_take]

def fitsReply (r : slot.Reply) : Prop :=
  r.chat.val.length ≤ 32 ∧ (luaLiteral (bytes r.text.val)).length ≤ maxText

@[step]
theorem prepare_reply_spec (reply : slot.Reply) :
    slot.prepare_reply reply ⦃ r => preparedFrom reply r ∧ fitsReply r ⦄ := by
  unfold slot.prepare_reply
  step*
  · simp only [Protocol.Seen.deref_val]; scalar_tac
  · simp only [Protocol.Seen.deref_val, List.nil_append, Nat.sub_zero, List.drop_zero] at *
    have hc : chat.val = reply.chat.val.take (min reply.chat.val.length 32) := by
      rw [chat_post, i1_post]; simp
    have ht : text.val = reply.text.val.take i2.val := text_post
    refine ⟨⟨rfl, rfl, (congrArg bytes hc).trans (take_min_bytes _), ?_⟩, ?_, ?_⟩
    · show (text.val.map (·.bv)) <+: reply.text.val.map (·.bv)
      rw [ht]
      exact (List.take_prefix _ _).map _
    · exact (congrArg List.length hc).trans_le (by simp)
    · show (luaLiteral (bytes text.val)).length ≤ maxText
      rw [ht]
      exact i2_post2

def PrepInv (replies : Slice slot.Reply) (s : Nat) (st : alloc.vec.Vec slot.Reply × Usize) : Prop :=
  s ≤ st.2.val ∧ st.2.val ≤ max s replies.val.length ∧ st.1.val.length = st.2.val - s ∧
    List.Forall₂ preparedFrom ((replies.val.drop s).take (st.2.val - s)) st.1.val ∧
    ∀ r ∈ st.1.val, fitsReply r

/-- **S12, prepare.** -/
theorem prepare_replies_spec (replies : Slice slot.Reply) :
    slot.prepare_replies replies ⦃ ps =>
      fitsSlot ps.val ∧
      List.Forall₂ preparedFrom (replies.val.drop (replies.val.length - maxReplies)) ps.val ⦄ := by
  unfold slot.prepare_replies
  step*
  unfold slot.prepare_replies_loop
  apply loop.spec_decr_nat (fun st => replies.val.length - st.2.val) (PrepInv replies i1.val) _ _ _ _
    ⟨le_refl _, le_max_left _ _, by simp, by simp, by simp⟩
  rintro ⟨out, i⟩ ⟨hs, hi, hlen, hf, hfit⟩
  simp only at hs hi hlen hf hfit
  unfold slot.prepare_replies_loop.body
  step*
  · have hlt : i.val < replies.val.length := by scalar_tac
    have hs30 : i1.val ≤ replies.val.length := by scalar_tac
    unfold PrepInv
    dsimp only
    have htake : (replies.val.drop i1.val).take (i2.val - i1.val) =
        (replies.val.drop i1.val).take (i.val - i1.val) ++ [replies.val[i.val]] := by
      rw [i2_post, show i.val + 1 - i1.val = (i.val - i1.val) + 1 by omega, List.take_add_one,
        List.getElem?_drop, show i1.val + (i.val - i1.val) = i.val by omega,
        List.getElem?_eq_getElem hlt]
      rfl
    refine ⟨⟨by omega, by omega, by simp [out1_post, hlen]; omega, ?_, ?_⟩, by omega⟩
    · rw [htake, out1_post]
      exact List.rel_append hf (List.Forall₂.cons (r_post ▸ r1_post1) .nil)
    · intro x hx
      rw [out1_post, List.mem_append, List.mem_singleton] at hx
      rcases hx with hx | rfl
      · exact hfit x hx
      · exact r1_post2
  · have hs30 : i1.val = replies.val.length - maxReplies := by simp [maxReplies]; scalar_tac
    have hn : i.val = replies.val.length := by scalar_tac
    rw [hn, List.take_of_length_le (by simp)] at hf
    refine ⟨⟨?_, hfit⟩, by rw [← hs30]; exact hf⟩
    rw [hlen, hn, hs30, maxReplies]
    omega

/-! ## Size bound (pure) -/

theorem flatMap_length_le {α β : Type} (f : α → List β) (k : Nat) (hf : ∀ a, (f a).length ≤ k) :
    ∀ l : List α, (l.flatMap f).length ≤ k * l.length
  | [] => by simp
  | a :: l => by
    have := hf a
    have := flatMap_length_le f k hf l
    simp only [List.flatMap_cons, List.length_append, List.length_cons]
    rw [Nat.mul_succ]
    omega

theorem literal_length_le (s : List Spec.Byte) : (luaLiteral s).length ≤ 2 + 4 * s.length := by
  rw [literal_length]
  have := flatMap_length_le luaEscapeByte 4 Protocol.Lua.luaEscapeByte_length_le s
  omega

theorem status_literal_length (st : slot.Status) : (luaLiteral (ascii (statusWord st))).length ≤ 9 := by
  cases st <;> decide

theorem replyLine_length (r : slot.Reply) (h : fitsReply r) : (replyLine r).length ≤ 32955 := by
  obtain ⟨hchat, htext⟩ := h
  have hc := literal_length_le (bytes r.chat.val)
  have hd := decimal_u32_length r.id
  have hs := status_literal_length r.status
  have hb : (bytes r.chat.val).length = r.chat.val.length := by simp [bytes]
  simp only [maxText] at htext
  simp only [replyLine, List.length_append]
  have : (ascii "{chat = ").length = 8 := rfl
  have : (ascii ", id = ").length = 7 := rfl
  have : (ascii ", status = ").length = 11 := rfl
  have : (ascii ", text = ").length = 9 := rfl
  have : (ascii "},\n").length = 3 := rfl
  omega

/-- The bound of S12 holds for the body of every app. -/
theorem slot_body_of_bound (app : apps.App) (now : Nat) (replies : List slot.Reply)
    (hnow : now < 2 ^ 32) (hfits : fitsSlot replies) :
    (slotBodyOf app now replies).length ≤ slotBodyLimit := by
  have hg := Protocol.Apps.slot_global_length app
  obtain ⟨hlen, hall⟩ := hfits
  have hd : (decimal now).length ≤ 10 := decimal_length_le now 10 (by omega) (by omega)
  have hlines : (replies.flatMap replyLine).length ≤ 32955 * replies.length := by
    rw [List.length_flatMap]
    have : ∀ x ∈ replies.map (fun r => (replyLine r).length), x ≤ 32955 := by
      intro x hx
      obtain ⟨r, hr, rfl⟩ := List.mem_map.mp hx
      exact replyLine_length r (hall r hr)
    have := List.sum_le_card_nsmul _ _ this
    simpa [mul_comm] using this
  simp only [slotBodyOf, List.length_append]
  have : (ascii " = {proto = 1, now = ").length = 21 := rfl
  have : (ascii ", replies = {\n").length = 14 := rfl
  have : (ascii "}}\n").length = 3 := rfl
  simp only [maxReplies] at hlen
  simp only [slotBodyLimit]
  omega

/-- **S12, bound.** -/
theorem slot_body_bound (now : Nat) (replies : List slot.Reply) (hnow : now < 2 ^ 32)
    (hfits : fitsSlot replies) : (slotBodyBytes now replies).length ≤ slotBodyLimit :=
  slot_body_of_bound .Relay now replies hnow hfits

/-! ## The template (S9) -/

theorem head_bytes : bytes (Array.to_slice slot.HEAD).val = ascii " = {proto = 1, now = " := by
  unfold slot.HEAD; rfl

@[simp, scalar_tac_simps]
theorem head_length : (Array.to_slice slot.HEAD).val.length = 21 := by unfold slot.HEAD; rfl

theorem replies_bytes : bytes (Array.to_slice slot.REPLIES).val = ascii ", replies = {\n" := by
  unfold slot.REPLIES; rfl

@[simp, scalar_tac_simps]
theorem replies_length : (Array.to_slice slot.REPLIES).val.length = 14 := by unfold slot.REPLIES; rfl

theorem tail_bytes : bytes (Array.to_slice slot.TAIL).val = ascii "}}\n" := by
  unfold slot.TAIL; rfl

@[simp, scalar_tac_simps]
theorem tail_length : (Array.to_slice slot.TAIL).val.length = 3 := by unfold slot.TAIL; rfl

theorem chat_bytes : bytes (Array.to_slice slot.CHAT).val = ascii "{chat = " := by
  unfold slot.CHAT; rfl

@[simp, scalar_tac_simps]
theorem chat_length : (Array.to_slice slot.CHAT).val.length = 8 := by unfold slot.CHAT; rfl

theorem id_bytes : bytes (Array.to_slice slot.ID).val = ascii ", id = " := by
  unfold slot.ID; rfl

@[simp, scalar_tac_simps]
theorem id_length : (Array.to_slice slot.ID).val.length = 7 := by unfold slot.ID; rfl

theorem status_bytes : bytes (Array.to_slice slot.STATUS).val = ascii ", status = " := by
  unfold slot.STATUS; rfl

@[simp, scalar_tac_simps]
theorem status_length : (Array.to_slice slot.STATUS).val.length = 11 := by unfold slot.STATUS; rfl

theorem text_bytes : bytes (Array.to_slice slot.TEXT).val = ascii ", text = " := by
  unfold slot.TEXT; rfl

@[simp, scalar_tac_simps]
theorem text_length : (Array.to_slice slot.TEXT).val.length = 9 := by unfold slot.TEXT; rfl

theorem reply_end_bytes : bytes (Array.to_slice slot.REPLY_END).val = ascii "},\n" := by
  unfold slot.REPLY_END; rfl

@[simp, scalar_tac_simps]
theorem reply_end_length : (Array.to_slice slot.REPLY_END).val.length = 3 := by unfold slot.REPLY_END; rfl

theorem working_bytes : bytes (Array.to_slice slot.WORKING).val = luaLiteral (ascii "working") := by
  unfold slot.WORKING; decide

@[simp, scalar_tac_simps]
theorem working_length : (Array.to_slice slot.WORKING).val.length = 9 := by unfold slot.WORKING; rfl

theorem done_bytes : bytes (Array.to_slice slot.DONE).val = luaLiteral (ascii "done") := by
  unfold slot.DONE; decide

@[simp, scalar_tac_simps]
theorem done_length : (Array.to_slice slot.DONE).val.length = 6 := by unfold slot.DONE; rfl

theorem error_bytes : bytes (Array.to_slice slot.ERROR).val = luaLiteral (ascii "error") := by
  unfold slot.ERROR; decide

@[simp, scalar_tac_simps]
theorem error_length : (Array.to_slice slot.ERROR).val.length = 7 := by unfold slot.ERROR; rfl

@[step]
theorem push_status_spec (out : alloc.vec.Vec U8) (st : slot.Status) (hroom : out.val.length + 9 ≤ Usize.max) :
    slot.push_status out st ⦃ r =>
      bytes r.val = bytes out.val ++ luaLiteral (ascii (statusWord st)) ∧ r.val.length ≤ out.val.length + 9 ⦄ := by
  unfold slot.push_status
  induction st
  all_goals
    step*
    subst s_post
    refine ⟨?_, by simp at r_post2; omega⟩
    rw [r_post1, bytes, List.map_append]
    congr 1
  · exact working_bytes
  · exact done_bytes
  · exact error_bytes

theorem length_le_flatMap (s : List Spec.Byte) : s.length ≤ (s.flatMap luaEscapeByte).length := by
  induction s with
  | nil => simp
  | cons b s ih =>
    have : 1 ≤ (luaEscapeByte b).length := by unfold luaEscapeByte; split <;> simp
    simp only [List.flatMap_cons, List.length_append, List.length_cons]
    omega

theorem text_length_le (r : slot.Reply) (h : fitsReply r) : r.text.val.length ≤ 32766 := by
  have h1 := h.2
  have h2 := length_le_flatMap (bytes r.text.val)
  rw [literal_length] at h1
  simp only [bytes, List.length_map, maxText] at h1 h2 ⊢
  omega

@[step]
theorem lua_string_len_spec (s : Slice U8) (h : s.val.length ≤ 2 ^ 20) :
    lua.lua_string s ⦃ v =>
      bytes v.val = luaLiteral (bytes s.val) ∧ v.val.length ≤ 2 + 4 * s.val.length ⦄ := by
  apply WP.spec_mono (Protocol.Lua.lua_string_spec s h)
  intro v hv
  refine ⟨hv, ?_⟩
  have h1 := congrArg List.length hv
  have h2 := literal_length_le (bytes s.val)
  simp only [bytes, List.length_map] at h1 h2
  omega

@[step]
theorem push_reply_spec (out : alloc.vec.Vec U8) (reply : slot.Reply)
    (hroom : out.val.length ≤ 2 ^ 31) (hfit : fitsReply reply) :
    slot.push_reply out reply ⦃ r =>
      bytes r.val = bytes out.val ++ replyLine reply ∧ r.val.length ≤ out.val.length + 32955 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hchat := hfit.1
  have htext := text_length_le reply hfit
  unfold slot.push_reply
  step*
  all_goals try (subst_vars; simp only [Protocol.Seen.deref_val, chat_length, id_length, status_length,
    text_length, reply_end_length] at *; scalar_tac)
  have happ : ∀ a b : List U8, bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]
  have hderef : ∀ w : alloc.vec.Vec U8, bytes (alloc.vec.Vec.deref w).val = bytes w.val := fun _ => rfl
  subst s_post s3_post s4_post s5_post s8_post
  have hb : bytes r.val = bytes out.val ++ replyLine reply := by
    rw [r_post1, happ, out8_post1, happ, out7_post1, happ, out6_post1, out5_post1, happ, out4_post1,
      out3_post1, happ, out2_post1, happ, out1_post1, happ, hderef, hderef, v_post1, v1_post1,
      chat_bytes, id_bytes, status_bytes, text_bytes, reply_end_bytes]
    simp only [replyLine, List.append_assoc]
    rfl
  refine ⟨hb, ?_⟩
  have h1 := congrArg List.length hb
  have h2 := replyLine_length reply hfit
  simp only [bytes, List.length_map, List.length_append] at h1
  omega

def BodyInv (replies : Slice slot.Reply) (pre : List Spec.Byte) (st : alloc.vec.Vec U8 × Usize) : Prop :=
  st.2.val ≤ replies.val.length ∧
    bytes st.1.val = pre ++ (replies.val.take st.2.val).flatMap replyLine ∧
    st.1.val.length ≤ 100 + 32955 * st.2.val

theorem slot_body_loop_spec (replies : Slice slot.Reply) (pre : List Spec.Byte)
    (out : alloc.vec.Vec U8) (hfits : fitsSlot replies.val) (hinv : BodyInv replies pre (out, 0#usize)) :
    slot.slot_body_loop replies out 0#usize ⦃ r =>
      bytes r.val = pre ++ replies.val.flatMap replyLine ∧ r.val.length ≤ 100 + 32955 * 30 ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  obtain ⟨hn, hall⟩ := hfits
  simp only [maxReplies] at hn
  unfold slot.slot_body_loop
  apply loop.spec_decr_nat (fun st => replies.val.length - st.2.val) (BodyInv replies pre) _ _ _ _ hinv
  rintro ⟨out, i⟩ ⟨hi, hout, hlen⟩
  simp only at hi hout hlen
  unfold slot.slot_body_loop.body
  step*
  · rw [r_post]; exact hall _ (List.getElem_mem _)
  · have hlt : i.val < replies.val.length := by scalar_tac
    unfold BodyInv
    dsimp only
    refine ⟨⟨by omega, ?_, by omega⟩, by omega⟩
    rw [out1_post1, hout, i2_post, List.take_add_one, List.getElem?_eq_getElem hlt, List.flatMap_append,
      ← r_post]
    simp
  · have : i.val = replies.val.length := by scalar_tac
    rw [this, List.take_length] at hout
    exact ⟨hout, by omega⟩

/-- **S9.** -/
theorem slot_body_spec (app : apps.App) (now : U32) (replies : Slice slot.Reply)
    (hfits : fitsSlot replies.val) :
    slot.slot_body app now replies ⦃ v => bytes v.val = slotBodyOf app now.val replies.val ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have happ : ∀ a b : List U8, bytes (a ++ b) = bytes a ++ bytes b := by simp [bytes]
  have hg := Protocol.Apps.slot_global_length app
  unfold slot.slot_body
  step*
  subst s_post s1_post
  have hpre : bytes out3.val = ascii (slotGlobal app) ++ ascii " = {proto = 1, now = " ++
      decimal now.val ++ ascii ", replies = {\n" := by
    rw [out3_post1, happ, out2_post1, out1_post1, happ, out_post1, head_bytes, replies_bytes]
    simp [bytes]
  have hlen : out3.val.length ≤ 100 := by
    have := decimal_u32_length now
    have h := congrArg List.length hpre
    simp only [bytes, List.length_map, List.length_append] at h
    have : (ascii " = {proto = 1, now = ").length = 21 := rfl
    have : (ascii ", replies = {\n").length = 14 := rfl
    omega
  step with slot_body_loop_spec replies (ascii (slotGlobal app) ++ ascii " = {proto = 1, now = " ++
      decimal now.val ++ ascii ", replies = {\n") out3 hfits ⟨by simp, by simp [hpre], by simp [hlen]⟩
    as ⟨out4, h4, h4len⟩
  step*
  subst s2_post
  rw [v_post1, happ, h4, tail_bytes]
  simp [slotBodyOf]

end Protocol.Slot
