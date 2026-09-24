import Protocol.Ascii
import Protocol.Spec.Popup

/-! # The permission popup (S15) -/

open Aeneas Aeneas.Std Result protocol Protocol.Spec Protocol.Ascii

namespace Protocol.Popup

@[simp, scalar_tac_simps, grind =, agrind =]
theorem command_budget_val : popup.COMMAND_BUDGET.val = 300 := by unfold popup.COMMAND_BUDGET; rfl

@[simp, scalar_tac_simps, grind =, agrind =]
theorem label_budget_val : popup.LABEL_BUDGET.val = 120 := by unfold popup.LABEL_BUDGET; rfl

theorem cut_start_bytes : bytes (Array.to_slice popup.CUT_START).val = ascii " [... " := by
  unfold popup.CUT_START; rfl

theorem cut_end_bytes : bytes (Array.to_slice popup.CUT_END).val = ascii " bytes cut ...] " := by
  unfold popup.CUT_END; rfl

theorem agent_says_bytes :
    bytes (Array.to_slice popup.AGENT_SAYS).val = ascii "\nthe agent says: " := by
  unfold popup.AGENT_SAYS; rfl

@[simp, scalar_tac_simps]
theorem cut_start_length : (Array.to_slice popup.CUT_START).val.length = 6 := by
  unfold popup.CUT_START; rfl

@[simp, scalar_tac_simps]
theorem cut_end_length : (Array.to_slice popup.CUT_END).val.length = 16 := by
  unfold popup.CUT_END; rfl

@[simp, scalar_tac_simps]
theorem agent_says_length : (Array.to_slice popup.AGENT_SAYS).val.length = 17 := by
  unfold popup.AGENT_SAYS; rfl

@[simp, scalar_tac_simps]
theorem label_cut_length : (Array.to_slice popup.LABEL_CUT).val.length = 6 := by
  unfold popup.LABEL_CUT; rfl

theorem label_cut_bytes : bytes (Array.to_slice popup.LABEL_CUT).val = ascii " [...]" := by
  unfold popup.LABEL_CUT; rfl

@[step]
theorem clamp_to_u32_spec (n : Usize) (h : n.val ≤ U32.max) :
    popup.clamp_to_u32 n ⦃ r => r.val = n.val ⦄ := by
  unfold popup.clamp_to_u32
  step*
  -- The "too large" branch cannot happen under the precondition.
  exfalso
  rename_i hgt
  have : i.val = U32.max := by
    rw [i_post]
    simp only [UScalar.cast_val_eq]
    rcases System.Platform.numBits_eq with hb | hb <;> simp [hb, U32.rMax, U32.max, U32.numBits]
  scalar_tac

@[step]
theorem hex_digit_spec (n : U8) (h : n.val < 16) :
    popup.hex_digit n ⦃ r => r.bv = hexDigit n.val ⦄ := by
  unfold popup.hex_digit
  step*
  all_goals
    rw [U8_bv_eq_ofNat]
    apply BitVec.eq_of_toNat_eq
  · simp [hexDigit, ch, r_post, show n.val < 10 by scalar_tac]
  · simp [hexDigit, ch, r_post, i_post1, show ¬ n.val < 10 by omega]

theorem printable_iff (b : U8) : printable b.bv = true ↔ 32 ≤ b.val ∧ b.val ≤ 126 := by
  simp [printable, ch, BitVec.le_def]

theorem is_backslash_iff (b : U8) : b.bv = ch '\\' ↔ b.val = 92 := by
  constructor
  · intro h; have := congrArg BitVec.toNat h; simpa [ch] using this
  · intro h; apply BitVec.eq_of_toNat_eq; simp [ch, h]

@[step]
theorem push_shown_spec (out : alloc.vec.Vec U8) (b : U8) (hroom : out.val.length + 4 ≤ Usize.max) :
    popup.push_shown out b ⦃ r => bytes r.val = bytes out.val ++ showByte b.bv ⦄ := by
  unfold popup.push_shown
  step*
  all_goals try (simp only [*, List.length_append, List.length_cons, List.length_nil]; omega)
  · -- A backslash shows doubled.
    simp [bytes, r_post, out1_post, showByte]
    decide
  · -- Printable ASCII shows as is.
    have hb : ¬ b.bv = ch '\\' := by rw [is_backslash_iff]; scalar_tac
    have hp : printable b.bv = true := by rw [printable_iff]; scalar_tac
    simp [bytes, r_post, showByte, hb, hp]
  all_goals
    -- Anything else shows as \xHH.
    have hb : ¬ b.bv = ch '\\' := by rw [is_backslash_iff]; scalar_tac
    have hp : ¬ printable b.bv = true := by rw [printable_iff]; scalar_tac
    simp [bytes, r_post, out3_post, out2_post, out1_post, showByte, hb, hp, i1_post, i3_post,
      i_post, i2_post]
    decide

end Protocol.Popup

namespace Protocol.Popup

theorem showByte_length_le (b : Spec.Byte) : (showByte b).length ≤ 4 := by
  unfold showByte; split <;> [simp; split <;> simp]

theorem showBytes_length_le (l : List Spec.Byte) : (showBytes l).length ≤ 4 * l.length := by
  induction l with
  | nil => simp [showBytes]
  | cons b rest ih =>
    simp only [showBytes, List.flatMap_cons, List.length_append, List.length_cons] at ih ⊢
    have := showByte_length_le b
    omega

theorem take_drop_succ {α : Type} (l : List α) (s i : Nat) (hs : s ≤ i) (hi : i < l.length) :
    (l.drop s).take (i + 1 - s) = (l.drop s).take (i - s) ++ [l[i]] := by
  rw [show i + 1 - s = (i - s) + 1 by omega, List.take_add_one,
    List.getElem?_eq_getElem (by simp; omega)]
  simp [Nat.add_sub_cancel' hs]

def RangeInv (out0 : alloc.vec.Vec U8) (src : Slice U8) (start : Nat)
    (st : alloc.vec.Vec U8 × Usize) : Prop :=
  start ≤ st.2.val ∧
  bytes st.1.val = bytes out0.val ++ showBytes (bytes ((src.val.drop start).take (st.2.val - start)))

theorem push_shown_range_loop_spec (out0 : alloc.vec.Vec U8) (src : Slice U8) (start stop : Usize)
    (out : alloc.vec.Vec U8) (i : Usize)
    (hle : start.val ≤ stop.val) (hstop : stop.val ≤ src.val.length)
    (hroom : out0.val.length + 4 * (stop.val - start.val) ≤ Usize.max)
    (hi : i.val ≤ stop.val) (hinv : RangeInv out0 src start.val (out, i)) :
    popup.push_shown_range_loop out src stop i ⦃ r =>
      bytes r.val = bytes out0.val ++ showBytes (bytes ((src.val.drop start.val).take (stop.val - start.val))) ⦄ := by
  unfold popup.push_shown_range_loop
  apply loop.spec_decr_nat (fun st => stop.val - st.2.val)
    (fun st => st.2.val ≤ stop.val ∧ RangeInv out0 src start.val st) _ _ _ _ ⟨hi, hinv⟩
  rintro ⟨out, i⟩ ⟨hi, hs, hout⟩
  simp only at hi hs hout
  have hlen : out.val.length ≤ out0.val.length + 4 * (i.val - start.val) := by
    have h1 := congrArg List.length hout
    have h2 := showBytes_length_le (bytes ((src.val.drop start.val).take (i.val - start.val)))
    simp only [bytes, List.length_map, List.length_append, List.length_take, List.length_drop] at h1 h2
    omega
  unfold popup.push_shown_range_loop.body
  step*
  · refine ⟨by scalar_tac, ⟨by scalar_tac, ?_⟩, by scalar_tac⟩
    simp only
    rw [out1_post, hout, i2_post, take_drop_succ _ _ _ hs (by scalar_tac)]
    simp [bytes, showBytes, i1_post]

@[step]
theorem push_shown_range_spec (out : alloc.vec.Vec U8) (src : Slice U8) (start stop : Usize)
    (hle : start.val ≤ stop.val) (hstop : stop.val ≤ src.val.length)
    (hroom : out.val.length + 4 * (stop.val - start.val) ≤ Usize.max) :
    popup.push_shown_range out src start stop ⦃ r =>
      bytes r.val = bytes out.val ++ showBytes (bytes ((src.val.drop start.val).take (stop.val - start.val))) ∧
      r.val.length ≤ out.val.length + 4 * (stop.val - start.val) ⦄ := by
  unfold popup.push_shown_range
  apply WP.spec_mono (push_shown_range_loop_spec out src start stop out start hle hstop hroom hle
    (by simp [RangeInv, showBytes, bytes]))
  intro r hr
  refine ⟨hr, ?_⟩
  have h1 := congrArg List.length hr
  have h2 := showBytes_length_le (bytes ((src.val.drop start.val).take (stop.val - start.val)))
  simp only [bytes, List.length_map, List.length_append, List.length_take, List.length_drop] at h1 h2
  omega

end Protocol.Popup

namespace Protocol.Popup

theorem len_of_bytes {a b : List U8} {x : List Spec.Byte} (h : bytes a = bytes b ++ x) :
    a.length = b.length + x.length := by
  have := congrArg List.length h
  simpa [bytes] using this

theorem bytes_append (a b : List U8) : bytes (a ++ b) = bytes a ++ bytes b := by
  simp [bytes]

@[step]
theorem push_command_spec (out : alloc.vec.Vec U8) (command : Slice U8)
    (hc : command.val.length ≤ 2 ^ 20) (hroom : out.val.length + 1300 ≤ Usize.max) :
    popup.push_command out command ⦃ r =>
      bytes r.val = bytes out.val ++ commandPart (bytes command.val) ⦄ := by
  unfold popup.push_command
  dsimp only
  split
  · step*
    have hn : command.val.length ≤ 300 := by scalar_tac
    rw [r_post1]
    have hn' : (bytes command.val).length ≤ 300 := by simpa [bytes] using hn
    simp [commandPart, commandBudget, hn']
  · step*
    · rw [s_post] at out2_post2
      rw [s1_post] at out4_post2
      simp only [cut_start_length, cut_end_length] at out2_post2 out4_post2
      scalar_tac
    · have hn : 300 < command.val.length := by scalar_tac
      have hhalf : half.val = 150 := by scalar_tac
      have hi2 : i2.val = command.val.length - 150 := by scalar_tac
      have hi1 : i1.val = command.val.length - 300 := by scalar_tac
      rw [r_post1, out4_post1, bytes_append, out3_post1, out2_post1, bytes_append, out1_post1,
        s_post, s1_post, cut_start_bytes, cut_end_bytes, hhalf, hi2, hi1]
      simp only [commandPart, commandBudget, cutMarker, show ¬ command.val.length ≤ 300 by omega,
        if_false, List.drop_zero, List.append_assoc, bytes, List.map_take, List.map_drop,
        List.length_map]
      have hlast : ((command.val.map (·.bv)).drop (command.val.length - 150)).take 150 =
          (command.val.map (·.bv)).drop (command.val.length - 150) :=
        List.take_of_length_le (by simp; omega)
      rw [show command.len.val - (command.val.length - 150) = 150 by scalar_tac, hlast]

theorem commandPart_length_le (c : List Spec.Byte) (h : c.length ≤ 2 ^ 20) :
    (commandPart c).length ≤ 1300 := by
  unfold commandPart commandBudget cutMarker
  split
  · have := showBytes_length_le c; omega
  · have h1 := showBytes_length_le (c.take 150)
    have h2 := showBytes_length_le (c.drop (c.length - 150))
    have h3 := decimal_length_le (c.length - 300) 7 (by omega) (by omega)
    simp only [List.length_append, List.length_take, List.length_drop, ascii] at *
    simp at *
    omega

theorem labelPart_length_le (l : List Spec.Byte) : (labelPart l).length ≤ 500 := by
  unfold labelPart labelBudget
  split
  · have := showBytes_length_le l; omega
  · have := showBytes_length_le (l.take 120)
    simp [ascii] at *
    omega

@[step]
theorem push_label_spec (out : alloc.vec.Vec U8) (label : Slice U8)
    (hroom : out.val.length + 500 ≤ Usize.max) :
    popup.push_label out label ⦃ r => bytes r.val = bytes out.val ++ labelPart (bytes label.val) ⦄ := by
  unfold popup.push_label
  dsimp only
  split
  · step*
    have hn' : (bytes label.val).length ≤ 120 := by simp [bytes]; scalar_tac
    rw [r_post1]
    simp [labelPart, labelBudget, hn']
  · step*
    have hn : ¬ label.val.length ≤ 120 := by scalar_tac
    rw [r_post1, bytes_append, out1_post1, s_post, label_cut_bytes]
    simp [labelPart, labelBudget, hn, bytes, List.map_take]

/-- **S15, writer.** -/
theorem popup_text_spec (command label : Slice U8) (hc : command.val.length ≤ 2 ^ 20)
    (_hl : label.val.length ≤ 2 ^ 20) :
    popup.popup_text command label ⦃ v =>
      bytes v.val = popupBytes (bytes command.val) (bytes label.val) ⦄ := by
  have husize : 2 ^ 32 - 1 ≤ Usize.max := by scalar_tac
  have hcmd := commandPart_length_le (bytes command.val) (by simpa [bytes] using hc)
  unfold popup.popup_text
  step*
  · have := len_of_bytes out_post; rw [s_post]; simp at this ⊢; omega
  · have h1 := len_of_bytes out_post
    have h2 : out1.val.length = out.val.length + 17 := by rw [out1_post2, s_post]; simp
    simp at h1
    omega
  · rw [v_post, out1_post1, bytes_append, out_post, s_post, agent_says_bytes]
    simp [popupBytes, bytes]

/-! ## What the popup shows -/

theorem hexDigit_printable (n : Nat) (h : n < 16) : printable (hexDigit n) = true := by
  interval_cases n <;> decide

theorem showByte_printable (b : Spec.Byte) : ∀ x ∈ showByte b, printable x = true := by
  unfold showByte
  split
  · simp; decide
  · split
    · simp_all
    · simp only [List.mem_cons, List.not_mem_nil, or_false]
      rintro x (rfl | rfl | rfl | rfl)
      · decide
      · decide
      · exact hexDigit_printable _ (by have := b.isLt; omega)
      · exact hexDigit_printable _ (Nat.mod_lt _ (by omega))

theorem showBytes_printable (l : List Spec.Byte) : ∀ x ∈ showBytes l, printable x = true := by
  intro x hx
  simp only [showBytes, List.mem_flatMap] at hx
  obtain ⟨b, -, hb⟩ := hx
  exact showByte_printable b x hb

theorem decimal_printable (n : Nat) : ∀ x ∈ decimal n, printable x = true := by
  induction n using Nat.strong_induction_on with
  | _ n ih =>
    rw [decimal_eq]
    intro x hx
    simp only [List.mem_append, List.mem_singleton] at hx
    rcases hx with hx | rfl
    · split at hx
      · simp at hx
      · exact ih (n / 10) (by omega) x hx
    · have : n % 10 < 10 := Nat.mod_lt _ (by omega)
      generalize n % 10 = d at this
      interval_cases d <;> decide

theorem commandPart_printable (c : List Spec.Byte) : ∀ x ∈ commandPart c, printable x = true := by
  unfold commandPart cutMarker
  split
  · exact showBytes_printable _
  · intro x hx
    simp only [List.mem_append, or_assoc] at hx
    rcases hx with h | h | h | h | h
    · exact showBytes_printable _ x h
    · revert x; decide
    · exact decimal_printable _ x h
    · revert x; decide
    · exact showBytes_printable _ x h

theorem labelPart_printable (l : List Spec.Byte) : ∀ x ∈ labelPart l, printable x = true := by
  unfold labelPart
  split
  · exact showBytes_printable _
  · intro x hx
    simp only [List.mem_append] at hx
    rcases hx with h | h
    · exact showBytes_printable _ x h
    · revert x; decide

/-- **S15, no tricks.** -/
theorem popup_printable (command label : List Spec.Byte) :
    ∀ b ∈ popupBytes command label, b = ch '\n' ∨ printable b = true := by
  intro b hb
  simp only [popupBytes, List.mem_append, or_assoc] at hb
  rcases hb with h | h | h
  · exact Or.inr (commandPart_printable _ b h)
  · revert b; decide
  · exact Or.inr (labelPart_printable _ b h)

/-! ## The popup reads back as the raw bytes -/

theorem unshow_backslash (rest : List Spec.Byte) :
    unshow (ch '\\' :: ch '\\' :: rest) = (ch '\\' :: ·) <$> unshow rest := by
  rw [unshow.eq_def]
  simp

theorem unshow_plain (b : Spec.Byte) (rest : List Spec.Byte) (hb : ¬ b = ch '\\')
    (hp : printable b = true) : unshow (b :: rest) = (b :: ·) <$> unshow rest := by
  rw [unshow.eq_def]
  simp [hb, hp]

theorem hexDigit_value (n : Nat) (h : n < 16) :
    (if ch '0' ≤ hexDigit n ∧ hexDigit n ≤ ch '9' then some ((hexDigit n).toNat - 48)
     else if ch 'a' ≤ hexDigit n ∧ hexDigit n ≤ ch 'f' then some ((hexDigit n).toNat - 87)
     else none) = some n := by
  interval_cases n <;> decide

theorem unshow_hex (b : Spec.Byte) (rest : List Spec.Byte) :
    unshow (ch '\\' :: ch 'x' :: hexDigit (b.toNat / 16) :: hexDigit (b.toNat % 16) :: rest) =
      (b :: ·) <$> unshow rest := by
  rw [unshow.eq_def]
  have h1 := hexDigit_value (b.toNat / 16) (by have := b.isLt; omega)
  have h2 := hexDigit_value (b.toNat % 16) (Nat.mod_lt _ (by omega))
  simp only at h1 h2 ⊢
  rw [h1, h2]
  have hx : ¬ ch 'x' = ch '\\' := by decide
  simp only [if_true, hx, if_false]
  rw [Nat.div_add_mod]
  simp

theorem unshow_showByte (b : Spec.Byte) (rest : List Spec.Byte) :
    unshow (showByte b ++ rest) = (b :: ·) <$> unshow rest := by
  unfold showByte
  split
  · rename_i hb
    subst hb
    exact unshow_backslash rest
  · split
    · rename_i hb hp
      exact unshow_plain b rest hb hp
    · exact unshow_hex b rest

theorem showBytes_faithful (s : List Spec.Byte) : unshow (showBytes s) = some s := by
  induction s with
  | nil => simp [showBytes, unshow]
  | cons b rest ih =>
    rw [showBytes, List.flatMap_cons, unshow_showByte]
    rw [← showBytes, ih]
    rfl

end Protocol.Popup
